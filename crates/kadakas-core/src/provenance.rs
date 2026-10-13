//! Where imported objects came from (IOP-22).
//!
//! An importer of DXF or DWG files keeps, for each object, a small tag in `Object::data[KEY]`:
//! the source file, the DXF handle of the entity, and a hash of the entity content. A layer that
//! the import made keeps the source in `Layer::extra[LAYER_KEY]`. A later import of a new revision
//! of the file uses the tags to match its entities to the objects (see `kadakas_io::dxf::revision`).
//!
//! Why object data: it is the place where importers already keep per-object facts (the GeoJSON
//! and Shapefile importers, the rotation of imported text). It is saved with the object, copied
//! with `Clone`, and synced by collaboration as the property `data.import`, with no new field in
//! the file format. A record format is for fields that people edit, so it does not fit.
//!
//! The tag says where the object came from, not what it is now. A copy is new work, so the
//! commands that copy objects drop the tag (`strip_copy`). A moved object keeps it: the hash
//! then differs from the entity, and that tells an update that the user edited the object.
//!
//! The tag comes from files and from other users, so it is checked (`check`) when a command sets
//! it and when a document is loaded (`repair`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Document;

/// The key in `Object::data` that holds the tag.
pub const KEY: &str = "import";
/// The key in `Layer::extra` that holds the source of an imported layer.
pub const LAYER_KEY: &str = "import_source";
/// Longest text in a tag (a file name or a block name), in characters.
pub const MAX_TEXT: usize = 256;
/// Longest DXF handle, in characters. Handles are hexadecimal; AutoCAD uses at most 16 digits.
pub const MAX_HANDLE: usize = 32;
/// Most INSERTs between the model and an object (blocks inside blocks).
pub const MAX_DEPTH: usize = 16;
/// Most notes that `repair` makes. More are counted in one last note.
const MAX_NOTES: usize = 20;

/// A block reference that an object came through.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InsertRef {
    /// Handle of the INSERT entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// Hash of the INSERT entity (position, scale and rotation).
    pub hash: String,
}

/// The block definition that holds the entity of an object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRef {
    pub name: String,
    /// Handle of the BLOCK entity of the definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
}

/// The tag of an imported object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportTag {
    /// The source: file name, `#`, and the hash of the file header.
    pub source: String,
    /// DXF handle (group code 5) of the entity. None for files that have no handles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// DXF entity type, for example `LINE`.
    pub kind: String,
    /// Hash of the entity content in the file (16 hexadecimal digits). It does not use the handle.
    pub hash: String,
    /// The block definition, for an entity that is inside a block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockRef>,
    /// The INSERTs from the model to the entity, the outer one first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<InsertRef>,
    /// The copy of an array INSERT, counted from 0 (rows after columns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<u32>,
    /// The part, when one entity makes several objects (the loops of a HATCH), counted from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
}

/// The source of an imported layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerSource {
    /// The same text as `ImportTag::source`.
    pub source: String,
    /// The name of the DXF layer, when the layer maps one. Empty for a layer that holds the
    /// whole file.
    #[serde(default)]
    pub dxf_layer: String,
}

fn text_ok(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}

fn handle_ok(h: &Option<String>) -> bool {
    h.as_ref().is_none_or(|h| {
        !h.is_empty() && h.len() <= MAX_HANDLE && h.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn hash_ok(h: &str) -> bool {
    h.len() == 16 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl ImportTag {
    /// Why the tag is not valid, if it is not.
    pub fn problem(&self) -> Option<&'static str> {
        if !text_ok(&self.source, MAX_TEXT) {
            return Some("the source is empty, too long or has control characters");
        }
        if !text_ok(&self.kind, 32) {
            return Some("the entity type is not valid");
        }
        if !hash_ok(&self.hash) {
            return Some("the hash is not 16 hexadecimal digits");
        }
        if !handle_ok(&self.handle) {
            return Some("the handle is not valid");
        }
        if let Some(b) = &self.block
            && !(text_ok(&b.name, MAX_TEXT) && handle_ok(&b.handle))
        {
            return Some("the block is not valid");
        }
        if self.via.len() > MAX_DEPTH {
            return Some("too many block references");
        }
        if self
            .via
            .iter()
            .any(|i| !(hash_ok(&i.hash) && handle_ok(&i.handle)))
        {
            return Some("a block reference is not valid");
        }
        None
    }

    /// The tag in a value of `Object::data`, if there is a valid one.
    pub fn of(v: &Value) -> Option<ImportTag> {
        let t: ImportTag = serde_json::from_value(v.clone()).ok()?;
        t.problem().is_none().then_some(t)
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("a tag is plain data")
    }
}

impl LayerSource {
    pub fn problem(&self) -> Option<&'static str> {
        if !text_ok(&self.source, MAX_TEXT) {
            return Some("the source is empty, too long or has control characters");
        }
        if self.dxf_layer.chars().count() > MAX_TEXT || self.dxf_layer.chars().any(char::is_control)
        {
            return Some("the DXF layer name is too long or has control characters");
        }
        None
    }

    pub fn of(v: &Value) -> Option<LayerSource> {
        let s: LayerSource = serde_json::from_value(v.clone()).ok()?;
        s.problem().is_none().then_some(s)
    }
}

/// Checks the value of the data key `KEY`. The error says what is wrong.
pub fn check(v: &Value) -> Result<(), String> {
    match serde_json::from_value::<ImportTag>(v.clone()) {
        Ok(t) => t
            .problem()
            .map_or(Ok(()), |p| Err(format!("the import tag is not valid: {p}"))),
        Err(e) => Err(format!("the import tag is not valid: {e}")),
    }
}

/// The tag of an object's data, if it has a valid one.
pub fn tag(data: &BTreeMap<String, Value>) -> Option<ImportTag> {
    data.get(KEY).and_then(ImportTag::of)
}

/// Drops the tag from the data of a copy: a copy is new work and does not come from the file.
pub fn strip_copy(data: &mut BTreeMap<String, Value>) {
    data.remove(KEY);
}

/// Removes the tags that are not valid, from a document that was just loaded. Bad tags come from
/// hand-edited files or other programs. The document stays usable, and the notes say what went.
pub fn repair(doc: &mut Document) -> Vec<String> {
    let mut notes = Vec::new();
    let mut dropped = 0usize;
    let mut note = |notes: &mut Vec<String>, text: String| {
        if notes.len() < MAX_NOTES {
            notes.push(text);
        } else {
            dropped += 1;
        }
    };
    for o in doc.objects.values_mut() {
        if let Some(v) = o.data.get(KEY)
            && let Err(why) = check(v)
        {
            o.data.remove(KEY);
            note(
                &mut notes,
                format!("object {}: {why}; it was removed", o.id),
            );
        }
    }
    for l in doc.layers.iter_mut() {
        if let Some(v) = l.extra.get(LAYER_KEY)
            && LayerSource::of(v).is_none()
        {
            l.extra.remove(LAYER_KEY);
            note(
                &mut notes,
                format!(
                    "layer {}: the import source is not valid; it was removed",
                    l.id
                ),
            );
        }
    }
    if dropped > 0 {
        notes.push(format!(
            "{dropped} more import tags were not valid and were removed"
        ));
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tag_value() -> Value {
        json!({"source": "plan.dxf#00ff00ff00ff00ff", "handle": "2F", "kind": "LINE", "hash": "0123456789abcdef"})
    }

    #[test]
    fn a_good_tag_round_trips() {
        let t = ImportTag::of(&tag_value()).unwrap();
        assert_eq!(t.handle.as_deref(), Some("2F"));
        assert_eq!(t.to_value(), tag_value());
        assert!(check(&tag_value()).is_ok());
        let mut data = BTreeMap::new();
        data.insert(KEY.to_string(), tag_value());
        assert_eq!(tag(&data), Some(t));
        strip_copy(&mut data);
        assert_eq!(tag(&data), None);
    }

    #[test]
    fn block_tags_are_checked() {
        let mut v = tag_value();
        v["block"] = json!({"name": "TREE", "handle": "A1"});
        v["via"] = json!([{"handle": "B2", "hash": "0123456789abcdef"}]);
        v["copy"] = json!(3);
        assert!(check(&v).is_ok());
        v["via"] = json!([{"handle": "B 2", "hash": "0123456789abcdef"}]);
        assert!(check(&v).is_err());
        v["via"] = json!(vec![json!({"hash": "0123456789abcdef"}); MAX_DEPTH + 1]);
        assert!(check(&v).is_err());
        v["via"] = json!([]);
        v["block"] = json!({"name": ""});
        assert!(check(&v).is_err());
    }

    #[test]
    fn bad_tags_are_refused() {
        for (field, bad) in [
            ("source", json!("")),
            ("source", json!("a\nb")),
            ("source", json!("x".repeat(MAX_TEXT + 1))),
            ("kind", json!("")),
            ("hash", json!("XYZ")),
            ("hash", json!("0123456789ABCDEF")),
            ("handle", json!("")),
            ("handle", json!("a".repeat(MAX_HANDLE + 1))),
            ("handle", json!("2 F")),
            ("handle", json!(5)),
        ] {
            let mut v = tag_value();
            v[field] = bad;
            assert!(check(&v).is_err(), "{field}");
            assert!(ImportTag::of(&v).is_none());
        }
        let mut v = tag_value();
        v["extra"] = json!(1);
        assert!(check(&v).is_err());
        assert!(check(&json!("text")).is_err());
        assert!(check(&json!(null)).is_err());
    }

    #[test]
    fn layer_sources_are_checked() {
        assert!(LayerSource::of(&json!({"source": "a#0123456789abcdef"})).is_some());
        assert!(LayerSource::of(&json!({"source": "a", "dxf_layer": "Walls"})).is_some());
        assert!(LayerSource::of(&json!({"source": ""})).is_none());
        assert!(LayerSource::of(&json!({"source": "a", "dxf_layer": "x\u{7}"})).is_none());
        assert!(LayerSource::of(&json!({"source": "a", "dxf_layer": "x".repeat(300)})).is_none());
        assert!(LayerSource::of(&json!({"source": "a", "other": 1})).is_none());
    }

    #[test]
    fn repair_removes_bad_tags_and_bounds_the_notes() {
        use crate::model::{ClassId, Object, ObjectId, Shape};
        let mut doc = Document::with_sequential_ids();
        let layer = doc.layers[0].id;
        for n in 0..(MAX_NOTES + 5) {
            let mut o = Object::new(
                ObjectId::from_num(100 + n as u64),
                layer,
                ClassId::from_num(0),
                Shape::Locus3d {
                    p: Default::default(),
                },
            );
            o.data.insert(KEY.into(), json!({"source": 1}));
            doc.objects.insert(o.id, o);
        }
        let mut good = Object::new(
            ObjectId::from_num(999),
            layer,
            ClassId::from_num(0),
            Shape::Locus3d {
                p: Default::default(),
            },
        );
        good.data.insert(KEY.into(), tag_value());
        doc.objects.insert(good.id, good);
        doc.layers[0].extra.insert(LAYER_KEY.into(), json!(7));
        let notes = repair(&mut doc);
        assert_eq!(notes.len(), MAX_NOTES + 1);
        assert!(notes.last().unwrap().contains("more import tags"));
        assert!(
            doc.objects
                .values()
                .filter(|o| o.data.contains_key(KEY))
                .count()
                == 1
        );
        assert!(!doc.layers[0].extra.contains_key(LAYER_KEY));
        assert!(repair(&mut doc).is_empty());
    }

    #[test]
    fn commands_check_copies_drop_and_loads_repair() {
        use crate::command::{Command, CommandError, Editor};
        use crate::model::{Attributes, ObjectId, Shape};
        let mut ed = Editor::default();
        let id = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: Default::default(),
                    radius: 5.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let set = |v: Value| Command::SetData {
            id,
            set: [(KEY.to_string(), v)].into(),
            remove: vec![],
        };
        let bad = ed.execute(&set(json!({"source": 5})));
        assert!(matches!(bad, Err(CommandError::Invalid(m)) if m.contains("import tag")));
        ed.execute(&set(tag_value())).unwrap();
        assert!(tag(&ed.doc().object(id).unwrap().data).is_some());
        // Copies and "like" objects are new work: no tag.
        let copy = ed
            .execute(&Command::CopyObjects {
                ids: vec![id],
                delta: Default::default(),
            })
            .unwrap();
        let copy = ObjectId(copy.created[0]);
        assert!(!ed.doc().object(copy).unwrap().data.contains_key(KEY));
        let like = ed
            .execute(&Command::AddObjectLike {
                source: id,
                shape: Shape::Circle {
                    center: Default::default(),
                    radius: 1.0,
                },
            })
            .unwrap();
        assert!(
            !ed.doc()
                .object(ObjectId(like.created[0]))
                .unwrap()
                .data
                .contains_key(KEY)
        );
        // The original keeps its tag, and it is saved and loaded.
        let json = serde_json::to_string(ed.doc()).unwrap();
        let loaded = Document::from_json(&json).unwrap();
        assert!(tag(&loaded.object(id).unwrap().data).is_some());
        assert!(loaded.load_notes().is_empty());
        // A bad tag in a file is removed when it loads.
        let broken = json.replace("\"hash\":\"0123456789abcdef\"", "\"hash\":\"no\"");
        assert_ne!(broken, json);
        let loaded = Document::from_json(&broken).unwrap();
        assert!(!loaded.object(id).unwrap().data.contains_key(KEY));
        assert!(loaded.load_notes().iter().any(|n| n.contains("import tag")));
    }

    #[test]
    fn layer_sources_set_clear_and_survive_a_save() {
        use crate::command::{Command, CommandError, Editor};
        let mut ed = Editor::default();
        let layer = ed.doc().layers[0].id;
        let src = LayerSource {
            source: "a.dxf#0123456789abcdef".into(),
            dxf_layer: "Walls".into(),
        };
        ed.execute(&Command::SetLayerSource {
            id: layer,
            source: Some(src.clone()),
        })
        .unwrap();
        let loaded = Document::from_json(&serde_json::to_string(ed.doc()).unwrap()).unwrap();
        assert_eq!(
            LayerSource::of(&loaded.layers[0].extra[LAYER_KEY]),
            Some(src.clone())
        );
        assert!(ed.undo());
        assert!(!ed.doc().layers[0].extra.contains_key(LAYER_KEY));
        ed.execute(&Command::SetLayerSource {
            id: layer,
            source: Some(src),
        })
        .unwrap();
        ed.execute(&Command::SetLayerSource {
            id: layer,
            source: None,
        })
        .unwrap();
        assert!(!ed.doc().layers[0].extra.contains_key(LAYER_KEY));
        let bad = LayerSource {
            source: String::new(),
            dxf_layer: String::new(),
        };
        assert!(matches!(
            ed.execute(&Command::SetLayerSource {
                id: layer,
                source: Some(bad)
            }),
            Err(CommandError::Invalid(_))
        ));
        assert!(matches!(
            ed.execute(&Command::SetLayerSource {
                id: crate::model::LayerId::from_num(999),
                source: None
            }),
            Err(CommandError::NoLayer(_))
        ));
    }
}
