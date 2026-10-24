//! Helper programs for the tests, the same on every system: a shell script would not run on
//! Windows. The tests compile a small Rust program with the `rustc` of the toolchain that built
//! them (`CARGO` is the cargo of that toolchain: the compiler is next to it).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Compiles the Rust source `src` into the program `out` (with the `.exe` ending where it is
/// needed). Panics when the compiler fails.
pub(crate) fn compile(src: &Path, out: &Path) {
    let rustc =
        Path::new(env!("CARGO")).with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let o = Command::new(&rustc)
        .args([
            "--edition",
            "2021",
            "--crate-name",
            "stub",
            "-C",
            "debuginfo=0",
            "-o",
        ])
        .arg(out)
        .arg(src)
        .output()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", rustc.display()));
    assert!(
        o.status.success(),
        "the test program {} did not compile: {}",
        src.display(),
        String::from_utf8_lossy(&o.stderr)
    );
}

/// The script tool (see `testtool_main.rs`). It is compiled once and kept in the temp folder under
/// a name that holds a hash of its source, so the next run (and a parallel test process) reuses it.
pub(crate) fn tool() -> &'static Path {
    use std::hash::{Hash, Hasher};
    static TOOL: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    TOOL.get_or_init(|| {
        let source = include_str!("testtool_main.rs");
        let mut h = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut h);
        let dir = std::env::temp_dir().join(format!("kadakas-script-tool-{:016x}", h.finish()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = |n: &str| format!("{n}{}", std::env::consts::EXE_SUFFIX);
        let out = dir.join(exe("tool"));
        if !out.is_file() {
            // Build under a name of this process, then move it: no process sees a half-written file.
            let mine = format!("{}", std::process::id());
            let src = dir.join(format!("tool-{mine}.rs"));
            std::fs::write(&src, source).unwrap();
            let tmp = dir.join(exe(&format!("tool-{mine}")));
            compile(&src, &tmp);
            let moved = std::fs::rename(&tmp, &out);
            assert!(moved.is_ok() || out.is_file(), "{moved:?}");
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::remove_file(&src);
        }
        out
    })
}

/// A command that runs the one-line-per-command script `script` with the tool.
pub(crate) fn command(script: &str) -> Command {
    let mut c = Command::new(tool());
    c.arg("-c").arg(script);
    c
}
