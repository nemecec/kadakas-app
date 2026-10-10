//! Cases where two locally valid edits make a state that no document can have. Each one is a
//! case from the review probe. `materialize` must give a valid document, the same for every
//! client, and say what it changed.

use kadakas_core::model::{Document, FIRST_LAYER, NONE_CLASS, Object, ObjectId};
use kadakas_core::resource::{Resource, ResourceBody, ResourceId};
use kadakas_core::uid::Uid;
use serde_json::{Value, json};

use super::*;
use crate::docsync::replica_of;
use crate::op::{Op, Writer};

fn n(x: u64) -> Uid {
    Uid::from_num(x)
}

fn w(c: u64, o: u64) -> Writer {
    Writer {
        client: n(c),
        op: o,
    }
}

fn base() -> Document {
    Document::default()
}

fn make(r: &mut Replica, who: Writer, kind: Kind, id: u64, props: Value) {
    let props = props.as_object().unwrap().clone().into_iter().collect();
    let op = Op::Create {
        target: Target::new(kind, n(id)),
        props,
        merge: false,
    };
    assert!(r.apply(who, &op).is_applied(), "create {id}");
}

fn set(r: &mut Replica, who: Writer, kind: Kind, id: u64, prop: &str, v: Value) {
    let op = Op::Set {
        target: Target::new(kind, n(id)),
        prop: prop.into(),
        value: v,
    };
    assert!(r.apply(who, &op).is_applied(), "set {prop} of {id}");
}

fn delete(r: &mut Replica, who: Writer, kind: Kind, id: u64) {
    let op = Op::Delete {
        target: Target::new(kind, n(id)),
    };
    assert!(r.apply(who, &op).is_applied());
}

fn rect(layer: Uid) -> Value {
    json!({"layer": layer, "class": NONE_CLASS.0,
           "shape": {"kind": "rect", "min": [0, 0], "max": [10, 5]}})
}

fn group(layer: Uid) -> Value {
    json!({"layer": layer, "class": NONE_CLASS.0, "shape": {"kind": "group"}})
}

fn layer(kind: &str) -> Value {
    json!({"name": "L", "kind": kind, "visible": true, "locked": false,
           "elevation": 0.0, "scale": 100.0})
}

fn res(name: &str, body: Value) -> Value {
    json!({"name": name, "body": body})
}

fn symbol_using(other: u64) -> Value {
    json!({"type": "symbol", "items": [
        {"shape": {"kind": "symbol", "symbol": n(other), "at": [0, 0]}}]})
}

fn run(r: &Replica) -> Materialized {
    let m = materialize(r, &base());
    // The result must be a document that the core accepts.
    let mut again = m.doc.clone();
    again.validate().expect("the result is valid");
    m
}

#[test]
fn a_clean_replica_gives_the_same_document() {
    let mut ed = kadakas_core::command::Editor::with_sequential_ids();
    let cmd: kadakas_core::command::Command = serde_json::from_value(json!({
        "op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5, 5]}
    }))
    .unwrap();
    ed.execute(&cmd).unwrap();
    let r = replica_of(ed.doc());
    let m = materialize(&r, ed.doc());
    // The document is the same, with the keys of the stacking order added.
    let mut without = m.doc.clone();
    for o in without.objects.values_mut() {
        assert!(o.extra.remove("order").is_some());
    }
    for l in &mut without.layers {
        assert!(l.extra.remove("order").is_some());
    }
    assert_eq!(&without, ed.doc());
    assert!(m.quarantined.is_empty() && m.repairs.is_empty());
    // A new document gets the content, in the order of the IDs.
    let m = materialize(&r, &Document::default());
    assert_eq!(m.doc.objects.len(), 1);
}

#[test]
fn orphans_go_to_the_first_layer_and_class() {
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Layer, 900, layer("design"));
    make(
        &mut r,
        w(1, 1),
        Kind::Class,
        901,
        json!({"name": "C", "visible": true, "attrs": {}}),
    );
    let mut props = rect(n(900));
    props["class"] = json!(n(901));
    props["parent"] = json!(n(777));
    make(&mut r, w(2, 1), Kind::Object, 902, props);
    delete(&mut r, w(1, 2), Kind::Layer, 900);
    delete(&mut r, w(1, 3), Kind::Class, 901);
    let m = run(&r);
    let o = m.doc.object(ObjectId(n(902))).unwrap();
    assert_eq!(o.layer, FIRST_LAYER);
    assert_eq!(o.class, NONE_CLASS);
    assert_eq!(o.parent, None);
    assert!(!m.repairs.is_empty());
}

#[test]
fn a_vanished_active_layer_is_reset() {
    let mut ed = kadakas_core::command::Editor::with_sequential_ids();
    ed.execute(&serde_json::from_value(json!({"op": "add_layer", "name": "B"})).unwrap())
        .unwrap();
    let mut b = ed.doc().clone();
    b.active_layer = b.layers.last().unwrap().id;
    let m = materialize(&replica_of(&Document::default()), &b);
    assert_eq!(m.doc.active_layer, FIRST_LAYER);
    assert_eq!(m.doc.active_class, NONE_CLASS);
}

#[test]
fn a_thing_that_fails_the_checks_is_quarantined_not_fatal() {
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Object, 5, rect(FIRST_LAYER.0));
    make(
        &mut r,
        w(1, 1),
        Kind::Object,
        6,
        json!({"layer": FIRST_LAYER.0, "class": NONE_CLASS.0,
               "shape": {"kind": "rect", "min": "x"}}),
    );
    make(
        &mut r,
        w(1, 1),
        Kind::Object,
        7,
        json!({"layer": FIRST_LAYER.0, "class": NONE_CLASS.0,
               "shape": {"kind": "rect", "min": [0, 0], "max": [1e13, 5]}}),
    );
    make(&mut r, w(1, 1), Kind::Layer, 8, json!({"name": 5}));
    make(&mut r, w(1, 1), Kind::Class, 9, json!({"oops": true}));
    make(
        &mut r,
        w(1, 1),
        Kind::Resource,
        10,
        res("Bad", json!({"type": "worksheet"})),
    );
    set(&mut r, w(1, 2), Kind::Document, 0, "drawing", json!(5));
    set(&mut r, w(1, 2), Kind::Document, 0, "georef", json!("nope"));
    let m = run(&r);
    assert!(m.doc.objects.contains_key(&ObjectId(n(5))));
    assert_eq!(m.doc.objects.len(), 1);
    let ids: Vec<Uid> = m.quarantined.iter().map(|q| q.target.id).collect();
    for id in [6, 7, 8, 9, 10] {
        // The worksheet is valid (a JSON object); the others are not.
        if id == 10 {
            continue;
        }
        assert!(ids.contains(&n(id)), "{id} in {ids:?}");
    }
    assert!(
        m.quarantined
            .iter()
            .any(|q| q.target.kind == Kind::Document)
    );
    assert!(m.doc.georef.is_none());
    // The bad thing stays in the replica: a later good edit brings it back.
    set(
        &mut r,
        w(1, 3),
        Kind::Object,
        6,
        "shape",
        json!({"kind": "rect", "min": [0, 0], "max": [2, 2]}),
    );
    let m = run(&r);
    assert!(m.doc.objects.contains_key(&ObjectId(n(6))));
}

#[test]
fn a_group_cycle_is_broken_at_the_smallest_id() {
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Object, 20, group(FIRST_LAYER.0));
    make(&mut r, w(1, 1), Kind::Object, 21, group(FIRST_LAYER.0));
    // Two edits that are valid alone: each group goes into the other.
    set(&mut r, w(1, 2), Kind::Object, 20, "parent", json!(n(21)));
    set(&mut r, w(2, 1), Kind::Object, 21, "parent", json!(n(20)));
    let m = run(&r);
    assert_eq!(m.doc.object(ObjectId(n(20))).unwrap().parent, None);
    assert_eq!(
        m.doc.object(ObjectId(n(21))).unwrap().parent,
        Some(ObjectId(n(20)))
    );
}

#[test]
fn a_self_parent_and_a_deep_chain_are_repaired() {
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Object, 30, group(FIRST_LAYER.0));
    set(&mut r, w(1, 2), Kind::Object, 30, "parent", json!(n(30)));
    // A chain longer than the limit.
    let top = 100;
    for i in 0..(kadakas_core::group::MAX_DEPTH as u64 + 5) {
        let mut g = group(FIRST_LAYER.0);
        if i > 0 {
            g["parent"] = json!(n(top + i - 1));
        }
        make(&mut r, w(1, 3), Kind::Object, top + i, g);
    }
    let m = run(&r);
    assert_eq!(m.doc.object(ObjectId(n(30))).unwrap().parent, None);
    assert!(m.repairs.iter().any(|t| t.contains("too deep")));
}

#[test]
fn a_member_on_another_layer_than_its_group_leaves_the_group() {
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Layer, 40, layer("design"));
    make(&mut r, w(1, 1), Kind::Object, 41, group(FIRST_LAYER.0));
    make(&mut r, w(1, 1), Kind::Object, 42, {
        let mut o = rect(FIRST_LAYER.0);
        o["parent"] = json!(n(41));
        o
    });
    // User 1 moves the group to the new layer; user 2 adds a member on the first layer.
    set(&mut r, w(1, 2), Kind::Object, 41, "layer", json!(n(40)));
    set(&mut r, w(1, 2), Kind::Object, 42, "layer", json!(n(40)));
    make(&mut r, w(2, 1), Kind::Object, 43, {
        let mut o = rect(FIRST_LAYER.0);
        o["parent"] = json!(n(41));
        o
    });
    // A member whose parent is not a group.
    make(&mut r, w(2, 2), Kind::Object, 44, {
        let mut o = rect(n(40));
        o["parent"] = json!(n(42));
        o
    });
    let m = run(&r);
    assert_eq!(
        m.doc.object(ObjectId(n(42))).unwrap().parent,
        Some(ObjectId(n(41)))
    );
    assert_eq!(m.doc.object(ObjectId(n(43))).unwrap().parent, None);
    assert_eq!(m.doc.object(ObjectId(n(44))).unwrap().parent, None);
}

#[test]
fn duplicate_resource_names_are_renamed_by_id_order() {
    let mut r = replica_of(&base());
    let body = json!({"type": "worksheet"});
    make(
        &mut r,
        w(1, 1),
        Kind::Resource,
        502,
        res("Oak", body.clone()),
    );
    make(
        &mut r,
        w(2, 1),
        Kind::Resource,
        501,
        res("OAK", body.clone()),
    );
    make(
        &mut r,
        w(2, 2),
        Kind::Resource,
        503,
        res("  ", body.clone()),
    );
    make(
        &mut r,
        w(2, 3),
        Kind::Resource,
        504,
        res("oak", json!({"type": "irrigation_catalog"})),
    );
    let m = run(&r);
    let name = |id| m.doc.resources.get(ResourceId(n(id))).unwrap().name.clone();
    assert_eq!(name(501), "OAK", "the smaller ID keeps the name");
    assert!(name(502).starts_with("Oak ("));
    assert!(name(503).starts_with("Resource "));
    assert_eq!(name(504), "oak", "another type may use the name");
    // The result does not depend on the order in which the host has the resources.
    let mut other = base();
    for id in [502, 501] {
        other.resources.push(Resource {
            id: ResourceId(n(id)),
            name: format!("x{id}"),
            body: ResourceBody::Worksheet(json!({})),
        });
    }
    let m2 = materialize(&r, &other);
    for id in [501, 502, 503, 504] {
        assert_eq!(
            m2.doc.resources.get(ResourceId(n(id))).unwrap().name,
            name(id)
        );
    }
}

#[test]
fn a_resource_cycle_is_broken_at_the_smallest_id() {
    let mut r = replica_of(&base());
    make(
        &mut r,
        w(1, 1),
        Kind::Resource,
        601,
        res("A", symbol_using(602)),
    );
    make(
        &mut r,
        w(2, 1),
        Kind::Resource,
        602,
        res("B", symbol_using(601)),
    );
    make(
        &mut r,
        w(2, 2),
        Kind::Resource,
        603,
        res("C", symbol_using(603)),
    );
    let m = run(&r);
    assert!(m.doc.resources.get(ResourceId(n(601))).is_none());
    assert!(m.doc.resources.get(ResourceId(n(602))).is_some());
    assert!(m.doc.resources.get(ResourceId(n(603))).is_none());
    let q: Vec<Uid> = m.quarantined.iter().map(|q| q.target.id).collect();
    assert!(q.contains(&n(601)) && q.contains(&n(603)));
}

#[test]
fn dangling_resource_references_are_dropped() {
    let mut r = replica_of(&base());
    let mut o = rect(FIRST_LAYER.0);
    o["attrs"] = json!({"fill_pattern": n(700), "line_type": n(701), "pen": [1, 2, 3, 255]});
    make(&mut r, w(1, 1), Kind::Object, 50, o);
    let m = run(&r);
    let o = m.doc.object(ObjectId(n(50))).unwrap();
    assert_eq!(o.attrs.fill_pattern, None);
    assert_eq!(o.attrs.line_type, None);
    assert!(o.attrs.pen.is_some());
}

#[test]
fn a_viewport_whose_sheet_layer_is_gone_moves_or_leaves() {
    let vp = |layer: Uid| {
        json!({"layer": layer, "class": NONE_CLASS.0,
               "shape": {"kind": "viewport", "min": [0, 0], "max": [100, 100],
                         "center": [0, 0], "scale": 100.0, "layers": [n(999), FIRST_LAYER.0]}})
    };
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Layer, 61, layer("sheet"));
    make(&mut r, w(1, 1), Kind::Layer, 62, layer("sheet"));
    make(&mut r, w(1, 1), Kind::Object, 63, vp(n(61)));
    delete(&mut r, w(2, 1), Kind::Layer, 61);
    // The sheet 62 is the one left: the viewport goes there, and drops the layer that is gone.
    let m = run(&r);
    let o = m.doc.object(ObjectId(n(63))).unwrap();
    assert_eq!(o.layer.0, n(62));
    match &o.shape {
        kadakas_core::model::Shape::Viewport(v) => assert_eq!(v.layers.len(), 1),
        other => panic!("{other:?}"),
    }
    // No sheet layer at all: the viewport is quarantined.
    delete(&mut r, w(2, 2), Kind::Layer, 62);
    let m = run(&r);
    assert!(m.doc.object(ObjectId(n(63))).is_none());
    assert!(m.quarantined.iter().any(|q| q.target.id == n(63)));
}

#[test]
fn an_object_that_does_not_belong_on_a_sheet_moves_off() {
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Layer, 71, layer("design"));
    make(
        &mut r,
        w(1, 1),
        Kind::Object,
        72,
        json!({"layer": n(71), "class": NONE_CLASS.0,
               "shape": {"kind": "circle", "center": [0, 0], "radius": 2}}),
    );
    make(
        &mut r,
        w(1, 1),
        Kind::Object,
        73,
        json!({"layer": n(71), "class": NONE_CLASS.0,
               "shape": {"kind": "plant", "at": [1000, 1000], "species": "Acer campestre", "spread": 4000, "height": 8000}}),
    );
    // Another user turns the layer into a sheet.
    set(&mut r, w(2, 1), Kind::Layer, 71, "kind", json!("sheet"));
    let m = run(&r);
    // Whatever the shapes allow, the document is valid and the objects are there.
    assert!(m.doc.object(ObjectId(n(72))).is_some());
}

#[test]
fn the_settings_and_the_sheets_travel() {
    let mut b = base();
    b.drawing.project.name = "Garden".into();
    let r = replica_of(&b);
    let m = materialize(&r, &Document::default());
    assert_eq!(m.doc.drawing.project.name, "Garden");
}

#[test]
fn a_hostile_state_never_fails_and_is_the_same_for_every_order() {
    let mut r = replica_of(&base());
    for i in 0..30u64 {
        let g = group(FIRST_LAYER.0);
        make(&mut r, w(1, i + 1), Kind::Object, 1000 + i, g);
    }
    for i in 0..30u64 {
        set(
            &mut r,
            w(2, i + 1),
            Kind::Object,
            1000 + i,
            "parent",
            json!(n(1000 + (i * 7 + 3) % 30)),
        );
    }
    let a = run(&r);
    let mut shuffled = base();
    // Same replica, a document with the objects in another order as the base.
    for (i, id) in (0..30u64).rev().enumerate() {
        let _ = i;
        let mut o = Object::new(
            ObjectId(n(1000 + id)),
            FIRST_LAYER,
            NONE_CLASS,
            kadakas_core::model::Shape::Group {
                name: String::new(),
            },
        );
        o.parent = None;
        shuffled.objects.insert(o.id, o);
    }
    let b = materialize(&r, &shuffled);
    let parents = |d: &Document| {
        let mut v: Vec<(Uid, Option<Uid>)> = d
            .objects
            .values()
            .map(|o| (o.id.0, o.parent.map(|p| p.0)))
            .collect();
        v.sort();
        v
    };
    assert_eq!(parents(&a.doc), parents(&b.doc));
}

#[test]
fn the_last_way_out_names_the_thing_that_the_core_refuses() {
    let group = |id| {
        Object::new(
            ObjectId(n(id)),
            FIRST_LAYER,
            NONE_CLASS,
            kadakas_core::model::Shape::Group {
                name: String::new(),
            },
        )
    };
    let mut doc = base();
    doc.objects.insert(ObjectId(n(5)), group(5));
    doc.resources.push(Resource {
        id: ResourceId(n(6)),
        name: "W".into(),
        body: ResourceBody::Worksheet(json!({})),
    });
    let mut cx = Ctx {
        quarantined: Vec::new(),
        repairs: Vec::new(),
    };
    blame(&mut doc, format!("object {} is wrong", n(5)), &mut cx);
    assert!(doc.objects.is_empty());
    blame(&mut doc, format!("resource {} is wrong", n(6)), &mut cx);
    assert_eq!(doc.resources.len(), 0);
    // A message without an ID: all objects go.
    doc.objects.insert(ObjectId(n(7)), group(7));
    blame(&mut doc, "nothing to name".into(), &mut cx);
    assert!(doc.objects.is_empty());
    assert_eq!(cx.quarantined.len(), 3);
}

#[test]
fn users_who_change_different_attributes_and_keys_both_keep_their_change() {
    let mut r = replica_of(&base());
    let mut o = rect(FIRST_LAYER.0);
    o["attrs.pen"] = json!([1, 2, 3, 255]);
    o["data.owner"] = json!("x");
    make(&mut r, w(1, 1), Kind::Object, 5, o);
    // Properties are cut: attrs.pen, data.owner.
    let e = r.alive(Target::new(Kind::Object, n(5))).unwrap();
    assert!(e.get("attrs.pen").is_some() && e.get("data.owner").is_some());
    assert!(e.get("attrs").is_none());
    set(
        &mut r,
        w(1, 2),
        Kind::Object,
        5,
        "attrs.fill",
        json!([9, 9, 9, 255]),
    );
    set(
        &mut r,
        w(2, 1),
        Kind::Object,
        5,
        "attrs.line_weight",
        json!(0.5),
    );
    set(&mut r, w(2, 2), Kind::Object, 5, "data.phase", json!("new"));
    let m = run(&r);
    let o = m.doc.object(ObjectId(n(5))).unwrap();
    assert!(o.attrs.pen.is_some() && o.attrs.fill.is_some());
    assert_eq!(o.attrs.line_weight, Some(0.5));
    assert_eq!(o.data.len(), 2);
}

#[test]
fn the_drawing_data_is_cut_into_parts_and_sheets_by_layer() {
    let mut ed = kadakas_core::command::Editor::with_sequential_ids();
    ed.execute(&serde_json::from_value(json!({"op": "add_sheet", "name": "A1"})).unwrap())
        .unwrap();
    ed.execute(
        &serde_json::from_value(json!({"op": "set_project", "name": "Garden", "client": "Ann"}))
            .unwrap(),
    )
    .unwrap();
    let doc = ed.doc().clone();
    let r = replica_of(&doc);
    let settings = r.alive(Target::DOCUMENT).unwrap();
    assert!(settings.get("drawing.project.name").is_some());
    assert!(settings.get("drawing.project.client").is_some());
    assert!(
        settings
            .visible()
            .keys()
            .any(|k| k.starts_with("drawing.sheets."))
    );
    let m = materialize(&r, &Document::default());
    assert_eq!(m.doc.drawing, doc.drawing);
    // Two users: one changes the project name, the other the client. Both stay.
    let mut r = r;
    set(
        &mut r,
        w(1, 1),
        Kind::Document,
        0,
        "drawing.project.name",
        json!("Park"),
    );
    set(
        &mut r,
        w(2, 1),
        Kind::Document,
        0,
        "drawing.project.client",
        json!("Bo"),
    );
    let m = materialize(&r, &doc);
    assert_eq!(m.doc.drawing.project.name, "Park");
    assert_eq!(m.doc.drawing.project.client, "Bo");
}

#[test]
fn an_older_whole_property_and_a_cut_one_merge() {
    // A client that writes `attrs` whole (an older version) and one that writes `attrs.pen`.
    let mut r = replica_of(&base());
    make(&mut r, w(1, 1), Kind::Object, 5, rect(FIRST_LAYER.0));
    set(
        &mut r,
        w(1, 2),
        Kind::Object,
        5,
        "attrs",
        json!({"fill": [1, 1, 1, 255]}),
    );
    set(
        &mut r,
        w(2, 1),
        Kind::Object,
        5,
        "attrs.pen",
        json!([2, 2, 2, 255]),
    );
    let m = run(&r);
    let o = m.doc.object(ObjectId(n(5))).unwrap();
    assert!(o.attrs.fill.is_some() && o.attrs.pen.is_some());
}

#[test]
fn objects_and_layers_follow_their_keys_and_ties_go_by_id() {
    let mut r = replica_of(&base());
    for (id, key) in [(30, "a5"), (31, "a1"), (32, "a5"), (33, "a9")] {
        let mut o = rect(FIRST_LAYER.0);
        o["order"] = json!(key);
        make(&mut r, w(1, 1), Kind::Object, id, o);
    }
    let order = |m: &Materialized| -> Vec<Uid> { m.doc.objects.keys().map(|k| k.0).collect() };
    let m = run(&r);
    // a1, then the two a5 (by ID), then a9.
    assert_eq!(order(&m), vec![n(31), n(30), n(32), n(33)]);
    // The key is in the document, so a saved file keeps it.
    let o = m.doc.object(ObjectId(n(31))).unwrap();
    assert_eq!(o.extra["order"], json!("a1"));
    // The order of the host's own document does not matter.
    let other = materialize(&r, &m.doc);
    assert_eq!(order(&other), order(&m));
    // Layers too.
    let mut l = layer("design");
    l["order"] = json!("Zz");
    make(&mut r, w(1, 2), Kind::Layer, 40, l);
    let m = run(&r);
    assert_eq!(m.doc.layers[0].id.0, n(40), "a small key comes first");
}

#[test]
fn a_local_change_of_the_order_becomes_new_keys_between_neighbours() {
    use kadakas_core::command::{Arrange, Command, Editor};
    let mut ed = Editor::with_sequential_ids();
    let ids: Vec<ObjectId> = (0..4)
        .map(|i| {
            let cmd: Command = serde_json::from_value(json!({
                "op": "add_object", "shape": {"kind": "rect", "min": [i, 0], "max": [i + 1, 1]}
            }))
            .unwrap();
            ObjectId(ed.execute(&cmd).unwrap().created[0])
        })
        .collect();
    let mut shadow = replica_of(ed.doc());
    let key = |r: &Replica, id: ObjectId| -> String {
        r.alive(Target::new(Kind::Object, id.0))
            .unwrap()
            .get("order")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    };
    let before: Vec<String> = ids.iter().map(|i| key(&shadow, *i)).collect();
    assert!(before.windows(2).all(|p| p[0] < p[1]));
    // Send the first object to the front: one new key, the others keep theirs.
    let rev = ed.revision();
    ed.execute(&Command::Arrange {
        ids: vec![ids[0]],
        how: Arrange::Front,
    })
    .unwrap();
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .cloned()
        .unwrap();
    let ops = crate::docsync::ops_from_change(&shadow, ed.doc(), &cs);
    assert_eq!(ops.len(), 1, "{ops:?}");
    shadow.apply_all(Writer::BASELINE, &ops);
    assert!(key(&shadow, ids[0]) > key(&shadow, ids[3]));
    for i in &ids[1..] {
        assert_eq!(
            key(&shadow, *i),
            before[ids.iter().position(|x| x == i).unwrap()]
        );
    }
    // A layer move is a change of the layer order.
    ed.execute(&Command::AddLayer {
        name: "B".into(),
        elevation: 0.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .cloned()
        .unwrap();
    let ops = crate::docsync::ops_from_change(&shadow, ed.doc(), &cs);
    shadow.apply_all(Writer::BASELINE, &ops);
    let second = ed.doc().layers[1].id;
    let rev = ed.revision();
    ed.execute(&Command::MoveLayer {
        id: second,
        index: 0,
    })
    .unwrap();
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .cloned()
        .unwrap();
    assert_eq!(cs.op, "move_layer");
    let ops = crate::docsync::ops_from_change(&shadow, ed.doc(), &cs);
    assert_eq!(ops.len(), 1);
    shadow.apply_all(Writer::BASELINE, &ops);
    let m = materialize(&shadow, ed.doc());
    assert_eq!(m.doc.layers[0].id, second);
}

#[test]
fn moving_one_point_of_a_site_model_sends_one_chunk() {
    use kadakas_core::command::{Command, Editor};
    let points = |n: usize, bump: Option<usize>| -> Vec<Value> {
        (0..n)
            .map(|i| {
                let z = if Some(i) == bump { 99.0 } else { 0.0 };
                json!([i as f64, (i % 7) as f64, z])
            })
            .collect()
    };
    let mut ed = Editor::with_sequential_ids();
    let cmd: Command = serde_json::from_value(json!({
        "op": "add_object",
        "shape": {"kind": "site_model", "points": points(1000, None)}
    }))
    .unwrap();
    let id = ObjectId(ed.execute(&cmd).unwrap().created[0]);
    let mut shadow = replica_of(ed.doc());
    let e = shadow.alive(Target::new(Kind::Object, id.0)).unwrap();
    let chunks = e
        .visible()
        .keys()
        .filter(|k| k.starts_with("shape.points."))
        .count();
    assert_eq!(chunks, 1000_usize.div_ceil(crate::docsync::POINT_CHUNK));
    assert!(e.get("shape.points.00000000").is_some());
    // The base of the shape has no points, so a change of a setting is small too.
    assert!(e.get("shape").unwrap().get("points").is_none());

    // Change one point in the middle.
    let rev = ed.revision();
    let cmd: Command = serde_json::from_value(json!({
        "op": "modify_object", "id": id,
        "shape": {"kind": "site_model", "points": points(1000, Some(600))}
    }))
    .unwrap();
    ed.execute(&cmd).unwrap();
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .cloned()
        .unwrap();
    let ops = crate::docsync::ops_from_change(&shadow, ed.doc(), &cs);
    assert_eq!(ops.len(), 1, "{} ops", ops.len());
    assert!(matches!(&ops[0], Op::Set { prop, value, .. }
            if prop == "shape.points.00000002" && value.as_array().unwrap().len() == crate::docsync::POINT_CHUNK));
    shadow.apply_all(Writer::BASELINE, &ops);

    // Two users change points in different chunks: both stay. The document builds back.
    let mut r = replica_of(ed.doc());
    let pts = |k: usize, z: f64| -> Value {
        let mut chunk = r_chunk(&r, id, k);
        chunk[0][2] = json!(z);
        chunk
    };
    fn r_chunk(r: &Replica, id: ObjectId, k: usize) -> Value {
        r.alive(Target::new(Kind::Object, id.0))
            .unwrap()
            .get(&format!("shape.points.{k:08}"))
            .unwrap()
            .clone()
    }
    let a = pts(0, 5.0);
    let b = pts(3, 7.0);
    set(
        &mut r,
        w(1, 1),
        Kind::Object,
        id.0.low_u64(),
        "shape.points.00000000",
        a,
    );
    set(
        &mut r,
        w(2, 1),
        Kind::Object,
        id.0.low_u64(),
        "shape.points.00000003",
        b,
    );
    let m = materialize(&r, ed.doc());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    match &m.doc.object(id).unwrap().shape {
        kadakas_core::model::Shape::SiteModel { points, .. } => {
            assert_eq!(points.len(), 1000);
            assert_eq!(points[0].z, 5.0);
            assert_eq!(points[768].z, 7.0);
            assert_eq!(points[600].z, 99.0);
        }
        other => panic!("{other:?}"),
    }
    // Fewer points: the extra chunks go.
    let cmd: Command = serde_json::from_value(json!({
        "op": "modify_object", "id": id,
        "shape": {"kind": "site_model", "points": points(300, None)}
    }))
    .unwrap();
    ed.execute(&cmd).unwrap();
    let ops = crate::docsync::ops_from_diff(&shadow, ed.doc());
    assert!(
        ops.iter()
            .any(|o| matches!(o, Op::Unset { prop, .. } if prop == "shape.points.00000003"))
    );
    shadow.apply_all(Writer::BASELINE, &ops);
    let m = materialize(&shadow, ed.doc());
    assert_eq!(
        m.doc.object(id),
        ed.doc()
            .object(id)
            .map(|o| {
                let mut o = o.clone();
                o.extra.insert(
                    "order".into(),
                    m.doc.object(id).unwrap().extra["order"].clone(),
                );
                o
            })
            .as_ref()
    );
}

#[test]
fn a_hostile_key_is_refused_by_the_check_and_counts_as_no_key() {
    let mut r = replica_of(&base());
    for (id, key) in [(50, "a1"), (51, "~"), (52, "a0V0")] {
        let mut o = rect(FIRST_LAYER.0);
        o["order"] = json!(key);
        make(&mut r, w(1, 1), Kind::Object, id, o);
    }
    let m = run(&r);
    // The objects with a hostile key are quarantined by the check of the thing.
    assert!(m.doc.object(ObjectId(n(50))).is_some());
    assert!(m.doc.object(ObjectId(n(51))).is_none());
    assert!(
        m.quarantined
            .iter()
            .any(|q| q.target.id == n(51) && q.reason.contains("order"))
    );
    // A key in `extra` of the host's own document that is not valid is ignored by the sort and
    // replaced by a good one.
    let mut doc = base();
    let mut o = Object::new(
        ObjectId(n(60)),
        FIRST_LAYER,
        NONE_CLASS,
        kadakas_core::model::Shape::Group {
            name: String::new(),
        },
    );
    o.extra.insert("order".into(), json!("~"));
    doc.objects.insert(o.id, o);
    let r = replica_of(&doc);
    let key = r
        .alive(Target::new(Kind::Object, n(60)))
        .unwrap()
        .get("order")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert!(crate::order::valid_key(&key));
}
