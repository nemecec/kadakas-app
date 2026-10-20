//! The signature of a crash: the same crash in the same version gets the same signature, on
//! every computer, so we can count it and fix the cause.
//!
//! It is the version and the top frames of the backtrace, after the frames of the panic
//! machinery and the runtime are skipped. What goes into it: the function names without the
//! compiler hash (`::h0123456789abcdef`). What does not: addresses, files, line numbers (they
//! move with every edit of a file, but the function stays), and the message (it may hold text of
//! the person).

use crate::file::{CrashFile, CrashKind, Frame};
use crate::scrub::strip_hash;
use sha2::{Digest, Sha256};

/// How many frames count.
pub const TOP_FRAMES: usize = 5;

/// Frames that are the panic machinery or the runtime, not the place of the bug.
const SKIP_PREFIXES: [&str; 27] = [
    "<panic location>",
    "crash_handler",
    "crash_context",
    "__restore_rt",
    "_sigtramp",
    "std::panicking",
    "std::panic",
    "core::panicking",
    "core::panic",
    "rust_begin_unwind",
    "rust_panic",
    "rust_panic_with_hook",
    "__rust_",
    "_rust_",
    "std::backtrace",
    "std::rt::",
    "std::sys",
    "std::thread",
    "kadakas_crash::",
    "backtrace::",
    "__pthread",
    "_pthread",
    "thread_start",
    "start_thread",
    "_start",
    "<unknown>",
    "__libc",
];

/// True for a frame that is not the place of the bug.
fn skipped(function: &str) -> bool {
    let f = function.trim_start_matches('<');
    SKIP_PREFIXES.iter().any(|p| function.starts_with(p) || f.starts_with(p))
        // The hook closure and the lang item that call it.
        || function.contains("{{closure}}") && function.contains("panicking")
        || function == "main"
        || function == "__main"
        || function == "start"
}

/// The name of a frame as the signature sees it.
pub fn frame_key(f: &Frame) -> String {
    strip_hash(f.function.trim()).to_string()
}

/// The signature of a crash file: 16 hexadecimal digits.
pub fn of(file: &CrashFile) -> String {
    let mut h = Sha256::new();
    h.update(b"kadakas-crash-1|");
    h.update(file.version.trim().as_bytes());
    h.update(b"|");
    h.update(file.kind.id().as_bytes());
    let keys: Vec<String> = file
        .frames
        .iter()
        .map(frame_key)
        .filter(|k| !k.is_empty() && !skipped(k))
        .take(TOP_FRAMES)
        .collect();
    if keys.is_empty() {
        // Without frames (a native crash, or a stripped binary) the kind of the crash is what
        // we have: the first word of the message (`SIGSEGV`, `EXCEPTION_ACCESS_VIOLATION`).
        h.update(b"|none|");
        let word = file
            .message
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .find(|w| !w.is_empty() && w.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
            .unwrap_or("");
        if file.kind == CrashKind::Native {
            h.update(word.as_bytes());
        }
    }
    for k in &keys {
        h.update(b"|");
        h.update(k.as_bytes());
    }
    let d = h.finalize();
    d[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fr(f: &str) -> Frame {
        Frame {
            function: f.into(),
            file: Some("/Users/x/a.rs".into()),
            line: Some(10),
        }
    }

    fn file(version: &str, frames: &[&str]) -> CrashFile {
        let mut c = CrashFile::new(CrashKind::Panic);
        c.version = version.into();
        c.message = "boom".into();
        c.frames = frames.iter().map(|f| fr(f)).collect();
        c
    }

    const STACK: [&str; 8] = [
        "std::backtrace_rs::backtrace::libunwind::trace::h0123456789abcdef",
        "std::panicking::begin_panic_handler::{{closure}}::h1111111111111111",
        "rust_begin_unwind",
        "core::panicking::panic_fmt::h2222222222222222",
        "kadakas_core::geom::offset::h3333333333333333",
        "kadakas_api::session::exec::h4444444444444444",
        "kadakas_app::app::App::update::h5555555555555555",
        "std::rt::lang_start::h6666666666666666",
    ];

    #[test]
    fn the_signature_is_stable_and_16_hex_digits() {
        let a = of(&file("0.3.1", &STACK));
        assert_eq!(a.len(), 16);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(a, of(&file("0.3.1", &STACK)));
    }

    #[test]
    fn hashes_files_lines_messages_and_threads_do_not_change_it() {
        let base = of(&file("0.3.1", &STACK));
        let mut other = file("0.3.1", &STACK);
        for (i, f) in other.frames.iter_mut().enumerate() {
            if let Some(at) = f.function.rfind("::h") {
                f.function = format!("{}::hfedcba9876543210", &f.function[..at]);
            }
            f.file = Some(format!("/other/place/{i}.rs"));
            f.line = Some(999);
        }
        other.message = "a different message with Oak trees".into();
        other.thread = "worker-3".into();
        other.uptime_secs = 12345;
        other.os = "linux x86_64".into();
        assert_eq!(base, of(&other));
    }

    #[test]
    fn the_version_and_the_place_change_it() {
        let a = of(&file("0.3.1", &STACK));
        assert_ne!(a, of(&file("0.3.2", &STACK)));
        let mut moved = STACK;
        moved[4] = "kadakas_core::geom::other::h3333333333333333";
        assert_ne!(a, of(&file("0.3.1", &moved)));
    }

    #[test]
    fn only_the_top_frames_count() {
        let ours: Vec<String> = (0..8).map(|i| format!("kadakas_core::f{i}")).collect();
        let refs: Vec<&str> = ours.iter().map(String::as_str).collect();
        let a = of(&file("1", &refs));
        let mut tail = refs.clone();
        tail[5] = "changed::tail";
        tail.push("one::more");
        assert_eq!(
            a,
            of(&file("1", &tail)),
            "the sixth frame and later do not count"
        );
        let mut head = refs.clone();
        head[4] = "changed::fifth";
        assert_ne!(a, of(&file("1", &head)), "the fifth frame counts");
    }

    #[test]
    fn the_panic_machinery_is_skipped() {
        let with = of(&file("1", &STACK));
        let without = of(&file("1", &STACK[4..]));
        assert_eq!(with, without);
    }

    #[test]
    fn native_crashes_group_by_kind_and_version() {
        let mk = |msg: &str, v: &str| {
            let mut c = CrashFile::new(CrashKind::Native);
            c.version = v.into();
            c.message = msg.into();
            c
        };
        let a = of(&mk("SIGSEGV at address 0x10", "1"));
        assert_eq!(a, of(&mk("SIGSEGV at address 0x2000", "1")));
        assert_ne!(a, of(&mk("SIGABRT", "1")));
        assert_ne!(a, of(&mk("SIGSEGV", "2")));
    }

    #[test]
    fn a_panic_without_frames_does_not_depend_on_the_message() {
        let mut a = CrashFile::new(CrashKind::Panic);
        a.version = "1".into();
        a.message = "Oak".into();
        let mut b = a.clone();
        b.message = "Pine".into();
        assert_eq!(of(&a), of(&b));
    }
}
