//! Crash reports (PLAT-20).
//!
//! The app writes what it knows when it dies. The launcher sees the abnormal end and keeps the
//! file. At the next start the app shows the report, and sends it only when the person says so
//! (or has said so before). The report goes through the feedback channel (`kadakas-feedback`).
//!
//! - [`file`]: the crash file, its frames, and the reading of a Rust backtrace.
//! - [`scrub`]: removes paths, file names, user names, e-mail addresses and tokens. A pure
//!   function, tested hard: this is what stands between the person and the server.
//! - [`signature`]: groups equal crashes (the top frames of the backtrace and the version).
//! - [`ring`]: the last log lines and the names of the last commands, in memory.
//! - [`queue`]: the folder of kept reports (at most 10, the oldest go first).
//! - [`report`]: from a crash file to a report in the outbox, and the exact text that goes out.
//! - `capture` (native, feature `capture`): the panic hook and the native crash handler.
//!
//! Without the feature `capture` the crate has no code that can fail the program: the launcher
//! uses it to move files, the app uses it to read and send.
//!
//! **Limit.** A native crash (a signal or an exception in a driver or a library) leaves a small
//! record: the signal or exception, the thread, the log lines and the command names. It holds no
//! stack: a minidump needs a second process that writes the dump while the first is dead
//! (`minidumper`), and that is not built yet. The stack of a panic is complete.

pub mod file;
#[cfg(not(target_arch = "wasm32"))]
pub mod queue;
pub mod report;
pub mod ring;
pub mod scrub;
pub mod signature;

#[cfg(all(feature = "capture", not(target_arch = "wasm32")))]
pub mod capture;

pub use file::{CrashFile, CrashKind, Frame, Gpu};
#[cfg(not(target_arch = "wasm32"))]
pub use queue::MAX_KEPT;
