//! `import.update` and `import.apply` (IOP-23): bring a new revision of an imported DXF or DWG
//! drawing into the document. Native only: they read files.
//!
//! The matching and the planning are in `kadakas_io::dxf::update`. This module joins them with the
//! session:
//!
//! - **`import.update {path, drawing?, old?, layers?, tolerance?, dry_run?, limit?, options?}`**
//!   reads the new file (DXF, or DWG through the converter), finds the drawing by its lineage,
//!   rebuilds the old side (the stored base, the linked original file checked by size and hash
//!   through the path policy, or the `old` file), and answers with the counts, a bounded list of
//!   changes with stable IDs, the conflicts and notes. Nothing changes in the document. The plan
//!   is kept in the session (one at a time, bounded by `SessionLimits::update_bytes`) until the
//!   document changes, a new `import.update` replaces it, or `import.apply` uses it.
//! - **`import.apply {update_id, accept, reject?, conflicts?}`** saves the version "Before update
//!   to <file>", applies the accepted changes as one undo step, and saves "After update to
//!   <file>" (`review_api::execute_between_versions`). A refused version "before" (a limit of
//!   versions) changes nothing.
//!
//! The app shows the pending update with the overlay of REV-03: [`Session::update_pending`] and
//! [`Session::update_preview`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kadakas_core::model::{Document, Object, ObjectId};
use kadakas_core::provenance::FileLink;
use kadakas_io::dwg::{self, RunOptions, Search};
use kadakas_io::dxf::link::{self, LinkError, ReadOptions};
use kadakas_io::dxf::revision::Rec;
use kadakas_io::dxf::update::{self, Built, MAX_NEW_RECS, Plan, Take, UpdateError};
use kadakas_io::{BaseCaps, BaseMode, Format, ImportOptions, Source};
use kadakas_review::overlay::{Change, CompareOverlay, Status};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{RpcError, Session, params, review_api};

pub(crate) const IMPORT_UPDATE: (&str, &str) = (
    "import.update",
    "Update the document from a new revision of an imported DXF or DWG drawing (IOP-23). Reads the file and compares its entities with the drawing in the document; changes nothing. params: {path, drawing? (the lineage; needed when several drawings were imported and the file does not say which), old? (the old revision, when the document has no update data and no usable link), layers? (only entities on these DXF layers), tolerance? (largest distance of a moved entity, in units of the drawing; default automatic), dry_run? (true: no pending update is kept), limit? (most changes listed, default 200, at most 1000), options? (import options of the new file such as hatches or base: auto|link|none)}. Returns {update_id (not with dry_run), drawing, name, from_revision, to_revision, old_from (base|link|file), counts, changes, listed, conflicts (the IDs of changes that concern an object the person edited), notes, match}. See describe.io.update. Then call import.apply.",
);
pub(crate) const IMPORT_APPLY: (&str, &str) = (
    "import.apply",
    "Apply the changes of the last import.update. params: {update_id, accept: \"all\" | [change ids], reject? [change ids], conflicts? (skip, the default, or overwrite: what accept \"all\" does with the changes that concern objects the person edited)}. Saves the version \"Before update to <file>\" (a limit of versions refuses it with the version_limit error and nothing changes), applies the accepted changes as ONE undo step (new objects with tags, changed objects replaced, removed objects deleted; objects of other sources are never touched), moves the layers and the update data to the new revision, and saves \"After update to <file>\". Fails with a conflict error if the document changed since import.update. Returns {applied: {added, replaced, removed, skipped_locked, kept_edits}, accepted, skipped_conflicts (the conflicts whose edited objects were kept), versions: {before, after}, drawing, revision, notes}.",
);

/// Most changes that an answer lists.
pub const MAX_LISTED: usize = 1000;
/// The default number of changes that an answer lists.
pub const DEFAULT_LISTED: usize = 200;
/// Most conflicts that an answer names by ID.
const MAX_CONFLICTS_LISTED: usize = 200;

/// What `describe` says about the update.
pub(crate) fn describe() -> Value {
    json!({
        "what": "A landscape designer imports the base drawing of an architect (import.file), and later updates it to the next revision (import.update, then import.apply). Each entity of the new file is matched with the objects that the old revision made: by DXF handle, and, for files whose handles changed (another program wrote them), by content, geometry and nearest position. A changed block definition changes every instance of the block.",
        "classes": "Each change is `added`, `removed` or `changed` (`what` names the changed properties: geometry, layer, color, linetype, lineweight, text, other; `distance` is how far it moved). Entities that did not change are counted. A change is a `conflict` when the person edited an object of it since the import (the object no longer equals its state at the import, provenance::is_edited) and the new revision changes or removes it as well.",
        "old_side": "The update data of the drawing in the document (the base of the last import), else the linked original file (import option base: link; read from the path on the layer with the path policy of the session, checked against the stored size and SHA-256), else the file given as `old`. If the file is gone or changed, or the drawing was imported with base: none, import.update fails with a conflict error that says so and asks for the old file.",
        "pending": "import.update keeps its plan in the session until the document changes (any edit, undo or load), another import.update replaces it, or import.apply uses it. dry_run keeps nothing. The plan is bounded (limit update_bytes of the session, 256 MiB by default); a file over it fails with a limit error: give `layers`.",
        "apply": "import.apply saves a version before, applies the accepted changes, and saves a version after, all in one undo step: undo takes back the update and both versions. If a limit of versions refuses the version before, the call fails with the version_limit error and nothing changes (pass it to the person, as for version.create). If only the version after is refused, the update is applied and `notes` says so. The changes you reject are not offered again in the next update: the layers and the update data move to the new revision either way.",
        "conflicts": "accept: \"all\" applies a conflict to the objects that the person did not edit and keeps the edited ones as they are (applied.kept_edits counts them, skipped_conflicts names the changes) unless conflicts: \"overwrite\" is given. A conflict that is named in the list of accept is applied to all its objects: it replaces the edit of the person.",
        "never_touched": "Only objects whose tag names this drawing are changed. Objects from other drawings, other importers and the objects the person drew are never touched. Objects on a locked layer that is not a layer of the drawing are left alone (applied.skipped_locked).",
        "base": "The new revision gets update data like the old one: summaries of the entities (auto), or a link to the new file when the drawing was imported with base: link. A new revision that is too big for summaries fails with the choices link and none (see import.file).",
        "limits": "The new file is read with the limits of import.file (max_bytes, import_text_bytes of the session, max_objects). At most 1,000,000 entities of the new file are matched, and the match has the bounds of kadakas_io::dxf::revision (work budget, truncated flag).",
        "examples": [
            {"method": "import.update", "params": {"path": "/data/site_rev2.dxf"}},
            {"method": "import.update", "params": {"path": "/data/site_rev2.dxf", "drawing": "site.dxf", "layers": ["WALLS", "TREES"], "dry_run": true}},
            {"method": "import.apply", "params": {"update_id": "u1", "accept": "all"}},
            {"method": "import.apply", "params": {"update_id": "u1", "accept": "all", "reject": ["c0123456789"]}}
        ]
    })
}

/// A plan that waits for `import.apply`.
struct Pending {
    id: String,
    /// The revision of the editor that the plan was made for.
    revision: u64,
    plan: Arc<Plan>,
    preview: Option<Arc<UpdatePreview>>,
}

/// The pending update and the counter of IDs, in a session.
#[derive(Default)]
pub struct UpdateState {
    pending: Option<Pending>,
    next: u64,
}

impl UpdateState {
    /// The memory that the pending update holds, about.
    pub fn approx_bytes(&self) -> u64 {
        self.pending.as_ref().map_or(0, |p| {
            p.plan.bytes()
                + p.preview
                    .as_ref()
                    .map_or(0, |v| crate::limits::document_bytes(&v.doc) * 2)
        })
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }
}

/// The pending update, for a window that shows it.
#[derive(Clone)]
pub struct PendingUpdate {
    pub id: String,
    pub plan: Arc<Plan>,
}

/// What the document would be after all changes, with the overlay of REV-03 that shows them.
pub struct UpdatePreview {
    /// The document after every change is applied (the conflicts too).
    pub doc: Arc<Document>,
    /// The document as it is now, for the ghosts of removed objects.
    pub old: Arc<Document>,
    pub overlay: Arc<CompareOverlay>,
    /// The objects of each change of the plan (same order) in `doc`: the new objects of an added
    /// change, the changed objects, and for a removed change the objects that are gone (they are
    /// ghosts in `old`).
    pub objects: Vec<Vec<ObjectId>>,
}

#[derive(Deserialize)]
struct UpdateParams {
    path: PathBuf,
    drawing: Option<String>,
    old: Option<PathBuf>,
    #[serde(default)]
    layers: Vec<String>,
    tolerance: Option<f64>,
    #[serde(default)]
    dry_run: bool,
    limit: Option<usize>,
    #[serde(default)]
    options: Value,
}

#[derive(Deserialize)]
struct ApplyParams {
    update_id: String,
    accept: Value,
    #[serde(default)]
    reject: Vec<String>,
    #[serde(default)]
    conflicts: Option<String>,
}

/// A drawing file that was read: its DXF bytes (for a DWG, the converted DXF), its name, and the
/// link to the file when one was asked for.
pub(crate) struct Loaded {
    pub data: Vec<u8>,
    pub name: String,
    pub link: Option<FileLink>,
}

pub(crate) fn update_error(e: UpdateError) -> RpcError {
    match e {
        UpdateError::NoDrawing => RpcError::not_found(e),
        UpdateError::Ambiguous(_) | UpdateError::Unknown(..) => RpcError::invalid_params(e),
        UpdateError::NeedsOldFile(_) => {
            let text = e.to_string();
            RpcError::conflict(text).with_details(json!({"needs_old_file": true}))
        }
        UpdateError::OldFile(_) | UpdateError::NewFile(_) => RpcError::validation(e),
        UpdateError::Limit(_) => RpcError::limit(e),
        UpdateError::Base(e) => crate::io::rpc(e),
    }
}

impl Session {
    /// Reads a DXF or DWG file for a comparison or an update. A DWG goes through the converter
    /// (only if the host allows it for a checked request). The size is limited like an import. With
    /// `want_link` the answer has a link to the file (its path, size and hash).
    pub(crate) fn read_drawing(
        &self,
        path: &Path,
        checked: bool,
        opts: &ImportOptions,
        run: &RunOptions,
        want_link: bool,
    ) -> Result<Loaded, RpcError> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if crate::dwg_api::is_dwg(None, path) {
            if checked && !self.paths.converters_allowed() {
                return Err(crate::dwg_api::converters_off());
            }
            let path = if checked {
                self.check_path(path, false, &["dwg"])
                    .map_err(RpcError::policy)?
            } else {
                path.to_path_buf()
            };
            let search = Search::system();
            let conv = dwg::detect(&search)
                .ok_or_else(|| crate::dwg_api::rpc(dwg::no_converter(&search)))?;
            let (data, link) = if want_link {
                let (d, l) = dwg::dwg_to_dxf_linked(&path, &conv, run, opts.max_bytes)
                    .map_err(crate::dwg_api::rpc)?;
                (d, Some(l))
            } else {
                let d = dwg::dwg_to_dxf(&path, &conv, run, opts.max_bytes)
                    .map_err(crate::dwg_api::rpc)?;
                (d, None)
            };
            return Ok(Loaded { data, name, link });
        }
        let path = if checked {
            self.check_path(path, false, &["dxf"])
                .map_err(RpcError::policy)?
        } else {
            path.to_path_buf()
        };
        let src = Source::read_limited(&path, opts.max_bytes).map_err(crate::io::rpc)?;
        let link = if want_link {
            Some(kadakas_io::file_link(&path, &src.data).map_err(crate::io::rpc)?)
        } else {
            None
        };
        Ok(Loaded {
            data: src.data,
            name,
            link,
        })
    }

    /// The import options of an update or a compare: the options of the request, with the limits
    /// of the session.
    pub(crate) fn drawing_options(
        &self,
        options: &Value,
    ) -> Result<(ImportOptions, RunOptions), RpcError> {
        let mut opts = ImportOptions::from_json(options).map_err(crate::io::rpc)?;
        opts.max_bytes = opts.max_bytes.min(self.limits.import_text_bytes);
        opts.base_caps = BaseCaps::with_records(self.limits.base_records);
        let run = RunOptions::from_json(options).map_err(crate::dwg_api::rpc)?;
        Ok((opts, run))
    }

    /// `import.update` for an agent: the path policy applies.
    pub(crate) fn import_update(&mut self, p: Value) -> Result<Value, RpcError> {
        self.update_impl(p, true)
    }

    /// `import.update` for the user's own file dialog: the path policy for agents does not apply.
    pub fn import_update_trusted(&mut self, p: Value) -> Result<Value, RpcError> {
        self.update_impl(p, false)
    }

    fn update_impl(&mut self, p: Value, checked: bool) -> Result<Value, RpcError> {
        let UpdateParams {
            path,
            drawing,
            old,
            layers,
            tolerance,
            dry_run,
            limit,
            options,
        } = params(p)?;
        let limit = limit.unwrap_or(DEFAULT_LISTED);
        if limit == 0 || limit > MAX_LISTED {
            return Err(RpcError::invalid_params(format!(
                "limit is a number from 1 to {MAX_LISTED}"
            )));
        }
        let (opts, run) = self.drawing_options(&options)?;
        let known = update::drawings(self.editor.doc());
        let want_link = opts.base == BaseMode::Link || known.iter().any(|d| d.linked);
        let new = self.read_drawing(&path, checked, &opts, &run, want_link)?;
        let old = old
            .map(|p| self.read_drawing(&p, checked, &opts, &run, false))
            .transpose()?;
        let max_bytes = self.limits.update_bytes;
        let read_link = |link: &FileLink| -> Result<Vec<Rec>, LinkError> {
            let policy = |p: &Path| -> Result<PathBuf, String> {
                if !checked {
                    return Ok(p.to_path_buf());
                }
                let ext = if is_dwg_text(&link.path) {
                    "dwg"
                } else {
                    "dxf"
                };
                self.check_path(p, false, &[ext])
            };
            let read = ReadOptions::new(opts.max_bytes, MAX_NEW_RECS);
            if is_dwg_text(&link.path) {
                if checked && !self.paths.converters_allowed() {
                    return Err(LinkError::Policy(
                        "the original file is a DWG file, and this host does not let agents start a converter".into(),
                    ));
                }
                dwg::read_linked_records(link, &policy, &read, &run, &Search::system())
            } else {
                link::read_records(link, &policy, &read)
            }
        };
        let plan = update::plan(&update::Request {
            doc: self.editor.doc(),
            drawing: drawing.as_deref(),
            name: &new.name,
            data: &new.data,
            new_link: new.link.clone(),
            old_file: old.as_ref().map(|o| (o.data.as_slice(), o.name.as_str())),
            read_link: &read_link,
            options: &opts,
            tolerance,
            layers: &layers,
            max_bytes,
        })
        .map_err(update_error)?;
        let plan = Arc::new(plan);
        let mut out = json!({
            "drawing": plan.drawing,
            "name": plan.name,
            "from_revision": plan.old_revision,
            "to_revision": plan.new_revision,
            "old_from": plan.old_from,
            "counts": plan.counts,
            "changes": plan.changes.iter().take(limit).collect::<Vec<_>>(),
            "listed": plan.changes.len().min(limit),
            "total_changes": plan.changes.len(),
            "conflicts": plan.changes.iter().filter(|c| c.conflict).take(MAX_CONFLICTS_LISTED).map(|c| &c.id).collect::<Vec<_>>(),
            "notes": plan.notes,
            "match": {
                "truncated": plan.truncated,
                "renumbered": plan.renumbered,
                "tolerance": plan.tolerance,
            },
            "dry_run": dry_run,
        });
        if plan.changes.len() > limit {
            out["notes"] = json!(
                plan.notes
                    .iter()
                    .cloned()
                    .chain([format!(
                        "{} of {} changes are listed: raise limit, or limit the update with layers",
                        limit,
                        plan.changes.len()
                    )])
                    .collect::<Vec<_>>()
            );
        }
        if !dry_run {
            self.updates.next += 1;
            let id = format!("u{}", self.updates.next);
            out["update_id"] = json!(id);
            self.updates.pending = Some(Pending {
                id,
                revision: self.editor.revision(),
                plan,
                preview: None,
            });
        }
        Ok(out)
    }

    /// The pending update, if the document is as it was when the update was made. A pending update
    /// of an older state is dropped.
    pub fn update_pending(&mut self) -> Option<PendingUpdate> {
        let current = self.editor.revision();
        if self
            .updates
            .pending
            .as_ref()
            .is_some_and(|p| p.revision != current)
        {
            self.updates.pending = None;
        }
        self.updates.pending.as_ref().map(|p| PendingUpdate {
            id: p.id.clone(),
            plan: p.plan.clone(),
        })
    }

    /// Drops the pending update.
    pub fn update_discard(&mut self) {
        self.updates.pending = None;
    }

    /// The memory of the pending update, for `estimated_bytes`.
    pub(crate) fn update_bytes_held(&self) -> u64 {
        self.updates.approx_bytes()
    }

    /// Frees the preview of the pending update (the plan stays, the preview is built again).
    pub(crate) fn drop_update_preview(&mut self) {
        if let Some(p) = self.updates.pending.as_mut() {
            p.preview = None;
        }
    }

    /// What the pending update would make, with the overlay. Built once for a plan. `None`: no
    /// pending update.
    pub fn update_preview(&mut self) -> Result<Option<Arc<UpdatePreview>>, RpcError> {
        let Some(pending) = self.update_pending() else {
            return Ok(None);
        };
        if let Some(v) = self
            .updates
            .pending
            .as_ref()
            .and_then(|p| p.preview.clone())
        {
            return Ok(Some(v));
        }
        let plan = pending.plan;
        let chosen = vec![Take::All; plan.changes.len()];
        let built = plan
            .build(self.editor.doc(), &chosen)
            .map_err(update_error)?;
        let mut probe = kadakas_core::command::Editor::new(self.editor.doc().clone());
        probe.execute(&built.command).map_err(RpcError::from)?;
        let old = Arc::new(self.editor.doc().clone());
        let (changes, objects) = preview_changes(&plan, &old, &built);
        let preview = Arc::new(UpdatePreview {
            doc: Arc::new(probe.doc().clone()),
            old,
            overlay: Arc::new(CompareOverlay::from_changes(changes)),
            objects,
        });
        if let Some(p) = self.updates.pending.as_mut() {
            p.preview = Some(preview.clone());
        }
        Ok(Some(preview))
    }

    pub(crate) fn import_apply(&mut self, p: Value) -> Result<Value, RpcError> {
        let ApplyParams {
            update_id,
            accept,
            reject,
            conflicts,
        } = params(p)?;
        let Some(pending) = self.update_pending() else {
            return Err(RpcError::conflict(
                "no update is pending: it expired when the document changed, or import.update was not called. Call import.update again",
            ));
        };
        if pending.id != update_id {
            return Err(RpcError::not_found(format!(
                "the pending update is {}, not {update_id}",
                pending.id
            )));
        }
        let overwrite = match conflicts.as_deref() {
            None | Some("skip") => false,
            Some("overwrite") => true,
            Some(_) => {
                return Err(RpcError::invalid_params(
                    "conflicts is \"skip\" or \"overwrite\"",
                ));
            }
        };
        let plan = pending.plan;
        let index = |id: &str| {
            plan.index_of(id)
                .ok_or_else(|| RpcError::invalid_params(format!("no change has the ID {id}")))
        };
        let mut chosen = vec![Take::No; plan.changes.len()];
        match &accept {
            Value::String(t) if t == "all" => {
                for (i, c) in plan.changes.iter().enumerate() {
                    // A conflict keeps the objects that the person edited, unless overwrite.
                    chosen[i] = if c.conflict && !overwrite {
                        Take::KeepEdits
                    } else {
                        Take::All
                    };
                }
            }
            Value::Array(list) => {
                for v in list {
                    let id = v.as_str().ok_or_else(|| {
                        RpcError::invalid_params("accept lists change IDs (text)")
                    })?;
                    chosen[index(id)?] = Take::All;
                }
            }
            _ => {
                return Err(RpcError::invalid_params(
                    "give accept: \"all\" or a list of change IDs",
                ));
            }
        }
        for id in &reject {
            chosen[index(id)?] = Take::No;
        }
        let built = plan
            .build(self.editor.doc(), &chosen)
            .map_err(update_error)?;
        let name = plan.name.clone();
        let between = review_api::execute_between_versions(
            self,
            &built.command,
            (
                &format!("Before update to {name}"),
                &format!("After update to {name}"),
            ),
            "import_update",
            json!({"drawing": plan.drawing, "from": plan.old_revision, "to": plan.new_revision}),
        )?;
        self.updates.pending = None;
        self.prune_selection();
        let skipped: Vec<&String> = plan
            .changes
            .iter()
            .zip(&chosen)
            .filter(|(c, take)| c.conflict && **take != Take::All)
            .map(|(c, _)| &c.id)
            .take(MAX_CONFLICTS_LISTED)
            .collect();
        let mut notes = built.notes.clone();
        notes.extend(between.notes.iter().cloned());
        Ok(json!({
            "applied": built.applied,
            "accepted": chosen.iter().filter(|c| **c != Take::No).count(),
            "skipped_conflicts": skipped,
            "versions": {"before": between.before, "after": between.after},
            "drawing": plan.drawing,
            "revision": plan.new_revision,
            "notes": notes,
        }))
    }
}

fn is_dwg_text(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".dwg")
}

/// The overlay changes of a plan whose changes are all applied (`built`), and the objects of each
/// change in the document after the update.
fn preview_changes(
    plan: &Plan,
    old: &Document,
    built: &Built,
) -> (Vec<Change>, Vec<Vec<ObjectId>>) {
    let real: HashMap<ObjectId, ObjectId> = built.added.iter().copied().collect();
    let object = |id: &ObjectId| -> Option<Object> { old.objects.get(id).cloned() };
    let mut changes = Vec::new();
    let mut objects = Vec::with_capacity(plan.changes.len());
    for i in 0..plan.changes.len() {
        let mut mine = Vec::new();
        if let Some(ops) = plan.ops(i) {
            for n in &ops.add {
                if let Some(id) = real.get(n) {
                    changes.push(Change {
                        id: *id,
                        status: Status::Added,
                        old: None,
                        new: None,
                    });
                    mine.push(*id);
                }
            }
            for (d, _) in &ops.replace {
                changes.push(Change {
                    id: *d,
                    status: Status::Changed,
                    old: object(d),
                    new: None,
                });
                mine.push(*d);
            }
            for d in &ops.delete {
                changes.push(Change {
                    id: *d,
                    status: Status::Removed,
                    old: object(d),
                    new: None,
                });
                mine.push(*d);
            }
        }
        objects.push(mine);
    }
    (changes, objects)
}

/// Dispatch of the methods of this module.
pub(crate) fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    Some(match method {
        "import.update" => s.import_update(p),
        "import.apply" => s.import_apply(p),
        "dxf.compare" => s.dxf_compare(p),
        _ => return None,
    })
}

/// True if the path names a DXF or DWG file by its extension.
pub fn is_drawing_path(path: &Path) -> bool {
    Format::from_path(path) == Some(Format::Dxf) || crate::dwg_api::is_dwg(None, path)
}
