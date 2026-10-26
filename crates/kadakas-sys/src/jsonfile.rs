//! Settings files in JSON, changed key by key.
//!
//! A program that keeps its settings in memory must not write the whole copy back: another
//! program (the welcome dialog, an older version, a second window) may have changed a key in the
//! meantime. The rule here: write only the keys that this program changed, into the file as it is
//! now, under a lock, and keep every key that it does not know.
//!
//! - [`diff`] finds the keys that differ between two values (the copy as it was read or saved,
//!   and the copy now).
//! - [`update`] reads the file under a lock, lets the caller change the object and writes it
//!   again. [`write_changes`] is the usual use of the two.
//!
//! A file that is missing is an empty object. A file that cannot be parsed is copied to
//! `<name>.damaged-<seconds>` (a name that no write replaces) and then replaced; the `.bak` of the
//! last good version stays. Any other read error (permission, disk) stops the write and
//! is returned: the file is not replaced by defaults.

use crate::fsio;
use serde_json::{Map, Value};
use std::path::Path;
use std::time::Duration;

/// How long a writer waits for the lock of a settings file.
const LOCK_WAIT: Duration = Duration::from_secs(1);

/// One changed key. `path` goes through nested objects (`["units", "length"]`). `value` is
/// `None` when the key was removed.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: Vec<String>,
    pub value: Option<Value>,
}

/// The keys that differ between `old` and `new`, as paths into nested objects. A value that is
/// not an object in both is one change as a whole.
pub fn diff(old: &Value, new: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    walk(&mut Vec::new(), old, new, &mut out);
    out
}

fn walk(path: &mut Vec<String>, old: &Value, new: &Value, out: &mut Vec<Change>) {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, nv) in b {
                path.push(k.clone());
                match a.get(k) {
                    Some(ov) => walk(path, ov, nv, out),
                    None => out.push(Change {
                        path: path.clone(),
                        value: Some(nv.clone()),
                    }),
                }
                path.pop();
            }
            for k in a.keys().filter(|k| !b.contains_key(*k)) {
                path.push(k.clone());
                out.push(Change {
                    path: path.clone(),
                    value: None,
                });
                path.pop();
            }
        }
        _ if old == new => {}
        _ => out.push(Change {
            path: path.clone(),
            value: Some(new.clone()),
        }),
    }
}

/// Puts the changes into `object`. A removed key leaves an object that became empty in place.
pub fn apply(object: &mut Map<String, Value>, changes: &[Change]) {
    for c in changes {
        set_path(object, &c.path, c.value.clone());
    }
}

/// Sets (or, with `None`, removes) the key at `path`. Objects on the way are made when missing
/// or replaced when they are not objects.
pub fn set_path(object: &mut Map<String, Value>, path: &[String], value: Option<Value>) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        match value {
            Some(v) => {
                object.insert(first.clone(), v);
            }
            None => {
                object.remove(first);
            }
        }
        return;
    }
    if value.is_none() && !object.get(first).is_some_and(Value::is_object) {
        return;
    }
    let entry = object
        .entry(first.clone())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(inner) = entry {
        set_path(inner, rest, value);
    }
}

/// The value at `path`, if there is one.
pub fn get_path<'a>(object: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let v = object.get(*first)?;
    if rest.is_empty() {
        Some(v)
    } else {
        get_path(v.as_object()?, rest)
    }
}

/// Reads a settings file for reading only: a missing, damaged or unreadable file is an empty
/// object.
pub fn read_object(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| match v {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .unwrap_or_default()
}

/// What was in a settings file that is about to be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Before {
    /// A JSON object (a good file), or no file at all.
    Good,
    /// An empty file, or text that is not a JSON object.
    Damaged,
}

/// How many copies of each kind are kept. The oldest go first.
pub const KEEP_COPIES: usize = 10;

fn is_object(bytes: &[u8]) -> bool {
    matches!(serde_json::from_slice::<Value>(bytes), Ok(Value::Object(_)))
}

/// The copies of `kind` (`damaged` or `good`) next to `path`, the oldest first. A name is
/// `<name>.<kind>-<seconds>` or `<name>.<kind>-<seconds>-<n>`.
fn copies(path: &Path, kind: &str) -> Vec<std::path::PathBuf> {
    keyed_copies(path, kind)
        .into_iter()
        .map(|(_, _, p)| p)
        .collect()
}

/// Like [`copies`], with the seconds and the number of each name.
fn keyed_copies(path: &Path, kind: &str) -> Vec<(u64, u32, std::path::PathBuf)> {
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let prefix = format!(
        "{}.{kind}-",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
    );
    let mut out: Vec<(u64, u32, std::path::PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let rest = name.strip_prefix(&prefix)?;
            let mut parts = rest.splitn(2, '-');
            let secs = parts.next()?.parse().ok()?;
            let n = parts.next().map_or(Some(0), |n| n.parse().ok())?;
            Some((secs, n, e.path()))
        })
        .collect();
    out.sort();
    out
}

/// Keeps `bytes` as a copy of `kind` next to `path`, in a name that no write replaces. Nothing is
/// copied when the newest copy has the same bytes (a write that fails and is tried again every
/// few seconds makes one copy, not thousands). Only the newest [`KEEP_COPIES`] stay. An error
/// when no copy could be made: the caller does not replace the file then.
fn keep_copy_at(path: &Path, kind: &str, bytes: &[u8], secs: u64) -> std::io::Result<()> {
    let existing = copies(path, kind);
    if let Some(newest) = existing.last()
        && std::fs::read(newest).is_ok_and(|b| b == bytes)
    {
        return Ok(());
    }
    // The number goes on after the highest one of this second, so a name that an earlier prune
    // freed does not come back as the "oldest" name for the newest copy.
    let first = keyed_copies(path, kind)
        .iter()
        .filter(|(s, _, _)| *s == secs)
        .map(|(_, n, _)| n + 1)
        .max()
        .unwrap_or(0);
    for n in first..first + 1000 {
        let suffix = if n == 0 {
            format!(".{kind}-{secs}")
        } else {
            format!(".{kind}-{secs}-{n}")
        };
        let to = fsio::sibling(path, "", &suffix);
        if to.exists() {
            continue;
        }
        std::fs::write(&to, bytes)?;
        let all = copies(path, kind);
        for old in all.iter().take(all.len().saturating_sub(KEEP_COPIES)) {
            let _ = std::fs::remove_file(old);
        }
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "no free name for a copy of {}",
        path.display()
    )))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The copies that [`read_for_update`] made of damaged versions of `path`, the oldest first.
pub fn damaged_copies(path: &Path) -> Vec<std::path::PathBuf> {
    copies(path, "damaged")
}

/// The copies of the last good version (`<name>.good-<seconds>`) that were made before a damaged
/// file was replaced, the oldest first.
pub fn good_copies(path: &Path) -> Vec<std::path::PathBuf> {
    copies(path, "good")
}

/// The content of the last good version, from `<name>.bak`, if that is a JSON object.
fn good_backup(path: &Path) -> Option<(Vec<u8>, Map<String, Value>)> {
    let bytes = std::fs::read(fsio::sibling(path, "", ".bak")).ok()?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(o)) => Some((bytes, o)),
        _ => None,
    }
}

/// Reads a settings file that is about to be written, and says what was there.
///
/// - A missing file is an empty object.
/// - An empty file, or a file that is not a JSON object, is `Damaged`. A damaged file with text
///   is first kept as `<name>.damaged-<seconds>`. If `<name>.bak` holds a good version, the
///   object is that version (the settings come back), and the version is also kept as
///   `<name>.good-<seconds>`, because the next writes move `.bak` on.
/// - A read error other than "not found" is returned. So is a failure to make a copy: the file
///   is not replaced then.
pub fn read_for_update_ex(path: &Path) -> std::io::Result<(Map<String, Value>, Before)> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(o)) => Ok((o, Before::Good)),
            _ => {
                let secs = now_secs();
                if !bytes.iter().all(u8::is_ascii_whitespace) {
                    keep_copy_at(path, "damaged", &bytes, secs)?;
                }
                let restored = match good_backup(path) {
                    Some((good, object)) => {
                        keep_copy_at(path, "good", &good, secs)?;
                        object
                    }
                    None => Map::new(),
                };
                Ok((restored, Before::Damaged))
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Map::new(), Before::Good)),
        Err(e) => Err(e),
    }
}

/// Like [`read_for_update_ex`], for a caller that does not write the file itself.
pub fn read_for_update(path: &Path) -> std::io::Result<Map<String, Value>> {
    read_for_update_ex(path).map(|(o, _)| o)
}

/// The bytes of a settings file for a program that loads its settings. `Ok(None)`: no file. A file
/// that is empty or not a JSON object is replaced by the last good version (`<name>.bak`) when
/// there is one; the flag says so, and the program tells the person. The file itself is not
/// changed.
pub fn read_for_load(path: &Path) -> std::io::Result<Option<(Vec<u8>, bool)>> {
    match std::fs::read(path) {
        Ok(bytes) if is_object(&bytes) => Ok(Some((bytes, false))),
        Ok(bytes) => Ok(Some(match good_backup(path) {
            Some((good, _)) => (good, true),
            None => (bytes, false),
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The lock of a settings file: `<name>.lock` next to it. `None` on a file system that cannot
/// lock (one line on stderr): the write goes on without a lock. A writer waits up to a second for
/// another holder; the holders keep the lock for a few milliseconds.
pub fn lock(path: &Path) -> std::io::Result<Option<fsio::FileLock>> {
    lock_for(path, LOCK_WAIT)
}

/// Like [`lock`], with the wait given. The poll inside is 20 ms. When the wait runs out the error
/// is `ErrorKind::WouldBlock` with a text that names the file: the caller shows it and tries
/// again later (a slow disk or a virus scanner can hold a write for longer than the wait).
pub fn lock_for(path: &Path, wait: Duration) -> std::io::Result<Option<fsio::FileLock>> {
    fsio::lock_file_or_skip(&fsio::sibling(path, "", ".lock"), wait).map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!(
                    "{} is locked by another program (waited {} ms)",
                    path.display(),
                    wait.as_millis()
                ),
            )
        } else {
            e
        }
    })
}

/// Reads the file under its lock, lets `change` edit the object, and writes it again (the
/// previous file stays as `<name>.bak`).
pub fn update(path: &Path, change: impl FnOnce(&mut Map<String, Value>)) -> std::io::Result<()> {
    update_with_wait(path, LOCK_WAIT, change)
}

/// Like [`update`], with the wait for the lock given (the tests give a long one, so that a slow
/// machine does not decide the result).
pub fn update_with_wait(
    path: &Path,
    wait: Duration,
    change: impl FnOnce(&mut Map<String, Value>),
) -> std::io::Result<()> {
    let _guard = lock_for(path, wait)?;
    let (mut object, before) = read_for_update_ex(path)?;
    change(&mut object);
    let text = serde_json::to_vec_pretty(&Value::Object(object)).map_err(std::io::Error::other)?;
    match before {
        // The previous good version stays as `<name>.bak`.
        Before::Good => fsio::write_atomic(path, &text),
        // A damaged or empty file must not replace a good `.bak`; a damaged one with text is
        // kept under its own name already.
        Before::Damaged => fsio::write_atomic_no_backup(path, &text),
    }
}

/// Writes the changes into the file as it is now. Nothing is written when there are none.
pub fn write_changes(path: &Path, changes: &[Change]) -> std::io::Result<()> {
    if changes.is_empty() {
        return Ok(());
    }
    update(path, |o| apply(o, changes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-jsonfile-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_diff_names_the_changed_keys_only() {
        let old = json!({"a": 1, "b": {"x": 1, "y": 2}, "c": [1], "gone": true});
        let new = json!({"a": 1, "b": {"x": 5, "y": 2}, "c": [1, 2], "d": "n"});
        let mut got = diff(&old, &new);
        got.sort_by(|a, b| a.path.cmp(&b.path));
        let paths: Vec<_> = got.iter().map(|c| c.path.join(".")).collect();
        assert_eq!(paths, ["b.x", "c", "d", "gone"]);
        assert_eq!(got[3].value, None);
        assert!(diff(&old, &old).is_empty());
        // A type change is one change of the whole key.
        let got = diff(&json!({"u": 1}), &json!({"u": {"k": 1}}));
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn changes_go_into_the_file_as_it_is_now_and_other_keys_stay() {
        let d = tmp("changes");
        let p = d.join("prefs.json");
        std::fs::write(
            &p,
            r#"{"theme":"dark","units":{"length":"mm","angle":"dms"},"future":[1]}"#,
        )
        .unwrap();
        // This program read the file earlier, when the theme was "light".
        let old = json!({"theme": "light", "snap": 5, "units": {"length": "mm"}});
        let new = json!({"theme": "light", "snap": 7, "units": {"length": "ft"}});
        write_changes(&p, &diff(&old, &new)).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(
            v["theme"], "dark",
            "a key that this program did not change stays"
        );
        assert_eq!(v["snap"], 7);
        assert_eq!(v["units"], json!({"length": "ft", "angle": "dms"}));
        assert_eq!(v["future"], json!([1]));
        // Nothing changed: the file is not touched.
        let before = std::fs::metadata(&p).unwrap().modified().unwrap();
        write_changes(&p, &[]).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), before);
        // A removed key goes; a removed key below a missing object does nothing.
        let mut o = Map::new();
        set_path(&mut o, &["a".into(), "b".into()], None);
        assert!(o.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_file_is_empty_a_damaged_one_is_kept_aside_and_a_read_error_stops() {
        let d = tmp("damaged");
        let p = d.join("x.json");
        assert!(read_for_update(&p).unwrap().is_empty());
        std::fs::write(&p, "{ nope").unwrap();
        assert!(read_for_update(&p).unwrap().is_empty());
        let kept = damaged_copies(&p);
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), "{ nope");
        std::fs::write(&p, "[1]").unwrap();
        assert!(read_for_update(&p).unwrap().is_empty());
        // A folder where the file should be is a read error that is not "not found".
        let dir_as_file = d.join("isdir.json");
        std::fs::create_dir_all(&dir_as_file).unwrap();
        assert!(read_for_update(&dir_as_file).is_err());
        assert!(update(&dir_as_file, |_| {}).is_err());
        assert!(dir_as_file.is_dir(), "nothing replaced it");
        // `read_object` never fails.
        assert!(read_object(&dir_as_file).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_file_comes_back_from_the_backup_and_the_good_copy_is_never_rotated() {
        let d = tmp("damaged-twice");
        let p = d.join("p.json");
        // A good file with a good .bak behind it (the second write moves the first to .bak).
        std::fs::write(&p, r#"{"first": 1}"#).unwrap();
        update(&p, |o| {
            o.insert("second".into(), json!(2));
        })
        .unwrap();
        let good_bak = std::fs::read(d.join("p.json.bak")).unwrap();
        assert_eq!(good_bak, br#"{"first": 1}"#);
        // The file is damaged. Three writes follow.
        std::fs::write(&p, "{ half a file").unwrap();
        for (k, v) in [("a", 1), ("b", 2), ("c", 3)] {
            update(&p, |o| {
                o.insert(k.into(), json!(v));
            })
            .unwrap();
        }
        // The settings of the .bak came back, with the three changes.
        let now = read_object(&p);
        assert_eq!(now.len(), 4, "{now:?}");
        assert_eq!(now["first"], 1);
        // The damaged text is kept under its own name.
        let damaged = damaged_copies(&p);
        assert_eq!(damaged.len(), 1, "{damaged:?}");
        assert_eq!(
            std::fs::read_to_string(&damaged[0]).unwrap(),
            "{ half a file"
        );
        // The last good version stays as .good-<seconds>, although .bak moved on.
        let good = good_copies(&p);
        assert_eq!(good.len(), 1, "{good:?}");
        assert_eq!(std::fs::read(&good[0]).unwrap(), good_bak);
        assert_ne!(std::fs::read(d.join("p.json.bak")).unwrap(), good_bak);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_file_without_a_backup_starts_empty_and_is_kept() {
        let d = tmp("damaged-no-bak");
        let p = d.join("p.json");
        std::fs::write(&p, "{ half a file").unwrap();
        update(&p, |o| {
            o.insert("a".into(), json!(1));
        })
        .unwrap();
        assert_eq!(read_object(&p).len(), 1);
        assert_eq!(damaged_copies(&p).len(), 1);
        assert!(good_copies(&p).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_load_takes_the_backup_when_the_file_is_empty_or_damaged() {
        let d = tmp("load");
        let p = d.join("p.json");
        assert!(read_for_load(&p).unwrap().is_none(), "no file, nothing");
        std::fs::write(&p, r#"{"a": 1}"#).unwrap();
        assert_eq!(
            read_for_load(&p).unwrap().unwrap(),
            (br#"{"a": 1}"#.to_vec(), false)
        );
        std::fs::write(d.join("p.json.bak"), r#"{"old": 1}"#).unwrap();
        for bad in ["", "  ", "{ nope", "[1]"] {
            std::fs::write(&p, bad).unwrap();
            let (bytes, from_backup) = read_for_load(&p).unwrap().unwrap();
            assert_eq!(
                (bytes.as_slice(), from_backup),
                (br#"{"old": 1}"#.as_slice(), true),
                "{bad:?}"
            );
        }
        // Without a good backup the damaged bytes come back as they are.
        std::fs::remove_file(d.join("p.json.bak")).unwrap();
        let (bytes, from_backup) = read_for_load(&p).unwrap().unwrap();
        assert_eq!((bytes.as_slice(), from_backup), (b"[1]".as_slice(), false));
        // A deleted file is not brought back by the backup.
        std::fs::write(d.join("p.json.bak"), r#"{"old": 1}"#).unwrap();
        std::fs::remove_file(&p).unwrap();
        assert!(read_for_load(&p).unwrap().is_none());
        // Reading does not change the files.
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_write_that_fails_and_is_tried_again_makes_one_copy_of_the_same_damage() {
        let d = tmp("copies-same");
        let p = d.join("p.json");
        std::fs::write(&p, "{ same damage").unwrap();
        for _ in 0..50 {
            read_for_update(&p).unwrap();
        }
        assert_eq!(damaged_copies(&p).len(), 1);
        // Different damage is a new copy.
        std::fs::write(&p, "{ other damage").unwrap();
        read_for_update(&p).unwrap();
        assert_eq!(damaged_copies(&p).len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_the_newest_copies_stay() {
        let d = tmp("copies-cap");
        let p = d.join("p.json");
        for n in 0..(KEEP_COPIES + 5) {
            std::fs::write(&p, format!("{{ damage {n}")).unwrap();
            // The same second and different texts: the names get a number.
            keep_copy_at(&p, "damaged", format!("{{ damage {n}").as_bytes(), 1000).unwrap();
        }
        let kept = damaged_copies(&p);
        assert_eq!(kept.len(), KEEP_COPIES);
        // The oldest are gone, the newest is there, in the order they were made.
        let last = std::fs::read_to_string(kept.last().unwrap()).unwrap();
        assert_eq!(last, format!("{{ damage {}", KEEP_COPIES + 4));
        let first = std::fs::read_to_string(&kept[0]).unwrap();
        assert_eq!(first, "{ damage 5");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_file_is_not_replaced_when_no_copy_can_be_written() {
        let d = tmp("copies-full");
        let p = d.join("p.json");
        std::fs::write(&p, "{ damage").unwrap();
        // A copy that cannot be written is an error, and the file stays. `update` stops there.
        let blocked = d.join("sub");
        std::fs::write(&blocked, b"x").unwrap();
        assert!(keep_copy_at(&blocked.join("p.json"), "damaged", b"x", 1).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"{ damage");
        // Many copies of the same second get rising numbers and never reuse a pruned name.
        for n in 0..30 {
            keep_copy_at(&p, "damaged", format!("text {n}").as_bytes(), 5).unwrap();
        }
        let names: Vec<String> = damaged_copies(&p)
            .iter()
            .map(|c| std::fs::read_to_string(c).unwrap())
            .collect();
        let want: Vec<String> = (20..30).map(|n| format!("text {n}")).collect();
        assert_eq!(names, want);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_file_restores_the_backup_and_does_not_replace_it() {
        let d = tmp("empty");
        let p = d.join("p.json");
        std::fs::write(&p, r#"{"keep": "me"}"#).unwrap();
        update(&p, |o| {
            o.insert("x".into(), json!(1));
        })
        .unwrap();
        let good = std::fs::read(d.join("p.json.bak")).unwrap();
        assert_eq!(good, br#"{"keep": "me"}"#);
        // The file is cut to zero bytes (a crash, a full disk).
        std::fs::write(&p, b"").unwrap();
        update(&p, |o| {
            o.insert("y".into(), json!(2));
        })
        .unwrap();
        assert_eq!(
            std::fs::read(d.join("p.json.bak")).unwrap(),
            good,
            ".bak is untouched"
        );
        let now = read_object(&p);
        assert_eq!(
            (now.get("keep"), now.get("y")),
            (Some(&json!("me")), Some(&json!(2)))
        );
        assert!(
            damaged_copies(&p).is_empty(),
            "nothing to keep from an empty file"
        );
        assert_eq!(std::fs::read(&good_copies(&p)[0]).unwrap(), good);
        // Whitespace only counts as empty too.
        std::fs::write(&p, b" \n").unwrap();
        update(&p, |_| {}).unwrap();
        assert_eq!(std::fs::read(d.join("p.json.bak")).unwrap(), good);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_writer_waits_for_the_lock_and_then_adds_to_what_the_holder_wrote() {
        let d = tmp("ordered");
        let p = d.join("p.json");
        let (tx, rx) = std::sync::mpsc::channel();
        let holder = {
            let p = p.clone();
            std::thread::spawn(move || {
                // The holder has the lock and writes its key in the middle of the hold.
                let guard = lock(&p).unwrap();
                tx.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(150));
                std::fs::write(&p, r#"{"a": 1}"#).unwrap();
                drop(guard);
            })
        };
        rx.recv().unwrap();
        // Without the lock this would read the empty file, and the key "a" would be lost.
        update_with_wait(&p, Duration::from_secs(120), |o| {
            o.insert("b".into(), json!(2));
        })
        .unwrap();
        holder.join().unwrap();
        let o = read_object(&p);
        assert_eq!((o.get("a"), o.get("b")), (Some(&json!(1)), Some(&json!(2))));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn two_writers_do_not_lose_each_others_keys() {
        let d = tmp("race");
        let p = d.join("p.json");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        // A long wait: no timing decides this test, also on a slow runner.
                        update_with_wait(&p, Duration::from_secs(120), |o| {
                            o.insert(format!("k{i}"), json!(i));
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        assert_eq!(read_object(&p).len(), 8);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_writer_that_runs_out_of_wait_gets_a_clear_error_and_the_file_is_untouched() {
        let d = tmp("wait-out");
        let p = d.join("p.json");
        std::fs::write(&p, r#"{"a": 1}"#).unwrap();
        let held = lock(&p).unwrap();
        let e = update_with_wait(&p, Duration::from_millis(50), |o| {
            o.insert("b".into(), json!(2));
        })
        .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
        let text = e.to_string();
        assert!(text.contains("p.json") && text.contains("locked"), "{text}");
        assert_eq!(std::fs::read(&p).unwrap(), br#"{"a": 1}"#);
        drop(held);
        // Free again: the same call works.
        update_with_wait(&p, Duration::from_millis(50), |o| {
            o.insert("b".into(), json!(2));
        })
        .unwrap();
        assert_eq!(read_object(&p).len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn get_path_goes_through_objects() {
        let o = json!({"a": {"b": 3}}).as_object().unwrap().clone();
        assert_eq!(get_path(&o, &["a", "b"]), Some(&json!(3)));
        assert_eq!(get_path(&o, &["a", "c"]), None);
        assert_eq!(get_path(&o, &["a", "b", "c"]), None);
    }

    #[test]
    fn a_lock_waits_for_the_holder_and_then_gives_up() {
        let d = tmp("lock");
        let p = d.join("l.lock");
        let held = fsio::lock_file(&p, Duration::from_millis(10)).unwrap();
        let e = fsio::lock_file(&p, Duration::from_millis(60)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
        drop(held);
        assert!(fsio::lock_file(&p, Duration::from_millis(10)).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }
}
