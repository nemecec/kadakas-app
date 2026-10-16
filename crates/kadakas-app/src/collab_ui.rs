//! Collaboration in the window (PLAT-09): the Connect dialog, the status item, the list of
//! people, the cursors and selections of the others, and the per-user undo.
//!
//! The window never runs a second client. Every step goes through the same calls as an agent
//! makes (`collab.connect`, `collab.sync`, `collab.presence`, `collab.undo`), so all rules of the
//! client stay: the server copy replaces the open document on joining, unsaved work needs a
//! confirmation (`replace`), and a token lives in memory only. The logic that needs no window is
//! in `kadakas_ui_core::collab`. This file holds the state, the egui drawing and the calls.

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Ui, vec2};
use glam::DVec2;
use kadakas_api::Session;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::uid::Uid;
use kadakas_l10n::tr;
use kadakas_ui_core::collab::{
    ConnectForm, ConnectIssue, ConnectRequest, Defaults, Link, Notice, OwnPresence, Peer, PeerBook,
    Severity, StatusView, Tracker, UndoRoute, classify_connect_error, token_expired, undo_route,
};
use serde_json::{Value, json};

use crate::view::{View, ViewExt as _};

/// The shortest time between two rounds with the API.
pub const TICK_MS: u64 = 100;

/// The time of the program in milliseconds, for the calls that need `now_ms`.
#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The time of the program in milliseconds, for the calls that need `now_ms`.
#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// What the question is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// The open document has changes that nobody saved; the server copy replaces it.
    Replace,
    /// Edits that were made offline wait for another document or user; they would be dropped.
    Discard,
}

/// A question that waits for the answer of the person. The flags hold the answers that were given
/// already, so a second question does not lose the first answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub ask: Ask,
    pub request: ConnectRequest,
    pub replace: bool,
    pub discard: bool,
}

/// A way to lose waiting edits that needs a question: leave the shared document, or close the
/// program, while edits wait for the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leave {
    Disconnect,
    Close,
}

fn issue_of(e: &kadakas_api::RpcError) -> ConnectIssue {
    classify_connect_error(e.reason, &e.message)
}

/// The id of the token field (its edit state is removed after the call, see `wipe_secret`).
const TOKEN_FIELD: &str = "collab-token-field";

/// What this user shows: the pointer, the selection and the view.
pub struct Own {
    pub cursor: Option<[f64; 2]>,
    pub selection: Vec<String>,
    /// Center (mm), scale (mm per pixel) and size (pixels).
    pub viewport: Option<([f64; 2], f64, [f64; 2])>,
}

/// What one round gave back to the window.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// A text for the status line.
    pub status: Option<String>,
    /// The document changed (other users' work arrived).
    pub document_changed: bool,
}

/// What a click on the status item asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusClick {
    OpenDialog,
    ShowPeople,
}

pub struct CollabUi {
    form: ConnectForm,
    open: bool,
    form_error: Option<String>,
    confirm: Option<Confirm>,
    tracker: Tracker,
    view: StatusView,
    peers: PeerBook,
    own: OwnPresence,
    last_tick_ms: Option<u64>,
    /// The session has a client that we follow.
    active: bool,
    endpoint: Option<String>,
    last_notice: Option<Notice>,
    name: String,
    /// A question about leaving while edits wait.
    leave: Option<Leave>,
    /// The person agreed to close while edits wait.
    allow_close: bool,
    close_now: bool,
    /// The person chose "Save first" in the question about unsaved work.
    save_requested: bool,
    /// The token field must be wiped in the next frame.
    wipe_token: bool,
}

impl CollabUi {
    pub fn new(defaults: Defaults) -> CollabUi {
        CollabUi {
            name: defaults.name.clone(),
            form: ConnectForm::new(defaults),
            open: false,
            form_error: None,
            confirm: None,
            tracker: Tracker::default(),
            view: StatusView::default(),
            peers: PeerBook::default(),
            own: OwnPresence::default(),
            last_tick_ms: None,
            active: false,
            endpoint: None,
            last_notice: None,
            leave: None,
            allow_close: false,
            close_now: false,
            save_requested: false,
            wipe_token: false,
        }
    }

    pub fn view(&self) -> &StatusView {
        &self.view
    }

    pub fn peers(&self) -> &[Peer] {
        self.peers.peers()
    }

    #[cfg(test)]
    pub fn confirm(&self) -> Option<&Confirm> {
        self.confirm.as_ref()
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while the session waits for the first snapshot: it will replace the document, so
    /// the window must not take edits now.
    pub fn waiting_for_snapshot(&self) -> bool {
        self.view.waiting_for_snapshot
    }

    #[cfg(test)]
    pub fn set_waiting(&mut self, on: bool) {
        self.view.waiting_for_snapshot = on;
    }

    /// True while the session is joined or joining a shared document (the API refuses the normal
    /// undo; Connect would be refused too).
    pub fn is_connected(&self) -> bool {
        self.view.api_connected
    }

    /// True while the open document belongs to the shared one: connected, joining, waiting for
    /// the next try after a lost link, or failed. New, Open and recovery must not replace it
    /// then, and Disconnect has something to do.
    pub fn is_active(&self) -> bool {
        self.view.is_active()
    }

    /// True if the session has a collaboration client in any state: Edit > Undo is the
    /// per-user undo then.
    pub fn is_linked(&self) -> bool {
        self.view.linked
    }

    pub fn open_dialog(&mut self) {
        self.open = true;
        self.form_error = None;
    }

    /// Fills the form (for example from the address fragment of the browser).
    #[cfg(target_arch = "wasm32")]
    pub fn set_form(&mut self, form: ConnectForm) {
        self.name = form.name.clone();
        self.form = form;
    }

    /// Joins with the request. Returns a text for the status line when the call failed. A
    /// question for the person (`Confirm`) is not an error: the dialog asks it next.
    pub fn connect(
        &mut self,
        session: &mut Session,
        request: &ConnectRequest,
        replace: bool,
        discard: bool,
        now_ms: u64,
    ) -> Option<String> {
        self.confirm = None;
        // A browser cannot see why a WebSocket was refused (an old token looks like a lost
        // network), so an old token is told here, before the call.
        if request
            .token
            .as_deref()
            .is_some_and(|t| token_expired(t, now_ms / 1000))
        {
            return Some(tr!("collab-ui-token-expired"));
        }
        match session.handle("collab.connect", request.params(now_ms, replace, discard)) {
            Ok(status) => {
                self.tracker.reset();
                self.own.reset();
                self.peers.clear();
                self.last_notice = None;
                self.active = true;
                self.last_tick_ms = None;
                let (view, _) = self.tracker.observe(&status, &[], now_ms);
                self.endpoint = status["endpoint"].as_str().map(str::to_string);
                self.view = view;
                None
            }
            Err(e) => match issue_of(&e) {
                ConnectIssue::NeedsReplace | ConnectIssue::NeedsDiscard => {
                    let ask = if matches!(issue_of(&e), ConnectIssue::NeedsReplace) {
                        Ask::Replace
                    } else {
                        Ask::Discard
                    };
                    self.confirm = Some(Confirm {
                        ask,
                        request: request.clone(),
                        replace,
                        discard,
                    });
                    None
                }
                ConnectIssue::AlreadyConnected => Some(tr!("collab-ui-already-connected")),
                ConnectIssue::Failed(m) => Some(tr!("collab-ui-connect-failed", error = m)),
            },
        }
    }

    /// Disconnect from the menu: with edits that the server has not confirmed, the person is
    /// asked first (the edits stay in the program until it closes, but nobody else sees them).
    pub fn ask_disconnect(&mut self, session: &mut Session, now_ms: u64) -> Option<String> {
        if self.view.pending > 0 {
            self.leave = Some(Leave::Disconnect);
            return None;
        }
        Some(self.disconnect(session, now_ms))
    }

    /// The window is asked to close. True if it may close now; else the question is shown.
    pub fn ask_close(&mut self) -> bool {
        if self.allow_close || self.view.pending == 0 || !self.view.is_active() {
            return true;
        }
        self.leave = Some(Leave::Close);
        false
    }

    /// True once after the person agreed to close: the window sends the close command again.
    pub fn take_close_now(&mut self) -> bool {
        std::mem::take(&mut self.close_now)
    }

    /// True once after the person chose "Save first".
    pub fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.save_requested)
    }

    /// The number of edits that the server has not confirmed (the browser warns before the
    /// page closes while it is above zero).
    #[cfg(any(test, target_arch = "wasm32"))]
    pub fn pending(&self) -> usize {
        if self.view.is_active() {
            self.view.pending
        } else {
            0
        }
    }

    /// Leaves the shared document. Edits go on offline.
    pub fn disconnect(&mut self, session: &mut Session, now_ms: u64) -> String {
        let _ = now_ms;
        let _ = session.handle("collab.disconnect", json!({}));
        self.active = false;
        self.tracker.reset();
        self.peers.clear();
        self.own.reset();
        self.view = StatusView::default();
        self.last_notice = None;
        tr!("collab-ui-disconnected")
    }

    /// One round with the API, at most every [`TICK_MS`]: sync, presence, status. Call it
    /// every frame; it returns at once when it is not time.
    pub fn tick(&mut self, session: &mut Session, now_ms: u64, own: &Own) -> Outcome {
        let mut out = Outcome::default();
        if self
            .last_tick_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < TICK_MS)
        {
            return out;
        }
        self.last_tick_ms = Some(now_ms);
        if !self.active {
            // An agent may have joined with the session of the window.
            let joined = session.handle("collab.status", Value::Null).is_ok_and(|s| {
                matches!(
                    s["state"].as_str(),
                    Some("connecting" | "syncing" | "online")
                )
            });
            if !joined {
                return out;
            }
            self.tracker.reset();
            self.active = true;
        }
        // A script run is open: its calls are the only calls the session serves. The sync waits;
        // the heartbeat is a few seconds at most, and a run is bounded.
        if session.script_running() {
            return out;
        }
        let revision = session.editor.revision();
        let sync = match session.handle("collab.sync", json!({"now_ms": now_ms})) {
            Ok(v) => v,
            Err(e) => {
                self.active = false;
                self.view = StatusView::default();
                out.status = Some(tr!("collab-ui-error", error = e.message));
                return out;
            }
        };
        let events = sync["events"].as_array().cloned().unwrap_or_default();
        let (view, notices) = self.tracker.observe(&sync["status"], &events, now_ms);
        self.endpoint = sync["status"]["endpoint"].as_str().map(str::to_string);
        let problems = sync["problems"].as_array().map_or(0, Vec::len);
        // Others see us only while the link is up.
        if matches!(view.link, Link::Connected | Link::Reconnecting) {
            let params = self
                .own
                .params(now_ms, own.cursor, &own.selection, own.viewport);
            if let Ok(p) = session.handle("collab.presence", params) {
                self.peers.update(&p["peers"], now_ms);
            }
        } else {
            self.peers.clear();
        }
        self.view = view;
        if matches!(self.view.link, Link::Offline) {
            self.active = false;
        }
        out.document_changed = session.editor.revision() != revision;
        if let Some(n) = notices.last() {
            self.last_notice = Some(n.clone());
        }
        let mut lines: Vec<String> = notices.iter().map(Notice::text).collect();
        if problems > 0 {
            lines.push(tr!("collab-ui-problems", count = problems));
        }
        if !lines.is_empty() {
            out.status = Some(lines.join(" "));
        }
        out
    }

    /// The status of the link, read now (not from the last round).
    fn fresh_view(&self, session: &mut Session) -> StatusView {
        match session.handle("collab.status", Value::Null) {
            Ok(s) => self.tracker.clone().observe(&s, &[], now_ms()).0,
            Err(_) => StatusView::default(),
        }
    }

    /// Which undo to use now: while connected the per-user undo, else the normal one.
    pub fn route(&self, session: &mut Session) -> UndoRoute {
        undo_route(&self.fresh_view(session))
    }

    /// True if the Undo or Redo item of the Edit menu works now.
    pub fn can_undo(&self, redo: bool) -> bool {
        if redo {
            self.view.can_redo
        } else {
            self.view.can_undo
        }
    }

    /// The per-user undo or redo. Returns a text for the status line when something needs
    /// to be said.
    pub fn undo(&mut self, session: &mut Session, redo: bool, now_ms: u64) -> Option<String> {
        let method = if redo { "collab.redo" } else { "collab.undo" };
        match session.handle(method, json!({"now_ms": now_ms})) {
            Ok(r) => {
                let skipped = r["skipped"].as_array().map_or(0, Vec::len);
                if skipped > 0 {
                    Some(tr!("collab-ui-undo-skipped", count = skipped))
                } else if r["done"].as_bool() != Some(true) {
                    Some(if redo {
                        tr!("collab-ui-nothing-to-redo")
                    } else {
                        tr!("collab-ui-nothing-to-undo")
                    })
                } else {
                    None
                }
            }
            Err(e) => Some(tr!("collab-ui-error", error = e.message)),
        }
    }

    // ---- drawing ----

    /// The dialogs: Connect, the question about unsaved work, and the wait for the snapshot.
    /// Returns a text for the status line.
    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session, now: u64) -> Option<String> {
        let mut status = None;
        if self.confirm.is_some() {
            status = self.confirm_window(ctx, session, now);
        } else if self.open {
            self.connect_window(ctx, session, now);
        }
        if self.leave.is_some() {
            status = self.leave_window(ctx, session, now).or(status);
        }
        if self.waiting_for_snapshot() && self.joining_modal(ctx) {
            status = Some(self.disconnect(session, now));
        }
        self.wipe_secret(ctx);
        status
    }

    /// The question before waiting edits stay behind: Disconnect or Close.
    fn leave_window(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        now: u64,
    ) -> Option<String> {
        let leave = self.leave?;
        let count = self.view.pending;
        let (title, text, button) = match leave {
            Leave::Disconnect => (
                tr!("collab-ui-leave-disconnect-title"),
                tr!("collab-ui-leave-disconnect-text", count = count),
                tr!("collab-ui-disconnect"),
            ),
            Leave::Close => (
                tr!("collab-ui-leave-close-title"),
                tr!("collab-ui-leave-close-text", count = count),
                tr!("collab-ui-leave-close-button"),
            ),
        };
        let mut answer = None;
        egui::Window::new(title)
            .id(egui::Id::new("win-collab-leave"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.label(text);
                ui.horizontal(|ui| {
                    if ui.button(button).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(tr!("common-cancel")).clicked() {
                        answer = Some(false);
                    }
                });
            });
        self.leave_answer(session, answer?, now)
    }

    /// The answer to the question about leaving.
    pub fn leave_answer(&mut self, session: &mut Session, yes: bool, now: u64) -> Option<String> {
        let leave = self.leave.take()?;
        if !yes {
            return None;
        }
        match leave {
            Leave::Disconnect => Some(self.disconnect(session, now)),
            Leave::Close => {
                self.allow_close = true;
                self.close_now = true;
                None
            }
        }
    }

    fn connect_window(&mut self, ctx: &egui::Context, session: &mut Session, now: u64) {
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        egui::Window::new(tr!("collab-ui-dialog-title"))
            .id(egui::Id::new("win-collab-connect"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("collab-connect-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(tr!("collab-ui-server"));
                        ui.add(egui::TextEdit::singleline(&mut self.form.url).desired_width(300.0));
                        ui.end_row();
                        ui.label(tr!("collab-ui-document"));
                        ui.add(egui::TextEdit::singleline(&mut self.form.doc).desired_width(300.0));
                        ui.end_row();
                        ui.label(tr!("collab-ui-token"));
                        let token = ui.add(
                            egui::TextEdit::singleline(&mut self.form.token)
                                .id(egui::Id::new(TOKEN_FIELD))
                                .password(true)
                                .desired_width(300.0),
                        );
                        if token.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            go = true;
                        }
                        ui.end_row();
                        ui.label(tr!("collab-ui-name"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form.name).desired_width(300.0),
                        );
                        ui.end_row();
                    });
                if self.form.default_token_applies() {
                    ui.weak(tr!("collab-ui-default-token"));
                }
                ui.weak(tr!("collab-ui-token-note"));
                ui.weak(tr!("collab-ui-server-copy-note"));
                if let Some(e) = &self.form_error {
                    ui.colored_label(Color32::from_rgb(200, 50, 50), e);
                }
                ui.horizontal(|ui| {
                    if ui.button(tr!("collab-ui-connect-button")).clicked() {
                        go = true;
                    }
                    if ui.button(tr!("common-cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        let close = cancel || !open || (go && self.submit(session, now));
        self.open = !close;
    }

    /// The Connect button: checks the form and joins. True if the dialog may close (the call
    /// went through, or a question for the person took over).
    fn submit(&mut self, session: &mut Session, now: u64) -> bool {
        let req = match self.form.request() {
            Ok(r) => r,
            Err(e) => {
                self.form_error = Some(e.text());
                return false;
            }
        };
        self.name = self.form.name.clone();
        match self.connect(session, &req, false, false, now) {
            None => {
                // The secret leaves the dialog when the call is settled (`wipe_secret`): not
                // while a question waits, because the answer joins with the same token. A wrong
                // address keeps it, so the person need not type it again.
                self.wipe_token = self.confirm.is_none();
                self.form_error = None;
                true
            }
            Some(e) => {
                self.form_error = Some(e);
                false
            }
        }
    }

    fn confirm_window(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        now: u64,
    ) -> Option<String> {
        let confirm = self.confirm.clone()?;
        let (title, text, button) = match confirm.ask {
            Ask::Replace => (
                tr!("collab-ui-replace-title"),
                tr!("collab-ui-replace-text"),
                tr!("collab-ui-replace-button"),
            ),
            Ask::Discard => (
                tr!("collab-ui-discard-title"),
                tr!("collab-ui-discard-text"),
                tr!("collab-ui-discard-button"),
            ),
        };
        let mut answer = None;
        let mut save_first = false;
        egui::Window::new(title)
            .id(egui::Id::new("win-collab-confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.label(text);
                ui.horizontal(|ui| {
                    if ui.button(button).clicked() {
                        answer = Some(true);
                    }
                    if confirm.ask == Ask::Replace
                        && ui.button(tr!("collab-ui-save-first")).clicked()
                    {
                        save_first = true;
                    }
                    if ui.button(tr!("common-cancel")).clicked() {
                        answer = Some(false);
                    }
                });
            });
        if save_first {
            // The question goes; the window saves the document, and the person connects again.
            self.confirm = None;
            self.save_requested = true;
            self.wipe_token = false;
            return None;
        }
        answer.and_then(|yes| self.answer(session, yes, now))
    }

    /// The answer to a question (`Confirm`): yes joins with `replace` or `discard` (and with
    /// the answers that were given before), no drops the question.
    pub fn answer(&mut self, session: &mut Session, yes: bool, now: u64) -> Option<String> {
        let c = self.confirm.take()?;
        if !yes {
            self.wipe_token = true;
            return None;
        }
        let replace = c.replace || c.ask == Ask::Replace;
        let discard = c.discard || c.ask == Ask::Discard;
        let said = self.connect(session, &c.request, replace, discard, now);
        // A new question keeps the token; anything else settles it.
        self.wipe_token = self.confirm.is_none();
        said
    }

    /// Wipes the token field and its edit state (the undo history of the field holds the old
    /// text) when a call is settled.
    fn wipe_secret(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.wipe_token) {
            self.form.clear_secret();
            ctx.data_mut(|d| {
                d.remove::<egui::text_edit::TextEditState>(egui::Id::new(TOKEN_FIELD))
            });
        }
    }

    /// A box that blocks the window until the shared document has arrived. True if the person
    /// chose to give up.
    fn joining_modal(&self, ctx: &egui::Context) -> bool {
        let mut give_up = false;
        let stalled = self.view.stalled;
        egui::Modal::new(egui::Id::new("collab-joining")).show(ctx, |ui| {
            ui.label(tr!("collab-ui-joining"));
            if stalled {
                ui.colored_label(
                    Color32::from_rgb(200, 140, 0),
                    tr!("uic-collab-note-no-answer"),
                );
            }
            if ui.button(tr!("collab-ui-disconnect")).clicked() {
                give_up = true;
            }
        });
        give_up
    }

    /// The status item for the status bar. Returns what a click asks for.
    pub fn status_item(&self, ui: &mut Ui) -> Option<StatusClick> {
        let color = match self.view.link {
            Link::Connected => Color32::from_rgb(60, 160, 70),
            Link::Connecting | Link::Reconnecting => Color32::from_rgb(200, 140, 0),
            Link::Refused | Link::Failed => Color32::from_rgb(200, 50, 50),
            Link::Offline => ui.visuals().weak_text_color(),
        };
        let text = tr!("collab-ui-status", state = self.view.label());
        let mut tip = self.view.tooltip();
        if let Some(e) = &self.endpoint {
            tip = format!("{e}\n{tip}");
        }
        if let Some(n) = &self.last_notice {
            tip = format!("{tip}\n{}", n.text());
        }
        let r = ui
            .add(egui::Button::new(egui::RichText::new(text).color(color)).frame(false))
            .on_hover_text(tip);
        if r.clicked() {
            return Some(if self.view.is_active() {
                StatusClick::ShowPeople
            } else {
                StatusClick::OpenDialog
            });
        }
        None
    }

    /// The People palette: this user and the others.
    pub fn people_ui(&self, ui: &mut Ui, now: u64) {
        ui.label(tr!("collab-ui-status", state = self.view.label()));
        if let Some(n) = &self.last_notice {
            let color = match n.severity {
                Severity::Info => ui.visuals().text_color(),
                Severity::Warning => Color32::from_rgb(200, 140, 0),
                Severity::Error => Color32::from_rgb(200, 50, 50),
            };
            ui.colored_label(color, n.text());
        }
        ui.separator();
        let name = if self.name.trim().is_empty() {
            tr!("collab-ui-you-unnamed")
        } else {
            tr!("collab-ui-you", name = self.name.trim())
        };
        ui.label(name);
        if self.peers.peers().is_empty() {
            ui.weak(if self.view.link == Link::Connected {
                tr!("collab-ui-nobody")
            } else {
                tr!("collab-ui-not-connected")
            });
            return;
        }
        for p in self.peers.peers() {
            ui.horizontal(|ui| {
                let [r, g, b] = p.rgb();
                let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(swatch, 6.0, Color32::from_rgb(r, g, b));
                let mut line = p.label();
                if PeerBook::is_idle(p, now) {
                    line = tr!("collab-ui-quiet", name = line);
                }
                ui.label(line);
                if !p.selection.is_empty() {
                    ui.weak(tr!("collab-ui-selected", count = p.selection.len()));
                }
            });
        }
    }
}

/// Draws the cursors and the selections of the other users on the canvas. Presence is never
/// part of the document: this only paints.
pub fn paint_peers(
    painter: &egui::Painter,
    rect: Rect,
    view: &View,
    doc: &Document,
    peers: &[Peer],
    now: u64,
) {
    let painter = painter.with_clip_rect(rect);
    for p in peers {
        let [r, g, b] = p.rgb();
        let quiet = PeerBook::is_idle(p, now);
        let alpha = if quiet { 90 } else { 255 };
        let color = Color32::from_rgba_unmultiplied(r, g, b, alpha);
        for id in &p.selection {
            let Some(uid) = Uid::parse(id) else { continue };
            let Some(o) = doc.objects.get(&ObjectId(uid)) else {
                continue;
            };
            let (lo, hi) = doc.object_bounds(o);
            if !(lo.is_finite() && hi.is_finite()) {
                continue;
            }
            let area = Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi)).expand(4.0);
            painter.rect_filled(
                area,
                2.0,
                Color32::from_rgba_unmultiplied(r, g, b, if quiet { 12 } else { 30 }),
            );
            painter.rect_stroke(area, 2.0, Stroke::new(1.5, color), egui::StrokeKind::Middle);
        }
        let Some(c) = p.cursor else { continue };
        let at = view.to_screen(DVec2::new(c[0], c[1]));
        if !rect.expand(40.0).contains(at) {
            continue;
        }
        // A pointer: a small triangle with its tip at the position.
        painter.add(egui::Shape::convex_polygon(
            vec![
                at,
                Pos2::new(at.x, at.y + 15.0),
                Pos2::new(at.x + 10.5, at.y + 10.5),
            ],
            color,
            Stroke::new(1.0, Color32::WHITE),
        ));
        // The name on a small label in the color of the user.
        let galley =
            painter.layout_no_wrap(p.short_label(), FontId::proportional(12.0), Color32::WHITE);
        let pos = Pos2::new(at.x + 12.0, at.y + 14.0);
        let label = Rect::from_min_size(pos, galley.size()).expand2(vec2(4.0, 2.0));
        painter.rect_filled(label, 3.0, color);
        painter.galley(pos, galley, Color32::WHITE);
    }
}

/// The browser build: the address fragment, the warning before the page closes, and (behind the
/// cargo feature `test-hook`) the test hook.
#[cfg(target_arch = "wasm32")]
pub mod web {
    use super::*;
    use kadakas_ui_core::collab::Fragment;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast as _, JsValue};

    thread_local! {
        /// The edits that the server has not confirmed (the page asks before it closes).
        static PENDING: Cell<usize> = const { Cell::new(0) };
    }

    /// Reads `#collab=<ws url>&doc=<id>&token=<t>` and removes the token from the address bar
    /// with `history.replaceState`. The fragment never goes to a server. The token is removed
    /// also when the rest of the fragment is not valid.
    fn consume(win: &web_sys::Window) -> Option<Fragment> {
        let loc = win.location();
        let hash = loc.hash().ok().filter(|h| !h.is_empty())?;
        let parsed = Fragment::parse(&hash);
        let clean = Fragment::without_token(&hash);
        if clean != hash {
            let url = format!(
                "{}{}{}",
                loc.pathname().unwrap_or_default(),
                loc.search().unwrap_or_default(),
                clean
            );
            if let Ok(history) = win.history() {
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url));
            }
        }
        parsed
    }

    /// The fragment of the address at the start of the page.
    pub fn take_fragment() -> Option<Fragment> {
        consume(&web_sys::window()?)
    }

    /// A fragment that is pasted into the address bar of the open page (`hashchange`) is
    /// consumed the same way: the token leaves the address bar at once, and the fragment goes to
    /// the inbox for the window to join.
    pub fn watch_hash(inbox: Rc<RefCell<Vec<Fragment>>>) {
        let Some(win) = web_sys::window() else { return };
        let w = win.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if let Some(f) = consume(&w) {
                inbox.borrow_mut().push(f);
            }
        });
        let _ = win.add_event_listener_with_callback("hashchange", cb.as_ref().unchecked_ref());
        cb.forget();
    }

    /// The window tells the page how many edits wait for the server.
    pub fn set_pending(n: usize) {
        PENDING.with(|p| p.set(n));
    }

    /// Asks the browser to confirm before the page closes while edits wait for the server.
    pub fn warn_before_close() {
        let Some(win) = web_sys::window() else { return };
        let cb = Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(
            |e: web_sys::BeforeUnloadEvent| {
                if PENDING.with(Cell::get) > 0 {
                    e.prevent_default();
                    e.set_return_value("pending");
                }
            },
        );
        let _ = win.add_event_listener_with_callback("beforeunload", cb.as_ref().unchecked_ref());
        cb.forget();
    }

    #[cfg(feature = "test-hook")]
    mod hook {
        use super::*;
        use js_sys::{Array, Reflect};

        thread_local! {
            static HOOK: Cell<bool> = const { Cell::new(false) };
            /// Modules that the page gave to the hook and that are compiled now: (id of the
            /// call, the bytes or the reason that they could not be compiled).
            static MODULES: RefCell<Vec<(String, Result<Vec<u8>, String>)>> =
                const { RefCell::new(Vec::new()) };
        }

        /// The calls that exist only in the test build (the cargo feature `test-hook`). They are
        /// not API methods. `test.plugin_install {b64}` compiles a module (the browser compiles
        /// large modules only asynchronously) and installs it, as the install button does.
        /// `test.plugin_trust {plugin}` trusts the installed module with this id: **the trust
        /// dialog of the app is what a user presses, and a release build has neither this call nor
        /// any other way to trust**.
        fn test_call(session: &mut Session, key: &str, call: &Value) -> Option<Value> {
            use base64::Engine as _;
            match call["method"].as_str()? {
                "test.plugin_install" => {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(call["params"]["b64"].as_str().unwrap_or(""))
                        .map_err(|e| e.to_string());
                    let key = key.to_string();
                    wasm_bindgen_futures::spawn_local(async move {
                        let checked = match bytes {
                            Ok(b) => kadakas_plugin::web::precompile(
                                &b,
                                &kadakas_plugin::runtime::default_limits(),
                            )
                            .await
                            .map(|()| b)
                            .map_err(|e| e.to_string()),
                            Err(e) => Err(e),
                        };
                        MODULES.with(|m| m.borrow_mut().push((key, checked)));
                    });
                    None
                }
                "test.plugin_trust" => {
                    let id = call["params"]["plugin"].as_str().unwrap_or("");
                    Some(match kadakas_api::review_wasm_plugin(session, id) {
                        Some(r) => {
                            let hash = r["hash"].as_str().unwrap_or("").to_string();
                            match kadakas_api::trust_wasm_plugin(session, id, &hash) {
                                Ok(()) => json!({"ok": true}),
                                Err(e) => json!({"error": e}),
                            }
                        }
                        None => json!({"error": "no such plug-in"}),
                    })
                }
                _ => None,
            }
        }

        pub fn install() {
            let Some(win) = web_sys::window() else { return };
            let on = win.location().search().is_ok_and(|s| {
                s.trim_start_matches('?')
                    .split('&')
                    .any(|p| p == "kadakas_test=1")
            });
            if !on {
                return;
            }
            HOOK.with(|h| h.set(true));
            let hook = js_sys::Object::new();
            let _ = Reflect::set(&hook, &"cmds".into(), &Array::new());
            let _ = Reflect::set(&hook, &"results".into(), &js_sys::Object::new());
            let _ = Reflect::set(&hook, &"state".into(), &"{}".into());
            // The names of the test-only calls, as text that the page can read. It also keeps the
            // names in the data of the build, where `scripts/check-test-hook.sh` of the
            // kadakas-server repository looks for them: a release build must not have them.
            let _ = Reflect::set(
                &hook,
                &"extra_calls".into(),
                &"test.plugin_install test.plugin_trust".into(),
            );
            let _ = Reflect::set(&win, &"kadakasTest".into(), &hook);
        }

        pub fn frame(session: &mut Session, collab: &CollabUi, canvas: Option<Rect>) {
            if !HOOK.with(Cell::get) {
                return;
            }
            let Some(win) = web_sys::window() else { return };
            let Ok(hook) = Reflect::get(&win, &"kadakasTest".into()) else {
                return;
            };
            if let Ok(cmds) = Reflect::get(&hook, &"cmds".into())
                && let Ok(cmds) = cmds.dyn_into::<Array>()
            {
                let results = Reflect::get(&hook, &"results".into()).unwrap_or(JsValue::NULL);
                while cmds.length() > 0 {
                    let text = cmds.shift().as_string().unwrap_or_default();
                    let call: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    let key = call["id"].to_string();
                    if call["method"].as_str() == Some("test.plugin_install") {
                        test_call(session, &key, &call);
                        continue;
                    }
                    let answer = match call["method"].as_str() {
                        Some(m) if m.starts_with("test.") => test_call(session, &key, &call)
                            .unwrap_or_else(|| json!({"error": "no such test call"})),
                        Some(m) => match session.handle(m, call["params"].clone()) {
                            Ok(v) => json!({"ok": v}),
                            Err(e) => json!({"error": e.message, "kind": e.kind}),
                        },
                        None => json!({"error": "no method"}),
                    };
                    let _ = Reflect::set(&results, &key.into(), &answer.to_string().into());
                }
                // Modules that are compiled now are installed, and the page gets the answer.
                let ready: Vec<_> = MODULES.with(|m| m.borrow_mut().drain(..).collect());
                for (key, bytes) in ready {
                    let answer = match bytes {
                        Ok(b) => match kadakas_api::install_wasm_plugin(session, &b) {
                            Ok(v) => json!({"ok": v}),
                            Err(e) => json!({"error": e.message, "kind": e.kind}),
                        },
                        Err(e) => json!({"error": e, "kind": "invalid"}),
                    };
                    let _ = Reflect::set(&results, &key.into(), &answer.to_string().into());
                }
            }
            let state = json!({
                "objects": session.editor.doc().objects.len(),
                "link": collab.view().link.id(),
                "waiting": collab.waiting_for_snapshot(),
                "stalled": collab.view().stalled,
                "label": collab.view().label(),
                // The drawing area in screen pixels (points of the window), so a test can
                // aim at it without fixed coordinates.
                "canvas": canvas.map(|r| json!({"x": r.min.x, "y": r.min.y, "w": r.width(), "h": r.height()})),
                "peers": collab.peers().iter().map(|p| json!({
                    "name": p.label(), "cursor": p.cursor, "selected": p.selection.len(),
                })).collect::<Vec<_>>(),
            });
            let _ = Reflect::set(&hook, &"state".into(), &state.to_string().into());
        }
    }

    /// The test hook exists only in a build with the cargo feature `test-hook`, and then only
    /// when the address of the page has the query `kadakas_test=1` at the start. A page can run
    /// API calls and read the state of the window through `window.kadakasTest`:
    /// `cmds.push(JSON.stringify({id, method, params}))`, `results[id]` (a JSON text) and `state`
    /// (a JSON text that the window renews in each frame). `scripts/test-browser.sh` of the
    /// `kadakas-server` repository builds with the feature.
    pub fn install_test_hook() {
        #[cfg(feature = "test-hook")]
        hook::install();
    }

    /// One frame of the test hook: runs the waiting calls and publishes the state.
    pub fn test_hook(session: &mut Session, collab: &CollabUi, canvas: Option<Rect>) {
        #[cfg(feature = "test-hook")]
        hook::frame(session, collab, canvas);
        #[cfg(not(feature = "test-hook"))]
        let _ = (session, collab, canvas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_collab::docsync;
    use kadakas_collab::protocol::ErrorCode;
    use kadakas_collab::testkit::{Hub, Sequencer};
    use kadakas_collab_ws::testserver::TestServer;

    const DOC: &str = "0190f1c2-7a3b-7c1d-8e2f-123456789abc";

    fn call(s: &mut Session, method: &str, p: Value) -> Value {
        s.handle(method, p)
            .unwrap_or_else(|e| panic!("{method} failed: {}", e.message))
    }

    fn add_rect(s: &mut Session, x: i64) -> String {
        let out = call(
            s,
            "execute",
            json!({"command": {"op": "add_object",
                "shape": {"kind": "rect", "min": [x, 0], "max": [x + 1000, 500]}}}),
        );
        out["created"][0].as_str().unwrap().to_string()
    }

    fn count(s: &Session) -> usize {
        s.editor.doc().objects.len()
    }

    fn nothing() -> Own {
        Own {
            cursor: None,
            selection: Vec::new(),
            viewport: None,
        }
    }

    fn anon(name: &str) -> ConnectRequest {
        ConnectRequest {
            url: None,
            doc: None,
            token: None,
            user: None,
            name: Some(name.into()),
        }
    }

    /// Two windows (a session and a `CollabUi` each) on one in-memory hub.
    struct Pair {
        hub: Hub,
        a: Session,
        ua: CollabUi,
        b: Session,
        ub: CollabUi,
        t: u64,
    }

    impl Pair {
        fn new() -> Pair {
            let mut a = Session::default();
            let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
            a.set_collab_endpoint(Box::new(hub.endpoint()));
            let mut b = Session::default();
            b.set_collab_endpoint(Box::new(hub.endpoint()));
            let mut p = Pair {
                hub,
                a,
                ua: CollabUi::new(Defaults::default()),
                b,
                ub: CollabUi::new(Defaults::default()),
                t: 1_000,
            };
            assert_eq!(
                p.ua.connect(&mut p.a, &anon("Ann"), false, false, 1_000),
                None
            );
            assert_eq!(
                p.ub.connect(&mut p.b, &anon("Bo"), false, false, 1_000),
                None
            );
            p
        }

        /// A few rounds for both windows. Each round moves the clock by 100 ms.
        fn rounds(&mut self, own_a: &Own, own_b: &Own) -> (Outcome, Outcome) {
            let mut last = (Outcome::default(), Outcome::default());
            for _ in 0..4 {
                self.t += TICK_MS;
                self.hub.set_time(self.t);
                last = (
                    self.ua.tick(&mut self.a, self.t, own_a),
                    self.ub.tick(&mut self.b, self.t, own_b),
                );
            }
            last
        }
    }

    #[test]
    fn edits_presence_and_per_user_undo_between_two_windows() {
        let mut p = Pair::new();
        p.rounds(&nothing(), &nothing());
        assert_eq!(p.ua.view().link, Link::Connected);
        assert!(p.ua.is_connected() && !p.ua.waiting_for_snapshot());

        // An edit of Ann shows in the window of Bo.
        add_rect(&mut p.a, 0);
        p.rounds(&nothing(), &nothing());
        assert_eq!((count(&p.a), count(&p.b)), (1, 1));

        // Presence: Bo moves the pointer and selects the rectangle; Ann sees it, not in the
        // document.
        let id = p.b.editor.doc().objects.keys().next().unwrap().to_string();
        let own_b = Own {
            cursor: Some([120.0, 40.0]),
            selection: vec![id.clone()],
            viewport: Some(([0.0, 0.0], 0.5, [800.0, 600.0])),
        };
        p.rounds(&nothing(), &own_b);
        let peers = p.ua.peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].label(), "Bo");
        assert_eq!(peers[0].cursor, Some([120.0, 40.0]));
        assert_eq!(peers[0].selection, [id]);
        assert_eq!(p.ua.view().peers, 1);
        assert_eq!(count(&p.a), 1, "presence is not an object");

        // Both draw; Ann undoes. Only her rectangle goes.
        add_rect(&mut p.b, 5000);
        p.rounds(&nothing(), &nothing());
        assert_eq!((count(&p.a), count(&p.b)), (2, 2));
        add_rect(&mut p.a, 9000);
        p.rounds(&nothing(), &nothing());
        assert_eq!(p.ua.route(&mut p.a), UndoRoute::Collab);
        assert!(p.ua.can_undo(false));
        assert_eq!(p.ua.undo(&mut p.a, false, p.t + 1), None);
        p.rounds(&nothing(), &nothing());
        assert_eq!((count(&p.a), count(&p.b)), (2, 2));
        // Redo brings it back; nothing is left to redo after that.
        assert_eq!(p.ua.undo(&mut p.a, true, p.t + 1), None);
        p.rounds(&nothing(), &nothing());
        assert_eq!((count(&p.a), count(&p.b)), (3, 3));
        assert!(
            p.ua.undo(&mut p.a, true, p.t + 2).is_some(),
            "nothing to redo"
        );

        // Bo's window leaves: the link is offline and the normal undo is used again.
        let said = p.ub.disconnect(&mut p.b, p.t);
        assert!(said.contains("Disconnected"));
        assert_eq!(p.ub.view().link, Link::Offline);
        // The client is still there (offline), and holds the undo steps of the person.
        assert_eq!(p.ub.route(&mut p.b), UndoRoute::Collab);
        assert!(p.ub.peers().is_empty());
        p.rounds(&nothing(), &nothing());
        assert!(p.ua.peers().is_empty(), "the person who left is gone");
    }

    #[test]
    fn a_new_window_takes_the_server_copy_and_asks_before_it_replaces_unsaved_work() {
        let mut p = Pair::new();
        add_rect(&mut p.a, 0);
        p.rounds(&nothing(), &nothing());
        // A third window with unsaved drawing.
        let mut c = Session::default();
        c.set_collab_endpoint(Box::new(p.hub.endpoint()));
        add_rect(&mut c, 7000);
        add_rect(&mut c, 8000);
        let mut uc = CollabUi::new(Defaults::default());
        assert_eq!(uc.connect(&mut c, &anon("Cy"), false, false, 2_000), None);
        assert_eq!(
            uc.confirm(),
            Some(&Confirm {
                ask: Ask::Replace,
                request: anon("Cy"),
                replace: false,
                discard: false
            })
        );
        assert!(!uc.is_connected(), "nothing happened yet");
        assert_eq!(count(&c), 2);
        // No: the question goes, the drawing stays.
        assert_eq!(uc.answer(&mut c, false, 2_000), None);
        assert!(uc.confirm().is_none() && count(&c) == 2);
        // Yes: the server copy replaces it.
        assert_eq!(uc.connect(&mut c, &anon("Cy"), false, false, 2_000), None);
        assert_eq!(uc.answer(&mut c, true, 2_100), None);
        let own = nothing();
        for i in 0..4 {
            uc.tick(&mut c, 2_200 + i * TICK_MS, &own);
        }
        assert_eq!(count(&c), 1);
        assert!(uc.is_connected());
        // A second answer has nothing to answer.
        assert_eq!(uc.answer(&mut c, true, 3_000), None);
    }

    #[test]
    fn a_saved_document_joins_without_a_question() {
        let mut p = Pair::new();
        let mut c = Session::default();
        c.set_collab_endpoint(Box::new(p.hub.endpoint()));
        add_rect(&mut c, 7000);
        c.collab_mark_saved();
        let mut uc = CollabUi::new(Defaults::default());
        assert_eq!(uc.connect(&mut c, &anon("Cy"), false, false, 2_000), None);
        assert!(uc.confirm().is_none());
        p.rounds(&nothing(), &nothing());
    }

    #[test]
    fn a_failed_connect_says_why_and_a_second_connect_is_refused() {
        let mut s = Session::default();
        let mut ui = CollabUi::new(Defaults::default());
        let said = ui.connect(&mut s, &anon("Ann"), false, false, 1).unwrap();
        assert!(said.starts_with("Could not connect:"), "{said}");
        assert!(!ui.is_connected());
        // A tick on a window that is not connected does nothing and asks the API once in a while.
        assert_eq!(ui.tick(&mut s, 100, &nothing()), Outcome::default());
        let hub = Hub::new(Sequencer::new(docsync::replica_of(&Document::default())));
        s.set_collab_endpoint(Box::new(hub.endpoint()));
        assert_eq!(ui.connect(&mut s, &anon("Ann"), false, false, 200), None);
        let again = ui.connect(&mut s, &anon("Ann"), false, false, 300).unwrap();
        assert!(again.contains("Already connected"), "{again}");
    }

    #[test]
    fn an_old_token_is_told_before_any_call() {
        // {"exp":1000}
        let mut s = Session::default();
        let mut ui = CollabUi::new(Defaults::default());
        let mut req = anon("Ann");
        req.token = Some("e30.eyJleHAiOjEwMDB9.c2ln".into());
        let said = ui.connect(&mut s, &req, false, false, 5_000_000).unwrap();
        assert!(said.contains("run out"), "{said}");
        assert!(!ui.is_connected());
        // A token that is still good goes to the API (which has no endpoint here).
        let said = ui.connect(&mut s, &req, false, false, 500_000).unwrap();
        assert!(said.starts_with("Could not connect"), "{said}");
    }

    #[test]
    fn a_session_that_an_agent_joined_is_followed() {
        let mut p = Pair::new();
        let mut ui = CollabUi::new(Defaults::default());
        // `p.a` is joined by the window `p.ua`, but `ui` has never heard of it.
        ui.tick(&mut p.a, 5_000, &nothing());
        assert!(ui.is_connected() || ui.view().link != Link::Offline);
        ui.tick(&mut p.a, 5_000 + 2 * TICK_MS, &nothing());
        assert_eq!(ui.view().link, Link::Connected);
        // A tick that comes too early does nothing.
        let before = ui.last_tick_ms;
        ui.tick(&mut p.a, 5_000 + 2 * TICK_MS + 1, &nothing());
        assert_eq!(ui.last_tick_ms, before);
    }

    #[test]
    fn a_refused_edit_is_told_in_words() {
        let mut p = Pair::new();
        p.rounds(&nothing(), &nothing());
        p.hub.with(|s| s.inject_errors(ErrorCode::DocumentFull, 1));
        add_rect(&mut p.a, 0);
        let mut said = String::new();
        for i in 0..6 {
            let o = p.ua.tick(&mut p.a, p.t + (i + 1) * TICK_MS, &nothing());
            if let Some(s) = o.status {
                said = s;
            }
            p.hub.set_time(p.t + (i + 1) * TICK_MS);
        }
        assert!(said.contains("full"), "{said:?}");
        assert_eq!(count(&p.a), 0, "the refused edit was taken back");
    }

    #[test]
    fn the_connect_form_goes_through_the_same_call_and_wipes_the_token() {
        let mut p = Pair::new();
        let mut ui = CollabUi::new(Defaults {
            url: None,
            has_token: false,
            user: None,
            name: "Dee".into(),
        });
        // An empty form shows what is missing and stays open.
        assert!(!ui.submit(&mut p.a, 1_000));
        assert!(ui.form_error.as_deref().unwrap().contains("address"));
        ui.form.url = "ws://127.0.0.1:1".into();
        assert!(!ui.submit(&mut p.a, 1_000));
        assert!(ui.form_error.as_deref().unwrap().contains("document ID"));
        // A good form: the session is joined already, so the call is refused and the token
        // stays in the field for another try.
        ui.form.doc = DOC.into();
        ui.form.token = "secret".into();
        assert!(!ui.submit(&mut p.a, 1_000));
        assert!(
            ui.form_error
                .as_deref()
                .unwrap()
                .contains("Already connected")
        );
        assert_eq!(ui.form.token, "secret");
        // A session that is free: the call goes through and the field is wiped.
        let mut free = Session::default();
        let hub = Hub::new(Sequencer::new(docsync::replica_of(&Document::default())));
        free.set_collab_endpoint(Box::new(hub.endpoint()));
        free.set_collab_defaults(Some("ws://127.0.0.1:1".into()), None);
        free.set_collab_endpoint_factory(Box::new(|url, token| {
            kadakas_collab_ws::endpoint_for(url, token).map_err(|e| e.to_string())
        }));
        // The token of a Replace question stays until the question is answered.
        assert!(ui.submit(&mut free, 1_000));
        assert!(ui.wipe_token && ui.form_error.is_none());
        // The wipe happens in the next frame, and takes the edit state of the field with it.
        let ctx = egui::Context::default();
        let id = egui::Id::new(TOKEN_FIELD);
        ctx.data_mut(|d| d.insert_persisted(id, egui::text_edit::TextEditState::default()));
        ui.wipe_secret(&ctx);
        assert!(ui.form.token.is_empty());
        assert!(
            ctx.data_mut(|d| d.get_persisted::<egui::text_edit::TextEditState>(id))
                .is_none()
        );
        assert!(ui.is_connected());
        assert_eq!(ui.name, "Dee");
    }

    /// Real sockets: the window joins by URL with a token, and a wrong token ends as `Refused`.
    #[test]
    fn a_wrong_token_ends_as_refused_and_the_right_one_connects() {
        let srv = TestServer::start_with(Sequencer::new(docsync::replica_of(&Document::default())));
        srv.require_token("good", Uid::from_u128(7));
        let factory = || -> Session {
            let mut s = Session::default();
            s.set_collab_endpoint_factory(Box::new(|url, token| {
                kadakas_collab_ws::endpoint_for(url, token).map_err(|e| e.to_string())
            }));
            s
        };
        let request = |token: &str| ConnectRequest {
            url: Some(srv.base_url()),
            doc: Some(DOC.into()),
            token: Some(token.into()),
            user: Some(Uid::from_u128(7).to_string()),
            name: Some("Ann".into()),
        };
        let wait = |s: &mut Session, ui: &mut CollabUi, want: Link| {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut t = now_ms();
            while ui.view().link != want {
                assert!(std::time::Instant::now() < end, "timeout: {want:?}");
                std::thread::sleep(std::time::Duration::from_millis(10));
                t += TICK_MS;
                ui.tick(s, t, &nothing());
            }
        };
        let (mut bad, mut ui_bad) = (factory(), CollabUi::new(Defaults::default()));
        assert_eq!(
            ui_bad.connect(&mut bad, &request("wrong"), false, false, now_ms()),
            None
        );
        wait(&mut bad, &mut ui_bad, Link::Refused);
        assert!(
            ui_bad
                .last_notice
                .as_ref()
                .is_some_and(|n| n.text().contains("refused the token"))
        );
        let (mut good, mut ui_good) = (factory(), CollabUi::new(Defaults::default()));
        assert_eq!(
            ui_good.connect(&mut good, &request("good"), false, false, now_ms()),
            None
        );
        wait(&mut good, &mut ui_good, Link::Connected);
        assert!(
            ui_good
                .endpoint
                .as_deref()
                .is_some_and(|e| !e.contains("good"))
        );
    }

    fn question(ask: Ask) -> Confirm {
        Confirm {
            ask,
            request: anon("Ann"),
            replace: false,
            discard: false,
        }
    }

    fn run_ui(f: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), f)
    }

    #[test]
    fn the_status_item_and_the_people_list_draw_in_every_state() {
        let mut p = Pair::new();
        let own_b = Own {
            cursor: Some([1.0, 2.0]),
            selection: vec!["x".into(), "y".into()],
            viewport: None,
        };
        p.rounds(&nothing(), &own_b);
        let offline = CollabUi::new(Defaults::default());
        for language in ["en-US", "et"] {
            kadakas_l10n::with_language(language, || {
                for ui_state in [&offline, &p.ua] {
                    let out = run_ui(|ui| {
                        assert_eq!(ui_state.status_item(ui), None);
                        ui_state.people_ui(ui, 100_000);
                    });
                    assert!(!out.shapes.is_empty());
                }
            });
        }
        // Each state has a text.
        for link in [
            Link::Offline,
            Link::Connecting,
            Link::Connected,
            Link::Reconnecting,
            Link::Refused,
            Link::Failed,
        ] {
            let mut ui = CollabUi::new(Defaults::default());
            ui.view.link = link;
            ui.last_notice = Some(Notice {
                id: "uic-collab-err-auth",
                args: Vec::new(),
                severity: Severity::Error,
            });
            let _ = run_ui(|u| {
                ui.status_item(u);
                ui.people_ui(u, 0);
            });
        }
    }

    #[test]
    fn the_dialogs_draw() {
        let mut p = Pair::new();
        let ctx = egui::Context::default();
        let mut ui = CollabUi::new(Defaults::default());
        ui.open_dialog();
        assert!(ui.is_open());
        let _ = ctx.run_ui(Default::default(), |u| {
            assert_eq!(ui.show(u.ctx(), &mut p.a, 1_000), None);
        });
        // The question about unsaved work.
        ui.confirm = Some(question(Ask::Replace));
        let _ = ctx.run_ui(Default::default(), |u| {
            ui.show(u.ctx(), &mut p.a, 1_000);
        });
        ui.confirm = Some(question(Ask::Discard));
        let _ = ctx.run_ui(Default::default(), |u| {
            ui.show(u.ctx(), &mut p.a, 1_000);
        });
        // The wait for the snapshot blocks the window.
        ui.confirm = None;
        ui.view.waiting_for_snapshot = true;
        let _ = ctx.run_ui(Default::default(), |u| {
            ui.show(u.ctx(), &mut p.a, 1_000);
        });
    }

    #[test]
    fn the_others_are_painted_on_the_canvas() {
        let mut p = Pair::new();
        let id = add_rect(&mut p.a, 0);
        let own_b = Own {
            cursor: Some([100.0, 100.0]),
            selection: vec![id, Uid::from_u128(99).to_string()],
            viewport: None,
        };
        p.rounds(&nothing(), &own_b);
        p.rounds(&nothing(), &own_b);
        let mut peers: Vec<Peer> = p.ua.peers().to_vec();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].cursor, Some([100.0, 100.0]));
        // An id that is not an object, and text that is not an id, are skipped.
        peers[0].selection.push("not-an-id".into());
        let view = View::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let paint = |peers: &[Peer], now: u64| {
            run_ui(|ui| {
                paint_peers(
                    &ui.painter().clone(),
                    rect,
                    &view,
                    p.a.editor.doc(),
                    peers,
                    now,
                )
            })
            .shapes
            .len()
        };
        let none = paint(&[], 0);
        let seen = paint(&peers, 0);
        assert!(seen > none, "{seen} shapes against {none}");
        // A quiet user is still drawn, dimmed. A pointer far outside the canvas is not drawn.
        assert!(paint(&peers, 10 * 60_000) > none);
        let mut far = peers.clone();
        far[0].cursor = Some([1e9, 1e9]);
        far[0].selection.clear();
        assert_eq!(paint(&far, 0), none);
        far[0].cursor = None;
        assert_eq!(paint(&far, 0), none);
    }

    #[test]
    fn a_question_keeps_the_token_and_the_answers_that_were_given() {
        let hub = Hub::new(Sequencer::new(docsync::replica_of(&Document::default())));
        let mut s = Session::default();
        s.set_collab_endpoint(Box::new(hub.endpoint()));
        add_rect(&mut s, 0);
        let mut ui = CollabUi::new(Defaults::default());
        ui.form.url = "ws://127.0.0.1:1".into();
        ui.form.doc = DOC.into();
        ui.form.token = "secret".into();
        // The person already agreed to drop waiting edits; the open document is unsaved too.
        let mut req = anon("Ann");
        req.token = Some("secret".into());
        assert_eq!(ui.connect(&mut s, &req, false, true, 1_000), None);
        let q = ui.confirm().cloned().expect("a question");
        assert_eq!((q.ask, q.replace, q.discard), (Ask::Replace, false, true));
        assert_eq!(q.request.token.as_deref(), Some("secret"));
        // While the question waits, the token is not wiped.
        ui.wipe_token = false;
        assert_eq!(ui.form.token, "secret");
        // Yes: the call carries both answers, and the token is wiped after the answer.
        assert_eq!(ui.answer(&mut s, true, 2_000), None);
        assert!(ui.is_connected() && ui.wipe_token);
        // No: the question goes and the token is wiped too.
        let mut ui = CollabUi::new(Defaults::default());
        ui.confirm = Some(question(Ask::Discard));
        assert_eq!(ui.answer(&mut Session::default(), false, 1), None);
        assert!(ui.wipe_token && ui.confirm().is_none());
    }

    #[test]
    fn leaving_with_waiting_edits_asks_first() {
        let mut p = Pair::new();
        p.rounds(&nothing(), &nothing());
        // Nothing waits: Disconnect and Close go at once.
        assert!(p.ua.ask_close());
        // Edits wait for the server.
        p.ua.view.pending = 3;
        assert_eq!(p.ua.pending(), 3);
        assert!(!p.ua.ask_close());
        assert_eq!(p.ua.leave, Some(Leave::Close));
        let ctx = egui::Context::default();
        for language in ["en-US", "et"] {
            kadakas_l10n::with_language(language, || {
                let _ = ctx.run_ui(Default::default(), |u| {
                    p.ua.show(u.ctx(), &mut p.a, 1_000);
                });
            });
        }
        // No: nothing changes. Yes: the window may close, and does not ask again.
        assert_eq!(p.ua.leave_answer(&mut p.a, false, 1_000), None);
        assert!(!p.ua.take_close_now());
        assert!(!p.ua.ask_close());
        assert_eq!(p.ua.leave_answer(&mut p.a, true, 1_000), None);
        assert!(p.ua.take_close_now() && !p.ua.take_close_now());
        assert!(p.ua.ask_close());
        // Disconnect asks the same way.
        assert_eq!(p.ua.ask_disconnect(&mut p.a, 1_000), None);
        assert_eq!(p.ua.leave, Some(Leave::Disconnect));
        let said = p.ua.leave_answer(&mut p.a, true, 1_000).unwrap();
        assert!(said.contains("Disconnected"));
        assert_eq!(p.ua.pending(), 0);
        assert_eq!(
            p.ua.leave_answer(&mut p.a, true, 1_000),
            None,
            "no question left"
        );
        // Without edits waiting, Disconnect goes at once.
        assert!(p.ub.ask_disconnect(&mut p.b, 1_000).is_some());
    }

    /// The wait between two tries of a first connection (a browser with a wrong token, a server
    /// that is down) is still "joining": no edit, no New, Open or recovery, and Disconnect works.
    #[test]
    fn the_wait_between_two_tries_still_blocks_the_window() {
        let mut srv =
            TestServer::start_with(Sequencer::new(docsync::replica_of(&Document::default())));
        srv.stop();
        let mut s = Session::default();
        s.set_collab_endpoint_factory(Box::new(|url, token| {
            kadakas_collab_ws::endpoint_for(url, token).map_err(|e| e.to_string())
        }));
        let mut ui = CollabUi::new(Defaults::default());
        let req = ConnectRequest {
            url: Some(srv.base_url()),
            doc: Some(DOC.into()),
            token: None,
            user: Some(Uid::from_u128(7).to_string()),
            name: Some("Ann".into()),
        };
        assert_eq!(ui.connect(&mut s, &req, false, false, now_ms()), None);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut t = now_ms();
        loop {
            t += TICK_MS;
            ui.tick(&mut s, t, &nothing());
            let st = s.handle("collab.status", Value::Null).unwrap();
            if st["state"] == "offline" && st["last_error"].is_string() {
                break;
            }
            assert!(
                std::time::Instant::now() < end,
                "the first try never failed"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Between two tries: waiting, active, linked, and an edit is refused by the API.
        assert!(ui.waiting_for_snapshot() && ui.is_active() && ui.is_linked());
        assert!(
            !ui.is_connected(),
            "Connect stays possible, for a new token"
        );
        assert_eq!(ui.route(&mut s), UndoRoute::Collab);
        let e = s
            .handle(
                "execute",
                json!({"command": {"op": "add_object",
                    "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}}),
            )
            .expect_err("refused");
        assert_eq!(e.kind, "conflict");
        // The stall note shows in the box.
        let _ = run_ui(|u| {
            assert!(!ui.joining_modal(u.ctx()));
        });
        let said = ui.disconnect(&mut s, t);
        assert!(said.contains("Disconnected"));
        assert!(!ui.is_active() && !ui.waiting_for_snapshot());
    }

    #[test]
    fn the_clock_gives_milliseconds() {
        assert!(now_ms() > 1_600_000_000_000);
    }
}
