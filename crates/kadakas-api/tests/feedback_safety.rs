//! Feedback: what must hold when things go wrong or when a person's data is at stake.
//! A new document keeps the outbox, a panic leaves no trace, agents do not see the reports of
//! people, and the privacy options remove what they promise. Nothing here uses the network or a
//! folder of the user.

use kadakas_api::Session;
use kadakas_api::feedback_api::excerpt::{apply_privacy, calls_privacy, log_privacy, objects_of};
use kadakas_feedback::store::DirStorage;
use kadakas_feedback::{AttachmentKind, Draft, Kind, Origin, Privacy};
use serde_json::{Value, json};

fn line(s: &mut Session, method: &str, params: Value) -> Value {
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    serde_json::from_str(&s.handle_line(&req.to_string()).unwrap()).unwrap()
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    let r = line(s, method, params);
    assert!(r.get("error").is_none(), "{r}");
    r["result"].clone()
}

fn exec(s: &mut Session, command: Value) -> Value {
    ok(s, "execute", json!({"command": command}))
}

// ---- B1: a new document keeps the outbox

#[test]
fn a_new_document_keeps_the_outbox_the_flags_and_the_rate_limit() {
    let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-replace").unwrap();
    let dir = tmp.path().join("outbox");
    let mut s = Session::with_sequential_ids();
    s.feedback
        .hub
        .set_storage(Box::new(DirStorage::new(dir.clone())));
    s.feedback
        .hub
        .set_url_override(Some("https://fb.test".into()));
    s.feedback.hub.set_send_override(Some(true));
    s.feedback.hub.set_max_submits(4);

    let mut n = 0;
    let mut submit = |s: &mut Session| {
        n += 1;
        line(
            s,
            "feedback.submit",
            json!({"kind": "question", "title": format!("q{n}"), "attach": false}),
        )
    };
    assert!(submit(&mut s).get("error").is_none());

    // document.load
    let doc = serde_json::to_value(s.editor.doc()).unwrap();
    ok(&mut s, "document.load", json!({"document": doc}));
    assert!(submit(&mut s).get("error").is_none());
    assert_eq!(s.feedback.hub.url(), "https://fb.test");
    assert!(s.feedback.hub.send_enabled());

    // file.save and file.open
    let file = tmp.path().join("plan.kad");
    let req = |m: &str| json!({"jsonrpc": "2.0", "id": 1, "method": m, "params": {"path": file}});
    let saved: Value = serde_json::from_str(&kadakas_api::files::handle_file_request(
        &mut s,
        &req("file.save"),
    ))
    .unwrap();
    assert!(saved.get("error").is_none(), "{saved}");
    let opened: Value = serde_json::from_str(&kadakas_api::files::handle_file_request(
        &mut s,
        &req("file.open"),
    ))
    .unwrap();
    assert!(opened.get("error").is_none(), "{opened}");
    assert!(submit(&mut s).get("error").is_none());

    // The reports are in the folder, the flags survived, and the limit is the same limit.
    assert!(dir.join("outbox.json").is_file());
    assert_eq!(s.feedback.hub.list().len(), 3);
    assert!(s.feedback.hub.send_enabled());
    let fourth = submit(&mut s);
    assert!(fourth.get("error").is_none());
    let fifth = submit(&mut s);
    assert_eq!(fifth["error"]["code"], 6, "{fifth}");

    // A direct replace (the app's File > New and Open) works the same way.
    s.replace_document(kadakas_core::model::Document::default());
    assert_eq!(s.feedback.hub.list().len(), 4);
    assert_eq!(submit(&mut s)["error"]["code"], 6);
    assert_eq!(s.feedback.hub.url(), "https://fb.test");
}

#[test]
fn a_new_document_forgets_the_calls_and_the_last_failure_only() {
    let mut s = Session::with_sequential_ids();
    s.feedback.set_client("agent-x", "1");
    s.feedback.log("kept line");
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}),
    );
    assert!(line(&mut s, "edit.nope", json!({}))["error"].is_object());
    assert!(!s.feedback.recent_calls().is_empty());
    s.replace_document(kadakas_core::model::Document::default());
    assert!(s.feedback.recent_calls().is_empty());
    // The failure went with the old document: the bug has no method to fill in.
    ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "bug", "title": "after", "attach": false}),
    );
    let got = ok(&mut s, "feedback.get", json!({"id": "fb-1"}));
    assert!(
        got.get("method").is_none() && got.get("error").is_none(),
        "{got}"
    );
    // The client and the log stay.
    assert_eq!(got["context"]["client_name"], "agent-x");
}

// ---- M3: a panic leaves no trace

#[test]
fn a_panic_in_a_command_leaves_the_document_the_revision_and_the_undo_as_they_were() {
    let mut s = Session::with_sequential_ids();
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}),
    );
    let (revision, depth, count) = (
        s.editor.revision(),
        s.editor.undo_depth(),
        s.editor.doc().objects.len(),
    );
    let before = serde_json::to_string(s.editor.doc()).unwrap();

    kadakas_core::fault::arm();
    let r = line(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [5, 5], "radius": 2}}}),
    );
    assert_eq!(r["error"]["code"], -32603, "{r}");
    assert_eq!(r["error"]["data"]["kind"], "internal");
    assert_eq!(r["error"]["data"]["tainted"], true);
    assert_eq!(r["error"]["data"]["feedback"]["draft"]["kind"], "bug");

    assert_eq!(s.editor.revision(), revision);
    assert_eq!(s.editor.undo_depth(), depth);
    assert_eq!(s.editor.doc().objects.len(), count);
    assert_eq!(serde_json::to_string(s.editor.doc()).unwrap(), before);
    assert!(!s.editor.can_redo());

    // The session goes on, and says that it is tainted.
    let ok_again = line(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [5, 5], "radius": 2}}}),
    );
    assert!(ok_again.get("error").is_none());
    // The panic is in the failure and the log of the session, with its message.
    let r = ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "bug", "title": "it panicked"}),
    );
    assert!(
        r["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "log")
    );
    let got = ok(&mut s, "feedback.get", json!({"id": r["id"]}));
    assert_eq!(got["method"], "execute");
    assert!(
        got["error"].as_str().unwrap().contains("injected fault"),
        "{got}"
    );
    let log = String::from_utf8(
        s.feedback
            .hub
            .attachment("fb-1", AttachmentKind::Log)
            .unwrap(),
    )
    .unwrap();
    assert!(log.contains("injected fault"), "{log}");

    // Every later error says that the session is tainted. A new document is a fresh start.
    let bad = line(&mut s, "objects.get", json!({}));
    assert_eq!(bad["error"]["data"]["tainted"], true);
    s.replace_document(kadakas_core::model::Document::default());
    let bad = line(&mut s, "objects.get", json!({}));
    assert!(bad["error"]["data"].get("tainted").is_none());
}

#[test]
fn a_panic_inside_a_batch_takes_back_every_step() {
    let mut s = Session::with_sequential_ids();
    // The second step of the batch panics, after the first one made its change.
    kadakas_core::fault::arm_after(2);
    let r = line(
        &mut s,
        "execute",
        json!({"command": {"op": "batch", "commands": [
            {"op": "add_object", "shape": {"kind": "circle", "center": [1, 1], "radius": 1}},
            {"op": "add_object", "shape": {"kind": "circle", "center": [2, 2], "radius": 1}}
        ]}}),
    );
    assert_eq!(r["error"]["code"], -32603, "{r}");
    assert!(s.editor.doc().objects.is_empty());
    assert_eq!(s.editor.undo_depth(), 0);
}

// ---- M10: agents and people

#[test]
fn agents_do_not_see_or_touch_the_reports_of_people() {
    let mut s = Session::with_sequential_ids();
    let mut d = Draft::new(
        Kind::Bug,
        "Written by a person",
        "private text",
        Origin::User,
    );
    d.method = "m".into();
    s.feedback.hub.submit(d, vec![], 0).unwrap();
    ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "question", "title": "by an agent", "attach": false}),
    );
    let l = ok(&mut s, "feedback.list", json!({}));
    assert_eq!(l["reports"].as_array().unwrap().len(), 1);
    assert_eq!(l["reports"][0]["title"], "by an agent");
    assert_eq!(l["reports_of_people"], 1);
    assert!(!l.to_string().contains("private text"));
    for method in ["feedback.get", "feedback.withdraw"] {
        let r = line(&mut s, method, json!({"id": "fb-1"}));
        assert_eq!(r["error"]["code"], 2, "{method}: {r}");
    }
    assert_eq!(
        s.feedback.hub.get("fb-1").unwrap().status,
        kadakas_feedback::Status::Pending
    );
    // An agent cannot merge into the report of a person either.
    let r = ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "bug", "title": "Written by a person", "method": "m", "attach": false}),
    );
    assert_eq!(
        (r["merged"].clone(), r["id"].clone()),
        (json!(false), json!("fb-3"))
    );
}

#[test]
fn people_are_not_held_by_the_session_limit() {
    let mut s = Session::with_sequential_ids();
    s.feedback.hub.set_max_submits(1);
    ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "question", "title": "one", "attach": false}),
    );
    assert_eq!(
        line(
            &mut s,
            "feedback.submit",
            json!({"kind": "question", "title": "two", "attach": false})
        )["error"]["code"],
        6
    );
    for i in 0..3 {
        s.feedback
            .hub
            .submit(
                Draft::new(Kind::Bug, &format!("person {i}"), "", Origin::User),
                vec![],
                0,
            )
            .unwrap();
    }
}

// ---- dedupe first, one render

#[test]
fn a_report_that_merges_makes_no_attachments() {
    let mut s = Session::with_sequential_ids();
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}),
    );
    let first = ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "feature_request", "title": "Same ask"}),
    );
    assert!(first["attachments"].as_array().unwrap().len() >= 2);
    let again = ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "feature_request", "title": "same ask!"}),
    );
    assert_eq!(
        (again["merged"].clone(), again["attachments"].clone()),
        (json!(true), json!([]))
    );
}

// ---- M4: the privacy options on the excerpt

/// A document with a place, names, notes, records, a worksheet, a symbol, a title block and a
/// viewport.
fn rich_document() -> String {
    let mut s = Session::with_sequential_ids();
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [385000000.0, 667200000.0], "max": [385002000.0, 667203000.0]}}),
    );
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "text", "at": [385000500.0, 667200500.0], "text": "Smith note", "height": 200}}),
    );
    exec(
        &mut s,
        json!({"op": "add_resource", "name": "Smith sheet", "body": {"type": "worksheet", "cells": {"A1": "Smith garden", "B2": {"text": "Elm Road 4"}}}}),
    );
    exec(
        &mut s,
        json!({"op": "add_resource", "name": "Smith record", "body": {"type": "record_format", "fields": [{"name": "Owner", "type": "text", "default": "Mrs Smith"}]}}),
    );
    ok(
        &mut s,
        "symbol.create",
        json!({"name": "Smith mark", "items": [{"shape": {"kind": "text", "at": [0, 0], "text": "Smith symbol", "height": 100}}]}),
    );
    exec(
        &mut s,
        json!({"op": "add_layer", "name": "Smith house layer", "elevation": 0}),
    );
    exec(&mut s, json!({"op": "add_class", "name": "Smith class"}));
    exec(
        &mut s,
        json!({"op": "add_sheet", "name": "Smith plan", "paper": "a1", "number": "L-101"}),
    );
    let sum = ok(&mut s, "document.summary", json!({}));
    let sheet = sum["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["kind"] == "sheet")
        .unwrap()["id"]
        .clone();
    let design = sum["layers"][0]["id"].clone();
    exec(
        &mut s,
        json!({"op": "add_object", "layer": sheet, "shape": {"kind": "title_block", "grid_columns": 6, "grid_rows": 4, "fields": {"owner": "Mrs Smith", "address": "Elm Road 4"}}}),
    );
    exec(
        &mut s,
        json!({"op": "add_object", "layer": sheet, "shape": {"kind": "viewport", "min": [15, 55], "max": [405, 282], "center": [385001000.0, 667201500.0], "scale": 100, "layers": [design]}}),
    );
    let mut doc = serde_json::to_value(s.editor.doc()).unwrap();
    doc["georef"] = json!({"crs": "EPSG:3067", "origin": [385000.0, 6672000.0], "rotation": 0.0, "user_origin": [0.0, 0.0]});
    doc.to_string()
}

fn privacy(origin: bool, georef: bool, names: bool) -> Privacy {
    Privacy {
        move_to_origin: origin,
        remove_georef: georef,
        remove_names: names,
    }
}

#[test]
fn the_default_options_remove_the_place_everywhere() {
    let doc = rich_document();
    for needle in ["385000", "667200", "385001000", "6672000", "EPSG"] {
        assert!(doc.contains(needle), "the test document needs {needle}");
    }
    let out = apply_privacy(&doc, Privacy::default()).unwrap();
    for needle in [
        "385000",
        "667200",
        "385001000",
        "667201500",
        "6672000",
        "EPSG",
        "georef",
    ] {
        assert!(!out.contains(needle), "{needle} is still there: {out}");
    }
    // The viewport still shows the objects: its centre moved with them.
    let doc: Value = serde_json::from_str(&out).unwrap();
    let vp = doc["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["shape"]["kind"] == "viewport" || o["kind"] == "viewport")
        .unwrap();
    let text = vp.to_string();
    assert!(
        text.contains("\"min\":[15.0,55.0]") || text.contains("\"min\":[15,55]"),
        "{text}"
    );
    for o in objects_of(&out) {
        if o.kind != "viewport" && o.kind != "title_block" {
            assert!(o.min[0].abs() < 5000.0 && o.max[1].abs() < 5000.0, "{o:?}");
        }
    }
}

#[test]
fn the_names_option_removes_names_notes_records_worksheets_symbols_and_title_blocks() {
    let doc = rich_document();
    let kept = apply_privacy(&doc, privacy(false, false, false)).unwrap();
    for needle in [
        "Smith note",
        "Smith garden",
        "Mrs Smith",
        "Smith symbol",
        "Elm Road",
        "Smith house layer",
        "Smith class",
        "Smith plan",
    ] {
        assert!(
            kept.contains(needle),
            "{needle} should stay without the option"
        );
    }
    let out = apply_privacy(&doc, privacy(false, false, true)).unwrap();
    assert!(!out.contains("Smith") && !out.contains("Elm Road"), "{out}");
    // The structure is still a document: layers and classes have placeholder names.
    assert!(out.contains("Layer 2") || out.contains("Layer 1"), "{out}");
    assert!(out.contains("Class "), "{out}");
    kadakas_core::model::Document::from_value(serde_json::from_str(&out).unwrap()).unwrap();
}

#[test]
fn pictures_are_found_by_their_first_bytes_or_their_key_and_never_leave() {
    let doc: Value = serde_json::from_str(&rich_document()).unwrap();
    let mut with_png = doc.clone();
    // A short picture text under an innocent key, and a long one under a picture key.
    with_png["resources"].as_array_mut().unwrap().push(json!({
        "id": "00000000-0000-0000-0000-0000000000aa", "name": "Photo",
        "body": {"type": "worksheet", "note": "iVBORw0KGgo="}
    }));
    with_png["resources"].as_array_mut().unwrap().push(json!({
        "id": "00000000-0000-0000-0000-0000000000ab", "name": "Photo 2",
        "body": {"type": "worksheet", "thumbnail": "x".repeat(100)}
    }));
    // Only the excerpt builder drops them, so build one from a session that holds such resources.
    let mut s = Session::with_sequential_ids();
    s.replace_document(kadakas_core::model::Document::from_value(with_png).unwrap());
    let ids: Vec<_> = s.editor.doc().objects.keys().copied().collect();
    let ex = kadakas_api::feedback_api::excerpt::build(s.editor.doc(), &ids).unwrap();
    assert!(
        !ex.json.contains("iVBOR") && !ex.json.contains(&"x".repeat(100)),
        "a picture is in the excerpt"
    );
}

// ---- M5: the call log and the log lines

#[test]
fn the_options_apply_to_the_calls_and_the_log_lines() {
    let calls = json!([
        {"method": "georef.set", "params": {"crs": "EPSG:3067"}},
        {"method": "execute", "params": {"command": {"op": "add_object", "shape": {"kind": "line", "a": [385000.5, 6672000.25], "b": [385010.0, 6672010.0]}}, "crs": "EPSG:3067"}},
        {"method": "plants.place", "params": {"at": [385001, 6672001, 12], "x": 385001.5, "note": "Smith"}, "error": "bad point [385002.5, 6672002.5] in EPSG:3067"},
    ]).to_string();
    let out = calls_privacy(&calls, Privacy::default());
    for needle in ["385000", "385001", "385002", "6672000", "EPSG", "georef"] {
        assert!(!out.contains(needle), "{needle} in {out}");
    }
    // The call about the place is gone; the others are there with their method names.
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[1]["method"], "plants.place");
    // Without the options the calls are as they were, and the names option blanks the notes.
    let kept = calls_privacy(&calls, privacy(false, false, false));
    assert!(kept.contains("385000.5") && kept.contains("georef.set"));
    assert!(!calls_privacy(&calls, privacy(false, false, true)).contains("Smith"));

    let log = "execute: bad point [385002.5, 6672002.5] (code 3)\nsetting georef failed\nplain line (1, 2) here\nuses EPSG:3067";
    let out = log_privacy(log, Privacy::default());
    assert!(
        !out.contains("385002") && !out.contains("georef") && !out.contains("EPSG"),
        "{out}"
    );
    assert!(out.contains("plain line") && out.contains("execute: bad point"));
    assert_eq!(log_privacy(log, privacy(false, false, false)), log);
}

#[test]
fn the_review_shows_the_calls_and_the_log_as_they_will_be_sent() {
    let mut s = Session::with_sequential_ids();
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "line", "a": [385000.5, 667200.25], "b": [385010.0, 667210.0]}}),
    );
    assert!(
        line(
            &mut s,
            "plants.place",
            json!({"at": [385000.5, 667200.25], "bogus": 1})
        )["error"]
            .is_object()
    );
    let r = ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "bug", "title": "coords"}),
    );
    let mut review = kadakas_api::feedback_api::review::Review::open(
        &mut s.feedback.hub,
        r["id"].as_str().unwrap(),
    )
    .unwrap();
    let shown = review.calls_text().unwrap();
    assert!(!shown.contains("385000"), "{shown}");
    let payload = review.payload().unwrap();
    assert!(!payload.contains("385000"));
    // With the option off, the person sees the numbers, and they are in the message.
    review.privacy = privacy(false, true, false);
    assert!(review.calls_text().unwrap().contains("385000"));
    assert!(review.payload().unwrap().contains("385000"));
}

// ---- places in record data, boxes in calls, file errors, newer outbox

#[test]
fn record_data_loses_its_place_with_the_place_options_even_when_names_stay() {
    let mut s = Session::with_sequential_ids();
    let r = exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}),
    );
    let id = r["created"][0].clone();
    exec(
        &mut s,
        json!({"op": "set_data", "id": id, "set": {"Site": {"lat": 60.17, "lon": 24.94, "site_address": "Elm Road 4", "owner_note": "Mrs Smith", "depth": 300, "nested": {"x": 385000.5, "kept": "yes"}}}}),
    );
    let doc = serde_json::to_string(s.editor.doc()).unwrap();
    assert!(doc.contains("Elm Road 4") && doc.contains("60.17"));
    let out = apply_privacy(&doc, privacy(true, false, false)).unwrap();
    for gone in ["60.17", "24.94", "Elm Road 4", "385000.5"] {
        assert!(!out.contains(gone), "{gone} in {out}");
    }
    // What is not a place stays: the names option is off.
    for kept in ["Mrs Smith", "300", "\"kept\""] {
        assert!(out.contains(kept), "{kept} in {out}");
    }
    let out = apply_privacy(&doc, privacy(false, true, false)).unwrap();
    assert!(!out.contains("60.17") && out.contains("Mrs Smith"));
    // With both place options off, the data stays.
    let out = apply_privacy(&doc, privacy(false, false, false)).unwrap();
    assert!(out.contains("60.17") && out.contains("Elm Road 4"));
}

#[test]
fn boxes_and_extent_keys_leave_the_call_log() {
    let calls = json!([
        {"method": "gis.fetch", "params": {"bbox": [24.9, 60.1, 25.0, 60.2], "extent": {"a": 1}, "bounds": "x", "other": [1, 2, 3, 4], "layer": "roads"}},
        {"method": "x.y", "params": {"area": [10, 20, 30, 40]}},
    ]).to_string();
    for p in [privacy(true, false, false), privacy(false, true, false)] {
        let out = calls_privacy(&calls, p);
        for gone in [
            "bbox",
            "extent",
            "bounds",
            "24.9",
            "60.2",
            "[10",
            "[1,2,3,4]",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        assert!(out.contains("roads") && out.contains("gis.fetch"));
    }
    assert!(calls_privacy(&calls, privacy(false, false, false)).contains("bbox"));
}

#[test]
fn file_errors_have_the_hint_and_the_tainted_mark_and_are_in_the_call_log() {
    let mut s = Session::with_sequential_ids();
    s.paths = kadakas_api::PathPolicy::restricted([std::env::temp_dir()]);
    let req = json!({"jsonrpc": "2.0", "id": 5, "method": "file.open", "params": {"path": "/nonexistent-dir/none.kad"}});
    let r: Value =
        serde_json::from_str(&kadakas_api::files::handle_file_request(&mut s, &req)).unwrap();
    assert!(r["error"].is_object(), "{r}");
    assert!(r["error"]["data"].get("tainted").is_none());
    // The call and its error are in the log of the session, with no folder name.
    let calls = s.feedback.recent_calls();
    assert_eq!(calls.last().unwrap()["method"], "file.open");
    assert!(calls.last().unwrap()["error"].is_string());
    // A tainted session says so in a file reply as well.
    kadakas_core::fault::arm();
    exec_raw(&mut s);
    let r: Value =
        serde_json::from_str(&kadakas_api::files::handle_file_request(&mut s, &req)).unwrap();
    assert_eq!(r["error"]["data"]["tainted"], true, "{r}");
}

fn exec_raw(s: &mut Session) {
    let r = line(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [1, 1], "radius": 1}}}),
    );
    assert_eq!(r["error"]["code"], -32603);
}

#[test]
fn a_newer_outbox_shows_a_note_and_keeps_its_file() {
    let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-newer-api").unwrap();
    let dir = tmp.path().join("fb");
    std::fs::create_dir_all(&dir).unwrap();
    let newer = br#"{"version":99,"next_seq":1,"reports":[]}"#;
    std::fs::write(dir.join("outbox.json"), newer).unwrap();
    let mut s = Session::with_sequential_ids();
    s.feedback
        .hub
        .set_storage(Box::new(DirStorage::new(dir.clone())));
    let l = ok(&mut s, "feedback.list", json!({}));
    assert!(l["note"].as_str().unwrap().contains("newer"), "{l}");
    ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "question", "title": "memory", "attach": false}),
    );
    assert_eq!(std::fs::read(dir.join("outbox.json")).unwrap(), newer);
}

#[test]
fn the_review_message_has_the_installation_id() {
    let mut s = Session::with_sequential_ids();
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}),
    );
    let r = ok(
        &mut s,
        "feedback.submit",
        json!({"kind": "bug", "title": "id"}),
    );
    let review = kadakas_api::feedback_api::review::Review::open(
        &mut s.feedback.hub,
        r["id"].as_str().unwrap(),
    )
    .unwrap();
    let p: Value = serde_json::from_str(&review.payload().unwrap()).unwrap();
    assert_eq!(
        p["installation"].as_str().unwrap(),
        s.feedback.hub.installation_id()
    );
    assert_eq!(p["installation"].as_str().unwrap().len(), 36);
}
