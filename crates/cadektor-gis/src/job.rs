//! Work on a worker thread, so the UI never waits for a server.
//!
//! A [`Job`] runs a closure on its own thread. The caller asks `poll` once per frame (it never blocks),
//! or `wait` in a script. A job that is cancelled gives an error at once, and its late result is dropped.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use crate::{GisError, Result};

pub struct Job<T> {
    /// What the job does, for lists in the UI.
    pub label: String,
    rx: Receiver<Result<T>>,
    result: Option<Result<T>>,
    cancelled: bool,
    received: bool,
    started: Instant,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn<F>(label: &str, f: F) -> Job<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            // A panic ends the thread without a result: the receiver sees a closed channel.
            let _ = tx.send(f());
        });
        Job {
            label: label.to_string(),
            rx,
            result: None,
            cancelled: false,
            received: false,
            started: Instant::now(),
        }
    }

    fn pump(&mut self) {
        if self.result.is_some() || self.received {
            return;
        }
        match self.rx.try_recv() {
            Ok(r) => {
                self.result = Some(r);
                self.received = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.received = true;
                self.result = Some(Err(GisError::Http("the worker stopped without an answer".into())));
            }
        }
    }

    /// True when the result is there (or the job was cancelled). Never blocks.
    pub fn is_done(&mut self) -> bool {
        self.pump();
        self.cancelled || self.result.is_some()
    }

    /// The result, once. `None` while the job runs or after the result was taken.
    pub fn take(&mut self) -> Option<Result<T>> {
        self.pump();
        if self.cancelled {
            self.cancelled = false;
            self.result = None;
            return Some(Err(GisError::Http("cancelled".into())));
        }
        self.result.take()
    }

    /// Stops waiting for the job. The thread ends by itself; its result is dropped.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Time since the job started.
    pub fn age(&self) -> Duration {
        self.started.elapsed()
    }

    /// Waits for the result for at most `timeout`. For scripts and tests, not for the UI.
    pub fn wait(&mut self, timeout: Duration) -> Option<Result<T>> {
        let end = Instant::now() + timeout;
        loop {
            if self.is_done() {
                return self.take();
            }
            if Instant::now() >= end {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_does_not_block_and_the_result_comes_once() {
        let (go, wait) = channel::<()>();
        let mut j = Job::spawn("slow", move || {
            wait.recv().unwrap();
            Ok(5)
        });
        assert_eq!(j.label, "slow");
        let t = Instant::now();
        assert!(!j.is_done());
        assert!(j.take().is_none());
        assert!(t.elapsed() < Duration::from_millis(200));
        go.send(()).unwrap();
        assert_eq!(j.wait(Duration::from_secs(5)).unwrap().unwrap(), 5);
        assert!(j.take().is_none());
        assert!(j.age() < Duration::from_secs(5));
    }

    #[test]
    fn errors_panics_timeouts_and_cancel() {
        let mut j: Job<u8> = Job::spawn("e", || Err(GisError::Param("bad".into())));
        assert_eq!(j.wait(Duration::from_secs(5)).unwrap().unwrap_err(), GisError::Param("bad".into()));
        let mut p: Job<u8> = Job::spawn("p", || panic!("boom"));
        assert!(p.wait(Duration::from_secs(5)).unwrap().unwrap_err().to_string().contains("stopped"));
        let (go, wait) = channel::<()>();
        let mut s: Job<u8> = Job::spawn("s", move || {
            let _ = wait.recv();
            Ok(1)
        });
        assert!(s.wait(Duration::from_millis(20)).is_none());
        s.cancel();
        assert!(s.is_done());
        assert_eq!(s.take().unwrap().unwrap_err().to_string(), "cancelled");
        let _ = go.send(());
        assert!(s.take().is_none());
    }
}
