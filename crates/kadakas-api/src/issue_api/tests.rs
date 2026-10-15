use base64::Engine;
use kadakas_core::change::Origin;
use serde_json::{Value, json};

use super::*;
use crate::PathPolicy;
use crate::events::LOCAL;

fn agent(s: &mut Session, method: &str, p: Value) -> Result<Value, RpcError> {
    s.handle_as(LOCAL, Origin::Agent, method, p)
}

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    agent(s, method, p).unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> RpcError {
    agent(s, method, p).expect_err(&format!("{method} should fail"))
}

fn rect(s: &mut Session, x: i32) -> String {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape":
            {"kind": "rect", "min": [x, 0], "max": [x + 1000, 1000]}}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string()
}

fn session() -> Session {
    Session::with_sequential_ids()
}

fn new_issue(s: &mut Session, title: &str) -> String {
    ok(s, "issue.create", json!({"title": title, "now_ms": 5_000}))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn ids(v: &Value) -> Vec<String> {
    v["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn an_agent_makes_an_issue_with_its_origin_and_one_undo_step() {
    let mut s = session();
    let tree = rect(&mut s, 0);
    let depth = s.editor.undo_depth();
    let r = ok(
        &mut s,
        "issue.create",
        json!({"title": "Dead tree", "description": "Check it", "type": "error",
               "priority": "high", "assignee": "Mari", "labels": ["planting"],
               "due": "2026-12-01", "objects": [tree], "point": [100, 200, 0],
               "author": "Claude", "now_ms": 1_700_000_000_000i64}),
    );
    assert_eq!(s.editor.undo_depth(), depth + 1);
    let id = r["id"].as_str().unwrap();
    assert_eq!(r["issue"]["origin"], "agent");
    assert_eq!(r["issue"]["status"], "open");
    assert_eq!(r["issue"]["pin"], json!([100.0, 200.0, 0.0]));
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    assert_eq!(g["title"], "Dead tree");
    assert_eq!(g["type"], "error");
    assert_eq!(g["author"], "Claude");
    assert_eq!(g["due"], "2026-12-01");
    assert_eq!(g["created"], "2023-11-14T22:13:20.000Z");
    assert_eq!(g["created_ms"], 1_700_000_000_000i64);
    assert_eq!(g["objects"], json!([tree]));
    assert_eq!(g["objects_missing"], json!([]));
    assert_eq!(
        g["objects_ifc"].as_array().unwrap()[0]
            .as_str()
            .unwrap()
            .len(),
        22
    );
    assert_eq!(g["comments"], json!([]));
    // Undo takes it back.
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 0);
    assert!(s.editor.redo());
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 1);
    // The session of a person makes the origin user.
    let u = s
        .handle("issue.create", json!({"title": "By hand", "now_ms": 1}))
        .unwrap();
    assert_eq!(u["issue"]["origin"], "user");
}

#[test]
fn every_change_emits_an_issue_event() {
    let mut s = session();
    let rev = s.editor.revision();
    let id = new_issue(&mut s, "A");
    ok(
        &mut s,
        "issue.update",
        json!({"id": id, "status": "in_progress", "now_ms": 6_000}),
    );
    let c = ok(
        &mut s,
        "issue.comment",
        json!({"id": id, "text": "hi", "now_ms": 7_000}),
    );
    ok(&mut s, "issue.resolve", json!({"id": id, "now_ms": 8_000}));
    ok(&mut s, "issue.delete", json!({"id": id}));
    let polled = ok(
        &mut s,
        "events.poll",
        json!({"since_revision": rev, "kinds": ["issues"]}),
    );
    let changes = polled["changes"].as_array().unwrap();
    let ops: Vec<&str> = changes.iter().map(|c| c["op"].as_str().unwrap()).collect();
    assert_eq!(
        ops,
        vec![
            "add_issue",
            "modify_issue",
            "add_issue_comment",
            "batch",
            "batch"
        ]
    );
    assert_eq!(changes[0]["added"]["issues"], json!([id]));
    assert_eq!(changes[1]["changed"]["issues"], json!([id]));
    assert_eq!(changes[2]["added"]["comments"], json!([c["comment"]]));
    assert_eq!(
        changes[2]["changed"]["issues"],
        json!([id]),
        "a comment changes its issue"
    );
    assert_eq!(changes[4]["removed"]["issues"], json!([id]));
    assert_eq!(changes[4]["removed"]["comments"], json!([c["comment"]]));
    assert_eq!(changes[0]["origin"], "agent");
    // The kinds are checked and other kinds do not see issue changes.
    let objects = ok(
        &mut s,
        "events.poll",
        json!({"since_revision": rev, "kinds": ["objects"]}),
    );
    assert_eq!(objects["changes"], json!([]));
    assert_eq!(
        fail(
            &mut s,
            "events.poll",
            json!({"since_revision": rev, "kinds": ["nope"]})
        )
        .kind,
        "invalid_params"
    );
}

#[test]
fn a_capture_keeps_the_view_the_selection_and_a_snapshot() {
    let mut s = session();
    let a = rect(&mut s, 0);
    let b = rect(&mut s, 5000);
    let hidden = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Hidden"}}),
    )["created"][0]
        .clone();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_visibility", "layers": [hidden], "mode": "invisible"}}),
    );
    let r = ok(
        &mut s,
        "issue.create",
        json!({"title": "Look here", "objects": [a], "capture": true, "now_ms": 9_000}),
    );
    assert_eq!(r["viewpoints"].as_array().unwrap().len(), 1);
    let g = ok(
        &mut s,
        "issue.get",
        json!({"id": r["id"], "snapshots": true}),
    );
    let vp = &g["viewpoints"][0];
    assert_eq!(vp["view"]["kind"], "plan");
    assert_eq!(vp["selection"], json!([a]));
    let hash = vp["snapshot"].as_str().unwrap();
    assert_eq!(hash.len(), 64);
    // The layers: the hidden one is hidden in the viewpoint.
    let layer_pairs = vp["layers"].as_array().unwrap();
    assert!(layer_pairs.iter().any(|p| p[0] == hidden && p[1] == false));
    // The snapshot is a PNG picture, stored once as an image resource.
    let png = base64::engine::general_purpose::STANDARD
        .decode(vp["snapshot_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(&png[1..4], b"PNG");
    assert_eq!(kadakas_core::picture::sha256_hex(&png), hash);
    assert!(s.editor.doc().resources.find_image(hash).is_some());
    // The same picture in a second issue does not make a second resource.
    ok(
        &mut s,
        "issue.create",
        json!({"title": "Same", "objects": [a], "capture": true}),
    );
    let images = s
        .editor
        .doc()
        .resources
        .iter()
        .filter(|r| r.body.type_name() == "image")
        .count();
    assert_eq!(images, 1);
    // The selection of the session counts when no objects are given.
    s.selection = vec![kadakas_core::model::ObjectId(
        kadakas_core::uid::Uid::parse(&b).unwrap(),
    )];
    let c = ok(
        &mut s,
        "issue.create",
        json!({"title": "Selected", "capture": true}),
    );
    let g = ok(&mut s, "issue.get", json!({"id": c["id"]}));
    assert_eq!(g["viewpoints"][0]["selection"], json!([b]));
    // A 3D view of the session gives a camera.
    ok(&mut s, "view.set", json!({"view": "right_isometric"}));
    let d = ok(
        &mut s,
        "issue.create",
        json!({"title": "3D", "capture": true}),
    );
    let g = ok(&mut s, "issue.get", json!({"id": d["id"]}));
    assert_eq!(g["viewpoints"][0]["view"]["kind"], "camera");
    // Undo removes the issue and its picture.
    for _ in 0..4 {
        assert!(s.editor.undo());
    }
    assert!(s.editor.doc().resources.find_image(hash).is_none());
}

#[test]
fn a_viewpoint_can_be_given_by_hand() {
    let mut s = session();
    let r = ok(
        &mut s,
        "issue.create",
        json!({"title": "By hand", "viewpoint": {"name": "Close", "capture_visibility": false,
            "view": {"kind": "camera", "target": [0, 0, 0], "yaw": 0.5, "pitch": 0.5, "distance": 5000},
            "snapshot_base64": base64::engine::general_purpose::STANDARD.encode(png_header(8, 8))}}),
    );
    let g = ok(&mut s, "issue.get", json!({"id": r["id"]}));
    let vp = &g["viewpoints"][0];
    assert_eq!(vp["name"], "Close");
    assert_eq!(vp["layers"], Value::Null);
    assert!(vp["snapshot"].is_string());
    // A picture that is not valid, one over the limit, a view that is not valid.
    let e = fail(
        &mut s,
        "issue.create",
        json!({"title": "x", "viewpoint": {"snapshot_base64": "bm90IGEgcGljdHVyZQ=="}}),
    );
    assert_eq!(e.kind, "validation");
    let big = vec![0u8; MAX_SNAPSHOT_BYTES + 1];
    let e = fail(
        &mut s,
        "issue.create",
        json!({"title": "x", "viewpoint": {"snapshot_base64": base64::engine::general_purpose::STANDARD.encode(big)}}),
    );
    assert_eq!(e.kind, "limit");
    let e = fail(
        &mut s,
        "issue.create",
        json!({"title": "x", "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 0}}}),
    );
    assert_eq!(e.kind, "validation");
    let e = fail(
        &mut s,
        "issue.create",
        json!({"title": "x", "viewpoint": {"snapshot_base64": "!!"}}),
    );
    assert_eq!(e.kind, "invalid_params");
    assert_eq!(
        ok(&mut s, "issue.list", json!({}))["total"],
        1,
        "nothing was made"
    );
}

fn png_header(w: u32, h: u32) -> Vec<u8> {
    let mut img = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut img, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer
            .write_image_data(&vec![200u8; (w * h * 4) as usize])
            .unwrap();
    }
    img
}

#[test]
fn create_checks_its_input() {
    let mut s = session();
    let e = fail(
        &mut s,
        "issue.create",
        json!({"title": "x", "objects": ["0190f1c2-7a3b-7c1d-8e2f-123456789abc"]}),
    );
    assert_eq!(e.kind, "not_found");
    for (p, kind) in [
        (json!({}), "invalid_params"),
        (json!({"title": "x", "nope": 1}), "invalid_params"),
        (json!({"title": ""}), "validation"),
        (json!({"title": "x".repeat(300)}), "validation"),
        (json!({"title": "x", "due": "tomorrow"}), "validation"),
        (json!({"title": "x", "labels": ["a", "A"]}), "validation"),
        (json!({"title": "x", "status": "weird"}), "invalid_params"),
        (json!({"title": "x", "point": [1, 2]}), "invalid_params"),
        (json!("text"), "invalid_params"),
    ] {
        assert_eq!(fail(&mut s, "issue.create", p.clone()).kind, kind, "{p}");
    }
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 0);
}

#[test]
fn list_filters_sorts_and_lists_facets() {
    let mut s = session();
    let o = rect(&mut s, 0);
    ok(
        &mut s,
        "issue.create",
        json!({"title": "Tree", "assignee": "Ann", "labels": ["Planting"], "objects": [o], "now_ms": 1000}),
    );
    let b = ok(
        &mut s,
        "issue.create",
        json!({"title": "Sprinkler", "assignee": "bo", "now_ms": 2000, "description": "At the Gate"}),
    );
    ok(
        &mut s,
        "issue.create",
        json!({"title": "Wall", "labels": ["planting", "hardscape"], "now_ms": 3000}),
    );
    ok(
        &mut s,
        "issue.resolve",
        json!({"id": b["id"], "now_ms": 9000}),
    );
    let all = ok(&mut s, "issue.list", json!({}));
    assert_eq!(ids(&all), vec!["Tree", "Sprinkler", "Wall"]);
    assert_eq!(all["facets"]["assignees"], json!(["Ann", "bo"]));
    assert_eq!(all["facets"]["labels"], json!(["hardscape", "Planting"]));
    assert_eq!(all["issues"][0]["comments"], 0);
    let q = |s: &mut Session, p: Value| ids(&ok(s, "issue.list", p));
    assert_eq!(q(&mut s, json!({"status": "resolved"})), vec!["Sprinkler"]);
    assert_eq!(
        q(&mut s, json!({"status": ["open", "resolved"]})),
        vec!["Tree", "Sprinkler", "Wall"]
    );
    assert_eq!(q(&mut s, json!({"open_only": true})), vec!["Tree", "Wall"]);
    assert_eq!(q(&mut s, json!({"assignee": "ann"})), vec!["Tree"]);
    assert_eq!(
        q(&mut s, json!({"label": "PLANTING"})),
        vec!["Tree", "Wall"]
    );
    assert_eq!(q(&mut s, json!({"object": o})), vec!["Tree"]);
    assert_eq!(q(&mut s, json!({"text": "gate"})), vec!["Sprinkler"]);
    assert_eq!(
        q(&mut s, json!({"origin": "agent"})),
        vec!["Tree", "Sprinkler", "Wall"]
    );
    assert!(q(&mut s, json!({"origin": "user"})).is_empty());
    assert_eq!(
        q(&mut s, json!({"sort": "modified"})),
        vec!["Sprinkler", "Wall", "Tree"]
    );
    assert_eq!(
        q(&mut s, json!({"sort": "status"})),
        vec!["Tree", "Wall", "Sprinkler"]
    );
    let limited = ok(&mut s, "issue.list", json!({"limit": 1}));
    assert_eq!(
        (limited["count"].as_u64(), limited["total"].as_u64()),
        (Some(1), Some(3))
    );
    for p in [
        json!({"limit": 0}),
        json!({"sort": "random"}),
        json!({"status": "weird"}),
        json!({"origin": "robot"}),
        json!({"object": "nope"}),
        json!({"open_only": "yes"}),
    ] {
        assert_eq!(
            fail(&mut s, "issue.list", p.clone()).kind,
            "invalid_params",
            "{p}"
        );
    }
}

#[test]
fn update_changes_fields_and_no_change_makes_no_step() {
    let mut s = session();
    let a = rect(&mut s, 0);
    let b = rect(&mut s, 5000);
    let id = ok(&mut s, "issue.create", json!({"title": "A", "assignee": "Ann", "labels": ["one"], "objects": [a], "point": [1, 2, 3], "due": "2026-01-01", "now_ms": 1000}))["id"].clone();
    let depth = s.editor.undo_depth();
    let r = ok(
        &mut s,
        "issue.update",
        json!({"id": id, "title": "B", "description": "d", "type": "request", "priority": "low",
               "status": "in_progress", "add_labels": ["Two", "ONE"], "remove_labels": [],
               "add_objects": [b], "now_ms": 2000}),
    );
    assert_eq!(r["changed"], true);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    assert_eq!(g["title"], "B");
    assert_eq!(g["type"], "request");
    assert_eq!(g["priority"], "low");
    assert_eq!(g["status"], "in_progress");
    assert_eq!(
        g["labels"],
        json!(["one", "Two"]),
        "a label is not added twice"
    );
    assert_eq!(g["objects"], json!([a, b]));
    assert_eq!(g["modified_ms"], 2000);
    // Clear fields.
    ok(
        &mut s,
        "issue.update",
        json!({"id": id, "assignee": null, "due": "", "point": null, "remove_objects": [a], "remove_labels": ["ONE"], "now_ms": 3000}),
    );
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    assert_eq!(g["assignee"], Value::Null);
    assert_eq!(g["due"], Value::Null);
    assert_eq!(g["point"], Value::Null);
    assert_eq!(g["objects"], json!([b]));
    assert_eq!(g["labels"], json!(["Two"]));
    ok(
        &mut s,
        "issue.update",
        json!({"id": id, "labels": [], "objects": [], "point": [9, 8, 7]}),
    );
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    assert_eq!(
        (
            g["labels"].clone(),
            g["objects"].clone(),
            g["point"].clone()
        ),
        (json!([]), json!([]), json!([9.0, 8.0, 7.0]))
    );
    // No change, no step.
    let depth = s.editor.undo_depth();
    let r = ok(&mut s, "issue.update", json!({"id": id, "title": "B"}));
    assert_eq!(r["changed"], false);
    assert_eq!(s.editor.undo_depth(), depth);
    // Errors.
    for (p, kind) in [
        (
            json!({"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc", "title": "x"}),
            "not_found",
        ),
        (json!({"id": id, "title": ""}), "validation"),
        (json!({"id": id, "status": "nope"}), "invalid_params"),
        (json!({"id": id, "labels": "one"}), "invalid_params"),
        (json!({"id": id, "assignee": 5}), "invalid_params"),
        (json!({"id": id, "point": "here"}), "invalid_params"),
        (
            json!({"id": id, "objects": ["0190f1c2-7a3b-7c1d-8e2f-123456789abc"]}),
            "not_found",
        ),
        (
            json!({"id": id, "add_objects": ["0190f1c2-7a3b-7c1d-8e2f-123456789abc"]}),
            "not_found",
        ),
        (json!({"id": id, "typo": 1}), "invalid_params"),
        (json!({}), "invalid_params"),
        (json!({"id": "nope"}), "invalid_params"),
    ] {
        assert_eq!(fail(&mut s, "issue.update", p.clone()).kind, kind, "{p}");
    }
}

#[test]
fn comments_resolve_and_the_thread() {
    let mut s = session();
    let r = ok(
        &mut s,
        "issue.create",
        json!({"title": "A", "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 1}}, "now_ms": 1000}),
    );
    let id = r["id"].clone();
    let vp = r["viewpoints"][0].clone();
    let c1 = ok(
        &mut s,
        "issue.comment",
        json!({"id": id, "text": "first", "author": "Mari", "viewpoint": vp, "now_ms": 2000}),
    );
    assert_eq!(c1["issue"]["comments"], 1);
    ok(
        &mut s,
        "issue.comment",
        json!({"id": id, "text": "second", "now_ms": 1500}),
    );
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    let thread = g["comments"].as_array().unwrap();
    assert_eq!(thread[0]["text"], "second", "the thread is by time");
    assert_eq!(thread[1]["author"], "Mari");
    assert_eq!(thread[1]["origin"], "agent");
    assert_eq!(thread[1]["viewpoint"], vp);
    assert!(thread[1].get("issue").is_none());
    // Resolve with a comment is one undo step.
    let depth = s.editor.undo_depth();
    let r = ok(
        &mut s,
        "issue.resolve",
        json!({"id": id, "comment": "Fixed.", "now_ms": 3000}),
    );
    assert_eq!(r["issue"]["status"], "resolved");
    assert_eq!(r["issue"]["comments"], 3);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    s.editor.undo();
    assert_eq!(ok(&mut s, "issue.get", json!({"id": id}))["status"], "open");
    ok(
        &mut s,
        "issue.resolve",
        json!({"id": id, "status": "closed"}),
    );
    assert_eq!(
        ok(&mut s, "issue.get", json!({"id": id}))["status"],
        "closed"
    );
    let r = ok(
        &mut s,
        "issue.resolve",
        json!({"id": id, "status": "closed"}),
    );
    assert_eq!(r["changed"], false);
    for (m, p, kind) in [
        (
            "issue.comment",
            json!({"id": id, "text": " "}),
            "validation",
        ),
        ("issue.comment", json!({"id": id}), "invalid_params"),
        (
            "issue.comment",
            json!({"id": id, "text": "x", "viewpoint": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}),
            "validation",
        ),
        (
            "issue.comment",
            json!({"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc", "text": "x"}),
            "not_found",
        ),
        (
            "issue.comment",
            json!({"id": id, "text": "x", "viewpoint": "nope"}),
            "invalid_params",
        ),
        (
            "issue.comment",
            json!({"id": id, "text": "x", "typo": 1}),
            "invalid_params",
        ),
        (
            "issue.resolve",
            json!({"id": id, "status": "open"}),
            "invalid_params",
        ),
        (
            "issue.resolve",
            json!({"id": id, "typo": 1}),
            "invalid_params",
        ),
        (
            "issue.resolve",
            json!({"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}),
            "not_found",
        ),
        (
            "issue.get",
            json!({"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}),
            "not_found",
        ),
        (
            "issue.get",
            json!({"id": id, "snapshots": 1}),
            "invalid_params",
        ),
    ] {
        assert_eq!(fail(&mut s, m, p.clone()).kind, kind, "{m} {p}");
    }
}

#[test]
fn deleting_the_work_of_somebody_else_asks_first() {
    let mut s = session();
    // A person makes an issue (origin user) and comments on an agent's issue.
    let mine = ok(
        &mut s,
        "issue.create",
        json!({"title": "Mine", "now_ms": 1}),
    )["id"]
        .clone();
    let theirs = s
        .handle("issue.create", json!({"title": "Theirs", "now_ms": 2}))
        .unwrap()["id"]
        .clone();
    s.handle(
        "issue.comment",
        json!({"id": mine, "text": "from a person", "now_ms": 3}),
    )
    .unwrap();
    // The agent may not delete the issue of the person, or an issue with a comment of a person.
    for id in [&theirs, &mine] {
        let e = fail(&mut s, "issue.delete", json!({"id": id}));
        assert_eq!(e.kind, "conflict");
        assert_eq!(e.reason, Some("confirm_needed"));
        let d = e.details.as_ref().unwrap();
        assert_eq!(d["ask_the_person"], true);
        assert_eq!(d["issues"][0]["id"], *id);
        assert!(d["issues"][0]["why"].as_str().unwrap().len() > 5);
    }
    assert_eq!(
        ok(&mut s, "issue.list", json!({}))["total"],
        2,
        "nothing was deleted"
    );
    // One that needs the consent stops the whole call, also for the others.
    let own = ok(&mut s, "issue.create", json!({"title": "Own", "now_ms": 4}))["id"].clone();
    let e = fail(&mut s, "issue.delete", json!({"ids": [own, theirs]}));
    assert_eq!(e.reason, Some("confirm_needed"));
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 3);
    // Its own issue goes without a question. confirm false is no confirm.
    assert_eq!(
        fail(
            &mut s,
            "issue.delete",
            json!({"id": theirs, "confirm": false})
        )
        .kind,
        "conflict"
    );
    let r = ok(&mut s, "issue.delete", json!({"id": own}));
    assert_eq!(r["count"], 1);
    // With the consent, both go in one undo step.
    let depth = s.editor.undo_depth();
    let r = ok(
        &mut s,
        "issue.delete",
        json!({"ids": [theirs, mine], "confirm": true}),
    );
    assert_eq!(r["count"], 2);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 0);
    s.editor.undo();
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 2);
    // The person deleting an issue of an agent is asked too (the app asks).
    let agent_made = ok(
        &mut s,
        "issue.create",
        json!({"title": "Agent", "now_ms": 5}),
    )["id"]
        .clone();
    let e = s
        .handle("issue.delete", json!({"id": agent_made}))
        .unwrap_err();
    assert_eq!(e.reason, Some("confirm_needed"));
    for (p, kind) in [
        (json!({}), "invalid_params"),
        (
            json!({"id": agent_made, "ids": [agent_made]}),
            "invalid_params",
        ),
        (
            json!({"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}),
            "not_found",
        ),
        (
            json!({"id": agent_made, "confirm": "yes"}),
            "invalid_params",
        ),
        (json!({"id": agent_made, "typo": 1}), "invalid_params"),
    ] {
        assert_eq!(fail(&mut s, "issue.delete", p.clone()).kind, kind, "{p}");
    }
}

// ----- BCF -----

fn full_session() -> (Session, String, String) {
    let mut s = session();
    let a = rect(&mut s, 0);
    let b = rect(&mut s, 5000);
    let id = ok(
        &mut s,
        "issue.create",
        json!({"title": "Dead tree", "description": "See <photo> & plan", "type": "error",
               "priority": "high", "assignee": "mari@example.org", "labels": ["planting"],
               "due": "2026-12-01", "objects": [a, b], "author": "ann@example.org",
               "capture": true, "now_ms": 1_700_000_000_000i64}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &mut s,
        "issue.comment",
        json!({"id": id, "text": "On it", "author": "mari@example.org", "now_ms": 1_700_000_100_000i64}),
    );
    (s, a, id)
}

fn export(s: &mut Session, version: &str) -> Vec<u8> {
    let r = ok(s, "issue.export_bcf", json!({"version": version}));
    assert_eq!(r["version"], version);
    base64::engine::general_purpose::STANDARD
        .decode(r["base64"].as_str().unwrap())
        .unwrap()
}

#[test]
fn bcf_goes_out_and_in_in_both_versions() {
    for version in ["3.0", "2.1"] {
        let (mut s, a, id) = full_session();
        let r = ok(&mut s, "issue.export_bcf", json!({"version": version}));
        assert_eq!(r["topics"], 1);
        assert_eq!(r["viewpoints"], 1);
        assert_eq!(r["snapshots"], 1);
        assert_eq!(r["comments"], 1);
        let b64 = r["base64"].as_str().unwrap().to_string();
        // The same document: nothing to do.
        let back = ok(&mut s, "issue.import_bcf", json!({"base64": b64}));
        assert_eq!(back["version"], version);
        assert_eq!(back["imported"], 0);
        assert_eq!(back["unchanged"], 1);
        // Another document: the issue comes with its comments, its viewpoint and its snapshot.
        let mut t = session();
        let depth = t.editor.undo_depth();
        let back = ok(&mut t, "issue.import_bcf", json!({"base64": b64}));
        assert_eq!(
            (back["imported"].clone(), back["pictures"].clone()),
            (json!(1), json!(1))
        );
        assert_eq!(back["created"], json!([id]));
        assert_eq!(t.editor.undo_depth(), depth + 1, "one undo step");
        let g = ok(&mut t, "issue.get", json!({"id": id, "snapshots": true}));
        assert_eq!(g["title"], "Dead tree");
        assert_eq!(g["description"], "See <photo> & plan");
        assert_eq!(g["status"], "open");
        assert_eq!(g["origin"], "agent");
        assert_eq!(g["author"], "ann@example.org");
        assert_eq!(g["assignee"], "mari@example.org");
        assert_eq!(g["labels"], json!(["planting"]));
        assert_eq!(g["due"], "2026-12-01");
        assert_eq!(g["comments"].as_array().unwrap().len(), 1);
        assert_eq!(g["comments"][0]["text"], "On it");
        assert_eq!(g["objects"].as_array().unwrap().len(), 2);
        assert!(g["objects"].as_array().unwrap().contains(&json!(a)));
        // These objects are not in the new document.
        assert_eq!(g["objects_missing"].as_array().unwrap().len(), 2);
        assert!(g["viewpoints"][0]["snapshot_base64"].is_string());
        // The change event names the import.
        let polled = ok(
            &mut t,
            "events.poll",
            json!({"since_revision": 0, "kinds": ["issues"]}),
        );
        assert_eq!(polled["changes"][0]["op"], "issue_import_bcf");
        assert_eq!(polled["changes"][0]["detail"]["imported"], 1);
        // Undo removes the issue and its picture.
        t.editor.undo();
        assert_eq!(ok(&mut t, "issue.list", json!({}))["total"], 0);
        assert_eq!(t.editor.doc().resources.len(), 0);
    }
}

#[test]
fn a_conflict_asks_and_never_overwrites() {
    let (mut s, _a, id) = full_session();
    let b64 = base64::engine::general_purpose::STANDARD.encode(export(&mut s, "3.0"));
    // The person changes the issue here and a comment arrives from the file.
    let mut other = session();
    ok(&mut other, "issue.import_bcf", json!({"base64": b64}));
    ok(
        &mut other,
        "issue.update",
        json!({"id": id, "status": "resolved", "title": "Dead tree (done)", "now_ms": 1_700_000_500_000i64}),
    );
    ok(
        &mut other,
        "issue.comment",
        json!({"id": id, "text": "Fixed it", "now_ms": 1_700_000_600_000i64}),
    );
    let file = base64::engine::general_purpose::STANDARD.encode(export(&mut other, "3.0"));
    // Import into the first session: a conflict, nothing changes.
    let before = s.editor.revision();
    let e = fail(&mut s, "issue.import_bcf", json!({"base64": file}));
    assert_eq!(e.kind, "conflict");
    assert_eq!(e.reason, Some("bcf_conflict"));
    let d = e.details.as_ref().unwrap();
    assert_eq!(d["ask_the_person"], true);
    assert_eq!(d["conflicts"][0]["id"], id);
    assert_eq!(d["conflicts"][0]["ours"]["status"], "open");
    assert_eq!(d["conflicts"][0]["file"]["status"], "resolved");
    assert_eq!(d["options"], json!(["skip", "replace", "keep_both"]));
    assert_eq!(s.editor.revision(), before, "nothing was imported");
    // A dry run shows the plan and changes nothing.
    let plan = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "dry_run": true}),
    );
    assert_eq!(plan["dry_run"], true);
    assert_eq!(plan["plan"]["conflicts"].as_array().unwrap().len(), 1);
    assert_eq!(s.editor.revision(), before);
    // skip keeps ours.
    let r = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "on_conflict": "skip"}),
    );
    assert_eq!(
        (r["skipped_conflicts"].clone(), r["imported"].clone()),
        (json!(1), json!(0))
    );
    assert_eq!(s.editor.revision(), before, "no step for nothing");
    assert_eq!(ok(&mut s, "issue.get", json!({"id": id}))["status"], "open");
    // keep_both adds a copy with new IDs.
    let r = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "on_conflict": "keep_both"}),
    );
    assert_eq!(r["kept_both"], 1);
    let list = ok(&mut s, "issue.list", json!({}));
    assert_eq!(list["total"], 2);
    let copy = r["created"][0].as_str().unwrap();
    assert_ne!(copy, id);
    let g = ok(&mut s, "issue.get", json!({"id": copy}));
    assert_eq!(g["title"], "Dead tree (done)");
    assert_eq!(g["comments"].as_array().unwrap().len(), 2);
    assert_eq!(g["viewpoints"].as_array().unwrap().len(), 1);
    assert!(g["viewpoints"][0]["snapshot"].is_string());
    // The same file again: the copy now has the same content as... no: the copy has a new ID, so
    // the file conflicts with the first issue again.
    assert_eq!(
        fail(&mut s, "issue.import_bcf", json!({"base64": file})).reason,
        Some("bcf_conflict")
    );
    s.editor.undo();
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 1);
    // replace takes the values of the file, keeps what only we have.
    ok(
        &mut s,
        "issue.comment",
        json!({"id": id, "text": "Only here", "now_ms": 1_700_000_700_000i64}),
    );
    let depth = s.editor.undo_depth();
    let r = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "on_conflict": "replace"}),
    );
    assert_eq!(r["replaced"], 1);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    assert_eq!(g["status"], "resolved");
    assert_eq!(g["title"], "Dead tree (done)");
    let texts: Vec<&str> = g["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["On it", "Fixed it", "Only here"]);
    assert_eq!(g["viewpoints"].as_array().unwrap().len(), 1);
    // Layers of the viewpoint (not in BCF) stay.
    assert!(g["viewpoints"][0]["layers"].is_array());
    // The same file now is the same content except the comment that only we have: a conflict.
    s.editor.undo();
    assert_eq!(ok(&mut s, "issue.get", json!({"id": id}))["status"], "open");
}

#[test]
fn a_changed_comment_text_is_taken_by_replace() {
    let (mut s, _a, id) = full_session();
    let mut other = session();
    let b64 = base64::engine::general_purpose::STANDARD.encode(export(&mut s, "2.1"));
    ok(&mut other, "issue.import_bcf", json!({"base64": b64}));
    let comment = ok(&mut other, "issue.get", json!({"id": id}))["comments"][0]["id"].clone();
    other
        .editor
        .execute(&kadakas_core::command::Command::EditIssueComment {
            id: serde_json::from_value(comment).unwrap(),
            text: "Edited text".into(),
            now: 1_800_000_000_000,
        })
        .unwrap();
    let file = base64::engine::general_purpose::STANDARD.encode(export(&mut other, "2.1"));
    ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "on_conflict": "replace"}),
    );
    assert_eq!(
        ok(&mut s, "issue.get", json!({"id": id}))["comments"][0]["text"],
        "Edited text"
    );
}

#[test]
fn export_picks_issues_and_checks_its_input() {
    let (mut s, _a, id) = full_session();
    new_issue(&mut s, "Second");
    ok(&mut s, "issue.resolve", json!({"id": id}));
    let r = ok(&mut s, "issue.export_bcf", json!({"open_only": true}));
    assert_eq!(r["topics"], 1);
    let r = ok(&mut s, "issue.export_bcf", json!({"ids": [id]}));
    assert_eq!(r["topics"], 1);
    let r = ok(&mut s, "issue.export_bcf", json!({"status": ["resolved"]}));
    assert_eq!(r["topics"], 1);
    let r = ok(&mut s, "issue.export_bcf", json!({}));
    assert_eq!(r["topics"], 2);
    for (p, kind) in [
        (json!({"version": "4.0"}), "invalid_params"),
        (json!({"version": "2.0"}), "invalid_params"),
        (
            json!({"ids": ["0190f1c2-7a3b-7c1d-8e2f-123456789abc"]}),
            "not_found",
        ),
        (json!({"typo": 1}), "invalid_params"),
        (
            json!({"overwrite": 3, "path": "/nonexistent-dir-xyz/a.bcfzip"}),
            "invalid_params",
        ),
    ] {
        assert_eq!(
            fail(&mut s, "issue.export_bcf", p.clone()).kind,
            kind,
            "{p}"
        );
    }
}

#[test]
fn files_are_written_and_read_inside_the_allowed_folders() {
    let (mut s, _a, id) = full_session();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.bcfzip");
    let r = ok(
        &mut s,
        "issue.export_bcf",
        json!({"path": path.display().to_string(), "version": "3.0"}),
    );
    assert!(std::path::Path::new(r["path"].as_str().unwrap()).exists());
    assert!(r.get("base64").is_none());
    let mut t = session();
    let r = ok(
        &mut t,
        "issue.import_bcf",
        json!({"path": path.display().to_string()}),
    );
    assert_eq!(r["imported"], 1);
    assert_eq!(
        ok(&mut t, "issue.get", json!({"id": id}))["title"],
        "Dead tree"
    );
    // A restricted session: a folder outside is refused, the extension must fit, a file is not
    // replaced without overwrite.
    let other = tempfile::tempdir().unwrap();
    let mut r = session();
    r.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    r.handle("issue.create", json!({"title": "x"})).unwrap();
    let outside = other.path().join("x.bcfzip").display().to_string();
    assert_eq!(
        fail(&mut r, "issue.export_bcf", json!({"path": outside})).kind,
        "policy"
    );
    assert_eq!(
        fail(&mut r, "issue.import_bcf", json!({"path": outside})).kind,
        "policy"
    );
    let wrong = dir.path().join("x.txt").display().to_string();
    assert_eq!(
        fail(&mut r, "issue.export_bcf", json!({"path": wrong})).kind,
        "policy"
    );
    let inside = dir.path().join("in.bcfzip").display().to_string();
    ok(&mut r, "issue.export_bcf", json!({"path": inside}));
    assert_eq!(
        fail(&mut r, "issue.export_bcf", json!({"path": inside})).kind,
        "conflict"
    );
    ok(
        &mut r,
        "issue.export_bcf",
        json!({"path": inside, "overwrite": true}),
    );
    // A file that is not there.
    let missing = dir.path().join("none.bcfzip").display().to_string();
    assert!(
        r.handle("issue.import_bcf", json!({"path": missing}))
            .is_err()
    );
    // path and base64 together, neither.
    assert_eq!(
        fail(
            &mut s,
            "issue.import_bcf",
            json!({"path": inside, "base64": "AA=="})
        )
        .kind,
        "invalid_params"
    );
    assert_eq!(
        fail(&mut s, "issue.import_bcf", json!({})).kind,
        "invalid_params"
    );
}

#[test]
fn a_hostile_file_is_an_error_or_a_listed_skip() {
    let mut s = session();
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let e = fail(
        &mut s,
        "issue.import_bcf",
        json!({"base64": b64(b"not a zip")}),
    );
    assert_eq!(e.kind, "validation");
    let e = fail(&mut s, "issue.import_bcf", json!({"base64": "!!!"}));
    assert_eq!(e.kind, "invalid_params");
    // A bomb: a limit.
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    z.start_file("bcf.version", o).unwrap();
    std::io::Write::write_all(&mut z, b"<Version VersionId=\"3.0\"/>").unwrap();
    z.start_file("a/bomb", o).unwrap();
    std::io::Write::write_all(&mut z, &vec![0u8; 4 * 1024 * 1024]).unwrap();
    let bomb = z.finish().unwrap().into_inner();
    let e = fail(&mut s, "issue.import_bcf", json!({"base64": b64(&bomb)}));
    assert_eq!(e.kind, "limit");
    // A file with a bad topic imports the good ones and lists the bad one.
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    z.start_file("bcf.version", o).unwrap();
    std::io::Write::write_all(&mut z, b"<Version VersionId=\"3.0\"/>").unwrap();
    z.start_file("aaaaaaaa-1111-4222-8333-444455556666/markup.bcf", o)
        .unwrap();
    std::io::Write::write_all(
        &mut z,
        br#"<Markup><Topic Guid="aaaaaaaa-1111-4222-8333-444455556666" TopicType="Issue" TopicStatus="Open"><Title>Good</Title><CreationDate>2026-01-01T00:00:00Z</CreationDate><CreationAuthor>a</CreationAuthor></Topic></Markup>"#,
    )
    .unwrap();
    z.start_file("bbbbbbbb-1111-4222-8333-444455556666/markup.bcf", o)
        .unwrap();
    std::io::Write::write_all(&mut z, b"<Markup><Topic/></Markup>").unwrap();
    let mixed = z.finish().unwrap().into_inner();
    let r = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": b64(&mixed), "now_ms": 5}),
    );
    assert_eq!(r["imported"], 1);
    assert_eq!(r["topics_skipped"].as_array().unwrap().len(), 1);
    assert_eq!(
        r["topics_skipped"][0]["topic"],
        "bbbbbbbb-1111-4222-8333-444455556666"
    );
    for (p, kind) in [
        (
            json!({"base64": "AA==", "on_conflict": "overwrite"}),
            "invalid_params",
        ),
        (json!({"base64": "AA==", "typo": 1}), "invalid_params"),
        (json!({"base64": 5}), "invalid_params"),
    ] {
        assert_eq!(
            fail(&mut s, "issue.import_bcf", p.clone()).kind,
            kind,
            "{p}"
        );
    }
}

#[test]
fn an_import_checks_the_limits_of_the_document() {
    // The issue count of the document is a limit that stops the whole import (one undo step).
    let (mut s, _a, _id) = full_session();
    let file = export(&mut s, "3.0");
    let mut t = session();
    let mut doc = t.editor.doc().clone();
    doc.issues = kadakas_core::issue::IssueData::from_vecs(
        (0..kadakas_core::issue::MAX_ISSUES as u64)
            .map(|n| kadakas_core::issue::Issue {
                id: kadakas_core::issue::IssueId::from_num(n + 10),
                title: "x".into(),
                ..Default::default()
            })
            .collect(),
        vec![],
    )
    .into();
    t = Session::with_document(doc);
    let e = t
        .handle(
            "issue.import_bcf",
            json!({"base64": base64::engine::general_purpose::STANDARD.encode(file)}),
        )
        .unwrap_err();
    assert_eq!(e.kind, "validation");
    assert_eq!(t.editor.doc().issues.len(), kadakas_core::issue::MAX_ISSUES);
}

#[test]
fn the_self_description_covers_issues() {
    let d = describe();
    assert!(d["examples"].as_array().unwrap().len() >= 8);
    assert!(d["bcf"]["limits"].is_string());
    let all = json!(crate::METHODS.iter().map(|(m, _)| *m).collect::<Vec<_>>());
    for m in [
        "issue.list",
        "issue.get",
        "issue.create",
        "issue.update",
        "issue.comment",
        "issue.resolve",
        "issue.delete",
        "issue.import_bcf",
        "issue.export_bcf",
    ] {
        assert!(all.as_array().unwrap().contains(&json!(m)), "{m}");
    }
    let mut s = session();
    let full = ok(&mut s, "describe", json!({}));
    assert!(full["review"]["issues"]["model"].is_string());
    assert_eq!(
        fail(&mut s, "issue.nope", json!({})).kind,
        "method_not_found"
    );
}

#[test]
fn issues_follow_the_session_limits() {
    // An issue call that breaks the memory limit of the session is undone, like any change.
    let mut s = session();
    let mut limits = s.limits().clone();
    limits.max_objects = 1;
    s.set_limits(limits);
    let r = ok(&mut s, "issue.create", json!({"title": "Fits"}));
    assert!(r["id"].is_string());
}

#[test]
fn viewpoints_are_added_and_removed_through_the_api() {
    let mut s = session();
    let o = rect(&mut s, 0);
    let made = ok(
        &mut s,
        "issue.create",
        json!({"title": "A", "objects": [o], "now_ms": 1}),
    );
    let id = made["id"].clone();
    assert_eq!(made["viewpoints"], json!([]), "no viewpoint unless asked");
    // The current view of the session, with a snapshot.
    let depth = s.editor.undo_depth();
    let r = ok(
        &mut s,
        "issue.add_viewpoint",
        json!({"id": id, "name": "Close", "snapshot": true, "now_ms": 5}),
    );
    assert_eq!(s.editor.undo_depth(), depth + 1);
    let vp = r["viewpoint"].clone();
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    assert_eq!(g["viewpoints"][0]["name"], "Close");
    assert_eq!(
        g["viewpoints"][0]["selection"],
        json!([o]),
        "the objects of the issue"
    );
    let hash = g["viewpoints"][0]["snapshot"].as_str().unwrap().to_string();
    assert!(s.editor.doc().resources.find_image(&hash).is_some());
    // A viewpoint by hand, without a picture.
    let r2 = ok(
        &mut s,
        "issue.add_viewpoint",
        json!({"id": id, "view": {"kind": "plan", "center": [0, 0], "zoom": 2}, "selection": [],
               "capture_visibility": false, "now_ms": 6}),
    );
    assert_eq!(
        ok(&mut s, "issue.get", json!({"id": id}))["viewpoints"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Removing a viewpoint is part of update. Its picture stays in the document as a resource:
    // a command never deletes a resource (collab, and a person may want it back).
    let r = ok(
        &mut s,
        "issue.update",
        json!({"id": id, "remove_viewpoints": [vp], "now_ms": 7}),
    );
    assert_eq!(r["changed"], true);
    assert!(s.editor.doc().resources.find_image(&hash).is_some());
    assert_eq!(
        ok(&mut s, "issue.get", json!({"id": id}))["viewpoints"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    s.editor.undo();
    assert!(s.editor.doc().resources.find_image(&hash).is_some());
    for (m, p, kind) in [
        ("issue.add_viewpoint", json!({}), "invalid_params"),
        (
            "issue.add_viewpoint",
            json!({"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abc"}),
            "not_found",
        ),
        (
            "issue.add_viewpoint",
            json!({"id": id, "typo": 1}),
            "invalid_params",
        ),
        (
            "issue.add_viewpoint",
            json!({"id": id, "selection": ["0190f1c2-7a3b-7c1d-8e2f-123456789abc"]}),
            "not_found",
        ),
        (
            "issue.update",
            json!({"id": id, "remove_viewpoints": ["0190f1c2-7a3b-7c1d-8e2f-123456789abc"]}),
            "validation",
        ),
        (
            "issue.update",
            json!({"id": id, "remove_viewpoints": "x"}),
            "invalid_params",
        ),
    ] {
        assert_eq!(fail(&mut s, m, p.clone()).kind, kind, "{m} {p}");
    }
    assert!(r2["viewpoint"].is_string());
    // The limit of viewpoints for one issue.
    for n in 0..20 {
        let r = agent(
            &mut s,
            "issue.add_viewpoint",
            json!({"id": id, "capture_visibility": false, "now_ms": 10 + n}),
        );
        if n >= 14 {
            assert_eq!(r.unwrap_err().kind, "validation");
        }
    }
}

// ----- review fixes -----

fn zip_of(files: &[(&str, &[u8])]) -> String {
    use std::io::Write;
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        z.start_file(*name, o).unwrap();
        z.write_all(bytes).unwrap();
    }
    base64::engine::general_purpose::STANDARD.encode(z.finish().unwrap().into_inner())
}

fn snapshot_hash(s: &mut Session, id: &str) -> String {
    ok(s, "issue.get", json!({"id": id}))["viewpoints"][0]["snapshot"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_version_restore_keeps_the_pictures_of_issues() {
    // The sequence of the review: a version, an issue with a snapshot, then a restore of the
    // version. Versions do not hold issues, so the issue and its picture must stay.
    let mut s = session();
    rect(&mut s, 0);
    ok(
        &mut s,
        "version.create",
        json!({"name": "V1", "now_ms": 1_000}),
    );
    let id = ok(
        &mut s,
        "issue.create",
        json!({"title": "After V1", "capture": true, "now_ms": 2_000}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let hash = snapshot_hash(&mut s, &id);
    rect(&mut s, 5000);
    let r = ok(&mut s, "version.restore", json!({"id": 1, "now_ms": 3_000}));
    assert_eq!(r["changed"], true);
    let has = |s: &Session| {
        s.editor.doc().resources.find_image(&hash).is_some() && s.editor.doc().issues.len() == 1
    };
    assert!(has(&s), "the restore keeps the issue and its picture");
    assert_eq!(snapshot_hash(&mut s, &id), hash);
    // Undo and redo of the restore, and a save and reopen (document.get and load).
    assert!(s.editor.undo());
    assert!(has(&s), "undo of the restore");
    assert!(s.editor.redo());
    assert!(has(&s), "redo of the restore");
    let doc = ok(&mut s, "document.get", json!({}));
    let mut t = session();
    ok(&mut t, "document.load", json!({"document": doc}));
    assert!(has(&t), "after a save and reopen");
    // The picture of the issue is still the same file after all that.
    let g = ok(&mut t, "issue.get", json!({"id": id, "snapshots": true}));
    let png = base64::engine::general_purpose::STANDARD
        .decode(g["viewpoints"][0]["snapshot_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(kadakas_core::picture::sha256_hex(&png), hash);
}

#[test]
fn the_pictures_of_deleted_issues_stay_unless_the_caller_asks() {
    let mut s = session();
    let a = ok(
        &mut s,
        "issue.create",
        json!({"title": "A", "capture": true, "now_ms": 1}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let hash = snapshot_hash(&mut s, &a);
    // A second issue shares the picture.
    let b = ok(
        &mut s,
        "issue.create",
        json!({"title": "B", "capture": true, "now_ms": 2}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(snapshot_hash(&mut s, &b), hash);
    let rid = s.editor.doc().resources.find_image(&hash).unwrap();
    // The picture lists its issues.
    let used = ok(&mut s, "resource.get", json!({"id": rid.0}));
    assert_eq!(
        used["used_by"]["issues"].as_array().unwrap().len(),
        2,
        "{used}"
    );
    // A delete keeps the picture. With purge_snapshots, the picture of a shared snapshot stays
    // while another issue has it.
    ok(
        &mut s,
        "issue.delete",
        json!({"id": a, "purge_snapshots": true}),
    );
    assert!(s.editor.doc().resources.find_image(&hash).is_some());
    // The last issue with purge_snapshots: the picture goes, in the same undo step.
    let depth = s.editor.undo_depth();
    ok(
        &mut s,
        "issue.delete",
        json!({"id": b, "purge_snapshots": true}),
    );
    assert!(s.editor.doc().resources.find_image(&hash).is_none());
    assert_eq!(s.editor.undo_depth(), depth + 1);
    s.editor.undo();
    assert!(s.editor.doc().resources.find_image(&hash).is_some());
    assert_eq!(s.editor.doc().issues.len(), 1);
    // Without it the picture stays, and the person can delete it as a resource.
    ok(&mut s, "issue.delete", json!({"id": b}));
    assert!(s.editor.doc().resources.find_image(&hash).is_some());
}

#[test]
fn the_consent_compares_the_author_too() {
    let mut s = session();
    // The same origin (agent), two authors.
    let id = ok(
        &mut s,
        "issue.create",
        json!({"title": "Ann's", "author": "Ann", "capture": true, "now_ms": 1}),
    )["id"]
        .clone();
    let e = fail(&mut s, "issue.delete", json!({"id": id, "author": "Bob"}));
    assert_eq!(e.reason, Some("confirm_needed"));
    // The viewpoints of the issue of Ann are not removed by Bob without the consent.
    let vp = ok(&mut s, "issue.get", json!({"id": id}))["viewpoints"][0]["id"].clone();
    let e = fail(
        &mut s,
        "issue.update",
        json!({"id": id, "remove_viewpoints": [vp], "author": "Bob"}),
    );
    assert_eq!(e.reason, Some("confirm_needed"));
    assert_eq!(
        ok(&mut s, "issue.get", json!({"id": id}))["viewpoints"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let r = ok(
        &mut s,
        "issue.update",
        json!({"id": id, "remove_viewpoints": [vp], "author": "Bob", "confirm": true}),
    );
    assert_eq!(r["changed"], true);
    // Ann herself needs no consent.
    ok(&mut s, "issue.delete", json!({"id": id, "author": "Ann"}));
}

#[test]
fn issue_list_pages_with_an_offset() {
    let mut s = session();
    for n in 0..5 {
        new_issue(&mut s, &format!("I{n}"));
    }
    let p1 = ok(&mut s, "issue.list", json!({"limit": 2}));
    assert_eq!(ids(&p1), vec!["I0", "I1"]);
    assert_eq!(p1["next_offset"], 2);
    let p2 = ok(&mut s, "issue.list", json!({"limit": 2, "offset": 2}));
    assert_eq!(ids(&p2), vec!["I2", "I3"]);
    let p3 = ok(&mut s, "issue.list", json!({"limit": 2, "offset": 4}));
    assert_eq!(ids(&p3), vec!["I4"]);
    assert!(p3["next_offset"].is_null());
    assert_eq!(p3["total"], 5);
    let p4 = ok(&mut s, "issue.list", json!({"offset": 99}));
    assert_eq!(p4["count"], 0);
    assert_eq!(
        fail(&mut s, "issue.list", json!({"offset": -1})).kind,
        "invalid_params"
    );
}

const TOPIC: &str = "aaaaaaaa-1111-4222-8333-444455556666";

fn markup(topic: &str, title: &str, body: &str) -> String {
    format!(
        r#"<Markup><Topic Guid="{topic}" TopicType="Issue" TopicStatus="Open"><Title>{title}</Title><CreationDate>2026-01-01T00:00:00Z</CreationDate><CreationAuthor>a</CreationAuthor></Topic>{body}</Markup>"#
    )
}

#[test]
fn items_that_do_not_fit_ask_the_person_and_are_never_dropped_silently() {
    let mut s = session();
    let body = r#"<Comment Guid="cccccccc-1111-4222-8333-444455556666"><Date>2026-01-02T00:00:00Z</Date><Author>b</Author></Comment>
        <Viewpoints Guid="dddddddd-1111-4222-8333-444455556666"><Snapshot>junk.png</Snapshot></Viewpoints>"#;
    let file = zip_of(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        (
            &format!("{TOPIC}/markup.bcf"),
            markup(TOPIC, "With junk", body).as_bytes(),
        ),
        (&format!("{TOPIC}/junk.png"), b"this is not a picture"),
    ]);
    let before = s.editor.revision();
    let e = fail(&mut s, "issue.import_bcf", json!({"base64": file}));
    assert_eq!(e.kind, "conflict");
    assert_eq!(e.reason, Some("bcf_unfit"));
    let d = e.details.as_ref().unwrap();
    assert_eq!(d["ask_the_person"], true);
    assert_eq!(d["options"], json!(["skip_items", "cancel"]));
    let items: Vec<&str> = d["unfit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["item"].as_str().unwrap())
        .collect();
    assert!(
        items.contains(&"comment") && items.contains(&"snapshot"),
        "{d}"
    );
    assert_eq!(s.editor.revision(), before, "nothing was imported");
    // A dry run lists them and changes nothing.
    let plan = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "dry_run": true}),
    );
    assert_eq!(plan["unfit"].as_array().unwrap().len(), items.len());
    assert_eq!(plan["plan"]["unfit"], plan["unfit"]);
    assert_eq!(s.editor.revision(), before);
    // After the person said yes: imported without the items, and the call says which.
    let r = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "skip_items": true}),
    );
    assert_eq!(r["imported"], 1);
    assert_eq!(r["items_skipped"].as_array().unwrap().len(), items.len());
    assert_eq!(ok(&mut s, "issue.list", json!({}))["total"], 1);
}

#[test]
fn a_replace_keeps_our_picture_when_the_one_of_the_file_was_refused() {
    let (mut s, _a, id) = full_session();
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    let vp = g["viewpoints"][0]["id"].as_str().unwrap().to_string();
    let hash = g["viewpoints"][0]["snapshot"].as_str().unwrap().to_string();
    let body = format!(r#"<Viewpoints Guid="{vp}"><Snapshot>junk.png</Snapshot></Viewpoints>"#);
    let file = zip_of(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        (
            &format!("{id}/markup.bcf"),
            markup(&id, "Changed in the file", &body).as_bytes(),
        ),
        (&format!("{id}/junk.png"), b"this is not a picture"),
    ]);
    let r = ok(
        &mut s,
        "issue.import_bcf",
        json!({"base64": file, "on_conflict": "replace", "skip_items": true}),
    );
    assert_eq!(r["replaced"], 1);
    let g = ok(&mut s, "issue.get", json!({"id": id}));
    assert_eq!(g["title"], "Changed in the file");
    assert_eq!(
        g["viewpoints"][0]["snapshot"], hash,
        "our picture stays when the file's was refused"
    );
    assert!(s.editor.doc().resources.find_image(&hash).is_some());
}

#[test]
fn a_comment_guid_of_another_issue_does_not_fail_the_import() {
    let mut s = session();
    let c = "cccccccc-1111-4222-8333-444455556666";
    let comment = format!(
        r#"<Comment Guid="{c}"><Date>2026-01-02T00:00:00Z</Date><Author>b</Author><Comment>Same GUID</Comment></Comment>"#
    );
    let topic_a = "aaaaaaaa-1111-4222-8333-444455556666";
    let topic_b = "bbbbbbbb-1111-4222-8333-444455556666";
    // One file with two topics that both use the comment GUID, and then a second file.
    let file = zip_of(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        (
            &format!("{topic_a}/markup.bcf"),
            markup(topic_a, "A", &comment).as_bytes(),
        ),
        (
            &format!("{topic_b}/markup.bcf"),
            markup(topic_b, "B", &comment).as_bytes(),
        ),
    ]);
    let r = ok(&mut s, "issue.import_bcf", json!({"base64": file}));
    assert_eq!(r["imported"], 2, "{r}");
    let list = ok(&mut s, "issue.list", json!({}));
    for i in list["issues"].as_array().unwrap() {
        assert_eq!(i["comments"], 1, "each issue keeps its comment: {i}");
    }
    // The same comment GUID in a topic of a later file, when another issue has it already.
    let topic_c = "cccccccc-2222-4222-8333-444455556666";
    let file = zip_of(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        (
            &format!("{topic_c}/markup.bcf"),
            markup(topic_c, "C", &comment).as_bytes(),
        ),
    ]);
    let r = ok(&mut s, "issue.import_bcf", json!({"base64": file}));
    assert_eq!(r["imported"], 1, "{r}");
    assert_eq!(s.editor.doc().issues.data().comments.len(), 3);
}

#[test]
fn a_renumbered_copy_never_takes_an_id_that_another_topic_of_the_file_has() {
    let doc = Document::default();
    let old = IssueId::from_num(77);
    let mut taken: HashSet<IssueId> = HashSet::new();
    // The ID that the renumber would pick first is the ID of another topic of the file.
    let first = IssueId(Uid::derive(old.0, 10, 1));
    taken.insert(first);
    let mut used: HashSet<CommentId> = HashSet::new();
    let mut p = Prepared {
        issue: Issue {
            id: old,
            ..Issue::default()
        },
        comments: vec![],
        pictures: vec![],
        keep_snapshot: HashSet::new(),
        keep_view: HashSet::new(),
    };
    p.renumber(&doc, &mut taken, &mut used);
    assert_ne!(p.issue.id, first);
    assert_ne!(p.issue.id, old);
    assert!(taken.contains(&p.issue.id));
}
