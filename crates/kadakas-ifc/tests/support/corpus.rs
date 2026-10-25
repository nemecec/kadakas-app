//! Public sample IFC files, fetched at run time (IOP-29).
//!
//! The files are NOT in the repository. A test asks for a file by repository key and path. The
//! helper downloads it with `curl` from a fixed commit into `tests/data/ifc-public/` (a folder that
//! git ignores) and reads it from there the next time. Every file is checked against the SHA-256 in
//! `tests/fixtures/ifc_public_sha256.json`, which `tests/data/make_ifc_public_hashes.py` writes.
//!
//! When there is no network (or no `curl`), `need` returns `None` and the test prints `SKIP` and
//! ends. A missing network never fails a test. A network that answers with an HTTP error does: the
//! pinned list is wrong. Variables: `KADAKAS_CORPUS=off` fetches nothing. In CI (the variable `CI` is
//! set) nothing is fetched either, unless `KADAKAS_CORPUS=fetch` opts in. Files in the cache are
//! always used. A file that is a Git LFS pointer (about 130 bytes) is skipped with a message.
//!
//! Sources (all Creative Commons Attribution 4.0 International, see `docs/attribution.md`):
//! - `bsi-cert`: https://github.com/buildingSMART/Certification-datasets (buildingSMART International).
//! - `bsi-community`: https://github.com/buildingsmart-community/Community-Sample-Test-Files (the
//!   authors named in the folders). Its IFC files are in Git LFS and come from the media server.
//!
//! Files with an unclear licence (Schependomlaan, Duplex, attachments of issues) are never listed.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// The folder of the downloaded files (ignored by git).
pub fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/ifc-public")
}

static OFFLINE: AtomicBool = AtomicBool::new(false);

fn table() -> &'static serde_json::Value {
    static T: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str(include_str!("../fixtures/ifc_public_sha256.json")).unwrap()
    })
}

/// True when no file is fetched from the network: after a network failure, with
/// `KADAKAS_CORPUS=off`, and in CI (the variable `CI` is set) unless `KADAKAS_CORPUS=fetch` opts in.
/// Files that are in the cache are used all the same.
pub fn disabled() -> bool {
    if OFFLINE.load(Ordering::Relaxed) {
        return true;
    }
    match std::env::var("KADAKAS_CORPUS") {
        Ok(v) if v.eq_ignore_ascii_case("off") => true,
        Ok(v) if v.eq_ignore_ascii_case("fetch") => false,
        _ => std::env::var_os("CI").is_some(),
    }
}

/// The key of a file in the table: `<repo>:<path>`.
fn key(repo: &str, path: &str) -> String {
    format!("{repo}:{path}")
}

fn entry(repo: &str, path: &str) -> &'static serde_json::Value {
    let k = key(repo, path);
    let e = &table()["files"][&k];
    assert!(
        e.is_object(),
        "{k} is not in fixtures/ifc_public_sha256.json"
    );
    e
}

fn local(repo: &str, path: &str) -> PathBuf {
    let commit = table()["repos"][repo]["commit"]
        .as_str()
        .unwrap_or_else(|| panic!("unknown repository {repo}"));
    cache_dir().join(repo).join(&commit[..8]).join(path)
}

/// Percent-encodes a path for a URL (spaces and anything outside the safe set).
fn encode(path: &str) -> String {
    let mut out = String::new();
    for b in path.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b'('
            | b')' => out.push(b as char),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url(repo: &str, path: &str, lfs: bool) -> String {
    let slug = table()["repos"][repo]["slug"].as_str().unwrap();
    let commit = table()["repos"][repo]["commit"].as_str().unwrap();
    let host = if lfs {
        "media.githubusercontent.com/media"
    } else {
        "raw.githubusercontent.com"
    };
    format!("https://{host}/{slug}/{commit}/{}", encode(path))
}

fn is_lfs_pointer(bytes: &[u8]) -> bool {
    bytes.starts_with(b"version https://git-lfs") && bytes.len() < 1024
}

fn checked(repo: &str, path: &str, bytes: &[u8]) {
    let want = entry(repo, path)["sha256"].as_str().unwrap();
    let got = kadakas_core::picture::sha256_hex(bytes);
    assert_eq!(
        got, want,
        "{repo} {path}: the file does not have the SHA-256 of the pinned commit (delete it from tests/data/ifc-public to fetch it again)"
    );
}

/// Why a file is not there.
fn skip(repo: &str, path: &str, why: &str) -> Option<PathBuf> {
    eprintln!("SKIP: {repo} {path} is not available ({why})");
    None
}

/// The path of a file of the corpus on disk, or `None` when it is not there (offline, disabled, or a
/// Git LFS pointer). A file that the server does not have, or an answer with an HTTP error while the
/// network works, FAILS the test: the pinned list is wrong. So does a file with a wrong hash.
pub fn need(repo: &str, path: &str) -> Option<PathBuf> {
    let file = local(repo, path);
    if let Ok(bytes) = std::fs::read(&file) {
        if is_lfs_pointer(&bytes) {
            return skip(repo, path, "it is a Git LFS pointer, not the file");
        }
        checked(repo, path, &bytes);
        return Some(file);
    }
    if disabled() {
        return skip(
            repo,
            path,
            "offline, KADAKAS_CORPUS=off, or CI without KADAKAS_CORPUS=fetch",
        );
    }
    let lfs = entry(repo, path)["lfs"].as_bool().unwrap_or(false);
    std::fs::create_dir_all(file.parent()?).ok()?;
    let u = url(repo, path, lfs);
    // A name of its own for each download: tests run in threads of one process, and in several processes.
    let part = file.with_extension(format!(
        "part-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let out = Command::new("curl")
        .args(["-fsSL", "--connect-timeout", "8", "-m", "120", "-o"])
        .arg(&part)
        .arg(&u)
        .output();
    match out {
        Ok(out) if out.status.success() => {
            let bytes = std::fs::read(&part).ok();
            let _ = std::fs::remove_file(&part);
            let bytes = bytes?;
            if is_lfs_pointer(&bytes) {
                return skip(repo, path, "the server sent a Git LFS pointer");
            }
            checked(repo, path, &bytes);
            let tmp = file.with_extension(format!(
                "done-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &file);
            }
            Some(file)
        }
        Ok(out) => {
            let _ = std::fs::remove_file(&part);
            // curl exit code 22: an HTTP error. The network works, so the pinned list is wrong.
            if out.status.code() == Some(22) {
                panic!("{u}: {}", String::from_utf8_lossy(&out.stderr).trim());
            }
            OFFLINE.store(true, Ordering::Relaxed);
            skip(repo, path, "no network")
        }
        Err(_) => {
            OFFLINE.store(true, Ordering::Relaxed);
            skip(repo, path, "curl is not installed")
        }
    }
}

/// Every file of the table: (repository, path).
pub fn all() -> Vec<(String, String)> {
    table()["files"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| {
            let (r, p) = k.split_once(':').unwrap();
            (r.to_string(), p.to_string())
        })
        .collect()
}

/// Every IFC file (`.ifc`) of the table.
pub fn all_ifc() -> Vec<(String, String)> {
    all()
        .into_iter()
        .filter(|(_, p)| p.to_ascii_lowercase().ends_with(".ifc"))
        .collect()
}

/// Downloads many files at once (8 at a time). Files that cannot be had are left out.
pub fn prefetch(files: &[(String, String)]) {
    if disabled() {
        return;
    }
    let todo: Vec<&(String, String)> = files
        .iter()
        .filter(|(r, p)| !local(r, p).exists())
        .collect();
    let size = todo.len().div_ceil(8).max(1);
    std::thread::scope(|s| {
        for chunk in todo.chunks(size) {
            s.spawn(move || {
                for (r, p) in chunk {
                    let _ = need(r, p);
                }
            });
        }
    });
}

/// buildingSMART Certification-datasets, scene `name` of a schema folder (`2x3`, `4` or `4x3`).
pub fn scene(schema: &str, name: &str) -> Option<PathBuf> {
    let dir = match schema {
        "2x3" => "IFC 2.3.0.1 (IFC 2x3 TC1)",
        "4" => "IFC 4.0.2.1 (IFC 4 ADD2 TC1)",
        _ => "IFC 4.3.2.0 (IFC 4.3 ADD2)",
    };
    need("bsi-cert", &format!("{dir}/Simple-Scene/{name}.ifc"))
}

/// A file of the ISO Reference View samples.
pub fn reference_view(name: &str) -> Option<PathBuf> {
    need(
        "bsi-cert",
        &format!("IFC 4.0.2.1 (IFC 4 ADD2 TC1)/ISO Spec - ReferenceView_V1.2/{name}.ifc"),
    )
}

/// A file of the ISO spec archive of the community repository (extension included).
pub fn iso(name: &str) -> Option<PathBuf> {
    need(
        "bsi-community",
        &format!("IFC 4.0.2.1 (IFC 4)/ISO Spec archive/{name}"),
    )
}

/// A file of the IFC 4.3 RC archive of the community repository: `folder/file.ifc`.
pub fn rc(path: &str) -> Option<PathBuf> {
    need(
        "bsi-community",
        &format!("dev archive/IFC 4.3.RC Archive/{path}"),
    )
}

/// A file of the Bentley NURBS samples: `Cylinder/Facet.ifc`.
pub fn bentley(path: &str) -> Option<PathBuf> {
    need(
        "bsi-community",
        &format!("IFC 4.0.2.1 (IFC 4)/NURBS/Bentley Building Designer/{path}"),
    )
}
