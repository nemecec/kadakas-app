use super::*;
use crate::paving::fake::Fake;
use crate::paving::lot;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::layout::plan::{Plan, SplitLine};
use serde_json::json;

fn def(side: Side, edge: Option<&str>) -> CourseDef {
    CourseDef {
        name: "Kerb".into(),
        source: None,
        edge: edge.map(str::to_string),
        path: corners(&[DVec2::ZERO, DVec2::new(1000.0, 0.0)]),
        closed: true,
        side,
        offset: 20.0,
        along: 100.0,
        depth: 200.0,
        gap: Some(5.0),
        corner: Corner::Butt,
        unit: "200x100".into(),
    }
}

fn store(editor: &mut Editor, id: ObjectId, plan: &Plan) {
    editor
        .execute(&Command::SetData {
            id,
            set: [(plan::KEY.to_string(), serde_json::to_value(plan).unwrap())]
                .into_iter()
                .collect(),
            remove: Vec::new(),
        })
        .unwrap();
}

#[test]
fn a_new_course_has_the_stone_of_the_style_standing_on_its_end() {
    let (editor, id) = lot::new();
    let doc = editor.doc();
    assert_eq!(stone(doc, id), Some((200.0, 100.0, 5.0)));
    let f = CourseForm::new(doc, id);
    assert!(!f.existing);
    // Soldier: the short side runs along the row.
    assert_eq!((f.unit_length, f.unit_width), (100.0, 200.0));
    assert_eq!(f.orientation, Orientation::Soldier);
    let p = f.params(doc, id);
    assert_eq!(p["along"], "outline");
    assert_eq!(p["side"], "inside");
    assert_eq!(p["unit_length"], 100.0);
    assert_eq!(p["unit_width"], 200.0);
    assert_eq!(p["corner"], "miter");
    assert_eq!(p["id"], json!(id));
    assert!(
        p.get("unit").is_none(),
        "the stone of the style keeps its unit type"
    );
    // A stretcher swaps the sizes.
    let mut g = f.clone();
    g.set_orientation(Orientation::Stretcher);
    assert_eq!((g.unit_length, g.unit_width), (200.0, 100.0));
    g.set_orientation(Orientation::Stretcher);
    assert_eq!((g.unit_length, g.unit_width), (200.0, 100.0));
    // Another stone is a unit type of its own.
    g.unit_width = 120.0;
    assert_eq!(g.params(doc, id)["unit"], "200x120");
}

#[test]
fn the_placing_follows_the_edge_it_is_on() {
    let (editor, id) = lot::new();
    let doc = editor.doc();
    let mut f = CourseForm::new(doc, id);
    f.along = Along::Hole(2);
    f.placing = Placing::OnPaving;
    assert_eq!(f.params(doc, id)["side"], "outside");
    f.placing = Placing::Opposite;
    assert_eq!(f.params(doc, id)["side"], "inside");
    f.placing = Placing::Centered;
    assert_eq!(f.params(doc, id)["side"], "center");
    assert_eq!(f.params(doc, id)["along"], "hole:2");
    // And back from the definition.
    for (side, along, want) in [
        (Side::Left, Along::Outline, Placing::OnPaving),
        (Side::Right, Along::Outline, Placing::Opposite),
        (Side::Right, Along::Hole(0), Placing::OnPaving),
        (Side::Left, Along::Hole(0), Placing::Opposite),
        (Side::Center, Along::Outline, Placing::Centered),
    ] {
        assert_eq!(Placing::of(side, along), want);
    }
    for p in Placing::ALL {
        assert!(!p.label().is_empty());
    }
    for o in Orientation::ALL {
        assert!(!o.label().is_empty() && !o.id().is_empty());
    }
    for c in [Corner::Miter, Corner::Butt, Corner::CornerUnit] {
        assert!(!corner_label(c).is_empty());
    }
    assert_eq!(Along::parse("hole:3"), Some(Along::Hole(3)));
    assert_eq!(Along::parse("outline"), Some(Along::Outline));
    assert_eq!(Along::parse("hole:x"), None);
    assert_eq!(Along::Hole(0).id(), "hole:0");
    assert!(!Along::Hole(0).label().is_empty() && !Along::Outline.label().is_empty());
}

#[test]
fn only_a_course_along_an_edge_has_a_form() {
    assert!(CourseForm::from_def(&def(Side::Left, None), 5.0).is_none());
    assert!(CourseForm::from_def(&def(Side::Left, Some("nonsense")), 5.0).is_none());
    let f = CourseForm::from_def(&def(Side::Left, Some("outline")), 5.0).unwrap();
    assert!(f.existing);
    assert_eq!(f.placing, Placing::OnPaving);
    assert_eq!(f.orientation, Orientation::Soldier);
    assert_eq!(
        (f.unit_length, f.unit_width, f.offset),
        (100.0, 200.0, 20.0)
    );
    assert_eq!(f.corner, Corner::Butt);
}

#[test]
fn the_edges_are_the_outline_and_the_holes() {
    let (mut editor, id) = lot::new();
    assert_eq!(CourseForm::edges(editor.doc(), id), vec![Along::Outline]);
    editor
        .execute(&Command::SetHardscape {
            ids: vec![id],
            patch: kadakas_core::hardscape::HardscapePatch {
                holes: Some(vec![corners(&[
                    DVec2::new(1000.0, 1000.0),
                    DVec2::new(1500.0, 1000.0),
                    DVec2::new(1500.0, 1500.0),
                ])]),
                ..Default::default()
            },
        })
        .unwrap();
    assert_eq!(
        CourseForm::edges(editor.doc(), id),
        vec![Along::Outline, Along::Hole(0)]
    );
    assert!(
        CourseForm::edges(editor.doc(), ObjectId(kadakas_core::uid::Uid::default())).is_empty()
    );
}

#[test]
fn courses_are_listed_with_their_counts_and_found_by_name() {
    let (mut editor, id) = lot::new();
    assert!(rows(editor.doc(), id, 100_000).unwrap().is_empty());
    assert!(def_of(editor.doc(), id, "Kerb").is_none());
    let plan = Plan {
        courses: vec![CourseDef {
            path: corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(4000.0, 0.0),
                DVec2::new(4000.0, 3000.0),
                DVec2::new(0.0, 3000.0),
            ]),
            ..def(Side::Left, Some("outline"))
        }],
        ..Default::default()
    };
    store(&mut editor, id, &plan);
    let doc = editor.doc();
    let r = rows(doc, id, 100_000).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].name, "Kerb");
    assert!(r[0].units > 0 && r[0].editable);
    assert!(r[0].length_m > 10.0);
    assert!(def_of(doc, id, "kerb").is_some());
    // The editor opens on it, and a missing course is refused.
    let e = CourseEditor::edit(doc, id, "Kerb").unwrap();
    assert!(e.form.existing);
    assert!(CourseEditor::edit(doc, id, "Nope").is_err());
    // The outlines of the units are the pieces of the laid course.
    let o = doc.object(id).unwrap();
    let planned = plan::groups_of(doc, o, layout::TOLERANCE, 100_000)
        .unwrap()
        .unwrap();
    let units = unit_outlines(&planned, "KERB");
    assert_eq!(units.len(), r[0].units);
    assert!(unit_outlines(&planned, "other").is_empty());
}

#[test]
fn the_editor_refuses_what_has_no_pattern_and_previews_through_the_api() {
    let mut be = Fake::new();
    assert!(
        CourseEditor::new_course(be.document(), ObjectId(kadakas_core::uid::Uid::default()))
            .is_err()
    );
    let (mut editor, id) = lot::new();
    // A split line is also a plan; it does not matter for a course.
    let plan = Plan {
        splits: vec![SplitLine {
            source: None,
            points: corners(&[DVec2::new(2000.0, -500.0), DVec2::new(2000.0, 3500.0)]),
        }],
        ..Default::default()
    };
    store(&mut editor, id, &plan);
    be.doc = editor.doc().clone();
    let mut e = CourseEditor::new_course(&be.doc, id).unwrap();
    assert_eq!(e.form.name, "Course 1");
    // The preview asks hardscape.course once for a form, again after a change.
    be.previews
        .borrow_mut()
        .push_back(Ok(json!({"courses": [], "notes": ["joint wider"]})));
    e.refresh(&be);
    e.refresh(&be);
    assert_eq!(be.previewed.borrow().len(), 1);
    assert_eq!(be.previewed.borrow()[0].0, "hardscape.course");
    // The copy of the fake has no course, so the row is missing and the units are empty.
    let p = e.preview.as_ref().unwrap();
    assert!(p.row.is_none() && p.units.is_empty());
    assert_eq!(p.notes, vec!["joint wider".to_string()]);
    e.form.name = "  ".into();
    e.refresh(&be);
    assert!(e.preview.is_err());
    assert_eq!(be.previewed.borrow().len(), 1);
    // Apply and remove are the API calls.
    e.form.name = "Kerb".into();
    be.replies.push_back(Ok(json!({})));
    e.apply(&mut be).unwrap();
    be.replies.push_back(Ok(json!({})));
    remove(&mut be, id, "Kerb").unwrap();
    assert_eq!(be.calls[0].0, "hardscape.course");
    assert_eq!(be.calls[0].1["name"], "Kerb");
    assert_eq!(be.calls[1].0, "hardscape.course_remove");
    assert_eq!(be.calls[1].1["names"], json!(["Kerb"]));
}
