//! Surface models (MOD-06) in the collaboration: the shape is one property, so an edit syncs as
//! one `set`, comes back through `materialize`, and a broken model is refused by the check.

use kadakas_collab::docsync::replica_of;
use kadakas_collab::validate::validate_entity;
use kadakas_collab::{Kind, Op, Target, Writer, materialize};
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::surface::{Edit, SurfaceModel};
use kadakas_core::uid::Uid;
use serde_json::json;

fn square(x: f64) -> Edit {
    let p = [
        [x, 0.0, 0.0],
        [x + 1000.0, 0.0, 0.0],
        [x + 1000.0, 1000.0, 0.0],
        [x, 1000.0, 0.0],
    ];
    Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    }
}

fn document() -> (Document, ObjectId) {
    let mut ed = Editor::with_sequential_ids();
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let id = ObjectId(out.created[0]);
    ed.execute(&Command::EditSurface {
        id,
        edit: square(0.0),
    })
    .unwrap();
    (ed.doc().clone(), id)
}

#[test]
fn a_surface_model_round_trips_through_the_replica() {
    let (doc, id) = document();
    let m = materialize(&replica_of(&doc), &Document::default());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    assert_eq!(
        m.doc.object(id).unwrap().shape,
        doc.object(id).unwrap().shape
    );
}

#[test]
fn an_edit_of_another_user_arrives_as_one_shape_property() {
    let (doc, id) = document();
    let Shape::SurfaceModel(model) = &doc.object(id).unwrap().shape else {
        panic!()
    };
    let mut theirs: SurfaceModel = (**model).clone();
    theirs.apply(&square(1000.0)).unwrap();
    let mut replica = replica_of(&doc);
    replica.apply(
        Writer {
            client: Uid::from_num(7),
            op: 1,
        },
        &Op::Set {
            target: Target::new(Kind::Object, id.0),
            prop: "shape".into(),
            value: serde_json::to_value(Shape::SurfaceModel(Box::new(theirs.clone()))).unwrap(),
        },
    );
    let m = materialize(&replica, &doc);
    let Shape::SurfaceModel(now) = &m.doc.object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!(**now, theirs);
    assert_eq!(now.face_count(), 2);
}

#[test]
fn a_broken_model_is_refused() {
    let (doc, id) = document();
    let props = |shape: serde_json::Value| {
        let o = doc.object(id).unwrap();
        let mut p = serde_json::Map::new();
        p.insert("layer".into(), json!(o.layer));
        p.insert("class".into(), json!(o.class));
        p.insert("shape".into(), shape);
        p
    };
    let good = serde_json::to_value(&doc.object(id).unwrap().shape).unwrap();
    assert!(validate_entity(Kind::Object, Uid::from_num(900), &props(good.clone())).is_ok());
    // A face loop that names an edge that is not there.
    let mut bad = good.clone();
    bad["edges"].as_array_mut().unwrap().pop();
    assert!(validate_entity(Kind::Object, Uid::from_num(900), &props(bad)).is_err());
    let mut far = good;
    far["vertices"][0][1] = json!(5e9);
    assert!(validate_entity(Kind::Object, Uid::from_num(900), &props(far)).is_err());
}
