//! Builds the script module: QuickJS (quickjs-ng, vendored in `quickjs/`) and the shim in `c/` to
//! one WebAssembly module in `OUT_DIR`. Only with the cargo feature `engine`.
//!
//! Why a build script and not a prebuilt file: the module is made from text that people can
//! review (the vendored source, `c/shim.c`, `c/prelude.js`), with one pinned toolchain, and no
//! binary is committed. The build script runs only when the source, the toolchain or one of the
//! environment variables below changes (`rerun-if-changed`, `rerun-if-env-changed`). Cargo caches
//! the result in `target/`, and CI caches `target/` by the hash of the source and the toolchain.
//!
//! No download happens here. The toolchain is found in this order:
//!
//! 1. `WASI_SDK_PATH`: a wasi-sdk of the pinned version (`wasi-sdk.env`). A wrong version is an
//!    error that says how to install the right one.
//! 2. The default install folder of `scripts/install-wasi-sdk.sh` for the pinned version.
//! 3. `/opt/wasi-sdk` if it has the pinned version.
//! 4. Homebrew parts (`brew install llvm lld wasi-libc wasi-runtimes`), or the variables
//!    `WASI_CLANG`, `WASI_WASM_LD` and `WASI_SYSROOT`. The result may differ byte for byte from a
//!    build with the pinned wasi-sdk (which CI and releases use).
//!
//! **A cache for CI.** If `KADAKAS_JS_CACHE_DIR` names a folder, the finished module is kept there
//! as `script-<hash>.wasm`, where the hash covers the source, the flags and the pinned wasi-sdk
//! version, and a build with the same hash takes the module from there without a toolchain. CI
//! caches that folder (`.github/actions/wasi-sdk`). A module that a Homebrew toolchain made is
//! never put in it.
//!
//! The finished module may import only `kadakas.call` and `kadakas.read`. The build fails if it
//! imports anything else (WASI, a clock), so the sandbox has no way out.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCES: [&str; 5] = [
    "c/shim.c",
    "quickjs/quickjs.c",
    "quickjs/dtoa.c",
    "quickjs/libregexp.c",
    "quickjs/libunicode.c",
];

/// The flags of the compile and link. A change here is a change of the module.
const FLAGS: [&str; 10] = [
    "--target=wasm32-wasip1",
    "-O2",
    "-DNDEBUG",
    "-D_GNU_SOURCE",
    "-fno-exceptions",
    "-mexec-model=reactor",
    "-Wl,-z,stack-size=1048576",
    // The stack below the data: an overflow runs out of memory and traps, and does not write over
    // the data of the module.
    "-Wl,--stack-first",
    "-Wl,--export-memory",
    "-w",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wasi-sdk.env");
    println!("cargo:rerun-if-changed=c");
    println!("cargo:rerun-if-changed=quickjs");
    for var in [
        "WASI_SDK_PATH",
        "WASI_CLANG",
        "WASI_WASM_LD",
        "WASI_SYSROOT",
        "KADAKAS_JS_CACHE_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    if env::var_os("CARGO_FEATURE_ENGINE").is_none() {
        println!("cargo:rustc-env=KADAKAS_JS_TOOLCHAIN=not built (feature engine is off)");
        return;
    }
    if let Err(message) = build() {
        // `panic!` would print a backtrace note; a plain message is clearer.
        eprintln!("\nerror: the script engine cannot be built.\n\n{message}\n");
        std::process::exit(1);
    }
}

struct Pinned {
    version: String,
    tag: String,
}

fn read_pins() -> Pinned {
    let text = fs::read_to_string("wasi-sdk.env").expect("wasi-sdk.env is in the crate");
    let mut version = String::new();
    let mut tag = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "WASI_SDK_VERSION" => version = v.to_string(),
                "WASI_SDK_TAG" => tag = v.to_string(),
                _ => {}
            }
        }
    }
    Pinned { version, tag }
}

/// A toolchain that can compile C to wasm32.
struct Toolchain {
    /// Names the route and the version, without paths: `wasi-sdk 34.0`.
    id: String,
    clang: PathBuf,
    /// Extra clang arguments (sysroot, linker).
    args: Vec<String>,
    /// What the build prints and the module reports: the route and the version.
    label: String,
}

fn install_help(pins: &Pinned) -> String {
    let (v, tag) = (&pins.version, &pins.tag);
    [
        format!("The script engine needs the C compiler of wasi-sdk {v} (it builds QuickJS to WebAssembly)."),
        "Install it once:".to_string(),
        String::new(),
        "  macOS or Linux:  run scripts/install-wasi-sdk.sh".to_string(),
        format!("    It downloads wasi-sdk {v} ({tag}), checks its SHA-256 and installs it to"),
        "    ~/.local/share/wasi-sdk/".to_string() + v + ". Then set WASI_SDK_PATH to that folder",
        "    (the script prints the line).".to_string(),
        "  Windows: download wasi-sdk-".to_string() + v + "-x86_64-windows.tar.gz (or arm64) from",
        format!("    https://github.com/WebAssembly/wasi-sdk/releases/tag/{tag} , check its SHA-256 against"),
        "    crates/kadakas-script-js/wasi-sdk.env, unpack it, and set WASI_SDK_PATH to the folder.".to_string(),
        "  Homebrew instead (the result may differ byte for byte from the pinned build):".to_string(),
        "    brew install llvm lld wasi-libc wasi-runtimes".to_string(),
        String::new(),
        "To work without the engine, build without the feature: cargo build -p kadakas-api --no-default-features".to_string(),
    ]
    .join("\n")
}

fn sdk_version(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("VERSION")).ok()?;
    Some(text.lines().next()?.trim().to_string())
}

fn sdk_toolchain(dir: &Path, pins: &Pinned) -> Result<Toolchain, String> {
    println!("cargo:rerun-if-changed={}", dir.join("VERSION").display());
    let found = sdk_version(dir).ok_or_else(|| {
        format!(
            "{} is not a wasi-sdk (it has no VERSION file).\n\n{}",
            dir.display(),
            install_help(pins)
        )
    })?;
    if found != pins.version {
        return Err(format!(
            "WASI_SDK_PATH={} is wasi-sdk {found}, but this source needs wasi-sdk {}.\n\n{}",
            dir.display(),
            pins.version,
            install_help(pins)
        ));
    }
    let exe = if cfg!(windows) { "clang.exe" } else { "clang" };
    Ok(Toolchain {
        id: format!("wasi-sdk {found}"),
        clang: dir.join("bin").join(exe),
        args: vec![format!(
            "--sysroot={}",
            dir.join("share").join("wasi-sysroot").display()
        )],
        label: format!("wasi-sdk {found} ({})", dir.display()),
    })
}

fn brew_prefix(formula: &str) -> Option<PathBuf> {
    let out = Command::new("brew")
        .args(["--prefix", formula])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    p.is_dir().then_some(p)
}

fn homebrew_toolchain() -> Option<Toolchain> {
    let env_path = |k: &str| env::var_os(k).map(PathBuf::from);
    let clang =
        env_path("WASI_CLANG").or_else(|| brew_prefix("llvm").map(|p| p.join("bin/clang")))?;
    let wasm_ld =
        env_path("WASI_WASM_LD").or_else(|| brew_prefix("lld").map(|p| p.join("bin/wasm-ld")))?;
    let sysroot = env_path("WASI_SYSROOT")
        .or_else(|| brew_prefix("wasi-libc").map(|p| p.join("share/wasi-sysroot")))?;
    if !clang.is_file() || !wasm_ld.is_file() || !sysroot.is_dir() {
        return None;
    }
    let version = Command::new(&clang)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|t| t.lines().next().map(str::to_string))
        .unwrap_or_default();
    let mut args = vec![
        format!("--sysroot={}", sysroot.display()),
        format!("-fuse-ld={}", wasm_ld.display()),
    ];
    // The builtins of the compiler (for 128 bit arithmetic) come with wasi-runtimes.
    if let Some(rt) = brew_prefix("wasi-runtimes") {
        let lib = rt.join("share/wasi-runtimes/lib/wasm32-unknown-wasip1");
        if lib.join("libclang_rt.builtins.a").is_file() {
            args.push(format!("-L{}", lib.display()));
            args.push("-lclang_rt.builtins".to_string());
        }
    }
    Some(Toolchain {
        id: format!("homebrew {version}"),
        clang,
        args,
        label: format!("Homebrew llvm/lld/wasi-libc ({version})"),
    })
}

fn home() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn find_toolchain(pins: &Pinned) -> Result<Toolchain, String> {
    if let Some(dir) = env::var_os("WASI_SDK_PATH").filter(|v| !v.is_empty()) {
        return sdk_toolchain(Path::new(&dir), pins);
    }
    let mut defaults = Vec::new();
    if let Some(data) = env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        defaults.push(data.join("wasi-sdk").join(&pins.version));
    }
    if let Some(h) = home() {
        defaults.push(h.join(".local/share/wasi-sdk").join(&pins.version));
    }
    if let Some(local) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        defaults.push(local.join("wasi-sdk").join(&pins.version));
    }
    defaults.push(PathBuf::from("/opt/wasi-sdk"));
    for dir in defaults {
        if sdk_version(&dir).as_deref() == Some(pins.version.as_str()) {
            return sdk_toolchain(&dir, pins);
        }
    }
    if let Some(t) = homebrew_toolchain() {
        return Ok(t);
    }
    Err(format!(
        "No wasi-sdk {} was found (WASI_SDK_PATH is not set, and the default folders have none).\n\n{}",
        pins.version,
        install_help(pins)
    ))
}

/// The SHA-256 of everything that decides the module: the toolchain id, the flags, the vendored
/// source, the shim and the prelude.
fn input_hash(toolchain_id: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(toolchain_id.as_bytes());
    for f in FLAGS
        .iter()
        .chain(SOURCES.iter())
        .chain(["-lm", "-Iquickjs"].iter())
    {
        h.update(f.as_bytes());
        h.update([0]);
    }
    // This script too: it decides the steps of the build.
    h.update(fs::read("build.rs").map_err(|e| format!("build.rs: {e}"))?);
    for dir in ["c", "quickjs"] {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|e| format!("{dir}: {e}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        for f in files {
            h.update(f.to_string_lossy().as_bytes());
            h.update(fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?);
        }
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn build() -> Result<(), String> {
    let pins = read_pins();
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?);
    let wasm = out_dir.join("script.wasm");
    // The cache of CI: the module for the pinned toolchain and this exact source.
    let cache = env::var_os("KADAKAS_JS_CACHE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .map(|dir| {
            input_hash(&format!("wasi-sdk {}", pins.version))
                .map(|h| dir.join(format!("script-{h}.wasm")))
        })
        .transpose()?;
    if let Some(cached) = cache.as_ref().filter(|c| c.is_file()) {
        let bytes = fs::read(cached).map_err(|e| e.to_string())?;
        if check_module(&bytes).is_ok() {
            fs::write(&wasm, &bytes).map_err(|e| e.to_string())?;
            println!(
                "cargo:warning=script engine: QuickJS module taken from the cache {}",
                cached.display()
            );
            println!(
                "cargo:rustc-env=KADAKAS_JS_TOOLCHAIN=wasi-sdk {} (module from the build cache)",
                pins.version
            );
            println!("cargo:rustc-env=KADAKAS_JS_WASM={}", wasm.display());
            return Ok(());
        }
    }
    let tc = find_toolchain(&pins)?;

    // The prelude as a C array (bytes, so quotes and newlines need no care).
    let prelude = fs::read("c/prelude.js").map_err(|e| format!("c/prelude.js: {e}"))?;
    let mut header = String::from("static const char PRELUDE[] = {");
    for (i, b) in prelude.iter().enumerate() {
        if i % 16 == 0 {
            header.push('\n');
        }
        header.push_str(&format!("{b},"));
    }
    header.push_str("0};\n");
    fs::write(out_dir.join("prelude.h"), header).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(&tc.clang);
    cmd.args(FLAGS)
        .args(&tc.args)
        .arg("-Iquickjs")
        .arg(format!("-I{}", out_dir.display()))
        .arg("-o")
        .arg(&wasm)
        .args(SOURCES)
        .arg("-lm");
    let started = std::time::Instant::now();
    let output = cmd.output().map_err(|e| {
        format!(
            "cannot run {}: {e}\n\n{}",
            tc.clang.display(),
            install_help(&pins)
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "{} failed:\n{}{}\n\n{}",
            tc.clang.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            install_help(&pins)
        ));
    }
    let bytes = fs::read(&wasm).map_err(|e| e.to_string())?;
    check_module(&bytes)?;
    println!(
        "cargo:warning=script engine: QuickJS built with {} in {:.0} s, {} bytes",
        tc.label,
        started.elapsed().as_secs_f64(),
        bytes.len()
    );
    // Only a module of the pinned toolchain goes into the cache.
    if let Some(cached) = cache
        && tc.id == format!("wasi-sdk {}", pins.version)
        && let Some(dir) = cached.parent()
        && fs::create_dir_all(dir).is_ok()
    {
        let _ = fs::write(&cached, &bytes);
    }
    println!("cargo:rustc-env=KADAKAS_JS_TOOLCHAIN={}", tc.label);
    println!("cargo:rustc-env=KADAKAS_JS_WASM={}", wasm.display());
    Ok(())
}

/// The module imports `kadakas.call` and `kadakas.read` and nothing else, and has the exports.
fn check_module(bytes: &[u8]) -> Result<(), String> {
    use wasmparser::{Parser, Payload};
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|e| format!("the module is not valid: {e}"))? {
            Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    let i = i.map_err(|e| e.to_string())?;
                    imports.push(format!("{}.{}", i.module, i.name));
                }
            }
            Payload::ExportSection(r) => {
                for e in r {
                    exports.push(e.map_err(|e| e.to_string())?.name.to_string());
                }
            }
            _ => {}
        }
    }
    let bad: Vec<&String> = imports
        .iter()
        .filter(|i| *i != "kadakas.call" && *i != "kadakas.read")
        .collect();
    if !bad.is_empty() {
        return Err(format!(
            "the module imports {bad:?}. It may import only kadakas.call and kadakas.read (no files, no network, no clock). Define the missing system function in c/shim.c."
        ));
    }
    for need in ["memory", "alloc", "dealloc", "run"] {
        if !exports.iter().any(|e| e == need) {
            return Err(format!("the module does not export '{need}'"));
        }
    }
    Ok(())
}
