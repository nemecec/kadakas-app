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
            || kept.len() >= MAX_EXCERPT_OBJECTS
            || serde_json::to_value(o).is_ok_and(|v| has_image(&v, None));
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
            if has_image(&v, None) {
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

/// Maps of free values that belong to records and title blocks: every text in them goes.
const MAP_KEYS: [&str; 7] = [
    "fields",
    "values",
    "record",
    "records",
    "properties",
    "props",
    "data",
];

/// Keys whose text is part of the structure of a resource, not a name or a note.
const STRUCTURE_KEYS: [&str; 14] = [
    "type", "kind", "op", "unit", "units", "mode", "align", "anchor", "style", "fill", "format",
    "shape", "dash", "side",
];

fn is_uid_text(s: &str) -> bool {
    Uid::parse(s).is_some()
}

fn blank_names(v: &mut Value) {
    match v {
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if NAME_KEYS.contains(&k.as_str()) && x.is_string() {
                    *x = Value::String(String::new());
                } else if MAP_KEYS.contains(&k.as_str()) {
                    blank_all(x);
                } else {
                    blank_names(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(blank_names),
        _ => {}
    }
}

/// Blanks every text, at any depth, except object ids.
fn blank_all(v: &mut Value) {
    match v {
        Value::String(s) => {
            if !is_uid_text(s) {
                s.clear();
            }
        }
        Value::Array(a) => a.iter_mut().for_each(blank_all),
        Value::Object(o) => o.values_mut().for_each(blank_all),
        _ => {}
    }
}

/// Blanks the texts of a resource body: worksheet cells, symbol texts, defaults of record
/// fields, notes and so on. What stays: object ids and the words that set the structure
/// (`type`, `kind`, units).
fn blank_resource(v: &mut Value, key: Option<&str>) {
    match v {
        Value::String(s) => {
            if !is_uid_text(s) && !key.is_some_and(|k| STRUCTURE_KEYS.contains(&k)) {
                s.clear();
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| blank_resource(x, key)),
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                blank_resource(x, Some(k));
            }
        }
        _ => {}
    }
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

/// True if a text starts like a picture file in base64 (PNG, JPEG, GIF, WebP) or is a data address
/// of a picture.
fn image_text(s: &str) -> bool {
    ["iVBOR", "/9j/", "R0lGOD", "UklGR", "data:image"]
        .iter()
        .any(|m| s.starts_with(m))
}

/// True if a value holds a picture: a text that starts like one, or a long text (over 64
/// characters) under a key that names a picture.
fn has_image(v: &Value, key: Option<&str>) -> bool {
    const KEYS: [&str; 9] = [
        "png",
        "jpeg",
        "jpg",
        "image",
        "thumb",
        "thumbnail",
        "picture",
        "base64",
        "bytes",
    ];
    match v {
        Value::String(s) => {
            image_text(s) || (s.len() > 64 && key.is_some_and(|k| KEYS.contains(&k)))
        }
        Value::Array(a) => a.iter().any(|x| has_image(x, key)),
        Value::Object(o) => o.iter().any(|(k, x)| has_image(x, Some(k))),
        _ => false,
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
            if matches!(o.shape, Shape::Group { .. } | Shape::TitleBlock(_)) {
                continue;
            }
            // The frame of a viewport is on paper, in mm of the sheet. A viewport also names the
            // model point it shows: that point moves.
            if let Shape::Viewport(vp) = &mut o.shape {
                vp.center += d;
                continue;
            }
            o.shape.translate(d);
            kadakas_core::hardscape::translate_object_data(o, d);
        }
    }
    let mut v = serde_json::to_value(&doc).map_err(|e| e.to_string())?;
    if p.remove_names {
        strip_names(&mut v);
        // Texts in resources can break a rule (a name that must not be empty). Then the
        // resources go and the objects stay.
        if Document::from_value(v.clone()).is_err()
            && let Some(o) = v.as_object_mut()
        {
            o.remove("resources");
        }
    }
    // The result must open again.
    Document::from_value(v.clone())
        .map_err(|e| format!("the options leave an invalid document: {e}"))?;
    serde_json::to_string(&v).map_err(|e| e.to_string())
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
                    if key == "resources"
                        && let Some(body) = x.get_mut("body")
                    {
                        blank_resource(body, None);
                    }
                    // Any other text of a layer or a class (a note, a label) goes too.
                    if key != "resources"
                        && let Some(m) = x.as_object_mut()
                    {
                        for (k, y) in m.iter_mut() {
                            if k != "name"
                                && y.is_string()
                                && !is_uid_text(y.as_str().unwrap_or(""))
                                && !matches!(k.as_str(), "id" | "kind")
                            {
                                *y = Value::String(String::new());
                            }
                        }
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

/// The middle of the bounds of the model objects, if there is one. The viewports and title blocks
/// of sheets are on paper and do not count.
fn center(doc: &Document) -> Option<DVec2> {
    let (lo, hi) = doc
        .objects
        .values()
        .filter(|o| !matches!(o.shape, Shape::Viewport(_) | Shape::TitleBlock(_)))
        .map(|o| doc.object_bounds(o))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))?;
    Some((lo + hi) / 2.0)
}

// ---- the call log and the log lines

/// Keys that hold a place or a coordinate system.
const PLACE_KEYS: [&str; 9] = [
    "crs",
    "epsg",
    "srid",
    "wkt",
    "georef",
    "georeference",
    "projection_string",
    "datum",
    "proj",
];

/// Keys whose number is a coordinate.
const COORD_KEYS: [&str; 10] = [
    "x",
    "y",
    "z",
    "lat",
    "lon",
    "lng",
    "latitude",
    "longitude",
    "easting",
    "northing",
];

fn crs_text(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("epsg:") || l.contains("projcs") || l.contains("geogcs") || l.contains("+proj=")
}

/// Replaces `[1.0, 2.0]` and `(1, 2, 3)` in a text by a note. A group needs a comma and numbers only.
pub fn hide_coordinates_in_text(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '[' | '(')
            && let Some(len) = chars[i + 1..]
                .iter()
                .take(120)
                .position(|x| matches!(x, ']' | ')'))
        {
            let inner: String = chars[i + 1..i + 1 + len].iter().collect();
            let numbers = inner
                .split(',')
                .filter(|p| !p.trim().is_empty())
                .all(|p| p.trim().parse::<f64>().is_ok());
            if inner.contains(',') && numbers {
                out.push(c);
                out.push_str("<coordinates removed>");
                out.push(chars[i + 1 + len]);
                i += len + 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_point(a: &[Value]) -> bool {
    (2..=3).contains(&a.len()) && a.iter().all(Value::is_number)
}

fn private_value(v: &mut Value, p: Privacy) {
    match v {
        Value::String(s) => {
            if p.remove_georef && crs_text(s) {
                *s = "<crs removed>".into();
            } else if p.move_to_origin {
                *s = hide_coordinates_in_text(s);
            }
        }
        Value::Array(a) => {
            if p.move_to_origin && is_point(a) {
                *v = Value::String("<coordinates removed>".into());
            } else {
                a.iter_mut().for_each(|x| private_value(x, p));
            }
        }
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                let key = k.to_ascii_lowercase();
                if p.remove_georef && PLACE_KEYS.contains(&key.as_str()) {
                    *x = Value::String("<crs removed>".into());
                } else if p.move_to_origin && COORD_KEYS.contains(&key.as_str()) && x.is_number() {
                    *x = Value::String("<coordinates removed>".into());
                } else {
                    private_value(x, p);
                }
            }
        }
        _ => {}
    }
}

/// The call log with the options applied: georeference and CRS calls and values go, coordinate
/// pairs are hidden, and with `remove_names` the free texts are blank. A call about the place is
/// dropped.
pub fn calls_privacy(json: &str, p: Privacy) -> String {
    let Ok(Value::Array(calls)) = serde_json::from_str::<Value>(json) else {
        return json.to_string();
    };
    let mut kept = Vec::new();
    for mut c in calls {
        let method = c["method"].as_str().unwrap_or("").to_ascii_lowercase();
        if p.remove_georef && (method.contains("georef") || method.contains("crs")) {
            continue;
        }
        private_value(&mut c, p);
        if p.remove_names {
            blank_names(&mut c);
        }
        kept.push(c);
    }
    Value::Array(kept).to_string()
}

/// The log lines with the options applied: lines about the place go, coordinate pairs are hidden.
pub fn log_privacy(text: &str, p: Privacy) -> String {
    text.lines()
        .filter(|l| {
            if !p.remove_georef {
                return true;
            }
            let l = l.to_ascii_lowercase();
            !(l.contains("georef") || l.contains("crs") || l.contains("epsg") || l.contains("srid"))
        })
        .map(|l| {
            if p.move_to_origin {
                hide_coordinates_in_text(l)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
