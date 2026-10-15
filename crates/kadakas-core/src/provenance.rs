//! Where imported objects came from (IOP-22).
//!
//! An importer of DXF or DWG files keeps, for each object, a small tag in `Object::data[KEY]`:
//! the drawing and revision it came from, the DXF handle of the entity, a hash of the entity
//! content, and a hash of the object right after the import. A layer that the import made keeps
//! the source in `Layer::extra[LAYER_KEY]`. A later import of a new revision of the file uses the
//! tags to match its entities to the objects (see `kadakas_io::dxf::revision`). Other importers
//! (Shapefile, IFC) can use the same tag: only `kind` and `hash` are about DXF entities.
//!
//! **Identity.** `drawing` is the lineage: the same drawing in all its revisions and under any
//! file name. `revision` is one saved state of it. The importer takes `drawing` from
//! `$FINGERPRINTGUID` (else the file name; a file without a header gets the name and a hash of its
//! entities) and `revision` from `$VERSIONGUID` (else a hash of the header). Layers and objects are
//! matched on `drawing`. The layer source records the `revision` and the import transform (unit
//! scale and origin shift), which an update needs to put a new file in the same place.
//!
//! **Base side.** The summaries of the entities of one revision (not the file) are kept in a
//! document resource of type `import_base`, keyed by `drawing` and `revision` (`check_base`). An
//! update diffs these summaries with the new file and maps the result to objects by handle or
//! hash, `via` and `part`. A changed block definition fans out: one changed entity in the
//! definition maps to every object whose `block.key` is that definition and whose `via` leads
//! to it.
//!
//! **Link instead of base.** A drawing with more than `MAX_BASE_RECS` entities keeps no summary
//! (the cap protects the document and the collaboration frames). The import then asks the person
//! (`base: "auto"` refuses with the choices): `link` keeps a [`FileLink`] on the layer source
//! instead of the summary (the absolute path, the size and the SHA-256 of the file), and `none`
//! keeps no update data. An update reads the old file from the path, checks the size and the
//! hash, and diffs it against the new file; if the file is gone or changed, the update says so. The
//! path is a hint and data from files and other users: it is validated like the other texts of a
//! tag (`FileLink::problem`), and reading it needs the normal path policy of the reader.
//!
//! Why object data: it is the place where importers already keep per-object facts (the GeoJSON
//! and Shapefile importers, the rotation of imported text). It is saved with the object, copied
//! with `Clone`, and synced by collaboration as the property `data.import`, with no new field in
//! the file format. A record format is for fields that people edit, so it does not fit.
//!
//! The tag says where the object came from, not what it is now. A copy is new work, so the
//! commands that copy objects drop the tag (`strip_copy`). A moved object keeps it. `hash` is
//! the hash of the source entity and never changes. `obj` is the hash of the object state
//! (shape, attributes, layer, class) right after the import: if it differs from `state_hash` of
//! the object now, the user edited the object (`is_edited`).
//!
//! `obj` is computed in model coordinates, as the object is stored. A change of the georeference
//! does not move objects (it only says where the model is on the earth), so it is not an edit.
//! A move of a whole layer is not possible as one command (layers have no offset); moving all its
//! objects is an edit of each of them, which is what an update must ask about. The layer source
//! keeps the import transform (`unit_scale`, `shift`), so an update can put a new file in the same
//! place; it does not enter `obj`.
//!
//! The tag comes from files and from other users, so it is checked (`check`) when a command sets
//! it and when a document is loaded (`repair`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Attributes, ClassId, Document, LayerId, Object, ObjectId, Shape};

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
    /// The copy of an array INSERT, counted from 0 (rows after columns). None for a single INSERT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<u32>,
}

/// The block definition that holds the entity of an object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRef {
    /// The block name, for people. An anonymous block (`*U12`) has a name that changes at each save.
    pub name: String,
    /// The key of the definition: the BLOCK_RECORD handle, else the name, else for an anonymous
    /// block a hash of its content. Matching uses the key.
    pub key: String,
    /// Handle of the BLOCK entity of the definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
}

/// The tag of an imported object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportTag {
    /// The drawing (lineage) that the object came from.
    pub drawing: String,
    /// The revision of the drawing that the object came from.
    pub revision: String,
    /// DXF handle (group code 5) of the entity. None for files that have no handles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// DXF entity type, for example `LINE`.
    pub kind: String,
    /// Hash of the entity content in the file (16 hexadecimal digits). It does not use the handle.
    pub hash: String,
    /// Hash of the object state right after the import (`state_hash`).
    pub obj: String,
    /// The block definition, for an entity that is inside a block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockRef>,
    /// The INSERTs from the model to the entity, the outer one first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<InsertRef>,
    /// The part, when one entity makes several objects (the loops of a HATCH), counted from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
}

/// Longest path in a [`FileLink`], in characters.
pub const MAX_LINK_PATH: usize = 4096;

/// A link to the original file of an import, instead of the summaries of its entities (see the
/// module docs). It is a hint: the file may be gone or changed, and the hash tells.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileLink {
    /// The absolute path of the file when it was imported.
    pub path: String,
    /// The size of the file in bytes.
    pub bytes: u64,
    /// The SHA-256 of the file as 64 lower-case hexadecimal digits.
    pub sha256: String,
}

/// True for a path that is absolute by its text, on any platform (the file may come from another
/// computer): `/x`, `\\server\x` or `C:\x`.
fn absolute_text(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/')
        || p.starts_with('\\')
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

impl FileLink {
    /// The link of a file whose bytes are `data`. `path` must be absolute.
    pub fn of_bytes(path: &str, data: &[u8]) -> FileLink {
        FileLink {
            path: path.to_string(),
            bytes: data.len() as u64,
            sha256: crate::picture::sha256_hex(data),
        }
    }

    /// The link of a file that is read from `reader` (the size and the hash are of what it gives).
    /// The caller bounds the reader (`fsio::open_regular`).
    pub fn of_reader(path: &str, mut reader: impl std::io::Read) -> std::io::Result<FileLink> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut bytes = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            bytes += n as u64;
        }
        let sha256 = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(FileLink {
            path: path.to_string(),
            bytes,
            sha256,
        })
    }

    /// Why the link is not valid, if it is not. The path must be absolute and plain: no control
    /// characters, no `..`, at most `MAX_LINK_PATH` characters.
    pub fn problem(&self) -> Option<&'static str> {
        let p = &self.path;
        if p.is_empty() || p.chars().count() > MAX_LINK_PATH || p.chars().any(char::is_control) {
            return Some("the link path is empty, too long or has control characters");
        }
        if !absolute_text(p) {
            return Some("the link path is not absolute");
        }
        if p.split(['/', '\\']).any(|part| part == "..") {
            return Some("the link path has a .. part");
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Some("the link hash is not 64 hexadecimal digits");
        }
        None
    }
}

/// The source of an imported layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerSource {
    /// The drawing (lineage), as in `ImportTag::drawing`. Layers are matched on it.
    pub drawing: String,
    /// The revision that was imported last.
    pub revision: String,
    /// The name of the DXF layer, when the layer maps one. Empty for a layer that holds the
    /// whole file.
    #[serde(default)]
    pub dxf_layer: String,
    /// Millimeters per unit of the file, as the import used it.
    pub unit_scale: f64,
    /// The shift in mm that the import took off the coordinates of the file (the model is the file
    /// times `unit_scale` minus `shift`).
    pub shift: [f64; 2],
    /// The original file, when the import kept no summaries of the entities because the drawing
    /// has too many (`base: "link"`). An update reads the old file from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<FileLink>,
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
        if !text_ok(&self.drawing, MAX_TEXT) || !text_ok(&self.revision, MAX_TEXT) {
            return Some("the drawing or revision is empty, too long or has control characters");
        }
        if !text_ok(&self.kind, 32) {
            return Some("the entity type is not valid");
        }
        if !hash_ok(&self.hash) || !hash_ok(&self.obj) {
            return Some("a hash is not 16 hexadecimal digits");
        }
        if !handle_ok(&self.handle) {
            return Some("the handle is not valid");
        }
        if let Some(b) = &self.block
            && !(text_ok(&b.name, MAX_TEXT) && text_ok(&b.key, MAX_TEXT) && handle_ok(&b.handle))
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
        if !text_ok(&self.drawing, MAX_TEXT) || !text_ok(&self.revision, MAX_TEXT) {
            return Some("the drawing or revision is empty, too long or has control characters");
        }
        if !(self.unit_scale.is_finite() && self.unit_scale > 0.0)
            || self
                .shift
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1.0e12)
        {
            return Some("the import transform is not valid");
        }
        if self.dxf_layer.chars().count() > MAX_TEXT || self.dxf_layer.chars().any(char::is_control)
        {
            return Some("the DXF layer name is too long or has control characters");
        }
        if let Some(link) = &self.link {
            return link.problem();
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

/// A 64-bit FNV-1a hash that is the same on every platform and build, so hashes can be saved.
/// Every part is written with a length or a tag, so `"ab" + "c"` and `"a" + "bc"` differ. It is
/// not a security check.
#[derive(Clone, Copy)]
pub struct Hasher(u64);

impl Hasher {
    pub fn new() -> Hasher {
        Hasher(0xcbf2_9ce4_8422_2325)
    }

    fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    pub fn u64(&mut self, v: u64) {
        v.to_le_bytes().into_iter().for_each(|b| self.byte(b));
    }

    pub fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        s.bytes().for_each(|b| self.byte(b));
    }

    /// A number by its bits. Minus zero counts as zero.
    pub fn f64(&mut self, v: f64) {
        self.u64((v + 0.0).to_bits());
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for Hasher {
    fn default() -> Self {
        Hasher::new()
    }
}

/// A hash as 16 lower-case hexadecimal digits.
pub fn hex(h: u64) -> String {
    format!("{h:016x}")
}

/// The value of `ImportTag::obj` that an importer writes before it knows the object. The builder
/// replaces it with `state_hash`.
pub const PENDING_OBJ: &str = "0000000000000000";

/// The hash of the state of an object that an update must not overwrite without asking: shape,
/// attributes, layer and class. The data is not in it.
pub fn state_hash(shape: &Shape, attrs: &Attributes, layer: LayerId, class: ClassId) -> String {
    let mut h = Hasher::new();
    h.str(&serde_json::to_string(shape).unwrap_or_default());
    h.str(&serde_json::to_string(attrs).unwrap_or_default());
    h.str(&layer.0.to_string());
    h.str(&class.0.to_string());
    hex(h.finish())
}

/// True if the object has a tag and its state differs from the state at the import: the user
/// (or a script) edited it. None if it has no valid tag.
pub fn is_edited(o: &Object) -> Option<bool> {
    let t = tag(&o.data)?;
    Some(t.obj != state_hash(&o.shape, &o.attrs, o.layer, o.class))
}

/// The type name of the resource that keeps the base summaries of one revision.
pub const BASE_TYPE: &str = "import_base";
/// Most entity summaries in all parts of one base.
pub const MAX_BASE_RECS: usize = 200_000;
/// Most points in one stored summary.
pub const MAX_BASE_POINTS: usize = 64;
/// Most parts of one base. A big base is split in parts so that each part is a blob of the
/// collaboration (see `docs/collaboration.md`) that fits in a frame.
pub const MAX_BASE_PARTS: u64 = 16;
/// The size that the importer aims at for one part, in bytes of JSON. It is below the smallest
/// frame of a collaboration server (8 MiB).
pub const BASE_PART_BYTES: usize = 4 * 1024 * 1024;
/// The most that a part may hold, in estimated bytes of JSON.
pub const MAX_BASE_PART_BYTES: usize = 8 * 1024 * 1024;

const REC_KEYS: [&str; 14] = [
    "handle",
    "scope",
    "kind",
    "layer",
    "color",
    "linetype",
    "lineweight",
    "text",
    "hash",
    "geo_hash",
    "points",
    "sizes",
    "center",
    "diag",
];

fn number_pair(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() == 2 && a.iter().all(Value::is_number))
}

/// Checks one stored summary and returns an estimate of its size in bytes.
fn check_rec(r: &Value) -> Result<usize, String> {
    let r = r.as_object().ok_or("a rec is a JSON object")?;
    let mut size = 64;
    for (k, v) in r {
        if !REC_KEYS.contains(&k.as_str()) {
            return Err(format!("a rec has the unknown key '{k}'"));
        }
        match (k.as_str(), v) {
            ("points", Value::Array(p)) => {
                if p.len() > MAX_BASE_POINTS || !p.iter().all(number_pair) {
                    return Err("a rec has bad points".into());
                }
                size += 40 * p.len();
            }
            ("sizes", Value::Array(p)) => {
                if p.len() > MAX_BASE_POINTS || !p.iter().all(Value::is_number) {
                    return Err("a rec has bad sizes".into());
                }
                size += 20 * p.len();
            }
            ("center", v) if !(v.is_null() || number_pair(v)) => {
                return Err("a rec has a bad center".into());
            }
            ("diag", v) if !v.is_number() => return Err("a rec has a bad diag".into()),
            ("points" | "sizes" | "center" | "diag", _) => size += 40,
            (_, Value::String(t)) => {
                if t.chars().count() > 1024 {
                    return Err("a rec has a text that is too long".into());
                }
                size += t.len() + 8;
            }
            (_, Value::Null) => {}
            _ => return Err(format!("the rec key '{k}' must be text")),
        }
    }
    Ok(size)
}

/// Checks the body of an `import_base` resource: `{type, drawing, revision, unit_scale, recs,
/// part?, parts?}`. `recs` is a list of summaries (`kadakas_io::dxf::revision::Rec` reads them).
/// The check bounds the shape and the size (it runs for a command, a load and a peer); the
/// content is checked again when the summaries are read.
pub fn check_base(v: &Value) -> Result<(), String> {
    let o = v.as_object().ok_or("an import base is a JSON object")?;
    if let Some(k) = o.keys().find(|k| {
        ![
            "type",
            "drawing",
            "revision",
            "unit_scale",
            "recs",
            "part",
            "parts",
        ]
        .contains(&k.as_str())
    }) {
        return Err(format!("the import base has the unknown key '{k}'"));
    }
    for k in ["drawing", "revision"] {
        let ok = o
            .get(k)
            .and_then(Value::as_str)
            .is_some_and(|t| text_ok(t, MAX_TEXT));
        if !ok {
            return Err(format!("the import base needs a valid {k}"));
        }
    }
    if let Some(u) = o.get("unit_scale")
        && !u.as_f64().is_some_and(|u| u.is_finite() && u > 0.0)
    {
        return Err("the unit scale of the import base is not valid".into());
    }
    let (part, parts) = base_part(o);
    if o.contains_key("part") != o.contains_key("parts")
        || parts == 0
        || parts > MAX_BASE_PARTS
        || part >= parts
    {
        return Err("the part numbers of the import base are not valid".into());
    }
    let recs = o
        .get("recs")
        .and_then(Value::as_array)
        .ok_or("the import base needs a list of recs")?;
    if recs.len() > MAX_BASE_RECS {
        return Err(format!(
            "the import base has more than {MAX_BASE_RECS} recs"
        ));
    }
    let mut size = 0usize;
    for r in recs {
        size += check_rec(r)?;
        if size > MAX_BASE_PART_BYTES {
            return Err("the import base part is too big".into());
        }
    }
    Ok(())
}

/// The part number and the number of parts of a base body (a body without them is one part).
fn base_part(o: &serde_json::Map<String, Value>) -> (u64, u64) {
    let n = |k: &str, d: u64| o.get(k).map_or(d, |v| v.as_u64().unwrap_or(u64::MAX));
    (n("part", 0), n("parts", 1))
}

/// The summaries of a revision, from all its parts in order. None if the document has no such base
/// or a part is missing.
pub fn base_recs<'a>(doc: &'a Document, drawing: &str, revision: &str) -> Option<Vec<&'a Value>> {
    use crate::resource::ResourceBody;
    let mut found: Vec<(u64, u64, &'a Value)> = Vec::new();
    for r in doc.resources.iter() {
        if let ResourceBody::ImportBase(v) = &r.body
            && v["drawing"] == drawing
            && v["revision"] == revision
        {
            let (part, parts) = base_part(v.as_object()?);
            found.push((part, parts, &**v));
        }
    }
    let parts = found.first()?.1;
    found.sort_by_key(|f| f.0);
    let complete = found.len() as u64 == parts
        && found
            .iter()
            .enumerate()
            .all(|(i, f)| f.0 == i as u64 && f.1 == parts);
    complete.then(|| {
        found
            .iter()
            .flat_map(|f| f.2["recs"].as_array().into_iter().flatten())
            .collect()
    })
}

/// Who still needs the base of a revision: the objects that came from it, and the layers whose
/// import source names it. A base that nobody needs can be purged (`library::purge_plan`).
///
/// An object whose layer has moved on to a newer revision of the drawing (an update from DXF,
/// IOP-23, leaves the objects it did not change with the revision they came from) does not need
/// the old base: an update reads the base of the revision of the layer.
pub fn base_users(doc: &Document, drawing: &str, revision: &str) -> (Vec<ObjectId>, Vec<LayerId>) {
    let moved_on = |o: &Object| {
        doc.layer(o.layer)
            .and_then(|l| l.extra.get(LAYER_KEY))
            .and_then(LayerSource::of)
            .is_some_and(|s| s.drawing == drawing && s.revision != revision)
    };
    let objects = doc
        .objects
        .values()
        .filter(|o| tag(&o.data).is_some_and(|t| t.drawing == drawing && t.revision == revision))
        .filter(|o| !moved_on(o))
        .map(|o| o.id)
        .collect();
    let layers = doc
        .layers
        .iter()
        .filter(|l| {
            l.extra
                .get(LAYER_KEY)
                .and_then(LayerSource::of)
                .is_some_and(|s| s.drawing == drawing && s.revision == revision)
        })
        .map(|l| l.id)
        .collect();
    (objects, layers)
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
    // A base that is not valid would make the file unreadable, so it goes (an update then asks
    // for the old file).
    let bad: Vec<_> = doc
        .resources
        .iter()
        .filter_map(|r| match &r.body {
            crate::resource::ResourceBody::ImportBase(v) => check_base(v).err().map(|e| (r.id, e)),
            _ => None,
        })
        .collect();
    for (id, why) in bad {
        doc.resources.remove(id);
        note(&mut notes, format!("resource {id}: {why}; it was removed"));
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
        json!({"drawing": "plan.dxf", "revision": "00ff00ff00ff00ff", "handle": "2F", "kind": "LINE", "hash": "0123456789abcdef", "obj": "fedcba9876543210"})
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
        v["block"] = json!({"name": "TREE", "key": "rec:A1", "handle": "A1"});
        v["via"] = json!([{"handle": "B2", "hash": "0123456789abcdef", "copy": 3}]);
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
            ("drawing", json!("")),
            ("revision", json!("a\nb")),
            ("drawing", json!("x".repeat(MAX_TEXT + 1))),
            ("obj", json!("zz")),
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
        let ls = |extra: Value| {
            let mut v =
                json!({"drawing": "a", "revision": "r", "unit_scale": 1000.0, "shift": [0.0, 5.0]});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            v
        };
        assert!(LayerSource::of(&ls(json!({}))).is_some());
        assert!(LayerSource::of(&ls(json!({"dxf_layer": "Walls"}))).is_some());
        assert!(LayerSource::of(&ls(json!({"drawing": ""}))).is_none());
        assert!(LayerSource::of(&ls(json!({"dxf_layer": "x\u{7}"}))).is_none());
        assert!(LayerSource::of(&ls(json!({"dxf_layer": "x".repeat(300)}))).is_none());
        assert!(LayerSource::of(&ls(json!({"other": 1}))).is_none());
        assert!(LayerSource::of(&ls(json!({"unit_scale": 0.0}))).is_none());
        assert!(LayerSource::of(&ls(json!({"shift": [1e13, 0.0]}))).is_none());
    }

    #[test]
    fn file_links_are_checked_like_other_paths() {
        let link = |path: &str, sha: &str| {
            json!({"drawing": "a", "revision": "r", "unit_scale": 1.0, "shift": [0.0, 0.0],
                "link": {"path": path, "bytes": 7, "sha256": sha}})
        };
        let sha = "a".repeat(64);
        for good in [
            "/data/site.dxf",
            "C:\\data\\site.dxf",
            "D:/data/site.dxf",
            "\\\\server\\share\\site.dxf",
            "/data/..name/site.dxf",
        ] {
            let ok = LayerSource::of(&link(good, &sha));
            assert_eq!(ok.and_then(|s| s.link).map(|l| l.path), Some(good.into()));
        }
        for bad in [
            "",
            "site.dxf",
            "data/site.dxf",
            "./site.dxf",
            "/data/../etc/site.dxf",
            "C:\\data\\..\\site.dxf",
            "/data/\u{7}x",
            "/data/x\ny",
            "C:site.dxf",
        ] {
            assert!(LayerSource::of(&link(bad, &sha)).is_none(), "{bad:?}");
        }
        let long = format!("/{}", "x".repeat(MAX_LINK_PATH));
        assert!(LayerSource::of(&link(&long, &sha)).is_none());
        for bad_sha in ["", "abc", &"A".repeat(64), &"g".repeat(64), &"a".repeat(65)] {
            assert!(
                LayerSource::of(&link("/x", bad_sha)).is_none(),
                "{bad_sha:?}"
            );
        }
        // Unknown fields of the link are refused, and a link of the wrong type too.
        let mut v = link("/x", &sha);
        v["link"]["mode"] = json!(1);
        assert!(LayerSource::of(&v).is_none());
        v["link"] = json!("/x");
        assert!(LayerSource::of(&v).is_none());
        // A layer without a link has no `link` key in its JSON.
        let plain = LayerSource::of(
            &json!({"drawing": "a", "revision": "r", "unit_scale": 1.0, "shift": [0.0, 0.0]}),
        )
        .unwrap();
        assert!(plain.link.is_none());
        assert!(serde_json::to_value(&plain).unwrap().get("link").is_none());
    }

    #[test]
    fn a_link_is_made_from_bytes_or_a_reader_with_the_same_hash() {
        let data = b"0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n".repeat(5000);
        let a = FileLink::of_bytes("/data/a.dxf", &data);
        assert_eq!(a.bytes, data.len() as u64);
        assert_eq!(a.problem(), None);
        let b = FileLink::of_reader("/data/a.dxf", &data[..]).unwrap();
        assert_eq!(a, b);
        // The known hash of the empty input.
        let empty = FileLink::of_reader("/x", &b""[..]).unwrap();
        assert_eq!(empty.bytes, 0);
        assert_eq!(
            empty.sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // A reader that fails gives the error.
        struct Broken;
        impl std::io::Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk"))
            }
        }
        assert!(FileLink::of_reader("/x", Broken).is_err());
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
            o.data.insert(KEY.into(), json!({"drawing": 1}));
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
        let bad = ed.execute(&set(json!({"drawing": 5})));
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
            drawing: "a.dxf".into(),
            revision: "r1".into(),
            dxf_layer: "Walls".into(),
            unit_scale: 1000.0,
            shift: [0.0, 0.0],
            link: Some(FileLink::of_bytes("/data/a.dxf", b"0\nEOF\n")),
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
            drawing: String::new(),
            revision: "r1".into(),
            dxf_layer: String::new(),
            unit_scale: 1.0,
            shift: [0.0, 0.0],
            link: None,
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

    #[test]
    fn the_state_hash_sees_shape_attributes_layer_and_class_but_not_data() {
        use crate::model::{ClassId, Color, ObjectId};
        let shape = Shape::Circle {
            center: Default::default(),
            radius: 5.0,
        };
        let layer = LayerId::from_num(1);
        let class = ClassId::from_num(0);
        let attrs = Attributes::default();
        let h = state_hash(&shape, &attrs, layer, class);
        assert_eq!(h.len(), 16);
        assert_eq!(h, state_hash(&shape, &attrs, layer, class));
        let bigger = Shape::Circle {
            center: Default::default(),
            radius: 6.0,
        };
        assert_ne!(h, state_hash(&bigger, &attrs, layer, class));
        let pen = Attributes {
            pen: Some(Color::BLACK),
            ..Attributes::default()
        };
        assert_ne!(h, state_hash(&shape, &pen, layer, class));
        assert_ne!(h, state_hash(&shape, &attrs, LayerId::from_num(2), class));
        assert_ne!(h, state_hash(&shape, &attrs, layer, ClassId::from_num(3)));
        // An object is edited when its state differs from the tag.
        let mut o = Object::new(ObjectId::from_num(7), layer, class, shape);
        assert_eq!(is_edited(&o), None, "no tag");
        let mut v = tag_value();
        v["obj"] = json!(h);
        o.data.insert(KEY.into(), v);
        assert_eq!(is_edited(&o), Some(false));
        o.data.insert("note".into(), json!("data is not state"));
        assert_eq!(is_edited(&o), Some(false));
        o.shape = bigger;
        assert_eq!(is_edited(&o), Some(true));
        assert_eq!(Hasher::default().finish(), Hasher::new().finish());
    }

    #[test]
    fn import_bases_are_checked_stored_and_found() {
        use crate::resource::{ResourceBody, ResourceId};
        let rec = json!({"handle": "2F", "scope": "", "kind": "LINE", "points": [[0.0, 0.0]]});
        let base = json!({"type": BASE_TYPE, "drawing": "d", "revision": "r", "unit_scale": 1.0, "recs": [rec]});
        assert!(check_base(&base).is_ok());
        let bad = |f: &dyn Fn(&mut Value)| {
            let mut v = base.clone();
            f(&mut v);
            assert!(check_base(&v).is_err(), "{v}");
        };
        bad(&|v| *v = json!([]));
        bad(&|v| v["drawing"] = json!(""));
        bad(&|v| v["revision"] = json!(5));
        bad(&|v| v["recs"] = json!("x"));
        bad(&|v| v["recs"] = json!([5]));
        bad(&|v| v["recs"][0]["layer"] = json!("x".repeat(2000)));
        bad(&|v| v["recs"][0]["points"] = json!(vec![[0.0, 0.0]; MAX_BASE_POINTS + 1]));
        bad(&|v| v["recs"] = json!(vec![json!({}); MAX_BASE_RECS + 1]));
        // As a resource: it saves and loads, a command refuses a bad one, and the lookup finds it.
        let mut doc = Document::with_sequential_ids();
        let body = ResourceBody::ImportBase(std::sync::Arc::new(base.clone()));
        assert_eq!(body.type_name(), BASE_TYPE);
        assert!(body.validate().is_ok());
        assert!(
            ResourceBody::ImportBase(std::sync::Arc::new(json!(1)))
                .validate()
                .is_err()
        );
        doc.resources.push(crate::resource::Resource {
            id: ResourceId::from_num(900),
            name: "import base".into(),
            body,
        });
        assert_eq!(base_recs(&doc, "d", "r").unwrap().len(), 1);
        assert!(base_recs(&doc, "d", "other").is_none());
        let loaded = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert!(base_recs(&loaded, "d", "r").is_some());
        // Strict: unknown keys, bad inner values and bad part numbers are refused.
        bad(&|v| v["extra"] = json!(1));
        bad(&|v| v["recs"][0]["other"] = json!(1));
        bad(&|v| v["recs"][0]["points"] = json!([[1.0]]));
        bad(&|v| v["recs"][0]["sizes"] = json!(["x"]));
        bad(&|v| v["recs"][0]["center"] = json!([1.0]));
        bad(&|v| v["recs"][0]["diag"] = json!("x"));
        bad(&|v| v["recs"][0]["kind"] = json!(5));
        bad(&|v| v["unit_scale"] = json!(0.0));
        bad(&|v| v["part"] = json!(0));
        bad(&|v| {
            v["part"] = json!(2);
            v["parts"] = json!(2);
        });
        bad(&|v| {
            v["part"] = json!(0);
            v["parts"] = json!(MAX_BASE_PARTS + 1);
        });
        let big = json!({"handle": "x".repeat(1000), "kind": "k".repeat(1000), "layer": "l".repeat(1000)});
        bad(&|v| v["recs"] = Value::Array(vec![big.clone(); 3000]));
        let ok = |f: &dyn Fn(&mut Value)| {
            let mut v = base.clone();
            f(&mut v);
            assert!(check_base(&v).is_ok(), "{v}");
        };
        ok(&|v| {
            v["part"] = json!(1);
            v["parts"] = json!(2);
            v["recs"][0]["center"] = json!([1.0, 2.0]);
            v["recs"][0]["sizes"] = json!([1.0]);
            v["recs"][0]["diag"] = json!(3.0);
            v["recs"][0]["text"] = Value::Null;
        });
    }

    #[test]
    fn a_base_in_parts_is_found_only_when_all_parts_are_there() {
        use crate::resource::{Resource, ResourceBody, ResourceId};
        let mut doc = Document::with_sequential_ids();
        let part = |n: u64, parts: u64, kind: &str| {
            json!({"type": BASE_TYPE, "drawing": "d", "revision": "r", "part": n, "parts": parts,
                   "recs": [{"kind": kind, "points": [[0.0, 0.0]]}]})
        };
        fn push(doc: &mut Document, id: u64, v: Value) {
            doc.resources.push(Resource {
                id: ResourceId::from_num(id),
                name: format!("part {id}"),
                body: ResourceBody::ImportBase(std::sync::Arc::new(v)),
            })
        }
        push(&mut doc, 900, part(1, 2, "B"));
        assert!(base_recs(&doc, "d", "r").is_none(), "part 0 is missing");
        push(&mut doc, 901, part(0, 2, "A"));
        let recs = base_recs(&doc, "d", "r").unwrap();
        let kinds: Vec<_> = recs.iter().map(|r| r["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["A", "B"], "in the order of the parts");
        push(&mut doc, 902, part(1, 2, "C"));
        assert!(
            base_recs(&doc, "d", "r").is_none(),
            "a part twice is not a base"
        );
    }

    #[test]
    fn the_body_of_a_base_is_shared_by_the_undo_steps() {
        use crate::command::{Command, Editor};
        use crate::resource::ResourceBody;
        let mut ed = Editor::default();
        let recs: Vec<Value> = (0..2000)
            .map(|i| json!({"kind": "LINE", "handle": format!("{i:X}"), "points": [[0.0, 0.0]]}))
            .collect();
        let body = json!({"type": BASE_TYPE, "drawing": "d", "revision": "r", "recs": recs});
        ed.execute(&Command::AddResource {
            name: "base".into(),
            body: ResourceBody::ImportBase(std::sync::Arc::new(body)),
        })
        .unwrap();
        let held = |ed: &Editor| match &ed.doc().resources.iter().next().unwrap().body {
            ResourceBody::ImportBase(a) => a.clone(),
            _ => unreachable!(),
        };
        let first = held(&ed);
        let before = std::sync::Arc::strong_count(&first);
        // Many commands that copy the layers, classes and resources for the undo step.
        for n in 0..20 {
            ed.execute(&Command::AddLayer {
                name: format!("L{n}"),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap();
        }
        let after = held(&ed);
        assert!(
            std::sync::Arc::ptr_eq(&first, &after),
            "the text was never copied"
        );
        // Each step holds a pointer, not a copy: the count grows with the steps, the bytes do not.
        assert!(std::sync::Arc::strong_count(&after) <= before + 2 * 20 + 2);
        assert!(ed.undo() && ed.undo());
        assert!(std::sync::Arc::ptr_eq(&first, &held(&ed)));
    }

    #[test]
    fn a_base_is_purged_only_when_nothing_refers_to_its_revision() {
        use crate::library::{PURGE_TYPES, list, purge_plan, usage};
        use crate::resource::{Resource, ResourceBody, ResourceId};
        let mut doc = Document::with_sequential_ids();
        let id = ResourceId::from_num(900);
        doc.resources.push(Resource {
            id,
            name: "base".into(),
            body: ResourceBody::ImportBase(std::sync::Arc::new(
                json!({"type": BASE_TYPE, "drawing": "plan.dxf", "revision": "0123456789abcdef", "recs": []}),
            )),
        });
        let layer = doc.layers[0].id;
        assert_eq!(
            purge_plan(&doc, &PURGE_TYPES),
            vec![id],
            "nothing refers to it"
        );
        // The newest import of the drawing still needs it: its layer names the revision.
        doc.layers[0].extra.insert(
            LAYER_KEY.into(),
            json!({"drawing": "plan.dxf", "revision": "0123456789abcdef", "unit_scale": 1.0, "shift": [0.0, 0.0]}),
        );
        assert!(purge_plan(&doc, &PURGE_TYPES).is_empty());
        assert_eq!(usage(&doc, id).layers, vec![layer]);
        assert_eq!(list(&doc, Some(BASE_TYPE), None)[0].used_by, 1);
        // Another revision of the layer: the old base is used only by the objects that came from it.
        doc.layers[0].extra.insert(
            LAYER_KEY.into(),
            json!({"drawing": "plan.dxf", "revision": "other", "unit_scale": 1.0, "shift": [0.0, 0.0]}),
        );
        assert_eq!(purge_plan(&doc, &PURGE_TYPES), vec![id]);
        let mut o = Object::new(
            ObjectId::from_num(7),
            layer,
            ClassId::from_num(0),
            Shape::Locus3d {
                p: Default::default(),
            },
        );
        o.data
            .insert(KEY.into(), tag_value_for("plan.dxf", "0123456789abcdef"));
        doc.objects.insert(o.id, o);
        assert!(purge_plan(&doc, &PURGE_TYPES).is_empty());
        assert_eq!(usage(&doc, id).objects.len(), 1);
    }

    fn tag_value_for(drawing: &str, revision: &str) -> Value {
        let mut v = tag_value();
        v["drawing"] = json!(drawing);
        v["revision"] = json!(revision);
        v
    }
}
