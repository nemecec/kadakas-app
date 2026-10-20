//! The files of crash reports on disk (native only).
//!
//! ```text
//! <root>/runs/<run id>.crash.json    written by the app when it dies (see `capture`)
//! <root>/crash-reports/<run id>.json kept by the launcher after an abnormal end
//! ```
//!
//! `root` is the folder of the launcher (`KADAKAS_LAUNCHER_ROOT`, else `<state>/launcher`). An app
//! that no launcher started uses run ids that it makes itself (`standalone_id`) and keeps its
//! files in the same folders. At most [`MAX_KEPT`] reports are kept: when an eleventh comes, the
//! oldest goes. A report that waits for sending in the outbox is a different thing and has its
//! own limit (see `report::trim_outbox`).

use crate::file::CrashFile;
use std::path::{Path, PathBuf};

/// The most reports in the folder of kept reports.
pub const MAX_KEPT: usize = 10;

/// The prefix of a run id that the app makes itself, when no launcher started it. It is hex, so
/// it passes `valid_run_id` of the launcher. After it come the process id and the time, in hex.
pub const STANDALONE_PREFIX: &str = "5ca1ab1e";

/// A run id for an app that no launcher started.
pub fn standalone_id(pid: u32) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{STANDALONE_PREFIX}-{pid:x}-{nanos:x}")
}

/// The process id in a standalone run id.
pub fn standalone_pid(id: &str) -> Option<u32> {
    let rest = id.strip_prefix(STANDALONE_PREFIX)?.strip_prefix('-')?;
    u32::from_str_radix(rest.split('-').next()?, 16).ok()
}

/// A run id is hex digits and `-`, at most 64 characters (the same rule as the launcher's).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

pub fn runs_dir(root: &Path) -> PathBuf {
    root.join("runs")
}

pub fn kept_dir(root: &Path) -> PathBuf {
    root.join("crash-reports")
}

/// Where the app writes the crash file of run `id`.
pub fn live_path(root: &Path, id: &str) -> PathBuf {
    runs_dir(root).join(format!("{id}.crash.json"))
}

fn kept_path(root: &Path, id: &str) -> PathBuf {
    kept_dir(root).join(format!("{id}.json"))
}

/// A kept report.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub id: String,
    pub file: CrashFile,
}

fn read_file(path: &Path) -> Option<CrashFile> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > crate::file::MAX_FILE_BYTES {
        return None;
    }
    CrashFile::parse(&std::fs::read(path).ok()?)
}

fn ids_in(dir: &Path, suffix: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(suffix)?;
            valid_id(id).then(|| id.to_string())
        })
        .collect()
}

/// Reads the crash file of run `id` that the app wrote, if there is one.
pub fn read_live(root: &Path, id: &str) -> Option<CrashFile> {
    if !valid_id(id) {
        return None;
    }
    read_file(&live_path(root, id))
}

/// Deletes the crash file of run `id`, if any.
pub fn remove_live(root: &Path, id: &str) {
    if valid_id(id) {
        let _ = std::fs::remove_file(live_path(root, id));
    }
}

/// Stores `file` as a kept report under `id`, and trims the folder to [`MAX_KEPT`].
pub fn store(root: &Path, id: &str, file: &CrashFile) -> std::io::Result<()> {
    if !valid_id(id) {
        return Err(std::io::Error::other("not a run id"));
    }
    let dir = kept_dir(root);
    kadakas_sys::fsio::create_private_dir(&dir)?;
    kadakas_sys::fsio::write_private(&kept_path(root, id), &file.to_json())?;
    trim(root, MAX_KEPT);
    Ok(())
}

/// Moves the crash file of run `id` to the kept reports. Returns false when there was none.
/// The launcher calls this after an abnormal end.
///
/// A file that cannot be read (too large, damaged, or from a newer Kadakas) is never deleted
/// silently: it moves to `crash-reports/<id>.unreadable`, and a small report says so.
pub fn keep(root: &Path, id: &str) -> std::io::Result<bool> {
    keep_as(root, id, false)
}

/// Like [`keep`]. With `survived` the report says that the app survived the panic and ended
/// later by other means (a force quit, a shutdown): the gap between the panic and now is
/// written into the file.
pub fn keep_as(root: &Path, id: &str, survived: bool) -> std::io::Result<bool> {
    if !valid_id(id) {
        return Ok(false);
    }
    let live = live_path(root, id);
    let Ok(meta) = std::fs::metadata(&live) else {
        return Ok(false);
    };
    let Some(mut file) = read_file(&live) else {
        let mut stub = CrashFile::new(crate::file::CrashKind::Native);
        stub.run_id = id.to_string();
        stub.created = now_secs();
        stub.message = format!(
            "UNREADABLE_CRASH_FILE ({} bytes; too large, damaged or from a newer Kadakas; kept as {id}.unreadable)",
            meta.len()
        );
        kadakas_sys::fsio::create_private_dir(&kept_dir(root))?;
        std::fs::rename(&live, kept_dir(root).join(format!("{id}.unreadable")))?;
        store(root, id, &stub)?;
        return Ok(true);
    };
    if file.run_id.is_empty() {
        file.run_id = id.to_string();
    }
    if survived {
        file.survived_gap_secs = Some(now_secs().saturating_sub(file.created));
    }
    store(root, id, &file)?;
    let _ = std::fs::remove_file(&live);
    Ok(true)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The kept reports, oldest first.
pub fn list(root: &Path) -> Vec<Kept> {
    let mut out: Vec<Kept> = ids_in(&kept_dir(root), ".json")
        .into_iter()
        .filter_map(|id| {
            let file = read_file(&kept_path(root, &id))?;
            Some(Kept { id, file })
        })
        .collect();
    out.sort_by(|a, b| {
        a.file
            .created
            .cmp(&b.file.created)
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

/// How many reports are kept.
pub fn count(root: &Path) -> usize {
    ids_in(&kept_dir(root), ".json").len()
}

/// Deletes a kept report.
pub fn remove(root: &Path, id: &str) {
    if valid_id(id) {
        let _ = std::fs::remove_file(kept_path(root, id));
    }
}

/// Deletes the oldest kept reports until `max` are left. A file that cannot be read goes first.
pub fn trim(root: &Path, max: usize) {
    let dir = kept_dir(root);
    let mut entries: Vec<(u64, String)> = ids_in(&dir, ".json")
        .into_iter()
        .map(|id| {
            let created = read_file(&kept_path(root, &id)).map_or(0, |f| f.created);
            (created, id)
        })
        .collect();
    entries.sort();
    while entries.len() > max {
        let (_, id) = entries.remove(0);
        remove(root, &id);
    }
    // An unreadable original goes with its report.
    for id in ids_in(&dir, ".unreadable") {
        if !kept_path(root, &id).exists() {
            let _ = std::fs::remove_file(dir.join(format!("{id}.unreadable")));
        }
    }
}

/// A crash file of an app that no launcher started, and whose process is gone, is a crash: it
/// moves to the kept reports. `alive` tells whether a process lives (`pid_alive`). Returns how
/// many were kept.
pub fn adopt_standalone(root: &Path, alive: &dyn Fn(u32) -> bool) -> usize {
    let mut n = 0;
    for id in ids_in(&runs_dir(root), ".crash.json") {
        let Some(pid) = standalone_pid(&id) else {
            continue;
        };
        if pid == std::process::id() || alive(pid) {
            continue;
        }
        if keep(root, &id).unwrap_or(false) {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::CrashKind;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-crash-q-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn file(created: u64, msg: &str) -> CrashFile {
        let mut f = CrashFile::new(CrashKind::Panic);
        f.created = created;
        f.message = msg.into();
        f.version = "1.0.0".into();
        f
    }

    fn write_live(root: &Path, id: &str, f: &CrashFile) {
        std::fs::create_dir_all(runs_dir(root)).unwrap();
        std::fs::write(live_path(root, id), f.to_json()).unwrap();
    }

    #[test]
    fn a_live_file_moves_to_the_kept_reports() {
        let d = tmp("keep");
        write_live(&d, "1a-2b", &file(5, "boom"));
        assert_eq!(read_live(&d, "1a-2b").unwrap().message, "boom");
        assert!(keep(&d, "1a-2b").unwrap());
        assert!(read_live(&d, "1a-2b").is_none());
        let l = list(&d);
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].id.as_str(), l[0].file.message.as_str()),
            ("1a-2b", "boom")
        );
        assert_eq!(l[0].file.run_id, "1a-2b");
        // Nothing to keep the second time.
        assert!(!keep(&d, "1a-2b").unwrap());
        remove(&d, "1a-2b");
        assert_eq!(count(&d), 0);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_damaged_or_oversized_live_file_is_never_deleted_silently() {
        let d = tmp("damaged");
        std::fs::create_dir_all(runs_dir(&d)).unwrap();
        std::fs::write(live_path(&d, "aa"), b"{ not json").unwrap();
        let big = vec![b' '; (crate::file::MAX_FILE_BYTES + 10) as usize];
        std::fs::write(live_path(&d, "bb"), &big).unwrap();
        assert!(keep(&d, "aa").unwrap());
        assert!(keep(&d, "bb").unwrap());
        assert!(!live_path(&d, "aa").exists() && !live_path(&d, "bb").exists());
        // The originals are kept, and a report says what happened.
        assert_eq!(
            std::fs::read(kept_dir(&d).join("aa.unreadable")).unwrap(),
            b"{ not json"
        );
        assert_eq!(
            std::fs::read(kept_dir(&d).join("bb.unreadable"))
                .unwrap()
                .len(),
            big.len()
        );
        let l = list(&d);
        assert_eq!(l.len(), 2);
        assert!(l[0].file.message.starts_with("UNREADABLE_CRASH_FILE"));
        assert!(l.iter().any(|k| k.file.message.contains("bytes")));
        // The original goes with its report.
        remove(&d, "aa");
        trim(&d, 10);
        assert!(!kept_dir(&d).join("aa.unreadable").exists());
        assert!(kept_dir(&d).join("bb.unreadable").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_survived_panic_gets_the_gap_and_a_failure_does_not() {
        let d = tmp("survived");
        let mut f = file(1_000, "panic once");
        f.created = now_secs().saturating_sub(90);
        write_live(&d, "a1", &f);
        write_live(&d, "a2", &f);
        assert!(keep_as(&d, "a1", true).unwrap());
        assert!(keep_as(&d, "a2", false).unwrap());
        let l = list(&d);
        let gap = |id: &str| {
            l.iter()
                .find(|k| k.id == id)
                .unwrap()
                .file
                .survived_gap_secs
        };
        assert!((90..120).contains(&gap("a1").unwrap()));
        assert_eq!(gap("a2"), None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn at_most_ten_are_kept_and_the_oldest_go_first() {
        let d = tmp("limit");
        for i in 0..15u64 {
            let id = format!("{i:x}");
            write_live(&d, &id, &file(100 + i, &format!("crash {i}")));
            assert!(keep(&d, &id).unwrap());
            assert!(count(&d) <= MAX_KEPT);
        }
        let l = list(&d);
        assert_eq!(l.len(), MAX_KEPT);
        assert_eq!(l[0].file.message, "crash 5");
        assert_eq!(l.last().unwrap().file.message, "crash 14");
        trim(&d, 3);
        assert_eq!(list(&d).len(), 3);
        assert_eq!(list(&d)[0].file.message, "crash 12");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn ids_that_could_name_other_paths_are_refused() {
        let d = tmp("ids");
        for bad in ["", "../x", "a/b", "a.b", "zz", &"a".repeat(65)] {
            assert!(!valid_id(bad), "{bad}");
            assert!(read_live(&d, bad).is_none());
            assert!(!keep(&d, bad).unwrap());
            assert!(store(&d, bad, &file(1, "x")).is_err());
            remove(&d, bad);
            remove_live(&d, bad);
        }
        // A stray file with a bad name in the folder is ignored.
        std::fs::create_dir_all(kept_dir(&d)).unwrap();
        std::fs::write(kept_dir(&d).join("notes.json"), b"{}").unwrap();
        std::fs::write(kept_dir(&d).join("ab.json.tmp"), b"{}").unwrap();
        assert_eq!(count(&d), 0);
        assert!(list(&d).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn standalone_ids_carry_the_process() {
        let id = standalone_id(0xbeef);
        assert!(valid_id(&id));
        assert_eq!(standalone_pid(&id), Some(0xbeef));
        assert_eq!(standalone_pid("1a-2b"), None);
    }

    #[test]
    fn a_standalone_crash_is_adopted_when_its_process_is_gone() {
        let d = tmp("adopt");
        let dead = standalone_id(4_000_001);
        let alive = standalone_id(4_000_002);
        let own = standalone_id(std::process::id());
        let launched = "1a-2b-3c";
        for id in [&dead, &alive, &own] {
            write_live(&d, id, &file(1, id));
        }
        write_live(&d, launched, &file(1, "launcher run"));
        let n = adopt_standalone(&d, &|pid| pid == 4_000_002);
        assert_eq!(n, 1);
        let ids: Vec<String> = list(&d).into_iter().map(|k| k.id).collect();
        assert_eq!(ids, vec![dead.clone()]);
        // The others stay where they are.
        assert!(live_path(&d, &alive).exists());
        assert!(live_path(&d, &own).exists());
        assert!(live_path(&d, launched).exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
