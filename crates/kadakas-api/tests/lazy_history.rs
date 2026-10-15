//! The history of versions waits in the container file until a call uses it (DOC-21): opening is
//! cheap, a use reads and checks it against the drawing of the file, a save of a document that
//! was not touched copies the entries as they are, and a file that cannot be read is never
//! trimmed.

use std::path::{Path, PathBuf};

use kadakas_api::{PathPolicy, Session, files};
use kadakas_container::Container;
use serde_json::{Value, json};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kadakas-lazyhist-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::canonicalize(d).unwrap()
}

fn session(dir: &Path) -> Session {
    let mut s = Session::with_sequential_ids();
    s.paths = PathPolicy::restricted([dir.to_path_buf()]);
    s
}

fn rpc(s: &mut Session, method: &str, params: Value) -> Value {
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let text = if files::is_file_method(method) {
        files::handle_file_request(s, &req)
    } else {
        s.handle_line(&req.to_string()).unwrap()
    };
    serde_json::from_str(&text).unwrap()
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    let r = rpc(s, method, params);
    assert!(r.get("error").is_none(), "{method}: {r}");
    r["result"].clone()
}

fn circle(s: &mut Session, x: f64) {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [x, 0], "radius": 5}}}),
    );
}

/// A saved file with three versions and a picture.
fn make_file(dir: &Path, name: &str) -> PathBuf {
    let mut s = session(dir);
    for i in 0..3 {
        circle(&mut s, f64::from(i) * 20.0);
        ok(&mut s, "version.create", json!({"name": format!("v{i}")}));
    }
    circle(&mut s, 500.0);
    let f = dir.join(name);
    ok(&mut s, "file.save", json!({"path": f}));
    f
}

fn open(dir: &Path, f: &Path) -> Session {
    let mut s = session(dir);
    let r = ok(&mut s, "file.open", json!({"path": f}));
    assert_eq!(r["format"], "container");
    assert!(r.get("notes").is_none(), "{r}");
    s
}

fn names(s: &mut Session) -> Vec<String> {
    ok(s, "version.list", json!({"current": false}))["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect()
}

fn entry_hashes(f: &Path, prefix: &str) -> Vec<(String, String)> {
    let c = Container::from_bytes(std::fs::read(f).unwrap(), Default::default()).unwrap();
    c.entries()
        .iter()
        .filter(|e| e.name.starts_with(prefix))
        .map(|e| (e.name.clone(), kadakas_container::hex(&e.sha256)))
        .collect()
}

#[test]
fn the_history_waits_in_the_file_until_it_is_used() {
    let dir = tmp("waits");
    let f = make_file(&dir, "a.kad");
    assert!(
        !entry_hashes(&f, "history/").is_empty(),
        "the versions are entries of the file"
    );
    let mut s = open(&dir, &f);
    assert!(s.editor.doc().history.is_none());
    assert!(s.editor.doc().lazy_history.is_some(), "not read at open");
    // Drawing and selecting do not read it.
    ok(&mut s, "objects.list", json!({}));
    assert!(s.editor.doc().lazy_history.is_some());
    // The Versions window does.
    assert_eq!(names(&mut s), ["v0", "v1", "v2"]);
    assert!(s.editor.doc().history.is_some() && s.editor.doc().lazy_history.is_none());
    let list = ok(&mut s, "version.list", json!({}));
    assert_eq!(list["read_only"], false);
    assert_eq!(
        list["current_is_newest"], false,
        "the circle after the last version"
    );
    // A version of a state in the file, rebuilt from the file.
    let v = ok(&mut s, "version.get", json!({"id": 1, "document": true}));
    assert_eq!(v["things"]["objects"], 1, "{v}");
    // The load notes did not change because of it.
    assert!(s.load_notes().is_empty());
}

#[test]
fn a_save_of_an_untouched_document_copies_the_history_entries_as_they_are() {
    let dir = tmp("copy");
    let f = make_file(&dir, "a.kad");
    let mut s = open(&dir, &f);
    let g = dir.join("g.kad");
    ok(&mut s, "file.save", json!({"path": g}));
    assert!(
        s.editor.doc().lazy_history.is_some(),
        "the save did not read the history"
    );
    assert_eq!(entry_hashes(&f, "history/"), entry_hashes(&g, "history/"));
    assert_eq!(entry_hashes(&f, "blobs/"), entry_hashes(&g, "blobs/"));
    let mut t = open(&dir, &g);
    assert_eq!(names(&mut t), ["v0", "v1", "v2"]);
    // The same save over the file itself.
    ok(&mut s, "file.save", json!({"path": f}));
    assert!(s.editor.doc().lazy_history.is_some());
    assert_eq!(names(&mut open(&dir, &f)), ["v0", "v1", "v2"]);
}

#[test]
fn the_plain_json_form_needs_the_history_and_has_it() {
    let dir = tmp("json");
    let f = make_file(&dir, "a.kad");
    let mut s = open(&dir, &f);
    let j = dir.join("a.json");
    let r = ok(&mut s, "file.save", json!({"path": j}));
    assert_eq!(r["format"], "json");
    assert!(s.editor.doc().history.is_some(), "the save read it");
    let mut t = session(&dir);
    ok(&mut t, "file.open", json!({"path": j}));
    assert_eq!(names(&mut t), ["v0", "v1", "v2"]);
    // container -> json -> container gives the same versions.
    let k = dir.join("b.kad");
    ok(&mut t, "file.save", json!({"path": k}));
    let mut u = open(&dir, &k);
    assert_eq!(names(&mut u), ["v0", "v1", "v2"]);
    assert_eq!(
        u.editor.doc().history.clone().unwrap().to_value()["newest"]["anchor"],
        s.editor.doc().history.clone().unwrap().to_value()["newest"]["anchor"]
    );
}

#[test]
fn an_edit_before_the_first_use_reads_the_history_against_the_drawing_of_the_file() {
    let dir = tmp("edit");
    let f = make_file(&dir, "a.kad");
    let mut s = open(&dir, &f);
    circle(&mut s, 900.0);
    circle(&mut s, 950.0);
    // The drawing left the file's drawing: the history is read against the file's.
    assert_eq!(names(&mut s), ["v0", "v1", "v2"]);
    let list = ok(&mut s, "version.list", json!({}));
    assert_eq!(list["read_only"], false);
    assert!(s.load_notes().is_empty(), "{:?}", s.load_notes());
    // A new version and a save keep every version exact.
    ok(&mut s, "version.create", json!({"name": "v3"}));
    ok(&mut s, "file.save", json!({"path": f}));
    let mut t = open(&dir, &f);
    assert_eq!(names(&mut t), ["v0", "v1", "v2", "v3"]);
    assert_eq!(ok(&mut t, "version.list", json!({}))["read_only"], false);
}

#[test]
fn a_save_after_an_edit_reads_the_history_itself() {
    let dir = tmp("saveedit");
    let f = make_file(&dir, "a.kad");
    let mut s = open(&dir, &f);
    circle(&mut s, 700.0);
    // The history was never used: the save reads it, re-anchors it, and writes it.
    ok(&mut s, "file.save", json!({"path": f}));
    assert!(s.editor.doc().lazy_history.is_none());
    let mut t = open(&dir, &f);
    assert_eq!(names(&mut t), ["v0", "v1", "v2"]);
    let list = ok(&mut t, "version.list", json!({}));
    assert_eq!(list["read_only"], false);
    // The drawing has circles that no version has: the newest version is still v2.
    assert_eq!(list["current_is_newest"], false);
    assert!(t.load_notes().is_empty(), "{:?}", t.load_notes());
}

#[test]
fn a_file_that_changed_on_disk_keeps_the_versions_read_only_and_a_save_never_trims_them() {
    let dir = tmp("changed");
    let f = make_file(&dir, "a.kad");
    let mut s = open(&dir, &f);
    std::thread::sleep(std::time::Duration::from_millis(30));
    // Another program writes another file over it.
    let other = make_file(&dir, "other.kad");
    std::fs::copy(&other, &f).unwrap();
    let list = ok(&mut s, "version.list", json!({}));
    assert_eq!(list["read_only"], true, "{list}");
    assert_eq!(list["count"], 0);
    assert!(
        s.editor.doc().lazy_history.is_some(),
        "the versions are still in the file"
    );
    // No version can be made, restored or deleted, and the file cannot be saved without them.
    let r = rpc(&mut s, "version.create", json!({"name": "x"}));
    assert_eq!(r["error"]["data"]["reason"], "history_read_only", "{r}");
    circle(&mut s, 5.0);
    let g = dir.join("g.kad");
    let r = rpc(&mut s, "file.save", json!({"path": g}));
    assert_eq!(r["error"]["code"], 5, "{r}");
    assert!(!g.exists());
    // Only the explicit repair drops them.
    let r = rpc(&mut s, "version.repair", json!({}));
    assert!(r.get("error").is_some(), "confirm is needed");
    let r = ok(&mut s, "version.repair", json!({"confirm": true}));
    assert_eq!(r["kept"], 0);
    assert!(s.editor.doc().lazy_history.is_none() && s.editor.doc().history.is_none());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 0);
    // The pictures of the old file are gone with it; this document has none.
    ok(&mut s, "file.save", json!({"path": g}));
}

#[test]
fn a_damaged_history_entry_is_found_when_the_history_is_used() {
    let dir = tmp("damaged");
    let f = make_file(&dir, "a.kad");
    // The same file with a history entry that is not a list.
    let c = Container::from_bytes(std::fs::read(&f).unwrap(), Default::default()).unwrap();
    let mut w = kadakas_container::Writer::new(Vec::new());
    for e in c.entries() {
        if e.name == kadakas_file::HISTORY_VERSIONS {
            w.add_bytes(&e.name, kadakas_container::Compression::Stored, b"{}")
                .unwrap();
        } else {
            w.copy_raw(c.raw(&e.name).unwrap()).unwrap();
        }
    }
    let bad = dir.join("bad.kad");
    std::fs::write(&bad, w.finish().unwrap()).unwrap();
    let mut s = open(&dir, &bad);
    let list = ok(&mut s, "version.list", json!({}));
    assert_eq!(list["read_only"], true);
    assert!(
        list["notes"].to_string().contains("history/versions.json"),
        "the note names the entry: {list}"
    );
    // The untouched document can still be copied: the entries go over as they are, so nothing is
    // lost. With an edit they cannot be re-anchored, and the save is refused.
    let g = dir.join("g.kad");
    ok(&mut s, "file.save", json!({"path": g}));
    assert_eq!(entry_hashes(&bad, "history/"), entry_hashes(&g, "history/"));
    circle(&mut s, 9.0);
    let r = rpc(&mut s, "file.save", json!({"path": dir.join("h.kad")}));
    assert_eq!(r["error"]["code"], 5, "{r}");
}

#[test]
fn a_session_without_versions_has_nothing_that_waits() {
    let dir = tmp("none");
    let mut a = session(&dir);
    circle(&mut a, 1.0);
    let f = dir.join("a.kad");
    ok(&mut a, "file.save", json!({"path": f}));
    assert!(entry_hashes(&f, "history/").is_empty());
    let mut s = open(&dir, &f);
    assert!(s.editor.doc().lazy_history.is_none());
    assert_eq!(names(&mut s), Vec::<String>::new());
}
