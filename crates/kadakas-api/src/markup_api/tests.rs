use base64::Engine;
use kadakas_core::change::Origin;
use serde_json::{Value, json};

use super::*;
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

fn session() -> Session {
    Session::with_sequential_ids()
}

/// A box of 100 mm at the origin, as a surface model. Its top face is at z = 100.
fn box_model(s: &mut Session) -> String {
    let id = ok(s, "mesh.create", json!({"name": "Box"}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let c = |i: usize| -> [f64; 3] {
        std::array::from_fn(|k| if i >> k & 1 == 0 { 0.0 } else { 100.0 })
    };
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push(json!([c(i), c(i | bit)]));
            }
        }
    }
    ok(s, "mesh.add_edges", json!({"id": id, "edges": edges}));
    id
}

/// A rectangle with a fill, 10 mm wide, so a plan hit inside it counts.
fn filled_rect(s: &mut Session) -> String {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object",
            "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]},
            "attrs": {"fill": [200, 200, 200, 255]}}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string()
}

/// A top view of the box: 1 point of the canvas is about 4.1 mm. The middle of the canvas is the
/// middle of the box.
fn top_view() -> Value {
    json!({
        "view": {"kind": "camera", "target": [50, 50, 0],
                 "yaw": -std::f64::consts::FRAC_PI_2, "pitch": std::f64::consts::FRAC_PI_2,
                 "distance": 1000, "perspective": false},
        "canvas": [200, 200],
    })
}

/// A plan view of 10 mm by 10 mm: 10 points for 1 mm.
fn plan_view() -> Value {
    json!({
        "view": {"kind": "plan", "center": [5, 5], "zoom": 10},
        "canvas": [100, 100],
    })
}

fn mark(kind: &str, points: Value) -> Value {
    json!({"kind": kind, "points": points})
}

fn make(s: &mut Session, viewpoint: Value, marks: Value) -> String {
    ok(
        s,
        "markup.create",
        json!({"viewpoint": viewpoint, "marks": marks, "now_ms": 7000}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn get(s: &mut Session, id: &str) -> Value {
    ok(s, "markup.get", json!({"id": id, "render": false}))
}

fn png(v: &Value) -> (Vec<u8>, u32, u32) {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(v["png_base64"].as_str().unwrap())
        .unwrap();
    let dec = png::Decoder::new(std::io::Cursor::new(&bytes));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    let ch = info.color_type.samples();
    let mut rgba = Vec::new();
    for px in buf[..info.buffer_size()].chunks(ch) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    (rgba, info.width, info.height)
}

#[test]
fn an_agent_draws_a_markup_with_its_origin_and_one_undo_step() {
    let mut s = session();
    let depth = s.editor.undo_depth();
    let r = ok(
        &mut s,
        "markup.create",
        json!({
            "name": "Hedge", "note": "Add a hedge here.", "now_ms": 7000,
            "viewpoint": plan_view(),
            "marks": [
                {"kind": "arrow", "points": [[0.2, 0.2], [0.5, 0.6]], "color": "#d62828"},
                {"kind": "note", "points_canvas": [[20, 15]], "text": "Hedge"},
                {"kind": "pen", "points": [[0.1, 0.9], [0.5, 0.9], [0.9, 0.9]], "width": 5},
            ],
        }),
    );
    assert_eq!(r["marks"], 3);
    assert_eq!(s.editor.undo_depth(), depth + 1, "one undo step");
    let id = r["id"].as_str().unwrap().to_string();
    let m = get(&mut s, &id);
    assert_eq!(m["origin"], "agent");
    assert_eq!(m["name"], "Hedge");
    assert_eq!(m["created_ms"], 7000);
    assert_eq!(m["viewpoint"]["mode"], "plan");
    assert_eq!(m["viewpoint"]["canvas"], json!([100.0, 100.0]));
    let marks = m["marks"].as_array().unwrap();
    assert_eq!(marks.len(), 3);
    assert_eq!(marks[0]["kind"], "arrow");
    assert_eq!(marks[0]["color"], "#d62828");
    assert_eq!(marks[0]["id"], 1);
    assert_eq!(marks[2]["id"], 3);
    assert_eq!(marks[0]["points"], json!([[0.2, 0.2], [0.5, 0.6]]));
    assert_eq!(
        marks[0]["points_canvas"],
        json!([[20.0, 20.0], [50.0, 60.0]])
    );
    assert_eq!(marks[0]["bbox"]["relative"], json!([0.2, 0.2, 0.5, 0.6]));
    assert_eq!(marks[0]["bbox"]["canvas"], json!([20.0, 20.0, 50.0, 60.0]));
    assert_eq!(
        marks[1]["points"],
        json!([[0.2, 0.15]]),
        "canvas points become relative"
    );
    assert_eq!(marks[1]["text"], "Hedge");
    // Undo takes the whole markup back.
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "markup.list", json!({}))["total"], 0);
    // A markup is no object: the document has no object for it.
    assert!(s.editor.doc().objects.is_empty());
}

#[test]
fn the_list_has_the_summary_and_filters() {
    let mut s = session();
    let a = make(
        &mut s,
        plan_view(),
        json!([mark("pen", json!([[0.1, 0.1]]))]),
    );
    let b = make(&mut s, plan_view(), json!([]));
    let r = ok(&mut s, "markup.list", json!({}));
    assert_eq!(r["total"], 2);
    assert_eq!(r["markups"][0]["id"], a);
    assert_eq!(r["markups"][0]["marks"], 1);
    assert_eq!(r["markups"][0]["points"], 1);
    assert_eq!(r["markups"][1]["id"], b);
    assert_eq!(
        ok(&mut s, "markup.list", json!({"origin": "user"}))["total"],
        0
    );
    assert_eq!(
        ok(&mut s, "markup.list", json!({"origin": "agent"}))["total"],
        2
    );
    let page = ok(&mut s, "markup.list", json!({"limit": 1}));
    assert_eq!(page["count"], 1);
    assert_eq!(page["next_offset"], 1);
    let issue = ok(&mut s, "issue.create", json!({"title": "T", "now_ms": 1}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(&mut s, "markup.update", json!({"id": b, "issue": issue}));
    assert_eq!(
        ok(&mut s, "markup.list", json!({"issue": issue}))["total"],
        1
    );
    let u = ok(&mut s, "markup.update", json!({"id": b, "issue": null}));
    assert_eq!(u["markup"]["issue"], Value::Null);
    assert_eq!(u["changed"], true);
    let u = ok(&mut s, "markup.update", json!({"id": b, "note": "x"}));
    let again = ok(&mut s, "markup.update", json!({"id": b, "note": "x"}));
    assert_eq!(
        (u["changed"].clone(), again["changed"].clone()),
        (json!(true), json!(false))
    );
    let e = fail(
        &mut s,
        "markup.update",
        json!({"id": b, "issue": "00000000-0000-4000-8000-0000000000aa"}),
    );
    assert!(e.message.contains("does not exist"), "{}", e.message);
}

#[test]
fn bad_marks_and_limits_are_refused_and_nothing_is_made() {
    let mut s = session();
    let bad = |s: &mut Session, marks: Value| fail(s, "markup.create", json!({"marks": marks}));
    assert!(
        bad(&mut s, json!([mark("lasso", json!([[0.1, 0.1]]))]))
            .message
            .contains("kind")
    );
    assert!(
        bad(&mut s, json!([mark("arrow", json!([[0.1, 0.1]]))]))
            .message
            .contains("exactly 2")
    );
    assert!(
        bad(&mut s, json!([mark("pen", json!([[1.5, 0.1]]))]))
            .message
            .contains("0 to 1")
    );
    assert!(
        bad(&mut s, json!([{"kind": "note", "points": [[0.1, 0.1]]}]))
            .message
            .contains("text")
    );
    assert!(
        bad(&mut s, json!([{"kind": "pen"}]))
            .message
            .contains("points")
    );
    let many: Vec<Value> = (0..=kadakas_core::markup::MAX_MARKS)
        .map(|_| mark("pen", json!([[0.5, 0.5]])))
        .collect();
    assert!(bad(&mut s, json!(many)).message.contains("at most"));
    let long: Vec<[f64; 2]> = (0..=kadakas_core::markup::MAX_POINTS)
        .map(|i| [i as f64 / 2000.0, 0.5])
        .collect();
    assert!(
        bad(&mut s, json!([mark("pen", json!(long))]))
            .message
            .contains("at most")
    );
    assert_eq!(ok(&mut s, "markup.list", json!({}))["total"], 0);
    assert_eq!(s.editor.undo_depth(), 0);
}

#[test]
fn a_plan_mark_finds_the_object_under_it_and_a_stroke_beside_it_finds_none() {
    let mut s = session();
    let rect = filled_rect(&mut s);
    let id = make(
        &mut s,
        plan_view(),
        json!([
            mark("rectangle", json!([[0.3, 0.3], [0.7, 0.7]])),
            mark("pen", json!([[0.1, 0.05], [0.9, 0.05]])),
            mark("arrow", json!([[0.9, 0.9], [0.5, 0.5]])),
            json!({"kind": "note", "points": [[0.5, 0.5]], "text": "here"}),
        ]),
    );
    let m = get(&mut s, &id);
    let marks = m["marks"].as_array().unwrap();
    // The rectangle: inside it the filled rectangle is hit.
    assert_eq!(
        marks[0]["under"]["ids"],
        json!([rect]),
        "{}",
        marks[0]["under"]
    );
    assert!(marks[0]["under"]["samples"].as_u64().unwrap() <= 25);
    assert_eq!(marks[0]["under"]["objects"][0]["kind"], "rect");
    // The stroke runs at the top edge of the drawing: along y = 9.5 mm the rectangle is hit
    // for the whole length, so a stroke beside it is checked below.
    assert_eq!(marks[1]["under"]["ids"], json!([rect]));
    // The arrow: the tip is on the rectangle, the tail is at (9.0, 1.0) mm... also on it.
    assert_eq!(marks[2]["under"]["tip"]["ids"], json!([rect]));
    assert_eq!(marks[2]["under"]["tail"]["ids"], json!([rect]));
    // hint3d in the plan: the drawing point and the ids there.
    assert_eq!(marks[2]["hint3d"]["at"], "tip");
    assert_eq!(marks[2]["hint3d"]["kind"], "plan");
    assert_eq!(marks[2]["hint3d"]["point"], json!([5.0, 5.0]));
    assert_eq!(marks[3]["hint3d"]["at"], "point");
    // Now a view that shows an empty part of the drawing: nothing is under the marks.
    let far = make(
        &mut s,
        json!({"view": {"kind": "plan", "center": [500, 500], "zoom": 10}, "canvas": [100, 100]}),
        json!([mark("rectangle", json!([[0.3, 0.3], [0.7, 0.7]]))]),
    );
    let m = get(&mut s, &far);
    assert_eq!(m["marks"][0]["under"]["ids"], json!([]));
    assert_eq!(
        m["marks"][0]["under"]["nothing"],
        m["marks"][0]["under"]["samples"]
    );
}

#[test]
fn a_closed_stroke_covers_what_is_inside_it() {
    let mut s = session();
    let rect = filled_rect(&mut s);
    // A circle of strokes around the middle of the view.
    let ring: Vec<[f64; 2]> = (0..=24)
        .map(|i| {
            let a = f64::from(i) / 24.0 * std::f64::consts::TAU;
            [0.5 + 0.2 * a.cos(), 0.5 + 0.2 * a.sin()]
        })
        .collect();
    let id = make(&mut s, plan_view(), json!([mark("pen", json!(ring))]));
    let m = get(&mut s, &id);
    assert_eq!(m["marks"][0]["closed"], true);
    assert_eq!(m["marks"][0]["under"]["ids"], json!([rect]));
    assert!(m["marks"][0]["under"]["samples"].as_u64().unwrap() > 4);
}

#[test]
fn a_3d_arrow_finds_the_surface_model_and_the_point_behind_its_tip() {
    let mut s = session();
    let model = box_model(&mut s);
    let id = make(
        &mut s,
        top_view(),
        json!([
            mark("arrow", json!([[0.9, 0.9], [0.5, 0.5]])),
            mark("pen", json!([[0.9, 0.2], [0.95, 0.25]])),
            mark("rectangle", json!([[0.4, 0.4], [0.6, 0.6]])),
        ]),
    );
    let m = get(&mut s, &id);
    let marks = m["marks"].as_array().unwrap();
    let arrow = &marks[0];
    // The tip is on the top face of the box. The tail is beside it.
    assert_eq!(
        arrow["under"]["tip"]["ids"],
        json!([model]),
        "{}",
        arrow["under"]
    );
    let tip = &arrow["under"]["tip"]["models"][0];
    assert_eq!(tip["model"], json!(model));
    assert!(tip["faces"].as_array().is_some_and(|f| !f.is_empty()));
    assert_eq!(arrow["under"]["tail"]["ids"], json!([]));
    assert_eq!(arrow["under"]["tail"]["nothing"], 1);
    assert_eq!(arrow["hint3d"]["at"], "tip");
    assert_eq!(arrow["hint3d"]["kind"], "surface");
    let p = arrow["hint3d"]["point"].as_array().unwrap();
    let (x, y, z) = (
        p[0].as_f64().unwrap(),
        p[1].as_f64().unwrap(),
        p[2].as_f64().unwrap(),
    );
    assert!(
        (x - 50.0).abs() < 1.0 && (y - 50.0).abs() < 1.0 && (z - 100.0).abs() < 1e-6,
        "{p:?}"
    );
    // The same answer as mesh.pick for the same ray.
    let picked = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [50, 50, 5000], "direction": [0, 0, -1]}),
    );
    assert_eq!(picked["hit"]["model"], json!(model));
    assert_eq!(picked["hit"]["face"], arrow["hint3d"]["face"]);
    // A stroke off the box: nothing is under it, and the hint is a ground point.
    assert_eq!(marks[1]["under"]["ids"], json!([]));
    assert_eq!(marks[1]["hint3d"]["kind"], "ground");
    assert_eq!(marks[1]["hint3d"]["at"], "first_point");
    assert_eq!(marks[1]["hint3d"]["point"][2], 0.0);
    // The rectangle covers the middle of the box: the model is under it with several hits.
    assert_eq!(marks[2]["under"]["ids"], json!([model]));
    assert!(marks[2]["under"]["models"][0]["hits"].as_u64().unwrap() > 3);
    assert_eq!(marks[2]["hint3d"]["at"], "center");
}

#[test]
fn a_perspective_camera_and_another_object_in_front_are_picked() {
    let mut s = session();
    box_model(&mut s);
    // A tree of 3 m in front of the box, seen from above.
    let tree = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "plant", "at": [50, 50],
            "species": "Tilia cordata", "spread": 100, "height": 3000}}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string();
    let mut vp = top_view();
    vp["view"]["perspective"] = json!(true);
    vp["view"]["distance"] = json!(4000);
    vp["fov"] = json!(45);
    let id = make(
        &mut s,
        vp,
        json!([json!({"kind": "note", "points": [[0.5, 0.5]], "text": "tree"})]),
    );
    let m = get(&mut s, &id);
    let under = &m["marks"][0]["under"];
    assert_eq!(under["objects"][0]["id"], json!(tree), "{under}");
    assert_eq!(under["objects"][0]["kind"], "plant");
    assert_eq!(m["marks"][0]["hint3d"]["kind"], "object");
}

#[test]
fn hidden_grey_and_locked_models_follow_the_rules_of_the_pointer() {
    let mut s = session();
    let low = box_model(&mut s);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Up"}}),
    );
    let up = s
        .editor
        .doc()
        .layers
        .iter()
        .find(|l| l.name == "Up")
        .map(|l| l.id)
        .unwrap();
    let high = ok(
        &mut s,
        "mesh.create",
        json!({"name": "High", "origin": [0, 0, 300], "layer": up}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let edges: Vec<Value> = [
        [[0, 0, 0], [100, 0, 0]],
        [[100, 0, 0], [100, 100, 0]],
        [[100, 100, 0], [0, 100, 0]],
        [[0, 100, 0], [0, 0, 0]],
    ]
    .iter()
    .map(|e| json!(e))
    .collect();
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": high, "edges": edges}),
    );
    let id = make(
        &mut s,
        top_view(),
        json!([json!({"kind": "note", "points": [[0.5, 0.5]], "text": "x"})]),
    );
    let under = |s: &mut Session| get(s, &id)["marks"][0]["under"]["ids"].clone();
    assert_eq!(under(&mut s), json!([high]));
    // Locked: still met.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": up, "locked": true}}),
    );
    assert_eq!(under(&mut s), json!([high]));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": up, "locked": false}}),
    );
    // Grey: the person cannot pick it, so the model behind it is met.
    ok(
        &mut s,
        "visibility.set",
        json!({"mode": "grey", "layers": [up]}),
    );
    assert_eq!(under(&mut s), json!([low]));
    ok(
        &mut s,
        "visibility.set",
        json!({"mode": "visible", "layers": [up]}),
    );
    assert_eq!(under(&mut s), json!([high]));
    // Hidden layer: the one behind is met, and the answer says that the visibility differs from
    // the time of the markup.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": up, "visible": false}}),
    );
    assert_eq!(under(&mut s), json!([low]));
    let m = get(&mut s, &id);
    assert_eq!(m["viewpoint"]["visibility"]["matches"], false);
    assert_eq!(m["viewpoint"]["visibility"]["layers_differ"], json!([up]));
    // Hide with the Hide command.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": up, "visible": true}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_hidden", "ids": [high], "hidden": true}}),
    );
    assert_eq!(under(&mut s), json!([low]));
}

#[test]
fn the_samples_are_bounded() {
    let mut s = session();
    box_model(&mut s);
    let big = mark("rectangle", json!([[0.0, 0.0], [1.0, 1.0]]));
    let id = make(&mut s, top_view(), json!([big]));
    let m = ok(
        &mut s,
        "markup.get",
        json!({"id": id, "render": false, "samples": 100}),
    );
    assert!(m["marks"][0]["under"]["samples"].as_u64().unwrap() <= 100);
    let m = ok(
        &mut s,
        "markup.get",
        json!({"id": id, "render": false, "samples": 9}),
    );
    assert!(m["marks"][0]["under"]["samples"].as_u64().unwrap() <= 9);
    let e = fail(&mut s, "markup.get", json!({"id": id, "samples": 101}));
    assert!(e.message.contains("samples"), "{}", e.message);
    // Many big marks use the budget of the call: the answer says so.
    let marks: Vec<Value> = (0..100)
        .map(|_| mark("rectangle", json!([[0.0, 0.0], [1.0, 1.0]])))
        .collect();
    let id = make(&mut s, top_view(), json!(marks));
    let m = ok(
        &mut s,
        "markup.get",
        json!({"id": id, "render": false, "samples": 100}),
    );
    assert_eq!(m["truncated"], true);
    let total: u64 = m["marks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["under"]["samples"].as_u64().unwrap_or(0))
        .sum();
    assert!(total <= pick::MAX_RAYS as u64, "{total}");
    assert!(
        m["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("budget"))
    );
    // No picking at all.
    let m = ok(
        &mut s,
        "markup.get",
        json!({"id": id, "render": false, "pick": false}),
    );
    assert!(m["marks"][0].get("under").is_none());
}

#[test]
fn get_gives_the_clean_picture_and_the_picture_with_the_marks() {
    let mut s = session();
    box_model(&mut s);
    let id = make(
        &mut s,
        top_view(),
        json!([{"kind": "rectangle", "points": [[0.1, 0.1], [0.9, 0.9]], "color": "#ff0000", "width": 6}]),
    );
    let m = ok(&mut s, "markup.get", json!({"id": id, "width": 200}));
    let (clean, w, h) = png(&m["clean"]);
    let (marked, w2, h2) = png(&m["with_marks"]);
    assert_eq!((w, h), (200, 200));
    assert_eq!((w2, h2), (200, 200));
    // The line of the rectangle is red at x = 0.1 * 200 = 20.
    let at = |img: &[u8], x: u32, y: u32| {
        let i = ((y * 200 + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2]]
    };
    assert_eq!(at(&marked, 20, 100), [255, 0, 0]);
    assert_ne!(at(&clean, 20, 100), [255, 0, 0]);
    // Elsewhere the two pictures are equal.
    assert_eq!(at(&marked, 100, 100), at(&clean, 100, 100));
    assert_eq!(at(&marked, 2, 2), at(&clean, 2, 2));
    // markup.render gives the same two pictures.
    let r = ok(&mut s, "markup.render", json!({"id": id, "width": 200}));
    assert_eq!(r["with_marks"], true);
    assert_eq!(png(&r).0, marked);
    let r = ok(
        &mut s,
        "markup.render",
        json!({"id": id, "width": 200, "with_marks": false}),
    );
    assert_eq!(png(&r).0, clean);
    // The picture of a plan viewpoint is drawn from above.
    let plan = make(
        &mut s,
        plan_view(),
        json!([mark("pen", json!([[0.1, 0.1], [0.9, 0.9]]))]),
    );
    let r = ok(&mut s, "markup.render", json!({"id": plan, "width": 100}));
    assert_eq!(
        (r["width"].clone(), r["height"].clone()),
        (json!(100), json!(100))
    );
}

#[test]
fn deleting_and_clearing_the_work_of_somebody_else_needs_the_consent() {
    let mut s = session();
    // A person's markup: made with the origin user.
    let user = s
        .handle_as(
            LOCAL,
            Origin::User,
            "markup.create",
            json!({"viewpoint": plan_view(), "marks": [mark("pen", json!([[0.1, 0.1]]))], "author": "Ann"}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mine = make(
        &mut s,
        plan_view(),
        json!([mark("pen", json!([[0.2, 0.2]]))]),
    );
    // The agent clears and deletes its own without a question.
    ok(&mut s, "markup.clear", json!({"id": mine}));
    assert_eq!(get(&mut s, &mine)["marks"].as_array().unwrap().len(), 0);
    // The work of the person needs the consent.
    for (method, params) in [
        ("markup.clear", json!({"id": user})),
        ("markup.delete", json!({"id": user})),
        ("markup.delete", json!({"ids": [mine, user]})),
    ] {
        let e = fail(&mut s, method, params);
        assert_eq!(e.reason.as_deref(), Some("confirm_needed"), "{method}");
        assert_eq!(e.details.as_ref().unwrap()["ask_the_person"], true);
        assert_eq!(e.details.as_ref().unwrap()["markups"][0]["id"], user);
    }
    assert_eq!(
        ok(&mut s, "markup.list", json!({}))["total"],
        2,
        "nothing was deleted"
    );
    // After the person said yes.
    ok(
        &mut s,
        "markup.delete",
        json!({"ids": [mine, user], "confirm": true}),
    );
    assert_eq!(ok(&mut s, "markup.list", json!({}))["total"], 0);
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        ok(&mut s, "markup.list", json!({}))["total"],
        2,
        "one undo step"
    );
    let e = fail(
        &mut s,
        "markup.get",
        json!({"id": "00000000-0000-4000-8000-0000000000aa"}),
    );
    assert!(e.message.contains("does not exist"));
}

#[test]
fn a_markup_changes_events_and_leaves_the_model_alone() {
    let mut s = session();
    let rect = filled_rect(&mut s);
    let revision = s.editor.revision();
    make(
        &mut s,
        plan_view(),
        json!([mark("pen", json!([[0.1, 0.1]]))]),
    );
    let cs = s.editor.changes_since(revision);
    assert_eq!(cs.changes.len(), 1);
    assert!(cs.changes[0].document.markups);
    assert!(cs.changes[0].touches("markups"));
    assert!(!cs.changes[0].touches("objects"));
    // The objects are as they were, and the object list does not hold the markup.
    let objects = ok(&mut s, "objects.list", json!({}));
    assert_eq!(objects["total"].as_u64().unwrap_or(1), 1, "{objects}");
    assert_eq!(s.editor.doc().objects.len(), 1);
    assert!(
        s.editor
            .doc()
            .object(serde_json::from_value(json!(rect)).unwrap())
            .is_some()
    );
}

#[test]
fn a_markup_of_an_issue_is_a_viewpoint_with_the_marked_picture_in_the_bcf_file() {
    let mut s = session();
    box_model(&mut s);
    let issue = ok(
        &mut s,
        "issue.create",
        json!({"title": "Hedge", "now_ms": 1}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let plain = ok(&mut s, "issue.export_bcf", json!({}));
    assert_eq!(plain["viewpoints"], 0);
    let id = make(
        &mut s,
        top_view(),
        json!([{"kind": "rectangle", "points": [[0.1, 0.1], [0.9, 0.9]], "color": "#ff0000", "width": 6}]),
    );
    // Not linked yet: the file is as before.
    assert_eq!(ok(&mut s, "issue.export_bcf", json!({}))["viewpoints"], 0);
    ok(
        &mut s,
        "markup.update",
        json!({"id": id, "issue": issue, "name": "Where"}),
    );
    let out = ok(&mut s, "issue.export_bcf", json!({}));
    assert_eq!(out["viewpoints"], 1);
    assert_eq!(out["snapshots"], 1);
    assert_eq!(out["markups"]["in_file"], 1);
    // The document did not change: the picture is made for the file only.
    assert_eq!(
        s.editor
            .doc()
            .issues
            .get(serde_json::from_value(json!(issue)).unwrap())
            .unwrap()
            .viewpoints
            .len(),
        0
    );
    // A second session reads the file: the issue has a viewpoint with a picture, and no markup
    // (marks are not read from images).
    let mut t = session();
    let r = ok(&mut t, "issue.import_bcf", json!({"base64": out["base64"]}));
    assert_eq!(r["imported"], 1, "{r}");
    let got = ok(&mut t, "issue.get", json!({"id": issue, "snapshots": true}));
    let vp = &got["viewpoints"][0];
    assert_eq!(got["viewpoints"].as_array().unwrap().len(), 1);
    let (pic, w, _) = png(&json!({"png_base64": vp["snapshot_base64"]}));
    assert!(
        pic.chunks(4).any(|p| p[..3] == [255, 0, 0]),
        "the marks are in the picture ({w})"
    );
    assert_eq!(ok(&mut t, "markup.list", json!({}))["total"], 0);
}

#[test]
fn markups_are_saved_in_the_file_and_open_again() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("m.kad").display().to_string();
    let mut s = session();
    let id = make(
        &mut s,
        plan_view(),
        json!([mark("pen", json!([[0.1, 0.1], [0.4, 0.4]])), {"kind": "note", "points": [[0.5, 0.5]], "text": "hi"}]),
    );
    ok(&mut s, "file.save", json!({"path": f}));
    let mut t = session();
    ok(&mut t, "file.open", json!({"path": f}));
    let a = get(&mut s, &id);
    let b = get(&mut t, &id);
    assert_eq!(a["marks"], b["marks"]);
    assert_eq!(a["viewpoint"], b["viewpoint"]);
    assert_eq!(b["origin"], "agent");
}

#[test]
fn describe_has_the_markup_section_and_the_methods() {
    let d = describe();
    assert!(d["bounds"]["marks_per_markup"].as_u64().unwrap() > 0);
    for m in [
        "markup.list",
        "markup.get",
        "markup.create",
        "markup.update",
        "markup.delete",
        "markup.clear",
        "markup.render",
    ] {
        assert!(crate::METHODS.iter().any(|(n, _)| *n == m), "{m}");
    }
}
