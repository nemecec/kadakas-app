//! The background worker of the pattern plan on the canvas. Laying a plan, cutting a window of it
//! and checking it can take seconds on a big lot; the window must not wait for any of them. One
//! thread does one job at a time. The order is: plans first, then windows, then checks, so a check
//! never holds up the plan that the person waits for. Each object has one place ([`Slot`]) in the
//! queue: a newer job of the same kind for the same place replaces the older one that waits, a
//! newer plan for a place drops its checks (and stops the one that runs), and the old results of
//! the place are forgotten. A job that is done wakes the window
//! ([`egui::Context::request_repaint`]). Where there are no threads (the browser) the job runs at
//! once, in the window.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use kadakas_core::hardscape::draw::{BBox, DrawOptions, Drawn, PrepareJob, Prepared};
use kadakas_core::model::ObjectId;

/// The place of an object in the queue: its ID, whether it is a ghost of a compare (a ghost has
/// the ID of the live object, and its own plan), and the window thread that asked (two windows
/// do not share places; in the app there is one).
pub type Slot = (ObjectId, bool, std::thread::ThreadId);

/// The place of an object for the window on this thread.
pub fn slot(id: ObjectId, ghost: bool) -> Slot {
    (id, ghost, std::thread::current().id())
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Prepare,
    Draw,
    Check,
}

enum Job {
    Prepare {
        slot: Slot,
        job: Box<PrepareJob>,
        ctx: egui::Context,
    },
    Draw {
        slot: Slot,
        id: u64,
        prepared: Arc<Prepared>,
        window: BBox,
        opts: DrawOptions,
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
            Job::Prepare { slot, .. } | Job::Draw { slot, .. } | Job::Check { slot, .. } => *slot,
        }
    }

    fn kind(&self) -> Kind {
        match self {
            Job::Prepare { .. } => Kind::Prepare,
            Job::Draw { .. } => Kind::Draw,
            Job::Check { .. } => Kind::Check,
        }
    }
}

/// A plan that was laid.
pub struct Laid {
    pub slot: Slot,
    pub key: u64,
    pub plan: Result<Arc<Prepared>, String>,
}

/// A window that was cut from a plan.
pub struct Cut {
    pub slot: Slot,
    pub id: u64,
    pub drawn: Drawn,
}

enum Done {
    Laid(Laid),
    Cut(Cut),
}

impl Done {
    fn slot(&self) -> Slot {
        match self {
            Done::Laid(l) => l.slot,
            Done::Cut(c) => c.slot,
        }
    }
}

/// What runs now: the place, the kind, the key or request it is for, and the flag that stops it.
struct Running {
    slot: Slot,
    kind: Kind,
    what: u64,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    running: Option<Running>,
    done: Vec<Done>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

pub struct Worker {
    shared: Arc<Shared>,
    /// False for a worker of a test that looks at the queue: no thread takes the jobs.
    threaded: bool,
}

static WORKER: OnceLock<Worker> = OnceLock::new();

/// The worker of the process. The thread starts with the first job.
pub fn worker() -> &'static Worker {
    WORKER.get_or_init(|| Worker {
        shared: Arc::new(Shared::default()),
        threaded: true,
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
                            let (cancel, what) = match &job {
                                Job::Check {
                                    cancel, prepared, ..
                                } => (cancel.clone(), prepared.key),
                                Job::Prepare { job, .. } => {
                                    (Arc::new(AtomicBool::new(false)), job.key())
                                }
                                Job::Draw { id, .. } => (Arc::new(AtomicBool::new(false)), *id),
                            };
                            st.running = Some(Running {
                                slot: job.slot(),
                                kind: job.kind(),
                                what,
                                cancel,
                            });
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
                    // Results that no window asked for do not pile up. A window that asks again
                    // for a result that went is told by `pending_*`.
                    if st.done.len() > 64 {
                        st.done.remove(0);
                    }
                }
            }
        });
    // Without a thread the jobs wait for the next call, which runs them in the window.
    let _ = spawned;
}

fn run(job: Job) -> Option<Done> {
    match job {
        Job::Prepare { slot, job, ctx } => {
            let key = job.key();
            let plan = job.run();
            ctx.request_repaint();
            Some(Done::Laid(Laid { slot, key, plan }))
        }
        Job::Draw {
            slot,
            id,
            prepared,
            window,
            opts,
            ctx,
        } => {
            let drawn = prepared.draw(Some(window), &opts);
            ctx.request_repaint();
            Some(Done::Cut(Cut { slot, id, drawn }))
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

#[cfg(test)]
thread_local! {
    static PUSHED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many jobs this thread has handed to a worker. The worker of the process is shared by the
/// tests that run at the same time, so a test counts its own jobs with this, not with `busy`.
#[cfg(test)]
pub fn pushed() -> u64 {
    PUSHED.with(std::cell::Cell::get)
}

impl Worker {
    fn push(&self, job: Job) {
        #[cfg(test)]
        PUSHED.with(|p| p.set(p.get() + 1));
        {
            let mut st = lock(&self.shared);
            let (slot, kind) = (job.slot(), job.kind());
            // The newest job of a place and a kind wins over one that waits.
            st.queue.retain(|j| !(j.slot() == slot && j.kind() == kind));
            if kind == Kind::Prepare {
                // A new plan for the place: its checks are of the old plan. They are dropped, the
                // one that runs is stopped, and the results of the place are forgotten.
                st.queue
                    .retain(|j| !(j.slot() == slot && j.kind() != Kind::Prepare));
                if let Some(r) = &st.running
                    && r.slot == slot
                    && r.kind == Kind::Check
                {
                    r.cancel.store(true, Ordering::Relaxed);
                }
                st.done.retain(|d| d.slot() != slot);
            }
            if kind == Kind::Check
                && let Some(r) = &st.running
                && r.slot == slot
                && r.kind == Kind::Check
            {
                // A check for another plan of the place is not wanted any more.
                r.cancel.store(true, Ordering::Relaxed);
            }
            // The order: plans, windows, checks; first come first served inside a kind.
            let at = st
                .queue
                .iter()
                .position(|j| j.kind() > kind)
                .unwrap_or(st.queue.len());
            st.queue.insert(at, job);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.threaded {
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

    /// Asks for a window of a plan to be cut. `id` names the request.
    pub fn draw(
        &self,
        slot: Slot,
        id: u64,
        prepared: Arc<Prepared>,
        window: BBox,
        opts: DrawOptions,
        ctx: &egui::Context,
    ) {
        self.push(Job::Draw {
            slot,
            id,
            prepared,
            window,
            opts,
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
        st.queue
            .retain(|j| !(j.kind() == Kind::Check && j.slot() == slot));
        if let Some(r) = &st.running
            && r.slot == slot
            && r.kind == Kind::Check
        {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The plan that was laid for a place and a key, if it is done (it is handed out once).
    pub fn take(&self, slot: Slot, key: u64) -> Option<Laid> {
        let mut st = lock(&self.shared);
        let i = st
            .done
            .iter()
            .position(|d| matches!(d, Done::Laid(l) if l.slot == slot && l.key == key))?;
        match st.done.remove(i) {
            Done::Laid(l) => Some(l),
            Done::Cut(_) => None,
        }
    }

    /// The window that was cut for a place and a request, if it is done (handed out once).
    pub fn take_cut(&self, slot: Slot, id: u64) -> Option<Cut> {
        let mut st = lock(&self.shared);
        let i = st
            .done
            .iter()
            .position(|d| matches!(d, Done::Cut(c) if c.slot == slot && c.id == id))?;
        match st.done.remove(i) {
            Done::Cut(c) => Some(c),
            Done::Laid(_) => None,
        }
    }

    /// True if a plan for the place and key waits, runs or is done and not taken. A window that
    /// asked and finds this false asks again (the result went, or the job never came).
    pub fn pending_plan(&self, slot: Slot, key: u64) -> bool {
        let st = lock(&self.shared);
        st.queue.iter().any(
            |j| matches!(j, Job::Prepare { slot: s, job, .. } if *s == slot && job.key() == key),
        ) || st
            .running
            .as_ref()
            .is_some_and(|r| r.slot == slot && r.kind == Kind::Prepare && r.what == key)
            || st
                .done
                .iter()
                .any(|d| matches!(d, Done::Laid(l) if l.slot == slot && l.key == key))
    }

    /// The same for a window.
    pub fn pending_cut(&self, slot: Slot, id: u64) -> bool {
        let st = lock(&self.shared);
        st.queue
            .iter()
            .any(|j| matches!(j, Job::Draw { slot: s, id: i, .. } if *s == slot && *i == id))
            || st
                .running
                .as_ref()
                .is_some_and(|r| r.slot == slot && r.kind == Kind::Draw && r.what == id)
            || st
                .done
                .iter()
                .any(|d| matches!(d, Done::Cut(c) if c.slot == slot && c.id == id))
    }

    /// True while a job waits or runs.
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        let st = lock(&self.shared);
        st.running.is_some() || !st.queue.is_empty()
    }

    /// A worker without a thread: the jobs stay in the queue (tests look at the order).
    #[cfg(test)]
    pub fn detached() -> Worker {
        Worker {
            shared: Arc::new(Shared::default()),
            threaded: false,
        }
    }

    /// The kinds of the jobs that wait, in order (tests look at the priority).
    #[cfg(test)]
    pub fn queued(&self, slot: Option<Slot>) -> Vec<&'static str> {
        lock(&self.shared)
            .queue
            .iter()
            .filter(|j| slot.is_none_or(|s| j.slot() == s))
            .map(|j| match j.kind() {
                Kind::Prepare => "plan",
                Kind::Draw => "window",
                Kind::Check => "check",
            })
            .collect()
    }
}
