//! Guides, Hide and Tags in the document (MOD-05): the guide shape, its limit, its undo, hidden
//! objects and faces, classes as tags, and the counted notes of exports.

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::guide::{Guide, MAX_GUIDES};
use kadakas_core::leftout::{LeftOut, without_hidden};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::surface::{Edit, SurfaceModel};
use kadakas_core::visibility::{hidden_objects, is_hidden, set_hidden_command};

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn guide(g: Guide) -> Shape {
    Shape::Guide(Box::new(g))
}

fn boxed(ed: &mut Editor) -> ObjectId {
    let id = add(ed, Shape::SurfaceModel(Box::default()));
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let face = model(ed, id).faces().next().unwrap().0;
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::PushPull {
            face,
            distance: 500.0,
        },
    })
    .unwrap();
    id
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        _ => panic!("not a model"),
    }
}

// ---- guides ----------------------------------------------------------------------------------

#[test]
fn a_guide_is_an_object_that_round_trips_through_the_file() {
    let mut ed = Editor::with_sequential_ids();
    let line = add(
        &mut ed,
        guide(Guide::line(DVec3::new(1.0, 2.0, 3.0), DVec3::new(0.0, 1.0, 0.0))),
    );
    let seg = add(&mut ed, guide(Guide::segment(DVec3::ZERO, DVec3::X, 250.0)));
    let pt = add(&mut ed, guide(Guide::point(DVec3::new(5.0, 6.0, 7.0))));
    assert_eq!(ed.doc().guide_count(), 3);
    assert_eq!(ed.doc().object(line).unwrap().shape.kind(), "guide");
    let text = serde_json::to_string(ed.doc()).unwrap();
    assert!(text.contains(r#""kind":"guide""#));
    let back = Document::from_json(&text).unwrap();
    assert_eq!(&back, ed.doc());
    // A point has no direction and no length in the file.
    let v = serde_json::to_value(&ed.doc().object(pt).unwrap().shape).unwrap();
    assert!(v.get("dir").is_none() && v.get("length").is_none());
    let v = serde_json::to_value(&ed.doc().object(seg).unwrap().shape).unwrap();
    assert_eq!(v["length"], 250.0);
}

#[test]
fn a_guide_is_one_undo_step_and_goes_with_delete() {
    let mut ed = Editor::with_sequential_ids();
    let depth = ed.undo_depth();
    let id = add(&mut ed, guide(Guide::line(DVec3::ZERO, DVec3::X)));
    assert_eq!(ed.undo_depth(), depth + 1);
    assert!(ed.undo());
    assert_eq!(ed.doc().guide_count(), 0);
    assert!(ed.redo());
    assert!(ed.doc().object(id).is_some());
    ed.execute(&Command::DeleteObjects { ids: vec![id] }).unwrap();
    assert_eq!(ed.doc().guide_count(), 0);
    assert!(ed.undo());
    assert_eq!(ed.doc().guide_count(), 1);
}

#[test]
fn a_bad_guide_is_refused_with_a_reason() {
    let mut ed = Editor::with_sequential_ids();
    for bad in [
        Guide::line(DVec3::ZERO, DVec3::ZERO),
        Guide::point(DVec3::NAN),
        Guide::segment(DVec3::ZERO, DVec3::X, -5.0),
    ] {
        let r = ed.execute(&Command::AddObject {
            shape: guide(bad),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        });
        assert!(matches!(r, Err(CommandError::Invalid(_))), "{r:?}");
    }
    assert_eq!(ed.doc().guide_count(), 0);
}

#[test]
fn a_guide_does_not_go_on_a_sheet_layer() {
    let mut ed = Editor::with_sequential_ids();
    let sheet = LayerId(
        ed.execute(&Command::AddLayer {
            name: "Sheet".into(),
            elevation: 0.0,
            scale: None,
            kind: Some(LayerKind::Sheet),
        })
        .unwrap()
        .created[0],
    );
    let r = ed.execute(&Command::AddObject {
        shape: guide(Guide::point(DVec3::ZERO)),
        layer: Some(sheet),
        class: None,
        attrs: Attributes::default(),
    });
    assert!(r.is_err());
}

#[test]
fn a_document_holds_at_most_ten_thousand_guides() {
    let mut ed = Editor::with_sequential_ids();
    let one = |i: usize| Command::AddObject {
        shape: guide(Guide::point(DVec3::new(i as f64, 0.0, 0.0))),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    };
    ed.execute(&Command::Batch {
        commands: (0..MAX_GUIDES).map(one).collect(),
    })
    .unwrap();
    assert_eq!(ed.doc().guide_count(), MAX_GUIDES);
    let depth = ed.undo_depth();
    let err = ed.execute(&one(MAX_GUIDES)).unwrap_err();
    let CommandError::Invalid(m) = err else {
        panic!("{err:?}")
    };
    assert!(m.contains("at most 10000 guides"), "{m}");
    assert_eq!(ed.doc().guide_count(), MAX_GUIDES, "nothing changed");
    assert_eq!(ed.undo_depth(), depth);
    // A batch over the limit changes nothing either.
    assert!(
        ed.execute(&Command::Batch {
            commands: vec![one(1), one(2)]
        })
        .is_err()
    );
    assert_eq!(ed.doc().guide_count(), MAX_GUIDES);
    // Delete all guides in one step, and then there is room again.
    let ids = ed.doc().guide_ids();
    ed.execute(&Command::DeleteObjects { ids }).unwrap();
    assert_eq!(ed.doc().guide_count(), 0);
    ed.execute(&one(0)).unwrap();
}

#[test]
fn a_guide_is_not_drawn_and_does_not_change_the_extent() {
    let mut ed = Editor::with_sequential_ids();
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::new(1000.0, 0.0),
        },
    );
    let before = mesh::build(ed.doc());
    add(
        &mut ed,
        guide(Guide::line(DVec3::new(9e6, 9e6, 0.0), DVec3::X)),
    );
    add(&mut ed, guide(Guide::point(DVec3::new(-9e6, 5.0, 0.0))));
    let after = mesh::build(ed.doc());
    assert_eq!(before.lines.segment_count(), after.lines.segment_count());
    assert_eq!(before.mesh.triangle_count(), after.mesh.triangle_count());
    assert_eq!(before.bounds, after.bounds);
    let svg = kadakas_core::svg::render(ed.doc(), 800);
    assert!(!svg.contains("guide"), "{svg}");
}

#[test]
fn a_guide_moves_in_the_plan_and_follows_the_visibility_of_its_class() {
    let mut ed = Editor::with_sequential_ids();
    let tag = ClassId(
        ed.execute(&Command::AddClass {
            name: "Setout".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: guide(Guide::point(DVec3::new(1.0, 2.0, 3.0))),
            layer: None,
            class: Some(tag),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta: DVec2::new(10.0, 20.0),
    })
    .unwrap();
    let Shape::Guide(g) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!(g.at, DVec3::new(11.0, 22.0, 3.0), "z stays");
    assert!(ed.doc().is_visible(ed.doc().object(id).unwrap()));
    ed.execute(&Command::ModifyClass {
        id: tag,
        name: None,
        visible: Some(false),
        attrs: None,
    })
    .unwrap();
    assert!(
        !ed.doc().is_visible(ed.doc().object(id).unwrap()),
        "a hidden tag hides the guide"
    );
}

// ---- hidden objects --------------------------------------------------------------------------

#[test]
fn hide_is_one_undo_step_and_a_hidden_object_is_not_drawn() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X * 100.0,
        },
    );
    let b = add(
        &mut ed,
        Shape::Line {
            a: DVec2::Y * 50.0,
            b: DVec2::new(100.0, 50.0),
        },
    );
    let segments = |ed: &Editor| mesh::build(ed.doc()).lines.segment_count();
    assert_eq!(segments(&ed), 2);
    let depth = ed.undo_depth();
    let cmd = set_hidden_command(ed.doc(), &[a, b], true).unwrap();
    ed.execute(&cmd).unwrap();
    assert_eq!(ed.undo_depth(), depth + 1, "one step for both");
    assert!(is_hidden(ed.doc().object(a).unwrap()));
    assert_eq!(hidden_objects(ed.doc()), vec![a, b]);
    assert_eq!(segments(&ed), 0);
    assert!(!ed.doc().is_visible(ed.doc().object(a).unwrap()));
    assert!(ed.doc().is_visible_with_hidden(ed.doc().object(a).unwrap()));
    assert!(!ed.doc().is_pickable(ed.doc().object(a).unwrap()));
    // Nothing to do when they are hidden already.
    assert!(set_hidden_command(ed.doc(), &[a, b], true).is_none());
    // The flag survives the file.
    let back = Document::from_json(&serde_json::to_string(ed.doc()).unwrap()).unwrap();
    assert!(is_hidden(back.object(b).unwrap()));
    // Undo shows them, Unhide does the same in a step of its own.
    assert!(ed.undo());
    assert_eq!(segments(&ed), 2);
    ed.redo();
    let cmd = set_hidden_command(ed.doc(), &[a], false).unwrap();
    ed.execute(&cmd).unwrap();
    assert_eq!(segments(&ed), 1);
    assert_eq!(hidden_objects(ed.doc()), vec![b]);
}

#[test]
fn hiding_a_group_hides_its_members() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, Shape::Line { a: DVec2::ZERO, b: DVec2::X });
    let b = add(&mut ed, Shape::Line { a: DVec2::Y, b: DVec2::ONE });
    let out = ed.execute(&Command::Group { ids: vec![a, b], name: None }).unwrap();
    let group = ObjectId(out.created[0]);
    let cmd = set_hidden_command(ed.doc(), &[group], true).unwrap();
    ed.execute(&cmd).unwrap();
    assert!(is_hidden(ed.doc().object(a).unwrap()));
    assert!(is_hidden(ed.doc().object(b).unwrap()));
    assert!(is_hidden(ed.doc().object(group).unwrap()));
    assert_eq!(mesh::build(ed.doc()).lines.segment_count(), 0);
    // A group is not counted as an object that an export leaves out: its members are.
    assert_eq!(LeftOut::of(ed.doc()).hidden_objects, 2);
}

#[test]
fn a_hidden_face_is_not_in_the_3d_scene() {
    let mut ed = Editor::with_sequential_ids();
    let id = boxed(&mut ed);
    let tris = |ed: &Editor| mesh::build(ed.doc()).mesh.triangle_count();
    let full = tris(&ed);
    let lines = mesh::build(ed.doc()).lines.segment_count();
    let top = model(&ed, id)
        .faces()
        .filter(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(f, _)| f)
        .max_by(|a, b| {
            model(&ed, id).face_area(*a).unwrap().total_cmp(&model(&ed, id).face_area(*b).unwrap())
        })
        .unwrap();
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![top],
            edges: vec![],
            hidden: true,
        },
    })
    .unwrap();
    assert_eq!(tris(&ed), full - 2, "the two triangles of the top face are gone");
    assert_eq!(
        mesh::build(ed.doc()).lines.segment_count(),
        lines,
        "its edges stay drawn"
    );
    // A hidden edge is not drawn as a line.
    let edge = model(&ed, id).edges().next().unwrap().0;
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![],
            edges: vec![edge],
            hidden: true,
        },
    })
    .unwrap();
    assert_eq!(mesh::build(ed.doc()).lines.segment_count(), lines - 1);
    // Undo twice brings both back.
    ed.undo();
    ed.undo();
    assert_eq!(tris(&ed), full);
    assert_eq!(mesh::build(ed.doc()).lines.segment_count(), lines);
}

// ---- tags ------------------------------------------------------------------------------------

fn tag(ed: &mut Editor, name: &str) -> ClassId {
    ClassId(
        ed.execute(&Command::AddClass {
            name: name.into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

#[test]
fn deleting_a_class_moves_its_objects_to_the_default_class() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    let o = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Line { a: DVec2::ZERO, b: DVec2::X },
            layer: None,
            class: Some(t),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetActive {
        layer: None,
        class: Some(t),
    })
    .unwrap();
    let depth = ed.undo_depth();
    ed.execute(&Command::DeleteClass { id: t, move_to: None }).unwrap();
    assert_eq!(ed.undo_depth(), depth + 1, "one step");
    assert!(ed.doc().class(t).is_none());
    assert_eq!(ed.doc().object(o).unwrap().class, NONE_CLASS);
    assert_eq!(ed.doc().active_class, NONE_CLASS);
    assert!(ed.undo());
    assert!(ed.doc().class(t).is_some());
    assert_eq!(ed.doc().object(o).unwrap().class, t);
    assert_eq!(ed.doc().active_class, t);
    // To another class on request.
    let u = tag(&mut ed, "Walls");
    ed.execute(&Command::DeleteClass { id: t, move_to: Some(u) }).unwrap();
    assert_eq!(ed.doc().object(o).unwrap().class, u);
}

#[test]
fn a_class_is_deleted_only_when_it_makes_sense() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    assert!(matches!(
        ed.execute(&Command::DeleteClass { id: NONE_CLASS, move_to: None }),
        Err(CommandError::Invalid(_))
    ));
    assert_eq!(
        ed.execute(&Command::DeleteClass { id: ClassId::from_num(999), move_to: None }),
        Err(CommandError::NoClass(ClassId::from_num(999)))
    );
    assert_eq!(
        ed.execute(&Command::DeleteClass { id: t, move_to: Some(ClassId::from_num(999)) }),
        Err(CommandError::NoClass(ClassId::from_num(999)))
    );
    assert!(matches!(
        ed.execute(&Command::DeleteClass { id: t, move_to: Some(t) }),
        Err(CommandError::Invalid(_))
    ));
    assert!(ed.doc().class(t).is_some());
}

#[test]
fn a_hidden_tag_hides_its_objects_in_the_scene() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    for class in [None, Some(t)] {
        ed.execute(&Command::AddObject {
            shape: Shape::Line { a: DVec2::ZERO, b: DVec2::X },
            layer: None,
            class,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    let segments = |ed: &Editor| mesh::build(ed.doc()).lines.segment_count();
    assert_eq!(segments(&ed), 2);
    ed.execute(&Command::ModifyClass { id: t, name: None, visible: Some(false), attrs: None })
        .unwrap();
    assert_eq!(segments(&ed), 1);
    ed.execute(&Command::ModifyClass { id: t, name: Some("Plants".into()), visible: Some(true), attrs: None })
        .unwrap();
    assert_eq!(segments(&ed), 2);
    assert_eq!(ed.doc().class(t).unwrap().name, "Plants");
}

// ---- exports ---------------------------------------------------------------------------------

#[test]
fn what_an_export_leaves_out_is_counted() {
    let mut ed = Editor::with_sequential_ids();
    let id = boxed(&mut ed);
    let line = add(&mut ed, Shape::Line { a: DVec2::ZERO, b: DVec2::X });
    add(&mut ed, guide(Guide::line(DVec3::ZERO, DVec3::X)));
    add(&mut ed, guide(Guide::point(DVec3::ZERO)));
    assert_eq!(LeftOut::of(ed.doc()).guides, 2);
    assert_eq!(
        LeftOut::of(ed.doc()).notes(),
        vec!["2 guides left out: construction geometry"]
    );
    let face = model(&ed, id).faces().next().unwrap().0;
    let edge = model(&ed, id).edges().next().unwrap().0;
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden { faces: vec![face], edges: vec![edge], hidden: true },
    })
    .unwrap();
    ed.execute(&set_hidden_command(ed.doc(), &[line], true).unwrap()).unwrap();
    let n = LeftOut::of(ed.doc());
    assert_eq!(
        n,
        LeftOut { guides: 2, hidden_objects: 1, hidden_faces: 1, hidden_edges: 1 }
    );
    assert_eq!(n.notes().len(), 3);
    // The document for the export has none of them.
    let clean = without_hidden(ed.doc());
    assert_eq!(clean.guide_count(), 0);
    assert!(clean.object(line).is_none());
    let Shape::SurfaceModel(m) = &clean.object(id).unwrap().shape else {
        panic!()
    };
    assert!(m.hidden_faces().is_empty());
    assert_eq!(m.face_count(), model(&ed, id).face_count() - 1);
    // A hidden edge is only a mark on the border of faces: the export draws no line for it.
    assert_eq!(
        LeftOut::of(&clean),
        LeftOut { hidden_edges: 1, ..LeftOut::default() }
    );
    // The document itself is not changed.
    assert_eq!(ed.doc().guide_count(), 2);
    // Objects on a hidden tag are not counted: the exports leave them out by the rules of tags.
    let t = tag(&mut ed, "Off");
    ed.execute(&Command::AddObject {
        shape: guide(Guide::point(DVec3::X)),
        layer: None,
        class: Some(t),
        attrs: Attributes::default(),
    })
    .unwrap();
    assert_eq!(LeftOut::of(ed.doc()).guides, 3, "every guide counts");
    let line2 = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Line { a: DVec2::ZERO, b: DVec2::Y },
            layer: None,
            class: Some(t),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&set_hidden_command(ed.doc(), &[line2], true).unwrap()).unwrap();
    ed.execute(&Command::ModifyClass { id: t, name: None, visible: Some(false), attrs: None })
        .unwrap();
    assert_eq!(LeftOut::of(ed.doc()).hidden_objects, 1);
}
