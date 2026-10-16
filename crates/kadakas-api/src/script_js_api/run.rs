//! A run on a worker thread (native only), for a host with a window.
//!
//! The engine runs on its own thread. A call of the program becomes a message to the thread of
//! the session, which serves it with [`Session::script_pump`] (a few milliseconds at a time, in
//! each frame) and sends the reply back. So the window stays alive while the program runs. The
//! program can be stopped: [`ScriptRun::stop`] sets a flag, the next call of the program fails
//! with a stop error, and the clock of the engine interrupts a program that makes no calls.
//!
//! While the run is open the session refuses every call that is not one of the run (a conflict),
//! so the edits of a person or of another agent cannot become part of the one undo step of the
//! run. The window should also block the person's edits (a modal "Stop" window).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use kadakas_core::change::Origin;
use kadakas_core::command::RunMark;
use kadakas_script_js::{HostError, Output, ScriptHost};
use serde_json::{Map, Value};

use super::{Prepared, close_run, open_run, prepare, serve};
use crate::{RpcError, Session};

enum Msg {
    Call {
        method: String,
        params: Value,
        reply: Sender<Result<Value, HostError>>,
    },
    Done(Result<Output, kadakas_script_js::Failure>),
}

/// A run in progress. Give it to [`Session::script_pump`] until that returns the answer.
pub struct ScriptRun {
    rx: Receiver<Msg>,
    stop: Arc<AtomicBool>,
    prepared: Prepared,
    mark: RunMark,
    origin: Origin,
    revision: u64,
    calls: u64,
}

impl ScriptRun {
    /// Asks the program to stop. The run ends with a limit error at the next call or tick.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// True after [`ScriptRun::stop`].
    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Milliseconds since the run started.
    pub fn elapsed_ms(&self) -> u64 {
        self.prepared.clock.millis()
    }

    /// How many calls the program made so far.
    pub fn calls(&self) -> u64 {
        self.calls
    }
}

impl Drop for ScriptRun {
    fn drop(&mut self) {
        // A run that is dropped does not run on: its thread stops at the next call.
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The calls of the program, as messages.
struct ChannelHost {
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
}

impl ScriptHost for ChannelHost {
    fn call(&mut self, method: &str, params: Value, _typed: bool) -> Result<Value, HostError> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(HostError::Stop("stopped by the person".into()));
        }
        let (reply, answer) = channel();
        let gone = || HostError::Stop("the session is gone".into());
        self.tx
            .send(Msg::Call {
                method: method.to_string(),
                params,
                reply,
            })
            .map_err(|_| gone())?;
        answer.recv().map_err(|_| gone())?
    }
}

impl Session {
    /// Starts a program on a worker thread (see the module text). The parameters are those of
    /// `script.eval`. Call [`Session::script_pump`] in every frame until it gives the answer.
    pub fn script_start(&mut self, params: &Value) -> Result<ScriptRun, RpcError> {
        if self.script_js.open {
            return Err(RpcError::conflict("a script is running already"));
        }
        let empty = Map::new();
        let prepared = prepare(self, params.as_object().unwrap_or(&empty))?;
        let revision = self.editor.revision();
        let (mark, origin) = open_run(self);
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (p, tx, stop) = (prepared.clone(), tx.clone(), stop.clone());
            move || {
                let mut host = ChannelHost {
                    tx: tx.clone(),
                    stop: stop.clone(),
                };
                let watch = || {
                    if stop.load(Ordering::Relaxed) {
                        Some("stopped by the person".to_string())
                    } else {
                        p.expired()
                    }
                };
                let result = p.engine.run_watched(&p.request(), &mut host, &watch);
                let _ = tx.send(Msg::Done(result));
            }
        };
        if let Err(e) = std::thread::Builder::new()
            .name("kadakas-script".into())
            .spawn(worker)
        {
            self.editor.rollback_run(mark);
            self.editor.set_origin(origin);
            self.script_js.open = false;
            return Err(RpcError::internal(format!("cannot start the thread: {e}")));
        }
        Ok(ScriptRun {
            rx,
            stop,
            prepared,
            mark,
            origin,
            revision,
            calls: 0,
        })
    }

    /// Serves the calls of a run for at most `budget` (one call can take longer: a native
    /// operation does not stop in the middle). Returns `None` while the run goes on, and the
    /// answer of `script.eval` (or its error) when it has ended. After the answer the run is closed.
    pub fn script_pump(
        &mut self,
        run: &mut ScriptRun,
        budget: Duration,
    ) -> Option<Result<Value, RpcError>> {
        let until = Instant::now() + budget;
        loop {
            match run.rx.try_recv() {
                Ok(Msg::Call {
                    method,
                    params,
                    reply,
                }) => {
                    run.calls += 1;
                    let answer = serve(self, &run.prepared, &method, params);
                    let _ = reply.send(answer);
                    if Instant::now() >= until {
                        return None;
                    }
                }
                Ok(Msg::Done(result)) => {
                    let (mark, origin, revision) = (run.mark, run.origin, run.revision);
                    return Some(close_run(
                        self,
                        &run.prepared,
                        mark,
                        origin,
                        revision,
                        result,
                        run.calls,
                    ));
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    // The thread ended without an answer (it panicked).
                    let failure =
                        kadakas_script_js::Failure::from(kadakas_script_js::ScriptError::Trap(
                            "the script thread ended without an answer".into(),
                        ));
                    let (mark, origin, revision) = (run.mark, run.origin, run.revision);
                    return Some(close_run(
                        self,
                        &run.prepared,
                        mark,
                        origin,
                        revision,
                        Err(failure),
                        run.calls,
                    ));
                }
            }
        }
    }
}
