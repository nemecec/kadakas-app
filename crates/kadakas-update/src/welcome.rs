//! The mark of the welcome dialog (PLAT-26), in a file of its own: `welcome.json` next to
//! `launcher.json`. An older version of the app writes `launcher.json` without the keys that it
//! does not know, so a mark in that file would be lost when the person goes back to an older
//! version. This file is not written by any older program.
//!
//! ```json
//! { "done": 1, "applied": "1730000000000000000" }
//! ```
//!
//! - `done`: the version of the dialog that the person has seen (0: never).
//! - `applied`: changes when the dialog stored choices. A running program looks at it and reads
//!   its preferences again when it changes.
//!
//! The key `welcome_done` of `launcher.json` (the first form of the mark) is still read.

use kadakas_sys::jsonfile;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn mark_path(root: &Path) -> PathBuf {
    root.join("welcome.json")
}

/// The version of the dialog that the person has seen: the larger of the two marks.
pub fn done_version(root: &Path) -> u32 {
    let new = jsonfile::read_object(&mark_path(root))
        .get("done")
        .and_then(Value::as_u64);
    let old = jsonfile::read_object(&crate::Settings::path(root))
        .get("welcome_done")
        .and_then(Value::as_u64);
    new.max(old).unwrap_or(0).min(u64::from(u32::MAX)) as u32
}

/// The stamp of the last change that the dialog stored. Empty when there was none.
pub fn applied_stamp(root: &Path) -> String {
    jsonfile::read_object(&mark_path(root))
        .get("applied")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Marks the dialog `version` as seen (never lowers the mark). With `changed`, the stamp moves.
pub fn write_mark(root: &Path, version: u32, changed: bool) -> std::io::Result<()> {
    jsonfile::update(&mark_path(root), |o| {
        let have = o.get("done").and_then(Value::as_u64).unwrap_or(0);
        o.insert("done".into(), Value::from(have.max(u64::from(version))));
        if changed {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let old = o.get("applied").and_then(Value::as_str).unwrap_or("");
            // Strictly new, also when the clock is coarse or two changes share a tick.
            let mut stamp = nanos.to_string();
            if stamp.as_str() <= old && old.len() == stamp.len() {
                stamp = (old.parse::<u128>().unwrap_or(nanos) + 1).to_string();
            }
            o.insert("applied".into(), Value::from(stamp));
        }
    })
}

/// Removes the mark (the dialog is due again, with the current settings).
pub fn clear_mark(root: &Path) {
    let _ = std::fs::remove_file(mark_path(root));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{rm_dir, tmp};

    #[test]
    fn the_mark_lives_in_its_own_file_and_survives_an_older_writer() {
        let root = tmp("mark");
        assert_eq!(done_version(&root), 0);
        write_mark(&root, 1, false).unwrap();
        assert_eq!(done_version(&root), 1);
        assert_eq!(applied_stamp(&root), "");
        // An older app writes launcher.json without the key it does not know.
        crate::Settings::update(&root, |s| s.keep_older = 2).unwrap();
        std::fs::write(
            crate::Settings::path(&root),
            r#"{"channel":"stable","mode":"auto-update","keep_older":2,"site":"","flavor":"kadakas-app","profile":""}"#,
        )
        .unwrap();
        assert_eq!(done_version(&root), 1, "welcome.json is not touched by it");
        rm_dir(&root);
    }

    #[test]
    fn the_old_key_is_read_and_the_mark_never_goes_down() {
        let root = tmp("mark-old");
        std::fs::write(crate::Settings::path(&root), r#"{"welcome_done":1}"#).unwrap();
        assert_eq!(done_version(&root), 1);
        write_mark(&root, 2, false).unwrap();
        write_mark(&root, 1, false).unwrap();
        assert_eq!(done_version(&root), 2);
        clear_mark(&root);
        assert_eq!(done_version(&root), 1, "the old key still counts");
        rm_dir(&root);
    }

    #[test]
    fn the_stamp_moves_with_every_stored_change() {
        let root = tmp("mark-stamp");
        write_mark(&root, 1, true).unwrap();
        let a = applied_stamp(&root);
        write_mark(&root, 1, true).unwrap();
        let b = applied_stamp(&root);
        assert!(!a.is_empty() && a != b, "{a} {b}");
        write_mark(&root, 1, false).unwrap();
        assert_eq!(applied_stamp(&root), b, "no change, no new stamp");
        rm_dir(&root);
    }
}
