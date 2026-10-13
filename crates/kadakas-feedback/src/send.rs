//! Sending: a trait for the transport, a retry loop with a back-off, and fakes for tests.
//!
//! Nothing here touches the network. The real transport is `HttpSender` (native only).

use std::collections::VecDeque;
use std::sync::Mutex;

/// The address of the feedback server until it exists. `.invalid` never resolves, so nothing
/// leaves the computer by accident. A user or a headless run sets another address.
pub const DEFAULT_URL: &str = "https://feedback.kadakas.invalid";

/// What the server answers to a report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The id at the server.
    pub id: String,
    /// `received`, `duplicate`, or another word of the server.
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// Worth a retry now: a timeout, a dropped connection, a server error.
    Transient(String),
    /// The server cannot be reached (DNS or connect failed). The computer may be offline, so this
    /// does not count toward the lifetime limit of tries.
    Unreachable(String),
    /// The server is busy and may say when to come back (`Retry-After`, in seconds).
    Busy {
        message: String,
        retry_after: Option<u64>,
    },
    /// The server refused the report. A retry gives the same answer.
    Permanent(String),
}

/// One request to the server.
pub trait Sender: Send + Sync {
    fn send(&self, url: &str, payload: &str) -> Result<Reply, SendError>;
}

/// The wait between two tries.
pub trait Sleeper: Send + Sync {
    fn sleep_ms(&self, ms: u64);
}

/// Sleeps for real.
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// Retry with a back-off: `base_ms`, then double, never more than `max_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub base_ms: u64,
    pub max_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            attempts: 4,
            base_ms: 500,
            max_ms: 8000,
        }
    }
}

impl RetryPolicy {
    /// The wait after try number `n` (1 is the first try).
    pub fn wait_ms(&self, n: u32) -> u64 {
        self.base_ms
            .saturating_mul(1u64 << (n - 1).min(20))
            .min(self.max_ms)
    }
}

/// The result of all tries for one report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    Sent(Reply),
    Failed {
        message: String,
        /// True if the server refused the report: do not retry in later runs.
        permanent: bool,
        /// True if the server could not be reached. Such a failure is not counted.
        unreachable: bool,
        /// Seconds that the server asked to wait.
        retry_after: Option<u64>,
        /// Tries made in this start.
        attempts: u32,
    },
}

/// `<url>/v1/feedback`.
pub fn endpoint(url: &str) -> String {
    let u = url.trim();
    // A query or a fragment stays at the end.
    let (base, rest) = match u.find(['?', '#']) {
        Some(i) => (&u[..i], &u[i..]),
        None => (u, ""),
    };
    format!("{}/v1/feedback{rest}", base.trim_end_matches('/'))
}

/// Sends one payload, with retries on transient errors.
pub fn deliver(
    sender: &dyn Sender,
    sleeper: &dyn Sleeper,
    url: &str,
    payload: &str,
    policy: RetryPolicy,
) -> Delivery {
    let target = endpoint(url);
    let mut last = String::new();
    let tries = policy.attempts.max(1);
    for n in 1..=tries {
        match sender.send(&target, payload) {
            Ok(r) => return Delivery::Sent(r),
            Err(SendError::Permanent(m)) => {
                return Delivery::Failed {
                    message: m,
                    permanent: true,
                    unreachable: false,
                    retry_after: None,
                    attempts: n,
                };
            }
            // No point in more tries now: the next start comes after the back-off of the outbox.
            Err(SendError::Unreachable(m)) => {
                return Delivery::Failed {
                    message: m,
                    permanent: false,
                    unreachable: true,
                    retry_after: None,
                    attempts: n,
                };
            }
            Err(SendError::Busy {
                message,
                retry_after,
            }) => {
                return Delivery::Failed {
                    message,
                    permanent: false,
                    unreachable: false,
                    retry_after,
                    attempts: n,
                };
            }
            Err(SendError::Transient(m)) => {
                last = m;
                if n < tries {
                    sleeper.sleep_ms(policy.wait_ms(n));
                }
            }
        }
    }
    Delivery::Failed {
        message: last,
        permanent: false,
        unreachable: false,
        retry_after: None,
        attempts: tries,
    }
}

/// A sender with a script of answers, for tests. It records the requests.
#[derive(Default)]
pub struct FakeSender {
    script: Mutex<VecDeque<Result<Reply, SendError>>>,
    requests: Mutex<Vec<(String, String)>>,
}

impl FakeSender {
    /// Answers in this order. After the script, every request gets `received`.
    pub fn new(script: Vec<Result<Reply, SendError>>) -> FakeSender {
        FakeSender {
            script: Mutex::new(script.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// The (address, payload) of every request so far.
    pub fn requests(&self) -> Vec<(String, String)> {
        self.requests.lock().unwrap().clone()
    }
}

impl Sender for FakeSender {
    fn send(&self, url: &str, payload: &str) -> Result<Reply, SendError> {
        let mut r = self.requests.lock().unwrap();
        r.push((url.to_string(), payload.to_string()));
        let n = r.len();
        self.script.lock().unwrap().pop_front().unwrap_or_else(|| {
            Ok(Reply {
                id: format!("srv-{n}"),
                status: "received".into(),
            })
        })
    }
}

/// A sleeper that records the waits and does not wait, for tests.
#[derive(Default)]
pub struct NoSleep(pub Mutex<Vec<u64>>);

impl Sleeper for NoSleep {
    fn sleep_ms(&self, ms: u64) {
        self.0.lock().unwrap().push(ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(id: &str) -> Result<Reply, SendError> {
        Ok(Reply {
            id: id.into(),
            status: "received".into(),
        })
    }

    #[test]
    fn backs_off_and_succeeds() {
        let s = FakeSender::new(vec![
            Err(SendError::Transient("down".into())),
            Err(SendError::Transient("busy".into())),
            ok("a"),
        ]);
        let z = NoSleep::default();
        let d = deliver(&s, &z, "https://x.example/", "{}", RetryPolicy::default());
        assert_eq!(
            d,
            Delivery::Sent(Reply {
                id: "a".into(),
                status: "received".into()
            })
        );
        assert_eq!(*z.0.lock().unwrap(), vec![500, 1000]);
        let reqs = s.requests();
        assert_eq!(reqs.len(), 3);
        assert_eq!(reqs[0].0, "https://x.example/v1/feedback");
    }

    #[test]
    fn gives_up_after_the_tries() {
        let s = FakeSender::new(vec![Err(SendError::Transient("down".into())); 10]);
        let z = NoSleep::default();
        let p = RetryPolicy {
            attempts: 3,
            base_ms: 10,
            max_ms: 15,
        };
        let d = deliver(&s, &z, "u", "{}", p);
        assert_eq!(
            d,
            Delivery::Failed {
                message: "down".into(),
                permanent: false,
                unreachable: false,
                retry_after: None,
                attempts: 3
            }
        );
        // No wait after the last try, and the wait is capped.
        assert_eq!(*z.0.lock().unwrap(), vec![10, 15]);
    }

    #[test]
    fn a_refusal_is_not_retried() {
        let s = FakeSender::new(vec![Err(SendError::Permanent("422".into()))]);
        let z = NoSleep::default();
        let d = deliver(&s, &z, "u", "{}", RetryPolicy::default());
        assert_eq!(
            d,
            Delivery::Failed {
                message: "422".into(),
                permanent: true,
                unreachable: false,
                retry_after: None,
                attempts: 1
            }
        );
        assert_eq!(s.requests().len(), 1);
        assert!(z.0.lock().unwrap().is_empty());
    }

    #[test]
    fn offline_and_busy_stop_at_once() {
        let s = FakeSender::new(vec![Err(SendError::Unreachable("dns".into()))]);
        let z = NoSleep::default();
        let d = deliver(&s, &z, "u", "{}", RetryPolicy::default());
        assert_eq!(
            d,
            Delivery::Failed {
                message: "dns".into(),
                permanent: false,
                unreachable: true,
                retry_after: None,
                attempts: 1
            }
        );
        let s = FakeSender::new(vec![Err(SendError::Busy {
            message: "429".into(),
            retry_after: Some(90),
        })]);
        let d = deliver(&s, &z, "u", "{}", RetryPolicy::default());
        assert_eq!(
            d,
            Delivery::Failed {
                message: "429".into(),
                permanent: false,
                unreachable: false,
                retry_after: Some(90),
                attempts: 1
            }
        );
        assert!(z.0.lock().unwrap().is_empty());
    }

    #[test]
    fn the_address_keeps_a_query() {
        assert_eq!(
            endpoint("https://a.b/x/?k=v"),
            "https://a.b/x/v1/feedback?k=v"
        );
        assert_eq!(endpoint("https://a.b#f"), "https://a.b/v1/feedback#f");
    }

    #[test]
    fn wait_grows_without_overflow() {
        let p = RetryPolicy::default();
        assert_eq!(p.wait_ms(1), 500);
        assert_eq!(p.wait_ms(30), 8000);
        assert_eq!(endpoint(" https://a.b// "), "https://a.b/v1/feedback");
        let zero = RetryPolicy { attempts: 0, ..p };
        let s = FakeSender::default();
        assert!(matches!(
            deliver(&s, &NoSleep::default(), "u", "{}", zero),
            Delivery::Sent(_)
        ));
    }

    #[test]
    fn real_sleeper_sleeps_a_little() {
        ThreadSleeper.sleep_ms(1);
    }
}
