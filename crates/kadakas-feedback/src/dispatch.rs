//! Sends in a helper thread, so a program with a window or a request loop never waits for the
//! network. Native only.
//!
//! The host calls [`Dispatcher::pump`] now and then (after each request, once per frame). It
//! records what the helper threads finished and starts the next sends. The sender and the
//! sleeper are traits, so tests run the whole path with fakes.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender as Tx, channel};
use std::time::{Duration, Instant};

use crate::hub::{Hub, Outgoing};
use crate::send::{Delivery, RetryPolicy, Sender, Sleeper, deliver};

/// Reports in one start of sending.
const BATCH: usize = 10;

/// How long the end of a program waits for sends that run. A report that is not done stays in
/// the outbox (its claim ends after a while) and goes out at the next start.
pub const FINISH_WAIT: Duration = Duration::from_secs(2);

/// Seconds since 1970.
fn system_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub struct Dispatcher {
    sender: Arc<dyn Sender>,
    sleeper: Arc<dyn Sleeper>,
    policy: RetryPolicy,
    tx: Tx<(String, Delivery)>,
    rx: Receiver<(String, Delivery)>,
    in_flight: HashSet<String>,
    /// The clock of the outbox (seconds since 1970). Tests give a fixed one.
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl Dispatcher {
    pub fn new(sender: Arc<dyn Sender>, sleeper: Arc<dyn Sleeper>, policy: RetryPolicy) -> Self {
        let (tx, rx) = channel();
        Dispatcher {
            sender,
            sleeper,
            policy,
            tx,
            rx,
            in_flight: HashSet::new(),
            now: Arc::new(system_now),
        }
    }

    /// Another clock, for tests.
    pub fn with_clock(mut self, now: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now = now;
        self
    }

    /// The real transport with the default retry policy.
    pub fn http() -> Self {
        Dispatcher::new(
            Arc::new(crate::http::HttpSender::new()),
            Arc::new(crate::send::ThreadSleeper),
            RetryPolicy::default(),
        )
    }

    /// Reports that a thread is sending.
    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    fn start(&mut self, items: Vec<Outgoing>, url: &str) {
        for item in items {
            if !self.in_flight.insert(item.id.clone()) {
                continue;
            }
            let (sender, sleeper, tx) =
                (self.sender.clone(), self.sleeper.clone(), self.tx.clone());
            let (url, policy) = (url.to_string(), self.policy);
            std::thread::spawn(move || {
                let d = deliver(&*sender, &*sleeper, &url, &item.payload, policy);
                let _ = tx.send((item.id, d));
            });
        }
    }

    fn record(&mut self, hub: &mut Hub, done: Vec<(String, Delivery)>) -> usize {
        let n = done.len();
        for (id, d) in done {
            self.in_flight.remove(&id);
            let _ = hub.record(&id, &d, (self.now)());
        }
        n
    }

    /// Records finished sends, then starts the sends that are ready. Approved reports always go;
    /// text-only reports go if `include_text` and the person agreed. Returns how many sends
    /// finished.
    pub fn pump(&mut self, hub: &mut Hub, include_text: bool) -> usize {
        let done: Vec<_> = self.rx.try_iter().collect();
        let n = self.record(hub, done);
        let items: Vec<Outgoing> = hub
            .outgoing(include_text, BATCH, (self.now)())
            .into_iter()
            .filter(|o| !self.in_flight.contains(&o.id))
            .collect();
        if !items.is_empty() {
            let url = hub.url();
            self.start(items, &url);
        }
        n
    }

    /// Waits for the started sends, at most [`FINISH_WAIT`], and records those that finish. For
    /// the end of a program.
    pub fn finish(&mut self, hub: &mut Hub) {
        self.finish_within(hub, FINISH_WAIT);
    }

    /// Like [`Dispatcher::finish`], with another limit.
    pub fn finish_within(&mut self, hub: &mut Hub, limit: Duration) {
        let end = Instant::now() + limit;
        while !self.in_flight.is_empty() {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(d) => {
                    self.record(hub, vec![d]);
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::send::{FakeSender, NoSleep, Reply, SendError};

    fn hub_with(n: usize) -> Hub {
        let mut h = Hub::in_memory();
        for i in 0..n {
            h.submit(
                Draft::new(Kind::Bug, &format!("r{i}"), "b", Origin::User),
                vec![],
                0,
            )
            .unwrap();
        }
        h
    }

    fn dispatcher(script: Vec<Result<Reply, SendError>>) -> (Dispatcher, Arc<FakeSender>) {
        let fake = Arc::new(FakeSender::new(script));
        let policy = RetryPolicy {
            attempts: 2,
            base_ms: 1,
            max_ms: 1,
        };
        (
            Dispatcher::new(fake.clone(), Arc::new(NoSleep::default()), policy),
            fake,
        )
    }

    #[test]
    fn nothing_goes_out_without_consent() {
        let mut h = hub_with(2);
        let (mut d, fake) = dispatcher(vec![]);
        d.pump(&mut h, true);
        d.finish(&mut h);
        assert!(fake.requests().is_empty());
        assert_eq!(d.in_flight(), 0);
    }

    #[test]
    fn text_reports_go_with_consent_and_are_recorded() {
        let mut h = hub_with(3);
        h.set_send_override(Some(true));
        h.set_url_override(Some("https://fb.example".into()));
        let (mut d, fake) = dispatcher(vec![Err(SendError::Permanent("no".into()))]);
        d.pump(&mut h, true);
        d.finish(&mut h);
        // The first answer is a refusal, the others are sent.
        let reqs = fake.requests();
        assert_eq!(reqs.len(), 3);
        assert!(reqs.iter().all(|r| r.0 == "https://fb.example/v1/feedback"));
        let sent = h.list().iter().filter(|r| r.status == Status::Sent).count();
        assert_eq!(sent, 2);
        // The refused one is not tried again.
        d.pump(&mut h, true);
        d.finish(&mut h);
        assert_eq!(fake.requests().len(), 3);
    }

    #[test]
    fn a_report_in_flight_is_not_started_twice() {
        let mut h = hub_with(1);
        h.set_send_override(Some(true));
        let (mut d, fake) = dispatcher(vec![]);
        d.pump(&mut h, true);
        d.pump(&mut h, true);
        d.finish(&mut h);
        d.pump(&mut h, true);
        assert_eq!(fake.requests().len(), 1);
        assert_eq!(h.get("fb-1").unwrap().status, Status::Sent);
    }

    #[test]
    fn approved_reports_go_without_the_preference() {
        let mut h = Hub::in_memory();
        let log = AttachmentData {
            kind: AttachmentKind::Log,
            bytes: b"x".to_vec(),
            objects: None,
        };
        h.submit(Draft::new(Kind::Bug, "a", "b", Origin::Agent), vec![log], 0)
            .unwrap();
        let (mut d, fake) = dispatcher(vec![]);
        d.pump(&mut h, true);
        d.finish(&mut h);
        assert!(fake.requests().is_empty());
        let r = h.get("fb-1").unwrap();
        let p = crate::payload::build(
            &r,
            &crate::payload::PayloadParts {
                log: Some("x"),
                ..Default::default()
            },
        )
        .unwrap();
        h.approve("fb-1", &p, Privacy::default()).unwrap();
        d.pump(&mut h, false);
        d.finish(&mut h);
        assert_eq!(fake.requests()[0].1, p);
        assert_eq!(h.get("fb-1").unwrap().status, Status::Sent);
    }

    /// A sender that waits for a signal, like a slow server.
    struct Gate(std::sync::Mutex<std::sync::mpsc::Receiver<()>>);

    impl Sender for Gate {
        fn send(&self, _url: &str, _payload: &str) -> Result<Reply, SendError> {
            let _ = self.0.lock().unwrap().recv();
            Ok(Reply {
                id: "late".into(),
                status: "received".into(),
            })
        }
    }

    #[test]
    fn the_end_of_a_program_waits_a_short_time_only() {
        let (open, gate) = std::sync::mpsc::channel();
        let mut h = hub_with(1);
        h.set_send_override(Some(true));
        let mut d = Dispatcher::new(
            Arc::new(Gate(std::sync::Mutex::new(gate))),
            Arc::new(NoSleep::default()),
            RetryPolicy::default(),
        );
        d.pump(&mut h, true);
        assert_eq!(d.in_flight(), 1);
        // The server is slow: the wait ends, the report stays in the outbox.
        d.finish_within(&mut h, Duration::from_millis(30));
        assert_eq!(d.in_flight(), 1);
        assert_eq!(h.get("fb-1").unwrap().status, Status::Pending);
        // It answers later, and the next wait records it.
        open.send(()).unwrap();
        d.finish_within(&mut h, Duration::from_secs(10));
        assert_eq!(d.in_flight(), 0);
        assert_eq!(h.get("fb-1").unwrap().status, Status::Sent);
        assert_eq!(FINISH_WAIT, Duration::from_secs(2));
    }

    #[test]
    fn the_real_constructor_builds() {
        let d = Dispatcher::http();
        assert_eq!(d.in_flight(), 0);
    }
}
