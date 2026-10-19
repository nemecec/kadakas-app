//! A real crash in a child process produces a crash file (PLAT-20).
//!
//! The test starts itself again (`current_exe`) with a variable that tells the child test to
//! install the capture and then die: with a panic, or with a null write (a signal on Unix, an
//! exception on Windows). The parent reads the crash file that the child left.

use kadakas_crash::capture::{self, Config};
use kadakas_crash::file::{CrashFile, CrashKind, Gpu};
use kadakas_crash::report::assemble;
use kadakas_crash::scrub::Context;
use kadakas_crash::{queue, ring};
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = "KADAKAS_CRASH_TEST_ROOT";
const RUN: &str = "1a2b-3c4d";

fn child_setup() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os(ROOT)?);
    capture::install(Config {
        root: root.clone(),
        run_id: RUN.into(),
        version: "9.9.9".into(),
        channel: "nightly".into(),
        os: capture::default_os(),
    });
    capture::set_gpu(Gpu {
        adapter: "Test adapter".into(),
        driver: "Test driver 1".into(),
        backend: "Metal".into(),
    });
    ring::note_log("opened /Users/someone/Documents/Secret Plan.kad");
    ring::note_command("file.open");
    ring::note_command("edit.move");
    Some(root)
}

#[test]
fn child_panics() {
    if child_setup().is_none() {
        return;
    }
    panic!("boom in /Users/someone/Documents/Secret Plan.kad");
}

#[test]
fn child_survives_a_caught_panic_and_exits_clean() {
    let Some(root) = child_setup() else { return };
    {
        let _g = ring::CatchGuard::new();
        let r = std::panic::catch_unwind(|| panic!("caught on purpose"));
        assert!(r.is_err());
    }
    assert!(
        !queue::live_path(&root, RUN).exists(),
        "a caught panic is no crash"
    );
    // A panic that nobody catches writes the file; a clean exit takes it away again.
    let t = std::thread::spawn(|| panic!("a worker died"));
    assert!(t.join().is_err());
    assert!(queue::live_path(&root, RUN).exists());
    capture::clean_exit();
    assert!(!queue::live_path(&root, RUN).exists());
}

#[test]
fn child_segfaults() {
    if child_setup().is_none() {
        return;
    }
    // SAFETY: this is a crash on purpose, in a child process of the test.
    unsafe {
        std::ptr::null_mut::<u8>().write_volatile(1);
    }
}

fn run_child(name: &str, root: &Path) -> std::process::ExitStatus {
    let exe = std::env::current_exe().unwrap();
    Command::new(exe)
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(ROOT, root)
        .env("RUST_BACKTRACE", "0")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kadakas-crash-child-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_real_panic_in_a_child_leaves_a_crash_file() {
    let root = tmp("panic");
    let status = run_child("child_panics", &root);
    assert!(!status.success());
    let f = queue::read_live(&root, RUN).expect("the child wrote a crash file");
    assert_eq!(f.kind, CrashKind::Panic);
    assert_eq!(f.version, "9.9.9");
    assert_eq!(f.channel, "nightly");
    assert_eq!(f.run_id, RUN);
    assert!(f.message.starts_with("boom in "), "{}", f.message);
    assert!(f.thread.contains("child_panics"), "{}", f.thread);
    assert_eq!(f.gpu.as_ref().unwrap().adapter, "Test adapter");
    assert_eq!(f.commands, vec!["file.open", "edit.move"]);
    assert_eq!(f.log.len(), 1);
    assert!(f.created > 1_600_000_000);
    assert!(f.memory_peak_mib.is_some() || cfg!(not(unix)));
    // The panic place is the first frame, and the backtrace holds the test function.
    assert_eq!(f.frames[0].function, "<panic location>");
    assert!(
        f.frames[0]
            .file
            .as_deref()
            .unwrap()
            .ends_with("capture_child.rs"),
        "{:?}",
        f.frames[0]
    );
    assert!(f.frames.len() > 3);
    assert!(
        f.frames
            .iter()
            .any(|fr| fr.function.contains("child_panics")),
        "{:?}",
        f.frames.iter().map(|f| &f.function).collect::<Vec<_>>()
    );
    // The launcher's step: keep the file for the next start. Then the report is scrubbed.
    assert!(queue::keep(&root, RUN).unwrap());
    let kept = queue::list(&root);
    assert_eq!(kept.len(), 1);
    let a = assemble(&kept[0].file, &Context::new("someone", "/Users/someone"));
    let text = serde_json::to_string(&a.crash).unwrap() + &a.log + &a.commands;
    assert!(!text.contains("Secret"), "{text}");
    assert!(!text.contains("someone"), "{text}");
    assert_eq!(
        a.crash["frames"][0]["file"],
        "crates/kadakas-crash/tests/capture_child.rs"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_caught_panic_is_no_crash_and_a_clean_exit_removes_the_file() {
    let root = tmp("caught");
    let status = run_child("child_survives_a_caught_panic_and_exits_clean", &root);
    assert!(status.success());
    assert!(queue::read_live(&root, RUN).is_none());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_real_native_crash_in_a_child_leaves_a_crash_file() {
    let root = tmp("native");
    let status = run_child("child_segfaults", &root);
    assert!(!status.success());
    let f: CrashFile = queue::read_live(&root, RUN).expect("the child wrote a crash file");
    assert_eq!(f.kind, CrashKind::Native);
    assert_eq!(f.version, "9.9.9");
    let expect = if cfg!(target_os = "macos") {
        "EXC_BAD_ACCESS"
    } else if cfg!(windows) {
        "EXCEPTION_ACCESS_VIOLATION"
    } else {
        "SIGSEGV"
    };
    assert!(f.message.starts_with(expect), "{}", f.message);
    assert_eq!(f.commands, vec!["file.open", "edit.move"]);
    assert!(!f.log.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}
