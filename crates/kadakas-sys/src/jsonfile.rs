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
//! `<name>.bak` and then replaced. Any other read error (permission, disk) stops the write and
//! is returned: the file is not replaced by defaults.

use crate::fsio;
use serde_json::{Map, Value};
use std::path::Path;
use std::time::Duration;

/// How long a writer waits for the lock of a settings file.
const LOCK_WAIT: Duration = Duration::from_secs(2);

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

/// Reads a settings file that is about to be written. A missing file is an empty object. A file
/// that is not a JSON object is copied to `<name>.bak` and counts as empty. A read error other
/// than "not found" is returned.
pub fn read_for_update(path: &Path) -> std::io::Result<Map<String, Value>> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(o)) => Ok(o),
            _ => {
                std::fs::copy(path, fsio::sibling(path, "", ".bak"))?;
                Ok(Map::new())
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e),
    }
}

/// The lock of a settings file: `<name>.lock` next to it.
pub fn lock(path: &Path) -> std::io::Result<fsio::FileLock> {
    fsio::lock_file(&fsio::sibling(path, "", ".lock"), LOCK_WAIT)
}

/// Reads the file under its lock, lets `change` edit the object, and writes it again (the
/// previous file stays as `<name>.bak`).
pub fn update(path: &Path, change: impl FnOnce(&mut Map<String, Value>)) -> std::io::Result<()> {
    let _guard = lock(path)?;
    let mut object = read_for_update(path)?;
    change(&mut object);
    let text = serde_json::to_vec_pretty(&Value::Object(object)).map_err(std::io::Error::other)?;
    fsio::write_atomic(path, &text)
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
        assert_eq!(
            std::fs::read_to_string(d.join("x.json.bak")).unwrap(),
            "{ nope"
        );
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
    fn two_writers_do_not_lose_each_others_keys() {
        let d = tmp("race");
        let p = d.join("p.json");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        update(&p, |o| {
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
