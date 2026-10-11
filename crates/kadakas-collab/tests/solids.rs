//! Extrudes, sweeps and 3D polygons (3D-03) in the collaboration: they sync as properties (the
//! shape is one property), come back through `materialize`, and a bad shape is quarantined.

use kadakas_collab::docsync::replica_of;
use kadakas_collab::validate::validate_entity;
use kadakas_collab::{Kind, Op, Target, Writer, materialize};
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::model::{Attributes, Document, Shape};
use kadakas_core::solid3d::{Extrude, Sweep};
use kadakas_core::uid::Uid;
use kadakas_core::{DVec2, DVec3};
use serde_json::json;

fn sq(w: f64) -> Vec<kadakas_core::curve::Vertex> {
    corners(&[
        DVec2::ZERO,
        DVec2::new(w, 0.0),
        DVec2::new(w, w),
        DVec2::new(0.0, w),
    ])
}

fn document() -> Document {
    let mut ed = Editor::with_sequential_ids();
    let mut e = Extrude::new(sq(2000.0), 450.0);
    e.holes = vec![corners(&[
        DVec2::new(500.0, 500.0),
        DVec2::new(900.0, 500.0),
        DVec2::new(900.0, 900.0),
        DVec2::new(500.0, 900.0),
    ])];
    let mut s = Sweep::new(sq(400.0));
    s.origin = DVec3::new(5000.0, 0.0, 10.0);
    s.pitch = 100.0;
    for shape in [
        Shape::Extrude(Box::new(e)),
        Shape::Sweep(Box::new(s)),
        Shape::Polyline3d {
            points: vec![
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 5.0),
                DVec3::new(10.0, 10.0, 9.0),
            ],
            closed: true,
        },
    ] {
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    ed.doc().clone()
}

#[test]
fn the_new_kinds_round_trip_through_the_replica() {
    let doc = document();
    let replica = replica_of(&doc);
    let m = materialize(&replica, &Document::default());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    assert_eq!(m.doc.objects.len(), 3);
    for (id, o) in &doc.objects {
        assert_eq!(
            m.doc.object(*id).unwrap().shape,
            o.shape,
            "{}",
            o.shape.kind()
        );
    }
}

#[test]
fn one_user_changes_the_height_and_the_shape_property_follows() {
    let doc = document();
    let id = *doc.objects.keys().next().unwrap();
    let mut replica = replica_of(&doc);
    let Shape::Extrude(e) = &doc.object(id).unwrap().shape else {
        panic!()
    };
    let mut taller = (**e).clone();
    taller.height = 1200.0;
    let value = serde_json::to_value(Shape::Extrude(Box::new(taller))).unwrap();
    replica.apply(
        Writer {
            client: Uid::from_num(7),
            op: 1,
        },
        &Op::Set {
            target: Target::new(Kind::Object, id.0),
            prop: "shape".into(),
            value,
        },
    );
    let m = materialize(&replica, &doc);
    let Shape::Extrude(now) = &m.doc.object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!(now.height, 1200.0);
    assert_eq!(now.holes.len(), 1);
}

#[test]
fn bad_solids_are_refused_by_the_check_and_quarantined_when_they_arrive() {
    let doc = document();
    let id = *doc.objects.keys().next().unwrap();
    let props = |shape: serde_json::Value| {
        let layer = doc.object(id).unwrap().layer;
        let mut p = serde_json::Map::new();
        p.insert("layer".into(), json!(layer));
        p.insert("class".into(), json!(doc.object(id).unwrap().class));
        p.insert("shape".into(), shape);
        p
    };
    let ok = json!({"kind": "extrude", "profile": [[0, 0], [10, 0], [10, 10]], "height": 5});
    assert!(validate_entity(Kind::Object, Uid::from_num(900), &props(ok.clone())).is_ok());
    let zero = json!({"kind": "extrude", "profile": [[0, 0], [10, 0], [10, 10]], "height": 0});
    assert!(
        validate_entity(Kind::Object, Uid::from_num(900), &props(zero.clone()))
            .unwrap_err()
            .contains("zero")
    );
    let bow =
        json!({"kind": "extrude", "profile": [[0, 0], [10, 10], [10, 0], [0, 10]], "height": 5});
    assert!(
        validate_entity(Kind::Object, Uid::from_num(900), &props(bow))
            .unwrap_err()
            .contains("itself")
    );
    let sweep =
        json!({"kind": "sweep", "profile": [[-5, 0], [5, 0], [5, 9]], "sweep_angle_deg": 90});
    assert!(
        validate_entity(Kind::Object, Uid::from_num(900), &props(sweep))
            .unwrap_err()
            .contains("axis")
    );
    let far = json!({"kind": "sweep", "profile": [[1, 0], [5, 0], [5, 9]], "origin": [1e12, 0, 0]});
    assert!(validate_entity(Kind::Object, Uid::from_num(900), &props(far)).is_err());
    // A bad shape from another client leaves the object out and names the reason.
    let mut replica = replica_of(&doc);
    replica.apply(
        Writer {
            client: Uid::from_num(7),
            op: 1,
        },
        &Op::Create {
            target: Target::new(Kind::Object, Uid::from_num(900)),
            props: props(zero).into_iter().collect(),
            merge: false,
        },
    );
    let m = materialize(&replica, &doc);
    assert_eq!(m.doc.objects.len(), 3);
    assert_eq!(m.quarantined.len(), 1);
    assert!(
        m.quarantined[0].reason.contains("zero"),
        "{:?}",
        m.quarantined
    );
}
