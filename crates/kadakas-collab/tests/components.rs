//! Groups and components (MOD-07) in the collaboration: groups and instances are objects and a
//! definition is a resource, so every command syncs as changes of objects and resources. The
//! document comes back through `materialize` as it was, and a damaged definition is refused.

use kadakas_collab::docsync::replica_of;
use kadakas_collab::validate::validate_entity;
use kadakas_collab::{Kind, Op, Target, Writer, materialize};
use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::resource::ResourceId;
use kadakas_core::surface::Edit;
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

/// A group, a component with two instances and a second component inside the first.
fn document() -> (Document, ObjectId, ResourceId, ObjectId) {
    let mut ed = Editor::with_sequential_ids();
    let mut model = |x: f64| {
        let id = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::default()),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::EditSurface {
            id,
            edit: square(x),
        })
        .unwrap();
        id
    };
    let (a, b) = (model(0.0), model(3000.0));
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![a],
            name: Some("g".into()),
        })
        .unwrap()
        .created[0],
    );
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Tile".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![b],
            replace: true,
        })
        .unwrap();
    let (sym, inst) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    ed.execute(&Command::CopyObjects {
        ids: vec![inst],
        delta: DVec2::new(0.0, 4000.0),
    })
    .unwrap();
    (ed.doc().clone(), g, sym, inst)
}

#[test]
fn groups_instances_and_definitions_round_trip_through_the_replica() {
    let (doc, g, sym, inst) = document();
    let m = materialize(&replica_of(&doc), &Document::default());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    // The stacking key ("order") is added by the replica; everything else is as it was.
    for id in doc.objects.keys() {
        let (a, b) = (m.doc.object(*id).unwrap(), doc.object(*id).unwrap());
        assert_eq!(
            (&a.shape, a.parent, &a.attrs),
            (&b.shape, b.parent, &b.attrs),
            "{id}"
        );
    }
    assert_eq!(m.doc.object(g).unwrap().shape, doc.object(g).unwrap().shape);
    assert_eq!(
        m.doc.object(inst).unwrap().shape,
        doc.object(inst).unwrap().shape
    );
    assert_eq!(m.doc.resources.get(sym), doc.resources.get(sym));
    assert_eq!(m.doc.resources.symbol(sym).unwrap().parts.len(), 1);
}

#[test]
fn a_definition_that_another_user_changed_arrives_as_one_property() {
    let (doc, _, sym, inst) = document();
    // The other user edits the definition (adds a second square) and sends the resource body.
    let mut def = doc.resources.symbol(sym).unwrap().clone();
    let Shape::SurfaceModel(m) = &mut def.parts[0].shape else {
        panic!()
    };
    m.apply(&square(1000.0)).unwrap();
    let body = kadakas_core::resource::ResourceBody::Graphic(Box::new(
        kadakas_core::graphics::GraphicResource::Symbol(def),
    ));
    let mut replica = replica_of(&doc);
    replica.apply(
        Writer {
            client: Uid::from_num(7),
            op: 1,
        },
        &Op::Set {
            target: Target::new(Kind::Resource, sym.0),
            prop: "body".into(),
            value: serde_json::to_value(&body).unwrap(),
        },
    );
    let now = materialize(&replica, &doc);
    assert!(now.quarantined.is_empty(), "{:?}", now.quarantined);
    let area: f64 = kadakas_core::component::leaves(&now.doc.resources, sym)
        .unwrap()
        .iter()
        .map(|l| l.model.area())
        .sum();
    assert_eq!(area, 2_000_000.0);
    // Both instances use it, with no change to the objects.
    assert_eq!(
        now.doc.object(inst).unwrap().shape,
        doc.object(inst).unwrap().shape
    );
}

#[test]
fn an_instance_with_a_bad_transform_is_refused() {
    let (doc, _, _, inst) = document();
    let o = doc.object(inst).unwrap();
    let props = |shape: serde_json::Value| {
        let mut p = serde_json::Map::new();
        p.insert("layer".into(), json!(o.layer));
        p.insert("class".into(), json!(o.class));
        p.insert("shape".into(), shape);
        p
    };
    let good = serde_json::to_value(&o.shape).unwrap();
    assert!(validate_entity(Kind::Object, Uid::from_num(901), &props(good.clone())).is_ok());
    let mut flat = good.clone();
    flat["transform"] = json!([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(validate_entity(Kind::Object, Uid::from_num(901), &props(flat)).is_err());
    let mut far = good;
    far["transform"][9] = json!(5e9);
    assert!(validate_entity(Kind::Object, Uid::from_num(901), &props(far)).is_err());
}
