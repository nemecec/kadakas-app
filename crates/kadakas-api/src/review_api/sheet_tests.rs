//! A restore gives back a sheet layer that was deleted: its page settings, its viewport and its
//! title block.

use serde_json::{Value, json};

use kadakas_core::model::Shape;
use kadakas_review::restore::without_stacking_keys;

use super::*;

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn version(s: &mut Session, name: &str) -> u64 {
    ok(s, "version.create", json!({"name": name, "now_ms": 1}))["version"]["id"]
        .as_u64()
        .unwrap()
}

#[test]
fn restoring_a_deleted_sheet_layer_brings_back_its_page_and_content() {
    let mut s = Session::with_sequential_ids();
    let layer = ok(
        &mut s,
        "sheet.create",
        json!({"name": "Plan", "paper": "a1", "orientation": "portrait",
            "number": "L-101", "title": "Planting plan"}),
    )["layer"]
        .clone();
    let vp = ok(
        &mut s,
        "viewport.create",
        json!({"sheet": "Plan", "scale": 50}),
    )["id"]
        .clone();
    let tb = ok(
        &mut s,
        "title_block.create",
        json!({"sheet": "Plan", "grid_columns": 6, "grid_rows": 4}),
    )["id"]
        .clone();
    let first = s.editor.doc().clone();
    let one = version(&mut s, "one");
    ok(
        &mut s,
        "sheet.set",
        json!({"layer": layer, "paper": {"custom": {"width_mm": 500, "height_mm": 300}},
            "orientation": "landscape", "number": "L-102"}),
    );
    let second = s.editor.doc().clone();
    let two = version(&mut s, "two");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [vp, tb]}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_layer", "id": layer}}),
    );
    assert!(s.editor.doc().drawing.sheets.is_empty());
    version(&mut s, "without");
    // The file is written and read again: the versions come from the text.
    let mut text = Vec::new();
    s.document_for_save()
        .unwrap()
        .write_pretty(&mut text)
        .unwrap();
    let file = Document::from_json(std::str::from_utf8(&text).unwrap()).unwrap();
    let mut s = Session::with_document(file);
    for (id, want) in [(two, &second), (one, &first), (two, &second)] {
        ok(&mut s, "version.restore", json!({"id": id, "now_ms": 2}));
        let mut doc = s.editor.doc().clone();
        let mut want = (*want).clone();
        without_stacking_keys(&mut doc);
        without_stacking_keys(&mut want);
        assert_eq!(doc.drawing, want.drawing, "version {id}");
        assert_eq!(doc.layers, want.layers, "version {id}");
        assert!(doc.objects.iter().eq(want.objects.iter()), "version {id}");
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "delete_objects", "ids": [vp, tb]}}),
        );
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "delete_layer", "id": layer}}),
        );
    }
}

fn exec(s: &mut Session, command: Value) -> Value {
    ok(s, "execute", json!({"command": command}))
}

/// Takes the document out, empties it with `document.load` (the versions stay) and restores.
fn restore_after_wipe(s: &mut Session) -> (Document, Document) {
    let id = version(s, "full");
    let mut want = s.editor.doc().clone();
    let bare = serde_json::to_value(Document::with_sequential_ids()).unwrap();
    ok(s, "document.load", json!({"document": bare}));
    assert_ne!(s.editor.doc().layers.len(), want.layers.len().max(2) + 1);
    ok(s, "version.restore", json!({"id": id, "now_ms": 2}));
    let mut got = s.editor.doc().clone();
    without_stacking_keys(&mut got);
    without_stacking_keys(&mut want);
    (got, want)
}

#[test]
fn restoring_after_a_wipe_gives_back_every_kind_of_setting() {
    let mut s = Session::with_sequential_ids();
    // Design layer and class settings.
    let layer = exec(
        &mut s,
        json!({"op": "add_layer", "name": "Planting", "elevation": 450, "scale": 50}),
    )["created"][0]
        .clone();
    let class = exec(
        &mut s,
        json!({"op": "add_class", "name": "Trees", "attrs": {"pen": [0, 100, 0, 255]}}),
    )["created"][0]
        .clone();
    exec(
        &mut s,
        json!({"op": "modify_class", "id": class, "visible": false}),
    );
    // Project data, georeference, sun and display rules.
    exec(
        &mut s,
        json!({"op": "set_project", "name": "Riverside", "client": "Smith", "revision": "B"}),
    );
    exec(
        &mut s,
        json!({"op": "set_georeference", "georef": {"crs": "EPSG:3067",
            "origin": [385000.0, 6672000.0], "rotation": 0.5}}),
    );
    exec(
        &mut s,
        json!({"op": "set_view_rules", "rules": {"ne_grid": {"spacing": 25.0}}}),
    );
    exec(
        &mut s,
        json!({"op": "set_sun", "sun": {"latitude": 60.0, "longitude": 24.0, "year": 2026,
            "month": 6, "day": 21, "hour": 12.0, "tz_offset": 2.0, "dst": true,
            "north_angle": 90.0}}),
    );
    // Resources: a worksheet and a record format, the record on an object, a table of the sheet.
    let sheet_res = exec(
        &mut s,
        json!({"op": "add_resource", "name": "Areas",
            "body": {"type": "worksheet", "cells": {"A1": "Plant list"}}}),
    )["created"][0]
        .clone();
    let format = exec(
        &mut s,
        json!({"op": "add_resource", "name": "Soil", "body": {"type": "record_format",
            "fields": [{"name": "Depth", "type": "dimension", "default": 300}]}}),
    )["created"][0]
        .clone();
    let rect = exec(
        &mut s,
        json!({"op": "add_object", "layer": layer, "shape":
            {"kind": "rect", "min": [0, 0], "max": [10, 10]}}),
    )["created"][0]
        .clone();
    exec(
        &mut s,
        json!({"op": "attach_record", "ids": [rect], "record": format, "values": {"Depth": 250}}),
    );
    exec(
        &mut s,
        json!({"op": "modify_layer", "id": layer, "locked": true, "visible": false}),
    );
    // A sheet with a viewport, a title block and a table.
    ok(
        &mut s,
        "sheet.create",
        json!({"name": "Plan", "paper": "a1", "orientation": "portrait", "number": "L-101"}),
    );
    ok(
        &mut s,
        "viewport.create",
        json!({"sheet": "Plan", "scale": 50}),
    );
    ok(&mut s, "title_block.create", json!({"sheet": "Plan"}));
    let sheet_layer = ok(&mut s, "sheet.list", json!({}))["sheets"][0]["id"].clone();
    exec(
        &mut s,
        json!({"op": "add_worksheet_table", "worksheet": sheet_res, "layer": sheet_layer,
            "at": [10, 200], "size": [120, 40]}),
    );
    // A saved view.
    ok(&mut s, "saved_view.save", json!({"name": "Planting only"}));
    let (got, want) = restore_after_wipe(&mut s);
    // The test is not empty: every kind is in the document that was restored.
    assert_eq!(want.layers.len(), 3);
    assert!(want.layers.iter().any(|l| l.locked && l.scale == 50.0));
    assert!(want.classes.iter().any(|c| c.name == "Trees" && !c.visible));
    assert_eq!(want.resources.iter().count(), 2);
    assert_eq!(want.drawing.saved_views.len(), 1);
    assert_eq!(want.drawing.sheets.len(), 1);
    assert!(want.drawing.view_rules.ne_grid.is_some());
    assert!(want.drawing.presentation.sun.is_some());
    assert!(
        want.objects
            .values()
            .any(|o| matches!(o.shape, Shape::TitleBlock(_)))
    );
    assert_eq!(got.layers, want.layers, "layer settings");
    assert_eq!(got.classes, want.classes, "class settings");
    assert_eq!(
        got.resources, want.resources,
        "worksheets and record formats"
    );
    assert_eq!(
        got.drawing, want.drawing,
        "sheets, project, views, display rules"
    );
    assert_eq!(got.georef, want.georef);
    // The objects, by ID: the order inside a layer counts, the interleave of layers does not.
    let objects = |d: &Document| {
        let mut list: Vec<_> = d.objects.values().collect();
        list.sort_by_key(|o| o.id.0);
        serde_json::to_value(list).unwrap()
    };
    assert_eq!(
        objects(&got),
        objects(&want),
        "objects with records and tables"
    );
}
