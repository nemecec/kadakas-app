//! The background worker of the pattern plan on the canvas. Laying a plan and checking it can take
//! seconds on a big lot; the window must not wait for them. One thread does one job at a time.
//! Each object has one place ([`Slot`]) in the queue: a newer job for the same place replaces the
//! older one that waits, and a check that runs for an old key is stopped. A job that is done wakes
//! the window ([`egui::Context::request_repaint`]). Where there are no threads (the browser) the
//! job runs at once, in the window.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use kadakas_core::hardscape::draw::{PrepareJob, Prepared};
use kadakas_core::model::ObjectId;

/// The place of an object in the queue: its ID, whether it is a ghost of a compare (a ghost has
/// the ID of the live object, and its own plan), and the window thread that asked (two windows
/// do not share places; in the app there is one).
pub type Slot = (ObjectId, bool, std::thread::ThreadId);

/// The place of an object for the window on this thread.
pub fn slot(id: ObjectId, ghost: bool) -> Slot {
    (id, ghost, std::thread::current().id())
}

enum Job {
    Prepare {
        slot: Slot,
        job: Box<PrepareJob>,
        ctx: egui::Context,
    },
    Check {
        slot: Slot,
        prepared: Arc<Prepared>,
        cancel: Arc<AtomicBool>,
        ctx: egui::Context,
    },
}

impl Job {
    fn slot(&self) -> Slot {
        match self {
            Job::Prepare { slot, .. } | Job::Check { slot, .. } => *slot,
        }
    }

    fn is_check(&self) -> bool {
        matches!(self, Job::Check { .. })
    }
}

/// A plan that was laid.
pub struct Laid {
    pub slot: Slot,
    pub key: u64,
    pub plan: Result<Arc<Prepared>, String>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    /// The job that runs now: its place, whether it is a check, the key of the plan it works on,
    /// and the flag that stops it.
    running: Option<(Slot, bool, u64, Arc<AtomicBool>)>,
    done: Vec<Laid>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

pub struct Worker {
    shared: Arc<Shared>,
}

static WORKER: OnceLock<Worker> = OnceLock::new();

/// The worker of the process. The thread starts with the first job.
pub fn worker() -> &'static Worker {
    WORKER.get_or_init(|| Worker {
        shared: Arc::new(Shared::default()),
    })
}

fn lock(s: &Shared) -> std::sync::MutexGuard<'_, State> {
    s.state.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(not(target_arch = "wasm32"))]
fn start(shared: Arc<Shared>) {
    let spawned = std::thread::Builder::new()
        .name("kadakas-pattern-plan".into())
        .spawn(move || {
            loop {
                let job = {
                    let mut st = lock(&shared);
                    loop {
                        if let Some(job) = st.queue.pop_front() {
                            let cancel = match &job {
                                Job::Check { cancel, .. } => cancel.clone(),
                                Job::Prepare { .. } => Arc::new(AtomicBool::new(false)),
                            };
                            let key = match &job {
                                Job::Check { prepared, .. } => prepared.key,
                                Job::Prepare { job, .. } => job.key(),
                            };
                            st.running = Some((job.slot(), job.is_check(), key, cancel));
                            break job;
                        }
                        st = shared.wake.wait(st).unwrap_or_else(|e| e.into_inner());
                    }
                };
                let done = run(job);
                let mut st = lock(&shared);
                st.running = None;
                if let Some(d) = done {
                    st.done.push(d);
                    // Results that no window asked for do not pile up.
                    if st.done.len() > 64 {
                        st.done.remove(0);
                    }
                }
            }
        });
    // Without a thread the jobs wait for the next call, which runs them in the window.
    let _ = spawned;
}

fn run(job: Job) -> Option<Laid> {
    match job {
        Job::Prepare { slot, job, ctx } => {
            let key = job.key();
            let plan = job.run();
            ctx.request_repaint();
            Some(Laid { slot, key, plan })
        }
        Job::Check {
            prepared,
            cancel,
            ctx,
            ..
        } => {
            let _ = prepared.findings_cancellable(&cancel);
            ctx.request_repaint();
            None
        }
    }
}

impl Worker {
    fn push(&self, job: Job) {
        {
            let mut st = lock(&self.shared);
            // The newest job of a place and a kind wins over one that waits.
            st.queue
                .retain(|j| !(j.slot() == job.slot() && j.is_check() == job.is_check()));
            // A check for an older plan of the place is not wanted any more.
            if job.is_check()
                && let Some((slot, true, _, cancel)) = &st.running
                && *slot == job.slot()
            {
                cancel.store(true, Ordering::Relaxed);
            }
            st.queue.push_back(job);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            static STARTED: std::sync::Once = std::sync::Once::new();
            STARTED.call_once(|| start(self.shared.clone()));
            self.shared.wake.notify_one();
        }
        #[cfg(target_arch = "wasm32")]
        self.run_inline();
    }

    #[cfg(target_arch = "wasm32")]
    fn run_inline(&self) {
        loop {
            let job = lock(&self.shared).queue.pop_front();
            let Some(job) = job else { return };
            if let Some(d) = run(job) {
                lock(&self.shared).done.push(d);
            }
        }
    }

    /// Asks for the plan of an object to be laid.
    pub fn prepare(&self, slot: Slot, job: PrepareJob, ctx: &egui::Context) {
        self.push(Job::Prepare {
            slot,
            job: Box::new(job),
            ctx: ctx.clone(),
        });
    }

    /// Asks for the check of a plan. A check of another plan of the same place is stopped.
    pub fn check(&self, slot: Slot, prepared: Arc<Prepared>, ctx: &egui::Context) {
        self.push(Job::Check {
            slot,
            prepared,
            cancel: Arc::new(AtomicBool::new(false)),
            ctx: ctx.clone(),
        });
    }

    /// Stops the check of one place, if it waits or runs (the person turned the overlay off).
    pub fn cancel_check(&self, slot: Slot) {
        let mut st = lock(&self.shared);
        st.queue.retain(|j| !(j.is_check() && j.slot() == slot));
        if let Some((s, true, _, cancel)) = &st.running
            && *s == slot
        {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The plan that was laid for a place and a key, if it is done (it is handed out once).
    pub fn take(&self, slot: Slot, key: u64) -> Option<Laid> {
        let mut st = lock(&self.shared);
        let i = st
            .done
            .iter()
            .position(|d| d.slot == slot && d.key == key)?;
        Some(st.done.remove(i))
    }

    /// True while a job waits or runs.
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        let st = lock(&self.shared);
        st.running.is_some() || !st.queue.is_empty()
    }
}
