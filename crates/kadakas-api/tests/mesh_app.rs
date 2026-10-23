//! Surface models as objects of the app (MOD-13), seen by an agent: `objects.list` and
//! `objects.get` tell the counts, the closed-solid state, the box and the materials, the geometry
//! stays out of the list, and the usual methods select, move, turn, copy and delete them.

use kadakas_api::{METHODS, RpcError, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, RpcError> {
    s.handle(method, params)
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {} {:?}", e.message, e.details))
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        [[x0, y0, 0], [x1, y0, 0]],
        [[x1, y0, 0], [x1, y1, 0]],
        [[x1, y1, 0], [x0, y1, 0]],
        [[x0, y1, 0], [x0, y0, 0]]
    ])
}

/// A closed box of 4000 x 3000 x 2500 mm. Returns the id of the model.
fn shed(s: &mut Session, name: &str, x: f64) -> String {
    let id = ok(s, "mesh.create", json!({"name": name}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = ok(
        s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(x, 0.0, x + 4000.0, 3000.0)}),
    );
    let face = r["created"]["faces"][0].as_u64().unwrap();
    ok(
        s,
        "mesh.push_pull",
        json!({"id": id, "face": face, "distance": 2500}),
    );
    id
}

fn row(list: &Value, id: &str) -> Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == id)
        .unwrap_or_else(|| panic!("{id} is not in {list}"))
        .clone()
}

#[test]
fn the_list_tells_what_a_surface_model_is_without_its_geometry() {
    let mut s = Session::with_sequential_ids();
    let id = shed(&mut s, "Shed", 0.0);
    let list = ok(&mut s, "objects.list", json!({"kind": "surface_model"}));
    let o = row(&list, &id);
    assert_eq!(o["shape"]["kind"], "surface_model");
    assert_eq!(o["shape"]["name"], "Shed");
    assert!(
        o["shape"]["elided"]
            .as_str()
            .unwrap()
            .contains("mesh.faces")
    );
    assert!(o["shape"].get("vertices").is_none() && o["shape"].get("faces").is_none());
    let surface = &o["surface"];
    assert_eq!(surface["kind"], "surface_model");
    assert_eq!(
        (
            surface["vertices"].as_u64(),
            surface["edges"].as_u64(),
            surface["faces"].as_u64()
        ),
        (Some(8), Some(12), Some(6))
    );
    assert_eq!(surface["models"], 1);
    assert_eq!(surface["solid"]["state"], "closed");
    assert_eq!(surface["solid"]["solids"], 1);
    assert!(
        (surface["solid"]["volume_mm3"].as_f64().unwrap() - 4000.0 * 3000.0 * 2500.0).abs() < 1.0
    );
    let area = 2.0 * (4000.0 * 3000.0 + 4000.0 * 2500.0 + 3000.0 * 2500.0);
    assert!((surface["area_mm2"].as_f64().unwrap() - area).abs() < 1.0);
    assert_eq!(surface["bounds"]["min"], json!([0.0, 0.0, 0.0]));
    assert_eq!(surface["bounds"]["max"], json!([4000.0, 3000.0, 2500.0]));
    assert_eq!(surface["bounds"]["size"], json!([4000.0, 3000.0, 2500.0]));
    assert_eq!(surface["unpainted_faces"], 6);
    assert_eq!(surface["materials"], json!([]));
    // The same object, read alone, has the geometry too.
    let g = ok(&mut s, "objects.get", json!({"id": id}));
    assert_eq!(g["surface"], *surface);
    assert!(g["shape"]["vertices"].as_array().unwrap().len() == 8);
    // A circle has no such field.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}),
    );
    let all = ok(&mut s, "objects.list", json!({}));
    assert!(
        all.as_array()
            .unwrap()
            .iter()
            .filter(|o| o["shape"]["kind"] == "circle")
            .all(|o| o.get("surface").is_none())
    );
}

#[test]
fn an_open_surface_and_a_painted_one_show_in_the_summary() {
    let mut s = Session::with_sequential_ids();
    let id = ok(&mut s, "mesh.create", json!({}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(0.0, 0.0, 1000.0, 1000.0)}),
    );
    let m = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_resource", "name": "Brick", "body": {"kind": "render", "type": "material", "base_color": [180, 60, 40, 255]}}}),
    );
    let brick = m["created"][0].as_str().unwrap().to_string();
    let face = ok(&mut s, "mesh.faces", json!({"id": id}))["items"][0]["id"]
        .as_u64()
        .unwrap();
    ok(
        &mut s,
        "mesh.paint",
        json!({"id": id, "faces": [face], "material": brick}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": id}));
    assert_eq!(
        o["surface"]["solid"],
        json!({"state": "open", "open_edges": 4})
    );
    assert_eq!(
        o["surface"]["materials"],
        json!([{"name": "Brick", "faces": 1}])
    );
    assert_eq!(o["surface"]["unpainted_faces"], 0);
}

#[test]
fn a_component_instance_and_a_group_are_listed_with_their_numbers() {
    let mut s = Session::with_sequential_ids();
    let a = shed(&mut s, "A", 0.0);
    let b = shed(&mut s, "B", 10_000.0);
    let faces: Vec<Value> = ok(&mut s, "mesh.faces", json!({"id": a, "limit": 100}))["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].clone())
        .collect();
    let r = ok(
        &mut s,
        "mesh.make_component",
        json!({"id": a, "faces": faces, "name": "Shed", "replace": true}),
    );
    let instances: Vec<Value> = ok(&mut s, "objects.list", json!({"kind": "component"}))
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(instances.len(), 1, "{r}");
    let surface = &instances[0]["surface"];
    assert_eq!(surface["kind"], "component");
    assert_eq!(surface["faces"], 6);
    assert_eq!(surface["solid"]["state"], "closed");
    // A group of the instance and the other model counts both.
    let inst = instances[0]["id"].as_str().unwrap().to_string();
    let g = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "group", "ids": [inst, b]}}),
    );
    let gid = g["created"][0].as_str().unwrap().to_string();
    let group = ok(&mut s, "objects.get", json!({"id": gid}));
    assert_eq!(group["surface"]["kind"], "group");
    assert_eq!(group["surface"]["models"], 2);
    assert_eq!(group["surface"]["faces"], 12);
    assert_eq!(group["surface"]["solid"]["solids"], 2);
}

#[test]
fn an_agent_selects_moves_turns_copies_and_deletes_a_model_with_the_usual_methods() {
    let mut s = Session::with_sequential_ids();
    let id = shed(&mut s, "Shed", 0.0);
    let bounds = |s: &mut Session| ok(s, "mesh.bounds", json!({"id": id}))["world"]["min"].clone();
    ok(&mut s, "selection.set", json!({"ids": [id]}));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [500, 250]}}),
    );
    assert_eq!(bounds(&mut s), json!([500.0, 250.0, 0.0]));
    ok(&mut s, "undo", json!({}));
    assert_eq!(bounds(&mut s), json!([0.0, 0.0, 0.0]));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "copy_objects", "ids": [id], "delta": [9000, 0]}}),
    );
    let list = ok(&mut s, "objects.list", json!({"kind": "surface_model"}));
    assert_eq!(list.as_array().unwrap().len(), 2);
    ok(&mut s, "undo", json!({}));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [id]}}),
    );
    assert_eq!(
        ok(&mut s, "objects.list", json!({"kind": "surface_model"})),
        json!([])
    );
    ok(&mut s, "undo", json!({}));
    // Find it again with the query language.
    let q = ok(
        &mut s,
        "objects.query",
        json!({"criteria": "Kind = \"surface_model\""}),
    );
    assert_eq!(q["ids"], json!([id]));
}

#[test]
fn the_svg_render_and_the_snap_query_know_the_model() {
    let mut s = Session::with_sequential_ids();
    let id = shed(&mut s, "Shed", 0.0);
    let svg = ok(&mut s, "render.svg", json!({}));
    let text = svg["svg"]
        .as_str()
        .unwrap_or_else(|| svg.as_str().unwrap_or(""));
    assert!(text.contains(r#"data-kind="surface_model""#), "{svg}");
    assert!(text.contains(r#"data-faces="2" data-edges="8""#));
    // The snap finds a vertex, the middle of an edge and the centre of the top.
    let at = |s: &mut Session, p: [f64; 2]| {
        ok(
            s,
            "snap.query",
            json!({"point": p, "tolerance": 50, "grid_step": 100,
            "set": {"grid": false}}),
        )
    };
    let r = at(&mut s, [4010.0, 5.0]);
    assert_eq!(
        (r["kind"].as_str(), r["point"].clone()),
        (Some("endpoint"), json!([4000.0, 0.0]))
    );
    let r = at(&mut s, [2000.0, 10.0]);
    assert_eq!(r["kind"], "midpoint");
    let r = at(&mut s, [2000.0, 1490.0]);
    assert_eq!(
        (r["kind"].as_str(), r["point"].clone()),
        (Some("center"), json!([2000.0, 1500.0]))
    );
    let _ = id;
}

#[test]
fn the_documentation_of_the_methods_and_describe_mention_the_app() {
    let doc = |n: &str| METHODS.iter().find(|(m, _)| *m == n).unwrap().1;
    assert!(doc("objects.list").contains("surface"));
    assert!(doc("objects.list").contains("shape.elided"));
    assert!(doc("objects.get").contains("surface"));
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let text = d["mesh"]["in_the_app"].as_str().unwrap();
    assert!(text.contains("objects.list") && text.contains("Object Info"));
}

#[test]
fn an_empty_model_is_empty_and_a_long_list_of_small_models_is_checked_by_faces() {
    let mut s = Session::with_sequential_ids();
    let empty = ok(&mut s, "mesh.create", json!({"name": "Nothing"}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let o = ok(&mut s, "objects.get", json!({"id": empty}));
    assert_eq!(o["surface"]["solid"], json!({"state": "empty"}));
    assert_eq!(o["surface"]["bounds"], Value::Null);
    // 101 flat rectangles of one face each: far under the budget of faces of a list.
    let ids: Vec<String> = (0..101)
        .map(|i| {
            let id = ok(&mut s, "mesh.create", json!({}))["id"].as_str().unwrap().to_string();
            ok(
                &mut s,
                "mesh.add_edges",
                json!({"id": id, "edges": rect(f64::from(i) * 2000.0, 0.0, f64::from(i) * 2000.0 + 1000.0, 1000.0)}),
            );
            id
        })
        .collect();
    let list = ok(&mut s, "objects.list", json!({"kind": "surface_model"}));
    let state = |id: &str| row(&list, id)["surface"]["solid"]["state"].clone();
    assert_eq!(state(&ids[0]), "open");
    assert_eq!(state(&ids[99]), "open");
    assert_eq!(state(&ids[100]), "open");
    // One object alone is always checked.
    let one = ok(&mut s, "objects.get", json!({"id": ids[100]}));
    assert_eq!(one["surface"]["solid"]["state"], "open");
}

/// A flat grid of `n` x `n` squares of 10 mm cut in two, as a file has it (ids from one counter).
fn flat_grid(n: usize) -> kadakas_core::surface::SurfaceModel {
    use kadakas_core::surface::{Edge, EdgeId, Face, FaceId, Plane, SurfaceModel, VertexId};
    use std::collections::{BTreeMap, HashSet};
    let id_of = |x: usize, y: usize| VertexId((y * (n + 1) + x) as u32 + 1);
    let mut vertices = BTreeMap::new();
    for y in 0..=n {
        for x in 0..=n {
            vertices.insert(
                id_of(x, y),
                glam::DVec3::new(x as f64 * 10.0, y as f64 * 10.0, 0.0),
            );
        }
    }
    let (mut edges, mut faces) = (BTreeMap::new(), BTreeMap::new());
    let (mut seen, mut e, mut f) = (HashSet::new(), 0u32, 0u32);
    let up = Plane::new(glam::DVec3::ZERO, glam::DVec3::Z).unwrap();
    for y in 0..n {
        for x in 0..n {
            let (a, b, c, d) = (
                id_of(x, y),
                id_of(x + 1, y),
                id_of(x + 1, y + 1),
                id_of(x, y + 1),
            );
            for tri in [[a, b, c], [a, c, d]] {
                for k in 0..3 {
                    let (p, q) = (tri[k], tri[(k + 1) % 3]);
                    if seen.insert((p.min(q), p.max(q))) {
                        e += 1;
                        edges.insert(EdgeId(1_000_000 + e), Edge { v: [p, q] });
                    }
                }
                f += 1;
                faces.insert(FaceId(2_000_000 + f), Face::new(up, tri.to_vec(), vec![]));
            }
        }
    }
    SurfaceModel::from_parts(vertices, edges, faces, 3_000_000)
}

fn add_grid(s: &mut Session, n: usize) -> String {
    let out = s
        .editor
        .execute(&kadakas_core::command::Command::AddObject {
            shape: kadakas_core::model::Shape::SurfaceModel(Box::new(flat_grid(n))),
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
    out.created[0].to_string()
}

#[test]
fn the_list_keeps_the_symbol_of_an_instance_and_the_members_of_a_group() {
    let mut s = Session::with_sequential_ids();
    let a = shed(&mut s, "A", 0.0);
    let b = shed(&mut s, "B", 10_000.0);
    let faces: Vec<Value> = ok(&mut s, "mesh.faces", json!({"id": a, "limit": 100}))["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].clone())
        .collect();
    ok(
        &mut s,
        "mesh.make_component",
        json!({"id": a, "faces": faces, "name": "Shed", "replace": true}),
    );
    let inst = ok(&mut s, "objects.list", json!({"kind": "component"}))[0].clone();
    let id = inst["id"].as_str().unwrap().to_string();
    // The instance keeps what it is made of: the definition and the transform.
    assert!(inst["shape"]["symbol"].is_string(), "{inst}");
    assert_eq!(inst["shape"]["kind"], "component");
    assert_eq!(inst["shape"]["transform"].as_array().unwrap().len(), 12);
    assert!(inst["shape"].get("elided").is_none());
    // objects.get is the same for the instance.
    let got = ok(&mut s, "objects.get", json!({"id": id}));
    assert_eq!(got["shape"], inst["shape"]);
    // A group keeps its shape and gets no note about elided geometry.
    let g = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "group", "ids": [id, b]}}),
    );
    let gid = g["created"][0].as_str().unwrap().to_string();
    let list = ok(&mut s, "objects.list", json!({}));
    let group = row(&list, &gid);
    assert_eq!(group["shape"]["kind"], "group");
    assert!(group["shape"].get("elided").is_none(), "{group}");
    assert_eq!(group["surface"]["kind"], "group");
    // A surface model in the list is elided, and objects.get has the geometry.
    let model = row(&list, &b);
    assert!(model["shape"]["elided"].is_string());
    assert!(ok(&mut s, "objects.get", json!({"id": b}))["shape"]["vertices"].is_array());
}

#[test]
fn a_big_model_says_that_it_is_drawn_as_its_outline_in_the_api() {
    let mut s = Session::with_sequential_ids();
    // 101 x 101 squares: 20,402 faces and 30,603 edges, over the limits of the plan.
    let id = add_grid(&mut s, 101);
    let o = ok(&mut s, "objects.get", json!({"id": id}));
    let plan = &o["surface"]["plan"];
    assert_eq!(plan["detail"], "outline");
    assert_eq!(plan["faces"], 20_402);
    assert_eq!(plan["limit_faces"], 20_000);
    assert!(
        plan["note"]
            .as_str()
            .unwrap()
            .starts_with("Drawn as outline"),
        "{plan}"
    );
    // The state of the solid is not checked for this size by itself.
    assert_eq!(o["surface"]["solid"]["state"], "unchecked");
    // The list says it too, and a small model has no plan field.
    let small = shed(&mut s, "Shed", 5000.0);
    let list = ok(&mut s, "objects.list", json!({"kind": "surface_model"}));
    assert_eq!(row(&list, &id)["surface"]["plan"]["detail"], "outline");
    assert!(row(&list, &small)["surface"].get("plan").is_none());
    // The plan in render.svg says it, and so does the data of the group in the SVG.
    let svg = ok(&mut s, "render.svg", json!({}));
    assert_eq!(svg["notes"].as_array().unwrap().len(), 1, "{svg}");
    assert!(svg["notes"][0].as_str().unwrap().contains("outline"));
    assert_eq!(svg["simplified"][0]["object"], id);
    assert!(
        svg["svg"]
            .as_str()
            .unwrap()
            .contains(r#"data-detail="outline""#)
    );
    // Without a big model there are no notes.
    let mut t = Session::with_sequential_ids();
    shed(&mut t, "Shed", 0.0);
    assert!(ok(&mut t, "render.svg", json!({})).get("notes").is_none());
}

#[test]
fn a_hidden_line_picture_says_when_it_leaves_edges_out() {
    let mut s = Session::with_sequential_ids();
    // 130 x 130 squares: 50,960 lines, over the 50,000 lines of one object.
    add_grid(&mut s, 130);
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "mode": "hidden_line", "width": 64, "height": 64}),
    );
    let notes = r["notes"].as_array().expect("a note");
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("hidden-line limit")),
        "{r}"
    );
    // A shaded picture has no such note.
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "mode": "shaded", "width": 64, "height": 64}),
    );
    assert!(r.get("notes").is_none(), "{r}");
}

#[test]
fn the_solid_check_of_a_list_is_kept_for_the_model_and_not_run_again() {
    let mut s = Session::with_sequential_ids();
    let id = shed(&mut s, "Shed", 0.0);
    let list = ok(&mut s, "objects.list", json!({"kind": "surface_model"}));
    assert_eq!(row(&list, &id)["surface"]["solid"]["state"], "closed");
    // An edit of another object does not make it unchecked: a list with no budget still knows.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}),
    );
    let list = ok(&mut s, "objects.list", json!({"kind": "surface_model"}));
    assert_eq!(row(&list, &id)["surface"]["solid"]["state"], "closed");
}
