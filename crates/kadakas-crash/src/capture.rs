//! The app writes what it knows when it dies (native only, feature `capture`).
//!
//! - **A panic.** The hook of this module chains to the hook that was there before (so the
//!   message still goes to the terminal), and writes the crash file: message, thread, the whole
//!   backtrace, version, channel, system, graphics adapter, the last log lines, the names of the
//!   last commands, the peak memory and the uptime. A panic that a `catch_unwind` of Kadakas
//!   catches is no crash (`ring::CatchGuard`): no file.
//! - **A native crash** (a signal or an exception in a driver, a library or unsafe code): the
//!   handler of the `crash-handler` crate (Embark Studios, MIT OR Apache-2.0) runs when the
//!   process is already broken. It writes the same file with the signal or exception, the log
//!   lines and the command names, and lets the process die. It writes no minidump and no stack
//!   (on Linux it tries the stack of the crashing thread). On Unix a timer of 5 seconds ends the
//!   process if the handler hangs (a crash inside the allocator). See the module text of the
//!   crate for the limit.
//!
//! The file is `<runs>/<run id>.crash.json`. A clean exit calls [`clean_exit`], which deletes
//! it (a panic of a worker thread that the program survived is no crash). The launcher keeps
//! the file when the run ended without the clean-exit mark (`kadakas-update`).

use crate::file::{CrashFile, CrashKind, Frame, Gpu, parse_backtrace};
use crate::queue;
use std::panic::PanicHookInfo;
use std::path::PathBuf;
use std::sync::{Mutex, Once, OnceLock};
use std::time::Instant;

/// What the app tells the hook once, at the start.
#[derive(Clone, Debug)]
pub struct Config {
    /// The folder of the launcher (`runs/` is inside).
    pub root: PathBuf,
    pub run_id: String,
    pub version: String,
    pub channel: String,
    /// For example `macos aarch64`.
    pub os: String,
}

struct State {
    cfg: Config,
    path: PathBuf,
    start: Instant,
}

static STATE: OnceLock<State> = OnceLock::new();
static GPU: Mutex<Option<Gpu>> = Mutex::new(None);
static HOOKS: Once = Once::new();

/// Tells the hook which graphics adapter the app uses. Call it when the adapter is known.
pub fn set_gpu(gpu: Gpu) {
    if let Ok(mut g) = GPU.lock() {
        *g = Some(gpu);
    }
}

/// The path of the crash file of this run, once [`install`] ran.
pub fn crash_path() -> Option<PathBuf> {
    STATE.get().map(|s| s.path.clone())
}

/// A name for the system: `macos aarch64`.
pub fn default_os() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Installs the panic hook and the native crash handler. Once; the second call changes nothing.
pub fn install(cfg: Config) {
    let path = queue::live_path(&cfg.root, &cfg.run_id);
    let first = STATE
        .set(State {
            cfg,
            path,
            start: Instant::now(),
        })
        .is_ok();
    if !first {
        return;
    }
    HOOKS.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !crate::ring::catching() {
                write_panic(info);
            }
            previous(info);
        }));
        attach_native();
    });
}

/// The program ends in the normal way: the crash file of this run is not needed. Call it
/// before the clean-exit mark is written.
pub fn clean_exit() {
    if let Some(s) = STATE.get() {
        let _ = std::fs::remove_file(&s.path);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The peak resident memory of the process in MiB, where the system tells.
fn peak_memory_mib() -> Option<u64> {
    #[cfg(unix)]
    {
        // SAFETY: `getrusage` fills the struct that we give it.
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
        if r != 0 {
            return None;
        }
        let max = u64::try_from(u.ru_maxrss).ok()?;
        // Bytes on macOS, KiB elsewhere.
        let bytes = if cfg!(target_os = "macos") {
            max
        } else {
            max * 1024
        };
        Some(bytes >> 20)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn base(kind: CrashKind) -> Option<CrashFile> {
    let s = STATE.get()?;
    let mut f = CrashFile::new(kind);
    f.run_id = s.cfg.run_id.clone();
    f.created = now_secs();
    f.version = s.cfg.version.clone();
    f.channel = s.cfg.channel.clone();
    f.os = s.cfg.os.clone();
    f.gpu = GPU.try_lock().ok().and_then(|g| g.clone());
    let (log, commands) = crate::ring::snapshot();
    f.log = log;
    f.commands = commands;
    f.memory_peak_mib = peak_memory_mib();
    f.uptime_secs = s.start.elapsed().as_secs();
    Some(f)
}

fn write(f: CrashFile) {
    let Some(s) = STATE.get() else { return };
    if let Some(dir) = s.path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = kadakas_sys::fsio::write_private(&s.path, &f.bounded().to_json());
}

thread_local! {
    static IN_HOOK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn write_panic(info: &PanicHookInfo<'_>) {
    // A panic while the hook runs must not run it again.
    if IN_HOOK.try_with(|c| c.replace(true)).unwrap_or(true) {
        return;
    }
    let Some(mut f) = base(CrashKind::Panic) else {
        return;
    };
    let payload = info.payload();
    f.message = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into());
    f.thread = std::thread::current()
        .name()
        .map_or_else(|| "unnamed".to_string(), str::to_string);
    let bt = std::backtrace::Backtrace::force_capture().to_string();
    f.frames = parse_backtrace(&bt);
    if let Some(l) = info.location() {
        // The place of the panic is a source file of the build computer; the report keeps the
        // file and the line only if the file belongs to Kadakas (`scrub::source_path`).
        f.frames.insert(
            0,
            Frame {
                function: "<panic location>".into(),
                file: Some(l.file().to_string()),
                line: Some(l.line()),
            },
        );
    }
    write(f);
    let _ = IN_HOOK.try_with(|c| c.set(false));
}

fn attach_native() {
    use crash_handler::{CrashContext, CrashEventResult, CrashHandler, make_crash_event};
    // SAFETY: the closure runs in a broken process. It does what it can and never relies on the
    // state of the program; a hang ends by the timer on Unix.
    let event = unsafe {
        make_crash_event(|ctx: &CrashContext| {
            // `alarm` only sets a timer. SIGALRM kills the process if we hang.
            #[cfg(unix)]
            libc::alarm(5);
            write_native(ctx);
            // Let the next handler run (the default one ends the process).
            CrashEventResult::Handled(false)
        })
    };
    if let Ok(handler) = CrashHandler::attach(event) {
        // The handler must live as long as the process.
        std::mem::forget(handler);
    }
}

fn write_native(ctx: &crash_handler::CrashContext) {
    let Some(mut f) = base(CrashKind::Native) else {
        return;
    };
    f.message = describe(ctx);
    f.thread = String::new();
    #[cfg(target_os = "linux")]
    {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        f.frames = parse_backtrace(&bt);
    }
    #[cfg(not(target_os = "linux"))]
    {
        f.frames = Vec::<Frame>::new();
    }
    write(f);
}

#[cfg(target_os = "linux")]
fn describe(ctx: &crash_handler::CrashContext) -> String {
    let no = ctx.siginfo.ssi_signo as i32;
    format!(
        "{} (signal {no}, code {}, address {:#x})",
        signal_name(no),
        ctx.siginfo.ssi_code,
        ctx.siginfo.ssi_addr
    )
}

#[cfg(target_os = "macos")]
fn describe(ctx: &crash_handler::CrashContext) -> String {
    match &ctx.exception {
        Some(e) => {
            let name = match e.kind {
                1 => "EXC_BAD_ACCESS",
                2 => "EXC_BAD_INSTRUCTION",
                3 => "EXC_ARITHMETIC",
                4 => "EXC_EMULATION",
                5 => "EXC_SOFTWARE",
                6 => "EXC_BREAKPOINT",
                7 => "EXC_SYSCALL",
                8 => "EXC_MACH_SYSCALL",
                9 => "EXC_RPC_ALERT",
                10 => "EXC_CRASH",
                11 => "EXC_RESOURCE",
                12 => "EXC_GUARD",
                13 => "EXC_CORPSE_NOTIFY",
                _ => "EXC_UNKNOWN",
            };
            match e.subcode {
                Some(sub) => format!("{name} (code {}, subcode {sub:#x})", e.code),
                None => format!("{name} (code {})", e.code),
            }
        }
        None => "EXC_UNKNOWN (no exception data)".to_string(),
    }
}

#[cfg(windows)]
fn describe(ctx: &crash_handler::CrashContext) -> String {
    let code = ctx.exception_code as u32;
    let name = match code {
        0xC000_0005 => "EXCEPTION_ACCESS_VIOLATION",
        0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO",
        0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION",
        0xC000_00FD => "EXCEPTION_STACK_OVERFLOW",
        0xC000_0374 => "STATUS_HEAP_CORRUPTION",
        0xC000_0409 => "STATUS_STACK_BUFFER_OVERRUN",
        0x8000_0003 => "EXCEPTION_BREAKPOINT",
        _ => "EXCEPTION_UNKNOWN",
    };
    format!("{name} ({code:#010x})")
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn describe(_ctx: &crash_handler::CrashContext) -> String {
    "native crash".to_string()
}

#[cfg(target_os = "linux")]
fn signal_name(no: i32) -> &'static str {
    match no {
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGBUS => "SIGBUS",
        libc::SIGILL => "SIGILL",
        libc::SIGFPE => "SIGFPE",
        libc::SIGABRT => "SIGABRT",
        libc::SIGTRAP => "SIGTRAP",
        _ => "SIGNAL",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_os_names_system_and_architecture() {
        let os = default_os();
        assert!(os.contains(std::env::consts::OS) && os.contains(std::env::consts::ARCH));
    }

    #[test]
    fn peak_memory_is_known_on_unix() {
        if cfg!(unix) {
            assert!(peak_memory_mib().is_some_and(|m| m < 1 << 20));
        }
    }
}
