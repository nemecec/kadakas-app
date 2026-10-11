//! The HTTP abstraction. The update code asks a [`Http`] for bytes and never talks to the network
//! itself. [`UreqHttp`] is the real client. [`MockHttp`] is the test implementation.

use crate::error::{Result, UpdateError};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant};

/// A client that fetches one address.
pub trait Http: Send + Sync {
    /// Writes the body of `url` to `out` and returns the number of bytes written.
    /// It fails when the body is longer than `limit` bytes.
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<u64>;
}

/// Accepts `https://` addresses only.
pub fn check_url(url: &str) -> Result<()> {
    check_url_with(url, false)
}

/// Like [`check_url`]. With `allow_loopback_http`, `http://127.0.0.1` and `http://localhost`
/// are accepted too. That is for tests and for local debugging, never for a release.
pub fn check_url_with(url: &str, allow_loopback_http: bool) -> Result<()> {
    let bad = || UpdateError::Http(format!("address not allowed: {url}"));
    let rest = if let Some(r) = url.strip_prefix("https://") {
        r
    } else if let Some(r) = url.strip_prefix("http://") {
        let host_end = r.find([':', '/']).unwrap_or(r.len());
        if !allow_loopback_http || !matches!(&r[..host_end], "127.0.0.1" | "localhost") {
            return Err(bad());
        }
        r
    } else {
        return Err(bad());
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || url.contains(char::is_whitespace) {
        return Err(bad());
    }
    Ok(())
}

/// A redirect may keep the scheme or go from http to https. It may never leave https.
fn hop_allowed(from: &str, to: &str, allow_loopback_http: bool) -> Result<()> {
    check_url_with(to, allow_loopback_http)?;
    if from.starts_with("https://") && !to.starts_with("https://") {
        return Err(UpdateError::Http(
            "a redirect from https to http is refused".into(),
        ));
    }
    Ok(())
}

const MAX_REDIRECTS: usize = 5;

/// The slowest transfer that we accept for the whole body, in bytes per second. It sets the time
/// budget: `total` plus `limit / MIN_RATE`.
const MIN_RATE: u64 = 100_000;

/// The real client, on `ureq`.
///
/// - It follows up to five redirects itself and checks each address.
/// - Connect and response headers have their own timeouts.
/// - The body is read in a helper thread. The caller waits for each chunk at most `stall`, and
///   for the whole body at most `total + limit / 100 kB/s`. A stalled or very slow server ends
///   as an error, so the launcher window never waits for ever. (`ureq` has no per-read timeout and
///   its body timeout is a total, so a large download would fail with it.)
pub struct UreqHttp {
    agent: ureq::Agent,
    allow_loopback_http: bool,
    stall: Duration,
    total: Duration,
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqHttp {
    pub fn new() -> UreqHttp {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_send_request(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            // Only a ceiling, so that a helper thread of a dead connection ends one day.
            .timeout_global(Some(Duration::from_secs(6 * 3600)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent("Kadakas-launcher/0.1")
            .build()
            .into();
        UreqHttp {
            agent,
            allow_loopback_http: false,
            stall: Duration::from_secs(30),
            total: Duration::from_secs(60),
        }
    }

    /// Also accepts `http://127.0.0.1` and `http://localhost`. For tests and local debugging.
    pub fn allow_loopback_http(mut self) -> UreqHttp {
        self.allow_loopback_http = true;
        self
    }

    /// Sets the longest wait for one chunk, and the fixed part of the time budget of a body.
    pub fn with_timeouts(mut self, stall: Duration, total: Duration) -> UreqHttp {
        self.stall = stall;
        self.total = total;
        self
    }
}

/// The address a `Location` header points to.
fn resolve(base: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_string();
    }
    let scheme_end = base.find("://").map_or(0, |i| i + 3);
    let origin_end = base[scheme_end..]
        .find('/')
        .map_or(base.len(), |i| scheme_end + i);
    format!("{}{}", &base[..origin_end], location)
}

impl Http for UreqHttp {
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<u64> {
        let mut url = url.to_string();
        check_url_with(&url, self.allow_loopback_http)?;
        for _ in 0..=MAX_REDIRECTS {
            let resp = self
                .agent
                .get(&url)
                .call()
                .map_err(|e| UpdateError::Http(e.to_string()))?;
            let status = resp.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let loc = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .filter(|l| l.starts_with('/') || l.contains("://"))
                    .ok_or_else(|| UpdateError::Http("redirect without a usable address".into()))?;
                let next = resolve(&url, loc);
                hop_allowed(&url, &next, self.allow_loopback_http)?;
                url = next;
                continue;
            }
            if status != 200 {
                return Err(UpdateError::Http(format!("server answered {status}")));
            }
            return self.read_body(resp.into_body(), limit, out);
        }
        Err(UpdateError::Http("too many redirects".into()))
    }
}

impl UreqHttp {
    fn read_body(&self, body: ureq::Body, limit: u64, out: &mut dyn Write) -> Result<u64> {
        let deadline = Instant::now() + self.total + Duration::from_secs(limit / MIN_RATE);
        let (tx, rx) = sync_channel::<std::io::Result<Vec<u8>>>(4);
        std::thread::spawn(move || {
            let mut body = body;
            let mut reader = body.as_reader().take(limit + 1);
            loop {
                let mut buf = vec![0u8; 64 * 1024];
                match reader.read(&mut buf) {
                    // Dropping the sender is the end of the body.
                    Ok(0) => return,
                    Ok(n) => {
                        buf.truncate(n);
                        if tx.send(Ok(buf)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            }
        });
        let mut written = 0u64;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(UpdateError::Http("the download is too slow".into()));
            }
            match rx.recv_timeout(self.stall.min(left)) {
                Ok(Ok(chunk)) => {
                    written += chunk.len() as u64;
                    if written > limit {
                        return Err(UpdateError::Http(format!("more than {limit} bytes")));
                    }
                    out.write_all(&chunk)?;
                }
                Ok(Err(e)) => return Err(UpdateError::Http(format!("read failed: {e}"))),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(UpdateError::Http("the download stalled".into()));
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(written),
            }
        }
    }
}

/// The test implementation: answers from a table, and can cut a body short.
#[derive(Default)]
pub struct MockHttp {
    files: Mutex<HashMap<String, Vec<u8>>>,
    cut: Mutex<HashMap<String, usize>>,
    log: Mutex<Vec<String>>,
}

impl MockHttp {
    pub fn new() -> MockHttp {
        MockHttp::default()
    }

    /// Serves `body` at `url`.
    pub fn put(&self, url: &str, body: Vec<u8>) {
        self.files.lock().unwrap().insert(url.to_string(), body);
    }

    /// Removes the file, so the address answers "not found".
    pub fn remove(&self, url: &str) {
        self.files.lock().unwrap().remove(url);
    }

    /// Sends only the first `bytes` bytes of the body, and reports success (a cut connection).
    pub fn cut_after(&self, url: &str, bytes: usize) {
        self.cut.lock().unwrap().insert(url.to_string(), bytes);
    }

    /// The addresses that were asked for, in order.
    pub fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Http for MockHttp {
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<u64> {
        self.log.lock().unwrap().push(url.to_string());
        let files = self.files.lock().unwrap();
        let body = files
            .get(url)
            .ok_or_else(|| UpdateError::Http("server answered 404".into()))?;
        if body.len() as u64 > limit {
            return Err(UpdateError::Http(format!("more than {limit} bytes")));
        }
        let cut = self.cut.lock().unwrap().get(url).copied();
        let part = &body[..cut.unwrap_or(body.len()).min(body.len())];
        out.write_all(part)?;
        Ok(part.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn address_rules() {
        for ok in ["https://example.org/a.tar.gz", "https://example.org:8443/a"] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        // Plain http is for the own machine, and only when asked for.
        for local in [
            "http://127.0.0.1:8080/a",
            "http://localhost/a",
            "http://localhost",
        ] {
            assert!(check_url(local).is_err(), "{local}");
            assert!(check_url_with(local, true).is_ok(), "{local}");
        }
        assert!(check_url_with("http://example.org/a", true).is_err());
        for bad in [
            "http://example.org/a",
            "http://127.0.0.1.evil.example/a",
            "ftp://example.org/a",
            "file:///etc/passwd",
            "https://",
            "https:///a",
            "https://user@example.org/a",
            "https://example.org/a b",
            "",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_redirect_never_leaves_https() {
        assert!(hop_allowed("https://a.org/x", "https://b.org/y", false).is_ok());
        assert!(hop_allowed("http://127.0.0.1/x", "https://b.org/y", true).is_ok());
        assert!(hop_allowed("http://127.0.0.1/x", "http://127.0.0.1/y", true).is_ok());
        assert!(hop_allowed("https://a.org/x", "http://a.org/y", false).is_err());
        assert!(hop_allowed("https://a.org/x", "http://127.0.0.1/y", true).is_err());
        assert!(hop_allowed("https://a.org/x", "ftp://a.org/y", true).is_err());
        assert!(hop_allowed("https://a.org/x", "file:///etc/passwd", true).is_err());
    }

    #[test]
    fn resolve_redirects() {
        assert_eq!(resolve("https://a.org/x/y", "/z"), "https://a.org/z");
        assert_eq!(resolve("https://a.org", "/z"), "https://a.org/z");
        assert_eq!(
            resolve("https://a.org/x", "https://b.org/q"),
            "https://b.org/q"
        );
    }

    #[test]
    fn mock_serves_and_cuts() {
        let m = MockHttp::new();
        m.put("https://x/a", b"hello".to_vec());
        let mut out = Vec::new();
        assert_eq!(m.get("https://x/a", 10, &mut out).unwrap(), 5);
        assert_eq!(out, b"hello");
        assert!(m.get("https://x/a", 4, &mut Vec::new()).is_err());
        assert!(m.get("https://x/none", 4, &mut Vec::new()).is_err());
        m.cut_after("https://x/a", 2);
        let mut out = Vec::new();
        assert_eq!(m.get("https://x/a", 10, &mut out).unwrap(), 2);
        assert_eq!(out, b"he");
        m.cut_after("https://x/a", 99);
        let mut out = Vec::new();
        assert_eq!(m.get("https://x/a", 10, &mut out).unwrap(), 5);
        m.remove("https://x/a");
        assert!(m.get("https://x/a", 10, &mut Vec::new()).is_err());
        assert_eq!(m.requests().len(), 6);
    }

    /// Answers each connection with the next canned reply. Returns the port.
    fn serve(replies: Vec<Vec<u8>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut s, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ = s.write_all(&reply);
                let _ = s.flush();
            }
        });
        port
    }

    fn ok(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn real_client_reads_a_body_and_follows_redirects() {
        let port = serve(vec![
            b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            ok("the body"),
        ]);
        let c = UreqHttp::default().allow_loopback_http();
        let mut out = Vec::new();
        let n = c
            .get(&format!("http://127.0.0.1:{port}/a"), 100, &mut out)
            .unwrap();
        assert_eq!(n, 8);
        assert_eq!(out, b"the body");
    }

    #[test]
    fn real_client_errors() {
        let c = UreqHttp::new().allow_loopback_http();
        // Wrong scheme: refused before any connection.
        assert!(c.get("http://example.org/a", 10, &mut Vec::new()).is_err());
        // Status 404.
        let port = serve(vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ]);
        let e = c
            .get(&format!("http://127.0.0.1:{port}/a"), 10, &mut Vec::new())
            .unwrap_err();
        assert!(e.to_string().contains("404"), "{e}");
        // Too long.
        let port = serve(vec![ok("0123456789")]);
        let e = c
            .get(&format!("http://127.0.0.1:{port}/a"), 5, &mut Vec::new())
            .unwrap_err();
        assert!(e.to_string().contains("more than"), "{e}");
        // A redirect to a forbidden address.
        let port = serve(vec![
            b"HTTP/1.1 301 Moved\r\nLocation: http://evil.example/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ]);
        assert!(
            c.get(&format!("http://127.0.0.1:{port}/a"), 10, &mut Vec::new())
                .is_err()
        );
        // A redirect without a usable address.
        let port = serve(vec![
            b"HTTP/1.1 301 Moved\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ]);
        assert!(
            c.get(&format!("http://127.0.0.1:{port}/a"), 10, &mut Vec::new())
                .is_err()
        );
        // An endless redirect loop.
        let loopy =
            b"HTTP/1.1 302 Found\r\nLocation: /a\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec();
        let port = serve(vec![loopy; MAX_REDIRECTS + 1]);
        let e = c
            .get(&format!("http://127.0.0.1:{port}/a"), 10, &mut Vec::new())
            .unwrap_err();
        assert!(e.to_string().contains("redirects"), "{e}");
        // A redirect to a foreign scheme.
        let port = serve(vec![
            b"HTTP/1.1 302 Found\r\nLocation: ftp://example.org/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ]);
        assert!(
            c.get(&format!("http://127.0.0.1:{port}/a"), 10, &mut Vec::new())
                .is_err()
        );
        // The strict client does not talk to http at all, not even to the own machine.
        let strict = UreqHttp::new();
        assert!(
            strict
                .get("http://127.0.0.1:1/a", 10, &mut Vec::new())
                .is_err()
        );
        // The connection ends before the announced length.
        let port = serve(vec![
            b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\nConnection: close\r\n\r\nshort".to_vec(),
        ]);
        assert!(
            c.get(&format!("http://127.0.0.1:{port}/a"), 100, &mut Vec::new())
                .is_err()
        );
        // Nobody listens.
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(
            c.get(&format!("http://127.0.0.1:{port}/a"), 10, &mut Vec::new())
                .is_err()
        );
    }

    #[test]
    fn a_stalled_server_ends_as_an_error() {
        // The server sends 3 of 100 bytes and then keeps the connection open.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc");
                let _ = s.flush();
                std::thread::sleep(Duration::from_secs(3));
            }
        });
        let c = UreqHttp::new()
            .allow_loopback_http()
            .with_timeouts(Duration::from_millis(300), Duration::from_secs(30));
        let e = c
            .get(&format!("http://127.0.0.1:{port}/a"), 100, &mut Vec::new())
            .unwrap_err();
        assert!(e.to_string().contains("stalled"), "{e}");
    }

    #[test]
    fn a_body_over_the_time_budget_is_refused() {
        let port = serve(vec![ok("0123456789")]);
        let c = UreqHttp::new()
            .allow_loopback_http()
            .with_timeouts(Duration::from_secs(5), Duration::ZERO);
        let e = c
            .get(&format!("http://127.0.0.1:{port}/a"), 100, &mut Vec::new())
            .unwrap_err();
        assert!(e.to_string().contains("too slow"), "{e}");
    }

    #[test]
    fn a_large_body_is_not_cut_by_a_fixed_total_time() {
        // 3 MB over loopback: fine. (The old total timeout of the library applied to the
        // whole body.)
        let body = "x".repeat(3_000_000);
        let port = serve(vec![ok(&body)]);
        let c = UreqHttp::new().allow_loopback_http();
        let mut out = Vec::new();
        assert_eq!(
            c.get(&format!("http://127.0.0.1:{port}/a"), 4_000_000, &mut out)
                .unwrap(),
            3_000_000
        );
    }
}
