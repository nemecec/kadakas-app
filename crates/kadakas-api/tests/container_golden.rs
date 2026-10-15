//! Committed containers (DOC-20) made by an earlier build must read with no notes and give the
//! same content as the plain JSON files of `versions_golden.rs`. This guards the layout of the
//! container (the names of the entries, the manifest, the references of the pictures, the parts of
//! the history) against drift: a change that makes a test fail here would make the files that
//! people have saved unreadable. Fix the reading, do not just make the files again.
//!
//! The JSON goldens are the plain form (DOC-22), so they also guard that today's format keeps
//! loading. To make the containers again on purpose (a new version of the layout):
//!
//! ```text
//! cargo test -p kadakas-api --test container_golden make_the_files -- --ignored
//! ```

use std::path::PathBuf;

use kadakas_api::{Session, files};
use kadakas_container::Container;
use kadakas_file::{Form, ReadMode};
use serde_json::{Value, json};

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

const PAIRS: [(&str, &str); 3] = [
    ("versions_golden.kad", "container_golden.kad"),
    (
        "versions_golden_picture.kad",
        "container_golden_picture.kad",
    ),
    ("versions_golden_base.kad", "container_golden_base.kad"),
];

fn call(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn open_json(name: &str) -> Session {
    let o = files::open_document(&path(name), u64::MAX, ReadMode::Lazy).unwrap();
    assert_eq!(o.form, Form::Json, "{name} is the plain form");
    Session::with_document(o.doc)
}

fn open_container(name: &str) -> Session {
    let o = files::open_document(&path(name), u64::MAX, ReadMode::Lazy).unwrap();
    assert_eq!(o.form, Form::Container, "{name} is a container");
    let mut s = Session::with_document(o.doc);
    s.file_source = o.source;
    s
}

/// Makes the containers again (see the top of this file). Not part of a normal run.
#[test]
#[ignore = "makes the committed files again"]
fn make_the_files() {
    for (json_name, container_name) in PAIRS {
        let mut s = open_json(json_name);
        let doc = s.document_for_save_in(true).unwrap().into_owned();
        files::write_document(&path(container_name), &doc, &files::SaveSpec::container()).unwrap();
    }
}

fn entry_names(name: &str) -> Vec<String> {
    let c = Container::from_bytes(std::fs::read(path(name)).unwrap(), Default::default()).unwrap();
    c.entries().iter().map(|e| e.name.clone()).collect()
}

#[test]
fn the_committed_containers_have_the_documented_layout() {
    let names = entry_names("container_golden.kad");
    assert_eq!(
        names,
        [
            "document.json",
            "history/top.json",
            "history/versions.json",
            "history/newest.json",
            "history/blobs.json"
        ]
    );
    let names = entry_names("container_golden_picture.kad");
    // The picture of the history is one blob, named by the SHA-256 of its text.
    assert_eq!(
        names.iter().filter(|n| n.starts_with("blobs/")).count(),
        1,
        "{names:?}"
    );
    assert_eq!(names[0], "document.json");
    let c = Container::from_bytes(
        std::fs::read(path("container_golden.kad")).unwrap(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(c.manifest().format, "kadakas-container");
    assert_eq!(c.manifest().format_version, 1);
}

#[test]
fn the_containers_give_the_same_versions_as_the_plain_files() {
    for (json_name, container_name) in PAIRS {
        let mut a = open_json(json_name);
        let mut b = open_container(container_name);
        assert!(
            b.load_notes().is_empty(),
            "{container_name}: {:?}",
            b.load_notes()
        );
        let la = call(&mut a, "version.list", json!({}));
        let lb = call(&mut b, "version.list", json!({}));
        assert!(
            b.load_notes().is_empty(),
            "{container_name}: {:?}",
            b.load_notes()
        );
        assert_eq!(lb["read_only"], false, "{container_name}: {lb}");
        assert_eq!(la["count"], lb["count"], "{container_name}");
        for id in 1..=la["count"].as_u64().unwrap() {
            let x = call(&mut a, "version.get", json!({"id": id, "document": true}));
            let y = call(&mut b, "version.get", json!({"id": id, "document": true}));
            assert_eq!(
                x["document"], y["document"],
                "{container_name} version {id}"
            );
        }
        let d = call(&mut b, "version.diff", json!({"from": 1, "to": "current"}));
        assert_eq!(d["exact"], true, "{container_name}");
    }
}

#[test]
fn a_container_with_versions_saves_and_reads_again_with_no_notes() {
    let mut s = open_container("container_golden.kad");
    let dir = std::env::temp_dir().join(format!("kadakas-container-golden-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("again.kad");
    // An edit and a save: the versions are read, written against the new drawing, and read again.
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [7, 7], "radius": 2}}}),
    );
    let doc = s.document_for_save_in(true).unwrap().into_owned();
    files::write_document(&out, &doc, &files::SaveSpec::container()).unwrap();
    let mut back = Session::with_document(
        files::open_document(&out, u64::MAX, ReadMode::Lazy)
            .unwrap()
            .doc,
    );
    assert_eq!(call(&mut back, "version.list", json!({}))["count"], 3);
    assert!(back.load_notes().is_empty(), "{:?}", back.load_notes());
    let _ = std::fs::remove_dir_all(dir);
}
