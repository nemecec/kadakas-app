//! Guides, Hide and Tags in the document (MOD-05): the guide shape, its limit, its undo, hidden
//! objects and faces, classes as tags, and the counted notes of exports.

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::guide::{Guide, MAX_GUIDES};
use kadakas_core::leftout::{LeftOut, prepare, prepare_with};
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
        guide(Guide::line(
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(0.0, 1.0, 0.0),
        )),
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
    ed.execute(&Command::DeleteObjects { ids: vec![id] })
        .unwrap();
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
    let a = add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
    let b = add(
        &mut ed,
        Shape::Line {
            a: DVec2::Y,
            b: DVec2::ONE,
        },
    );
    let out = ed
        .execute(&Command::Group {
            ids: vec![a, b],
            name: None,
        })
        .unwrap();
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
            model(&ed, id)
                .face_area(*a)
                .unwrap()
                .total_cmp(&model(&ed, id).face_area(*b).unwrap())
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
    assert_eq!(
        tris(&ed),
        full - 2,
        "the two triangles of the top face are gone"
    );
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
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
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
    ed.execute(&Command::DeleteClass {
        id: t,
        move_to: None,
    })
    .unwrap();
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
    ed.execute(&Command::DeleteClass {
        id: t,
        move_to: Some(u),
    })
    .unwrap();
    assert_eq!(ed.doc().object(o).unwrap().class, u);
}

#[test]
fn a_class_is_deleted_only_when_it_makes_sense() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees");
    assert!(matches!(
        ed.execute(&Command::DeleteClass {
            id: NONE_CLASS,
            move_to: None
        }),
        Err(CommandError::Invalid(_))
    ));
    assert_eq!(
        ed.execute(&Command::DeleteClass {
            id: ClassId::from_num(999),
            move_to: None
        }),
        Err(CommandError::NoClass(ClassId::from_num(999)))
    );
    assert_eq!(
        ed.execute(&Command::DeleteClass {
            id: t,
            move_to: Some(ClassId::from_num(999))
        }),
        Err(CommandError::NoClass(ClassId::from_num(999)))
    );
    assert!(matches!(
        ed.execute(&Command::DeleteClass {
            id: t,
            move_to: Some(t)
        }),
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
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
            layer: None,
            class,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    let segments = |ed: &Editor| mesh::build(ed.doc()).lines.segment_count();
    assert_eq!(segments(&ed), 2);
    ed.execute(&Command::ModifyClass {
        id: t,
        name: None,
        visible: Some(false),
        attrs: None,
    })
    .unwrap();
    assert_eq!(segments(&ed), 1);
    ed.execute(&Command::ModifyClass {
        id: t,
        name: Some("Plants".into()),
        visible: Some(true),
        attrs: None,
    })
    .unwrap();
    assert_eq!(segments(&ed), 2);
    assert_eq!(ed.doc().class(t).unwrap().name, "Plants");
}

// ---- exports ---------------------------------------------------------------------------------

#[test]
fn what_an_export_leaves_out_is_counted() {
    let mut ed = Editor::with_sequential_ids();
    let id = boxed(&mut ed);
    let line = add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
    );
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
        edit: Edit::SetHidden {
            faces: vec![face],
            edges: vec![edge],
            hidden: true,
        },
    })
    .unwrap();
    ed.execute(&set_hidden_command(ed.doc(), &[line], true).unwrap())
        .unwrap();
    let n = LeftOut::of(ed.doc());
    assert_eq!(
        n,
        LeftOut {
            guides: 2,
            hidden_objects: 1,
            hidden_faces: 1,
            hidden_edges: 1,
            ..LeftOut::default()
        }
    );
    assert_eq!(n.notes().len(), 3);
    // The document for the export has none of them.
    let prepared = prepare(ed.doc());
    let clean = prepared.doc;
    assert_eq!(clean.guide_count(), 0);
    assert!(clean.object(line).is_none());
    let Shape::SurfaceModel(m) = &clean.object(id).unwrap().shape else {
        panic!()
    };
    assert!(m.hidden_faces().is_empty());
    assert_eq!(m.face_count(), model(&ed, id).face_count() - 1);
    // The hidden edge borders a face that is shown (or is loose and goes): what really happened is
    // counted, and a border edge is said to stay.
    let l = prepared.left;
    assert_eq!((l.hidden_faces, l.guides, l.hidden_objects), (1, 2, 1));
    assert_eq!(l.hidden_edges + l.hidden_edges_kept, 1, "{l:?}");
    assert_eq!(l.hidden_faces_kept, 0);
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
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::Y,
            },
            layer: None,
            class: Some(t),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&set_hidden_command(ed.doc(), &[line2], true).unwrap())
        .unwrap();
    ed.execute(&Command::ModifyClass {
        id: t,
        name: None,
        visible: Some(false),
        attrs: None,
    })
    .unwrap();
    assert_eq!(LeftOut::of(ed.doc()).hidden_objects, 1);
}

#[test]
fn a_guide_moves_turns_and_scales_with_the_objects_around_it() {
    use glam::DAffine3;
    let mut ed = Editor::with_sequential_ids();
    let seg = add(
        &mut ed,
        guide(Guide::segment(DVec3::new(100.0, 0.0, 0.0), DVec3::X, 50.0)),
    );
    let pt = add(&mut ed, guide(Guide::point(DVec3::new(10.0, 20.0, 30.0))));
    let line = add(&mut ed, guide(Guide::line(DVec3::ZERO, DVec3::Y)));
    // Twice as large about the origin, then turned a quarter about Z.
    let m = DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2)
        * DAffine3::from_scale(DVec3::splat(2.0));
    let depth = ed.undo_depth();
    ed.execute(&Command::TransformObjects3d {
        ids: vec![seg, pt, line],
        matrix: m.to_cols_array(),
        copy: false,
        count: 1,
    })
    .unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    let g = |id| match &ed.doc().object(id).unwrap().shape {
        Shape::Guide(g) => (**g).clone(),
        _ => panic!(),
    };
    let s = g(seg);
    assert!(
        s.at.distance(DVec3::new(0.0, 200.0, 0.0)) < 1e-9,
        "{:?}",
        s.at
    );
    assert!(s.unit_dir().unwrap().distance(DVec3::Y) < 1e-9);
    assert!(
        (s.length.unwrap() - 100.0).abs() < 1e-9,
        "the length follows the scale"
    );
    assert!(g(pt).at.distance(DVec3::new(-40.0, 20.0, 60.0)) < 1e-9);
    assert!(g(line).unit_dir().unwrap().distance(-DVec3::X) < 1e-9);
    assert_eq!(g(line).length, None);
    // Copies work too, and the original stays.
    ed.execute(&Command::TransformObjects3d {
        ids: vec![pt],
        matrix: DAffine3::from_translation(DVec3::new(5.0, 0.0, 0.0)).to_cols_array(),
        copy: true,
        count: 2,
    })
    .unwrap();
    assert_eq!(ed.doc().guide_count(), 5);
}

#[test]
fn a_hidden_face_that_cannot_be_taken_out_is_counted_as_kept_not_as_left_out() {
    let mut ed = Editor::with_sequential_ids();
    let id = boxed(&mut ed);
    let face = model(&ed, id).faces().next().unwrap().0;
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![face],
            edges: vec![],
            hidden: true,
        },
    })
    .unwrap();
    let refused = |_: &mut SurfaceModel, _: Vec<kadakas_core::surface::FaceId>| {
        Err(kadakas_core::surface::SurfaceError::Unsolved("no".into()))
    };
    let p = prepare_with(ed.doc(), &refused);
    assert_eq!((p.left.hidden_faces, p.left.hidden_faces_kept), (0, 1));
    let Shape::SurfaceModel(m) = &p.doc.object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!(m.hidden_faces(), vec![face], "the face stays in the copy");
    let notes = p.left.notes();
    assert!(
        notes
            .iter()
            .any(|n| n == "Hidden faces that could not be taken out stay in the export: 1"),
        "{notes:?}"
    );
}

#[test]
fn a_hidden_edge_that_no_face_uses_is_taken_out_and_one_on_a_face_is_said_to_stay() {
    let mut ed = Editor::with_sequential_ids();
    let id = add(&mut ed, Shape::SurfaceModel(Box::default()));
    // A loose edge, and a square with a face.
    let sq = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    let mut edges: Vec<[[f64; 3]; 2]> = (0..4).map(|i| [sq[i], sq[(i + 1) % 4]]).collect();
    edges.push([[3000.0, 0.0, 0.0], [4000.0, 0.0, 0.0]]);
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges { edges },
    })
    .unwrap();
    let m = model(&ed, id);
    let loose = m
        .edges()
        .map(|(e, _)| e)
        .find(|e| m.faces_of_edge(*e).is_empty())
        .unwrap();
    let border = m
        .edges()
        .map(|(e, _)| e)
        .find(|e| !m.faces_of_edge(*e).is_empty())
        .unwrap();
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![],
            edges: vec![loose, border],
            hidden: true,
        },
    })
    .unwrap();
    let p = prepare(ed.doc());
    assert_eq!((p.left.hidden_edges, p.left.hidden_edges_kept), (1, 1));
    let Shape::SurfaceModel(copy) = &p.doc.object(id).unwrap().shape else {
        panic!()
    };
    assert!(copy.edge(loose).is_none(), "the loose hidden edge is gone");
    assert!(copy.edge(border).is_some() && copy.edge_hidden(border));
    assert!(p.left.notes().iter().any(|n| n.contains("stay in the geometry")));
}

#[test]
fn a_guide_this_program_cannot_read_stays_unknown_and_is_written_back_unchanged() {
    // A newer build may write a guide in a form that this build cannot read (here a direction as
    // an object). The shape is kept as it is, as for every unknown kind, and only delete works.
    let mut ed = Editor::with_sequential_ids();
    let id = add(&mut ed, guide(Guide::point(DVec3::ZERO)));
    let mut v = serde_json::to_value(ed.doc()).unwrap();
    let future = serde_json::json!({
        "kind": "guide",
        "at": [1, 2, 3],
        "dir": {"x": 0, "y": 1, "z": 0},
        "curve": "arc"
    });
    v["objects"][0]["shape"] = future.clone();
    let doc = Document::from_value(v.clone()).unwrap();
    let o = doc.object(id).unwrap();
    assert!(matches!(o.shape, Shape::Unknown { .. }), "{:?}", o.shape);
    assert_eq!(o.shape.kind(), "unknown");
    assert_eq!(serde_json::to_value(&doc).unwrap(), v, "written back unchanged");
    let mut ed = Editor::new(doc);
    assert_eq!(ed.doc().guide_count(), 0, "it is not a guide for this program");
    assert!(
        ed.execute(&Command::SetHidden {
            ids: vec![id],
            hidden: true
        })
        .is_err(),
        "it cannot be edited"
    );
    ed.execute(&Command::DeleteObjects { ids: vec![id] }).unwrap();
    // A guide that this build can read is a guide.
    let mut ok = v.clone();
    ok["objects"][0]["shape"] = serde_json::json!({"kind": "guide", "at": [1, 2, 3]});
    let doc = Document::from_value(ok).unwrap();
    assert_eq!(doc.guide_count(), 1);
}

#[test]
fn the_hide_mark_is_a_reserved_data_key_that_set_data_cannot_write() {
    let mut ed = Editor::with_sequential_ids();
    let o = add(&mut ed, Shape::Line { a: DVec2::ZERO, b: DVec2::X });
    for set in [true, false] {
        let cmd = if set {
            Command::SetData {
                id: o,
                set: [("kadakas.hidden".to_string(), serde_json::json!(true))].into(),
                remove: vec![],
            }
        } else {
            Command::SetData {
                id: o,
                set: Default::default(),
                remove: vec!["kadakas.hidden".into()],
            }
        };
        let e = ed.execute(&cmd).unwrap_err().to_string();
        assert!(e.contains("reserved") && e.contains("kadakas."), "{e}");
    }
    assert!(!is_hidden(ed.doc().object(o).unwrap()));
    // The owner of the key writes it. Reading the data is free.
    ed.execute(&set_hidden_command(ed.doc(), &[o], true).unwrap()).unwrap();
    assert_eq!(
        ed.doc().object(o).unwrap().data.get("kadakas.hidden"),
        Some(&serde_json::json!(true))
    );
    // Other keys still work.
    ed.execute(&Command::SetData {
        id: o,
        set: [("note".to_string(), serde_json::json!("x"))].into(),
        remove: vec![],
    })
    .unwrap();
    // The command refuses an empty list and a missing object.
    assert!(ed.execute(&Command::SetHidden { ids: vec![], hidden: true }).is_err());
    assert!(
        ed.execute(&Command::SetHidden {
            ids: vec![ObjectId(kadakas_core::uid::Uid::from_num(9999))],
            hidden: true
        })
        .is_err()
    );
}
