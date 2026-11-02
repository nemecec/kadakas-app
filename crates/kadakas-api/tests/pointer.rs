//! AGR-10: the pointer of the person. The host shows the pointer to the session with
//! `set_pointer`, and the session answers the `pointer` field of `view.get` from it. The
//! window that calls this is tested in the app crate. Here: the shape of the answer, the empty
//! answer, the ray that `mesh.pick` accepts and the hit that equals the answer of `mesh.pick`.

use glam::{DVec2, DVec3};
use kadakas_api::{Aim, PixelSize, PointerInput, Session};
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {:?}", e.message, e.details))
}

/// A box of 100 mm at the origin, as a surface model. Its top face is at z = 100.
fn box_model(s: &mut Session) -> String {
    box_at(s, json!({"name": "Box"}))
}

/// The same box where `params` of `mesh.create` put it (an origin, a layer).
fn box_at(s: &mut Session, params: Value) -> String {
    let id = ok(s, "mesh.create", params)["id"]
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

fn ray(origin: [f64; 3], direction: [f64; 3]) -> Aim {
    Aim::Ray {
        origin: DVec3::from(origin),
        direction: DVec3::from(direction),
        pixel: PixelSize {
            constant: 1.0,
            per_distance: 0.0,
        },
    }
}

fn show(s: &mut Session, canvas: [f64; 2], now_ms: f64, aim: Aim) {
    s.pointer_new_frame();
    s.set_pointer(PointerInput {
        canvas,
        now_ms,
        navigating: false,
        focused: true,
        aim,
    });
}

#[test]
fn there_is_no_pointer_until_the_host_shows_one_and_after_it_leaves() {
    let mut s = Session::with_sequential_ids();
    assert_eq!(s.pointer_json(0.0), Value::Null);
    show(
        &mut s,
        [10.0, 20.0],
        1000.0,
        Aim::Plan {
            point: DVec2::new(5.0, 6.0),
            ids: vec![],
        },
    );
    assert!(s.pointer_json(1000.0).is_object());
    // The pointer leaves the drawing area.
    s.clear_pointer();
    assert_eq!(s.pointer_json(1100.0), Value::Null);
    assert_eq!(s.pointer_hit_point(), None);
}

#[test]
fn a_pointer_that_no_frame_renewed_is_old() {
    let mut s = Session::with_sequential_ids();
    let aim = || Aim::Plan {
        point: DVec2::ZERO,
        ids: vec![],
    };
    show(&mut s, [1.0, 1.0], 0.0, aim());
    assert!(s.pointer_json(10.0).is_object());
    // A frame starts that does not draw the area (another tab is on screen).
    s.pointer_new_frame();
    assert_eq!(s.pointer_json(20.0), Value::Null);
    // A frame that draws it again.
    show(&mut s, [1.0, 1.0], 30.0, aim());
    assert!(s.pointer_json(30.0).is_object());
}

#[test]
fn the_plan_answer_has_the_point_the_objects_and_the_age() {
    let mut s = Session::with_sequential_ids();
    let rect = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string();
    let id = serde_json::from_value(json!(rect)).unwrap();
    show(
        &mut s,
        [120.5, 80.0],
        5000.0,
        Aim::Plan {
            point: DVec2::new(4.0, 5.0),
            ids: vec![id],
        },
    );
    let p = s.pointer_json(5000.0);
    assert_eq!(p["over_canvas"], true);
    assert_eq!(p["canvas"], json!([120.5, 80.0]));
    assert_eq!(p["mode"], "plan");
    assert_eq!(p["point"], json!([4.0, 5.0]));
    assert_eq!(p["ids"], json!([rect]));
    assert_eq!(p["age_ms"], 0.0);
    assert!(p.get("ray").is_none() && p.get("hit").is_none());
    // The pointer rests: the age grows with the clock of the host, at answer time.
    show(
        &mut s,
        [120.5, 80.0],
        5400.0,
        Aim::Plan {
            point: DVec2::new(4.0, 5.0),
            ids: vec![id],
        },
    );
    assert_eq!(s.pointer_json(5750.0)["age_ms"], 750.0);
    // It moves: the age starts again.
    show(
        &mut s,
        [121.0, 80.0],
        6000.0,
        Aim::Plan {
            point: DVec2::new(4.1, 5.0),
            ids: vec![id],
        },
    );
    assert_eq!(s.pointer_json(6100.0)["age_ms"], 100.0);
    // The object that an edit deleted is not under the pointer any more.
    ok(&mut s, "objects.delete", json!({"ids": [rect]}));
    assert_eq!(s.pointer_json(6100.0)["ids"], json!([]));
}

#[test]
fn the_3d_answer_has_a_ray_that_mesh_pick_accepts_and_the_same_hit() {
    let mut s = Session::with_sequential_ids();
    let model = box_model(&mut s);
    // From above, over the middle of the top face.
    show(
        &mut s,
        [300.0, 200.0],
        100.0,
        ray([30.0, 40.0, 500.0], [0.0, 0.0, -1.0]),
    );
    let p = s.pointer_json(100.0);
    assert_eq!(p["mode"], "3d");
    assert_eq!(p["canvas"], json!([300.0, 200.0]));
    assert!(p.get("point").is_none());
    let origin = p["ray"]["origin"].clone();
    let direction = p["ray"]["direction"].clone();
    assert_eq!(origin, json!([30.0, 40.0, 500.0]));
    // The ray goes to mesh.pick as it is, and the answer is the hit of the pointer.
    let picked = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": origin, "direction": direction}),
    );
    let mut hit = p["hit"].clone();
    assert!(hit.is_object(), "{p}");
    assert_eq!(hit["model"], json!(model));
    assert!((hit["distance"].as_f64().unwrap() - 400.0).abs() < 1e-6);
    assert!((hit["point"][2].as_f64().unwrap() - 100.0).abs() < 1e-6);
    // The hit has the model, the face, the point and the distance. No edge: the point is in the
    // middle of the face.
    assert!(hit.get("edge").is_none());
    assert!(hit["face"].is_u64());
    hit.as_object_mut().unwrap().remove("edge");
    assert_eq!(hit, picked["hit"]);
    // The status bar uses the same point.
    let at = s.pointer_hit_point().unwrap();
    assert!((at[0] - 30.0).abs() < 1e-9 && (at[1] - 40.0).abs() < 1e-9);
}

#[test]
fn the_hit_names_the_edge_when_the_pointer_is_on_one() {
    let mut s = Session::with_sequential_ids();
    box_model(&mut s);
    // 2 mm from the edge x = 0 of the top face: 6 px of 1 mm reach it.
    show(
        &mut s,
        [1.0, 1.0],
        0.0,
        ray([2.0, 50.0, 500.0], [0.0, 0.0, -1.0]),
    );
    let hit = s.pointer_json(0.0)["hit"].clone();
    let edge = hit["edge"].as_u64().expect("an edge");
    let e = ok(
        &mut s,
        "mesh.edges",
        json!({"id": hit["model"], "ids": [edge]}),
    );
    let faces = e["items"][0]["faces"].as_array().unwrap();
    assert!(faces.contains(&hit["face"]));
    // 20 mm away is no edge.
    show(
        &mut s,
        [1.0, 1.0],
        0.0,
        ray([20.0, 50.0, 500.0], [0.0, 0.0, -1.0]),
    );
    assert!(s.pointer_json(0.0)["hit"].get("edge").is_none());
}

#[test]
fn a_ray_that_hits_nothing_has_a_null_hit_and_no_status_point() {
    let mut s = Session::with_sequential_ids();
    box_model(&mut s);
    show(
        &mut s,
        [5.0, 5.0],
        0.0,
        ray([900.0, 900.0, 500.0], [0.0, 0.0, -1.0]),
    );
    let p = s.pointer_json(0.0);
    assert_eq!(p["hit"], Value::Null);
    assert!(p["ray"]["origin"].is_array());
    assert_eq!(s.pointer_hit_point(), None);
    // An empty drawing: the same.
    let mut e = Session::with_sequential_ids();
    show(
        &mut e,
        [5.0, 5.0],
        0.0,
        ray([0.0, 0.0, 500.0], [0.0, 0.0, -1.0]),
    );
    assert_eq!(e.pointer_json(0.0)["hit"], Value::Null);
}

#[test]
fn the_hit_follows_the_model_and_the_status_point_waits_while_the_view_moves() {
    let mut s = Session::with_sequential_ids();
    let model = box_model(&mut s);
    show(
        &mut s,
        [5.0, 5.0],
        0.0,
        ray([50.0, 50.0, 500.0], [0.0, 0.0, -1.0]),
    );
    assert!(s.pointer_json(0.0)["hit"].is_object());
    // A change of the drawing makes the hit again: push the top face by 50 mm.
    let faces = ok(
        &mut s,
        "mesh.faces",
        json!({"id": model, "limit": 100, "with_points": true}),
    );
    let top = faces["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| {
            f["points"]["outer"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p[2].as_f64().unwrap() == 100.0)
        })
        .unwrap()["id"]
        .clone();
    ok(
        &mut s,
        "mesh.push_pull",
        json!({"id": model, "face": top, "distance": 50}),
    );
    let d = s.pointer_json(0.0)["hit"]["distance"].as_f64().unwrap();
    // The face moved by 50 mm, along its normal, which the box makes point either way.
    assert!((d - 400.0).abs() > 49.0 && (d - 400.0).abs() < 51.0, "{d}");
    // While the person moves the view, the status bar shows no point, but the agent still reads
    // the ray and the hit.
    s.pointer_new_frame();
    s.set_pointer(PointerInput {
        canvas: [5.0, 5.0],
        now_ms: 0.0,
        navigating: true,
        focused: true,
        aim: ray([50.0, 50.0, 500.0], [0.0, 0.0, -1.0]),
    });
    assert_eq!(s.pointer_hit_point(), None);
    let p = s.pointer_json(0.0);
    assert_eq!(p["navigating"], true);
    assert!(p["hit"].is_object());
}

/// Looks straight down at (50, 50) from 1000 mm up and gives the pointer answer.
fn look_down(s: &mut Session) -> Value {
    show(
        s,
        [1.0, 1.0],
        0.0,
        ray([50.0, 50.0, 1000.0], [0.0, 0.0, -1.0]),
    );
    s.pointer_json(0.0)
}

fn pick_down(s: &mut Session) -> Value {
    ok(
        s,
        "mesh.pick",
        json!({"origin": [50.0, 50.0, 1000.0], "direction": [0.0, 0.0, -1.0]}),
    )["hit"]
        .clone()
}

#[test]
fn a_model_hidden_with_hide_is_not_met_by_the_pointer_or_by_mesh_pick() {
    let mut s = Session::with_sequential_ids();
    let low = box_model(&mut s);
    let high = box_at(&mut s, json!({"name": "High", "origin": [0, 0, 300]}));
    assert_eq!(look_down(&mut s)["hit"]["model"], json!(high));
    // Hide the model in front: the one behind is met.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_hidden", "ids": [high], "hidden": true}}),
    );
    assert_eq!(look_down(&mut s)["hit"]["model"], json!(low));
    assert_eq!(pick_down(&mut s)["model"], json!(low));
    // Shown again: the front model is back.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_hidden", "ids": [high], "hidden": false}}),
    );
    assert_eq!(look_down(&mut s)["hit"]["model"], json!(high));
    assert_eq!(pick_down(&mut s)["model"], json!(high));
}

#[test]
fn a_model_on_a_hidden_layer_is_not_met() {
    let mut s = Session::with_sequential_ids();
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
    let high = box_at(
        &mut s,
        json!({"name": "High", "origin": [0, 0, 300], "layer": up}),
    );
    assert_eq!(look_down(&mut s)["hit"]["model"], json!(high));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": up, "visible": false}}),
    );
    assert_eq!(look_down(&mut s)["hit"]["model"], json!(low));
}

#[test]
fn grey_models_are_skipped_and_locked_models_are_met() {
    let mut s = Session::with_sequential_ids();
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
    let high = box_at(
        &mut s,
        json!({"name": "High", "origin": [0, 0, 300], "layer": up}),
    );
    // Locked: the person cannot edit it, but points at it, so the agent hears about it.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": up, "locked": true}}),
    );
    assert_eq!(look_down(&mut s)["hit"]["model"], json!(high));
    // Grey: the person cannot pick it, so the pointer is not on it. mesh.pick still reads it.
    ok(
        &mut s,
        "visibility.set",
        json!({"mode": "grey", "layers": [up]}),
    );
    assert_eq!(look_down(&mut s)["hit"]["model"], json!(low));
    assert_eq!(pick_down(&mut s)["model"], json!(high));
    assert_eq!(s.pointer_hit_point().map(|p| p[2].round()), Some(100.0));
}

fn plant(s: &mut Session, at: [f64; 2], height: f64) -> String {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "plant", "at": at, "species": "Tilia cordata", "spread": 100, "height": height}}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn an_object_that_is_not_a_surface_model_in_front_takes_the_hit() {
    let mut s = Session::with_sequential_ids();
    box_model(&mut s);
    let tree = plant(&mut s, [50.0, 50.0], 3000.0);
    let p = look_down(&mut s);
    assert_eq!(p["hit"], Value::Null, "{p}");
    let o = &p["nearest_object"];
    assert_eq!(o["id"], json!(tree), "{p}");
    assert_eq!(o["kind"], "plant");
    let d = o["distance"].as_f64().unwrap();
    assert!(d > 0.0 && d < 900.0, "{o}");
    assert!((o["point"][2].as_f64().unwrap() - (1000.0 - d)).abs() < 1e-6);
    // The status bar shows no point there either.
    assert_eq!(s.pointer_hit_point(), None);
    // The ray from the side meets the box before the tree behind it.
    show(
        &mut s,
        [1.0, 1.0],
        0.0,
        ray([-500.0, 50.0, 50.0], [1.0, 0.0, 0.0]),
    );
    let p = s.pointer_json(0.0);
    assert!(p["hit"].is_object(), "{p}");
    assert!(p.get("nearest_object").is_none());
    // Far from the tree there is nothing but the box, as before.
    show(
        &mut s,
        [1.0, 1.0],
        0.0,
        ray([97.0, 3.0, 1000.0], [0.0, 0.0, -1.0]),
    );
    let p = s.pointer_json(0.0);
    assert!(
        p["hit"].is_object() && p.get("nearest_object").is_none(),
        "{p}"
    );
    // An object that is hidden is not met.
    show(
        &mut s,
        [1.0, 1.0],
        0.0,
        ray([50.0, 50.0, 1000.0], [0.0, 0.0, -1.0]),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_hidden", "ids": [tree], "hidden": true}}),
    );
    let p = s.pointer_json(0.0);
    assert!(
        p["hit"].is_object() && p.get("nearest_object").is_none(),
        "{p}"
    );
}

#[test]
fn the_reading_can_be_forbidden_and_says_why() {
    let mut s = Session::with_sequential_ids();
    assert!(s.pointer_enabled());
    assert_eq!(s.pointer_reason(), Some("not_over_canvas"));
    let aim = || Aim::Plan {
        point: DVec2::ZERO,
        ids: vec![],
    };
    show(&mut s, [1.0, 1.0], 0.0, aim());
    assert_eq!(s.pointer_reason(), None);
    s.set_pointer_enabled(false);
    assert_eq!(s.pointer_json(10.0), Value::Null);
    assert_eq!(s.pointer_reason(), Some("disabled_by_user"));
    // Nothing was read.
    assert_eq!(s.pointer_last_read_ms(), None);
    assert!(s.pointer_read_notes().is_empty());
    // The status bar of the person is not affected.
    show(&mut s, [1.0, 1.0], 20.0, aim());
    assert!(s.pointer_over_plan());
    s.set_pointer_enabled(true);
    assert!(s.pointer_json(30.0).is_object());
}

#[test]
fn a_read_is_noted_for_the_status_bar_and_once_a_minute_for_the_panel() {
    let mut s = Session::with_sequential_ids();
    let aim = || Aim::Plan {
        point: DVec2::ZERO,
        ids: vec![],
    };
    show(&mut s, [1.0, 1.0], 0.0, aim());
    assert!(!s.pointer_read_recently(5.0));
    let _ = s.pointer_json(1_000.0);
    assert_eq!(s.pointer_last_read_ms(), Some(1_000.0));
    assert!(s.pointer_read_recently(10_999.0));
    assert!(!s.pointer_read_recently(11_001.0));
    // More reads inside a minute: still one line.
    for t in [2_000.0, 30_000.0, 60_000.0] {
        let _ = s.pointer_json(t);
    }
    assert_eq!(s.pointer_read_notes(), &[1_000.0]);
    let _ = s.pointer_json(61_500.0);
    assert_eq!(s.pointer_read_notes(), &[1_000.0, 61_500.0]);
    // A read that finds no pointer is not a read of the pointer.
    s.clear_pointer();
    let _ = s.pointer_json(200_000.0);
    assert_eq!(s.pointer_read_notes().len(), 2);
}

#[test]
fn a_client_that_reads_too_fast_gets_the_last_answer_and_no_error() {
    let mut s = Session::with_sequential_ids();
    let aim = |x: f64| Aim::Plan {
        point: DVec2::new(x, 0.0),
        ids: vec![],
    };
    show(&mut s, [1.0, 1.0], 0.0, aim(1.0));
    let first = s.pointer_json(0.0);
    for _ in 1..60 {
        assert_eq!(s.pointer_json(0.0), first);
    }
    // The pointer moves. The 61st read in the same second gets the old answer.
    show(&mut s, [2.0, 1.0], 2.0, aim(2.0));
    let limited = s.pointer_json(0.0);
    assert_eq!(limited, first);
    // A new second: a new answer.
    let fresh = s.pointer_json(1_500.0);
    assert_eq!(fresh["point"], json!([2.0, 0.0]));
}

#[test]
fn the_answer_tells_if_the_window_has_the_focus() {
    let mut s = Session::with_sequential_ids();
    s.pointer_new_frame();
    s.set_pointer(PointerInput {
        canvas: [1.0, 1.0],
        now_ms: 0.0,
        navigating: false,
        focused: false,
        aim: Aim::Plan {
            point: DVec2::ZERO,
            ids: vec![],
        },
    });
    assert_eq!(s.pointer_json(0.0)["focused"], false);
    show(
        &mut s,
        [1.0, 1.0],
        0.0,
        Aim::Plan {
            point: DVec2::ZERO,
            ids: vec![],
        },
    );
    assert_eq!(s.pointer_json(5_000.0)["focused"], true);
}
