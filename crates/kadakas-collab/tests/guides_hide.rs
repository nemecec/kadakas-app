//! Guides, Hide and tags (MOD-05) in the collaboration: a guide is an object with one `shape`
//! property, the Hide flag is a key of `data`, a hidden face is inside the shape of its model, and
//! the deletion of a tag is a change of classes and of the class of objects. All of it comes back
//! through `materialize` like any other object.

use kadakas_collab::docsync::replica_of;
use kadakas_collab::validate::validate_entity;
use kadakas_collab::{Kind, Op, Target, Writer, materialize};
use kadakas_core::command::{Command, Editor};
use kadakas_core::guide::Guide;
use kadakas_core::model::{Attributes, ClassId, Document, NONE_CLASS, ObjectId, Shape};
use kadakas_core::surface::Edit;
use kadakas_core::uid::Uid;
use kadakas_core::visibility::{is_hidden, set_hidden_command};
use kadakas_core::{DVec2, DVec3};

fn add(ed: &mut Editor, shape: Shape, class: Option<ClassId>) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn guide(g: Guide) -> Shape {
    Shape::Guide(Box::new(g))
}

#[test]
fn guides_round_trip_through_the_replica() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(
        &mut ed,
        guide(Guide::line(DVec3::new(1.0, 2.0, 3.0), DVec3::Y)),
        None,
    );
    let b = add(
        &mut ed,
        guide(Guide::segment(DVec3::ZERO, DVec3::X, 300.0)),
        None,
    );
    let c = add(
        &mut ed,
        guide(Guide::point(DVec3::new(5.0, 5.0, 5.0))),
        None,
    );
    let m = materialize(&replica_of(ed.doc()), &Document::default());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    for id in [a, b, c] {
        assert_eq!(
            m.doc.object(id).unwrap().shape,
            ed.doc().object(id).unwrap().shape
        );
    }
    assert_eq!(m.doc.guide_count(), 3);
}

#[test]
fn a_guide_of_another_user_arrives_and_a_broken_one_is_refused() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, guide(Guide::point(DVec3::ZERO)), None);
    let doc = ed.doc().clone();
    let mut replica = replica_of(&doc);
    let moved = guide(Guide::point(DVec3::new(10.0, 0.0, 0.0)));
    replica.apply(
        Writer {
            client: Uid::from_num(7),
            op: 1,
        },
        &Op::Set {
            target: Target::new(Kind::Object, a.0),
            prop: "shape".into(),
            value: serde_json::to_value(&moved).unwrap(),
        },
    );
    let m = materialize(&replica, &doc);
    assert_eq!(m.doc.object(a).unwrap().shape, moved);
    // A guide whose direction has no length is not a valid object: the check refuses it.
    let props = |shape: serde_json::Value| {
        let o = doc.object(a).unwrap();
        let mut p = serde_json::Map::new();
        p.insert("layer".into(), serde_json::json!(o.layer));
        p.insert("class".into(), serde_json::json!(o.class));
        p.insert("shape".into(), shape);
        p
    };
    let good = serde_json::to_value(guide(Guide::line(DVec3::ZERO, DVec3::X))).unwrap();
    assert!(validate_entity(Kind::Object, Uid::from_num(900), &props(good)).is_ok());
    for bad in [
        serde_json::json!({"kind": "guide", "at": [0, 0, 0], "dir": [0, 0, 0]}),
        serde_json::json!({"kind": "guide", "at": [0, 0, 0], "length": 5}),
        serde_json::json!({"kind": "guide", "at": [5e9, 0, 0]}),
    ] {
        let r = validate_entity(Kind::Object, Uid::from_num(900), &props(bad.clone()));
        assert!(r.is_err(), "{bad}: {r:?}");
    }
}

#[test]
fn the_hide_flag_and_a_hidden_face_sync_like_other_data() {
    let mut ed = Editor::with_sequential_ids();
    let line = add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
        None,
    );
    let model = add(&mut ed, Shape::SurfaceModel(Box::default()), None);
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    ed.execute(&Command::EditSurface {
        id: model,
        edit: Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let face = match &ed.doc().object(model).unwrap().shape {
        Shape::SurfaceModel(m) => m.faces().next().unwrap().0,
        _ => panic!(),
    };
    ed.execute(&Command::EditSurface {
        id: model,
        edit: Edit::SetHidden {
            faces: vec![face],
            edges: vec![],
            hidden: true,
        },
    })
    .unwrap();
    ed.execute(&set_hidden_command(ed.doc(), &[line], true).unwrap())
        .unwrap();
    let m = materialize(&replica_of(ed.doc()), &Document::default());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    assert!(is_hidden(m.doc.object(line).unwrap()));
    match &m.doc.object(model).unwrap().shape {
        Shape::SurfaceModel(s) => assert_eq!(s.hidden_faces(), vec![face]),
        _ => panic!(),
    }
}

#[test]
fn deleting_a_tag_comes_through_as_a_change_of_classes_and_objects() {
    let mut ed = Editor::with_sequential_ids();
    let tag = ClassId(
        ed.execute(&Command::AddClass {
            name: "Trees".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let o = add(
        &mut ed,
        Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        },
        Some(tag),
    );
    let before = ed.doc().clone();
    ed.execute(&Command::DeleteClass {
        id: tag,
        move_to: None,
    })
    .unwrap();
    // The other replica had the tag. The deletion arrives as the ops of a client: the object
    // gets the default class and the class goes.
    let mut replica = replica_of(&before);
    replica.apply(
        Writer {
            client: Uid::from_num(9),
            op: 1,
        },
        &Op::Set {
            target: Target::new(Kind::Object, o.0),
            prop: "class".into(),
            value: serde_json::to_value(NONE_CLASS).unwrap(),
        },
    );
    replica.apply(
        Writer {
            client: Uid::from_num(9),
            op: 2,
        },
        &Op::Delete {
            target: Target::new(Kind::Class, tag.0),
        },
    );
    let m = materialize(&replica, &before);
    assert!(m.doc.class(tag).is_none(), "{:?}", m.quarantined);
    assert_eq!(m.doc.object(o).unwrap().class, NONE_CLASS);
}
