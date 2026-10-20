//! The app writes what it knows when it dies (native only, feature `capture`).
//!
//! - **A panic.** The hook of this module chains to the hook that was there before (so the
//!   message still goes to the terminal), and writes the crash file: message, thread, the whole
//!   backtrace, version, channel, system, graphics adapter, the last log lines, the names of the
//!   last commands, the peak memory and the uptime. A panic that a `catch_unwind` of Kadakas
//!   catches is no crash (`ring::CatchGuard`): no file. Only the **first** uncaught panic of a
//!   run is written: it is the cause, and the later ones are often its effect. A panic that is
//!   followed by an abort (a panic in code that cannot unwind) keeps its file: the native
//!   handler sees the flag and writes nothing, so the backtrace stays.
//! - **A native crash** (a signal or an exception in a driver, a library or unsafe code): the
//!   handler of the `crash-handler` crate (Embark Studios, MIT OR Apache-2.0) runs when the
//!   process is already broken, so it must not allocate, format, serialise or take a lock (a
//!   crash inside `malloc` would make it wait for ever). The report is built before: a
//!   background thread rebuilds the bytes of the file every 2 seconds into one of two fixed
//!   buffers (log lines, command names, memory, run time), and the handler only writes the
//!   active buffer and the name and code of the signal or exception with raw system calls
//!   (`open`, `write`, `rename`) and lets the process die. It writes no minidump and no stack.
//!   On Unix a timer of 5 seconds (`alarm`) ends the process if the handler hangs; the
//!   launcher names that end `SIGALRM`. On Windows the handler uses `std::fs` (it allocates
//!   for the path) and a watchdog thread ends the process with the exit code `0xE0000001` after
//!   5 seconds; the launcher names that end `KADAKAS_CRASH_HANDLER_HUNG`. Both are reports with
//!   a name, from the launcher, when the handler did not write the file. See the module text of
//!   the crate for the limit.
//!
//! The file is `<runs>/<run id>.crash.json`. A clean exit calls [`clean_exit`], which deletes
//! it. The launcher keeps the file when the run ended without the clean-exit mark, and labels it
//! a survived panic when the process did not fail (`kadakas-update`).
//!
//! `std::panic::resume_unwind` does not call the panic hook. A panic that is resumed inside a
//! `CatchGuard` writes no file.

use crate::file::{CrashFile, CrashKind, Frame, Gpu, parse_backtrace};
use crate::queue;
use std::cell::UnsafeCell;
use std::panic::PanicHookInfo;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

/// What the app tells the hook once, at the start.
#[derive(Clone, Debug)]
pub struct Config {
    /// The folder of the launcher (`runs/` is inside).
    pub root: PathBuf,
    pub run_id: String,
    /// The name of the program: `kadakas-app`, `kadakas-modeler`.
    pub program: String,
    pub version: String,
    pub channel: String,
    /// For example `macos aarch64`.
    pub os: String,
}

struct State {
    cfg: Config,
    path: PathBuf,
    tmp: PathBuf,
    start: Instant,
}

static STATE: OnceLock<State> = OnceLock::new();
static GPU: Mutex<Option<Gpu>> = Mutex::new(None);
static HOOKS: Once = Once::new();
/// An uncaught panic wrote its file. The native handler then writes nothing.
static PANIC_WRITTEN: AtomicBool = AtomicBool::new(false);
/// The native handler runs (the watchdog looks at it).
static IN_NATIVE: AtomicBool = AtomicBool::new(false);

/// The size of one buffer of the prepared native report.
const CAP: usize = 128 << 10;
/// The exit code of the watchdog on Windows (a custom exception-like code).
pub const HUNG_EXIT_CODE: u32 = 0xE000_0001;

struct Slot(UnsafeCell<[u8; CAP]>);
// SAFETY: one thread (the refresher) writes the inactive slot; the handler reads the active one.
// The flip is an atomic store after the write. The handler runs when the process is dying, so a
// torn read in the rare case of two flips during one read costs a bad report, not safety.
unsafe impl Sync for Slot {}

static SLOTS: [Slot; 2] = [
    Slot(UnsafeCell::new([0; CAP])),
    Slot(UnsafeCell::new([0; CAP])),
];
static LENS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Rebuilds the prepared native report now (it is rebuilt every 2 seconds by itself). Call it
/// when the log lines or the commands must be in the report of a crash that follows at once.
pub fn refresh() {
    refresh_native();
}

/// Tells the hook which graphics adapter the app uses. Call it when the adapter is known.
pub fn set_gpu(gpu: Gpu) {
    if let Ok(mut g) = GPU.lock() {
        *g = Some(gpu);
    }
    refresh_native();
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
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let first = STATE
        .set(State {
            cfg,
            path,
            tmp: PathBuf::from(tmp),
            start: Instant::now(),
        })
        .is_ok();
    if !first {
        return;
    }
    if let Some(dir) = STATE.get().and_then(|s| s.path.parent()) {
        let _ = std::fs::create_dir_all(dir);
    }
    #[cfg(unix)]
    prepare_paths();
    refresh_native();
    HOOKS.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !crate::ring::catching() {
                write_panic(info);
            }
            previous(info);
        }));
        attach_native();
        start_helpers();
    });
}

/// The program ends in the normal way: the crash file of this run is not needed. Call it
/// before the clean-exit mark is written.
pub fn clean_exit() {
    if let Some(s) = STATE.get() {
        let _ = std::fs::remove_file(&s.path);
        let _ = std::fs::remove_file(&s.tmp);
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
    f.program = s.cfg.program.clone();
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

/// Clears the flag of the hook on every way out of it.
struct HookDone;

impl Drop for HookDone {
    fn drop(&mut self) {
        let _ = IN_HOOK.try_with(|c| c.set(false));
    }
}

fn write_panic(info: &PanicHookInfo<'_>) {
    // A panic while the hook runs must not run it again.
    if IN_HOOK.try_with(|c| c.replace(true)).unwrap_or(true) {
        return;
    }
    let _done = HookDone;
    // The first uncaught panic is the cause. Later ones do not replace its file.
    if PANIC_WRITTEN.swap(true, Ordering::SeqCst) {
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
}

// ---- the native crash path

/// Rebuilds the bytes that the native handler writes: the crash file as compact JSON without its
/// last `}` and without `message`. The handler adds `,"message":"NAME (code N)"}`.
fn refresh_native() {
    let Some(mut f) = base(CrashKind::Native) else {
        return;
    };
    let build = |f: &CrashFile| -> Option<Vec<u8>> {
        let mut v = serde_json::to_value(f.clone().bounded()).ok()?;
        v.as_object_mut()?.remove("message");
        let mut bytes = serde_json::to_vec(&v).ok()?;
        (bytes.pop() == Some(b'}')).then_some(bytes)
    };
    let bytes = loop {
        match build(&f) {
            // Room for the tail (at most about 100 bytes).
            Some(b) if b.len() + 200 <= CAP => break b,
            _ if !f.log.is_empty() => f.log.drain(..f.log.len().div_ceil(2)),
            _ if !f.commands.is_empty() => f.commands.drain(..f.commands.len().div_ceil(2)),
            _ => return,
        };
    };
    let idle = 1 - ACTIVE.load(Ordering::Acquire);
    // SAFETY: only this function writes the idle slot, and the refresher thread is the only
    // caller that runs often; two callers at once would only mix two valid reports of the same
    // process, and `set_gpu` runs before the first crash can happen in practice.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            SLOTS[idle].0.get().cast::<u8>(),
            bytes.len(),
        );
    }
    LENS[idle].store(bytes.len(), Ordering::Release);
    ACTIVE.store(idle, Ordering::Release);
}

/// Writes `,"message":"NAME (code N)"}` into `out` without allocating. Returns the length.
/// `name` has letters, digits and `_` only (a name from a fixed list).
pub fn message_tail(name: &str, code: i64, out: &mut [u8; 160]) -> usize {
    let mut n = 0;
    let mut put = |bytes: &[u8]| {
        for &b in bytes {
            if n < out.len() {
                out[n] = b;
                n += 1;
            }
        }
    };
    put(b",\"message\":\"");
    for &b in name.as_bytes().iter().take(60) {
        put(&[if b.is_ascii_alphanumeric() || b == b'_' {
            b
        } else {
            b'_'
        }]);
    }
    put(b" (code ");
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    let mut v = code.unsigned_abs();
    loop {
        i -= 1;
        digits[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if code < 0 {
        put(b"-");
    }
    put(&digits[i..]);
    put(b")\"}");
    n
}

#[cfg(unix)]
static PATHS: OnceLock<(std::ffi::CString, std::ffi::CString)> = OnceLock::new();

#[cfg(unix)]
fn prepare_paths() {
    use std::os::unix::ffi::OsStrExt;
    if let Some(s) = STATE.get()
        && let (Ok(tmp), Ok(fin)) = (
            std::ffi::CString::new(s.tmp.as_os_str().as_bytes()),
            std::ffi::CString::new(s.path.as_os_str().as_bytes()),
        )
    {
        let _ = PATHS.set((tmp, fin));
    }
}

/// Writes the two byte strings to `tmp` and renames it to `fin`, with `open`, `write` and
/// `rename` only: nothing here allocates, formats or takes a lock. Returns true when all was
/// written.
#[cfg(unix)]
pub fn emit(head: &[u8], tail: &[u8], tmp: &std::ffi::CStr, fin: &std::ffi::CStr) -> bool {
    unsafe fn write_all(fd: i32, mut p: &[u8]) -> bool {
        while !p.is_empty() {
            // SAFETY: the pointer and the length come from a slice.
            let n = unsafe { libc::write(fd, p.as_ptr().cast(), p.len()) };
            if n <= 0 {
                return false;
            }
            p = &p[n as usize..];
        }
        true
    }
    // SAFETY: plain system calls on a valid C string and a file descriptor that we own.
    unsafe {
        let fd = libc::open(
            tmp.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
            0o600 as libc::c_uint,
        );
        if fd < 0 {
            return false;
        }
        let ok = write_all(fd, head) && write_all(fd, tail);
        libc::close(fd);
        ok && libc::rename(tmp.as_ptr(), fin.as_ptr()) == 0
    }
}

fn attach_native() {
    use crash_handler::{CrashContext, CrashEventResult, CrashHandler, make_crash_event};
    // SAFETY: the closure runs in a broken process. It never allocates (Unix) and relies on no
    // state of the program; a hang ends by the timer (Unix) or the watchdog thread (Windows).
    let event = unsafe {
        make_crash_event(|ctx: &CrashContext| {
            IN_NATIVE.store(true, Ordering::SeqCst);
            // `alarm` only sets a timer. SIGALRM kills the process if we hang.
            #[cfg(unix)]
            libc::alarm(5);
            if !PANIC_WRITTEN.load(Ordering::SeqCst) {
                let (name, code) = describe(ctx);
                write_native(name, code);
            }
            // Let the next handler run (the default one ends the process).
            CrashEventResult::Handled(false)
        })
    };
    if let Ok(handler) = CrashHandler::attach(event) {
        // The handler must live as long as the process.
        std::mem::forget(handler);
    }
}

/// Starts the thread that rebuilds the prepared report, and on Windows the watchdog.
fn start_helpers() {
    let _ = std::thread::Builder::new()
        .name("kadakas-crash-refresh".into())
        .spawn(|| {
            let mut stuck = 0u32;
            loop {
                std::thread::sleep(Duration::from_millis(250));
                // The watchdog: the handler has run for 5 seconds and the process is alive.
                if cfg!(windows) && IN_NATIVE.load(Ordering::SeqCst) {
                    stuck += 1;
                    if stuck >= 20 {
                        std::process::exit(HUNG_EXIT_CODE as i32);
                    }
                    continue;
                }
                static TICK: AtomicUsize = AtomicUsize::new(0);
                if TICK.fetch_add(1, Ordering::Relaxed).is_multiple_of(8) {
                    refresh_native();
                }
            }
        });
}

fn write_native(name: &str, code: i64) {
    let mut tail = [0u8; 160];
    let n = message_tail(name, code, &mut tail);
    let idx = ACTIVE.load(Ordering::Acquire);
    let len = LENS[idx].load(Ordering::Acquire);
    if len == 0 {
        return;
    }
    // SAFETY: see `Slot`; `len` bytes of the active slot were written before the flip.
    let head = unsafe { std::slice::from_raw_parts(SLOTS[idx].0.get().cast::<u8>(), len) };
    #[cfg(unix)]
    if let Some((tmp, fin)) = PATHS.get() {
        let _ = emit(head, &tail[..n], tmp, fin);
    }
    #[cfg(not(unix))]
    if let Some(s) = STATE.get() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::File::create(&s.tmp) {
            let ok = f.write_all(head).is_ok() && f.write_all(&tail[..n]).is_ok();
            drop(f);
            if ok {
                let _ = std::fs::rename(&s.tmp, &s.path);
            }
        }
    }
}

/// The name and the code of the signal or exception. No allocation.
#[cfg(target_os = "linux")]
fn describe(ctx: &crash_handler::CrashContext) -> (&'static str, i64) {
    let no = ctx.siginfo.ssi_signo as i32;
    (signal_name(no), i64::from(no))
}

#[cfg(target_os = "macos")]
fn describe(ctx: &crash_handler::CrashContext) -> (&'static str, i64) {
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
            (name, e.code as i64)
        }
        None => ("EXC_UNKNOWN", 0),
    }
}

#[cfg(windows)]
fn describe(ctx: &crash_handler::CrashContext) -> (&'static str, i64) {
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
    (name, i64::from(code))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn describe(_ctx: &crash_handler::CrashContext) -> (&'static str, i64) {
    ("NATIVE_CRASH", 0)
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

    #[test]
    fn the_tail_is_made_without_allocation_and_is_valid_json() {
        let mut out = [0u8; 160];
        let n = message_tail("EXC_BAD_ACCESS", 1, &mut out);
        assert_eq!(&out[..n], br#","message":"EXC_BAD_ACCESS (code 1)"}"#);
        let n = message_tail("X", -42, &mut out);
        assert_eq!(&out[..n], br#","message":"X (code -42)"}"#);
        let n = message_tail("A\"B\\C", 0, &mut out);
        assert_eq!(&out[..n], br#","message":"A_B_C (code 0)"}"#);
        let n = message_tail(&"N".repeat(500), i64::MIN, &mut out);
        assert!(n <= out.len());
        // A name that is too long is cut, and the tail still closes the object.
        assert!(out[..n].ends_with(b")\"}"));
        let n = message_tail("Y", i64::MAX, &mut out);
        assert!(
            std::str::from_utf8(&out[..n])
                .unwrap()
                .contains("9223372036854775807")
        );
    }

    #[cfg(unix)]
    #[test]
    fn emit_writes_the_head_and_the_tail_and_renames() {
        use std::ffi::CString;
        let d = std::env::temp_dir().join(format!("kadakas-emit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let tmp = CString::new(d.join("a.tmp").to_str().unwrap()).unwrap();
        let fin = CString::new(d.join("a.json").to_str().unwrap()).unwrap();
        assert!(emit(
            b"{\"format\":1",
            b",\"message\":\"M (code 1)\"}",
            &tmp,
            &fin
        ));
        let text = std::fs::read(d.join("a.json")).unwrap();
        let f = CrashFile::parse(&text).unwrap();
        assert_eq!(f.message, "M (code 1)");
        assert!(!d.join("a.tmp").exists());
        // A path in a folder that does not exist fails without a panic.
        let bad = CString::new(d.join("no/such/x").to_str().unwrap()).unwrap();
        assert!(!emit(b"x", b"y", &bad, &fin));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
