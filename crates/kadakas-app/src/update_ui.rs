//! File > Import > Update from DXF/DWG... (IOP-23).
//!
//! The person picks the new revision of an imported drawing. The window shows what changed with
//! the overlay of REV-03 on the plan (added objects green, changed objects orange, removed objects
//! as ghosts, each with a line pattern and a mark as well as the color) and a list of the changes
//! with the conflicts marked: a conflict is a change of an object that the person edited. The
//! person accepts all changes or each one, then presses Apply. Apply is `import.apply`: a version
//! before, the changes as one undo step, a version after.
//!
//! The window only draws and asks. The matching, the plan and the apply are in `kadakas-api`
//! (`Session::import_update_trusted`, `update_pending`, `update_preview`, and the method
//! `import.apply`), so the app and an agent use one code path.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Color32, Ui};
use kadakas_api::{ChangeItem, ChangeKind, PendingUpdate, Session, UpdatePreview};
use kadakas_core::model::{Document, Object, ObjectId};
use kadakas_l10n::tr;
use kadakas_review::overlay::{
    CompareStyle, GhostReport, Palette, Status, UnchangedMode, ghosts_in,
};
use kadakas_ui_core::compare::frame_box;
use serde_json::json;

use crate::compare_draw::CompareDraw;
use crate::compare_ui::{ZoomRequest, swatch};

/// The most rows that the list draws. The counts above it are exact.
const LIST_ROWS: usize = 1000;

/// What the person must do to go on after a refusal.
#[derive(Clone, Debug, PartialEq)]
enum Problem {
    /// The old side is not available: choose the old file.
    NeedsOldFile(String),
    /// Several drawings were imported: choose which one this is a revision of.
    WhichDrawing(Vec<String>),
    /// The file says it is another drawing than the only one in the document: the person decides.
    ConfirmDrawing { file_says: String, drawing: String },
    /// The update data of the new revision does not fit: keep a link, or none (as the import
    /// dialog asks). The text is the question.
    BaseChoice(String),
    /// Anything else.
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Filter {
    added: bool,
    removed: bool,
    changed: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            added: true,
            removed: true,
            changed: true,
        }
    }
}

/// State of the window.
#[derive(Default)]
pub struct UpdateUi {
    open: bool,
    /// The new revision.
    path: Option<PathBuf>,
    /// The old revision, when the person had to give it.
    old: Option<PathBuf>,
    /// The drawing that the new revision is for, when the person had to choose.
    drawing: Option<String>,
    /// The update data of the new revision, when the person had to choose: `link` or `none`.
    base: Option<&'static str>,
    /// The changes of the last plan (ID, and whether it was a conflict), to keep the choices of the
    /// person when the plan is made again.
    last_plan: std::collections::HashMap<String, bool>,
    problem: Option<Problem>,
    plan: Option<PendingUpdate>,
    /// The IDs of the changes to apply.
    accepted: HashSet<String>,
    preview: Option<Arc<UpdatePreview>>,
    ghosts: Vec<(Object, &'static str)>,
    report: GhostReport,
    style: CompareStyle,
    show_on_drawing: bool,
    filter: Filter,
    focus: Option<ObjectId>,
    zoom: Option<ZoomRequest>,
    /// The result of the last apply, shown until the window is closed.
    done: Option<String>,
    /// The plan is old: the document changed after it was made.
    expired: bool,
    /// A message above the plan: the last apply was refused.
    note: Option<String>,
    style_dirty: bool,
}

fn status_of(kind: ChangeKind) -> Status {
    match kind {
        ChangeKind::Added => Status::Added,
        ChangeKind::Removed => Status::Removed,
        ChangeKind::Changed => Status::Changed,
    }
}

fn symbol(kind: ChangeKind) -> char {
    status_of(kind).symbol()
}

/// The words for what differs in a changed entity.
fn what_text(item: &ChangeItem) -> String {
    use kadakas_api::EntityChange as Prop;
    item.what
        .iter()
        .map(|p| match p {
            Prop::Geometry => tr!("update-ui-what-geometry"),
            Prop::Layer => tr!("update-ui-what-layer"),
            Prop::Color => tr!("update-ui-what-color"),
            Prop::Linetype => tr!("update-ui-what-linetype"),
            Prop::Lineweight => tr!("update-ui-what-lineweight"),
            Prop::Text => tr!("update-ui-what-text"),
            Prop::Other => tr!("update-ui-what-other"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl UpdateUi {
    pub fn new() -> UpdateUi {
        UpdateUi {
            show_on_drawing: true,
            ..UpdateUi::default()
        }
    }

    /// File > Import > Update from DXF/DWG...: asks for the new revision, then makes the update.
    pub fn start(&mut self, session: &mut Session) {
        let picked = rfd::FileDialog::new()
            .add_filter(tr!("update-ui-filter"), &["dxf", "dwg"])
            .add_filter("DXF", &["dxf"])
            .add_filter(tr!("io-dialog-dwg-needs-a-converter"), &["dwg"])
            .pick_file();
        if let Some(path) = picked {
            self.begin(session, path);
        }
    }

    /// Starts an update from the file `path`.
    fn begin(&mut self, session: &mut Session, path: PathBuf) {
        session.update_discard();
        self.path = Some(path);
        self.old = None;
        self.drawing = None;
        self.base = None;
        self.last_plan.clear();
        self.accepted.clear();
        self.done = None;
        self.open = true;
        self.run(session);
    }

    /// Reads the new revision and makes the plan. The ticks of the person stay for the changes
    /// that were in the last plan (the IDs are stable); a change that is a conflict now and was not
    /// one before is unticked, and its ID is in the answer.
    fn run(&mut self, session: &mut Session) -> Vec<String> {
        self.plan = None;
        self.preview = None;
        self.ghosts.clear();
        self.problem = None;
        self.expired = false;
        self.note = None;
        self.focus = None;
        let Some(path) = self.path.clone() else {
            return Vec::new();
        };
        let mut params = json!({"path": path, "limit": 1});
        if let Some(b) = self.base {
            params["options"] = json!({"base": b});
        }
        if let Some(d) = &self.drawing {
            params["drawing"] = json!(d);
        }
        if let Some(o) = &self.old {
            params["old"] = json!(o);
        }
        match session.import_update_trusted(params) {
            Ok(_) => {
                self.plan = session.update_pending();
                let mut newly: Vec<String> = Vec::new();
                let mut accepted = HashSet::new();
                for c in self.plan.iter().flat_map(|p| &p.plan.changes) {
                    let on = match self.last_plan.get(&c.id) {
                        // Known: the choice of the person stays, unless it is a conflict now.
                        Some(was_conflict) => {
                            let on = self.accepted.contains(&c.id);
                            if c.conflict && !was_conflict {
                                if on {
                                    newly.push(c.id.clone());
                                }
                                false
                            } else {
                                on
                            }
                        }
                        // New: every change that does not concern an edit of the person.
                        None => !c.conflict,
                    };
                    if on {
                        accepted.insert(c.id.clone());
                    }
                }
                self.accepted = accepted;
                self.last_plan = self
                    .plan
                    .iter()
                    .flat_map(|p| &p.plan.changes)
                    .map(|c| (c.id.clone(), c.conflict))
                    .collect();
                if !newly.is_empty() {
                    self.note = Some(tr!("update-ui-new-conflicts", count = newly.len()));
                }
                self.build_preview(session);
                return newly;
            }
            Err(e) => {
                self.problem = Some(
                    if e.details
                        .as_ref()
                        .is_some_and(|d| d["needs_old_file"] == true)
                    {
                        Problem::NeedsOldFile(e.message)
                    } else if let Some((entities, cap, bytes)) =
                        crate::io_dialog::base_choice_of(&e)
                    {
                        Problem::BaseChoice(crate::io_dialog::base_question_text(
                            entities, cap, bytes,
                        ))
                    } else if let Some(d) = e
                        .details
                        .as_ref()
                        .filter(|d| d["ask_the_person"] == true && d["file_says"].is_string())
                    {
                        Problem::ConfirmDrawing {
                            file_says: d["file_says"].as_str().unwrap_or_default().to_string(),
                            drawing: d["drawing"].as_str().unwrap_or_default().to_string(),
                        }
                    } else if e.kind == "invalid_params" && e.message.contains("drawing") {
                        Problem::WhichDrawing(
                            session
                                .imported_drawings()
                                .into_iter()
                                .map(|d| d.drawing)
                                .collect(),
                        )
                    } else {
                        Problem::Failed(e.message)
                    },
                );
            }
        }
        Vec::new()
    }

    fn build_preview(&mut self, session: &mut Session) {
        match session.update_preview() {
            Ok(p) => {
                self.preview = p;
                self.restyle();
            }
            Err(e) => {
                self.preview = None;
                self.problem = Some(Problem::Failed(e.message));
            }
        }
    }

    /// Makes the ghosts again for the style.
    fn restyle(&mut self) {
        self.style_dirty = false;
        let Some(p) = &self.preview else { return };
        let found = ghosts_in(&p.doc, &p.old, &p.overlay, &self.style);
        self.ghosts = found.items;
        self.report = found.report;
    }

    /// Forgets everything, for a new document.
    pub fn reset(&mut self, session: &mut Session) {
        session.update_discard();
        *self = UpdateUi::new();
    }

    /// True while the plan shows the document as it would be after the update: read only.
    pub fn read_only(&self, plan: bool) -> bool {
        plan && self.history_doc().is_some()
    }

    /// The document that the plan shows instead of the open drawing: the drawing after all
    /// changes, read only, while the window is open and the person wants to see the overlay.
    pub fn history_doc(&self) -> Option<Arc<Document>> {
        self.preview
            .as_ref()
            .filter(|_| self.open && self.show_on_drawing && self.plan.is_some())
            .map(|p| p.doc.clone())
    }

    /// The overlay for the canvas.
    pub fn draw(&self) -> Option<CompareDraw<'_>> {
        let p = self
            .preview
            .as_ref()
            .filter(|_| self.open && self.show_on_drawing && self.plan.is_some())?;
        Some(CompareDraw {
            overlay: &p.overlay,
            style: &self.style,
            ghosts: &self.ghosts,
            ghost_doc: Some(&p.old),
            report: self.report,
            focus: self.focus,
        })
    }

    /// Sets the colors for the theme of the canvas.
    pub fn set_dark(&mut self, dark: bool) {
        if self.style.dark != dark {
            self.style.dark = dark;
            self.style_dirty = true;
        }
    }

    /// The part of the model to show, after a click in the list.
    pub fn take_zoom(&mut self) -> Option<ZoomRequest> {
        self.zoom.take()
    }

    /// Closes the window and drops the pending update.
    fn close(&mut self, session: &mut Session) {
        self.open = false;
        self.focus = None;
        self.preview = None;
        self.plan = None;
        self.ghosts.clear();
        session.update_discard();
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        if !self.open {
            return;
        }
        // The document changed after the plan was made: it is out of date.
        if self.plan.is_some() && session.update_pending().is_none() {
            self.plan = None;
            self.preview = None;
            self.ghosts.clear();
            self.expired = true;
        }
        if self.style_dirty {
            self.restyle();
        }
        let mut open = self.open;
        egui::Window::new(tr!("update-ui-title"))
            .id(egui::Id::new("win-update-from-dxf"))
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(ctx.content_rect().height() * 0.8)
                    .show(ui, |ui| self.body(ui, session, status));
            });
        if !open {
            self.close(session);
        }
    }

    fn body(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        ui.small(tr!("update-ui-intro"));
        if let Some(p) = &self.path {
            let name = p
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            ui.label(tr!("update-ui-file", name = name));
        }
        if let Some(done) = self.done.clone() {
            ui.separator();
            ui.label(done);
            if ui.button(tr!("update-ui-close")).clicked() {
                self.close(session);
            }
            return;
        }
        if self.expired {
            ui.colored_label(Color32::from_rgb(200, 120, 0), tr!("update-ui-expired"));
            if ui.button(tr!("update-ui-update-again")).clicked() {
                self.run(session);
            }
            return;
        }
        if let Some(problem) = self.problem.clone() {
            self.problem_ui(ui, session, &problem);
            return;
        }
        if self.plan.is_none() {
            return;
        }
        self.plan_ui(ui, session, status);
    }

    fn problem_ui(&mut self, ui: &mut Ui, session: &mut Session, problem: &Problem) {
        let red = Color32::from_rgb(200, 60, 40);
        match problem {
            Problem::NeedsOldFile(text) => {
                ui.colored_label(red, text);
                if ui.button(tr!("update-ui-pick-old")).clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter(tr!("update-ui-filter"), &["dxf", "dwg"])
                        .pick_file()
                {
                    self.old = Some(p);
                    self.run(session);
                }
            }
            Problem::WhichDrawing(names) => {
                ui.label(tr!("update-ui-pick-drawing"));
                for n in names {
                    if ui.button(n).clicked() {
                        self.drawing = Some(n.clone());
                        self.run(session);
                    }
                }
            }
            Problem::ConfirmDrawing { file_says, drawing } => {
                ui.label(tr!(
                    "update-ui-confirm-drawing",
                    new = file_says.clone(),
                    drawing = drawing.clone()
                ));
                ui.horizontal(|ui| {
                    if ui.button(tr!("update-ui-confirm-yes")).clicked() {
                        self.drawing = Some(drawing.clone());
                        self.run(session);
                    }
                    if ui.button(tr!("update-ui-cancel")).clicked() {
                        self.close(session);
                    }
                });
            }
            Problem::BaseChoice(question) => {
                ui.label(tr!("io-dialog-base-question-title"));
                ui.label(question);
                ui.horizontal(|ui| {
                    if ui.button(tr!("io-dialog-base-keep-link")).clicked() {
                        self.base = Some("link");
                        self.run(session);
                    }
                    if ui.button(tr!("io-dialog-base-no-update-data")).clicked() {
                        self.base = Some("none");
                        self.run(session);
                    }
                    if ui.button(tr!("update-ui-cancel")).clicked() {
                        self.close(session);
                    }
                });
            }
            Problem::Failed(text) => {
                ui.colored_label(red, tr!("update-ui-failed", message = text.clone()));
            }
        }
    }

    fn plan_ui(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        let Some(pending) = self.plan.clone() else {
            return;
        };
        let plan = &pending.plan;
        let c = &plan.counts;
        if let Some(note) = &self.note {
            ui.colored_label(Color32::from_rgb(200, 60, 40), note);
        }
        ui.label(tr!("update-ui-drawing", drawing = plan.drawing.clone()));
        ui.small(match plan.old_from {
            "link" => tr!("update-ui-old-link"),
            "file" => tr!("update-ui-old-file"),
            _ => tr!("update-ui-old-base"),
        });
        if plan.changes.is_empty() {
            ui.label(tr!("update-ui-no-changes"));
        } else {
            ui.horizontal_wrapped(|ui| {
                for (st, n) in [
                    (Status::Added, c.added),
                    (Status::Removed, c.removed),
                    (Status::Changed, c.changed),
                ] {
                    let text = match st {
                        Status::Added => tr!("compare-ui-legend-added", count = n),
                        Status::Removed => tr!("compare-ui-legend-removed", count = n),
                        Status::Changed => tr!("compare-ui-legend-changed", count = n),
                    };
                    swatch(ui, &self.style, st);
                    ui.label(text);
                }
                ui.label(tr!("update-ui-unchanged", count = c.unchanged));
            });
        }
        let amber = Color32::from_rgb(200, 120, 0);
        if plan.truncated {
            ui.colored_label(amber, tr!("update-ui-truncated"));
        }
        for n in &plan.notes {
            ui.small(n);
        }
        if c.conflicts > 0 {
            ui.colored_label(amber, tr!("update-ui-conflicts", count = c.conflicts));
        }
        ui.separator();
        self.display_options(ui);
        ui.separator();
        // Accept and reject.
        ui.horizontal(|ui| {
            if ui.button(tr!("update-ui-accept-all")).clicked() {
                self.accepted = plan
                    .changes
                    .iter()
                    .filter(|c| !c.conflict)
                    .map(|c| c.id.clone())
                    .collect();
            }
            if ui.button(tr!("update-ui-reject-all")).clicked() {
                self.accepted.clear();
            }
            ui.checkbox(&mut self.filter.added, tr!("compare-ui-filter-added"));
            ui.checkbox(&mut self.filter.removed, tr!("compare-ui-filter-removed"));
            ui.checkbox(&mut self.filter.changed, tr!("compare-ui-filter-changed"));
        });
        ui.small(tr!("update-ui-click"));
        let mut clicked: Option<usize> = None;
        let mut shown = 0usize;
        egui::ScrollArea::vertical()
            .id_salt("update-rows")
            .max_height(260.0)
            .show(ui, |ui| {
                for (i, item) in plan.changes.iter().enumerate() {
                    let wanted = match item.status {
                        ChangeKind::Added => self.filter.added,
                        ChangeKind::Removed => self.filter.removed,
                        ChangeKind::Changed => self.filter.changed,
                    };
                    if !wanted {
                        continue;
                    }
                    shown += 1;
                    if shown > LIST_ROWS {
                        break;
                    }
                    ui.horizontal(|ui| {
                        let mut on = self.accepted.contains(&item.id);
                        if ui.checkbox(&mut on, "").changed() {
                            if on {
                                self.accepted.insert(item.id.clone());
                            } else {
                                self.accepted.remove(&item.id);
                            }
                        }
                        swatch(ui, &self.style, status_of(item.status));
                        let mut text =
                            format!("{} {} {}", symbol(item.status), item.entity, item.layer);
                        if !item.what.is_empty() {
                            text = format!("{text} ({})", what_text(item));
                        }
                        if let Some(t) = &item.text {
                            text = format!("{text}: {} > {}", t.old, t.new);
                        }
                        let focused = item
                            .object_ids
                            .first()
                            .is_some_and(|o| self.focus == Some(*o));
                        if ui.selectable_label(focused, text).clicked() {
                            clicked = Some(i);
                        }
                        if item.conflict {
                            ui.colored_label(amber, tr!("update-ui-conflict-mark"))
                                .on_hover_text(tr!("update-ui-conflict-hint"));
                        }
                        if item.objects + item.new_objects > 1 {
                            ui.small(tr!(
                                "update-ui-row-objects",
                                count = item.objects.max(item.new_objects)
                            ));
                        }
                    });
                }
            });
        if shown > LIST_ROWS {
            ui.small(tr!("update-ui-more"));
        }
        if let Some(i) = clicked {
            self.click(i);
        }
        ui.separator();
        let n = self.accepted.len();
        ui.horizontal(|ui| {
            if ui.button(tr!("update-ui-apply", count = n)).clicked() {
                self.apply(session, status);
            }
            if ui.button(tr!("update-ui-cancel")).clicked() {
                self.close(session);
            }
        });
    }

    fn display_options(&mut self, ui: &mut Ui) {
        let before = (self.style, self.show_on_drawing);
        ui.checkbox(&mut self.show_on_drawing, tr!("compare-ui-show-on-drawing"));
        ui.horizontal(|ui| {
            ui.label(tr!("compare-ui-unchanged"));
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Show,
                tr!("compare-ui-unchanged-show"),
            );
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Dim,
                tr!("compare-ui-unchanged-dim"),
            );
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Hide,
                tr!("compare-ui-unchanged-hide"),
            );
        });
        ui.horizontal(|ui| {
            ui.label(tr!("compare-ui-palette"));
            ui.radio_value(
                &mut self.style.palette,
                Palette::Standard,
                tr!("compare-ui-palette-standard"),
            );
            ui.radio_value(
                &mut self.style.palette,
                Palette::ColorBlind,
                tr!("compare-ui-palette-blind"),
            );
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.style.show_ghosts, tr!("compare-ui-ghosts"));
            ui.checkbox(&mut self.style.show_previous, tr!("compare-ui-previous"));
        });
        if (self.style, self.show_on_drawing) != before {
            self.style_dirty = true;
        }
        if self.show_on_drawing {
            ui.small(tr!("update-ui-read-only"));
        }
    }

    /// A click on a row: zoom to the objects of the change, and frame them.
    fn click(&mut self, index: usize) {
        let (Some(p), Some(plan)) = (&self.preview, &self.plan) else {
            return;
        };
        let item = &plan.plan.changes[index];
        let Some(ids) = p.objects.get(index) else {
            return;
        };
        let removed = item.status == ChangeKind::Removed;
        let mut lo = glam::DVec2::splat(f64::INFINITY);
        let mut hi = glam::DVec2::splat(f64::NEG_INFINITY);
        for id in ids {
            let doc: &Document = if removed { &p.old } else { &p.doc };
            if let Some(o) = doc.object(*id) {
                let (a, b) = doc.object_bounds(o);
                if a.is_finite() && b.is_finite() {
                    lo = lo.min(a);
                    hi = hi.max(b);
                }
            }
        }
        if !lo.is_finite() || !hi.is_finite() {
            return;
        }
        self.focus = ids.first().copied();
        if let Some((lo, hi)) = frame_box(lo, hi) {
            self.zoom = Some(ZoomRequest { id: ids[0], lo, hi });
        }
    }

    /// Apply: `import.apply` with the changes that the person accepted. If the plan is out of date
    /// (for example after the person deleted versions to make room), it is made again with the
    /// same choice, which the stable IDs of the changes allow.
    fn apply(&mut self, session: &mut Session, status: &mut String) {
        if session.update_pending().is_none() {
            let newly = self.run(session);
            if self.plan.is_none() {
                return;
            }
            // A change that concerns an edit now was ticked: the person looks at it first.
            if !newly.is_empty() {
                return;
            }
        }
        let Some(pending) = session.update_pending() else {
            return;
        };
        let mut ids: Vec<&String> = self.accepted.iter().collect();
        ids.sort();
        match session.handle(
            "import.apply",
            json!({"update_id": pending.id, "accept": ids}),
        ) {
            Ok(r) => {
                let a = &r["applied"];
                let text = tr!(
                    "update-ui-done",
                    added = a["added"].as_u64().unwrap_or(0),
                    replaced = a["replaced"].as_u64().unwrap_or(0),
                    removed = a["removed"].as_u64().unwrap_or(0)
                );
                *status = text.clone();
                self.done = Some(text);
                self.plan = None;
                self.preview = None;
                self.ghosts.clear();
            }
            Err(e) if e.reason == Some(kadakas_api::REASON_VERSION_LIMIT) => {
                self.note = Some(tr!("update-ui-version-limit"));
                *status = tr!("update-ui-version-limit");
            }
            Err(e) => {
                let text = tr!("update-ui-failed", message = e.message);
                self.note = Some(text.clone());
                *status = text;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Command;

    const GUID: &str = "A1B2C3D4-0001";

    fn file(version: &str, ents: &str) -> String {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$FINGERPRINTGUID\n2\n{GUID}\n9\n$VERSIONGUID\n2\n{version}\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
        )
    }

    fn line(handle: u32, layer: &str, y: i64) -> String {
        format!("0\nLINE\n5\n{handle:X}\n8\n{layer}\n10\n0\n20\n{y}\n11\n1000\n21\n{y}\n")
    }

    fn v1() -> String {
        file(
            "AAAA0001",
            &[
                line(0x10, "A", 0),
                line(0x11, "A", 100),
                line(0x12, "A", 200),
                line(0x13, "A", 300),
            ]
            .concat(),
        )
    }

    fn v2() -> String {
        file(
            "AAAA0002",
            &[
                line(0x10, "A", 0),
                line(0x11, "A", 150),
                line(0x13, "B", 300),
                line(0x14, "A", 400),
            ]
            .concat(),
        )
    }

    struct Fixture {
        _dir: kadakas_sys::tmp::PrivateTempDir,
        session: Session,
        new: PathBuf,
    }

    fn fixture(options: serde_json::Value) -> Fixture {
        let dir = kadakas_sys::tmp::PrivateTempDir::new("kadakas-app-update").unwrap();
        let first = dir.path().join("site.dxf");
        std::fs::write(&first, v1()).unwrap();
        let new = dir.path().join("site_v2.dxf");
        std::fs::write(&new, v2()).unwrap();
        let mut session = Session::with_sequential_ids();
        session
            .import_file_trusted(json!({"path": first, "options": options}))
            .unwrap();
        Fixture {
            _dir: dir,
            session,
            new,
        }
    }

    fn frame(ui: &mut UpdateUi, session: &mut Session, status: &mut String) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |u| {
            ui.show(u.ctx(), session, status);
        });
    }

    #[test]
    fn the_window_makes_the_plan_shows_the_overlay_and_applies_the_choice() {
        let mut f = fixture(json!({}));
        let mut ui = UpdateUi::new();
        let mut status = String::new();
        ui.begin(&mut f.session, f.new.clone());
        assert!(ui.problem.is_none());
        let plan = ui.plan.clone().unwrap();
        assert_eq!(plan.plan.changes.len(), 4);
        // Everything but the conflicts is ticked by default.
        assert_eq!(ui.accepted.len(), 4);
        // The plan shows the drawing as it would be, read only, with the overlay.
        assert!(ui.preview.is_some() && ui.history_doc().is_some() && ui.draw().is_some());
        assert!(ui.read_only(true) && !ui.read_only(false));
        frame(&mut ui, &mut f.session, &mut status);
        assert!(ui.open);
        // A click on a row asks for a zoom.
        ui.click(0);
        assert!(ui.take_zoom().is_some() && ui.take_zoom().is_none());
        let removed = plan
            .plan
            .changes
            .iter()
            .position(|c| c.status == ChangeKind::Removed)
            .unwrap();
        ui.click(removed);
        assert!(ui.focus.is_some());
        // Reject the removal, then apply.
        let id = plan.plan.changes[removed].id.clone();
        ui.accepted.remove(&id);
        let before = f.session.editor.doc().objects.len();
        ui.apply(&mut f.session, &mut status);
        assert!(ui.done.is_some(), "{status}");
        assert!(status.contains("added"), "{status}");
        assert_eq!(f.session.editor.doc().objects.len(), before + 1);
        // The window stays with the result; closing it drops the overlay.
        frame(&mut ui, &mut f.session, &mut status);
        assert!(ui.history_doc().is_none() && ui.draw().is_none());
        ui.close(&mut f.session);
        assert!(!ui.open);
    }

    #[test]
    fn a_style_change_makes_the_ghosts_again() {
        let mut f = fixture(json!({}));
        let mut ui = UpdateUi::new();
        let mut status = String::new();
        ui.begin(&mut f.session, f.new.clone());
        assert!(!ui.ghosts.is_empty());
        ui.style.show_ghosts = false;
        ui.style.show_previous = false;
        ui.style_dirty = true;
        frame(&mut ui, &mut f.session, &mut status);
        assert!(ui.ghosts.is_empty());
        ui.set_dark(true);
        assert!(ui.style.dark && ui.style_dirty);
        ui.show_on_drawing = false;
        assert!(ui.history_doc().is_none() && ui.draw().is_none());
    }

    #[test]
    fn an_edit_makes_the_plan_old_and_apply_makes_it_again() {
        let mut f = fixture(json!({}));
        let mut ui = UpdateUi::new();
        let mut status = String::new();
        ui.begin(&mut f.session, f.new.clone());
        f.session
            .editor
            .execute(&Command::AddObject {
                shape: kadakas_core::model::Shape::Circle {
                    center: glam::DVec2::ZERO,
                    radius: 10.0,
                },
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
        frame(&mut ui, &mut f.session, &mut status);
        assert!(ui.expired && ui.plan.is_none());
        // "Update again" makes the plan again and keeps the ticks of the person.
        let mut ticks: Vec<String> = ui.accepted.iter().cloned().collect();
        ticks.sort();
        let unticked = ticks.pop().unwrap();
        ui.accepted.remove(&unticked);
        let expected: HashSet<String> = ticks.iter().cloned().collect();
        f.session.editor.undo();
        ui.run(&mut f.session);
        assert_eq!(ui.accepted, expected, "the choice of the person stays");
        // Apply on an old plan makes it again, with the same choice, and applies it.
        f.session
            .editor
            .execute(&Command::AddObject {
                shape: kadakas_core::model::Shape::Circle {
                    center: glam::DVec2::ZERO,
                    radius: 10.0,
                },
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
        f.session.editor.undo();
        assert!(f.session.update_pending().is_none());
        ui.apply(&mut f.session, &mut status);
        assert!(ui.done.is_some(), "{status}");
        assert!(f.session.editor.doc().objects.len() >= 4);
    }

    #[test]
    fn a_change_that_became_a_conflict_is_unticked_and_not_applied_in_the_same_click() {
        let mut f = fixture(json!({}));
        let mut ui = UpdateUi::new();
        let mut status = String::new();
        ui.begin(&mut f.session, f.new.clone());
        let ticked = ui.accepted.len();
        assert_eq!(ticked, 4);
        // The person edits the line 0x11 (it moves in v2) after the plan was made.
        let id = f
            .session
            .editor
            .doc()
            .objects
            .values()
            .find(|o| {
                kadakas_core::provenance::tag(&o.data)
                    .and_then(|t| t.handle)
                    .as_deref()
                    == Some("11")
            })
            .unwrap()
            .id;
        f.session
            .editor
            .execute(&Command::MoveObjects {
                ids: vec![id],
                delta: glam::DVec2::new(0.0, 7.0),
            })
            .unwrap();
        frame(&mut ui, &mut f.session, &mut status);
        assert!(ui.expired);
        let before = f.session.editor.doc().objects.len();
        ui.apply(&mut f.session, &mut status);
        // The plan is shown again with the new conflict unticked, and nothing was applied.
        assert!(ui.done.is_none() && ui.plan.is_some(), "{status}");
        assert_eq!(f.session.editor.doc().objects.len(), before);
        assert_eq!(ui.accepted.len(), ticked - 1);
        assert!(ui.note.is_some());
        let conflict = ui
            .plan
            .as_ref()
            .unwrap()
            .plan
            .changes
            .iter()
            .find(|c| c.conflict)
            .unwrap()
            .id
            .clone();
        assert!(!ui.accepted.contains(&conflict));
        // Pressing Apply now applies the other changes.
        ui.apply(&mut f.session, &mut status);
        assert!(ui.done.is_some(), "{status}");
    }

    #[test]
    fn a_file_of_another_lineage_is_confirmed_and_a_big_drawing_asks_for_a_link_or_none() {
        let mut f = fixture(json!({}));
        let anon = f._dir.path().join("anon.dxf");
        std::fs::write(&anon, v2().replace(GUID, "00000000")).unwrap();
        let mut ui = UpdateUi::new();
        let mut status = String::new();
        ui.begin(&mut f.session, anon);
        let Some(Problem::ConfirmDrawing { drawing, .. }) = ui.problem.clone() else {
            panic!("{:?}", ui.problem)
        };
        frame(&mut ui, &mut f.session, &mut status);
        ui.drawing = Some(drawing);
        ui.run(&mut f.session);
        assert!(ui.problem.is_none() && ui.plan.is_some());
        // The update data of the new revision does not fit: the same question as the import.
        f.session.set_limits(kadakas_api::SessionLimits {
            base_records: 2,
            ..kadakas_api::SessionLimits::default()
        });
        ui.begin(&mut f.session, f.new.clone());
        let Some(Problem::BaseChoice(question)) = ui.problem.clone() else {
            panic!("{:?}", ui.problem)
        };
        assert!(question.contains("link"), "{question}");
        frame(&mut ui, &mut f.session, &mut status);
        ui.base = Some("none");
        ui.run(&mut f.session);
        assert!(ui.problem.is_none() && ui.plan.is_some());
    }

    #[test]
    fn a_drawing_without_update_data_asks_for_the_old_file() {
        let mut f = fixture(json!({"base": "none"}));
        let mut ui = UpdateUi::new();
        let mut status = String::new();
        ui.begin(&mut f.session, f.new.clone());
        assert!(matches!(ui.problem, Some(Problem::NeedsOldFile(_))));
        frame(&mut ui, &mut f.session, &mut status);
        // The old file given: the update works.
        ui.old = Some(f._dir.path().join("old.dxf"));
        std::fs::write(f._dir.path().join("old.dxf"), v1()).unwrap();
        ui.run(&mut f.session);
        assert!(ui.problem.is_none() && ui.plan.is_some());
    }

    #[test]
    fn several_drawings_ask_which_one_and_a_bad_file_is_told() {
        let mut f = fixture(json!({}));
        let other = f._dir.path().join("other.dxf");
        std::fs::write(&other, v1().replace(GUID, "FFFF0002-OTHER")).unwrap();
        f.session
            .import_file_trusted(json!({"path": other}))
            .unwrap();
        let anon = f._dir.path().join("anon.dxf");
        std::fs::write(&anon, v2().replace(GUID, "00000000")).unwrap();
        let mut ui = UpdateUi::new();
        let mut status = String::new();
        ui.begin(&mut f.session, anon);
        let Some(Problem::WhichDrawing(names)) = ui.problem.clone() else {
            panic!("{:?}", ui.problem)
        };
        assert_eq!(names.len(), 2);
        frame(&mut ui, &mut f.session, &mut status);
        ui.drawing = Some(names[0].clone());
        ui.run(&mut f.session);
        assert!(ui.problem.is_none());
        let junk = f._dir.path().join("junk.dxf");
        std::fs::write(&junk, "not a drawing").unwrap();
        ui.begin(&mut f.session, junk);
        assert!(matches!(ui.problem, Some(Problem::Failed(_))));
        frame(&mut ui, &mut f.session, &mut status);
        ui.reset(&mut f.session);
        assert!(!ui.open && ui.plan.is_none());
    }
}
