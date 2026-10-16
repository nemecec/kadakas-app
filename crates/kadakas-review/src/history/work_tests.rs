//! Pictures in a history, and the work of loads and saves.

use kadakas_core::command::Editor;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_props::blob::{MAX_BLOB_BYTES, sha256_hex};
use serde_json::{Value, json};

use super::tests::{limits, through_file};
use super::*;
use crate::state::APPLIED;
use crate::testutil::{circle, cmd, editor, new_version};

/// A document with an underlay whose picture text is about `megabytes` MiB.
fn editor_with_underlay(megabytes: usize) -> (Editor, ObjectId) {
    let mut ed = editor();
    let id = add_underlay(&mut ed, megabytes);
    (ed, id)
}

/// Adds the underlay of `editor_with_underlay` to a document.
fn add_underlay(ed: &mut Editor, megabytes: usize) -> ObjectId {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut seed = 7u32;
    for _ in 0..megabytes * 1024 * 1024 * 3 / 4 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        bytes.push((seed >> 24) as u8);
    }
    let png = kadakas_core::underlay::base64_encode(&bytes);
    let out = ed
        .execute(&cmd(
            json!({"op": "add_object", "shape": {"kind": "underlay",
            "min": [0, 0], "max": [6400, 6400], "width": 64, "height": 64,
            "png": png, "opacity": 1.0}}),
        ))
        .unwrap();
    ObjectId(out.created[0])
}

#[test]
fn a_picture_is_stored_once_however_many_versions_move_it() {
    // The first version is the anchor: the picture is added after it, so the history holds it.
    let mut ed = editor();
    let mut h = History::new();
    h.add(ed.doc(), new_version("start"), Limits::default())
        .unwrap();
    let id = add_underlay(&mut ed, 10);
    let one = h
        .add(ed.doc(), new_version("with picture"), Limits::default())
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
    // Five versions, one picture of 10 MiB: far under the default cap of 64 MiB.
    assert_eq!(h.len(), 5);
    assert_eq!(h.blobs().len(), 1);
    assert!(
        h.bytes() > 10 * 1024 * 1024 && h.bytes() < 11 * 1024 * 1024,
        "{}",
        h.bytes()
    );
    assert!(h.bytes() <= Limits::default().max_bytes);
    for v in h.versions().take(4) {
        assert!(h.delta_bytes(v.id).unwrap() < 2000);
    }
    // Every version builds the document with the picture.
    for (v, state) in &states {
        assert_eq!(&h.state_of(*v).unwrap(), state);
        let built = h.state_of(*v).unwrap().build(ed.doc());
        assert!(built.quarantined.is_empty(), "{:?}", built.quarantined);
        let Shape::Underlay(u) = &built.doc.objects.values().next().unwrap().shape else {
            panic!("an underlay")
        };
        assert!(u.png.len() > 7 * 1024 * 1024);
    }
    // The memory form round trips; the file form leaves out the picture that the drawing holds.
    let again = History::load_with(&h.to_value(), ed.doc(), &[h.anchor_pair()]);
    assert!(again.notes.is_empty(), "{:?}", again.notes);
    assert_eq!(again.history.unwrap(), h);
    let (value, read) = through_file(&h, ed.doc());
    assert!(read.notes.is_empty(), "{:?}", read.notes);
    let read = read.history.unwrap();
    for m in h.versions() {
        assert_eq!(read.state_of(m.id).unwrap(), h.state_of(m.id).unwrap());
    }
    assert!(value["blobs"].as_object().unwrap().is_empty());
    assert!(
        value.to_string().len() < 20_000,
        "the file form is {} bytes",
        value.to_string().len()
    );
    // A picture stays while a version uses it.
    let mut fold = h.clone();
    fold.remove(3).unwrap();
    assert_eq!(fold.blobs().len(), 1);
    let mut gone = History::new();
    let plain = editor();
    gone.add(plain.doc(), new_version("plain"), Limits::default())
        .unwrap();
    gone.add(ed.doc(), new_version("picture"), Limits::default())
        .unwrap();
    assert_eq!(gone.blobs().len(), 1);
    gone.remove(1).unwrap();
    assert_eq!(
        gone.blobs().len(),
        1,
        "the newest version holds the picture"
    );
    gone.remove(2).unwrap();
    assert!(gone.is_empty() && gone.blobs().is_empty());
}

#[test]
fn deleting_the_only_version_with_a_picture_frees_it() {
    let (mut ed, id) = editor_with_underlay(6);
    let mut h = History::new();
    h.add(ed.doc(), new_version("with picture"), Limits::default())
        .unwrap();
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [id]})))
        .unwrap();
    h.add(ed.doc(), new_version("without"), Limits::default())
        .unwrap();
    // Version 1 holds the picture as a reverse delta.
    assert_eq!(h.blobs().len(), 1);
    let sizes = h.infos();
    assert!(sizes[0].bytes > 6 * 1024 * 1024, "{sizes:?}");
    assert!(sizes[1].bytes < 1000);
    h.remove(1).unwrap();
    assert!(h.blobs().is_empty());
    assert!(h.bytes() < 1000);
}

#[test]
fn pictures_in_a_file_are_checked() {
    let mut ed = editor();
    let mut h = History::new();
    h.add(ed.doc(), new_version("one"), Limits::default())
        .unwrap();
    add_underlay(&mut ed, 1);
    h.add(ed.doc(), new_version("picture"), Limits::default())
        .unwrap();
    let known = [h.anchor_pair()];
    let load = |v: &Value, d: &Document| History::load_with(v, d, &known);
    let good = h.to_value();
    assert!(load(&good, ed.doc()).notes.is_empty());
    // A picture that does not match its hash is left out, and the newest version then lacks it.
    let mut bad = good.clone();
    let hash = bad["blobs"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    bad["blobs"][&hash] = json!("tampered");
    let l = load(&bad, ed.doc());
    assert!(l.history.is_none());
    assert!(l.notes.iter().any(|n| n.contains("hash")), "{:?}", l.notes);
    assert!(
        l.notes.iter().any(|n| n.contains("missing")),
        "{:?}",
        l.notes
    );
    let mut bad = good.clone();
    bad["blobs"][&hash] = json!(5);
    assert!(load(&bad, ed.doc()).history.is_none());
    let mut bad = good.clone();
    bad.as_object_mut().unwrap().remove("blobs");
    assert!(load(&bad, ed.doc()).history.is_none());
    // A picture that no version uses is dropped when the file is read.
    let mut extra = good.clone();
    let text = "extra picture";
    extra["blobs"][sha256_hex(text.as_bytes())] = json!(text);
    assert_eq!(load(&extra, ed.doc()).history.unwrap().blobs().len(), 1);
    // A delta that needs a picture that the table lacks leaves that version and older ones out.
    let mut two = h.clone();
    two.add(ed.doc(), new_version("two"), Limits::default())
        .unwrap();
    let mut v = two.to_value();
    v["versions"][0]["delta"] = json!([{"op": "create",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000aa"},
        "props": {"shape": {"kind": "underlay", "png": {"$blob": "ab".repeat(32)}}}}]);
    let l = load(&v, ed.doc());
    assert_eq!(l.history.unwrap().len(), 2);
    assert!(l.notes.iter().any(|n| n.contains("missing")));
    // A picture above the largest blob is left out.
    let mut huge = good;
    huge["blobs"]["00"] = json!("x".repeat(MAX_BLOB_BYTES + 1));
    assert!(
        load(&huge, ed.doc())
            .notes
            .iter()
            .any(|n| n.contains("too large"))
    );
}

fn hostile_file(snapshot: u64, versions: u64) -> Value {
    let id = |n: u64| format!("00000000-0000-0000-0000-{n:012x}");
    let ops: Vec<Value> = (1..=snapshot)
        .map(|n| json!({"op": "create", "target": {"kind": "object", "id": id(n)}, "props": {"n": n}}))
        .collect();
    let list: Vec<Value> = (1..=versions)
        .map(|v| {
            let mut m = json!({"id": v, "name": format!("v{v}"), "author": "a"});
            if v > 1 {
                m["parents"] = json!([v - 1]);
            }
            if v < versions {
                m["via"] = json!(v + 1);
                m["delta"] = json!([{"op": "set", "prop": "n", "value": v,
                    "target": {"kind": "object", "id": id(1 + v % snapshot)}}]);
            }
            m
        })
        .collect();
    json!({"format": 2, "next_id": versions + 1, "head": versions,
        "newest": {"id": versions, "delta": ops,
        "anchor": anchor_digest(&State::of(&Document::default(), None))}, "versions": list})
}

/// A large newest version and many tiny versions: one pass over the file, nothing is dropped.
#[test]
fn the_hostile_shape_of_a_big_drawing_and_many_tiny_versions_is_one_pass() {
    const BIG: u64 = 3000;
    const VERSIONS: u64 = 400;
    let file = hostile_file(BIG, VERSIONS);
    let doc = Document::default();
    APPLIED.with(|c| c.set(0));
    let l = History::load(&file, &doc);
    let work = APPLIED.with(std::cell::Cell::get) as u64;
    let h = l.history.unwrap();
    assert_eq!(h.len(), VERSIONS as usize, "no version is lost");
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    // The snapshot and each delta once.
    assert!(work <= BIG + VERSIONS + 100, "{work} operations applied");
    // Opening the oldest version replays the snapshot and every delta once.
    APPLIED.with(|c| c.set(0));
    h.state_of(1).unwrap();
    assert!(APPLIED.with(std::cell::Cell::get) as u64 <= BIG + VERSIONS);
    // The limits do not change the reading.
    let strict = limits(20, 16 * 1024 * 1024);
    let ed = editor();
    let mut over = h;
    let err = over
        .add(ed.doc(), new_version("x"), strict)
        .expect_err("over the limit");
    assert!(matches!(err, ReviewError::LimitReached(_)));
    assert_eq!(over.len(), VERSIONS as usize);
}

/// A save makes the newest version again from the drawing: the work is the size of the drawing,
/// not of the history.
#[test]
fn a_save_and_a_save_after_many_versions_cost_the_drawing() {
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
    let tip = h.tip_state().unwrap().unwrap();
    APPLIED.with(|c| c.set(0));
    let _ = h.to_disk(ed.doc(), &tip);
    assert_eq!(
        APPLIED.with(std::cell::Cell::get),
        0,
        "a save replays nothing"
    );
    // A new version makes the old newest a delta: the snapshot is applied once.
    APPLIED.with(|c| c.set(0));
    h.add(ed.doc(), new_version("last"), Limits::default())
        .unwrap();
    let work = APPLIED.with(std::cell::Cell::get);
    assert!(work < 2 * (200 + 10), "{work}");
}
