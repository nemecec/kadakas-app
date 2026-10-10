//! Files from a newer Cadektor: unknown shape kinds and unknown fields survive load and save.

use cadektor_core::command::{Command, Editor};
use cadektor_core::model::{Document, FORMAT_VERSION, LoadError, ObjectId, Shape};
use glam::DVec2;
use serde_json::{Value, json};

/// A document with one circle, as JSON.
fn base_json() -> Value {
    let mut ed = Editor::default();
    ed.execute(&Command::AddObject {
        shape: Shape::Circle {
            center: DVec2::new(1.0, 2.0),
            radius: 3.0,
        },
        layer: None,
        class: None,
        attrs: Default::default(),
    })
    .unwrap();
    serde_json::to_value(ed.doc()).unwrap()
}

/// The same document, as a newer build could write it: a shape kind that we do not know,
/// and fields that we do not know on the document, a layer, a class, an object and the drawing.
fn future_json() -> Value {
    let mut v = base_json();
    v["new_top_level"] = json!({"a": [1, 2, 3], "b": null});
    v["layers"][0]["future_layer_field"] = json!("x");
    v["classes"][0]["future_class_field"] = json!(7);
    let obj = &mut v["objects"][0];
    obj["shape"] =
        json!({"kind": "spiral", "turns": 3.5, "center": [0, 0], "nested": {"k": [true]}});
    obj["future_object_field"] = json!({"z": 1});
    v["drawing"] = serde_json::to_value(cadektor_core::sheets::Drawing::default()).unwrap();
    v["drawing"]["future_drawing_field"] = json!([1]);
    v
}

#[test]
fn the_format_version_is_2_and_old_files_migrate() {
    assert_eq!(FORMAT_VERSION, 2);
    let mut v = base_json();
    v["format_version"] = json!(1);
    let doc = Document::from_value(v.clone()).unwrap();
    assert_eq!(
        doc.format_version, 2,
        "the migration records the new version"
    );
    assert_eq!(doc.objects.len(), 1, "no data changed");
    assert_eq!(serde_json::to_value(&doc).unwrap(), base_json());
    // Version 0 never existed.
    v["format_version"] = json!(0);
    assert!(matches!(
        Document::from_value(v),
        Err(LoadError::Invalid(_))
    ));
}

#[test]
fn a_newer_file_is_not_called_a_non_cadektor_file() {
    let mut v = base_json();
    v["format_version"] = json!(FORMAT_VERSION + 1);
    // A newer file may have a different shape. The version is checked first.
    v["layers"] = json!("???");
    let e = Document::from_value(v).unwrap_err();
    assert_eq!(e, LoadError::Newer(FORMAT_VERSION + 1));
    assert_eq!(
        e.to_string(),
        format!(
            "file was made by a newer Cadektor (format {})",
            FORMAT_VERSION + 1
        )
    );
    assert!(!e.to_string().contains("not a Cadektor file"));
}

#[test]
fn a_future_shape_and_unknown_fields_round_trip_unchanged() {
    let original = future_json();
    let text = serde_json::to_string(&original).unwrap();
    let doc = Document::from_json(&text).unwrap();
    // The unknown shape is kept as it is.
    let o = doc.objects.values().next().unwrap();
    match &o.shape {
        Shape::Unknown { kind, raw } => {
            assert_eq!(kind, "spiral");
            assert_eq!(raw, &original["objects"][0]["shape"]);
        }
        other => panic!("expected an unknown shape, got {other:?}"),
    }
    assert_eq!(o.shape.kind(), "unknown");
    assert_eq!(serde_json::to_value(&doc).unwrap(), original);
    // And a second round gives the same again.
    let again = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&again).unwrap(), original);
    assert_eq!(again, doc);
}

#[test]
fn a_known_kind_with_bad_fields_is_still_an_error() {
    let mut v = base_json();
    v["objects"][0]["shape"] = json!({"kind": "circle", "center": [0, 0]});
    assert!(matches!(Document::from_value(v), Err(LoadError::Parse(_))));
    let mut v = base_json();
    v["objects"][0]["shape"] = json!({"center": [0, 0]});
    assert!(matches!(Document::from_value(v), Err(LoadError::Parse(_))));
}

#[test]
fn an_unknown_shape_can_only_be_deleted() {
    let doc = Document::from_value(future_json()).unwrap();
    let id: ObjectId = *doc.objects.keys().next().unwrap();
    let mut ed = Editor::new(doc);
    let before = serde_json::to_value(ed.doc()).unwrap();

    let e = ed
        .execute(&Command::MoveObjects {
            ids: vec![id],
            delta: DVec2::new(5.0, 5.0),
        })
        .unwrap_err();
    assert!(e.to_string().contains("unknown kind"), "{e}");
    assert_eq!(
        serde_json::to_value(ed.doc()).unwrap(),
        before,
        "nothing changed"
    );

    // A new object cannot be an unknown shape.
    let e = ed
        .execute(&Command::AddObject {
            shape: Shape::Unknown {
                kind: "spiral".into(),
                raw: json!({"kind": "spiral"}),
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap_err();
    assert!(e.to_string().contains("unknown kind"), "{e}");

    // Delete works, and undo brings the shape back unchanged.
    ed.execute(&Command::DeleteObjects { ids: vec![id] })
        .unwrap();
    assert!(ed.doc().objects.is_empty());
    assert!(ed.undo());
    let after = serde_json::to_value(ed.doc()).unwrap();
    for (k, v) in before.as_object().unwrap() {
        assert_eq!(&after[k], v, "{k}");
    }
    assert_eq!(after, before);
}

#[test]
fn an_unknown_shape_has_no_geometry() {
    let doc = Document::from_value(future_json()).unwrap();
    let o = doc.objects.values().next().unwrap();
    assert!(o.shape.is_finite());
    // It is not drawn.
    let svg = cadektor_core::svg::render(&doc, 400);
    assert!(!svg.contains("spiral"), "{svg}");
    assert!(!svg.contains("data-id"), "{svg}");
}
