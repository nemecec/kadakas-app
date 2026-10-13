//! The source tags of imported objects (IOP-22) in the collaboration: the tag is the property
//! `data.import` of the object and the source of a layer is the property `import_source`. Both
//! sync, come back through `materialize`, and a bad value from another user is refused.

use kadakas_collab::docsync::replica_of;
use kadakas_collab::validate::validate_entity;
use kadakas_collab::{Kind, Op, Target, Writer, materialize};
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::provenance::{self, ImportTag, LayerSource};
use kadakas_core::uid::Uid;
use serde_json::json;

fn tag() -> ImportTag {
    ImportTag {
        drawing: "plan.dxf".into(),
        revision: "0123456789abcdef".into(),
        handle: Some("2F".into()),
        kind: "LINE".into(),
        hash: "fedcba9876543210".into(),
        obj: "0011223344556677".into(),
        block: None,
        via: vec![],
        part: None,
    }
}

fn document() -> Document {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddLayer {
        name: "plan.dxf".into(),
        elevation: 0.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let layer = ed.doc().layers.last().unwrap().id;
    ed.execute(&Command::SetLayerSource {
        id: layer,
        source: Some(LayerSource {
            drawing: tag().drawing,
            revision: tag().revision,
            dxf_layer: String::new(),
            unit_scale: 1000.0,
            shift: [0.0, 0.0],
        }),
    })
    .unwrap();
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Locus3d {
                p: Default::default(),
            },
            layer: Some(layer),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetData {
        id,
        set: [(provenance::KEY.to_string(), tag().to_value())].into(),
        remove: vec![],
    })
    .unwrap();
    ed.doc().clone()
}

#[test]
fn tags_and_layer_sources_round_trip_through_the_replica() {
    let doc = document();
    let m = materialize(&replica_of(&doc), &Document::default());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    let o = m.doc.objects.values().next().unwrap();
    assert_eq!(provenance::tag(&o.data), Some(tag()));
    let l = m.doc.layers.last().unwrap();
    assert_eq!(
        LayerSource::of(&l.extra[provenance::LAYER_KEY])
            .unwrap()
            .drawing,
        tag().drawing
    );
}

#[test]
fn another_user_can_not_set_a_bad_tag() {
    let doc = document();
    let o = doc.objects.values().next().unwrap();
    let id = o.id;
    let mut props = serde_json::Map::new();
    props.insert("layer".into(), json!(o.layer));
    props.insert("class".into(), json!(o.class));
    props.insert("shape".into(), serde_json::to_value(&o.shape).unwrap());
    props.insert("data.import".into(), tag().to_value());
    assert!(validate_entity(Kind::Object, id.0, &props).is_ok());
    props.insert("data.import".into(), json!({"drawing": "", "kind": "x"}));
    assert!(
        validate_entity(Kind::Object, id.0, &props)
            .unwrap_err()
            .contains("import tag")
    );
    // A bad tag that arrives anyway is quarantined with the object.
    let mut replica = replica_of(&doc);
    replica.apply(
        Writer {
            client: Uid::from_num(7),
            op: 1,
        },
        &Op::Set {
            target: Target::new(Kind::Object, id.0),
            prop: "data.import".into(),
            value: json!({"drawing": 5}),
        },
    );
    let m = materialize(&replica, &doc);
    assert_eq!(m.quarantined.len(), 1);

    let layer = doc.layers.last().unwrap();
    let mut lp = serde_json::Map::new();
    lp.insert("name".into(), json!("x"));
    lp.insert("visible".into(), json!(true));
    lp.insert("locked".into(), json!(false));
    lp.insert("elevation".into(), json!(0.0));
    lp.insert("scale".into(), json!(100.0));
    lp.insert("import_source".into(), json!({"drawing": ""}));
    assert!(validate_entity(Kind::Layer, layer.id.0, &lp).is_err());
    lp.insert(
        "import_source".into(),
        json!({"drawing": "a", "revision": "r", "unit_scale": 1.0, "shift": [0.0, 0.0]}),
    );
    assert!(validate_entity(Kind::Layer, layer.id.0, &lp).is_ok());
}
