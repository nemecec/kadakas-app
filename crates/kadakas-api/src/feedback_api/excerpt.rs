//! The model excerpt of a feedback report: a small, valid `.kad` document with some objects and
//! what they need (their layers, classes, resources and parent groups).
//!
//! `build` makes it when the report is made. The privacy options (`apply_privacy`) work on the
//! text that is sent, in the review, so the person sees the result.

use std::collections::BTreeSet;

use glam::DVec2;
use kadakas_core::model::*;
use kadakas_core::uid::Uid;
use kadakas_feedback::{MAX_EXCERPT_BYTES, MAX_EXCERPT_OBJECTS, Privacy};
use serde_json::Value;

/// A resource with a text this long holds a picture. Pictures are not part of an excerpt.
const PICTURE_TEXT: usize = 20_000;

pub struct Excerpt {
    /// The text of the document.
    pub json: String,
    pub objects: usize,
    /// Objects left out: unknown shapes, pictures under the drawing, and the ones over the cap.
    pub omitted: usize,
}

/// Every id in a JSON value that looks like a UUID, up to `max`.
pub fn uids_in(v: &Value, max: usize) -> BTreeSet<Uid> {
    fn walk(v: &Value, out: &mut BTreeSet<Uid>, max: usize, budget: &mut usize) {
        if *budget == 0 || out.len() >= max {
            return;
        }
        *budget -= 1;
        match v {
            Value::String(s) => {
                if let Some(u) = Uid::parse(s) {
                    out.insert(u);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out, max, budget)),
            Value::Object(o) => o.values().for_each(|x| walk(x, out, max, budget)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(v, &mut out, max, &mut 2000);
    out
}

fn has_picture(v: &Value) -> bool {
    match v {
        Value::String(s) => s.len() > PICTURE_TEXT,
        Value::Array(a) => a.iter().any(has_picture),
        Value::Object(o) => o.values().any(has_picture),
        _ => false,
    }
}

/// Builds an excerpt of the named objects. Groups bring their members, and members bring their
/// parent groups. Returns `None` if no object is left.
pub fn build(doc: &Document, ids: &[ObjectId]) -> Option<Excerpt> {
    let mut want: BTreeSet<ObjectId> = BTreeSet::new();
    for id in doc.leaves(ids) {
        want.insert(id);
    }
    for id in ids {
        if doc.object(*id).is_some() {
            want.insert(*id);
        }
    }
    // Parent groups, so that the document stays valid.
    let mut parents = Vec::new();
    for id in &want {
        let mut cur = doc.object(*id).and_then(|o| o.parent);
        let mut depth = 0;
        while let Some(p) = cur.filter(|_| depth < 16) {
            parents.push(p);
            cur = doc.object(p).and_then(|o| o.parent);
            depth += 1;
        }
    }
    want.extend(parents);

    let mut omitted = 0;
    let mut kept: Vec<&Object> = Vec::new();
    for o in doc.objects.values().filter(|o| want.contains(&o.id)) {
        // Unknown shapes may hold coordinates that no option can move, and an underlay is a
        // picture of the place. Both stay out, and so do objects over the cap.
        let left_out = matches!(o.shape, Shape::Unknown { .. } | Shape::Underlay(_))
            || kept.len() >= MAX_EXCERPT_OBJECTS;
        if left_out {
            omitted += 1;
        } else {
            kept.push(o);
        }
    }
    loop {
        let ex = assemble(doc, &kept)?;
        let json = serde_json::to_string(&ex).ok()?;
        if json.len() <= MAX_EXCERPT_BYTES {
            return Some(Excerpt {
                json,
                objects: kept.len(),
                omitted,
            });
        }
        // Too big: keep the first half.
        let n = kept.len() / 2;
        omitted += kept.len() - n;
        kept.truncate(n);
    }
}

fn assemble(doc: &Document, kept: &[&Object]) -> Option<Document> {
    if kept.is_empty() {
        return None;
    }
    let ids: BTreeSet<ObjectId> = kept.iter().map(|o| o.id).collect();
    let mut ex = Document::default();
    ex.objects = kept
        .iter()
        .map(|o| {
            let mut o = (*o).clone();
            o.extra.clear();
            // A parent that was left out would break the document.
            if o.parent.is_some_and(|p| !ids.contains(&p)) {
                o.parent = None;
            }
            (o.id, o)
        })
        .collect();
    let layers: BTreeSet<_> = kept.iter().map(|o| o.layer).collect();
    let classes: BTreeSet<_> = kept.iter().map(|o| o.class).collect();
    ex.layers = doc
        .layers
        .iter()
        .filter(|l| layers.contains(&l.id))
        .cloned()
        .map(|mut l| {
            l.extra.clear();
            l
        })
        .collect();
    ex.classes = doc
        .classes
        .iter()
        .filter(|c| classes.contains(&c.id) || c.id == NONE_CLASS)
        .cloned()
        .map(|mut c| {
            c.extra.clear();
            c
        })
        .collect();
    ex.active_layer = ex.layers.first()?.id;
    ex.active_class = NONE_CLASS;
    ex.georef = doc.georef.clone();
    ex.drawing.sheets = doc
        .drawing
        .sheets
        .iter()
        .filter(|(l, _)| layers.contains(l))
        .map(|(l, s)| (*l, s.clone()))
        .collect();

    // Resources that the objects name, and those that these name in turn.
    let mut need = uids_in(
        &serde_json::to_value(ex.objects.values().collect::<Vec<_>>()).ok()?,
        5000,
    );
    let mut chosen: BTreeSet<Uid> = BTreeSet::new();
    for _ in 0..5 {
        let before = chosen.len();
        for r in doc.resources.iter() {
            let rid = r.id.0;
            if !need.contains(&rid) || chosen.contains(&rid) {
                continue;
            }
            let v = serde_json::to_value(r).ok()?;
            if has_picture(&v) {
                continue;
            }
            chosen.insert(rid);
            need.extend(uids_in(&v, 5000));
        }
        if chosen.len() == before {
            break;
        }
    }
    for r in doc.resources.iter().filter(|r| chosen.contains(&r.id.0)) {
        ex.resources.push(r.clone());
    }
    Some(ex)
}

const NAME_KEYS: [&str; 10] = [
    "name",
    "note",
    "notes",
    "text",
    "label",
    "description",
    "comment",
    "title",
    "tag",
    "remarks",
];

fn blank_names(v: &mut Value) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if NAME_KEYS.contains(&k.as_str()) && x.is_string() {
                    *x = Value::String(String::new());
                } else {
                    blank_names(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(blank_names),
        _ => {}
    }
}

/// Applies the privacy options to the text of an excerpt. The answer is a valid document again,
/// or an error if the options leave no valid document.
pub fn apply_privacy(json: &str, p: Privacy) -> Result<String, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut doc = Document::from_value(value).map_err(|e| e.to_string())?;
    if p.remove_georef {
        doc.georef = None;
    }
    if p.move_to_origin
        && let Some(c) = center(&doc)
    {
        let d = -c.round();
        for o in doc.objects.values_mut() {
            if !matches!(o.shape, Shape::Group { .. }) {
                o.shape.translate(d);
                kadakas_core::hardscape::translate_object_data(o, d);
            }
        }
    }
    let mut v = serde_json::to_value(&doc).map_err(|e| e.to_string())?;
    if p.remove_names {
        strip_names(&mut v);
    }
    // The result must open again.
    Document::from_value(v.clone())
        .map_err(|e| format!("the options leave an invalid document: {e}"))?;
    serde_json::to_string(&v).map_err(|e| e.to_string())
}

/// Blanks the free texts in the call log (the same keys as in an excerpt).
pub fn blank_names_in(json: &str) -> String {
    match serde_json::from_str::<Value>(json) {
        Ok(mut v) => {
            blank_names(&mut v);
            v.to_string()
        }
        Err(_) => json.to_string(),
    }
}

fn strip_names(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        for (key, prefix) in [
            ("layers", "Layer"),
            ("classes", "Class"),
            ("resources", "Resource"),
        ] {
            if let Some(list) = o.get_mut(key).and_then(Value::as_array_mut) {
                for (i, x) in list.iter_mut().enumerate() {
                    let keep = key == "classes" && x["id"] == Value::String(NONE_CLASS.to_string());
                    if !keep && x.get("name").is_some() {
                        x["name"] = Value::String(format!("{prefix} {}", i + 1));
                    }
                }
            }
        }
        if let Some(list) = o.get_mut("objects").and_then(Value::as_array_mut) {
            for x in list {
                if let Some(m) = x.as_object_mut() {
                    // Record data and attributes with free text.
                    m.remove("data");
                }
                blank_names(x);
            }
        }
        if let Some(d) = o.get_mut("drawing") {
            blank_names(d);
        }
    }
}

/// The middle of the bounds of the objects, if there is one.
fn center(doc: &Document) -> Option<DVec2> {
    let (lo, hi) = doc
        .objects
        .values()
        .map(|o| doc.object_bounds(o))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))?;
    Some((lo + hi) / 2.0)
}

/// What the review shows about one object.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectBrief {
    pub id: ObjectId,
    pub kind: String,
    pub layer: String,
    pub min: [f64; 2],
    pub max: [f64; 2],
}

/// The objects of an excerpt text, for the review list and the small plan.
pub fn objects_of(json: &str) -> Vec<ObjectBrief> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Ok(doc) = Document::from_value(v) else {
        return Vec::new();
    };
    doc.objects
        .values()
        .map(|o| {
            let (lo, hi) = doc.object_bounds(o);
            let fin = |p: DVec2| {
                if p.is_finite() {
                    [p.x, p.y]
                } else {
                    [0.0, 0.0]
                }
            };
            ObjectBrief {
                id: o.id,
                kind: o.shape.kind().to_string(),
                layer: doc
                    .layers
                    .iter()
                    .find(|l| l.id == o.layer)
                    .map(|l| l.name.clone())
                    .unwrap_or_default(),
                min: fin(lo),
                max: fin(hi),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn session() -> (Session, Vec<ObjectId>) {
        let mut s = Session::with_sequential_ids();
        let mut ids = Vec::new();
        for (cmd, _) in [
            (
                json!({"op": "add_object", "shape": {"kind": "rect", "min": [500000, 600000], "max": [502000, 603000]}}),
                0,
            ),
            (
                json!({"op": "add_object", "shape": {"kind": "text", "at": [500500, 600500], "text": "Mrs Smith's garden", "height": 200}}),
                0,
            ),
            (
                json!({"op": "add_object", "shape": {"kind": "plant", "at": [501000, 601000], "species": "Acer", "spread": 3000, "height": 8000}}),
                0,
            ),
        ] {
            let r = s.handle("execute", json!({"command": cmd})).unwrap();
            ids.push(ObjectId(
                kadakas_core::uid::Uid::parse(r["created"][0].as_str().unwrap()).unwrap(),
            ));
        }
        s.handle(
            "execute",
            json!({"command": {"op": "add_layer", "name": "Smith house", "elevation": 0}}),
        )
        .unwrap();
        let g = json!({"command": {"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000.0, 6672000.0]}}});
        let _ = s.handle("execute", g);
        (s, ids)
    }

    #[test]
    fn the_excerpt_is_a_valid_small_document() {
        let (s, ids) = session();
        let ex = build(s.editor.doc(), &ids[..2]).unwrap();
        assert_eq!((ex.objects, ex.omitted), (2, 0));
        let doc = Document::from_value(serde_json::from_str(&ex.json).unwrap()).unwrap();
        assert_eq!(doc.objects.len(), 2);
        // Only the layers and classes that the objects use.
        assert_eq!(doc.layers.len(), 1);
        assert!(build(s.editor.doc(), &[]).is_none());
        assert!(build(s.editor.doc(), &[ObjectId::from_num(999)]).is_none());
    }

    #[test]
    fn privacy_moves_to_the_origin_and_drops_the_georeference() {
        let (s, ids) = session();
        let mut doc = s.editor.doc().clone();
        doc.georef = Some(kadakas_core::georef::Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385000.0, 6672000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        });
        let ex = build(&doc, &ids).unwrap();
        assert!(ex.json.contains("385000"));
        let out = apply_privacy(&ex.json, Privacy::default()).unwrap();
        assert!(!out.contains("385000") && !out.contains("6672000"), "{out}");
        assert!(!out.contains("georef"), "{out}");
        for o in objects_of(&out) {
            for c in o.min.iter().chain(&o.max) {
                assert!(c.abs() < 5000.0, "{o:?}");
            }
        }
        // With both options off the text is the same document.
        let off = Privacy {
            move_to_origin: false,
            remove_georef: false,
            remove_names: false,
        };
        let same = apply_privacy(&ex.json, off).unwrap();
        assert!(same.contains("385000") && same.contains("500000"));
        // Only the georeference.
        let g = Privacy {
            move_to_origin: false,
            remove_georef: true,
            remove_names: false,
        };
        let t = apply_privacy(&ex.json, g).unwrap();
        assert!(!t.contains("385000") && t.contains("500000"));
    }

    #[test]
    fn privacy_removes_names_notes_and_records() {
        let (mut s, ids) = session();
        s.handle(
            "execute",
            json!({"command": {"op": "add_resource", "name": "Smith soil", "body": {"type": "record_format", "fields": [{"name": "Depth", "type": "dimension", "default": 300}]}}}),
        )
        .unwrap();
        let mut doc = s.editor.doc().clone();
        let rid = doc.resources.iter().next().unwrap().id;
        doc.objects
            .get_mut(&ids[0])
            .unwrap()
            .data
            .insert("owner".into(), json!("Mrs Smith, 12 Elm Road"));
        doc.objects
            .get_mut(&ids[0])
            .unwrap()
            .data
            .insert("record".into(), json!(rid.to_string()));
        for l in doc.layers.iter_mut() {
            l.name = "Smith house".into();
        }
        let ex = build(&doc, &ids).unwrap();
        assert!(ex.json.contains("Mrs Smith"));
        assert!(ex.json.contains("Smith house") && ex.json.contains("Smith soil"));
        let p = Privacy {
            remove_names: true,
            ..Privacy::default()
        };
        let out = apply_privacy(&ex.json, p).unwrap();
        for leak in ["Smith", "Elm Road", "owner"] {
            assert!(!out.contains(leak), "{leak} in {out}");
        }
        assert!(out.contains("Layer 1"));
        // Without the option the names stay.
        assert!(
            apply_privacy(&ex.json, Privacy::default())
                .unwrap()
                .contains("Smith house")
        );
    }

    #[test]
    fn unknown_shapes_pictures_and_extras_are_left_out() {
        let (s, ids) = session();
        let mut doc = s.editor.doc().clone();
        let u = Object::new(
            ObjectId::from_num(900),
            doc.active_layer,
            NONE_CLASS,
            Shape::Unknown {
                kind: "future".into(),
                raw: json!({"kind": "future", "at": [385000, 6672000]}),
            },
        );
        doc.objects.insert(u.id, u);
        doc.objects
            .get_mut(&ids[0])
            .unwrap()
            .extra
            .insert("secret_place".into(), json!([385000, 6672000]));
        let mut all = ids.clone();
        all.push(ObjectId::from_num(900));
        let ex = build(&doc, &all).unwrap();
        assert_eq!((ex.objects, ex.omitted), (3, 1));
        assert!(!ex.json.contains("future") && !ex.json.contains("secret_place"));
    }

    #[test]
    fn groups_bring_members_and_the_size_is_capped() {
        let (mut s, ids) = session();
        let g = s
            .handle("execute", json!({"command": {"op": "group_objects", "ids": [ids[0].to_string(), ids[2].to_string()]}}))
            .map(|r| Uid::parse(r["created"][0].as_str().unwrap()).map(ObjectId));
        if let Ok(Some(g)) = g {
            let ex = build(s.editor.doc(), &[g]).unwrap();
            assert_eq!(ex.objects, 3);
            Document::from_value(serde_json::from_str(&ex.json).unwrap()).unwrap();
            // A member alone brings its group.
            let ex = build(s.editor.doc(), &[ids[2]]).unwrap();
            assert!(ex.objects >= 2);
        }
        // Many objects: at most the cap.
        let mut s = Session::with_sequential_ids();
        let mut all = Vec::new();
        for i in 0..(MAX_EXCERPT_OBJECTS + 20) {
            let r = s
                .handle("execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [i * 10, 0], "radius": 3}}}))
                .unwrap();
            all.push(ObjectId(
                Uid::parse(r["created"][0].as_str().unwrap()).unwrap(),
            ));
        }
        let ex = build(s.editor.doc(), &all).unwrap();
        assert_eq!(ex.objects, MAX_EXCERPT_OBJECTS);
        assert_eq!(ex.omitted, 20);
    }

    #[test]
    fn a_bad_text_is_an_error_and_ids_are_found() {
        assert!(apply_privacy("{", Privacy::default()).is_err());
        assert!(apply_privacy("{\"a\":1}", Privacy::default()).is_err());
        assert!(objects_of("{").is_empty());
        assert!(objects_of("{\"a\":1}").is_empty());
        let id = Uid::from_num(5).to_string();
        let v = json!({"a": [id.clone(), "nope", {"b": id}]});
        assert_eq!(uids_in(&v, 10).len(), 1);
    }
}
