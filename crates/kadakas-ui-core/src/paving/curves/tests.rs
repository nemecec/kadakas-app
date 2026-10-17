use super::*;
use crate::paving::fake::Fake;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, Shape};
use serde_json::json;

fn square() -> Value {
    json!([[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [0.0, 3000.0]])
}

fn vertices_of_square() -> Vec<Vertex> {
    serde_json::from_value(square()).unwrap()
}

fn reply() -> Value {
    json!({
        "regions": [
            {"index": 0, "outline": square(), "holes": [square()], "area_m2": 12.0, "depth": 0,
             "role": "area", "doubt": null, "inside": [100.0, 100.0],
             "bbox": {"lo": [0.0, 0.0], "hi": [4000.0, 3000.0]}, "sources": []},
            {"index": 1, "outline": square(), "holes": [], "area_m2": 1.0, "depth": 1,
             "role": "island", "doubt": null, "inside": [2000.0, 1500.0],
             "bbox": {"lo": [1000.0, 1000.0], "hi": [2000.0, 2000.0]}, "sources": []},
            {"index": 2, "outline": square(), "holes": [], "area_m2": 0.5, "depth": 0,
             "role": "island?", "doubt": "check it", "inside": [3000.0, 1500.0],
             "bbox": {"lo": [0.0, 0.0], "hi": [1.0, 1.0]}, "sources": []},
        ],
        "closed_gaps": {"count": 2, "total_movement_mm": 1.5, "chained_nodes": 0,
            "listed": [{"at": [10.0, 20.0], "size_mm": 0.8, "moved_mm": 0.4}]},
        "dead_ends": {"count": 2, "open": 1, "stubs": 1, "listed": [
            {"at": [30.0, 40.0], "kind": "open", "length_mm": 250.0,
             "nearest": {"at": [31.0, 41.0], "distance_mm": 12.0}},
            {"at": [50.0, 60.0], "kind": "stub", "length_mm": 5.0, "nearest": null}]},
        "complete": false,
        "skipped": [{"id": "x", "reason": "no such object"}],
        "ignored_short": 3, "duplicates": 1, "dropped_slivers": 1,
        "dropped_sliver_list": [{"at": [70.0, 80.0], "area_mm2": 2.0}],
        "warnings": ["a warning"],
        "tolerance_mm": 1.0, "dry_run": true,
    })
}

fn line(editor: &mut Editor, a: (f64, f64), b: (f64, f64)) -> ObjectId {
    ObjectId(
        editor
            .execute(&Command::AddObject {
                shape: Shape::Polyline {
                    points: kadakas_core::curve::corners(&[
                        glam::DVec2::new(a.0, a.1),
                        glam::DVec2::new(b.0, b.1),
                    ]),
                    closed: false,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
    )
}

#[test]
fn the_reply_becomes_regions_notes_and_marks() {
    let f = Found::parse(&reply()).unwrap();
    assert_eq!(f.regions.len(), 3);
    assert_eq!(f.regions[0].holes.len(), 1);
    assert_eq!(f.regions[0].outline, vertices_of_square());
    assert_eq!(
        [f.regions[0].role, f.regions[1].role, f.regions[2].role],
        [Role::Area, Role::Island, Role::Doubtful]
    );
    assert_eq!(f.regions[2].doubt.as_deref(), Some("check it"));
    assert!(!f.complete);
    assert_eq!((f.gaps, f.dead_ends), (2, 2));
    // Every kind of note is there, and the ones with a place carry it.
    for kind in [
        NoteKind::GapClosed,
        NoteKind::DeadEnd,
        NoteKind::Stub,
        NoteKind::Short,
        NoteKind::Duplicate,
        NoteKind::Sliver,
        NoteKind::Skipped,
        NoteKind::Warning,
    ] {
        assert!(f.notes.iter().any(|n| n.kind == kind), "{kind:?}");
    }
    assert!(f.notes.iter().all(|n| !n.text.is_empty()));
    assert_eq!(f.notes.iter().filter(|n| n.at.is_some()).count(), 3);
    assert_eq!(f.marks.len(), 4);
    assert!(f.marks.iter().any(|m| m.kind == NoteKind::Sliver));
}

#[test]
fn a_quiet_reply_has_no_notes_and_a_bad_one_is_refused() {
    let quiet = json!({"regions": [], "complete": true});
    let f = Found::parse(&quiet).unwrap();
    assert!(f.notes.is_empty() && f.complete && f.regions.is_empty());
    assert!(Found::parse(&json!({})).is_err());
    assert!(Found::parse(&json!({"regions": [{"outline": []}]})).is_err());
}

#[test]
fn areas_are_on_by_default_and_the_person_can_switch_any() {
    let f = Found::parse(&reply()).unwrap();
    let mut form = CurvesForm::default();
    assert_eq!(form.chosen(&f), vec![0]);
    form.toggle(&f.regions[1]);
    form.toggle(&f.regions[0]);
    assert_eq!(form.chosen(&f), vec![1]);
    form.make = Make::Polyline;
    form.switched.clear();
    assert_eq!(form.chosen(&f), vec![0, 1, 2]);
}

#[test]
fn the_calls_carry_the_form() {
    let f = Found::parse(&reply()).unwrap();
    let mut editor = Editor::default();
    let a = line(&mut editor, (0.0, 0.0), (1000.0, 0.0));
    let ids = vec![a];
    let mut form = CurvesForm {
        tolerance: 2.5,
        style: Some("Pavers".into()),
        delete_source: true,
        ..Default::default()
    };
    let p = form.preview_params(&ids);
    assert_eq!(p["dry_run"], true);
    assert_eq!(p["tolerance"], 2.5);
    assert!(p.get("create").is_none());
    let c = form.commit_params(&ids, &f);
    assert_eq!(c["create"], "hardscape");
    assert_eq!(c["regions"], json!([0]));
    assert_eq!(c["style"], "Pavers");
    assert_eq!(c["delete_source"], true);
    assert!(c.get("dry_run").is_none());
    // A landscape area takes no hardscape style.
    form.make = Make::LandscapeArea;
    assert!(form.commit_params(&ids, &f).get("style").is_none());
    for m in Make::ALL {
        assert!(!m.label().is_empty());
        assert!(!m.id().is_empty());
    }
    for r in [Role::Area, Role::Island, Role::Doubtful] {
        assert!(!r.label().is_empty());
    }
}

#[test]
fn the_dialog_opens_on_shapes_previews_and_makes_the_regions() {
    // Nothing to use: a message.
    let be = Fake::new();
    assert!(CurvesDialog::open(&be).is_err());

    let mut editor = Editor::default();
    let a = line(&mut editor, (0.0, 0.0), (1000.0, 0.0));
    let b = line(&mut editor, (0.0, 0.0), (0.0, 1000.0));
    let mut be = Fake::new().preview(reply());
    be.doc = editor.doc().clone();
    be.selection = vec![a, b];
    let mut d = CurvesDialog::open(&be).unwrap();
    assert_eq!(d.ids, vec![a, b]);
    assert_eq!(d.found.as_ref().unwrap().regions.len(), 3);
    assert_eq!(be.previewed.borrow().len(), 1);
    assert_eq!(be.previewed.borrow()[0].0, "region.from_curves");
    // Nothing changed: no new preview.
    d.refresh(&be);
    assert_eq!(be.previewed.borrow().len(), 1);
    // A new tolerance asks again, and forgets the switches.
    d.form.switched.insert(1, true);
    d.form.tolerance = 3.0;
    assert!(d.stale(&be));
    be.previews.borrow_mut().push_back(Ok(reply()));
    d.refresh(&be);
    assert_eq!(be.previewed.borrow().len(), 2);
    assert!(d.form.switched.is_empty());
    // A tolerance out of range is a message and no call.
    d.form.tolerance = 0.0;
    d.refresh(&be);
    assert!(d.found.is_err());
    assert_eq!(be.previewed.borrow().len(), 2);
    d.form.tolerance = 1.0;
    be.previews.borrow_mut().push_back(Ok(reply()));
    d.refresh(&be);
    // Commit sends the very regions of the preview, and reads what was made.
    be.replies.push_back(Ok(json!({
        "created": [{"region": 0, "id": a, "kind": "hardscape"}],
        "kept_sources": [{"id": b, "reason": "x"}],
    })));
    let made = d.commit(&mut be).unwrap();
    assert_eq!(made.created, vec![a]);
    assert_eq!(made.kept, 1);
    let (method, params) = &be.calls[0];
    assert_eq!(method, "region.from_curves");
    assert_eq!(params["regions"], json!([0]));
    assert_eq!(params["ids"], json!([a, b]));
    // With every region switched off there is nothing to make.
    let found = d.found.clone().unwrap();
    d.form.toggle(&found.regions[0]);
    assert!(d.commit(&mut be).is_err());
    assert_eq!(be.calls.len(), 1);
}
