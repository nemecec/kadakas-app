//! A panic that the IFC import catches is no crash (IOP-29): it writes no crash file, it does not
//! use up the "first panic" mark of the capture, and a real crash later is still written.
//!
//! Like `kadakas-crash/tests/capture_child.rs`, the test starts itself again with a variable that
//! tells the child test to install the capture.

use std::path::{Path, PathBuf};
use std::process::Command;

use kadakas_crash::capture::{self, Config};
use kadakas_crash::queue;
use kadakas_ifc::{IfcImportError, run_guarded};

const ROOT: &str = "KADAKAS_IFC_CRASH_TEST_ROOT";
const RUN: &str = "5e6f-7a8b";

fn child_setup() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os(ROOT)?);
    capture::install(Config {
        root: root.clone(),
        run_id: RUN.into(),
        program: "kadakas-test".into(),
        version: "9.9.9".into(),
        channel: "nightly".into(),
        os: capture::default_os(),
    });
    Some(root)
}

#[test]
fn child_catches_an_import_panic_and_then_crashes_for_real() {
    let Some(root) = child_setup() else { return };
    let r = run_guarded(|| panic!("a crafted file"));
    let Err(IfcImportError::Internal(detail)) = r else {
        panic!("{r:?}")
    };
    // The text has the message and the place, for the bug report.
    assert!(detail.starts_with("a crafted file at "), "{detail}");
    assert!(detail.contains("ifc_crash.rs"), "{detail}");
    assert!(
        !queue::live_path(&root, RUN).exists(),
        "a caught import panic is no crash"
    );
    // A panic that nobody catches still writes the file: the mark of the first panic is unset.
    let t = std::thread::spawn(|| panic!("a worker died"));
    assert!(t.join().is_err());
    let f = queue::read_live(&root, RUN).expect("the real panic is written");
    assert_eq!(f.message, "a worker died");
    capture::clean_exit();
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kadakas-ifc-crash-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run_child(name: &str, root: &Path) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(ROOT, root)
        .env("RUST_BACKTRACE", "0")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
}

#[test]
fn a_caught_import_panic_leaves_the_crash_capture_free_for_a_real_crash() {
    let root = tmp("caught");
    let status = run_child(
        "child_catches_an_import_panic_and_then_crashes_for_real",
        &root,
    );
    assert!(status.success());
    let _ = std::fs::remove_dir_all(&root);
}
