//! Exports leave out guides and hidden items, and say how many (MOD-05).

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::Session;

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kadakas-api-guides-{}-{name}", std::process::id()))
}

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {e:?}"))
}

/// Two lines and two guides. The ids of the lines come back.
fn session() -> (Session, Vec<Value>) {
    let mut s = Session::with_sequential_ids();
    let mut ids = Vec::new();
    for shape in [
        json!({"kind": "line", "a": [0, 0], "b": [1000, 0]}),
        json!({"kind": "line", "a": [0, 10], "b": [1000, 10]}),
        json!({"kind": "guide", "at": [0, 0, 0], "dir": [1, 0, 0]}),
        json!({"kind": "guide", "at": [5, 5, 0]}),
    ] {
        let r = ok(
            &mut s,
            "execute",
            json!({"command": {"op": "add_object", "shape": shape}}),
        );
        ids.push(r["created"][0].clone());
    }
    (s, ids)
}

#[test]
fn guides_can_be_added_and_read_back_through_the_api() {
    let (s, _) = session();
    assert_eq!(s.editor.doc().guide_count(), 2);
}

#[test]
fn an_export_says_how_many_guides_it_left_out() {
    let (mut s, _) = session();
    for (name, format) in [
        ("a.dxf", "dxf"),
        ("a.glb", "glb"),
        ("a.obj", "obj"),
        ("a.ifc", "ifc"),
    ] {
        let path = tmp(name);
        let r = ok(
            &mut s,
            "export.file",
            json!({"path": path, "format": format, "overwrite": true}),
        );
        assert_eq!(r["left_out"]["guides"], 2, "{format}: {r}");
        let notes = r["left_out"]["notes"].as_array().unwrap();
        assert_eq!(notes[0], "2 guides left out: construction geometry");
        let warnings = r["warnings"].as_array().unwrap();
        assert!(
            warnings
                .iter()
                .any(|w| w == "2 guides left out: construction geometry"),
            "{format}: {r}"
        );
        let _ = std::fs::remove_file(path);
    }
    // The document itself keeps the guides.
    assert_eq!(s.editor.doc().guide_count(), 2);
}

#[test]
fn an_export_with_nothing_to_leave_out_has_no_note() {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}}}),
    );
    let path = tmp("b.dxf");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": path, "overwrite": true}),
    );
    assert!(r.get("left_out").is_none(), "{r}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn hidden_objects_are_left_out_and_counted() {
    let (mut s, ids) = session();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_hidden", "ids": [ids[1]], "hidden": true}}),
    );
    let path = tmp("c.geojson");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": path, "format": "geojson", "overwrite": true}),
    );
    assert_eq!(r["left_out"]["hidden_objects"], 1, "{r}");
    assert_eq!(r["exported"], 1, "only the shown line is written: {r}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("\"10.0\"") && text.matches("coordinates").count() >= 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_api_refuses_to_write_the_reserved_data_keys_and_reads_stay_free() {
    let (mut s, ids) = session();
    let line = ids[0].clone();
    for command in [
        json!({"op": "set_data", "id": line, "set": {"kadakas.hidden": true}}),
        json!({"op": "set_data", "id": line, "set": {}, "remove": ["kadakas.plugin"]}),
        json!({"op": "batch", "commands": [
            {"op": "set_data", "id": line, "set": {"note": 1}},
            {"op": "set_data", "id": line, "set": {"kadakas.anything": 1}}
        ]}),
    ] {
        let e = s
            .handle("execute", json!({"command": command.clone()}))
            .unwrap_err();
        assert!(format!("{e:?}").contains("reserved"), "{command}: {e:?}");
    }
    // Nothing of the batch was applied.
    let o = s.handle("objects.get", json!({"id": line})).unwrap();
    assert!(o["data"].get("note").is_none());
    // Another key works, and the mark that set_hidden wrote can be read.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_data", "id": line, "set": {"note": 1}}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_hidden", "ids": [line], "hidden": true}}),
    );
    let o = s.handle("objects.get", json!({"id": line})).unwrap();
    assert_eq!(o["data"]["kadakas.hidden"], true);
    assert_eq!(o["data"]["note"], 1);
}
