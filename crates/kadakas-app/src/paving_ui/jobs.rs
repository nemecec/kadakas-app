//! Work that must not stop the window: the counts of units, the zones, the courses and the pattern
//! check of a hardscape take a long time on a big lot, and every step of an agent changes the
//! document. A [`Job`] runs one such piece of work on a thread, on a copy of what it needs, and
//! keeps the result with the revision it is for. While the work runs, the window shows the result
//! of an earlier revision (marked old) or a "counting" label. The browser has no threads: there
//! the work runs at once.

use std::sync::mpsc::Receiver;

use kadakas_core::model::ObjectId;

type Work<T> = Box<dyn FnOnce() -> Result<T, String> + Send>;

struct Running<T> {
    key: (u64, ObjectId),
    rx: Receiver<Result<T, String>>,
}

/// One kind of background work for one object at a time.
pub struct Job<T> {
    running: Option<Running<T>>,
    done: Option<((u64, ObjectId), Result<T, String>)>,
}

impl<T> Default for Job<T> {
    fn default() -> Self {
        Job {
            running: None,
            done: None,
        }
    }
}

/// What a job has for the revision that is asked for.
pub enum Poll<'a, T> {
    /// The result is of this revision and this object.
    Fresh(&'a Result<T, String>),
    /// The result is of an earlier revision (or of the same object only): work is under way.
    Old(&'a Result<T, String>),
    /// There is no result for this object yet: work is under way.
    Busy,
}

impl<T: Send + 'static> Job<T> {
    /// Takes in a result that has arrived.
    fn collect(&mut self) {
        let Some(r) = &self.running else { return };
        match r.rx.try_recv() {
            Ok(res) => {
                let key = r.key;
                self.running = None;
                self.done = Some((key, res));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            // The thread ended without an answer (it panicked): that is the result for its
            // key, so the work is not started again at every frame.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                let key = r.key;
                self.running = None;
                self.done = Some((key, Err(kadakas_l10n::tr!("paving-ui-the-work-stopped"))));
            }
        }
    }

    /// Starts `work` for this revision and object, unless a job runs. `work` runs on a thread
    /// (in the browser, here and now).
    pub fn start(&mut self, ctx: &egui::Context, rev: u64, id: ObjectId, work: Work<T>) {
        self.collect();
        if self.running.is_some() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ctx;
            self.done = Some(((rev, id), work()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(work());
                ctx.request_repaint();
            });
            self.running = Some(Running { key: (rev, id), rx });
        }
    }

    /// The result for this revision and object, starting work when there is none and none is on
    /// its way for it. `make` builds the work (it copies what the thread needs) and is called only
    /// when work starts.
    pub fn poll(
        &mut self,
        ctx: &egui::Context,
        rev: u64,
        id: ObjectId,
        make: impl FnOnce() -> Work<T>,
    ) -> Poll<'_, T> {
        self.collect();
        let fresh = self.done.as_ref().is_some_and(|(k, _)| *k == (rev, id));
        if !fresh {
            let on_its_way = self.running.as_ref().is_some_and(|r| r.key == (rev, id));
            if !on_its_way && self.running.is_none() {
                self.start(ctx, rev, id, make());
                self.collect();
            }
            if self.running.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
            }
        }
        match &self.done {
            Some((k, r)) if *k == (rev, id) => Poll::Fresh(r),
            Some((k, r)) if k.1 == id => Poll::Old(r),
            _ => Poll::Busy,
        }
    }

    /// Takes the result of work that [`start`](Self::start) began, when it has come.
    pub fn take(&mut self) -> Option<Result<T, String>> {
        self.collect();
        let r = self.done.take()?;
        Some(r.1)
    }

    /// True while work runs.
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// The last result for this object, of any revision.
    pub fn last(&self, id: ObjectId) -> Option<&Result<T, String>> {
        self.done
            .as_ref()
            .filter(|(k, _)| k.1 == id)
            .map(|(_, r)| r)
    }

    /// Forgets everything (a new document). Work that runs finishes and is dropped.
    pub fn clear(&mut self) {
        self.running = None;
        self.done = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ObjectId {
        ObjectId::from_num(1)
    }

    #[test]
    fn work_runs_in_the_background_and_the_old_result_shows_until_the_new_one_comes() {
        let ctx = egui::Context::default();
        let mut job: Job<u32> = Job::default();
        let wait = |job: &mut Job<u32>, rev: u64| loop {
            let ready = matches!(
                job.poll(&ctx, rev, id(), || Box::new(move || Ok(rev as u32 * 10))),
                Poll::Fresh(_)
            );
            if ready {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        // Nothing yet: busy, then the result of the revision.
        assert!(matches!(
            job.poll(&ctx, 1, id(), || Box::new(|| Ok(10))),
            Poll::Busy | Poll::Fresh(_)
        ));
        wait(&mut job, 1);
        assert!(matches!(job.last(id()), Some(Ok(10))));
        // A new revision: the old result stays until the new one is there.
        let old = matches!(
            job.poll(&ctx, 2, id(), || Box::new(|| {
                std::thread::sleep(std::time::Duration::from_millis(40));
                Ok(20)
            })),
            Poll::Old(Ok(10)) | Poll::Fresh(_)
        );
        assert!(old);
        wait(&mut job, 2);
        assert!(matches!(job.last(id()), Some(Ok(20))));
        // Another object has no result: busy.
        assert!(matches!(
            job.poll(&ctx, 2, ObjectId::from_num(2), || Box::new(|| Ok(1))),
            Poll::Busy | Poll::Fresh(_)
        ));
        assert!(job.last(ObjectId::from_num(3)).is_none());
        job.clear();
        assert!(job.last(id()).is_none() && !job.busy());
    }

    #[test]
    fn a_job_that_panics_is_an_error_for_its_key_and_is_not_started_again() {
        let ctx = egui::Context::default();
        let mut job: Job<u32> = Job::default();
        let starts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut seen_error = false;
        for _ in 0..400 {
            let n = starts.clone();
            let p = job.poll(&ctx, 1, id(), || {
                Box::new(move || {
                    n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    panic!("boom")
                })
            });
            if matches!(p, Poll::Fresh(Err(_))) {
                seen_error = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(seen_error);
        // Asked again for the same key: the error stays, nothing starts.
        for _ in 0..5 {
            let n = starts.clone();
            let p = job.poll(&ctx, 1, id(), || {
                Box::new(move || {
                    n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(1)
                })
            });
            assert!(matches!(p, Poll::Fresh(Err(_))));
        }
        assert_eq!(starts.load(std::sync::atomic::Ordering::SeqCst), 1);
        // A new revision is a new try.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let p = job.poll(&ctx, 2, id(), || Box::new(|| Ok(5)));
        assert!(matches!(p, Poll::Old(Err(_)) | Poll::Fresh(Ok(5))));
    }
}
