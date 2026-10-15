//! `import.update` and `import.apply` (IOP-23): an architect sends a new revision of a drawing, and
//! the document is updated from it. The files are written to a private temporary folder.

use kadakas_api::{PathPolicy, RpcError, Session, SessionLimits};
use kadakas_core::provenance::{self, LayerSource};
use serde_json::{Value, json};

const GUID: &str = "A1B2C3D4-0001";

fn file(version: &str, blocks: &str, ents: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$FINGERPRINTGUID\n2\n{GUID}\n9\n$VERSIONGUID\n2\n{version}\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(handle: u32, layer: &str, y: i64) -> String {
    format!("0\nLINE\n5\n{handle:X}\n8\n{layer}\n10\n0\n20\n{y}\n11\n1000\n21\n{y}\n")
}

fn base_text() -> String {
    file(
        "AAAA0001",
        "",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 100),
            line(0x12, "A", 200),
            line(0x13, "A", 300),
            line(0x14, "A", 400),
        ]
        .concat(),
    )
}

fn revision_two() -> String {
    file(
        "AAAA0002",
        "",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 150),
            line(0x12, "B", 200),
            line(0x14, "A", 400),
            line(0x15, "A", 500),
        ]
        .concat(),
    )
}

fn temp(name: &str) -> kadakas_sys::tmp::PrivateTempDir {
    kadakas_sys::tmp::PrivateTempDir::new(name).unwrap()
}

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> RpcError {
    s.handle(method, p)
        .expect_err(&format!("{method} should fail"))
}

/// A session that has imported `text` as plan.dxf in a private folder.
fn session_with(dir: &std::path::Path, text: &str, options: Value) -> Session {
    let mut s = Session::with_sequential_ids();
    let path = dir.join("plan.dxf");
    std::fs::write(&path, text).unwrap();
    ok(
        &mut s,
        "import.file",
        json!({"path": path, "options": options}),
    );
    s
}

fn write(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn ys(s: &Session) -> Vec<i64> {
    let mut v: Vec<i64> = s
        .editor
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            kadakas_core::model::Shape::Line { a, .. } => Some(a.y as i64),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

fn version_names(s: &mut Session) -> Vec<String> {
    ok(s, "version.list", json!({"current": false}))["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect()
}

fn change_for<'a>(r: &'a Value, handle: &str) -> &'a Value {
    r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["handle"] == handle)
        .unwrap_or_else(|| panic!("no change for {handle}: {r}"))
}

#[test]
fn an_update_shows_the_changes_and_applies_them_in_one_undo_step() {
    let dir = temp("kadakas-update-flow");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let before = s.editor.doc().clone();
    let steps = s.editor.undo_depth();

    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(r["counts"]["unchanged"], 2);
    assert_eq!(r["counts"]["changed"], 2);
    assert_eq!(r["counts"]["added"], 1);
    assert_eq!(r["counts"]["removed"], 1);
    assert_eq!(r["counts"]["conflicts"], 0);
    assert_eq!(r["old_from"], "base");
    assert_eq!(r["name"], "plan_rev2.dxf");
    assert_eq!(change_for(&r, "11")["status"], "changed");
    assert_eq!(change_for(&r, "11")["what"], json!(["geometry"]));
    assert_eq!(change_for(&r, "12")["what"], json!(["layer"]));
    assert_eq!(change_for(&r, "13")["status"], "removed");
    assert_eq!(change_for(&r, "15")["status"], "added");
    let id = r["update_id"].as_str().unwrap().to_string();
    // Nothing changed yet: not the document, not the undo steps.
    assert_eq!(s.editor.doc(), &before);
    assert_eq!(s.editor.undo_depth(), steps);

    let applied = ok(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": "all"}),
    );
    assert_eq!(applied["applied"]["added"], 1);
    assert_eq!(applied["applied"]["replaced"], 2);
    assert_eq!(applied["applied"]["removed"], 1);
    assert_eq!(ys(&s), vec![0, 150, 200, 400, 500]);
    // One undo step for the whole update (the versions are part of it).
    assert_eq!(s.editor.undo_depth(), steps + 1);
    assert_eq!(
        version_names(&mut s),
        vec![
            "Before update to plan_rev2.dxf",
            "After update to plan_rev2.dxf"
        ]
    );
    assert_eq!(applied["versions"]["before"], 1);
    assert_eq!(applied["versions"]["after"], 2);
    // The "before" version is the drawing as it was, the "after" the new one.
    // The diff between the versions lists what the update changed, and nothing else.
    let d = ok(
        &mut s,
        "version.diff",
        json!({"from": 1, "to": 2, "kinds": ["objects"]}),
    );
    assert_eq!(d["counts"]["objects"]["added"], 1);
    assert_eq!(d["counts"]["objects"]["removed"], 1);
    assert_eq!(d["counts"]["objects"]["changed"], 2);
    let d = ok(
        &mut s,
        "version.diff",
        json!({"from": 2, "to": "current", "kinds": ["objects"]}),
    );
    assert_eq!(d["totals"]["added"], 0);
    assert_eq!(d["totals"]["removed"], 0);
    assert_eq!(d["totals"]["changed"], 0);

    // The layer source and the tags are at the new revision.
    let revisions: Vec<String> = s
        .editor
        .doc()
        .layers
        .iter()
        .filter_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .map(|l| l.revision)
        .collect();
    assert_eq!(revisions, vec![applied["revision"].as_str().unwrap()]);
    assert!(s.editor.doc().objects.values().all(|o| {
        let t = provenance::tag(&o.data).unwrap();
        let moved = matches!(t.handle.as_deref(), Some("11" | "12" | "15"));
        (t.revision == applied["revision"]) == moved && provenance::is_edited(o) == Some(false)
    }));
    // The update was used up.
    let again = fail(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": "all"}),
    );
    assert_eq!(again.kind, "conflict");

    // Undo takes the update back, with both versions. Redo brings it again.
    assert!(s.editor.undo());
    assert_eq!(s.editor.doc().objects, before.objects);
    assert_eq!(s.editor.doc().layers, before.layers);
    assert!(version_names(&mut s).is_empty());
    assert!(s.editor.redo());
    assert_eq!(ys(&s), vec![0, 150, 200, 400, 500]);
    assert_eq!(version_names(&mut s).len(), 2);

    // The next update goes from the new revision.
    let third = file(
        "AAAA0003",
        "",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 175),
            line(0x12, "B", 200),
            line(0x14, "A", 400),
            line(0x15, "A", 500),
        ]
        .concat(),
    );
    let rev3 = write(dir.path(), "plan_rev3.dxf", &third);
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": rev3, "dry_run": true}),
    );
    assert_eq!(r["counts"]["changed"], 1);
    assert_eq!(r["from_revision"], applied["revision"]);
}

#[test]
fn a_dry_run_keeps_nothing_and_the_pending_update_expires_on_an_edit() {
    let dir = temp("kadakas-update-expiry");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": rev2, "dry_run": true}),
    );
    assert!(r.get("update_id").is_none());
    assert_eq!(r["dry_run"], true);
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": "u1", "accept": "all"}),
    );
    assert_eq!(e.kind, "conflict");

    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    let id = r["update_id"].as_str().unwrap().to_string();
    assert!(s.update_pending().is_some());
    // Any edit expires it, also one that has nothing to do with the drawing.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [9000, 9000], "radius": 20}}}),
    );
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": "all"}),
    );
    assert_eq!(e.kind, "conflict");
    assert!(e.message.contains("import.update"), "{}", e.message);
    assert!(s.update_pending().is_none());
    assert_eq!(ys(&s), vec![0, 100, 200, 300, 400]);

    // A new update replaces the pending one; a wrong ID or change ID is told.
    let first = ok(&mut s, "import.update", json!({"path": rev2}));
    let second = ok(&mut s, "import.update", json!({"path": rev2}));
    assert_ne!(first["update_id"], second["update_id"]);
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": first["update_id"], "accept": "all"}),
    );
    assert_eq!(e.kind, "not_found");
    let id = second["update_id"].clone();
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": ["nope"]}),
    );
    assert_eq!(e.kind, "invalid_params");
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": 3}),
    );
    assert_eq!(e.kind, "invalid_params");
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": "all", "conflicts": "maybe"}),
    );
    assert_eq!(e.kind, "invalid_params");
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": "all", "reject": ["nope"]}),
    );
    assert_eq!(e.kind, "invalid_params");
    // Loading another document drops it too.
    ok(
        &mut s,
        "document.load",
        json!({"document": ok(&mut Session::with_sequential_ids(), "document.get", json!({}))}),
    );
    assert!(s.update_pending().is_none());
}

#[test]
fn a_partial_apply_takes_the_accepted_changes_only() {
    let dir = temp("kadakas-update-partial");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    let moved = change_for(&r, "11")["id"].clone();
    let removed = change_for(&r, "13")["id"].clone();
    let id = r["update_id"].clone();
    // Accept the move and the removal, and reject the removal again.
    let a = ok(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": [moved, removed], "reject": [removed]}),
    );
    assert_eq!(a["accepted"], 1);
    assert_eq!(a["applied"]["replaced"], 1);
    assert_eq!(a["applied"]["removed"], 0);
    assert_eq!(a["applied"]["added"], 0);
    assert_eq!(ys(&s), vec![0, 150, 200, 300, 400]);

    // "all" with a reject.
    let mut t = session_with(dir.path(), &base_text(), json!({}));
    let r = ok(&mut t, "import.update", json!({"path": rev2}));
    let added = change_for(&r, "15")["id"].clone();
    ok(
        &mut t,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all", "reject": [added]}),
    );
    assert_eq!(ys(&t), vec![0, 150, 200, 400]);
}

#[test]
fn an_object_the_person_edited_is_a_conflict_and_is_kept_unless_accepted() {
    let dir = temp("kadakas-update-conflict");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    // The person moves the line 0x11 (the revision moves it too) and 0x14 (it does not change).
    let ids: Vec<Value> = ok(&mut s, "objects.list", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| {
            matches!(
                o["data"]["import"]["handle"].as_str(),
                Some("11") | Some("14")
            )
        })
        .map(|o| o["id"].clone())
        .collect();
    assert_eq!(ids.len(), 2);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": ids, "delta": [0, 3]}}),
    );
    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(r["counts"]["conflicts"], 1);
    assert_eq!(r["conflicts"].as_array().unwrap().len(), 1);
    let conflict = change_for(&r, "11");
    assert_eq!(conflict["conflict"], true);
    assert_eq!(conflict["edited_ids"].as_array().unwrap().len(), 1);
    assert_eq!(r["conflicts"][0], conflict["id"]);
    let id = r["update_id"].clone();

    // "all" keeps the edit and says which change it skipped.
    let a = ok(
        &mut s,
        "import.apply",
        json!({"update_id": id, "accept": "all"}),
    );
    assert_eq!(a["skipped_conflicts"], json!([conflict["id"]]));
    assert_eq!(ys(&s), vec![0, 103, 200, 403, 500]);
    assert_eq!(a["applied"]["replaced"], 1);
    assert_eq!(a["applied"]["kept_edits"], 1);

    // Naming the conflict, or conflicts: overwrite, applies it.
    let mut t = session_with(dir.path(), &base_text(), json!({}));
    let ids: Vec<Value> = ok(&mut t, "objects.list", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["data"]["import"]["handle"] == "11")
        .map(|o| o["id"].clone())
        .collect();
    ok(
        &mut t,
        "execute",
        json!({"command": {"op": "move_objects", "ids": ids, "delta": [0, 3]}}),
    );
    let r = ok(&mut t, "import.update", json!({"path": rev2}));
    ok(
        &mut t,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all", "conflicts": "overwrite"}),
    );
    assert_eq!(ys(&t), vec![0, 150, 200, 400, 500]);
    // A named conflict keeps the edit too, unless conflicts: overwrite is given.
    for overwrite in [false, true] {
        let mut u = session_with(dir.path(), &base_text(), json!({}));
        let ids: Vec<Value> = ok(&mut u, "objects.list", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["data"]["import"]["handle"] == "13")
            .map(|o| o["id"].clone())
            .collect();
        ok(
            &mut u,
            "execute",
            json!({"command": {"op": "move_objects", "ids": ids, "delta": [0, 3]}}),
        );
        let r = ok(&mut u, "import.update", json!({"path": rev2}));
        let gone = change_for(&r, "13");
        assert_eq!(gone["conflict"], true, "{r}");
        let mut params = json!({"update_id": r["update_id"], "accept": [gone["id"]]});
        if overwrite {
            params["conflicts"] = json!("overwrite");
        }
        let a = ok(&mut u, "import.apply", params);
        let notes = a["notes"].to_string();
        if overwrite {
            assert_eq!(ys(&u), vec![0, 100, 200, 400]);
            assert!(!notes.contains("overwrite"), "{notes}");
        } else {
            assert_eq!(ys(&u), vec![0, 100, 200, 303, 400]);
            assert_eq!(a["applied"]["kept_edits"], 1);
            assert_eq!(a["skipped_conflicts"], json!([gone["id"]]));
            assert!(notes.contains("conflicts: \\\"overwrite\\\""), "{notes}");
        }
    }
}

#[test]
fn a_refused_version_changes_nothing() {
    let dir = temp("kadakas-update-limit");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    ok(&mut s, "version.create", json!({"name": "Concept"}));
    s.set_limits(SessionLimits {
        max_versions: 1,
        ..SessionLimits::default()
    });
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    let doc = s.editor.doc().clone();
    let steps = s.editor.undo_depth();
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all"}),
    );
    assert_eq!((e.code, e.kind), (6, "limit"));
    assert_eq!(e.reason, Some(kadakas_api::REASON_VERSION_LIMIT));
    assert_eq!(
        e.details.as_ref().unwrap()["versions"][0]["name"],
        "Concept"
    );
    assert_eq!(s.editor.doc(), &doc);
    assert_eq!(s.editor.undo_depth(), steps);
    assert_eq!(version_names(&mut s), vec!["Concept"]);
    // The update is still pending: the person makes room. Deleting a version is an edit of the
    // document, so the update has to be made again.
    assert!(s.update_pending().is_some());
    ok(&mut s, "version.delete", json!({"id": 1}));
    assert!(s.update_pending().is_none());
    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    ok(
        &mut s,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all"}),
    );

    // Room for one version: the update is applied with the version before, and the version after
    // is told as a note.
    let mut t = session_with(dir.path(), &base_text(), json!({}));
    t.set_limits(SessionLimits {
        max_versions: 1,
        ..SessionLimits::default()
    });
    let r = ok(&mut t, "import.update", json!({"path": rev2}));
    let a = ok(
        &mut t,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all"}),
    );
    assert!(a["versions"]["after"].is_null());
    assert!(
        a["notes"][0]
            .as_str()
            .unwrap()
            .contains("After update to plan_rev2.dxf"),
        "{a}"
    );
    assert_eq!(
        version_names(&mut t),
        vec!["Before update to plan_rev2.dxf"]
    );
    assert_eq!(ys(&t), vec![0, 150, 200, 400, 500]);
}

#[test]
fn objects_of_other_sources_are_left_alone() {
    let dir = temp("kadakas-update-others");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    // A second drawing with the same handles, and a circle that the person drew.
    let other = write(
        dir.path(),
        "other.dxf",
        &base_text().replace(GUID, "FFFF0002-OTHER"),
    );
    ok(&mut s, "import.file", json!({"path": other}));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [9000, 9000], "radius": 20}}}),
    );
    let others = |s: &Session| -> Vec<String> {
        let mut v: Vec<String> = s
            .editor
            .doc()
            .objects
            .values()
            .filter(|o| {
                provenance::tag(&o.data).is_none_or(|t| t.drawing != format!("guid:{GUID}"))
            })
            .map(|o| serde_json::to_string(o).unwrap())
            .collect();
        v.sort();
        v
    };
    let before = others(&s);
    assert_eq!(before.len(), 6);
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(r["drawing"], format!("guid:{GUID}"));
    ok(
        &mut s,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all"}),
    );
    assert_eq!(others(&s), before);

    // A new file of no known lineage must be told which drawing it is for.
    let anon = write(
        dir.path(),
        "anon.dxf",
        &revision_two().replace(GUID, "00000000"),
    );
    let e = fail(&mut s, "import.update", json!({"path": anon}));
    assert_eq!(e.kind, "invalid_params");
    assert!(e.message.contains("drawing"), "{}", e.message);
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": anon, "drawing": "guid:FFFF0002-OTHER", "dry_run": true}),
    );
    assert_eq!(r["drawing"], "guid:FFFF0002-OTHER");
    let e = fail(
        &mut s,
        "import.update",
        json!({"path": anon, "drawing": "missing"}),
    );
    assert_eq!(e.kind, "invalid_params");
}

#[test]
fn a_drawing_kept_as_a_link_is_read_from_its_file() {
    let dir = temp("kadakas-update-link");
    let mut s = session_with(dir.path(), &base_text(), json!({"base": "link"}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    // The original file is there and unchanged: the update reads it.
    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(r["old_from"], "link");
    assert_eq!(r["counts"]["changed"], 2);
    ok(
        &mut s,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all"}),
    );
    assert_eq!(ys(&s), vec![0, 150, 200, 400, 500]);
    // The link moved to the new file, and no summaries were stored.
    let link = s
        .editor
        .doc()
        .layers
        .iter()
        .find_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .and_then(|l| l.link)
        .expect("a link");
    assert!(link.path.ends_with("plan_rev2.dxf"), "{}", link.path);
    assert!(
        s.editor
            .doc()
            .resources
            .iter()
            .all(|r| !matches!(r.body, kadakas_core::resource::ResourceBody::ImportBase(_)))
    );

    // The original file is gone: the update says so and asks for the old file.
    let mut t = session_with(dir.path(), &base_text(), json!({"base": "link"}));
    std::fs::remove_file(dir.path().join("plan.dxf")).unwrap();
    let e = fail(&mut t, "import.update", json!({"path": rev2}));
    assert_eq!(e.kind, "conflict");
    assert!(
        e.message.contains("gone") && e.message.contains("old file"),
        "{}",
        e.message
    );
    assert_eq!(e.details.as_ref().unwrap()["needs_old_file"], true);
    // With the old file given, it works.
    let old = write(dir.path(), "old_copy.dxf", &base_text());
    let r = ok(&mut t, "import.update", json!({"path": rev2, "old": old}));
    assert_eq!(r["old_from"], "file");
    assert_eq!(r["counts"]["changed"], 2);

    // The original file changed since the import (same size, other content): told.
    let mut u = session_with(dir.path(), &base_text(), json!({"base": "link"}));
    let altered = base_text().replace("AAAA0001", "BBBB0001");
    assert_eq!(altered.len(), base_text().len());
    std::fs::write(dir.path().join("plan.dxf"), altered).unwrap();
    let e = fail(&mut u, "import.update", json!({"path": rev2}));
    assert!(e.message.contains("changed"), "{}", e.message);
    // And a file of another size.
    std::fs::write(dir.path().join("plan.dxf"), "0\nEOF\n").unwrap();
    let e = fail(&mut u, "import.update", json!({"path": rev2}));
    assert!(e.message.contains("changed"), "{}", e.message);

    // A drawing imported with base: none needs the old file as well.
    let mut v = session_with(dir.path(), &base_text(), json!({"base": "none"}));
    let e = fail(&mut v, "import.update", json!({"path": rev2}));
    assert_eq!(e.kind, "conflict");
    assert!(e.message.contains("old file"));
}

#[test]
fn the_path_policy_applies_to_the_new_file_the_old_file_and_the_link() {
    let dir = temp("kadakas-update-policy");
    let outside = temp("kadakas-update-outside");
    let mut s = session_with(dir.path(), &base_text(), json!({"base": "link"}));
    let rev2 = write(outside.path(), "plan_rev2.dxf", &revision_two());
    s.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    // The new file is outside the allowed folders.
    let e = fail(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(e.kind, "policy");
    // The new file is inside, but the linked original is outside now: the policy refuses it, the
    // update asks for the old file.
    let inside = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let moved = write(outside.path(), "plan.dxf", &base_text());
    std::fs::remove_file(dir.path().join("plan.dxf")).unwrap();
    // Point the link at the outside copy.
    let mut doc = s.editor.doc().clone();
    for l in &mut doc.layers {
        if let Some(src) = l.extra.get_mut(provenance::LAYER_KEY)
            && let Some(link) = src.get_mut("link")
        {
            link["path"] = json!(moved.to_str().unwrap());
        }
    }
    let mut t = Session::with_sequential_ids();
    t.adopt_document(doc);
    t.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    let e = fail(&mut t, "import.update", json!({"path": inside}));
    assert_eq!(e.kind, "conflict", "{}", e.message);
    assert!(e.message.contains("may not be read"), "{}", e.message);
    // The old file outside is refused as well.
    let e = fail(
        &mut t,
        "import.update",
        json!({"path": inside, "old": moved}),
    );
    assert_eq!(e.kind, "policy");
    // The trusted call of the file dialog is not limited.
    let r = t
        .import_update_trusted(json!({"path": rev2, "old": moved, "dry_run": true}))
        .unwrap();
    assert_eq!(r["counts"]["changed"], 2);
    // A wrong extension is refused in a restricted session.
    let odd = write(dir.path(), "plan.txt", &revision_two());
    let e = fail(&mut t, "import.update", json!({"path": odd}));
    assert_eq!(e.kind, "policy");
}

#[test]
fn the_list_is_bounded_and_the_layers_limit_the_update() {
    let dir = temp("kadakas-update-bounds");
    let many = |shift: i64| -> String {
        let ents: String = (0..300)
            .map(|k| {
                line(
                    0x100 + k as u32,
                    if k % 2 == 0 { "EVEN" } else { "ODD" },
                    k * 100 + shift,
                )
            })
            .collect();
        file(&format!("V{shift}"), "", &ents)
    };
    let mut s = session_with(dir.path(), &many(0), json!({}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &many(30));
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": rev2, "limit": 50, "dry_run": true}),
    );
    assert_eq!(r["counts"]["changed"], 300);
    assert_eq!(r["listed"], 50);
    assert_eq!(r["total_changes"], 300);
    assert_eq!(r["changes"].as_array().unwrap().len(), 50);
    assert!(
        r["notes"].to_string().contains("50 of 300"),
        "{}",
        r["notes"]
    );
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": rev2, "layers": ["even"], "dry_run": true}),
    );
    assert_eq!(r["counts"]["changed"], 150);
    for bad in [0, 1001] {
        let e = fail(&mut s, "import.update", json!({"path": rev2, "limit": bad}));
        assert_eq!(e.kind, "invalid_params");
    }
    // A plan that does not fit the memory limit of the session is refused.
    s.set_limits(SessionLimits {
        update_bytes: 1000,
        ..SessionLimits::default()
    });
    let e = fail(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(e.kind, "limit");
    assert!(e.message.contains("layers"), "{}", e.message);
    // A file that is not a drawing, a missing file, and a bad tolerance.
    let junk = write(dir.path(), "junk.dxf", "this is not a drawing");
    let e = fail(&mut s, "import.update", json!({"path": junk}));
    assert!(matches!(e.kind, "validation" | "error"), "{}", e.kind);
    let e = fail(
        &mut s,
        "import.update",
        json!({"path": dir.path().join("none.dxf")}),
    );
    assert!(!e.message.is_empty());
    s.set_limits(SessionLimits::default());
    let e = fail(
        &mut s,
        "import.update",
        json!({"path": rev2, "tolerance": -1}),
    );
    assert_eq!(e.kind, "validation");
}

#[test]
fn a_changed_block_definition_updates_all_its_instances_through_the_api() {
    let dir = temp("kadakas-update-blocks");
    let block = |y: i64| {
        format!(
            "0\nBLOCK\n5\n21\n2\nTREE\n10\n0\n20\n0\n0\nLINE\n5\n30\n8\nA\n10\n0\n20\n{y}\n11\n10\n21\n{y}\n0\nENDBLK\n"
        )
    };
    let inserts = "0\nINSERT\n5\n40\n8\nA\n2\nTREE\n10\n0\n20\n0\n0\nINSERT\n5\n41\n8\nA\n2\nTREE\n10\n5000\n20\n0\n";
    let mut s = session_with(dir.path(), &file("V1", &block(0), inserts), json!({}));
    assert_eq!(s.editor.doc().objects.len(), 2);
    let rev2 = write(dir.path(), "tree2.dxf", &file("V2", &block(70), inserts));
    let r = ok(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(r["counts"]["changed"], 1);
    assert_eq!(r["changes"][0]["objects"], 2);
    ok(
        &mut s,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": "all"}),
    );
    assert_eq!(ys(&s), vec![70, 70]);
}

#[test]
fn the_update_is_in_describe() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for m in ["import.update", "import.apply", "dxf.compare"] {
        assert!(names.contains(&m), "{m}");
    }
    assert!(d["io"]["update"]["examples"].as_array().unwrap().len() >= 3);
    assert!(d["review"]["dxf_compare"]["examples"].is_array());
    // Without an imported drawing there is nothing to update.
    let dir = temp("kadakas-update-none");
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let e = fail(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(e.kind, "not_found");
}

#[test]
fn the_pending_update_has_a_preview_with_the_overlay_that_the_app_draws() {
    let dir = temp("kadakas-update-preview");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    assert!(s.update_preview().unwrap().is_none(), "nothing is pending");
    assert_eq!(s.imported_drawings().len(), 1);
    let before = s.editor.doc().clone();
    let held = s.estimated_bytes();
    ok(&mut s, "import.update", json!({"path": rev2}));
    let pending = s.update_pending().unwrap();
    assert!(
        s.estimated_bytes() > held,
        "the plan is part of the memory of the session"
    );

    let preview = s.update_preview().unwrap().unwrap();
    // The document is not touched, the preview is the drawing after all changes.
    assert_eq!(s.editor.doc(), &before);
    assert_eq!(preview.old.objects, before.objects);
    assert_eq!(preview.doc.objects.len(), 5);
    let c = preview.overlay.counts();
    assert_eq!((c.added, c.removed, c.changed), (1, 1, 2));
    assert_eq!(preview.overlay.ghosts().count(), 1);
    // The objects of each change, in the document after: added and changed ones are there, a
    // removed one is only in the old document.
    assert_eq!(preview.objects.len(), pending.plan.changes.len());
    for (i, item) in pending.plan.changes.iter().enumerate() {
        assert!(!preview.objects[i].is_empty(), "{item:?}");
        for id in &preview.objects[i] {
            let doc = if item.status == kadakas_api::ChangeKind::Removed {
                &preview.old
            } else {
                &preview.doc
            };
            assert!(doc.objects.contains_key(id));
        }
    }
    // It is made once for a plan.
    let again = s.update_preview().unwrap().unwrap();
    assert!(std::sync::Arc::ptr_eq(&preview, &again));
    // Freeing the caches drops the preview but not the plan.
    s.drop_caches();
    assert!(s.update_pending().is_some());
    let rebuilt = s.update_preview().unwrap().unwrap();
    assert!(!std::sync::Arc::ptr_eq(&preview, &rebuilt));
    // Discarding, and an edit, both end it.
    s.update_discard();
    assert!(s.update_pending().is_none() && s.update_preview().unwrap().is_none());
    ok(&mut s, "import.update", json!({"path": rev2}));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [9000, 9000], "radius": 20}}}),
    );
    assert!(s.update_preview().unwrap().is_none());
}

#[test]
fn an_update_too_big_for_the_update_data_gives_the_choices_of_import_file() {
    let dir = temp("kadakas-update-base-limit");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    s.set_limits(SessionLimits {
        base_records: 3,
        ..SessionLimits::default()
    });
    let e = fail(&mut s, "import.update", json!({"path": rev2}));
    assert_eq!(e.reason, Some(kadakas_api::REASON_BASE_TOO_MANY));
    assert_eq!(e.details.as_ref().unwrap()["cap"], 3);
    // The choices work for an update as they do for an import.
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": rev2, "options": {"base": "none"}}),
    );
    assert!(r["notes"].to_string().contains("base: none"), "{r}");
    let e = fail(
        &mut s,
        "import.apply",
        json!({"update_id": r["update_id"], "accept": [1]}),
    );
    assert_eq!(e.kind, "invalid_params");
}

#[test]
fn a_file_of_another_lineage_needs_the_person_to_confirm() {
    let dir = temp("kadakas-update-lineage");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    let anon = write(
        dir.path(),
        "other_name.dxf",
        &revision_two().replace(GUID, "00000000"),
    );
    let e = fail(&mut s, "import.update", json!({"path": anon}));
    assert_eq!(e.kind, "invalid_params");
    let d = e.details.as_ref().unwrap();
    assert_eq!(d["ask_the_person"], true);
    assert_eq!(d["drawing"], format!("guid:{GUID}"));
    assert_eq!(d["file_says"], "other_name.dxf");
    assert!(s.update_pending().is_none());
    // After the person agrees, the agent names the drawing.
    let r = ok(
        &mut s,
        "import.update",
        json!({"path": anon, "drawing": format!("guid:{GUID}")}),
    );
    assert_eq!(r["counts"]["changed"], 2);
}

#[test]
fn the_object_limit_of_the_session_bounds_the_new_file() {
    let dir = temp("kadakas-update-max-objects");
    let mut s = session_with(dir.path(), &base_text(), json!({}));
    s.set_limits(SessionLimits {
        max_objects: 3,
        ..SessionLimits::default()
    });
    let rev2 = write(dir.path(), "plan_rev2.dxf", &revision_two());
    let e = fail(
        &mut s,
        "import.update",
        json!({"path": rev2, "options": {"max_objects": 1_000_000}}),
    );
    assert_eq!(e.kind, "limit", "{e:?}");
}
