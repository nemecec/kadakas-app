//! The limit of a save and the cap of a read are one measure: a file within the limits is never
//! read only.

use super::*;
use crate::testutil::{circle, editor, new_version};

#[test]
fn a_history_at_the_limit_is_read_completely_by_a_reader_with_the_production_ratio() {
    // The same ratio between what a save allows and what a read allows as in production
    // (`WRITE_MAX_BYTES` and `HARD_MAX_BYTES`), at a size a test can reach.
    let write_max = 60_000usize;
    let limits = Limits {
        max_versions: 1000,
        max_bytes: write_max,
    };
    let caps = ReadCaps {
        max_versions: HARD_MAX_VERSIONS,
        max_bytes: write_max * (HARD_MAX_BYTES / WRITE_MAX_BYTES),
    };
    assert_eq!(HARD_MAX_BYTES, 2 * WRITE_MAX_BYTES);
    let mut ed = editor();
    let mut h = History::new();
    let mut anchor = None;
    let mut i = 0;
    loop {
        circle(&mut ed, f64::from(i));
        match h.add(ed.doc(), new_version(&format!("v{i}")), limits) {
            Ok(_) => {
                anchor.get_or_insert_with(|| ed.doc().clone());
                i += 1;
            }
            Err(ReviewError::LimitReached(info)) => {
                assert_eq!(info.kind, LimitKind::Bytes);
                break;
            }
            Err(e) => panic!("{e}"),
        }
        assert!(i < 500, "the limit was never reached");
    }
    assert!(i > 20, "{i} versions fit");
    let anchor = anchor.unwrap();
    // The memory form and the file form both read completely.
    let json = h.to_value();
    let l = History::load_with_caps(&json, &anchor, &[], caps);
    assert!(l.notes.is_empty() && l.info.is_empty(), "{:?}", l.notes);
    assert_eq!(l.history.unwrap().len(), h.len());
    let (file, state) = {
        let tip = h.tip_state().unwrap().unwrap();
        h.to_disk(ed.doc(), &tip)
    };
    let mut saved = ed.doc().clone();
    state.stamp_keys(&mut saved);
    let l = History::load_with_caps(&file, &saved, &[], caps);
    assert!(l.notes.is_empty() && l.info.is_empty(), "{:?}", l.notes);
    // The text is far above the estimate of the limit, and the guard of a reader allows for it.
    assert!(
        json.to_string().len() <= caps.max_bytes * JSON_FACTOR,
        "{} bytes of text for a limit of {write_max}",
        json.to_string().len()
    );
    // The estimate counts the newest change too, the way a reader does.
    assert!(h.bytes() <= write_max);
}
