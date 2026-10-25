//! The real transport: `POST <url>/v1/feedback` with a JSON body. Native only.
//!
//! The client is `ureq`, the same as in `kadakas-update` and `kadakas-gis`. It accepts `https://`
//! addresses only. Tests may allow `http://` for 127.0.0.1 and localhost. It follows no redirects.
//!
//! Answers: 200, 201 and 202 with `{"id": "...", "status": "..."}` mean sent. 408, 425, 429 and
//! 5xx are transient (retry). Any other status is a refusal.

use std::io::Read;
use std::time::Duration;

use crate::send::{Reply, SendError, Sender};

/// The longest answer that we read.
const ANSWER_LIMIT: u64 = 64 << 10;

pub struct HttpSender {
    agent: ureq::Agent,
    allow_loopback_http: bool,
}

impl Default for HttpSender {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpSender {
    pub fn new() -> HttpSender {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(concat!("Kadakas/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        HttpSender {
            agent,
            allow_loopback_http: false,
        }
    }

    /// Also accepts `http://127.0.0.1` and `http://localhost`. For tests and local debugging.
    pub fn allow_loopback_http(mut self) -> HttpSender {
        self.allow_loopback_http = true;
        self
    }

    /// The real sender. Loopback `http://` is allowed only when the variable
    /// `KADAKAS_FEEDBACK_ALLOW_LOOPBACK_HTTP` is `1`, for a local server in tests.
    pub fn from_env() -> HttpSender {
        let sender = HttpSender::new();
        if loopback_from(
            std::env::var("KADAKAS_FEEDBACK_ALLOW_LOOPBACK_HTTP")
                .ok()
                .as_deref(),
        ) {
            sender.allow_loopback_http()
        } else {
            sender
        }
    }
}

/// Whether the value of `KADAKAS_FEEDBACK_ALLOW_LOOPBACK_HTTP` turns loopback `http://` on.
fn loopback_from(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Accepts `https://host/...`, and loopback `http://` if `loopback`.
pub fn check_url(url: &str, loopback: bool) -> Result<(), String> {
    let bad = || format!("address not allowed: {}", crate::scrub::text(url, 100));
    let rest = if let Some(r) = url.strip_prefix("https://") {
        r
    } else if let Some(r) = url.strip_prefix("http://") {
        let host = r.split(['/', ':']).next().unwrap_or("");
        if !loopback || !matches!(host, "127.0.0.1" | "localhost") {
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

impl Sender for HttpSender {
    fn send(&self, url: &str, payload: &str) -> Result<Reply, SendError> {
        check_url(url, self.allow_loopback_http).map_err(SendError::Permanent)?;
        let resp = self
            .agent
            .post(url)
            .content_type("application/json")
            .send(payload)
            .map_err(|e| match e {
                // Offline or a wrong name: not the fault of the report.
                ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
                    SendError::Unreachable(e.to_string())
                }
                ureq::Error::Io(ref io)
                    if matches!(
                        io.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::NotFound
                            | std::io::ErrorKind::AddrNotAvailable
                            | std::io::ErrorKind::NetworkUnreachable
                            | std::io::ErrorKind::HostUnreachable
                    ) =>
                {
                    SendError::Unreachable(e.to_string())
                }
                _ => SendError::Transient(e.to_string()),
            })?;
        let status = resp.status().as_u16();
        match status {
            200..=202 => {
                let mut text = String::new();
                resp.into_body()
                    .into_reader()
                    .take(ANSWER_LIMIT)
                    .read_to_string(&mut text)
                    .map_err(|e| SendError::Transient(e.to_string()))?;
                let v: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|_| SendError::Transient("the answer is not JSON".into()))?;
                let id = v["id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| SendError::Transient("the answer has no id".into()))?;
                Ok(Reply {
                    id: id.to_string(),
                    status: v["status"].as_str().unwrap_or("received").to_string(),
                })
            }
            429 | 503 => {
                // `Retry-After` in seconds. A date is not read: the back-off applies.
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok());
                Err(SendError::Busy {
                    message: format!("server answered {status}"),
                    retry_after,
                })
            }
            408 | 425 | 500..=599 => Err(SendError::Transient(format!("server answered {status}"))),
            _ => Err(SendError::Permanent(format!("server answered {status}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// A one-shot server on the loopback address: reads one request, answers, and returns the
    /// request text.
    fn serve(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            let mut need = None;
            loop {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(p) = text.find("\r\n\r\n") {
                    let len = text[..p]
                        .to_ascii_lowercase()
                        .split("content-length:")
                        .nth(1)
                        .and_then(|r| r.lines().next()?.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    need = Some(p + 4 + len);
                }
                if n == 0 || need.is_some_and(|m| got.len() >= m) {
                    break;
                }
            }
            s.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&got).to_string()
        });
        (url, h)
    }

    fn post(status: &str, body: &str) -> (Result<Reply, SendError>, String) {
        let (url, h) = serve(status, body);
        let r = HttpSender::new()
            .allow_loopback_http()
            .send(&format!("{url}/v1/feedback"), r#"{"protocol":1}"#);
        (r, h.join().unwrap())
    }

    #[test]
    fn a_report_is_posted_as_json() {
        let (r, req) = post("201 Created", r#"{"id":"srv-9","status":"received"}"#);
        assert_eq!(
            r,
            Ok(Reply {
                id: "srv-9".into(),
                status: "received".into()
            })
        );
        assert!(req.starts_with("POST /v1/feedback HTTP/1.1"), "{req}");
        assert!(
            req.to_ascii_lowercase()
                .contains("content-type: application/json")
        );
        assert!(req.ends_with(r#"{"protocol":1}"#));
    }

    #[test]
    fn statuses_map_to_retry_or_refusal() {
        let (r, _) = post("200 OK", r#"{"id":"a"}"#);
        assert_eq!(r.unwrap().status, "received");
        for s in ["500 Internal Server Error", "408 Request Timeout"] {
            assert!(
                matches!(post(s, "{}").0, Err(SendError::Transient(_))),
                "{s}"
            );
        }
        // A busy server may say when to come back. The header is part of the status text here.
        for (s, after) in [
            ("429 Too Many Requests\r\nRetry-After: 90", Some(90)),
            ("503 Service Unavailable", None),
            ("429 Too Many Requests\r\nRetry-After: soon", None),
        ] {
            assert_eq!(
                post(s, "{}").0,
                Err(SendError::Busy {
                    message: format!("server answered {}", &s[..3]),
                    retry_after: after
                }),
                "{s}"
            );
        }
        for s in ["400 Bad Request", "404 Not Found", "302 Found"] {
            assert!(
                matches!(post(s, "{}").0, Err(SendError::Permanent(_))),
                "{s}"
            );
        }
        assert!(matches!(
            post("200 OK", "not json").0,
            Err(SendError::Transient(_))
        ));
        assert!(matches!(
            post("200 OK", r#"{"status":"x"}"#).0,
            Err(SendError::Transient(_))
        ));
    }

    #[test]
    fn an_unreachable_server_is_not_counted() {
        // Port 1 has no listener on a test machine. A port that this test frees first can be
        // taken by another test or program before the connect, and Windows may then answer with
        // a reset (10054) instead of a refusal (10061).
        let r = HttpSender::default()
            .allow_loopback_http()
            .send("http://127.0.0.1:1/v1/feedback", "{}");
        assert!(matches!(r, Err(SendError::Unreachable(_))), "{r:?}");
    }

    #[test]
    fn loopback_http_needs_the_variable_set_to_one() {
        assert!(loopback_from(Some("1")));
        assert!(!loopback_from(None));
        assert!(!loopback_from(Some("true")));
        assert!(!loopback_from(Some("")));
    }

    #[test]
    fn only_safe_addresses_are_used() {
        assert!(check_url("https://feedback.example/v1/feedback", false).is_ok());
        for bad in [
            "http://feedback.example/x",
            "http://127.0.0.1/x",
            "ftp://x",
            "https://",
            "https://user@host/x",
            "https://a b/x",
            "feedback.example",
        ] {
            assert!(check_url(bad, false).is_err(), "{bad}");
        }
        assert!(check_url("http://localhost:8080/x", true).is_ok());
        assert!(check_url("http://evil.example/x", true).is_err());
        let r = HttpSender::new().send("http://127.0.0.1:1/x", "{}");
        assert!(matches!(r, Err(SendError::Permanent(_))));
    }
}
