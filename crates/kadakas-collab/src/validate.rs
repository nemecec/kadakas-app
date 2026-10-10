//! Validation of one thing, as the core checks a command or a loaded file.
//!
//! A thing has properties (JSON). [`validate_entity`] tells if they make a valid object, layer,
//! class, resource or document setting: they must read as the right type, numbers must be finite
//! and in range, and the shape must pass the checks of the file format. The same function runs in
//! three places:
//!
//! - a **server** can run it on the things that a transaction touches, after a tentative apply,
//!   and refuse the transaction with `invalid_txn` (the test sequencer does this);
//! - [`crate::materialize::materialize`] runs it on every received thing and quarantines the ones
//!   that fail, so one bad thing never blocks a client;
//! - a client can run it on its own edits.
//!
//! The checks do not look at other things (references between things are repaired when the
//! document is built).

use kadakas_core::model::{
    Class, Document, Layer, MAX_COORD, Object, Shape, VertexKind, arc_sweep,
};
use kadakas_core::resource::Resource;
use kadakas_core::uid::Uid;
use serde_json::{Map, Value};

use crate::op::Kind;

/// Reads a thing from its properties and its ID.
pub(crate) fn with_id(id: Uid, props: &Map<String, Value>) -> Value {
    let mut m = props.clone();
    m.insert("id".into(), id.into());
    Value::Object(m)
}

/// Why an object is not valid, or `None`. These are the checks of `Document::validate` that look
/// at one object alone.
pub fn object_problem(o: &Object) -> Option<String> {
    let bad_arc = matches!(o.shape, Shape::Arc { start, end, .. } if arc_sweep(start, end) == 0.0);
    let bad_vertex = o.shape.vertices().is_some_and(|vs| {
        vs.iter()
            .any(|v| matches!(v.kind, VertexKind::Arc { radius } if radius < 0.0))
    });
    if !o.is_finite()
        || !o.shape.is_in_range()
        || !o.shape.has_valid_thickness()
        || o.shape.problem().is_some()
        || bad_arc
        || bad_vertex
    {
        return Some(
            o.shape
                .problem()
                .unwrap_or_else(|| "the object has an invalid shape".into()),
        );
    }
    None
}

/// Why a layer is not valid, or `None`.
pub fn layer_problem(l: &Layer) -> Option<String> {
    if !l.elevation.is_finite() || l.elevation.abs() > MAX_COORD {
        return Some("the elevation is out of range".into());
    }
    if !l.scale.is_finite() {
        return Some("the scale must be a finite number".into());
    }
    None
}

/// Checks the properties of one thing. `Ok` if they make a valid thing of this kind.
pub fn validate_entity(kind: Kind, id: Uid, props: &Map<String, Value>) -> Result<(), String> {
    if matches!(kind, Kind::Object | Kind::Layer)
        && let Some(k) = props.get("order")
        && !k.as_str().is_some_and(crate::order::valid_key)
    {
        return Err(format!(
            "order must be a key of the digits 0-9A-Za-z (no 0 at the end, at most {} characters)",
            crate::order::MAX_KEY_LEN
        ));
    }
    let props = &crate::docsync::unflatten(kind, props);
    let full = with_id(id, props);
    let read = |e: serde_json::Error| format!("not a valid {kind:?}: {e}");
    match kind {
        Kind::Object => {
            let o: Object = serde_json::from_value(full).map_err(read)?;
            match object_problem(&o) {
                Some(p) => Err(p),
                None => Ok(()),
            }
        }
        Kind::Layer => {
            let l: Layer = serde_json::from_value(full).map_err(read)?;
            layer_problem(&l).map_or(Ok(()), Err)
        }
        Kind::Class => {
            let c: Class = serde_json::from_value(full).map_err(read)?;
            if c.attrs.is_finite() {
                Ok(())
            } else {
                Err("class attributes must be finite".into())
            }
        }
        Kind::Resource => {
            let r: Resource = serde_json::from_value(full).map_err(read)?;
            // An empty or repeated name is not an error here: it is repaired when the document
            // is built.
            r.body.validate()
        }
        Kind::Document => {
            let mut doc = Document::default();
            for (k, v) in props {
                match k.as_str() {
                    "georef" => {
                        doc.georef = Some(serde_json::from_value(v.clone()).map_err(read)?);
                    }
                    "drawing" => {
                        doc.drawing = serde_json::from_value(v.clone()).map_err(read)?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
    }
}

/// Finds the first ID (UUID text) in a message.
pub(crate) fn find_uid(msg: &str) -> Option<Uid> {
    let b = msg.as_bytes();
    (0..b.len().saturating_sub(35)).find_map(|i| {
        if b[i + 8] == b'-' && msg.is_char_boundary(i) && msg.is_char_boundary(i + 36) {
            Uid::parse(&msg[i..i + 36])
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn rect() -> Map<String, Value> {
        props(json!({
            "layer": Uid::from_num(1), "class": Uid::from_num(0),
            "shape": {"kind": "rect", "min": [0, 0], "max": [5, 5]}
        }))
    }

    #[test]
    fn objects() {
        let id = Uid::from_num(5);
        assert_eq!(validate_entity(Kind::Object, id, &rect()), Ok(()));
        let mut bad = rect();
        bad.insert("shape".into(), json!({"kind": "rect", "min": "x"}));
        assert!(validate_entity(Kind::Object, id, &bad).is_err());
        let mut far = rect();
        far.insert(
            "shape".into(),
            json!({"kind": "rect", "min": [0, 0], "max": [5e12, 5]}),
        );
        assert!(validate_entity(Kind::Object, id, &far).is_err());
        let mut arc = rect();
        arc.insert(
            "shape".into(),
            json!({"kind": "arc", "center": [0, 0], "radius": 5, "start": 1.0, "end": 1.0}),
        );
        assert!(validate_entity(Kind::Object, id, &arc).is_err());
        let mut neg = rect();
        neg.insert(
            "shape".into(),
            json!({"kind": "polyline", "closed": false,
                   "points": [[0, 0], {"p": [5, 0], "kind": "arc", "radius": -3}, [9, 9]]}),
        );
        assert!(validate_entity(Kind::Object, id, &neg).is_err());
        assert!(validate_entity(Kind::Object, id, &Map::new()).is_err());
    }

    #[test]
    fn layers_classes_resources_settings() {
        let id = Uid::from_num(7);
        let layer = props(json!({"name": "L", "visible": true, "locked": false,
                                 "elevation": 0.0, "scale": 100.0}));
        assert_eq!(validate_entity(Kind::Layer, id, &layer), Ok(()));
        let mut far = layer.clone();
        far.insert("elevation".into(), json!(1e12));
        assert!(validate_entity(Kind::Layer, id, &far).is_err());
        assert!(validate_entity(Kind::Layer, id, &Map::new()).is_err());
        let class = props(json!({"name": "C", "visible": true, "attrs": {}}));
        assert_eq!(validate_entity(Kind::Class, id, &class), Ok(()));
        let res = props(json!({"name": "W", "body": {"type": "worksheet"}}));
        assert_eq!(validate_entity(Kind::Resource, id, &res), Ok(()));
        // An empty name is repaired when the document is built, not refused.
        let res = props(json!({"name": " ", "body": {"type": "worksheet"}}));
        assert_eq!(validate_entity(Kind::Resource, id, &res), Ok(()));
        let res = props(json!({"name": "D", "body": {"type": "plant_style", "common_name": 5}}));
        assert!(validate_entity(Kind::Resource, id, &res).is_err());
        assert_eq!(
            validate_entity(Kind::Document, Uid::NIL, &Map::new()),
            Ok(())
        );
        let bad = props(json!({"drawing": 5}));
        assert!(validate_entity(Kind::Document, Uid::NIL, &bad).is_err());
        let bad = props(json!({"georef": 5}));
        assert!(validate_entity(Kind::Document, Uid::NIL, &bad).is_err());
        // The key of the stacking order: a good key passes, a hostile one does not.
        let mut o = rect();
        o.insert("order".into(), json!("a0V"));
        assert_eq!(validate_entity(Kind::Object, id, &o), Ok(()));
        for hostile in [
            json!("~"),
            json!(""),
            json!("a0V0"),
            json!(5),
            json!("a".repeat(2000)),
        ] {
            o.insert("order".into(), hostile);
            assert!(validate_entity(Kind::Object, id, &o).is_err());
        }
        let mut l = layer.clone();
        l.insert("order".into(), json!("~"));
        assert!(validate_entity(Kind::Layer, id, &l).is_err());
    }

    #[test]
    fn finds_ids_in_messages() {
        let u = Uid::from_num(0x1234);
        assert_eq!(find_uid(&format!("object {u} has a problem")), Some(u));
        assert_eq!(find_uid("no id here"), None);
        assert_eq!(find_uid("é"), None);
    }
}
