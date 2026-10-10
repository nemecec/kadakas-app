//! The HTTP abstraction. The update code asks a [`Http`] for bytes and never talks to the network
//! itself. [`UreqHttp`] is the real client. [`MockHttp`] is the test implementation.

use crate::error::{Result, UpdateError};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::Duration;

/// A client that fetches one address.
pub trait Http: Send + Sync {
    /// Writes the body of `url` to `out` and returns the number of bytes written.
    /// It fails when the body is longer than `limit` bytes.
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<u64>;
}

/// Accepts `https://` addresses, and `http://` only for the own machine (for tests).
pub fn check_url(url: &str) -> Result<()> {
    let bad = || UpdateError::Http(format!("address not allowed: {url}"));
    let rest = if let Some(r) = url.strip_prefix("https://") {
        r
    } else if let Some(r) = url.strip_prefix("http://") {
        let host_end = r.find([':', '/']).unwrap_or(r.len());
        if !matches!(&r[..host_end], "127.0.0.1" | "localhost") {
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

const MAX_REDIRECTS: usize = 5;

/// The real client, on `ureq`. It follows up to five redirects itself and checks each address.
pub struct UreqHttp {
    agent: ureq::Agent,
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
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent("Kadakas-launcher/0.1")
            .build()
            .into();
        UreqHttp { agent }
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
        for _ in 0..=MAX_REDIRECTS {
            check_url(&url)?;
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
                url = resolve(&url, loc);
                continue;
            }
            if status != 200 {
                return Err(UpdateError::Http(format!("server answered {status}")));
            }
            let mut body = resp.into_body();
            let mut reader = body.as_reader().take(limit + 1);
            let n = std::io::copy(&mut reader, out)
                .map_err(|e| UpdateError::Http(format!("read failed: {e}")))?;
            if n > limit {
                return Err(UpdateError::Http(format!("more than {limit} bytes")));
            }
            return Ok(n);
        }
        Err(UpdateError::Http("too many redirects".into()))
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
        for ok in [
            "https://example.org/a.tar.gz",
            "https://example.org:8443/a",
            "http://127.0.0.1:8080/a",
            "http://localhost/a",
            "http://localhost",
        ] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
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
        let c = UreqHttp::default();
        let mut out = Vec::new();
        let n = c
            .get(&format!("http://127.0.0.1:{port}/a"), 100, &mut out)
            .unwrap();
        assert_eq!(n, 8);
        assert_eq!(out, b"the body");
    }

    #[test]
    fn real_client_errors() {
        let c = UreqHttp::new();
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
}
