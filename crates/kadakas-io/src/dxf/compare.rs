//! Compare two DXF files without importing them into a document (IOP-24).
//!
//! The two files are matched with the engine of the update ([`revision::diff`], the same rules
//! as IOP-23) and each change is mapped to objects with the same index. The objects come from a
//! scratch import of each file. The new file makes the *new document*. The objects of the old
//! file are added to a copy of it, the *all document*, with IDs that cannot meet those of the new
//! one, and the old file is placed with the unit and the shift of the new one, so the two line up.
//! A caller draws the new document with the overlay of the changes (added and changed objects are
//! in it, removed objects are ghosts of the all document).
//!
//! The module is pure: it reads no file. The files are untrusted, so the work is bounded like an
//! update (entities, the work of the match, the objects of the import).

use std::collections::{HashMap, HashSet};

use kadakas_core::command::Editor;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::provenance::{self, ImportTag, LayerSource};
use serde::Serialize;

use super::import;
use super::parse;
use super::revision::{self, Rec, Status, TagIndex};
use super::source::Tracking;
use super::update::{
    ChangeItem, Counts, Kind, MAX_NEW_RECS, UpdateError, change_id, classify, item, unit_of,
};
use crate::{BaseMode, Context, ImportOptions, LayerMode, Lineage};

/// What to compare.
pub struct Request<'a> {
    /// The old file: its DXF bytes and its name with the extension.
    pub base: (&'a [u8], &'a str),
    /// The new file.
    pub new: (&'a [u8], &'a str),
    /// The import options for both files (hatches, faces, points, limits). The unit, the layer
    /// mode and the tracking are decided here.
    pub options: &'a ImportOptions,
    /// The tolerance of the nearest match, in units of the drawing. None: automatic.
    pub tolerance: Option<f64>,
    /// Only the entities on these DXF layers (any case). Empty: all.
    pub layers: &'a [String],
}

/// The status of one object of the comparison, for an overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectChange {
    /// The object: of the new document (added, changed), or of the all document (removed).
    pub id: ObjectId,
    pub status: Kind,
    /// For a changed object, the object it was in the all document.
    pub old: Option<ObjectId>,
}

/// The result of a comparison.
pub struct Compared {
    pub items: Vec<ChangeItem>,
    pub counts: Counts,
    pub notes: Vec<String>,
    pub truncated: bool,
    pub renumbered: bool,
    pub tolerance: f64,
    /// The new file as a document.
    pub new_doc: Document,
    /// The new document and the objects of the old file (they have their own IDs).
    pub all_doc: Document,
    /// One entry for each changed object.
    pub changes: Vec<ObjectChange>,
    /// The name of the new file, for the labels.
    pub name: String,
    /// The name of the old file.
    pub base_name: String,
}

fn scratch(
    name: &str,
    dxf: &parse::Dxf,
    ctx: &Context,
    opts: &ImportOptions,
    side: &str,
) -> Result<crate::ImportResult, UpdateError> {
    let r = import::import_parsed(name, dxf, ctx, opts, None)
        .map_err(|e| UpdateError::NewFile(format!("the {side} file could not be read: {e}")))?;
    // A comparison that stops at the limit of objects would miss changes: it is refused.
    if r.report
        .notes
        .iter()
        .any(|n| n.contains("stopped at the object limit"))
    {
        return Err(UpdateError::Limit(format!(
            "the {side} file makes more objects than the limit of objects allows, so the comparison would miss changes: raise max_objects"
        )));
    }
    Ok(r)
}

/// Compares two files. See the module text.
pub fn compare(req: &Request<'_>) -> Result<Compared, UpdateError> {
    let (old_dxf, new_dxf) = (
        parse::parse(req.base.0).map_err(|e| UpdateError::OldFile(e.to_string()))?,
        parse::parse(req.new.0).map_err(|e| UpdateError::NewFile(format!("the new file: {e}")))?,
    );
    let mut notes = Vec::new();

    // The diff.
    let (old, cut_old) = revision::records(&old_dxf, MAX_NEW_RECS);
    let (new, cut_new) = revision::records(&new_dxf, MAX_NEW_RECS);
    if cut_old || cut_new {
        return Err(UpdateError::Limit(format!(
            "a file has more than {MAX_NEW_RECS} entities, more than a comparison reads"
        )));
    }
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
    let diff = revision::diff(&old, &new, &dopts);
    if diff.truncated {
        notes.push("a limit stopped the match: some changes may show as a removal and an addition. Give a smaller tolerance or limit the layers".into());
    }
    if diff.renumbered {
        notes.push("the handles of the files do not belong to the same entities (another program wrote one of them): the entities were matched by their content and their place".into());
    }

    // The new file as a document.
    let mut opts = req.options.clone();
    opts.track_source = true;
    opts.base = BaseMode::Off;
    opts.layer = None;
    opts.lineage = None;
    // A comparison is about the model. Paper space makes no object of the list.
    opts.paper_space = false;
    opts.source_name = Some(req.new.1.to_string());
    let empty = Document::with_sequential_ids();
    let imported = scratch(req.new.1, &new_dxf, &Context::of(&empty), &opts, "new")?;
    let skipped = imported.report.skipped_total();
    let mut editor = Editor::new(empty);
    editor
        .execute(&imported.command)
        .map_err(|e| UpdateError::NewFile(format!("the new file could not be read: {e}")))?;
    let new_doc = editor.doc().clone();
    let source = new_doc
        .layers
        .iter()
        .find_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?));
    let (scale, shift) = source.map_or((1.0, [0.0, 0.0]), |s| (s.unit_scale, s.shift));
    let unit = unit_of(scale).or(opts.unit);

    // The old file, placed like the new one, with IDs that follow those of the new document.
    let mut old_opts = opts.clone();
    old_opts.source_name = Some(req.base.1.to_string());
    old_opts.unit = unit;
    old_opts.layer_mode = opts.layer_mode;
    old_opts.lineage = Some(Lineage {
        drawing: Tracking::new(req.base.1, &old_dxf).drawing,
        revision: None,
        shift,
    });
    let old_unit = old_dxf
        .header_value("$INSUNITS")
        .and_then(parse::parse_int)
        .and_then(crate::Unit::from_insunits);
    if let (Some(a), Some(b)) = (old_unit, unit)
        && a != b
    {
        notes.push(format!(
            "the files are in different units ({a:?} and {b:?}): the unit of the new file is used for both"
        ));
    }
    let old_import = scratch(
        req.base.1,
        &old_dxf,
        &Context::of(&new_doc),
        &old_opts,
        "old",
    )?;
    let mut all = Editor::new(new_doc.clone());
    all.execute(&old_import.command)
        .map_err(|e| UpdateError::OldFile(format!("the old file could not be read: {e}")))?;
    let all_doc = all.doc().clone();
    let skipped = skipped + old_import.report.skipped_total();
    if skipped > 0 {
        notes.push(format!(
            "{skipped} entities are not imported (see report.skipped of import.file for the reasons); a change of such an entity is not in the list"
        ));
    }
    if opts.layer_mode == LayerMode::Layers {
        notes.push("each DXF layer is a layer of the temporary document".into());
    }

    // The tags of the two sides.
    let mut new_tags: Vec<(ObjectId, ImportTag)> = Vec::new();
    let mut old_tags: Vec<(ObjectId, ImportTag)> = Vec::new();
    for o in all_doc.objects.values() {
        let Some(t) = provenance::tag(&o.data) else {
            continue;
        };
        if new_doc.objects.contains_key(&o.id) {
            new_tags.push((o.id, t));
        } else {
            old_tags.push((o.id, t));
        }
    }
    let drawing_of = |tags: &[(ObjectId, ImportTag)]| {
        tags.first()
            .map(|(_, t)| t.drawing.clone())
            .unwrap_or_default()
    };
    let (old_drawing, new_drawing) = (drawing_of(&old_tags), drawing_of(&new_tags));
    let old_index = TagIndex::new(&old_drawing, old_tags.iter().map(|(i, t)| (*i, t)));
    let new_index = TagIndex::new(&new_drawing, new_tags.iter().map(|(i, t)| (*i, t)));
    let tag_of_old: HashMap<ObjectId, &ImportTag> = old_tags.iter().map(|(i, t)| (*i, t)).collect();
    let tag_of_new: HashMap<ObjectId, &ImportTag> = new_tags.iter().map(|(i, t)| (*i, t)).collect();
    let wanted: HashSet<String> = req.layers.iter().map(|l| l.to_lowercase()).collect();
    let on_layer = |r: &Rec| wanted.is_empty() || wanted.contains(&r.layer.to_lowercase());

    let centers = |_: bool, id: ObjectId| {
        all_doc
            .objects
            .get(&id)
            .and_then(|o| super::update::center_of(&o.shape))
    };
    let mut counts = Counts::default();
    let mut items = Vec::new();
    let mut changes = Vec::new();
    let mut seen: HashMap<String, u32> = HashMap::new();
    let mut taken: HashSet<ObjectId> = HashSet::new();
    for e in &diff.entries {
        let (b, n) = (e.base.map(|i| &old[i]), e.new.map(|i| &new[i]));
        let rec = n.or(b).expect("an entry has a side");
        if e.status == Status::Unchanged {
            if on_layer(rec) {
                counts.unchanged += 1;
            }
            continue;
        }
        if !(b.is_some_and(on_layer) || n.is_some_and(on_layer)) {
            continue;
        }
        let old_objs = b.map_or_else(Vec::new, |r| old_index.objects_of(r));
        let new_objs = n.map_or_else(Vec::new, |r| new_index.objects_of(r));
        let (status, ops) = classify(
            e.status,
            &old_objs,
            &new_objs,
            &tag_of_old,
            &tag_of_new,
            &centers,
        );
        if ops.replace.is_empty() && ops.delete.is_empty() && ops.add.is_empty() {
            counts.ignored += 1;
            continue;
        }
        match status {
            Kind::Added => counts.added += 1,
            Kind::Removed => counts.removed += 1,
            Kind::Changed => counts.changed += 1,
        }
        counts.objects_added += ops.add.len();
        counts.objects_replaced += ops.replace.len();
        counts.objects_removed += ops.delete.len();
        let id = change_id(e.status, b, n, &mut seen);
        let mut it = item(id, status, e, (b, n), &ops, &[]);
        // The objects to point at: the new side where there is one.
        it.object_ids = ops
            .replace
            .iter()
            .map(|(_, n)| *n)
            .chain(ops.add.iter().copied())
            .chain(ops.delete.iter().copied())
            .take(super::update::LISTED_OBJECTS)
            .collect();
        items.push(it);
        // The statuses by object. An object has one status, the first change counts.
        for (o, n) in &ops.replace {
            if taken.insert(*n) {
                changes.push(ObjectChange {
                    id: *n,
                    status: Kind::Changed,
                    old: Some(*o),
                });
            }
        }
        for n in &ops.add {
            if taken.insert(*n) {
                changes.push(ObjectChange {
                    id: *n,
                    status: Kind::Added,
                    old: None,
                });
            }
        }
        for o in &ops.delete {
            if taken.insert(*o) {
                changes.push(ObjectChange {
                    id: *o,
                    status: Kind::Removed,
                    old: None,
                });
            }
        }
    }
    if counts.ignored > 0 {
        notes.push(format!(
            "{} changed entities make no object (paper space, or entities that are not imported): they are not in the list",
            counts.ignored
        ));
    }
    Ok(Compared {
        items,
        counts,
        notes,
        truncated: diff.truncated,
        renumbered: diff.renumbered,
        tolerance: diff.tolerance,
        new_doc,
        all_doc,
        changes,
        name: req.new.1.to_string(),
        base_name: req.base.1.to_string(),
    })
}

/// A short account of a result, for answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub unchanged: usize,
}

impl Compared {
    pub fn summary(&self) -> Summary {
        Summary {
            added: self.counts.added,
            removed: self.counts.removed,
            changed: self.counts.changed,
            unchanged: self.counts.unchanged,
        }
    }
}

#[cfg(test)]
mod tests;
