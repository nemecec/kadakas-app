//! `massing.from_shapes {delete_source: false}` keeps the shape as it is, and the model keeps a link
//! to it. The model does not follow the shape until `massing.follow_sources` is called.

use kadakas_api::Session;
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    s.handle(m, p)
        .unwrap_or_else(|e| panic!("{m} failed: {} {}", e.code, e.message))
}

fn square(s: &mut Session, w: i64) -> Value {
    let r = call(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "polyline", "closed": true,
            "points": [[0, 0], [w, 0], [w, w], [0, w]]}}}),
    );
    let id = r["created"][0].clone();
    // A tag of an import: the model must not take a copy of it.
    call(
        s,
        "execute",
        json!({"command": {"op": "set_data", "id": id, "set": {
            "import": {"drawing": "d", "revision": "r", "handle": "1", "kind": "LWPOLYLINE",
                "hash": "0123456789abcdef", "obj": "0000000000000000"},
            "height": 6}}}),
    );
    id
}

#[test]
fn the_model_links_to_its_shape_and_follows_on_request() {
    let mut s = Session::with_sequential_ids();
    let shape = square(&mut s, 10_000);
    let before = call(&mut s, "objects.get", json!({"id": shape}));
    let r = call(
        &mut s,
        "massing.from_shapes",
        json!({"ids": [shape], "delete_source": false}),
    );
    let model = r["created"][0]["id"].clone();
    assert_eq!(r["created"][0]["follows_source"], false);
    assert!(r["note"].as_str().unwrap().contains("follow_sources"));
    assert_eq!(call(&mut s, "objects.get", json!({"id": shape})), before);
    let m = call(&mut s, "objects.get", json!({"id": model}));
    assert!(m["data"].get("import").is_none(), "no copy of the tag");
    assert_eq!(m["data"]["massing_source"], shape);
    assert_eq!(m["data"]["height"], 6, "the properties go along");
    // Nothing to follow yet.
    let f = call(&mut s, "massing.follow_sources", json!({}));
    assert_eq!(
        f,
        json!({"changed": [], "unchanged": [model], "source_missing": []})
    );
    // The shape changes: the model keeps its footprint until it is asked.
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_object", "id": shape, "shape": {"kind": "polyline", "closed": true,
            "points": [[0, 0], [12_000, 0], [12_000, 10_000], [0, 10_000]]}}}),
    );
    let now = call(&mut s, "massing.get", json!({"id": model}));
    assert_eq!(now["footprint"][1], json!([10_000.0, 0.0]));
    let f = call(&mut s, "massing.follow_sources", json!({"ids": [model]}));
    assert_eq!(f["changed"], json!([model]));
    let now = call(&mut s, "massing.get", json!({"id": model}));
    assert_eq!(now["footprint"][1], json!([12_000.0, 0.0]));
    // Models without a link are left out; a lost shape is reported.
    let plain = call(
        &mut s,
        "massing.add",
        json!({"footprint": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]]}),
    )["id"]
        .clone();
    let f = call(&mut s, "massing.follow_sources", json!({}));
    assert_eq!(f["changed"], json!([]));
    assert!(!f["unchanged"].as_array().unwrap().contains(&plain));
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [shape]}}),
    );
    let f = call(&mut s, "massing.follow_sources", json!({}));
    assert_eq!(f["source_missing"], json!([model]));
    // The source of the default (delete_source true) is replaced and the model has no link.
    let shape2 = square(&mut s, 5000);
    let r = call(&mut s, "massing.from_shapes", json!({"ids": [shape2]}));
    let m = call(&mut s, "objects.get", json!({"id": r["created"][0]["id"]}));
    assert!(m["data"].get("massing_source").is_none());
    assert!(r["note"].as_str().unwrap().contains("replaced"));
}
