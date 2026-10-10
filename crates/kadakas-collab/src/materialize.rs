//! Building a document from a replica. This step is **total**: it always gives a valid
//! document, the same one on every client that has the same replica.
//!
//! Two users can make edits that are fine alone and wrong together: two groups that become each
//! other's parent, an object that moves to another layer than its group, two resources with the
//! same name, a layer that one user deletes while another puts a viewport on it. The merge
//! rules of the replica do not look at such links, so the replica can hold a state that no
//! document can have. If the build failed, every client would be stuck on that state for good.
//! So the build repairs it, with rules that depend only on the content of the replica and on IDs
//! (never on the order of arrival or on the local order of objects):
//!
//! - an object whose layer or class is gone goes to the first layer or the "None" class;
//! - a reference to a resource that is gone (fill pattern, line type) is dropped;
//! - a viewport shows only layers that exist;
//! - an object that is only allowed on paper (a viewport, a title block) goes to the sheet layer
//!   with the smallest ID, and to quarantine if there is none; an object that is not allowed on
//!   paper goes to the first layer when it sits on a sheet layer;
//! - a group parent is cleared when it is gone, is not a group, or is on another layer than its
//!   child;
//! - a cycle of groups is broken: the object with the smallest ID in the cycle loses its parent;
//!   a group chain that is too deep loses a parent in the same way;
//! - a cycle of resources (symbols, tiles, line types that use each other) is broken: the
//!   resource with the smallest ID in the cycle goes to quarantine;
//! - an empty resource name becomes `Resource <last 8 characters of the ID>`; when two resources
//!   of one type have the same name (without case), the one with the smaller ID keeps it and
//!   the others get ` (<last 8 characters of the ID>)` added;
//! - **quarantine**: a thing that does not read as its type, or fails the checks of the file
//!   format (`validate_entity`), is left out and reported. It stays in the replica, so nothing
//!   is lost: when a later edit makes it valid, it appears. Objects that need a quarantined
//!   layer, class or resource are repaired as above.
//!
//! The result lists what was quarantined and what was repaired, so a user interface can tell the
//! user. The repairs are *not* edits: they are not sent to the others (every client makes the same
//! repairs on its own).

use std::collections::{BTreeMap, BTreeSet};

use kadakas_core::group::MAX_DEPTH;
use kadakas_core::model::{
    Class, ClassId, Document, FIRST_LAYER, Layer, LayerId, LayerKind, NONE_CLASS, Object, ObjectId,
    Shape,
};
use kadakas_core::resource::{Resource, ResourceId};
use kadakas_core::sheets::allowed_on_sheet;
use kadakas_core::uid::Uid;
use serde_json::{Map, Value};

use crate::docsync::unflatten;
use crate::op::{Kind, Target};
use crate::replica::Replica;
use crate::validate::{find_uid, validate_entity, with_id};

/// A thing that was left out of the document, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quarantined {
    pub target: Target,
    pub reason: String,
}

/// The result of [`materialize`].
#[derive(Clone, Debug)]
pub struct Materialized {
    pub doc: Document,
    /// Things of the replica that are not in the document.
    pub quarantined: Vec<Quarantined>,
    /// Changes that the build made to fit things together (short texts).
    pub repairs: Vec<String>,
}

fn short(id: Uid) -> String {
    let t = id.to_string();
    t[t.len() - 8..].to_string()
}

/// Alive things of one kind: first those in `first` (the order that the host has), then the rest
/// in ID order.
fn ordered(
    view: &Replica,
    kind: Kind,
    first: impl Iterator<Item = Uid>,
) -> Vec<(Uid, Map<String, Value>)> {
    let mut out = Vec::new();
    let mut done = BTreeSet::new();
    for id in first {
        if let Some(e) = view.alive(Target::new(kind, id)) {
            done.insert(id);
            out.push((id, e.visible()));
        }
    }
    for (t, e) in view.alive_of(kind) {
        if !done.contains(&t.id) {
            out.push((t.id, e.visible()));
        }
    }
    out
}

struct Ctx {
    quarantined: Vec<Quarantined>,
    repairs: Vec<String>,
}

impl Ctx {
    fn quarantine(&mut self, kind: Kind, id: Uid, reason: impl Into<String>) {
        self.quarantined.push(Quarantined {
            target: Target::new(kind, id),
            reason: reason.into(),
        });
    }

    fn repair(&mut self, text: String) {
        self.repairs.push(text);
    }
}

fn read<T: serde::de::DeserializeOwned>(
    kind: Kind,
    id: Uid,
    props: &Map<String, Value>,
) -> Result<T, String> {
    serde_json::from_value(with_id(id, &unflatten(kind, props))).map_err(|e| e.to_string())
}

/// Builds the document that a replica describes. `base` gives what is not shared: the order of
/// things that exist in both, the generator of IDs and the active layer and class. Never fails.
pub fn materialize(view: &Replica, base: &Document) -> Materialized {
    let mut cx = Ctx {
        quarantined: Vec::new(),
        repairs: Vec::new(),
    };
    let defaults = Document::default();

    // Layers and classes.
    let mut layers: Vec<Layer> = Vec::new();
    for (id, props) in ordered(view, Kind::Layer, base.layers.iter().map(|l| l.id.0)) {
        let parsed = validate_entity(Kind::Layer, id, &props)
            .and_then(|()| read::<Layer>(Kind::Layer, id, &props));
        match parsed {
            Ok(l) => layers.push(l),
            Err(e) => cx.quarantine(Kind::Layer, id, e),
        }
    }
    if !layers.iter().any(|l| l.id == FIRST_LAYER) {
        layers.insert(0, defaults.layers[0].clone());
        cx.repair("the first layer was not valid: the default layer is used".into());
    }
    let mut classes: Vec<Class> = Vec::new();
    for (id, props) in ordered(view, Kind::Class, base.classes.iter().map(|c| c.id.0)) {
        let parsed = validate_entity(Kind::Class, id, &props)
            .and_then(|()| read::<Class>(Kind::Class, id, &props));
        match parsed {
            Ok(c) => classes.push(c),
            Err(e) => cx.quarantine(Kind::Class, id, e),
        }
    }
    if !classes.iter().any(|c| c.id == NONE_CLASS) {
        classes.insert(0, defaults.classes[0].clone());
        cx.repair("the None class was not valid: the default class is used".into());
    }

    // Resources.
    let resources = build_resources(view, base, &mut cx);

    // Objects.
    let mut objects: Vec<Object> = Vec::new();
    for (id, props) in ordered(view, Kind::Object, base.objects.keys().map(|o| o.0)) {
        let parsed = validate_entity(Kind::Object, id, &props)
            .and_then(|()| read::<Object>(Kind::Object, id, &props));
        match parsed {
            Ok(o) => objects.push(o),
            Err(e) => cx.quarantine(Kind::Object, id, e),
        }
    }

    // The settings of the document.
    let settings = unflatten(
        Kind::Document,
        &view
            .alive(Target::DOCUMENT)
            .map(|e| e.visible())
            .unwrap_or_default(),
    );
    let mut out = base.clone();
    out.georef = None;
    out.drawing = Default::default();
    if let Some(v) = settings.get("georef") {
        match serde_json::from_value(v.clone()) {
            Ok(g) => out.georef = Some(g),
            Err(e) => cx.quarantine(Kind::Document, Uid::NIL, format!("georef: {e}")),
        }
    }
    if let Some(v) = settings.get("drawing") {
        match serde_json::from_value(v.clone()) {
            Ok(d) => out.drawing = d,
            Err(e) => cx.quarantine(Kind::Document, Uid::NIL, format!("drawing: {e}")),
        }
    }

    out.layers = layers;
    out.classes = classes;
    out.resources = resources;
    out.active_layer = if out.layers.iter().any(|l| l.id == base.active_layer) {
        base.active_layer
    } else {
        FIRST_LAYER
    };
    out.active_class = if out.classes.iter().any(|c| c.id == base.active_class) {
        base.active_class
    } else {
        NONE_CLASS
    };

    // Repair the links, then check with the file format. A thing that still fails is
    // quarantined, and the links are repaired again (the thing may have been a group).
    let mut pending = objects;
    for _ in 0..=(pending.len() + out.resources.len() + 4) {
        let mut objs = std::mem::take(&mut pending);
        repair_links(&mut out, &mut objs, &mut cx);
        out.objects = objs.into_iter().map(|o| (o.id, o)).collect();
        match out.validate() {
            Ok(()) => break,
            Err(msg) => {
                blame(&mut out, msg, &mut cx);
                pending = std::mem::take(&mut out.objects).into_values().collect();
            }
        }
    }
    if out.validate().is_err() {
        // The last way out: the current document. It is valid, because it was checked before.
        cx.repair("the shared state could not be built: the open document is kept".into());
        out = base.clone();
    }
    Materialized {
        doc: out,
        quarantined: cx.quarantined,
        repairs: cx.repairs,
    }
}

/// The core refused the document with this message. Leaves out the thing that the message names
/// (an object, or a resource); if it names none, leaves out all objects.
fn blame(out: &mut Document, msg: String, cx: &mut Ctx) {
    let blamed = find_uid(&msg);
    if let Some(id) = blamed.filter(|id| out.objects.contains_key(&ObjectId(*id))) {
        out.objects.shift_remove(&ObjectId(id));
        cx.quarantine(Kind::Object, id, msg);
    } else if let Some(id) = blamed.filter(|id| out.resources.get(ResourceId(*id)).is_some()) {
        out.resources.remove(ResourceId(id));
        cx.quarantine(Kind::Resource, id, msg);
    } else {
        for (id, _) in out.objects.drain(..) {
            cx.quarantine(Kind::Object, id.0, format!("the document failed: {msg}"));
        }
    }
}

/// Reads the resources, names them, and breaks cycles.
fn build_resources(
    view: &Replica,
    base: &Document,
    cx: &mut Ctx,
) -> kadakas_core::resource::Resources {
    let mut list: Vec<Resource> = Vec::new();
    for (id, props) in ordered(view, Kind::Resource, base.resources.iter().map(|r| r.id.0)) {
        let parsed = validate_entity(Kind::Resource, id, &props)
            .and_then(|()| read::<Resource>(Kind::Resource, id, &props));
        match parsed {
            Ok(r) => list.push(r),
            Err(e) => cx.quarantine(Kind::Resource, id, e),
        }
    }
    // Names are repaired in the order of the IDs, so that every client gives the same names.
    let mut by_id: Vec<usize> = (0..list.len()).collect();
    by_id.sort_by_key(|i| list[*i].id.0);
    let mut taken: BTreeSet<(&'static str, String)> = BTreeSet::new();
    for i in by_id {
        let ty = list[i].body.type_name();
        let unknown = matches!(
            list[i].body,
            kadakas_core::resource::ResourceBody::Unknown(_)
        );
        let id = list[i].id.0;
        if list[i].name.trim().is_empty() {
            list[i].name = format!("Resource {}", short(id));
            cx.repair(format!("resource {id} had no name"));
        }
        if unknown {
            continue;
        }
        let mut name = list[i].name.clone();
        if !taken.insert((ty, name.to_lowercase())) {
            name = format!("{} ({})", list[i].name, short(id));
            let mut n = 2;
            while !taken.insert((ty, name.to_lowercase())) {
                name = format!("{} ({}-{n})", list[i].name, short(id));
                n += 1;
            }
            cx.repair(format!(
                "resource {id} got the name '{name}', the name was taken"
            ));
            list[i].name = name;
        }
    }
    let mut res = kadakas_core::resource::Resources::default();
    for r in list {
        res.push(r);
    }
    // Cycles: the smallest ID in the cycle goes to quarantine, until there is no cycle.
    while res.find_cycle().is_some() {
        let members: Vec<ResourceId> = res
            .iter()
            .filter(|r| {
                res.graphic(r.id).is_some_and(|g| {
                    g.refs()
                        .into_iter()
                        .any(|(x, _)| res.symbol_reaches(x, r.id))
                })
            })
            .map(|r| r.id)
            .collect();
        let Some(cut) = members.into_iter().min() else {
            break;
        };
        res.remove(cut);
        cx.quarantine(
            Kind::Resource,
            cut.0,
            "it uses itself through other resources",
        );
    }
    res
}

/// Makes the links between objects, layers, classes and resources valid.
fn repair_links(doc: &mut Document, objs: &mut Vec<Object>, cx: &mut Ctx) {
    let layer_kind: BTreeMap<LayerId, LayerKind> =
        doc.layers.iter().map(|l| (l.id, l.kind)).collect();
    let class_ids: BTreeSet<ClassId> = doc.classes.iter().map(|c| c.id).collect();
    let sheet = layer_kind
        .iter()
        .filter(|(_, k)| **k == LayerKind::Sheet)
        .map(|(id, _)| *id)
        .min();
    let resource_ids: BTreeSet<ResourceId> = doc.resources.iter().map(|r| r.id).collect();
    // The resources that attributes point to.
    for c in &mut doc.classes {
        drop_dangling(&mut c.attrs, &resource_ids);
    }
    let mut dropped: Vec<usize> = Vec::new();
    for (n, o) in objs.iter_mut().enumerate() {
        if !layer_kind.contains_key(&o.layer) {
            cx.repair(format!("object {} moved to the first layer", o.id));
            o.layer = FIRST_LAYER;
        }
        if !class_ids.contains(&o.class) {
            o.class = NONE_CLASS;
        }
        drop_dangling(&mut o.attrs, &resource_ids);
        if let Shape::Viewport(vp) = &mut o.shape {
            vp.layers.retain(|l| layer_kind.contains_key(l));
        }
        let on_sheet = layer_kind.get(&o.layer) == Some(&LayerKind::Sheet);
        let paper_only = matches!(o.shape, Shape::Viewport(_) | Shape::TitleBlock(_));
        if paper_only && !on_sheet {
            match sheet {
                Some(s) => {
                    cx.repair(format!("object {} moved to a sheet layer", o.id));
                    o.layer = s;
                }
                None => {
                    cx.quarantine(
                        Kind::Object,
                        o.id.0,
                        "it needs a sheet layer, and there is none",
                    );
                    dropped.push(n);
                }
            }
        } else if on_sheet && !allowed_on_sheet(&o.shape) {
            cx.repair(format!("object {} moved off a sheet layer", o.id));
            o.layer = FIRST_LAYER;
        }
    }
    for n in dropped.into_iter().rev() {
        objs.remove(n);
    }
    repair_groups(objs, cx);
}

fn drop_dangling(attrs: &mut kadakas_core::model::Attributes, ids: &BTreeSet<ResourceId>) {
    if attrs.fill_pattern.is_some_and(|r| !ids.contains(&r)) {
        attrs.fill_pattern = None;
    }
    if attrs.line_type.is_some_and(|r| !ids.contains(&r)) {
        attrs.line_type = None;
    }
}

/// Group parents: exist, are groups, share the layer, no cycle, not too deep.
fn repair_groups(objs: &mut [Object], cx: &mut Ctx) {
    let index: BTreeMap<ObjectId, usize> =
        objs.iter().enumerate().map(|(i, o)| (o.id, i)).collect();
    for i in 0..objs.len() {
        let Some(p) = objs[i].parent else { continue };
        let ok = index.get(&p).is_some_and(|&j| {
            matches!(objs[j].shape, Shape::Group { .. }) && objs[j].layer == objs[i].layer
        });
        if !ok {
            cx.repair(format!("object {} left its group", objs[i].id));
            objs[i].parent = None;
        }
    }
    // Cycles: walk the parents. In a cycle, the smallest ID loses its parent.
    loop {
        let mut cut: Option<ObjectId> = None;
        'scan: for start in 0..objs.len() {
            let mut seen: Vec<ObjectId> = Vec::new();
            let mut cur = Some(objs[start].id);
            while let Some(id) = cur {
                if let Some(at) = seen.iter().position(|s| *s == id) {
                    cut = seen[at..].iter().min().copied();
                    break 'scan;
                }
                seen.push(id);
                cur = objs[index[&id]].parent;
            }
        }
        let Some(id) = cut else { break };
        cx.repair(format!(
            "the groups of object {id} formed a cycle: it left its group"
        ));
        objs[index[&id]].parent = None;
    }
    // Depth: an object with too many ancestors loses its parent (smallest ID first).
    loop {
        let mut deep: Option<ObjectId> = None;
        for o in objs.iter() {
            let mut n = 0;
            let mut cur = o.parent;
            while let Some(p) = cur {
                n += 1;
                cur = objs[index[&p]].parent;
            }
            if n >= MAX_DEPTH && deep.is_none_or(|d| o.id < d) {
                deep = Some(o.id);
            }
        }
        let Some(id) = deep else { break };
        cx.repair(format!(
            "object {id} was in groups that are too deep: it left its group"
        ));
        objs[index[&id]].parent = None;
    }
}

#[cfg(test)]
mod tests;
