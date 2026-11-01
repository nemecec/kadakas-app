//! AGR-09: the gaps that an agent found while it built a model through `mesh.*`: a listed
//! delete method, the closest methods in the answer to an unknown name, and the faces and edges
//! in `selection.get`.

use kadakas_api::{METHODS, RpcError, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, RpcError> {
    s.handle(method, params)
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {} {:?}", e.message, e.details))
}

fn err(s: &mut Session, method: &str, params: Value) -> RpcError {
    match call(s, method, params) {
        Ok(v) => panic!("{method} should fail but gave {v}"),
        Err(e) => e,
    }
}

fn error_id(e: &RpcError) -> String {
    e.details.as_ref().unwrap()["error_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn rect(s: &mut Session, x: f64) -> String {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [x, 0], "max": [x + 10.0, 10]}}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string()
}

fn count(s: &mut Session) -> u64 {
    ok(s, "document.summary", json!({}))["object_count"]
        .as_u64()
        .unwrap()
}

/// A model with one box, and the ids of its faces and edges.
fn box_model(s: &mut Session) -> (String, Vec<u64>, Vec<u64>) {
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
    let list = |s: &mut Session, m: &str| -> Vec<u64> {
        ok(s, m, json!({"id": id, "limit": 1000}))["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_u64().unwrap())
            .collect()
    };
    let faces = list(s, "mesh.faces");
    let edges = list(s, "mesh.edges");
    (id, faces, edges)
}

#[test]
fn objects_delete_is_listed_and_has_a_schema() {
    assert!(METHODS.iter().any(|(m, _)| *m == "objects.delete"));
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({"method": "objects.delete"}));
    assert!(d["doc"].as_str().unwrap().contains("delete_objects"));
    assert_eq!(d["params"]["required"], json!(["ids"]));
}

#[test]
fn objects_delete_removes_objects_in_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    let (a, b, c) = (rect(&mut s, 0.0), rect(&mut s, 20.0), rect(&mut s, 40.0));
    ok(&mut s, "selection.set", json!({"ids": [a, b]}));
    let depth = s.editor.undo_depth();
    let out = ok(&mut s, "objects.delete", json!({"ids": [a, b]}));
    assert_eq!(out["count"], 2);
    assert_eq!(out["deleted"], json!([a, b]));
    assert_eq!(count(&mut s), 1);
    assert_eq!(s.editor.undo_depth(), depth + 1, "one undo step");
    // The deleted objects leave the selection.
    assert_eq!(ok(&mut s, "selection.get", json!({}))["ids"], json!([]));
    assert_eq!(ok(&mut s, "undo", json!({}))["done"], true);
    assert_eq!(count(&mut s), 3);
    assert_eq!(s.editor.undo_depth(), depth);
    // The same result as the command form.
    let before = ok(&mut s, "document.get", json!({}));
    ok(&mut s, "objects.delete", json!({"ids": [c]}));
    ok(&mut s, "undo", json!({}));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [c]}}),
    );
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "document.get", json!({})), before);
}

#[test]
fn objects_delete_takes_the_members_of_a_group_and_a_surface_model() {
    let mut s = Session::with_sequential_ids();
    let (a, b) = (rect(&mut s, 0.0), rect(&mut s, 20.0));
    let g = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "group", "ids": [a, b]}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string();
    let (model, _, _) = box_model(&mut s);
    let out = ok(&mut s, "objects.delete", json!({"ids": [g, model]}));
    assert_eq!(out["count"], 4, "{out}");
    assert_eq!(count(&mut s), 0);
}

#[test]
fn objects_delete_refuses_an_unknown_id_and_deletes_nothing() {
    let mut s = Session::with_sequential_ids();
    let a = rect(&mut s, 0.0);
    let missing = kadakas_core::uid::Uid::from_num(999).to_string();
    let e = err(&mut s, "objects.delete", json!({"ids": [a, missing]}));
    assert_eq!((e.code, error_id(&e).as_str()), (2, "no_such_object"));
    assert!(e.message.contains(&missing), "{}", e.message);
    assert_eq!(count(&mut s), 1);
    let e = err(&mut s, "objects.delete", json!({"ids": []}));
    assert_eq!(error_id(&e), "ids_empty");
    assert_eq!(err(&mut s, "objects.delete", json!({})).code, -32602);
    assert_eq!(
        err(&mut s, "objects.delete", json!({"ids": [a], "idz": 1})).code,
        -32602
    );
    assert_eq!(count(&mut s), 1);
}

#[test]
fn objects_delete_follows_the_rules_of_locked_layers() {
    let mut s = Session::with_sequential_ids();
    let a = rect(&mut s, 0.0);
    let layer = ok(&mut s, "document.summary", json!({}))["layers"][0]["id"].clone();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": layer, "locked": true}}),
    );
    let command = err(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [a]}}),
    );
    let method = err(&mut s, "objects.delete", json!({"ids": [a]}));
    assert_eq!(
        (method.code, &method.message),
        (command.code, &command.message)
    );
    assert_eq!(count(&mut s), 1);
}

#[test]
fn objects_delete_counts_as_a_change_of_the_document() {
    // The policy table classifies it as an edit, so a read-only script cannot call it and the
    // activity list of the agent shows it.
    assert!(kadakas_api::may_change_document("objects.delete"));
    assert!(!kadakas_api::may_change_document("selection.get"));
}

#[test]
fn an_unknown_method_names_the_closest_listed_ones() {
    let mut s = Session::with_sequential_ids();
    for guess in [
        "object.delete",
        "objects.remove",
        "delete_objects",
        "objects.erase",
    ] {
        let e = err(&mut s, guess, json!({"ids": []}));
        assert_eq!((e.code, e.kind), (-32601, "method_not_found"), "{guess}");
        assert!(e.message.starts_with(&format!("unknown method: {guess}")));
        assert!(e.message.contains("objects.delete {ids}"), "{}", e.message);
        assert!(e.message.contains("\"delete_objects\""), "{}", e.message);
        let near = e.details.as_ref().unwrap()["closest"]
            .as_array()
            .unwrap()
            .clone();
        assert!(!near.is_empty() && near.len() <= 3, "{guess}: {near:?}");
        assert!(
            near.iter()
                .all(|m| METHODS.iter().any(|(n, _)| n == &m.as_str().unwrap())),
            "{guess}: {near:?}"
        );
    }
    // A typing mistake names the listed method, in a family that has its own handler too.
    let e = err(&mut s, "mesh.facez", json!({}));
    assert!(
        e.message.contains("Closest listed methods: mesh.faces"),
        "{}",
        e.message
    );
    let e = err(&mut s, "objcts.list", json!({}));
    assert!(
        e.message.contains("Closest listed methods: objects.list"),
        "{}",
        e.message
    );
    // A name that is near nothing says where the list is.
    let e = err(&mut s, "qqqqqqqqqqqq", json!({}));
    assert!(
        e.message.contains("describe lists the methods"),
        "{}",
        e.message
    );
}

#[test]
fn selection_get_has_empty_maps_when_nothing_is_selected() {
    let mut s = Session::with_sequential_ids();
    assert_eq!(
        ok(&mut s, "selection.get", json!({})),
        json!({"ids": [], "faces": {}, "edges": {}})
    );
}

#[test]
fn selection_holds_faces_and_edges_with_the_ids_of_mesh_faces_and_edges() {
    let mut s = Session::with_sequential_ids();
    let (model, faces, edges) = box_model(&mut s);
    assert_eq!(faces.len(), 6);
    let out = ok(
        &mut s,
        "selection.set",
        json!({"faces": {model.clone(): [faces[1], faces[0]]}, "edges": {model.clone(): [edges[2]]}}),
    );
    let want = json!({
        "ids": [],
        "faces": {model.clone(): [faces[0].min(faces[1]), faces[0].max(faces[1])]},
        "edges": {model.clone(): [edges[2]]},
    });
    assert_eq!(out, want);
    assert_eq!(ok(&mut s, "selection.get", json!({})), want);
    // The ids are the ones that the readers give.
    let listed = ok(
        &mut s,
        "mesh.faces",
        json!({"id": model, "ids": [faces[0]]}),
    );
    assert_eq!(listed["items"][0]["id"], faces[0]);
    // A set replaces all: ids alone clears the parts.
    let a = rect(&mut s, 0.0);
    let out = ok(&mut s, "selection.set", json!({"ids": [a]}));
    assert_eq!(out, json!({"ids": [a], "faces": {}, "edges": {}}));
    ok(&mut s, "selection.set", json!({"ids": []}));
    assert_eq!(
        ok(&mut s, "selection.get", json!({})),
        json!({"ids": [], "faces": {}, "edges": {}})
    );
}

#[test]
fn selection_set_checks_the_parts_and_the_mix() {
    let mut s = Session::with_sequential_ids();
    let (model, faces, edges) = box_model(&mut s);
    let a = rect(&mut s, 0.0);
    let e = err(
        &mut s,
        "selection.set",
        json!({"faces": {model.clone(): [9999]}}),
    );
    assert_eq!((e.code, error_id(&e).as_str()), (2, "no_such_face"));
    let e = err(
        &mut s,
        "selection.set",
        json!({"edges": {model.clone(): [9999]}}),
    );
    assert_eq!(error_id(&e), "no_such_edge");
    let e = err(&mut s, "selection.set", json!({"faces": {a.clone(): [1]}}));
    assert_eq!(error_id(&e), "not_a_surface_model");
    let e = err(&mut s, "selection.set", json!({"faces": {"nonsense": [1]}}));
    assert_eq!(e.code, -32602);
    let e = err(&mut s, "selection.set", json!({}));
    assert_eq!(e.code, -32602);
    let e = err(
        &mut s,
        "selection.set",
        json!({"ids": [a], "faces": {model.clone(): [faces[0]]}}),
    );
    assert_eq!(e.code, -32602);
    // A refused call leaves the selection as it was.
    ok(
        &mut s,
        "selection.set",
        json!({"edges": {model.clone(): [edges[0]]}}),
    );
    err(
        &mut s,
        "selection.set",
        json!({"faces": {model.clone(): [9999]}}),
    );
    assert_eq!(
        ok(&mut s, "selection.get", json!({}))["edges"],
        json!({model: [edges[0]]})
    );
}

#[test]
fn selected_parts_go_when_the_part_or_the_model_goes() {
    let mut s = Session::with_sequential_ids();
    let (model, faces, edges) = box_model(&mut s);
    ok(
        &mut s,
        "selection.set",
        json!({"faces": {model.clone(): faces.clone()}, "edges": {model.clone(): [edges[0]]}}),
    );
    // An undo that removes the faces drops them from the selection.
    ok(&mut s, "undo", json!({}));
    let got = ok(&mut s, "selection.get", json!({}));
    assert_eq!(
        (got["faces"].clone(), got["edges"].clone()),
        (json!({}), json!({})),
        "{got}"
    );
    // Deleting the model drops its parts too.
    ok(&mut s, "redo", json!({}));
    ok(
        &mut s,
        "selection.set",
        json!({"faces": {model.clone(): [faces[0]]}}),
    );
    ok(&mut s, "objects.delete", json!({"ids": [model]}));
    assert_eq!(
        ok(&mut s, "selection.get", json!({})),
        json!({"ids": [], "faces": {}, "edges": {}})
    );
}

#[test]
fn an_editor_takes_the_change_of_selection_set_once() {
    let mut s = Session::with_sequential_ids();
    let (model, faces, _) = box_model(&mut s);
    assert!(s.take_parts_change().is_none());
    ok(
        &mut s,
        "selection.set",
        json!({"faces": {model.clone(): [faces[2]]}}),
    );
    let parts = s.take_parts_change().expect("a change");
    assert_eq!(parts.faces().count(), 1);
    assert!(s.take_parts_change().is_none(), "taken once");
}
