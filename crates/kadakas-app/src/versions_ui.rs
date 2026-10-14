//! The Versions window (REV-01, REV-02): File > Versions...
//!
//! It lists the named versions of the document, saves a new one, restores or deletes one, and
//! compares two versions or a version and the open drawing. The window only draws. It calls the
//! `version.*` methods of the API, so the app and an agent use the same path and every change is
//! one undo step. The logic of the form and of the answers is in `kadakas-ui-core::versions`.
//!
//! The colored compare in the plan and in 3D is REV-03 and is not here.

use egui::{Color32, Ui};
use kadakas_api::Session;
use kadakas_core::model::ObjectId;
use kadakas_core::uid::Uid;
use kadakas_l10n::tr;
use kadakas_ui_core::versions::{
    Change, ChangeRow, DiffView, LimitKind, LimitReached, SaveError, SaveForm, Side, SizeUnit,
    VersionList, VersionRow, diff_params, size_parts, time_parts,
};
use serde_json::{Value, json};

/// The most things that a comparison lists.
const DIFF_LIMIT: usize = 200;
/// How long the drawing must stay unchanged before the window compares it with the newest
/// version (a pass over the whole drawing), in seconds.
const CHECK_DELAY: f64 = 0.5;

/// What the person is asked to confirm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confirm {
    Restore(u64),
    Delete(u64),
    /// Keep only the part of the history that could be read.
    Repair,
}

/// What goes on after the person made room, or raised the limit.
#[derive(Clone, Debug, PartialEq)]
enum Retry {
    Save(Value),
    Restore(u64),
}

/// The dialog of a save that the limit stopped: the versions, to tick the ones to delete.
struct LimitDialog {
    info: LimitReached,
    retry: Retry,
}

/// State of the window.
pub struct VersionsUi {
    open: bool,
    form: SaveForm,
    list: VersionList,
    /// The revision of the document when the list was read.
    revision: u64,
    from: Option<Side>,
    to: Side,
    diff: Option<DiffView>,
    confirm: Option<Confirm>,
    /// A save or a restore that a limit stopped, waiting for the choice of the person.
    limit: Option<LimitDialog>,
    /// The person asked to raise the limit: the app opens Preferences.
    preferences_requested: bool,
    /// When the comparison of the drawing with the newest version is due (the time of egui).
    check_due: Option<f64>,
}

impl Default for VersionsUi {
    fn default() -> Self {
        VersionsUi {
            open: false,
            form: SaveForm::default(),
            list: VersionList::default(),
            revision: 0,
            from: None,
            to: Side::Current,
            diff: None,
            confirm: None,
            limit: None,
            preferences_requested: false,
            check_due: None,
        }
    }
}

/// The date and the time of day (UTC) of a version, in the date format of the person.
fn time_text(row: &VersionRow) -> String {
    match time_parts(row.time_ms) {
        None => tr!("versions-ui-unknown-time"),
        Some((secs, h, m)) => format!(
            "{} {h:02}:{m:02}",
            kadakas_units::current().date_text(kadakas_units::Date::from_unix_secs(secs))
        ),
    }
}

fn origin_text(origin: &str) -> String {
    match origin {
        "agent" => tr!("versions-ui-origin-agent"),
        "script" => tr!("versions-ui-origin-script"),
        "remote" => tr!("versions-ui-origin-remote"),
        _ => tr!("versions-ui-origin-user"),
    }
}

fn group_text(group: &str) -> String {
    match group {
        "layers" => tr!("versions-ui-group-layers"),
        "classes" => tr!("versions-ui-group-classes"),
        "resources" => tr!("versions-ui-group-resources"),
        "document" => tr!("versions-ui-group-document"),
        _ => tr!("versions-ui-group-objects"),
    }
}

fn side_text(list: &VersionList, side: Side) -> String {
    match side {
        Side::Current => tr!("versions-ui-current"),
        Side::Version(id) => list.name_of(id),
    }
}

impl VersionsUi {
    /// Opens the window and reads the versions.
    pub fn open(&mut self, session: &mut Session) {
        self.open = true;
        self.refresh(session);
        if self.form.name.is_empty() {
            self.form.name = self.default_name();
        }
    }

    fn default_name(&self) -> String {
        let number = self
            .list
            .rows
            .iter()
            .map(|r| r.id)
            .max()
            .map_or(1, |m| m + 1);
        tr!("versions-ui-default-name", number = number)
    }

    fn refresh(&mut self, session: &mut Session) {
        self.refresh_with(session, true);
    }

    /// Reads the versions. With `current` false the answer has no comparison of the drawing with
    /// the newest version, which costs a pass over the drawing.
    fn refresh_with(&mut self, session: &mut Session, current: bool) {
        self.revision = session.editor.revision();
        self.list = session
            .handle("version.list", json!({"current": current}))
            .map(|v| VersionList::parse(&v))
            .unwrap_or_default();
        if let Some(Confirm::Restore(id) | Confirm::Delete(id)) = self.confirm
            && self.list.find(id).is_none()
        {
            self.confirm = None;
        }
        if self.confirm == Some(Confirm::Repair) && !self.list.read_only {
            self.confirm = None;
        }
        // A side that is gone cannot be compared. The drawing changed: a comparison with it is old.
        let gone = |s: Side| matches!(s, Side::Version(id) if self.list.find(id).is_none());
        if self.from.is_some_and(gone) {
            self.from = None;
        }
        if gone(self.to) {
            self.to = Side::Current;
        }
        if self.from == Some(Side::Current) || self.to == Side::Current || self.from.is_none() {
            self.diff = None;
        }
    }

    /// True once after the person asked to raise the limit in Preferences.
    pub fn take_preferences_request(&mut self) -> bool {
        std::mem::take(&mut self.preferences_requested)
    }

    fn save(&mut self, session: &mut Session, status: &mut String) {
        let params = match self.form.params() {
            Ok(mut p) => {
                p["now_ms"] = json!(crate::collab_ui::now_ms());
                p
            }
            Err(e) => {
                *status = match e {
                    SaveError::NameEmpty => tr!("versions-ui-error-name"),
                    SaveError::NameTooLong => tr!(
                        "versions-ui-error-name-long",
                        max = kadakas_ui_core::versions::NAME_MAX
                    ),
                    SaveError::NoteTooLong => tr!(
                        "versions-ui-error-note-long",
                        max = kadakas_ui_core::versions::NOTE_MAX
                    ),
                };
                return;
            }
        };
        self.try_save(session, params, status);
    }

    /// Saves a version. A limit that stops it opens the dialog; the choice of the person decides.
    fn try_save(&mut self, session: &mut Session, params: Value, status: &mut String) {
        match session.handle("version.create", params.clone()) {
            Ok(v) => {
                *status = tr!(
                    "versions-ui-saved",
                    name = v["version"]["name"].as_str().unwrap_or("")
                );
                self.form.clear();
                self.refresh(session);
                self.form.name = self.default_name();
            }
            Err(e) => self.failed(e, Retry::Save(params), status),
        }
    }

    /// An error of a call: a limit opens the dialog, anything else is a message.
    fn failed(&mut self, e: kadakas_api::RpcError, retry: Retry, status: &mut String) {
        if e.reason == Some(kadakas_api::REASON_VERSION_LIMIT)
            && let Some(info) = e.details.as_ref().and_then(LimitReached::parse)
        {
            self.limit = Some(LimitDialog { info, retry });
            return;
        }
        *status = tr!("panels-error", e = e.message);
    }

    fn restore(&mut self, session: &mut Session, id: u64, status: &mut String) {
        let name = self.list.name_of(id);
        match session.handle(
            "version.restore",
            json!({"id": id, "now_ms": crate::collab_ui::now_ms()}),
        ) {
            Ok(v) => {
                let saved = v["saved_version"]["id"]
                    .as_u64()
                    .filter(|_| v["saved_version"]["new"] == json!(true));
                self.refresh(session);
                *status = match saved {
                    Some(s) => tr!(
                        "versions-ui-restored-saved",
                        name = name,
                        saved = self.list.name_of(s)
                    ),
                    None => tr!("versions-ui-restored", name = name),
                };
            }
            Err(e) => self.failed(e, Retry::Restore(id), status),
        }
    }

    fn delete(&mut self, session: &mut Session, id: u64, status: &mut String) {
        let name = self.list.name_of(id);
        match session.handle("version.delete", json!({"id": id})) {
            Ok(_) => {
                self.refresh(session);
                *status = tr!("versions-ui-deleted", name = name);
            }
            Err(e) => *status = tr!("panels-error", e = e.message),
        }
    }

    /// Keeps only the part of the history that could be read (the person said yes).
    fn repair(&mut self, session: &mut Session, status: &mut String) {
        match session.handle("version.repair", json!({"confirm": true})) {
            Ok(v) => {
                self.refresh(session);
                *status = tr!(
                    "versions-ui-repaired",
                    count = v["kept"].as_u64().unwrap_or(0)
                );
            }
            Err(e) => *status = tr!("panels-error", e = e.message),
        }
    }

    /// Deletes the versions that the person ticked in the dialog, and goes on with the save.
    fn delete_ticked_and_go_on(&mut self, session: &mut Session, status: &mut String) {
        let Some(dialog) = self.limit.take() else {
            return;
        };
        let ids = dialog.info.ticked();
        match session.handle("version.delete", json!({"ids": ids})) {
            Ok(_) => {
                *status = tr!("versions-ui-deleted-many", count = ids.len());
                self.refresh(session);
                self.go_on(session, dialog.retry, status);
            }
            Err(e) => *status = tr!("panels-error", e = e.message),
        }
    }

    /// Tries the save or the restore again (the limit may be higher now).
    fn go_on(&mut self, session: &mut Session, retry: Retry, status: &mut String) {
        match retry {
            Retry::Save(params) => self.try_save(session, params, status),
            Retry::Restore(id) => self.restore(session, id, status),
        }
    }

    /// The dialog of a save that the limit stopped.
    fn limit_dialog(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        let mut action = None;
        let mut open = true;
        let Some(dialog) = &mut self.limit else {
            return;
        };
        egui::Window::new(tr!("versions-ui-limit-title"))
            .id(egui::Id::new("win-versions-limit"))
            .open(&mut open)
            .collapsible(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                let size = |b: u64| {
                    let (v, unit) = size_parts(b);
                    let n = kadakas_units::current().number_text(v, 1);
                    match unit {
                        SizeUnit::Kib => tr!("versions-ui-size-kib", size = n),
                        SizeUnit::Mib => tr!("versions-ui-size-mib", size = n),
                    }
                };
                let info = &mut dialog.info;
                ui.label(match info.kind {
                    LimitKind::Versions => tr!(
                        "versions-ui-limit-versions",
                        needed = info.needed,
                        max = info.max
                    ),
                    LimitKind::Bytes => tr!(
                        "versions-ui-limit-bytes",
                        needed = size(info.needed),
                        max = size(info.max)
                    ),
                });
                ui.label(tr!("versions-ui-limit-choose"));
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("versions-limit-list")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for row in info.rows.iter_mut().rev() {
                            ui.horizontal_wrapped(|ui| {
                                ui.checkbox(&mut row.ticked, "");
                                ui.strong(&row.name);
                                if row.auto {
                                    ui.small(tr!("versions-ui-auto"));
                                }
                                ui.small(size(row.bytes));
                                ui.small(time_text(&VersionRow {
                                    id: row.id,
                                    name: row.name.clone(),
                                    note: None,
                                    author: String::new(),
                                    time_ms: row.time_ms,
                                    origin: String::new(),
                                    auto: row.auto,
                                    branch: String::new(),
                                }));
                            });
                        }
                    });
                ui.separator();
                let ticked = !info.ticked().is_empty();
                if ticked {
                    ui.small(if info.enough() {
                        tr!("versions-ui-limit-room")
                    } else {
                        tr!("versions-ui-limit-no-room")
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            ticked,
                            egui::Button::new(tr!(
                                "versions-ui-limit-delete",
                                count = info.ticked().len()
                            )),
                        )
                        .clicked()
                    {
                        action = Some(0);
                    }
                    if ui.button(tr!("versions-ui-limit-preferences")).clicked() {
                        action = Some(1);
                    }
                    if ui.button(tr!("versions-ui-limit-again")).clicked() {
                        action = Some(2);
                    }
                    if ui.button(tr!("common-cancel")).clicked() {
                        action = Some(3);
                    }
                });
            });
        if !open {
            action = Some(3);
        }
        match action {
            Some(0) => self.delete_ticked_and_go_on(session, status),
            Some(1) => self.preferences_requested = true,
            Some(2) => {
                if let Some(d) = self.limit.take() {
                    self.go_on(session, d.retry, status);
                }
            }
            Some(_) => self.limit = None,
            None => {}
        }
    }

    fn compare(&mut self, session: &mut Session, status: &mut String) {
        let Some(from) = self.from else { return };
        match session.handle("version.diff", diff_params(from, self.to, DIFF_LIMIT)) {
            Ok(v) => self.diff = Some(DiffView::parse(&v)),
            Err(e) => {
                self.diff = None;
                *status = tr!("panels-error", e = e.message);
            }
        }
    }

    /// Selects one object of the comparison in the drawing.
    fn select(&self, session: &mut Session, row: &ChangeRow, status: &mut String) {
        let Some(id) = Uid::parse(&row.id).map(ObjectId) else {
            return;
        };
        if session.editor.doc().object(id).is_none() {
            *status = tr!("versions-ui-gone");
            return;
        }
        if let Err(e) = session.handle("selection.set", json!({"ids": [row.id]})) {
            *status = e.message;
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        if !self.open {
            return;
        }
        // The drawing changes often (every move of the pointer in a drag). The list is read at
        // once, the comparison with the newest version when the drawing has been quiet.
        let now = ctx.input(|i| i.time);
        if self.revision != session.editor.revision() {
            self.refresh_with(session, false);
            self.check_due = Some(now + CHECK_DELAY);
        }
        if let Some(due) = self.check_due {
            if now >= due {
                self.check_due = None;
                self.refresh(session);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            }
        }
        let mut open = self.open;
        egui::Window::new(tr!("versions-ui-title"))
            .id(egui::Id::new("win-versions"))
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| self.body(ui, session, status));
        self.open = open;
        self.limit_dialog(ctx, session, status);
    }

    fn body(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        ui.small(tr!("versions-ui-intro"));
        if self.list.read_only {
            self.read_only_notice(ui, session, status);
        }
        ui.separator();
        ui.strong(tr!("versions-ui-save-heading"));
        egui::Grid::new("versions-save")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(tr!("versions-ui-name"));
                ui.text_edit_singleline(&mut self.form.name);
                ui.end_row();
                ui.label(tr!("versions-ui-note"));
                ui.text_edit_multiline(&mut self.form.note);
                ui.end_row();
            });
        if ui
            .add_enabled(
                !self.list.read_only,
                egui::Button::new(tr!("versions-ui-save")),
            )
            .clicked()
        {
            self.save(session, status);
        }
        ui.small(tr!("versions-ui-undo-note"));
        ui.separator();
        self.list_section(ui, session, status);
        ui.separator();
        self.compare_section(ui, session, status);
    }

    /// The history of the file could not be read completely: it is kept as it is. The person may
    /// choose to keep only what could be read.
    fn read_only_notice(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        ui.separator();
        ui.colored_label(Color32::from_rgb(200, 120, 0), tr!("versions-ui-read-only"));
        for note in &self.list.notes {
            ui.small(note);
        }
        // The button says how many versions it deletes, when that is known.
        let button = match self.list.unreadable {
            Some(n) => tr!("versions-ui-readable-only-count", count = n),
            None => tr!("versions-ui-readable-only"),
        };
        if self.confirm == Some(Confirm::Repair) {
            ui.label(match self.list.unreadable {
                Some(n) => tr!("versions-ui-confirm-repair-count", count = n),
                None => tr!("versions-ui-confirm-repair"),
            });
            ui.horizontal(|ui| {
                if ui.button(&button).clicked() {
                    self.confirm = None;
                    self.repair(session, status);
                }
                if ui.button(tr!("common-cancel")).clicked() {
                    self.confirm = None;
                }
            });
        } else if ui.button(&button).clicked() {
            self.confirm = Some(Confirm::Repair);
        }
    }

    fn list_section(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        ui.strong(tr!("versions-ui-list-heading"));
        if self.list.rows.is_empty() {
            ui.label(tr!("versions-ui-none"));
            return;
        }
        ui.small(tr!(
            "versions-ui-limits",
            count = self.list.rows.len(),
            max = self.list.max_versions
        ));
        match self.list.current_is_newest {
            Some(true) => {
                ui.small(tr!("versions-ui-newest-is-current"));
            }
            Some(false) => {
                ui.small(tr!("versions-ui-newest-differs"));
            }
            None => {}
        }
        let mut ask = None;
        egui::ScrollArea::vertical()
            .id_salt("versions-list")
            .max_height(220.0)
            .show(ui, |ui| {
                for row in self.list.rows.iter().rev() {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(&row.name);
                        if row.auto {
                            ui.small(tr!("versions-ui-auto"));
                        }
                    });
                    let mut meta = format!("{}, {}", time_text(row), origin_text(&row.origin));
                    if !row.author.is_empty() {
                        meta = format!(
                            "{meta}, {}",
                            tr!("versions-ui-by", author = row.author.clone())
                        );
                    }
                    ui.small(meta);
                    if let Some(note) = &row.note {
                        ui.small(note);
                    }
                    ui.horizontal(|ui| {
                        let ok = !self.list.read_only;
                        if ui
                            .add_enabled(ok, egui::Button::new(tr!("versions-ui-restore")))
                            .clicked()
                        {
                            ask = Some(Confirm::Restore(row.id));
                        }
                        if ui
                            .add_enabled(ok, egui::Button::new(tr!("versions-ui-delete")))
                            .clicked()
                        {
                            ask = Some(Confirm::Delete(row.id));
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        if ask.is_some() {
            self.confirm = ask;
        }
        let Some(confirm) = self.confirm else { return };
        let (text, label) = match confirm {
            Confirm::Restore(id) => (
                tr!("versions-ui-confirm-restore", name = self.list.name_of(id)),
                tr!("versions-ui-restore"),
            ),
            Confirm::Delete(id) => (
                tr!("versions-ui-confirm-delete", name = self.list.name_of(id)),
                tr!("versions-ui-delete"),
            ),
            // The notice has its own question.
            Confirm::Repair => return,
        };
        ui.separator();
        ui.label(text);
        ui.horizontal(|ui| {
            if ui.button(label).clicked() {
                self.confirm = None;
                match confirm {
                    Confirm::Restore(id) => self.restore(session, id, status),
                    Confirm::Delete(id) => self.delete(session, id, status),
                    Confirm::Repair => {}
                }
            }
            if ui.button(tr!("common-cancel")).clicked() {
                self.confirm = None;
            }
        });
    }

    fn side_combo(
        &self,
        ui: &mut Ui,
        salt: &str,
        current: Option<Side>,
        with_current: bool,
    ) -> Option<Side> {
        let mut chosen = current;
        let text = current.map_or_else(|| tr!("versions-ui-pick"), |s| side_text(&self.list, s));
        egui::ComboBox::from_id_salt(salt)
            .selected_text(text)
            .show_ui(ui, |ui| {
                for row in self.list.rows.iter().rev() {
                    ui.selectable_value(&mut chosen, Some(Side::Version(row.id)), &row.name);
                }
                if with_current {
                    ui.selectable_value(
                        &mut chosen,
                        Some(Side::Current),
                        tr!("versions-ui-current"),
                    );
                }
            });
        (chosen != current).then_some(chosen).flatten()
    }

    fn compare_section(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        ui.strong(tr!("versions-ui-compare-heading"));
        if self.list.rows.is_empty() {
            return;
        }
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(tr!("versions-ui-from"));
            if let Some(s) = self.side_combo(ui, "versions-from", self.from, true) {
                self.from = Some(s);
                changed = true;
            }
            ui.label(tr!("versions-ui-to"));
            if let Some(s) = self.side_combo(ui, "versions-to", Some(self.to), true) {
                self.to = s;
                changed = true;
            }
        });
        if changed {
            self.diff = None;
        }
        if ui
            .add_enabled(
                self.from.is_some(),
                egui::Button::new(tr!("versions-ui-compare")),
            )
            .clicked()
        {
            self.compare(session, status);
        }
        let Some(diff) = self.diff.clone() else {
            return;
        };
        if diff.is_empty() {
            ui.label(tr!("versions-ui-no-changes"));
            return;
        }
        ui.label(tr!(
            "versions-ui-counts",
            added = diff.total_added,
            removed = diff.total_removed,
            changed = diff.total_changed
        ));
        if !diff.exact {
            ui.colored_label(Color32::from_rgb(200, 120, 0), tr!("versions-ui-partial"));
        }
        if diff.truncated {
            ui.small(tr!("versions-ui-more"));
        }
        ui.small(tr!("versions-ui-click-to-select"));
        egui::ScrollArea::vertical()
            .id_salt("versions-diff")
            .max_height(260.0)
            .show(ui, |ui| {
                let mut group = "";
                for row in &diff.rows {
                    if row.group != group {
                        group = &row.group;
                        ui.add_space(4.0);
                        if let Some(c) = diff.groups.iter().find(|c| c.group == row.group) {
                            ui.strong(format!(
                                "{}: {}",
                                group_text(group),
                                tr!(
                                    "versions-ui-counts",
                                    added = c.added,
                                    removed = c.removed,
                                    changed = c.changed
                                )
                            ));
                        } else {
                            ui.strong(group_text(group));
                        }
                    }
                    self.diff_row(ui, session, row, status);
                }
            });
    }

    fn diff_row(&self, ui: &mut Ui, session: &mut Session, row: &ChangeRow, status: &mut String) {
        let (tag, color) = match row.change {
            Change::Added => (tr!("versions-ui-added"), Color32::from_rgb(40, 140, 60)),
            Change::Removed => (tr!("versions-ui-removed"), Color32::from_rgb(200, 50, 50)),
            Change::Changed => (tr!("versions-ui-changed"), Color32::from_rgb(220, 130, 0)),
        };
        let what = row
            .shape
            .clone()
            .or_else(|| row.name.clone())
            .unwrap_or_else(|| row.id.chars().take(8).collect());
        let mut text = what;
        if !row.paths.is_empty() {
            text = format!("{text} ({})", row.paths.join(", "));
        }
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(color, tag);
            if row.selectable() {
                if ui.selectable_label(false, text).clicked() {
                    self.select(session, row, status);
                }
            } else {
                ui.label(text);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(s: &mut Session, x: i32) -> String {
        s.handle(
            "execute",
            json!({"command": {"op": "add_object", "shape":
                {"kind": "rect", "min": [x, 0], "max": [x + 10, 10]}}}),
        )
        .unwrap()["created"][0]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn save_restore_delete_and_compare_work_through_the_api() {
        let mut s = Session::with_sequential_ids();
        let mut ui = VersionsUi::default();
        let mut status = String::new();
        add(&mut s, 0);
        ui.open(&mut s);
        assert!(ui.open && ui.list.rows.is_empty());
        assert!(
            !ui.form.name.is_empty(),
            "the name starts with a suggestion"
        );
        ui.form.name = "First".into();
        ui.form.note = "a note".into();
        ui.save(&mut s, &mut status);
        assert!(status.contains("First"), "{status}");
        assert_eq!(ui.list.rows.len(), 1);
        assert!(ui.form.note.is_empty() && !ui.form.name.is_empty());
        // A bad form is a message, not a call.
        ui.form.name = "  ".into();
        ui.save(&mut s, &mut status);
        assert_eq!(ui.list.rows.len(), 1);
        ui.form.name = "n".repeat(500);
        ui.save(&mut s, &mut status);
        ui.form.name = "ok".into();
        ui.form.note = "n".repeat(5000);
        ui.save(&mut s, &mut status);
        assert_eq!(ui.list.rows.len(), 1);

        let second = add(&mut s, 50);
        ui.form.name = "Second".into();
        ui.form.note.clear();
        ui.save(&mut s, &mut status);
        assert_eq!(ui.list.rows.len(), 2);
        add(&mut s, 70);

        // Compare the first version with the drawing.
        ui.from = Some(Side::Version(1));
        ui.to = Side::Current;
        ui.compare(&mut s, &mut status);
        let d = ui.diff.clone().unwrap();
        assert_eq!(d.total_added, 2);
        assert_eq!(d.rows[0].id, second);
        // A click selects the object; an object that is gone says so.
        ui.select(&mut s, &d.rows[0], &mut status);
        assert_eq!(s.selection.len(), 1);
        let mut gone = d.rows[0].clone();
        gone.id = "00000000-0000-0000-0000-0000000000ff".into();
        ui.select(&mut s, &gone, &mut status);
        assert_eq!(status, tr!("versions-ui-gone"));
        gone.id = "nonsense".into();
        ui.select(&mut s, &gone, &mut status);

        // Restore, then delete.
        ui.restore(&mut s, 1, &mut status);
        assert!(status.contains("First"));
        assert_eq!(s.editor.doc().objects.len(), 1);
        assert_eq!(
            ui.list.rows.len(),
            3,
            "the work was saved as a version first"
        );
        assert!(status.contains("Undo"));
        ui.delete(&mut s, 3, &mut status);
        assert_eq!(ui.list.rows.len(), 2);
        // An error shows as a message.
        ui.delete(&mut s, 99, &mut status);
        ui.restore(&mut s, 99, &mut status);
        ui.from = Some(Side::Version(99));
        ui.compare(&mut s, &mut status);
        assert!(ui.diff.is_none());
        // The window follows the document.
        add(&mut s, 90);
        ui.refresh(&mut s);
        assert_eq!(ui.list.current_is_newest, Some(false));
    }

    #[test]
    fn the_window_draws_in_every_language() {
        for lang in kadakas_l10n::languages() {
            kadakas_l10n::with_language(lang.tag, || {
                let mut s = Session::with_sequential_ids();
                let mut ui = VersionsUi::default();
                let mut status = String::new();
                let ctx = egui::Context::default();
                let draw = |ui: &mut VersionsUi, s: &mut Session, status: &mut String| {
                    let _ = ctx.run_ui(egui::RawInput::default(), |u| {
                        ui.show(&u.ctx().clone(), s, status);
                    });
                };
                draw(&mut ui, &mut s, &mut status);
                ui.open(&mut s);
                draw(&mut ui, &mut s, &mut status);
                add(&mut s, 0);
                ui.form.name = "One".into();
                ui.form.note = "note".into();
                ui.save(&mut s, &mut status);
                add(&mut s, 30);
                ui.refresh(&mut s);
                ui.from = Some(Side::Version(1));
                ui.compare(&mut s, &mut status);
                draw(&mut ui, &mut s, &mut status);
                assert!(ui.diff.is_some());
                s.handle(
                    "execute",
                    json!({"command": {"op": "add_layer", "name": "Plan"}}),
                )
                .unwrap();
                ui.refresh(&mut s);
                ui.compare(&mut s, &mut status);
                draw(&mut ui, &mut s, &mut status);
                ui.from = Some(Side::Current);
                ui.compare(&mut s, &mut status);
                draw(&mut ui, &mut s, &mut status);
                assert!(ui.diff.as_ref().unwrap().is_empty());
            });
        }
    }

    #[test]
    fn restore_and_delete_ask_first_and_say_what_undo_does() {
        for lang in kadakas_l10n::languages() {
            kadakas_l10n::with_language(lang.tag, || {
                let mut s = Session::with_sequential_ids();
                let mut ui = VersionsUi::default();
                let mut status = String::new();
                add(&mut s, 0);
                ui.open(&mut s);
                ui.form.name = "One".into();
                ui.save(&mut s, &mut status);
                add(&mut s, 30);
                let ctx = egui::Context::default();
                let draw = |ui: &mut VersionsUi, s: &mut Session, status: &mut String| {
                    let _ = ctx.run_ui(egui::RawInput::default(), |u| {
                        ui.show(&u.ctx().clone(), s, status);
                    });
                };
                // The question is drawn, and nothing was changed by drawing it.
                ui.confirm = Some(Confirm::Restore(1));
                draw(&mut ui, &mut s, &mut status);
                assert_eq!(s.editor.doc().objects.len(), 2);
                assert!(tr!("versions-ui-confirm-restore", name = "One").contains("One"));
                assert!(!tr!("versions-ui-confirm-delete", name = "One").is_empty());
                assert!(!tr!("versions-ui-undo-note").is_empty());
                assert!(!tr!("versions-ui-pick").is_empty());
                ui.confirm = Some(Confirm::Delete(1));
                draw(&mut ui, &mut s, &mut status);
                assert_eq!(ui.list.rows.len(), 1);
                // A question about a version that is gone is dropped.
                ui.delete(&mut s, 1, &mut status);
                ui.confirm = Some(Confirm::Delete(1));
                ui.refresh(&mut s);
                assert!(ui.confirm.is_none());
            });
        }
    }

    #[test]
    fn the_window_compares_with_the_newest_version_only_when_the_drawing_is_quiet() {
        let mut s = Session::with_sequential_ids();
        let mut ui = VersionsUi::default();
        let mut status = String::new();
        add(&mut s, 0);
        ui.open(&mut s);
        ui.form.name = "One".into();
        ui.save(&mut s, &mut status);
        assert_eq!(ui.list.current_is_newest, Some(true));
        // The drawing changes: the list is read at once without the comparison.
        add(&mut s, 30);
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |u| {
            ui.show(&u.ctx().clone(), &mut s, &mut status);
        });
        assert_eq!(ui.list.current_is_newest, None);
        assert!(ui.check_due.is_some());
        // When the time has come, the comparison is made.
        ui.check_due = Some(f64::MIN);
        let _ = ctx.run_ui(egui::RawInput::default(), |u| {
            ui.show(&u.ctx().clone(), &mut s, &mut status);
        });
        assert_eq!(ui.list.current_is_newest, Some(false));
        assert!(ui.check_due.is_none());
    }

    #[test]
    fn a_limit_stops_the_save_and_the_dialog_lets_the_person_choose() {
        let mut s = Session::with_sequential_ids();
        s.set_limits(kadakas_api::SessionLimits {
            max_versions: 2,
            ..Default::default()
        });
        let mut ui = VersionsUi::default();
        let mut status = String::new();
        for (i, name) in ["Alpha", "Beta"].into_iter().enumerate() {
            add(&mut s, i as i32 * 30);
            ui.open(&mut s);
            ui.form.name = name.into();
            ui.save(&mut s, &mut status);
        }
        assert_eq!(ui.list.rows.len(), 2);
        add(&mut s, 90);
        ui.form.name = "Gamma".into();
        ui.form.note = "n".into();
        ui.save(&mut s, &mut status);
        // Nothing was saved and nothing was deleted; the dialog asks.
        assert_eq!(ui.list.rows.len(), 2);
        let dialog = ui.limit.as_ref().expect("the limit dialog");
        assert_eq!(dialog.info.kind, LimitKind::Versions);
        assert_eq!(dialog.info.rows.len(), 2);
        assert!(matches!(dialog.retry, Retry::Save(_)));
        // The dialog draws in every language.
        for lang in kadakas_l10n::languages() {
            kadakas_l10n::with_language(lang.tag, || {
                let ctx = egui::Context::default();
                let _ = ctx.run_ui(egui::RawInput::default(), |u| {
                    ui.show(&u.ctx().clone(), &mut s, &mut status);
                });
            });
        }
        // The person ticks the newer one (not the oldest), and the save goes on.
        ui.limit.as_mut().unwrap().info.rows[1].ticked = true;
        assert!(ui.limit.as_ref().unwrap().info.enough());
        let ctx = egui::Context::default();
        ui.delete_ticked_and_go_on(&mut s, &mut status);
        assert!(ui.limit.is_none());
        let names: Vec<_> = ui.list.rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, ["Alpha", "Gamma"]);
        // The request to raise the limit goes to the app, and a retry with a higher limit works.
        add(&mut s, 120);
        ui.form.name = "Delta".into();
        ui.save(&mut s, &mut status);
        assert!(ui.limit.is_some());
        ui.preferences_requested = true;
        assert!(ui.take_preferences_request() && !ui.take_preferences_request());
        s.set_limits(kadakas_api::SessionLimits {
            max_versions: 3,
            ..Default::default()
        });
        let retry = ui.limit.take().unwrap().retry;
        ui.go_on(&mut s, retry, &mut status);
        assert_eq!(ui.list.rows.len(), 3);
        // A restore that the limit stops opens the dialog too, and Cancel closes it.
        add(&mut s, 150);
        ui.restore(&mut s, 1, &mut status);
        assert!(matches!(
            ui.limit.as_ref().unwrap().retry,
            Retry::Restore(1)
        ));
        let _ = ctx.run_ui(egui::RawInput::default(), |u| {
            ui.limit_dialog(&u.ctx().clone(), &mut s, &mut status);
        });
        ui.limit = None;
        // Another error is a message.
        ui.restore(&mut s, 99, &mut status);
        assert!(ui.limit.is_none() && !status.is_empty());
    }

    #[test]
    fn a_history_that_cannot_be_read_is_read_only_until_the_person_chooses() {
        let mut donor = Session::with_sequential_ids();
        add(&mut donor, 0);
        donor
            .handle("version.create", json!({"name": "One"}))
            .unwrap();
        add(&mut donor, 30);
        donor
            .handle("version.create", json!({"name": "Two"}))
            .unwrap();
        let mut file = serde_json::to_value(&*donor.document_for_save().unwrap()).unwrap();
        file["history"]["versions"][0]["delta"] = json!([{"op": "delete",
            "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000ee"}}]);
        let mut s = Session::with_sequential_ids();
        s.handle("document.load", json!({"document": file}))
            .unwrap();
        let mut ui = VersionsUi::default();
        let mut status = String::new();
        ui.open(&mut s);
        assert!(ui.list.read_only && !ui.list.notes.is_empty());
        for lang in kadakas_l10n::languages() {
            kadakas_l10n::with_language(lang.tag, || {
                let ctx = egui::Context::default();
                ui.confirm = None;
                let _ = ctx.run_ui(egui::RawInput::default(), |u| {
                    ui.show(&u.ctx().clone(), &mut s, &mut status);
                });
                ui.confirm = Some(Confirm::Repair);
                let _ = ctx.run_ui(egui::RawInput::default(), |u| {
                    ui.show(&u.ctx().clone(), &mut s, &mut status);
                });
            });
        }
        // A save is refused with a message (the buttons are disabled; this is the call).
        ui.form.name = "x".into();
        ui.save(&mut s, &mut status);
        assert!(ui.limit.is_none() && !status.is_empty());
        assert!(ui.list.read_only);
        // The person agrees: the readable part stays.
        ui.confirm = Some(Confirm::Repair);
        ui.repair(&mut s, &mut status);
        assert!(!ui.list.read_only);
        assert_eq!(ui.list.rows.len(), 1);
        assert!(ui.confirm.is_none());
        // Nothing to repair now: an error message, not a change.
        ui.repair(&mut s, &mut status);
        assert!(status.contains("nothing to repair"), "{status}");
        assert_eq!(ui.list.rows.len(), 1);
    }

    #[test]
    fn the_texts_exist_in_both_languages() {
        for lang in ["en-US", "et"] {
            kadakas_l10n::with_language(lang, || {
                let row = VersionRow {
                    id: 1,
                    name: "n".into(),
                    note: None,
                    author: String::new(),
                    time_ms: 1_700_000_000_000,
                    origin: "agent".into(),
                    auto: false,
                    branch: "main".into(),
                };
                assert!(time_text(&row).contains("22:13"));
                for o in ["user", "agent", "script", "remote"] {
                    assert!(!origin_text(o).is_empty());
                }
                for g in ["objects", "layers", "classes", "resources", "document"] {
                    assert!(!group_text(g).is_empty());
                }
                assert!(!side_text(&VersionList::default(), Side::Current).is_empty());
            });
        }
    }
}
