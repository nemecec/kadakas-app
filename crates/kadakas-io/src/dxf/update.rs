//! Update from DXF (IOP-23): reads a new revision of an imported drawing, matches its entities to
//! the objects of the document and plans what changes.
//!
//! The module is pure: it reads no file and makes no change. The caller gives the bytes of the new
//! file and a way to read a linked old file, [`plan`] gives a [`Plan`], and [`Plan::build`] turns
//! the changes that the person accepted into one command. The steps of a plan:
//!
//! 1. **The drawing.** The layers of the document that carry an import source
//!    (`Layer::extra["import_source"]`) name the drawings (lineages) that were imported. One of
//!    them is the drawing to update: the caller names it, or it is the only one, or the new file
//!    names it (its GUID, else its file name).
//! 2. **The old side.** The stored base of the drawing and its revision (`ImportBase`), else the
//!    linked original file (checked by size and SHA-256), else an old file that the caller gives.
//!    Without one the plan fails and says what to give.
//! 3. **The diff.** [`revision::diff`] matches the old records with those of the new file: by
//!    handle, then by content and geometry, then by position.
//! 4. **The objects.** The new file is imported into a scratch document with the tags of the
//!    drawing and the placement of the old import (the lineage option). A [`TagIndex`] of the
//!    document and one of the scratch document map each changed record to the objects of the
//!    document that it concerns (the fan-out of block definitions included) and to the new
//!    objects. The objects of the two sides are paired by their place in the file
//!    (block, INSERTs, part).
//! 5. **The classes.** Each change is added, changed or removed. It is a *conflict* when the
//!    person edited one of its objects (`provenance::is_edited`) and the new revision changes or
//!    removes it as well.
//!
//! Only objects with a tag of this drawing are ever touched. The files are untrusted: the work is
//! bounded by the limits of the diff, the import, and the size of what a plan may hold.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{
    Attributes, ClassId, Document, LayerId, LayerKind, NONE_CLASS, Object, ObjectId, Shape,
};
use kadakas_core::provenance::{self, FileLink, ImportTag, LayerSource};
use kadakas_core::resource::ResourceBody;
use serde::Serialize;

use super::hash::{content_hash, hex};
use super::import;
use super::link::LinkError;
use super::parse;
use super::revision::{self, Change as Prop, Diff, PAPER, Rec, Status, TagIndex, Via};
use super::source::Tracking;
use crate::builder::Builder;
use crate::{BaseMode, Context, ImportOptions, IoError, LayerMode, Lineage, Unit};

/// Most DXF entities of the new file that a plan reads.
pub const MAX_NEW_RECS: usize = 1_000_000;
/// Most object IDs that a change lists for the caller (the count is exact).
pub const LISTED_OBJECTS: usize = 8;

/// Why a plan could not be made.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// The document has no drawing that came from a DXF or DWG import.
    #[error(
        "the document has no imported DXF or DWG drawing to update. Import the first revision with import.file"
    )]
    NoDrawing,
    /// Several drawings were imported and nothing says which one is meant.
    #[error("several drawings were imported: name the one to update with `drawing`: {0}")]
    Ambiguous(String),
    /// The new file says it is another drawing than the only one in the document. The person
    /// confirms it, and the caller then names the drawing.
    #[error(
        "the new file says it is the drawing {new}, not {drawing} (the only drawing in the document). If it is a new revision of {drawing}, give drawing: \"{drawing}\""
    )]
    Mismatch { new: String, drawing: String },
    /// The named drawing is not in the document.
    #[error("no imported drawing is named {0}. The drawings are: {1}")]
    Unknown(String, String),
    /// The old side is not available. The caller asks for the old file.
    #[error("{0}")]
    NeedsOldFile(String),
    /// The old file that the caller gave cannot be read.
    #[error("the old file could not be read: {0}")]
    OldFile(String),
    /// The new file is not a DXF file that can be read, or the options are not valid.
    #[error("{0}")]
    NewFile(String),
    /// A limit stopped the plan.
    #[error("{0}")]
    Limit(String),
    /// The update data of the new revision does not fit (see `base`).
    #[error("{0}")]
    Base(IoError),
}

/// An imported drawing of the document.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawingInfo {
    /// The lineage, as in the tags.
    pub drawing: String,
    /// The revision that was imported last.
    pub revision: String,
    /// The names of its layers.
    pub layers: Vec<String>,
    /// How many objects carry its tag.
    pub objects: usize,
    /// The document keeps the summaries of the entities of this revision.
    pub has_base: bool,
    /// The layers hold a link to the original file.
    pub linked: bool,
}

/// The imported drawings of a document, sorted by lineage.
pub fn drawings(doc: &Document) -> Vec<DrawingInfo> {
    // The layers of each drawing, and how many objects carry its tag.
    type Layers = Vec<(LayerId, String, LayerSource)>;
    let mut by: BTreeMap<String, (Layers, usize)> = BTreeMap::new();
    for l in doc.layers.iter().filter(|l| l.kind == LayerKind::Design) {
        if let Some(src) = l.extra.get(provenance::LAYER_KEY).and_then(LayerSource::of) {
            by.entry(src.drawing.clone())
                .or_default()
                .0
                .push((l.id, l.name.clone(), src));
        }
    }
    for o in doc.objects.values() {
        if let Some(t) = provenance::tag(&o.data)
            && let Some(e) = by.get_mut(&t.drawing)
        {
            e.1 += 1;
        }
    }
    by.into_iter()
        .map(|(drawing, (layers, objects))| {
            // The revision of the layers; if they differ, one that has a base.
            let has = |rev: &str| provenance::base_recs(doc, &drawing, rev).is_some();
            let revision = layers
                .iter()
                .map(|(_, _, s)| s.revision.as_str())
                .find(|r| has(r))
                .unwrap_or_else(|| layers[0].2.revision.as_str())
                .to_string();
            DrawingInfo {
                has_base: has(&revision),
                linked: layers.iter().any(|(_, _, s)| s.link.is_some()),
                layers: layers.iter().map(|(_, n, _)| n.clone()).collect(),
                drawing,
                revision,
                objects,
            }
        })
        .collect()
}

/// What the person or the agent gave for a plan, and how to read a linked file.
pub struct Request<'a> {
    pub doc: &'a Document,
    /// The lineage to update. None: the only drawing, or the one the new file names.
    pub drawing: Option<&'a str>,
    /// The name of the new file, with its extension (for the tags and the version names).
    pub name: &'a str,
    /// The bytes of the new DXF (for a DWG, the converted DXF).
    pub data: &'a [u8],
    /// The link to the new file, for a drawing that is kept as a link.
    pub new_link: Option<FileLink>,
    /// An old file to compare with, when the document has neither a base nor a usable link:
    /// its DXF bytes and its name.
    pub old_file: Option<(&'a [u8], &'a str)>,
    /// Reads the linked original file (the path policy of the caller applies).
    pub read_link: &'a dyn Fn(&FileLink) -> Result<Vec<Rec>, LinkError>,
    /// The import options that the update reuses (hatches, faces, points, tolerance, limits).
    /// `unit`, `layer_mode` and `lineage` are decided from the drawing.
    pub options: &'a ImportOptions,
    /// The caller gave `options.base` itself. If not, the new revision gets update data like the
    /// old one (a link stays a link, none stays none).
    pub base_given: bool,
    /// The tolerance of the nearest match, in units of the drawing. None: automatic.
    pub tolerance: Option<f64>,
    /// Only the entities on these DXF layers (names, any case). Empty: all.
    pub layers: &'a [String],
    /// The most bytes of the plan (the new objects and the new update data).
    pub max_bytes: u64,
}

/// The kind of a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Added,
    Removed,
    Changed,
}

/// The old and the new text of a changed text entity.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextChange {
    pub old: String,
    pub new: String,
}

/// One change, as the person sees it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChangeItem {
    /// A stable ID: the same for the same two files.
    pub id: String,
    pub status: Kind,
    /// The DXF type, for example `LINE`.
    pub entity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// The DXF layer.
    pub layer: String,
    /// The handle of the entity in the old file, if it is in the old file and has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_handle: Option<String>,
    /// The handle of the entity in the new file, if it is in the new file and has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_handle: Option<String>,
    /// What differs, for a changed entity.
    pub what: Vec<Prop>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
    /// How far a changed entity moved, in units of the drawing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextChange>,
    /// Objects of the document that the change replaces or removes (the count is exact).
    pub objects: usize,
    /// Objects the change adds or brings in.
    pub new_objects: usize,
    /// The first objects of the document, for the list.
    pub object_ids: Vec<ObjectId>,
    /// The person edited an object of this change: accepting it overwrites the edit.
    pub conflict: bool,
    /// The edited objects (the first ones).
    pub edited_ids: Vec<ObjectId>,
}

/// The counts of a plan.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Entities that did not change.
    pub unchanged: usize,
    pub changed: usize,
    pub added: usize,
    pub removed: usize,
    /// Changes (of those above) that concern an object the person edited.
    pub conflicts: usize,
    /// Changes of entities that make no object (paper space, entities the importer skips), or whose
    /// objects the person deleted. They are not in the list.
    pub ignored: usize,
    /// Objects the update would add, replace and delete.
    pub objects_added: usize,
    pub objects_replaced: usize,
    pub objects_removed: usize,
}

/// What a change does to objects. IDs of the document, or of the scratch document of the new file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ops {
    /// Objects of the document and the new object that takes their place.
    pub replace: Vec<(ObjectId, ObjectId)>,
    /// Objects of the document that go.
    pub delete: Vec<ObjectId>,
    /// New objects (of the scratch document) that come in.
    pub add: Vec<ObjectId>,
}

/// A layer of the new file, as the import made it.
#[derive(Clone, Debug)]
struct NewLayer {
    name: String,
    visible: bool,
    source: LayerSource,
}

/// A class of the new file.
#[derive(Clone, Debug)]
struct NewClass {
    name: String,
    visible: bool,
    attrs: Attributes,
}

/// An object of the new file that some change brings in.
#[derive(Clone, Debug)]
struct NewObject {
    shape: Shape,
    attrs: Attributes,
    tag: ImportTag,
    layer: usize,
    class: Option<usize>,
}

/// The update data that the new revision gets.
#[derive(Clone)]
enum NewBase {
    Plan(import::BasePlan),
    Link,
    Nothing,
}

/// A plan: what the new revision changes. See the module text.
pub struct Plan {
    pub drawing: String,
    pub old_revision: String,
    pub new_revision: String,
    /// The name of the new file.
    pub name: String,
    pub counts: Counts,
    /// The changes, in the order of the diff (canonical, so the same for the same files).
    pub changes: Vec<ChangeItem>,
    pub notes: Vec<String>,
    /// A limit of the match stopped it: some changes may be shown as removed and added.
    pub truncated: bool,
    /// The new file has other handles than the old one.
    pub renumbered: bool,
    pub tolerance: f64,
    /// Where the old side came from: `base`, `link` or `file`.
    pub old_from: &'static str,
    ops: Vec<Ops>,
    objects: BTreeMap<ObjectId, NewObject>,
    layers: Vec<NewLayer>,
    classes: Vec<NewClass>,
    new_base: NewBase,
    new_link: Option<FileLink>,
    /// The tags (identity fields) that objects get when they are not replaced: entities that did
    /// not change but have another handle or place in the new file.
    refresh: Vec<(ObjectId, ImportTag)>,
    /// The DXF layers (lower case) of a filtered update. Empty: all.
    filter: BTreeSet<String>,
    bytes: u64,
}

impl Plan {
    /// The memory that the plan holds, about.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// What the change at `index` does to objects.
    pub fn ops(&self, index: usize) -> Option<&Ops> {
        self.ops.get(index)
    }

    /// The index of the change with this ID.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.changes.iter().position(|c| c.id == id)
    }

    /// The new object (shape and look) for a new object ID of an [`Ops`], as a document object
    /// with no layer or class of the document (the layer and class are the placeholders).
    pub fn new_object(&self, id: ObjectId) -> Option<Object> {
        let n = self.objects.get(&id)?;
        let mut o = Object::new(id, LayerId(id.0), NONE_CLASS, n.shape.clone());
        o.attrs = n.attrs.clone();
        Some(o)
    }
}

fn names(known: &[DrawingInfo]) -> String {
    known
        .iter()
        .map(|k| k.drawing.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn unit_of(scale: f64) -> Option<Unit> {
    [
        Unit::Mm,
        Unit::Cm,
        Unit::M,
        Unit::Km,
        Unit::Inch,
        Unit::Foot,
        Unit::Yard,
        Unit::Mile,
    ]
    .into_iter()
    .find(|u| (u.mm_per_unit() - scale).abs() <= 1.0e-9 * scale.abs().max(1.0))
}

/// For each record: its place among the records that have the same type, scope and content and
/// no handle (identical entities of a file that has no handles), and how many there are. Such
/// records find the same objects, so each takes its own share of them.
fn twins(recs: &[Rec]) -> Vec<(usize, usize)> {
    let mut count: HashMap<(&str, &str, u64), usize> = HashMap::new();
    let mut rank = Vec::with_capacity(recs.len());
    for r in recs {
        if r.handle.is_some() {
            rank.push((0, 0));
            continue;
        }
        let n = count.entry((&r.kind, &r.scope, r.hash)).or_insert(0);
        rank.push((*n, 0));
        *n += 1;
    }
    for (r, rec) in rank.iter_mut().zip(recs) {
        if rec.handle.is_none() {
            r.1 = count[&(rec.kind.as_str(), rec.scope.as_str(), rec.hash)];
        }
    }
    rank
}

/// The share of `(rank, of)` among identical records of the objects that all of them find. The
/// objects are grouped by their place (the INSERTs on the way, the part) first, so that each
/// record takes its own object of each instance of a block.
fn share(objs: Vec<ObjectId>, (rank, of): (usize, usize), tags: &TagMap<'_>) -> Vec<ObjectId> {
    if of <= 1 {
        return objs;
    }
    let mut groups: BTreeMap<Option<Place>, Vec<ObjectId>> = BTreeMap::new();
    for id in objs {
        groups
            .entry(tags.get(&id).map(|t| place(t)))
            .or_default()
            .push(id);
    }
    let mut out = Vec::new();
    for group in groups.into_values() {
        let each = group.len() / of;
        if each == 0 {
            out.extend(group.get(rank).copied());
            continue;
        }
        let end = if rank + 1 == of {
            group.len()
        } else {
            (rank + 1) * each
        };
        out.extend_from_slice(&group[rank * each..end]);
    }
    out.sort_by_key(|o| o.0);
    out
}

/// True if two tags name the same entity in the same place of the file (the revision, the state
/// of the object and the drawing are not part of it).
fn same_identity(a: &ImportTag, b: &ImportTag) -> bool {
    a.kind == b.kind
        && a.handle == b.handle
        && a.hash == b.hash
        && a.block == b.block
        && a.via == b.via
        && a.part == b.part
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// Makes a plan. See the module text.
pub fn plan(req: &Request<'_>) -> Result<Plan, UpdateError> {
    let doc = req.doc;
    let new_dxf =
        parse::parse(req.data).map_err(|e| UpdateError::NewFile(format!("the new file: {e}")))?;
    let mut tracking = Tracking::new(req.name, &new_dxf);

    // 1. The drawing.
    let known = drawings(doc);
    let info = match (req.drawing, known.len()) {
        (_, 0) => return Err(UpdateError::NoDrawing),
        (Some(d), _) => known
            .iter()
            .find(|k| k.drawing == d)
            .ok_or_else(|| UpdateError::Unknown(d.to_string(), names(&known)))?,
        (None, 1) if known[0].drawing != tracking.drawing => {
            return Err(UpdateError::Mismatch {
                new: tracking.drawing.clone(),
                drawing: known[0].drawing.clone(),
            });
        }
        (None, 1) => &known[0],
        (None, _) => known
            .iter()
            .find(|k| k.drawing == tracking.drawing)
            .ok_or_else(|| UpdateError::Ambiguous(names(&known)))?,
    };
    let drawing = info.drawing.clone();
    let mut notes: Vec<String> = Vec::new();
    if tracking.drawing != drawing {
        notes.push(format!(
            "the new file says it is the drawing {}, not {drawing}: it is used as a new revision of {drawing} because you chose it",
            tracking.drawing
        ));
    }
    let sources: Vec<(LayerId, LayerSource)> = doc
        .layers
        .iter()
        .filter(|l| l.kind == LayerKind::Design)
        .filter_map(|l| Some((l.id, LayerSource::of(l.extra.get(provenance::LAYER_KEY)?)?)))
        .filter(|(_, s)| s.drawing == drawing)
        .collect();
    let first = sources
        .first()
        .map(|(_, s)| s.clone())
        .ok_or(UpdateError::NoDrawing)?;
    let old_revision = info.revision.clone();

    // The revision of the new file. A file that has the header of the old one but other content
    // still needs a revision of its own.
    let mut new_revision = tracking.revision.clone();
    if new_revision == old_revision {
        new_revision = hex(content_hash(&new_dxf));
        if new_revision == old_revision {
            new_revision = format!("{old_revision}+1");
        }
    }
    tracking.drawing = drawing.clone();
    tracking.revision = new_revision.clone();
    tracking.headerless = false;

    // 2. The old side.
    let (old, old_from) = old_records(req, doc, &drawing, &old_revision, &sources, &mut notes)?;

    // 3. The diff.
    let (mut new, cut) = revision::records(&new_dxf, MAX_NEW_RECS);
    if cut {
        return Err(UpdateError::Limit(format!(
            "the new file has more than {MAX_NEW_RECS} entities, more than an update reads"
        )));
    }
    new.iter_mut().for_each(Rec::compact);
    let mut dopts = revision::Options::for_dxf(&new_dxf);
    if let Some(t) = req.tolerance {
        if !(t.is_finite() && t >= 0.0) {
            return Err(UpdateError::NewFile(
                "tolerance must be a number of at least 0".into(),
            ));
        }
        dopts.tolerance = t;
        dopts.max_tolerance = dopts.max_tolerance.max(t);
    }
    let diff: Diff = revision::diff(&old, &new, &dopts);
    if diff.truncated {
        notes.push("a limit stopped the match: some changes may show as a removal and an addition. Check the list, or give a smaller tolerance".into());
    }
    if diff.renumbered {
        notes.push("the handles of the new file are not those of the old file (another program wrote it): the entities were matched by their content and their place".into());
    }

    // 4. The new objects: the new file imported into a scratch document, with the tags of the
    // drawing and the placement of the old import.
    let mut opts = req.options.clone();
    opts.track_source = true;
    opts.base = BaseMode::Off;
    opts.layer = None;
    opts.source_name = Some(req.name.to_string());
    opts.layer_mode = if sources.iter().any(|(_, s)| !s.dxf_layer.is_empty()) {
        LayerMode::Layers
    } else {
        LayerMode::Classes
    };
    opts.unit = unit_of(first.unit_scale);
    opts.lineage = Some(Lineage {
        drawing: drawing.clone(),
        revision: Some(new_revision.clone()),
        shift: first.shift,
    });
    if opts.unit.is_none() {
        return Err(UpdateError::NewFile(
            "the unit of the old import is not a unit that an update can use".into(),
        ));
    }
    let new_unit = new_dxf
        .header_value("$INSUNITS")
        .and_then(parse::parse_int)
        .and_then(Unit::from_insunits);
    if let Some(u) = new_unit
        && opts.unit != Some(u)
    {
        notes.push(format!(
            "the new file is in another unit ({u:?}) than the old import: the old unit is used, so the objects stay where they were, but the comparison of the entities may show every entity as changed"
        ));
    }
    let scratch_doc = Document::with_sequential_ids();
    let imported =
        import::import_parsed(req.name, &new_dxf, &Context::of(&scratch_doc), &opts, None)
            .map_err(|e| UpdateError::NewFile(e.to_string()))?;
    let mut editor = Editor::new(scratch_doc);
    editor
        .execute(&imported.command)
        .map_err(|e| UpdateError::NewFile(format!("the new file could not be read: {e}")))?;
    let scratch = editor.doc();
    let skipped = imported.report.skipped_total();
    if skipped > 0 {
        notes.push(format!(
            "{skipped} entities of the new file are not imported (see report.skipped of import.file for the reasons); a change of such an entity is not in the list"
        ));
    }
    if imported
        .report
        .notes
        .iter()
        .any(|n| n.contains("stopped at the object limit"))
    {
        return Err(UpdateError::Limit(
            "the new file makes more objects than the limit of objects allows, so the update would miss changes. The whole file is read whatever `layers` says: raise max_objects".into(),
        ));
    }
    for n in &imported.report.notes {
        // The scratch import keeps no update data and says so: that is not the news of the update.
        if !n.contains("imported before") && !n.contains("no update data was kept") {
            notes.push(n.clone());
        }
    }

    // 5. The base that the new revision gets. It follows the old import (a link stays a link,
    // none stays none) unless the caller chose.
    let wanted: BTreeSet<String> = req.layers.iter().map(|l| lower(l)).collect();
    let on_layer = |r: &Rec| wanted.is_empty() || wanted.contains(&lower(&r.layer));
    let link_mode = sources.iter().any(|(_, s)| s.link.is_some());
    if req.base_given && req.options.base == BaseMode::Off && !wanted.is_empty() {
        return Err(UpdateError::NewFile(
            "base \"none\" cannot be used with `layers`: the layers that are not updated need their update data. Update all layers, or leave base out".into(),
        ));
    }
    let mode = if req.base_given {
        req.options.base
    } else if link_mode && req.new_link.is_some() {
        BaseMode::Link
    } else if !info.has_base && !link_mode {
        BaseMode::Off
    } else {
        BaseMode::Auto
    };
    let new_base = match mode {
        BaseMode::Off => NewBase::Nothing,
        BaseMode::Link if req.new_link.is_none() => {
            return Err(UpdateError::NewFile(
                "base \"link\" needs a file on disk: no link could be made".into(),
            ));
        }
        BaseMode::Link if wanted.is_empty() => NewBase::Link,
        BaseMode::Link | BaseMode::Auto => {
            let mut t = Tracking::new(req.name, &new_dxf);
            t.drawing = drawing.clone();
            t.revision = new_revision.clone();
            let caps = (req.options.base_caps, first.unit_scale);
            let plan = if wanted.is_empty() {
                import::plan_base(BaseMode::Auto, &new_dxf, &mut t, false, None, caps)
            } else {
                // A filtered update moves only the layers it names: the update data keeps the
                // old entities of the other layers, so a later update offers their changes.
                if mode == BaseMode::Link {
                    notes.push("an update of some layers keeps update data in the document, not a link (a link cannot be kept for some layers only): the other layers are still at the old revision".into());
                }
                let mut recs: Vec<Rec> = Vec::new();
                for e in &diff.entries {
                    let (b, n) = (e.base.map(|i| &old[i]), e.new.map(|i| &new[i]));
                    let inside = b.is_some_and(on_layer) || n.is_some_and(on_layer);
                    if let Some(r) = if inside { n } else { b } {
                        recs.push(r.clone());
                    }
                }
                import::summary_plan(recs, &t, caps.0, caps.1)
            }
            .map_err(|e| match e {
                // A link cannot be kept for some layers only: the question about a link would
                // come back, so the answer is the way out.
                IoError::BaseChoice { .. } if !wanted.is_empty() && mode == BaseMode::Link => {
                    UpdateError::NewFile(
                        "the update data of this drawing is too big to keep for an update of some layers, and a link cannot be kept for some layers only. Update all layers at once (without `layers`) to keep a link".into(),
                    )
                }
                e => UpdateError::Base(e),
            })?;
            NewBase::Plan(plan)
        }
    };
    let new_link = match new_base {
        NewBase::Link => req.new_link.clone(),
        _ => None,
    };
    if matches!(new_base, NewBase::Nothing) {
        notes.push("no update data is kept for the new revision (base: none): the next update needs the old file".into());
    }

    // 6. The changes, with their objects.
    let mut tags: Vec<(ObjectId, ImportTag)> = Vec::new();
    for o in doc.objects.values() {
        if let Some(t) = provenance::tag(&o.data)
            && t.drawing == drawing
        {
            tags.push((o.id, t));
        }
    }
    let mut new_tags: Vec<(ObjectId, ImportTag)> = Vec::new();
    for o in scratch.objects.values() {
        if let Some(t) = provenance::tag(&o.data) {
            new_tags.push((o.id, t));
        }
    }
    let old_index = TagIndex::new(&drawing, tags.iter().map(|(i, t)| (*i, t)));
    let new_index = TagIndex::new(&drawing, new_tags.iter().map(|(i, t)| (*i, t)));
    let tag_of_old: HashMap<ObjectId, &ImportTag> = tags.iter().map(|(i, t)| (*i, t)).collect();
    let tag_of_new: HashMap<ObjectId, &ImportTag> = new_tags.iter().map(|(i, t)| (*i, t)).collect();

    let mut st = Planner {
        scratch,
        tag_of_old,
        tag_of_new,
        new_layers: Vec::new(),
        new_classes: Vec::new(),
        layer_ix: HashMap::new(),
        class_ix: HashMap::new(),
        objects: BTreeMap::new(),
        bytes: 0,
        max_bytes: req.max_bytes,
    };
    let mut counts = Counts::default();
    let mut changes: Vec<ChangeItem> = Vec::new();
    let mut ops_list: Vec<Ops> = Vec::new();
    let mut seen_ids: HashMap<String, u32> = HashMap::new();
    let mut refresh: Vec<(ObjectId, ImportTag)> = Vec::new();
    let (old_twins, new_twins) = (twins(&old), twins(&new));
    let centers = |new_side: bool, id: ObjectId| {
        let d = if new_side { scratch } else { doc };
        d.objects.get(&id).and_then(|o| center_of(&o.shape))
    };
    for e in &diff.entries {
        let (b, n) = (e.base.map(|i| &old[i]), e.new.map(|i| &new[i]));
        let old_objects =
            |r: &Rec, i: usize, t: &TagMap<'_>| share(old_index.objects_of(r), old_twins[i], t);
        let new_objects =
            |r: &Rec, i: usize, t: &TagMap<'_>| share(new_index.objects_of(r), new_twins[i], t);
        let rec = n.or(b).expect("an entry has a side");
        if e.status == Status::Unchanged {
            if on_layer(rec) {
                counts.unchanged += 1;
                // The entity is the same but the file may name it otherwise (another handle, an
                // INSERT with another handle): its objects take the identity of the new file, or
                // the next update would not find them.
                if let (Some(b), Some(n)) = (b, n)
                    && b.kind != "INSERT"
                    && b.scope != PAPER
                {
                    let (o, w) = (
                        old_objects(b, e.base.expect("a side"), &st.tag_of_old),
                        new_objects(n, e.new.expect("a side"), &st.tag_of_new),
                    );
                    if !o.is_empty() && !w.is_empty() {
                        let (pairs, _, _) =
                            pair_objects(&st.tag_of_old, &st.tag_of_new, &o, &w, &centers);
                        for (d, nw) in pairs {
                            let (ot, nt) = (st.tag_of_old[&d], st.tag_of_new[&nw]);
                            if !same_identity(ot, nt) {
                                refresh.push((d, nt.clone()));
                            }
                        }
                    }
                }
            }
            continue;
        }
        if !(b.is_some_and(on_layer) || n.is_some_and(on_layer)) {
            continue;
        }
        let old_objs = match (b, e.base) {
            (Some(r), Some(i)) => old_objects(r, i, &st.tag_of_old),
            _ => Vec::new(),
        };
        let new_objs = match (n, e.new) {
            (Some(r), Some(i)) => new_objects(r, i, &st.tag_of_new),
            _ => Vec::new(),
        };
        let (status, ops) = classify(
            e.status,
            &old_objs,
            &new_objs,
            &st.tag_of_old,
            &st.tag_of_new,
            &centers,
        );
        let nothing = ops.replace.is_empty() && ops.delete.is_empty() && ops.add.is_empty();
        if nothing {
            counts.ignored += 1;
            continue;
        }
        let touched: Vec<ObjectId> = ops
            .replace
            .iter()
            .map(|(d, _)| *d)
            .chain(ops.delete.iter().copied())
            .collect();
        let edited: Vec<ObjectId> = touched
            .iter()
            .copied()
            .filter(|id| {
                doc.objects
                    .get(id)
                    .is_some_and(|o| provenance::is_edited(o) == Some(true))
            })
            .collect();
        st.claim(&ops)?;
        let id = change_id(e.status, b, n, &mut seen_ids);
        match status {
            Kind::Added => counts.added += 1,
            Kind::Removed => counts.removed += 1,
            Kind::Changed => counts.changed += 1,
        }
        if !edited.is_empty() {
            counts.conflicts += 1;
        }
        counts.objects_added += ops.add.len();
        counts.objects_replaced += ops.replace.len();
        counts.objects_removed += ops.delete.len();
        changes.push(item(id, status, e, (b, n), &ops, &edited));
        ops_list.push(ops);
    }
    if counts.ignored > 0 {
        notes.push(format!(
            "{} changed entities make no object (paper space, entities that are not imported, or objects that were deleted): they are not in the list",
            counts.ignored
        ));
    }
    let bytes = st.bytes
        + match &new_base {
            NewBase::Plan(import::BasePlan::Summary(v, _)) => {
                v.iter().map(|b| b.to_string().len() as u64).sum()
            }
            _ => 0,
        }
        + (changes.len() as u64) * 400
        + (refresh.len() as u64) * 300;
    if bytes > req.max_bytes {
        return Err(UpdateError::Limit(format!(
            "the update is too large to hold ({bytes} bytes, the limit is {}): limit it to some DXF layers with `layers`",
            req.max_bytes
        )));
    }
    Ok(Plan {
        drawing,
        old_revision,
        new_revision,
        name: req.name.to_string(),
        counts,
        changes,
        notes,
        truncated: diff.truncated,
        renumbered: diff.renumbered,
        tolerance: diff.tolerance,
        old_from,
        ops: ops_list,
        objects: st.objects,
        layers: st.new_layers,
        classes: st.new_classes,
        new_base,
        new_link,
        refresh,
        filter: wanted,
        bytes,
    })
}

pub(super) fn clip(s: &str) -> String {
    s.chars().take(120).collect()
}

pub(super) fn change_id(
    status: Status,
    old: Option<&Rec>,
    new: Option<&Rec>,
    seen: &mut HashMap<String, u32>,
) -> String {
    let mut h = provenance::Hasher::new();
    h.str(match status {
        Status::Added => "a",
        Status::Removed => "r",
        _ => "c",
    });
    for r in [old, new] {
        match r {
            Some(r) => {
                h.str(&r.scope);
                h.str(r.handle.as_deref().unwrap_or(""));
                h.u64(r.hash);
                h.str(&r.kind);
            }
            None => h.str("-"),
        }
    }
    let base = format!("c{}", &hex(h.finish())[..10]);
    let n = seen.entry(base.clone()).or_insert(0);
    *n += 1;
    if *n == 1 { base } else { format!("{base}-{n}") }
}

/// The old records: the stored base, the linked file, or the file that the caller gave.
fn old_records(
    req: &Request<'_>,
    doc: &Document,
    drawing: &str,
    revision: &str,
    sources: &[(LayerId, LayerSource)],
    notes: &mut Vec<String>,
) -> Result<(Vec<Rec>, &'static str), UpdateError> {
    if let Some((data, name)) = req.old_file {
        let recs = super::link::records_of_dxf(data, name, MAX_NEW_RECS)
            .map_err(|e| UpdateError::OldFile(e.to_string()))?;
        notes.push(format!(
            "the old side is the file {name} that you gave, not the update data of the document"
        ));
        return Ok((recs, "file"));
    }
    if let Some(values) = provenance::base_recs(doc, drawing, revision) {
        let mut recs = Vec::with_capacity(values.len());
        for v in values {
            recs.push(Rec::from_value(v).ok_or_else(|| {
                UpdateError::NeedsOldFile(
                    "the update data of the drawing is not valid: give the old file with `old`"
                        .into(),
                )
            })?);
        }
        return Ok((recs, "base"));
    }
    let link = sources
        .iter()
        .find(|(_, s)| s.revision == revision && s.link.is_some())
        .or_else(|| sources.iter().find(|(_, s)| s.link.is_some()))
        .and_then(|(_, s)| s.link.clone());
    match link {
        Some(link) => match (req.read_link)(&link) {
            Ok(recs) => Ok((recs, "link")),
            Err(e) => Err(UpdateError::NeedsOldFile(format!(
                "{e}. Give the old file with `old`"
            ))),
        },
        None => Err(UpdateError::NeedsOldFile(
            "the document has no update data for this drawing (it was imported with base: none, or the data was purged). Give the old file with `old`".into(),
        )),
    }
}

/// The state of the walk over the changes.
struct Planner<'a> {
    scratch: &'a Document,
    tag_of_old: TagMap<'a>,
    tag_of_new: TagMap<'a>,
    new_layers: Vec<NewLayer>,
    new_classes: Vec<NewClass>,
    layer_ix: HashMap<LayerId, usize>,
    class_ix: HashMap<ClassId, usize>,
    objects: BTreeMap<ObjectId, NewObject>,
    bytes: u64,
    max_bytes: u64,
}

/// Where an object sits in the file: the block, the INSERTs on the way, the part.
pub(super) type Place = (
    Option<String>,
    Vec<(Option<String>, Option<u32>)>,
    Option<u32>,
);

fn place(t: &ImportTag) -> Place {
    (
        t.block.as_ref().map(|b| b.key.clone()),
        t.via.iter().map(|v| (v.handle.clone(), v.copy)).collect(),
        t.part,
    )
}

/// The objects of a document, by ID, with their tags.
pub(super) type TagMap<'a> = HashMap<ObjectId, &'a ImportTag>;

/// Where an object is: the middle of its box. `true` asks about the new side.
pub(super) type Centers<'a> = &'a dyn Fn(bool, ObjectId) -> Option<[f64; 2]>;

/// The middle of the box of a shape, if it has a finite one.
pub(super) fn center_of(shape: &Shape) -> Option<[f64; 2]> {
    let (a, b) = shape.bounds();
    (a.is_finite() && b.is_finite()).then(|| [(a.x + b.x) / 2.0, (a.y + b.y) / 2.0])
}

/// The place of an object without the handles, which another program renumbers: the name of the
/// block, the INSERTs on the way by their content (position, scale, rotation) and copy, the part.
type Content = (Option<String>, Vec<(String, Option<u32>)>, Option<u32>);

fn content_place(t: &ImportTag) -> Content {
    (
        t.block.as_ref().map(|b| b.name.clone()),
        t.via.iter().map(|v| (v.hash.clone(), v.copy)).collect(),
        t.part,
    )
}

/// Most pairs of leftovers that are compared by distance.
const NEAREST_WORK: usize = 4_000_000;

/// Pairs the objects of the old side with the new ones by their place in the file. What is left
/// over is paired by the content of the INSERTs on the way (the handles may be new), then by the
/// nearest position, and objects that are not reached through an INSERT by their order. The rest
/// goes (old) or comes (new). An object is never paired with another instance of a block by the
/// order of its ID.
pub(super) fn pair_objects(
    tag_of_old: &TagMap<'_>,
    tag_of_new: &TagMap<'_>,
    old: &[ObjectId],
    new: &[ObjectId],
    centers: Centers<'_>,
) -> (Vec<(ObjectId, ObjectId)>, Vec<ObjectId>, Vec<ObjectId>) {
    let mut by_place: BTreeMap<Place, Vec<ObjectId>> = BTreeMap::new();
    for id in new {
        if let Some(t) = tag_of_new.get(id) {
            by_place.entry(place(t)).or_default().push(*id);
        }
    }
    for v in by_place.values_mut() {
        v.reverse();
    }
    let mut pairs = Vec::new();
    let mut gone = Vec::new();
    for id in old {
        let hit = tag_of_old
            .get(id)
            .and_then(|t| by_place.get_mut(&place(t)))
            .and_then(Vec::pop);
        match hit {
            Some(n) => pairs.push((*id, n)),
            None => gone.push(*id),
        }
    }
    let mut fresh: Vec<ObjectId> = by_place.into_values().flatten().collect();
    fresh.sort_by_key(|o| o.0);
    // The same content of the INSERTs, whatever their handles.
    let mut by_content: BTreeMap<Content, Vec<ObjectId>> = BTreeMap::new();
    for id in &fresh {
        if let Some(t) = tag_of_new.get(id) {
            by_content.entry(content_place(t)).or_default().push(*id);
        }
    }
    for v in by_content.values_mut() {
        v.reverse();
    }
    let mut still_gone = Vec::new();
    let mut taken: BTreeSet<ObjectId> = BTreeSet::new();
    for id in gone {
        let hit = tag_of_old
            .get(&id)
            .and_then(|t| by_content.get_mut(&content_place(t)))
            .and_then(Vec::pop);
        match hit {
            Some(n) => {
                taken.insert(n);
                pairs.push((id, n));
            }
            None => still_gone.push(id),
        }
    }
    fresh.retain(|f| !taken.contains(f));
    // Objects with no INSERT on the way (the same entity with another part) are paired in order.
    let mut left_old = Vec::new();
    for id in still_gone {
        let plain = tag_of_old.get(&id).is_some_and(|t| t.via.is_empty());
        let mate = plain
            .then(|| {
                fresh
                    .iter()
                    .position(|f| tag_of_new.get(f).is_some_and(|t| t.via.is_empty()))
            })
            .flatten();
        match mate {
            Some(i) => pairs.push((id, fresh.remove(i))),
            None => left_old.push(id),
        }
    }
    // The rest by the nearest position, when the work is bounded.
    if left_old.len().saturating_mul(fresh.len()) > NEAREST_WORK {
        return (pairs, left_old, fresh);
    }
    let mut gone = Vec::new();
    for id in left_old {
        let best = centers(false, id).and_then(|c| {
            fresh
                .iter()
                .enumerate()
                .filter_map(|(i, f)| {
                    let q = centers(true, *f)?;
                    Some((i, (q[0] - c[0]).hypot(q[1] - c[1])))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
        });
        match best {
            Some((i, _)) => pairs.push((id, fresh.remove(i))),
            None => gone.push(id),
        }
    }
    (pairs, gone, fresh)
}

/// What a diff entry does to objects, from the objects of the two sides.
pub(super) fn classify(
    status: Status,
    old_objs: &[ObjectId],
    new_objs: &[ObjectId],
    tag_of_old: &TagMap<'_>,
    tag_of_new: &TagMap<'_>,
    centers: Centers<'_>,
) -> (Kind, Ops) {
    let mut ops = Ops::default();
    let mut kind = match status {
        Status::Added => Kind::Added,
        Status::Removed => Kind::Removed,
        _ => Kind::Changed,
    };
    match kind {
        Kind::Added => ops.add = new_objs.to_vec(),
        Kind::Removed => ops.delete = old_objs.to_vec(),
        Kind::Changed => {
            let (pairs, gone, fresh) =
                pair_objects(tag_of_old, tag_of_new, old_objs, new_objs, centers);
            ops.replace = pairs;
            ops.delete = gone;
            ops.add = fresh;
            if ops.replace.is_empty() && ops.add.is_empty() && !ops.delete.is_empty() {
                // The importer no longer reads the entity: its objects go.
                kind = Kind::Removed;
            }
        }
    }
    (kind, ops)
}

/// The change as the person sees it. `edited` are the objects of the old side that the person
/// edited.
pub(super) fn item(
    id: String,
    status: Kind,
    e: &revision::Entry,
    (b, n): (Option<&Rec>, Option<&Rec>),
    ops: &Ops,
    edited: &[ObjectId],
) -> ChangeItem {
    let rec = n.or(b).expect("an entry has a side");
    let touched: Vec<ObjectId> = ops
        .replace
        .iter()
        .map(|(d, _)| *d)
        .chain(ops.delete.iter().copied())
        .collect();
    let text = match (b, n) {
        (Some(b), Some(n)) if e.changes.contains(&Prop::Text) => Some(TextChange {
            old: clip(&b.text),
            new: clip(&n.text),
        }),
        _ => None,
    };
    ChangeItem {
        id,
        status,
        entity: rec.kind.clone(),
        handle: rec.handle.clone(),
        layer: rec.layer.clone(),
        base_handle: b.and_then(|r| r.handle.clone()),
        new_handle: n.and_then(|r| r.handle.clone()),
        what: e.changes.clone(),
        via: e.via,
        distance: e.distance,
        text,
        objects: touched.len(),
        new_objects: ops.add.len() + ops.replace.len(),
        object_ids: touched.iter().copied().take(LISTED_OBJECTS).collect(),
        conflict: !edited.is_empty(),
        edited_ids: edited.iter().copied().take(LISTED_OBJECTS).collect(),
    }
}

impl Planner<'_> {
    /// Keeps the new objects that a change brings, within the size limit.
    fn claim(&mut self, ops: &Ops) -> Result<(), UpdateError> {
        let ids: Vec<ObjectId> = ops
            .add
            .iter()
            .copied()
            .chain(ops.replace.iter().map(|(_, n)| *n))
            .collect();
        for id in ids {
            if self.objects.contains_key(&id) {
                continue;
            }
            let Some(o) = self.scratch.objects.get(&id) else {
                continue;
            };
            let Some(tag) = provenance::tag(&o.data) else {
                continue;
            };
            let layer = self.layer(o.layer);
            let class = self.class(o.class);
            self.bytes += 600
                + serde_json::to_string(&o.shape).map_or(0, |s| s.len() as u64)
                + serde_json::to_string(&o.data).map_or(0, |s| s.len() as u64);
            if self.bytes > self.max_bytes {
                return Err(UpdateError::Limit(format!(
                    "the update is too large to hold (the limit is {} bytes): limit it to some DXF layers with `layers`",
                    self.max_bytes
                )));
            }
            self.objects.insert(
                id,
                NewObject {
                    shape: o.shape.clone(),
                    attrs: o.attrs.clone(),
                    tag,
                    layer,
                    class,
                },
            );
        }
        Ok(())
    }

    fn layer(&mut self, id: LayerId) -> usize {
        if let Some(i) = self.layer_ix.get(&id) {
            return *i;
        }
        let l = self.scratch.layer(id);
        let source = l
            .and_then(|l| l.extra.get(provenance::LAYER_KEY))
            .and_then(LayerSource::of)
            .unwrap_or(LayerSource {
                drawing: String::new(),
                revision: String::new(),
                dxf_layer: String::new(),
                unit_scale: 1.0,
                shift: [0.0, 0.0],
                link: None,
            });
        self.new_layers.push(NewLayer {
            name: l.map_or_else(|| "Imported".into(), |l| l.name.clone()),
            visible: l.is_none_or(|l| l.visible),
            source,
        });
        let i = self.new_layers.len() - 1;
        self.layer_ix.insert(id, i);
        i
    }

    fn class(&mut self, id: ClassId) -> Option<usize> {
        if id == NONE_CLASS {
            return None;
        }
        if let Some(i) = self.class_ix.get(&id) {
            return Some(*i);
        }
        let c = self.scratch.class(id)?;
        self.new_classes.push(NewClass {
            name: c.name.clone(),
            visible: c.visible,
            attrs: c.attrs.clone(),
        });
        let i = self.new_classes.len() - 1;
        self.class_ix.insert(id, i);
        Some(i)
    }
}

/// What to do with a change when the command is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// Leave the change out.
    No,
    /// Apply the change to the objects that the person did not edit, and keep the edited ones as
    /// they are (what "accept all" does with a conflict).
    KeepEdits,
    /// Apply the change to every object, the edited ones too.
    All,
}

impl From<bool> for Take {
    fn from(on: bool) -> Take {
        if on { Take::All } else { Take::No }
    }
}

/// What [`Plan::build`] made.
pub struct Built {
    /// One command: the apply as one undo step.
    pub command: Command,
    /// The IDs that the new objects get in the document: (ID in the plan, ID in the document).
    pub added: Vec<(ObjectId, ObjectId)>,
    /// Counts of what the command does.
    pub applied: Applied,
    /// Things the person should know.
    pub notes: Vec<String>,
}

/// What a built command does.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Applied {
    pub added: usize,
    pub replaced: usize,
    pub removed: usize,
    /// Objects the command leaves alone because their layer is locked (not a layer of the drawing).
    pub skipped_locked: usize,
    /// Objects the person edited, that a change would have replaced or removed, and that were kept
    /// (`Take::KeepEdits`).
    pub kept_edits: usize,
    /// Parts of the update data of the old revision that the command deletes.
    pub dropped_base: usize,
}

impl Plan {
    /// The command that applies the changes for which `chosen` is true (one flag for each change
    /// of the plan, in order). The document must be the one the plan was made for.
    ///
    /// The command adds the new objects with their tags, replaces the shape and look of the
    /// changed ones (with the new revision in their tag), deletes the removed ones, renews the
    /// source of the layers of the drawing and stores the update data of the new revision. Objects
    /// that did not change keep the revision they came from: that way the update does not show up
    /// in the history of every object, and costs what changed.
    pub fn build(&self, doc: &Document, chosen: &[Take]) -> Result<Built, UpdateError> {
        let mut b = Builder::new(&Context::of(doc), "dxf", usize::MAX);
        let mut notes = Vec::new();
        let mut applied = Applied::default();
        // The layers of the drawing take the new revision (and are unlocked for the update).
        let mut drawing_layers: BTreeSet<LayerId> = BTreeSet::new();
        for l in doc.layers.iter().filter(|l| l.kind == LayerKind::Design) {
            let Some(src) = l.extra.get(provenance::LAYER_KEY).and_then(LayerSource::of) else {
                continue;
            };
            if src.drawing != self.drawing {
                continue;
            }
            // A filtered update moves only the layers it names (a layer that holds the whole
            // file has no DXF layer name and always moves).
            if !self.filter.is_empty()
                && !src.dxf_layer.is_empty()
                && !self.filter.contains(&lower(&src.dxf_layer))
            {
                drawing_layers.insert(l.id);
                continue;
            }
            let renewed = LayerSource {
                revision: self.new_revision.clone(),
                link: self.new_link.clone(),
                ..src
            };
            let id = b.source_layer(&l.name, l.visible, &renewed, &self.name);
            drawing_layers.insert(id);
        }

        // What the chosen changes do, once for each object.
        let mut replace: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
        let mut delete: BTreeSet<ObjectId> = BTreeSet::new();
        let mut add: BTreeSet<ObjectId> = BTreeSet::new();
        let edited = |id: &ObjectId| {
            doc.objects
                .get(id)
                .is_some_and(|o| provenance::is_edited(o) == Some(true))
        };
        for (i, ops) in self.ops.iter().enumerate() {
            let take = chosen.get(i).copied().unwrap_or(Take::No);
            if take == Take::No {
                continue;
            }
            let keep = take == Take::KeepEdits;
            for (d, n) in &ops.replace {
                if keep && edited(d) {
                    applied.kept_edits += 1;
                } else {
                    replace.insert(*d, *n);
                }
            }
            for d in &ops.delete {
                if keep && edited(d) {
                    applied.kept_edits += 1;
                } else {
                    delete.insert(*d);
                }
            }
            add.extend(ops.add.iter().copied());
        }
        delete.retain(|d| !replace.contains_key(d));
        // Only objects of this drawing, and not on a layer that is locked for other reasons.
        let allowed = |id: &ObjectId| -> bool {
            doc.objects.get(id).is_some_and(|o| {
                provenance::tag(&o.data).is_some_and(|t| t.drawing == self.drawing)
                    && (drawing_layers.contains(&o.layer)
                        || doc.layer(o.layer).is_none_or(|l| !l.locked))
            })
        };
        let before = replace.len() + delete.len();
        replace.retain(|d, _| allowed(d));
        delete.retain(allowed);
        applied.skipped_locked = before - replace.len() - delete.len();
        if applied.skipped_locked > 0 {
            notes.push(format!(
                "{} objects are on a locked layer that is not a layer of the drawing: they were left as they are",
                applied.skipped_locked
            ));
        }

        // Objects that stay as they are take the identity of their entity in the new file (handle,
        // hash, block, INSERTs), because the update data now holds the records of the new file.
        // This covers entities that did not change but were renumbered, and changes that were
        // left out or kept edits. The revision and the state of the object stay.
        let mut refreshed: BTreeMap<ObjectId, &ImportTag> =
            self.refresh.iter().map(|(d, t)| (*d, t)).collect();
        for ops in &self.ops {
            for (d, n) in &ops.replace {
                if let Some(o) = self.objects.get(n) {
                    refreshed.insert(*d, &o.tag);
                }
            }
        }
        refreshed.retain(|d, _| !replace.contains_key(d) && !delete.contains(d) && allowed(d));

        let place = |b: &mut Builder, n: &NewObject| -> (LayerId, ClassId, Attributes) {
            let l = &self.layers[n.layer];
            let src = LayerSource {
                revision: self.new_revision.clone(),
                link: self.new_link.clone(),
                ..l.source.clone()
            };
            let layer = b.source_layer(&l.name, l.visible, &src, &self.name);
            let class = match n.class {
                Some(c) => {
                    let c = &self.classes[c];
                    b.class(&c.name, &c.attrs, c.visible)
                }
                None => NONE_CLASS,
            };
            (layer, class, n.attrs.clone())
        };

        // New objects.
        let mut added = Vec::new();
        for id in &add {
            let Some(n) = self.objects.get(id) else {
                continue;
            };
            let (layer, class, attrs) = place(&mut b, n);
            let mut tag = n.tag.clone();
            tag.revision = self.new_revision.clone();
            tag.obj = provenance::PENDING_OBJ.into();
            let mut data = BTreeMap::new();
            data.insert(provenance::KEY.to_string(), tag.to_value());
            if b.add(&n.tag.kind, n.shape.clone(), layer, class, attrs, data) {
                added.push((*id, ObjectId(b.last_id())));
                applied.added += 1;
            }
        }
        // Changed objects.
        let mut touched: BTreeSet<ObjectId> = BTreeSet::new();
        for (old, new) in &replace {
            let Some(n) = self.objects.get(new) else {
                continue;
            };
            let (layer, class, attrs) = place(&mut b, n);
            let mut tag = n.tag.clone();
            tag.revision = self.new_revision.clone();
            tag.obj = provenance::state_hash(&n.shape, &attrs, layer, class);
            b.push(Command::ModifyObject {
                id: *old,
                shape: Some(n.shape.clone()),
                layer: Some(layer),
                class: Some(class),
                attrs: Some(attrs),
            });
            b.push(Command::SetData {
                id: *old,
                set: BTreeMap::from([(provenance::KEY.to_string(), tag.to_value())]),
                remove: vec![],
            });
            touched.insert(*old);
            applied.replaced += 1;
        }
        // Objects that stay: the new identity.
        for (id, new) in &refreshed {
            let Some(mut tag) = doc.objects.get(id).and_then(|o| provenance::tag(&o.data)) else {
                continue;
            };
            if same_identity(&tag, new) {
                continue;
            }
            tag.kind = new.kind.clone();
            tag.handle = new.handle.clone();
            tag.hash = new.hash.clone();
            tag.block = new.block.clone();
            tag.via = new.via.clone();
            tag.part = new.part;
            b.push(Command::SetData {
                id: *id,
                set: BTreeMap::from([(provenance::KEY.to_string(), tag.to_value())]),
                remove: vec![],
            });
        }
        // Removed objects.
        if !delete.is_empty() {
            applied.removed = delete.len();
            b.push(Command::DeleteObjects {
                ids: delete.iter().copied().collect(),
            });
        }
        // The update data moves to the new revision: the old summaries are of no use to the layers
        // any more (an update reads the base of the revision of the layer), so they go in the same
        // step, and undo brings them back. Untouched objects keep the revision they came from.
        for r in doc.resources.iter() {
            if let ResourceBody::ImportBase(v) = &r.body
                && v["drawing"] == self.drawing.as_str()
                && v["revision"] == self.old_revision.as_str()
            {
                b.push(Command::DeleteResource {
                    id: r.id,
                    reassign: None,
                });
                applied.dropped_base += 1;
            }
        }
        if let NewBase::Plan(plan) = &self.new_base {
            import::store_base(&mut b, plan.clone());
        }
        let result = b.finish();
        Ok(Built {
            command: result.command,
            added,
            applied,
            notes,
        })
    }
}

#[cfg(test)]
mod tests;
