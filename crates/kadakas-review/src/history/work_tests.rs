//! Pictures in a history, and the work of folds and loads.

use kadakas_core::command::Editor;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_props::blob::{MAX_BLOB_BYTES, sha256_hex};
use serde_json::{Value, json};

use super::*;
use crate::state::APPLIED;
use crate::testutil::{circle, cmd, editor, new_version};

fn limits(max_versions: usize, max_bytes: usize) -> Limits {
    Limits {
        max_versions,
        max_bytes,
    }
}

fn load(v: &Value) -> Loaded {
    History::load(v, Limits::default())
}

/// A document with an underlay whose picture text is about `megabytes` MiB.
fn editor_with_underlay(megabytes: usize) -> (Editor, ObjectId) {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut seed = 7u32;
    for _ in 0..megabytes * 1024 * 1024 * 3 / 4 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        bytes.push((seed >> 24) as u8);
    }
    let png = kadakas_core::underlay::base64_encode(&bytes);
    let mut ed = editor();
    let out = ed
        .execute(&cmd(
            json!({"op": "add_object", "shape": {"kind": "underlay",
            "min": [0, 0], "max": [6400, 6400], "width": 64, "height": 64,
            "png": png, "opacity": 1.0}}),
        ))
        .unwrap();
    (ed, ObjectId(out.created[0]))
}

#[test]
fn a_picture_is_stored_once_however_many_versions_move_it() {
    let (mut ed, id) = editor_with_underlay(10);
    let mut h = History::new();
    let one = h
        .add(ed.doc(), new_version("start"), Limits::default())
        .unwrap()
        .id;
    let mut states = vec![(one, h.state_of(one).unwrap())];
    for i in 0..3 {
        ed.execute(&cmd(
            json!({"op": "move_objects", "ids": [id], "delta": [100 * (i + 1), 0]}),
        ))
        .unwrap();
        let v = h
            .add(
                ed.doc(),
                new_version(&format!("moved {i}")),
                Limits::default(),
            )
            .unwrap()
            .id;
        states.push((v, h.state_of(v).unwrap()));
    }
    // Four versions, one picture of 10 MiB: far under the default cap of 64 MiB.
    assert_eq!(h.len(), 4);
    assert_eq!(h.blobs().len(), 1);
    assert!(
        h.bytes() > 10 * 1024 * 1024 && h.bytes() < 11 * 1024 * 1024,
        "{}",
        h.bytes()
    );
    assert!(h.bytes() <= Limits::default().max_bytes);
    for v in h.versions().skip(1) {
        assert!(h.delta_bytes(v.id).unwrap() < 2000);
    }
    // Every version builds the document with the picture, and the file round trips.
    for (v, state) in &states {
        assert_eq!(&h.state_of(*v).unwrap(), state);
        let built = h.state_of(*v).unwrap().build(ed.doc());
        assert!(built.quarantined.is_empty(), "{:?}", built.quarantined);
        let Shape::Underlay(u) = &built.doc.objects.values().next().unwrap().shape else {
            panic!("an underlay")
        };
        assert!(u.png.0.len() > 7 * 1024 * 1024);
    }
    let again = load(&h.to_value());
    assert!(again.notes.is_empty(), "{:?}", again.notes);
    assert_eq!(again.history.unwrap(), h);
    // The count of references: a picture stays while a version uses it.
    let mut fold = h.clone();
    fold.remove(2).unwrap();
    assert_eq!(fold.blobs().len(), 1);
    let mut gone = History::new();
    let plain = editor();
    gone.add(plain.doc(), new_version("plain"), Limits::default())
        .unwrap();
    gone.add(ed.doc(), new_version("picture"), Limits::default())
        .unwrap();
    assert_eq!(gone.blobs().len(), 1);
    gone.remove(1).unwrap();
    assert_eq!(gone.blobs().len(), 1, "the base now holds the picture");
    gone.remove(2).unwrap();
    assert!(gone.is_empty() && gone.blobs().is_empty());
}

#[test]
fn folding_frees_a_picture_that_no_kept_version_uses() {
    let (mut ed, id) = editor_with_underlay(6);
    let mut h = History::new();
    h.add(ed.doc(), new_version("with picture"), Limits::default())
        .unwrap();
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [id]})))
        .unwrap();
    h.add(ed.doc(), new_version("without"), Limits::default())
        .unwrap();
    assert_eq!(h.blobs().len(), 1);
    let before = h.bytes();
    let added = h
        .add(ed.doc(), new_version("again"), limits(1, usize::MAX))
        .unwrap();
    assert_eq!(added.dropped.len(), 2);
    assert!(h.blobs().is_empty());
    assert!(h.bytes() < before / 10, "{} vs {before}", h.bytes());
}

#[test]
fn pictures_in_a_file_are_checked() {
    let (ed, _) = editor_with_underlay(1);
    let mut h = History::new();
    h.add(ed.doc(), new_version("one"), Limits::default())
        .unwrap();
    let good = h.to_value();
    assert!(load(&good).notes.is_empty());
    // A picture that does not match its hash is left out, and the base then lacks it.
    let mut bad = good.clone();
    let hash = bad["blobs"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    bad["blobs"][&hash] = json!("tampered");
    let l = load(&bad);
    assert!(l.history.is_none());
    assert!(l.notes.iter().any(|n| n.contains("hash")), "{:?}", l.notes);
    assert!(
        l.notes.iter().any(|n| n.contains("missing")),
        "{:?}",
        l.notes
    );
    let mut bad = good.clone();
    bad["blobs"][&hash] = json!(5);
    assert!(load(&bad).history.is_none());
    let mut bad = good.clone();
    bad.as_object_mut().unwrap().remove("blobs");
    assert!(load(&bad).history.is_none());
    // A picture that no version uses is dropped when the file is read.
    let mut extra = good.clone();
    let text = "extra picture";
    extra["blobs"][sha256_hex(text.as_bytes())] = json!(text);
    assert_eq!(load(&extra).history.unwrap().blobs().len(), 1);
    // A delta that needs a picture that the table lacks drops that version and the later ones.
    let mut two = h.clone();
    two.add(ed.doc(), new_version("two"), Limits::default())
        .unwrap();
    let mut v = two.to_value();
    v["versions"][1]["delta"] = json!([{"op": "create",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000aa"},
        "props": {"shape": {"kind": "underlay", "png": {"$blob": "ab".repeat(32)}}}}]);
    let l = load(&v);
    assert_eq!(l.history.unwrap().len(), 1);
    assert!(l.notes.iter().any(|n| n.contains("missing")));
    // A picture above the largest blob is left out.
    let mut huge = good;
    huge["blobs"]["00"] = json!("x".repeat(MAX_BLOB_BYTES + 1));
    assert!(load(&huge).notes.iter().any(|n| n.contains("too large")));
}

fn hostile_file(base: u64, versions: u64) -> Value {
    let id = |n: u64| format!("00000000-0000-0000-0000-{n:012x}");
    let base_ops: Vec<Value> = (1..=base)
        .map(|n| json!({"op": "create", "target": {"kind": "object", "id": id(n)}, "props": {"n": n}}))
        .collect();
    let list: Vec<Value> = (1..=versions)
        .map(|v| {
            let mut m = json!({"id": v, "name": format!("v{v}"), "author": "a"});
            if v > 1 {
                m["delta"] = json!([{"op": "set", "prop": "n", "value": v,
                    "target": {"kind": "object", "id": id(1 + v % base)}}]);
            }
            m
        })
        .collect();
    json!({"format": 1, "next_id": versions + 1, "base": base_ops, "versions": list})
}

/// A large base and many tiny versions under strict limits: the oldest versions fold in one pass,
/// so the work is about the size of the file, not versions times base.
#[test]
fn the_hostile_shape_of_a_big_base_and_many_tiny_versions_is_one_pass() {
    const BASE: u64 = 3000;
    const VERSIONS: u64 = 400;
    let file = hostile_file(BASE, VERSIONS);
    let strict = limits(20, 16 * 1024 * 1024);
    APPLIED.with(|c| c.set(0));
    let l = History::load(&file, strict);
    let work = APPLIED.with(std::cell::Cell::get) as u64;
    let h = l.history.unwrap();
    assert_eq!(h.len(), 20);
    assert!(
        l.notes.iter().any(|n| n.contains("folded")),
        "{:?}",
        l.notes
    );
    // One replay to check the file and one to fold. One version at a time would be 380 times
    // the base.
    assert!(
        work <= 2 * (BASE + VERSIONS) + 100,
        "{work} operations applied"
    );
    // The result needs no fold when it is read again.
    APPLIED.with(|c| c.set(0));
    let again = History::load(&h.to_value(), strict);
    assert_eq!(again.history.unwrap(), h);
    assert!(APPLIED.with(std::cell::Cell::get) as u64 <= BASE + 30);
    // A base above the byte cap is dropped before any version is replayed.
    APPLIED.with(|c| c.set(0));
    let l = History::load(&file, limits(20, 10_000));
    assert!(l.history.is_none());
    assert!(l.notes[0].contains("limit"), "{:?}", l.notes);
    assert_eq!(APPLIED.with(std::cell::Cell::get), 0);
}

/// The same for `add`: a cap of two versions after many versions folds in one pass.
#[test]
fn add_folds_many_versions_in_one_pass() {
    let mut ed = editor();
    for i in 0..200 {
        circle(&mut ed, f64::from(i));
    }
    let mut h = History::new();
    for i in 0..30 {
        let first = *ed.doc().objects.keys().next().unwrap();
        ed.execute(&cmd(
            json!({"op": "move_objects", "ids": [first], "delta": [f64::from(i), 0.0]}),
        ))
        .unwrap();
        h.add(ed.doc(), new_version("v"), Limits::default())
            .unwrap();
    }
    APPLIED.with(|c| c.set(0));
    let added = h
        .add(ed.doc(), new_version("last"), limits(2, usize::MAX))
        .unwrap();
    assert_eq!(added.dropped.len(), 29);
    let work = APPLIED.with(std::cell::Cell::get);
    assert!(work < 3 * (200 + 30 + 10), "{work}");
}
