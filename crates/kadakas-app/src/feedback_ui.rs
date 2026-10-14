//! Feedback in the window (UI-14): the Feedback button of the status bar, the report form, the
//! window Help > Feedback (the outbox), the review of a report with attachments, and the consent
//! dialog. The logic is in `kadakas_ui_core::feedback` and `kadakas_api::feedback_api::review`;
//! this file draws it and talks to the window (screenshots, repaints).
//!
//! The reports of agents and of people go to the same outbox (`Session::feedback.hub`). A report
//! with attachments waits as `needs_review`. The badge on the Feedback button counts them, and
//! the window opens a review with one click. Nothing with attachments is sent before the person
//! presses Send in the review.

use kadakas_api::Session;
use kadakas_api::feedback_api::review::{Review, display_json};
use kadakas_feedback::{
    AttachmentKind, Hub, Kind, MAX_ATTEMPTS, MAX_BODY, MAX_IMAGE_BYTES, MAX_TITLE, Origin, Report,
    Status,
};
use kadakas_l10n::tr;
use kadakas_ui_core::feedback::Flow;
pub use kadakas_ui_core::feedback::{Form, Open, app_context, is_error_status};

/// The reports are read again at most this often, in seconds.
const REFRESH: f64 = 2.0;
/// The widest picture of the window that a form keeps.
const SHOT_WIDTHS: [u32; 4] = [1280, 960, 640, 480];
/// The keys of the screenshots of this module start here, far from the keys of agents.
const SHOT_KEY_BASE: u64 = 1 << 62;

/// The seconds since 1970, for the date of a report.
fn unix_now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as u64
    }
}

fn texture(ctx: &egui::Context, name: &str, png: &[u8]) -> Option<egui::TextureHandle> {
    let img = kadakas_render::texture::decode_png_limited(png, 16_000_000).ok()?;
    let ci = egui::ColorImage::from_rgba_unmultiplied(
        [img.width as usize, img.height as usize],
        &img.rgba,
    );
    Some(ctx.load_texture(name, ci, egui::TextureOptions::LINEAR))
}

fn show_texture(ui: &mut egui::Ui, t: &egui::TextureHandle, max_w: f32) {
    let s = t.size_vec2();
    let k = (max_w / s.x).min(1.0);
    ui.image((t.id(), s * k));
}

pub fn kind_name(k: Kind) -> String {
    match k {
        Kind::Bug => tr!("feedback-ui-kind-bug"),
        Kind::FeatureRequest => tr!("feedback-ui-kind-feature-request"),
        Kind::Question => tr!("feedback-ui-kind-question"),
    }
}

pub fn status_name(s: Status) -> String {
    match s {
        Status::Pending => tr!("feedback-ui-status-pending"),
        Status::NeedsReview => tr!("feedback-ui-status-needs-review"),
        Status::Approved => tr!("feedback-ui-status-approved"),
        Status::Sent => tr!("feedback-ui-status-sent"),
        Status::Withdrawn => tr!("feedback-ui-status-withdrawn"),
    }
}

fn origin_name(o: Origin) -> String {
    match o {
        Origin::Agent => tr!("feedback-ui-from-agent"),
        Origin::User => tr!("feedback-ui-from-you"),
        Origin::Crash => tr!("feedback-ui-from-crash"),
    }
}

/// What the button of the status bar asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// The button was clicked.
    Button,
}

/// The review dialog of one report.
struct ReviewState {
    review: Review,
    shots: Vec<(AttachmentKind, egui::TextureHandle)>,
    /// The message as text, made again when a tick or an option changes.
    json: String,
    last: Option<(Vec<(AttachmentKind, bool)>, kadakas_feedback::Privacy)>,
}

/// A form that waits for the picture of the window.
struct OpenForm {
    form: Form,
    texture: Option<egui::TextureHandle>,
    /// The key of the screenshot that was asked for, until it arrives.
    waiting: Option<u64>,
}

pub struct FeedbackUi {
    form: Option<OpenForm>,
    window: bool,
    review: Option<ReviewState>,
    flow: Flow,
    reports: Vec<Report>,
    asked: bool,
    send_on: bool,
    /// The outbox file is from a newer Kadakas: reports are in memory only.
    newer_outbox: bool,
    url: String,
    url_edit: Option<String>,
    next_refresh: f64,
    /// The result of the last action, shown in the window.
    note: String,
    shot_next: u64,
    #[cfg(not(target_arch = "wasm32"))]
    dispatcher: Option<kadakas_feedback::dispatch::Dispatcher>,
}

impl Default for FeedbackUi {
    fn default() -> Self {
        FeedbackUi {
            form: None,
            window: false,
            review: None,
            flow: Flow::default(),
            reports: Vec::new(),
            asked: true,
            send_on: false,
            newer_outbox: false,
            url: String::new(),
            url_edit: None,
            next_refresh: 0.0,
            note: String::new(),
            shot_next: SHOT_KEY_BASE,
            #[cfg(not(target_arch = "wasm32"))]
            dispatcher: None,
        }
    }
}

impl FeedbackUi {
    /// Turns the sending thread on (the real transport). Without it, nothing leaves the program.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_sending(&mut self) {
        self.dispatcher = Some(kadakas_feedback::dispatch::Dispatcher::http());
    }

    /// Turns sending on with another transport. For tests.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub fn set_dispatcher(&mut self, d: kadakas_feedback::dispatch::Dispatcher) {
        self.dispatcher = Some(d);
    }

    /// Opens what a Help command asks for.
    pub fn open(&mut self, what: Open) {
        match what {
            Open::ReportBug => self.open_form(Form::new(Kind::Bug)),
            Open::RequestFeature => self.open_form(Form::new(Kind::FeatureRequest)),
            Open::Window => {
                self.window = true;
                self.next_refresh = 0.0;
            }
        }
    }

    /// Opens the form with this content. The window takes its picture first, so that the
    /// picture shows the program and not the form.
    pub fn open_form(&mut self, form: Form) {
        let key = self.next_key();
        self.form = Some(OpenForm {
            form,
            texture: None,
            waiting: cfg!(not(target_arch = "wasm32")).then_some(key),
        });
        self.note.clear();
    }

    fn next_key(&mut self) -> u64 {
        self.shot_next += 1;
        self.shot_next
    }

    #[cfg(test)]
    pub fn is_form_open(&self) -> bool {
        self.form.is_some()
    }

    #[cfg(test)]
    pub fn form(&self) -> Option<&Form> {
        self.form.as_ref().map(|f| &f.form)
    }

    #[cfg(test)]
    pub fn is_window_open(&self) -> bool {
        self.window
    }

    /// Reports that wait for a review.
    pub fn badge(&self) -> usize {
        self.reports
            .iter()
            .filter(|r| r.status == Status::NeedsReview)
            .count()
    }

    /// The Feedback button of the status bar. With reports to review it opens the window, else the form.
    pub fn status_item(&self, ui: &mut egui::Ui) -> Option<Click> {
        let n = self.badge();
        let label = if n > 0 {
            tr!("feedback-ui-feedback-waiting", count = n)
        } else {
            tr!("feedback-ui-feedback")
        };
        let mut r = ui.small_button(label);
        if n > 0 {
            r = r.on_hover_text(tr!("feedback-ui-reports-wait-for-your-review"));
        }
        r.clicked().then_some(Click::Button)
    }

    /// What the button does.
    pub fn on_click(&mut self, _c: Click) {
        if self.badge() > 0 {
            self.open(Open::Window);
        } else {
            self.open_form(Form::new(Kind::Bug));
        }
    }

    /// Reads the outbox again if it is time, and runs the sending thread.
    fn refresh(&mut self, hub: &mut Hub, now: f64, force: bool) {
        if !force && now < self.next_refresh {
            return;
        }
        self.next_refresh = now + REFRESH;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(d) = &mut self.dispatcher {
            d.pump(hub, true);
        }
        self.reports = hub.list();
        self.asked = hub.consent_asked();
        self.send_on = hub.send_enabled();
        self.newer_outbox = hub.note().is_some();
        self.url = hub.url();
    }

    /// Waits for the sends that started (the end of the program).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish(&mut self, hub: &mut Hub) {
        if let Some(d) = &mut self.dispatcher {
            d.pump(hub, true);
            d.finish(hub);
        }
    }

    /// Takes the picture of a form from the events of this frame.
    #[cfg(not(target_arch = "wasm32"))]
    fn take_screenshots(&mut self, ctx: &egui::Context) {
        let Some(open) = &mut self.form else { return };
        let Some(key) = open.waiting else { return };
        let ppp = ctx.pixels_per_point();
        let got = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot {
                    user_data, image, ..
                } if user_data
                    .data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<u64>())
                    == Some(&key) =>
                {
                    Some(image.clone())
                }
                _ => None,
            })
        });
        if let Some(image) = got {
            open.waiting = None;
            for w in SHOT_WIDTHS {
                if let Ok(img) = crate::screenshot::prepare(&image, None, ppp, w) {
                    let png = img.to_png();
                    if png.len() <= MAX_IMAGE_BYTES {
                        open.texture = texture(ctx, "feedback-shot", &png);
                        open.form.screenshot = Some(png);
                        break;
                    }
                }
            }
        }
    }

    /// Draws the dialogs. Returns a line for the status bar when something happened.
    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) -> Option<String> {
        let now = ctx.input(|i| i.time);
        self.refresh(&mut session.feedback.hub, now, false);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.take_screenshots(ctx);
            // Ask for the picture of a new form in the frame after it opened (the form is not
            // drawn until the picture is there).
            if let Some(open) = &mut self.form
                && let Some(key) = open.waiting
                && open.form.screenshot.is_none()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(key)));
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        let mut out = None;
        if let Some(line) = self.form_window(ctx, &mut session.feedback.hub) {
            out = Some(line);
        }
        if self.window {
            let persistent = session.feedback.hub.persistent();
            self.outbox_window(ctx, &mut session.feedback.hub, persistent);
        }
        if let Some(line) = self.review_window(ctx, &mut session.feedback.hub) {
            out = Some(line);
        }
        self.consent_window(ctx, &mut session.feedback.hub);
        if self.form.is_some() || self.window || self.review.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
        out
    }

    // ---- the form

    fn form_window(&mut self, ctx: &egui::Context, hub: &mut Hub) -> Option<String> {
        let open = self.form.as_mut()?;
        // The form waits for the picture of the window (a frame or two), so that the picture
        // does not show the form.
        if open.waiting.is_some() && open.form.screenshot.is_none() {
            return None;
        }
        open.waiting = None;
        let mut send = false;
        let mut cancel = false;
        let title = match open.form.kind {
            Kind::Bug => tr!("feedback-ui-form-title-bug"),
            Kind::FeatureRequest => tr!("feedback-ui-form-title-feature"),
            Kind::Question => tr!("feedback-ui-form-title-question"),
        };
        let mut window_open = true;
        egui::Window::new(title)
            .id(egui::Id::new("win-feedback-form"))
            .open(&mut window_open)
            .collapsible(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let f = &mut open.form;
                ui.horizontal(|ui| {
                    ui.label(tr!("feedback-ui-kind"));
                    egui::ComboBox::from_id_salt("feedback-kind")
                        .selected_text(kind_name(f.kind))
                        .show_ui(ui, |ui| {
                            for k in [Kind::Bug, Kind::FeatureRequest, Kind::Question] {
                                ui.selectable_value(&mut f.kind, k, kind_name(k));
                            }
                        });
                });
                ui.label(tr!("feedback-ui-title"));
                ui.add(
                    egui::TextEdit::singleline(&mut f.title)
                        .char_limit(MAX_TITLE)
                        .desired_width(f32::INFINITY),
                );
                ui.label(tr!("feedback-ui-text"));
                ui.add(
                    egui::TextEdit::multiline(&mut f.body)
                        .char_limit(MAX_BODY)
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );
                if !f.error.is_empty() {
                    ui.weak(tr!("feedback-ui-error-line", text = f.error.clone()));
                }
                if f.screenshot.is_some() {
                    ui.checkbox(
                        &mut f.want_screenshot,
                        tr!("feedback-ui-include-screenshot"),
                    );
                    if f.want_screenshot {
                        ui.weak(tr!("feedback-ui-screenshot-preview"));
                        if let Some(t) = &open.texture {
                            show_texture(ui, t, 360.0);
                        }
                    }
                }
                if f.want_screenshot && f.screenshot.is_some() {
                    ui.weak(tr!("feedback-ui-scrub-note-screenshot"));
                } else {
                    ui.weak(tr!("feedback-ui-scrub-note"));
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(f.can_send(), egui::Button::new(tr!("feedback-ui-send")))
                        .clicked()
                    {
                        send = true;
                    }
                    if ui.button(tr!("feedback-ui-cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if !window_open || cancel {
            self.form = None;
            return None;
        }
        if !send {
            return None;
        }
        let form = self.form.take()?.form;
        Some(self.submit(hub, &form))
    }

    /// Files the report of a form (what the Send button does). Returns the line for the status bar.
    fn submit(&mut self, hub: &mut Hub, form: &Form) -> String {
        self.next_refresh = 0.0;
        let ctx_info = app_context(env!("CARGO_PKG_VERSION"), std::env::consts::OS);
        match self.flow.send(hub, form, ctx_info, unix_now()) {
            Ok(s) if s.merged => tr!("feedback-ui-merged"),
            Ok(_) => tr!("feedback-ui-saved"),
            Err(e) => tr!("feedback-ui-send-failed", e = e),
        }
    }

    /// For tests: the form is drawn without waiting for the picture of the window.
    #[cfg(test)]
    pub fn skip_picture(&mut self) {
        if let Some(f) = &mut self.form {
            f.waiting = None;
            f.form.screenshot = Some(Vec::new());
        }
    }

    /// For tests: edits the open form.
    #[cfg(test)]
    pub fn edit_form(&mut self, f: impl FnOnce(&mut Form)) {
        if let Some(o) = &mut self.form {
            f(&mut o.form);
        }
    }

    /// For tests: presses Send on the open form.
    #[cfg(test)]
    pub fn press_send(&mut self, hub: &mut Hub) -> Option<String> {
        let form = self.form.take()?.form;
        Some(self.submit(hub, &form))
    }

    // ---- the window with the outbox

    fn outbox_window(&mut self, ctx: &egui::Context, hub: &mut Hub, persistent: bool) {
        let mut open = true;
        let mut write = false;
        let mut review_id = None;
        let mut withdraw = None;
        let mut retry = None;
        #[allow(unused_mut)]
        let mut send_choice: Option<bool> = None;
        #[allow(unused_mut)]
        let mut new_url: Option<String> = None;
        egui::Window::new(tr!("feedback-ui-window-title"))
            .id(egui::Id::new("win-feedback"))
            .open(&mut open)
            .default_width(520.0)
            .default_height(380.0)
            .show(ctx, |ui| {
                // The browser build cannot send yet: no switch, no address, and a note.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let mut on = self.send_on;
                    if ui
                        .checkbox(&mut on, tr!("feedback-ui-send-reports"))
                        .changed()
                    {
                        send_choice = Some(on);
                    }
                    if !self.asked {
                        ui.weak(tr!("feedback-ui-not-asked"));
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr!("feedback-ui-server"));
                        let edit = self.url_edit.get_or_insert_with(|| self.url.clone());
                        let r = ui.add(egui::TextEdit::singleline(edit).desired_width(280.0));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            new_url = Some(edit.clone());
                        }
                    });
                }
                #[cfg(target_arch = "wasm32")]
                ui.weak(tr!("feedback-ui-web-not-sent"));
                if self.newer_outbox {
                    ui.colored_label(ui.visuals().warn_fg_color, tr!("feedback-ui-outbox-newer"));
                } else if !persistent {
                    ui.weak(tr!("feedback-ui-memory-only"));
                }
                ui.separator();
                if ui.button(tr!("feedback-ui-write-a-report")).clicked() {
                    write = true;
                }
                ui.heading(tr!("feedback-ui-outbox"));
                if self.reports.is_empty() {
                    ui.weak(tr!("feedback-ui-empty"));
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for r in self.reports.iter().rev() {
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(kind_name(r.kind));
                                ui.label(&r.title);
                                if r.count > 1 {
                                    ui.weak(tr!("feedback-ui-count", count = r.count));
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.label(status_name(r.status));
                                ui.weak(origin_name(r.origin));
                                if let Some(id) = &r.server_id {
                                    ui.weak(tr!("feedback-ui-server-id", id = id.clone()));
                                }
                            });
                            if let Some(e) = &r.last_error {
                                ui.weak(tr!("feedback-ui-last-error", e = e.clone()));
                            }
                            ui.horizontal(|ui| {
                                if r.status == Status::NeedsReview
                                    && ui.button(tr!("feedback-ui-review")).clicked()
                                {
                                    review_id = Some(r.id.clone());
                                }
                                if r.attempts >= MAX_ATTEMPTS
                                    && !matches!(r.status, Status::Sent | Status::Withdrawn)
                                    && ui.button(tr!("feedback-ui-try-again")).clicked()
                                {
                                    retry = Some(r.id.clone());
                                }
                                if !matches!(r.status, Status::Sent | Status::Withdrawn)
                                    && ui.button(tr!("feedback-ui-withdraw")).clicked()
                                {
                                    withdraw = Some(r.id.clone());
                                }
                            });
                        });
                    }
                });
            });
        self.window = open;
        if write {
            self.open_form(Form::new(Kind::Bug));
        }
        let mut changed = false;
        if let Some(on) = send_choice {
            self.note = hub
                .set_send(Some(on))
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            changed = true;
        }
        if let Some(u) = new_url {
            let u = u.trim().to_string();
            let default = kadakas_feedback::DEFAULT_URL;
            let _ = hub.set_url((u != default && !u.is_empty()).then_some(u));
            self.url_edit = None;
            changed = true;
        }
        if let Some(id) = withdraw {
            let _ = hub.withdraw(&id);
            changed = true;
        }
        if let Some(id) = retry {
            let _ = hub.retry(&id);
            changed = true;
        }
        if let Some(id) = review_id {
            self.start_review(ctx, hub, &id);
        }
        if changed {
            self.refresh(hub, 0.0, true);
        }
    }

    // ---- the review

    fn start_review(&mut self, ctx: &egui::Context, hub: &mut Hub, id: &str) {
        match Review::open(hub, id) {
            Ok(review) => {
                let shots = [AttachmentKind::Screenshot, AttachmentKind::Render]
                    .into_iter()
                    .filter_map(|k| {
                        let png = review.image(k)?;
                        Some((k, texture(ctx, &format!("feedback-{id}-{}", k.id()), png)?))
                    })
                    .collect();
                self.review = Some(ReviewState {
                    review,
                    shots,
                    json: String::new(),
                    last: None,
                });
            }
            Err(e) => self.note = e.to_string(),
        }
    }

    fn review_window(&mut self, ctx: &egui::Context, hub: &mut Hub) -> Option<String> {
        let st = self.review.as_mut()?;
        let mut open = true;
        let mut send = false;
        let mut withdraw = false;
        let mut keep = false;
        let id = st.review.report.id.clone();
        egui::Window::new(tr!("feedback-ui-review-title", id = id.clone()))
            .id(egui::Id::new("win-feedback-review"))
            .open(&mut open)
            .default_width(560.0)
            .default_height(520.0)
            .show(ctx, |ui| {
                ui.label(tr!("feedback-ui-review-intro"));
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| review_body(ui, st));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr!("feedback-ui-review-send")).clicked() {
                        send = true;
                    }
                    if ui.button(tr!("feedback-ui-review-keep")).clicked() {
                        keep = true;
                    }
                    if ui.button(tr!("feedback-ui-withdraw")).clicked() {
                        withdraw = true;
                    }
                });
            });
        if !open || keep {
            self.review = None;
            return None;
        }
        if withdraw {
            let _ = hub.withdraw(&id);
            self.review = None;
            self.refresh(hub, 0.0, true);
            return None;
        }
        if send {
            let st = self.review.take()?;
            let line = match st.review.approve(hub) {
                Ok(()) => tr!("feedback-ui-review-approved"),
                Err(e) => tr!("feedback-ui-send-failed", e = e),
            };
            self.refresh(hub, 0.0, true);
            return Some(line);
        }
        None
    }

    // ---- the consent

    fn consent_window(&mut self, ctx: &egui::Context, hub: &mut Hub) {
        // The browser build does not send, so it has nothing to ask.
        if cfg!(target_arch = "wasm32") {
            return;
        }
        if !self.flow.consent_due(self.asked, &self.reports) {
            return;
        }
        let mut answer = None;
        egui::Window::new(tr!("feedback-ui-consent-title"))
            .id(egui::Id::new("win-feedback-consent"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(tr!("feedback-ui-consent-text"));
                ui.weak(tr!("feedback-ui-consent-server", url = self.url.clone()));
                ui.weak(tr!("feedback-ui-consent-change"));
                ui.horizontal(|ui| {
                    if ui.button(tr!("feedback-ui-consent-allow")).clicked() {
                        answer = Some(Some(true));
                    }
                    if ui.button(tr!("feedback-ui-consent-deny")).clicked() {
                        answer = Some(Some(false));
                    }
                    if ui.button(tr!("feedback-ui-consent-later")).clicked() {
                        answer = Some(None);
                    }
                });
            });
        if let Some(a) = answer {
            if let Err(e) = self.flow.answer_consent(hub, a) {
                self.note = e;
            }
            self.refresh(hub, 0.0, true);
        }
    }

    /// For tests: answers the consent question without a click.
    #[cfg(test)]
    pub fn answer_consent(&mut self, hub: &mut Hub, a: Option<bool>) {
        self.flow.answer_consent(hub, a).unwrap();
        self.refresh(hub, 0.0, true);
    }

    /// For tests: reads the outbox now.
    #[cfg(test)]
    pub fn refresh_now(&mut self, hub: &mut Hub) {
        self.refresh(hub, 0.0, true);
    }

    #[cfg(test)]
    pub fn consent_is_due(&self) -> bool {
        self.flow.consent_due(self.asked, &self.reports)
    }

    /// For tests: files a form like the Send button does.
    #[cfg(test)]
    pub fn send_form(
        &mut self,
        hub: &mut Hub,
        form: &Form,
    ) -> Result<kadakas_ui_core::feedback::Sent, String> {
        let r = self.flow.send(hub, form, app_context("test", "test"), 0);
        self.refresh(hub, 0.0, true);
        r
    }
}

/// The content of the review: text, every attachment with its tick, the privacy options and the
/// exact message.
fn review_body(ui: &mut egui::Ui, st: &mut ReviewState) {
    let rep = st.review.report.clone();
    ui.strong(tr!(
        "feedback-ui-review-heading",
        kind = kind_name(rep.kind),
        title = rep.title.clone()
    ));
    for text in [&rep.body, &rep.wanted, &rep.tried, &rep.method, &rep.error] {
        if !text.is_empty() {
            ui.label(text);
        }
    }
    ui.weak(tr!(
        "feedback-ui-review-context",
        version = rep.context.version.clone(),
        os = rep.context.os.clone(),
        client = format!("{} {}", rep.context.client_name, rep.context.client_version)
    ));
    ui.separator();
    for (kind, _) in st.review.attachments().to_vec() {
        let mut on = st.review.included(kind);
        let label = match kind {
            AttachmentKind::Excerpt => tr!(
                "feedback-ui-review-excerpt",
                count = rep.attachment(kind).and_then(|a| a.objects).unwrap_or(0)
            ),
            AttachmentKind::Screenshot => tr!("feedback-ui-review-screenshot"),
            AttachmentKind::Render => tr!("feedback-ui-review-picture"),
            AttachmentKind::Calls => tr!("feedback-ui-review-calls"),
            AttachmentKind::Log => tr!("feedback-ui-review-log"),
        };
        if ui.checkbox(&mut on, label).changed() {
            st.review.set_included(kind, on);
        }
        if !on {
            continue;
        }
        ui.indent(("attachment", kind.id()), |ui| match kind {
            AttachmentKind::Excerpt => excerpt_view(ui, st),
            AttachmentKind::Screenshot | AttachmentKind::Render => {
                if let Some((_, t)) = st.shots.iter().find(|(k, _)| *k == kind) {
                    show_texture(ui, t, 420.0);
                }
            }
            AttachmentKind::Calls => text_box(
                ui,
                ("calls", 0),
                &st.review.calls_text().unwrap_or_default(),
            ),
            AttachmentKind::Log => {
                text_box(ui, ("log", 0), &st.review.log_text().unwrap_or_default())
            }
        });
    }
    ui.separator();
    // The exact message: made again when a tick or an option changed.
    let key = (st.review.attachments().to_vec(), st.review.privacy);
    let ticks: Vec<(AttachmentKind, bool)> = st
        .review
        .attachments()
        .iter()
        .map(|(k, _)| (*k, st.review.included(*k)))
        .collect();
    let now = (ticks, key.1);
    if st.last.as_ref() != Some(&now) {
        st.json = match st.review.payload() {
            Ok(p) => display_json(&p),
            Err(e) => e.to_string(),
        };
        st.last = Some(now);
    }
    egui::CollapsingHeader::new(tr!("feedback-ui-review-json"))
        .id_salt("review-json")
        .show(ui, |ui| {
            ui.weak(tr!("feedback-ui-review-json-note"));
            text_box(ui, ("json", 0), &st.json);
        });
}

/// A read-only text in a box of limited height.
fn text_box(ui: &mut egui::Ui, id: (&str, u8), text: &str) {
    let mut t = text;
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(160.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut t)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
        });
}

/// The options of the excerpt, the objects and the small plan.
fn excerpt_view(ui: &mut egui::Ui, st: &mut ReviewState) {
    let mut p = st.review.privacy;
    ui.checkbox(
        &mut p.move_to_origin,
        tr!("feedback-ui-review-move-to-origin"),
    );
    ui.checkbox(
        &mut p.remove_georef,
        tr!("feedback-ui-review-remove-georef"),
    );
    ui.checkbox(&mut p.remove_names, tr!("feedback-ui-review-remove-names"));
    st.review.privacy = p;
    ui.weak(tr!("feedback-ui-review-options-note"));
    if let Some(Err(e)) = st.review.excerpt_text() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            tr!("feedback-ui-review-excerpt-failed", e = e),
        );
        return;
    }
    let objects = st.review.excerpt_objects();
    plan(ui, &objects);
    egui::CollapsingHeader::new(tr!("feedback-ui-review-objects"))
        .id_salt("review-objects")
        .show(ui, |ui| {
            for o in objects.iter().take(60) {
                ui.label(tr!(
                    "feedback-ui-review-object",
                    kind = o.kind.clone(),
                    layer = o.layer.clone()
                ));
            }
        });
}

/// A small plan: the bounding boxes of the objects.
fn plan(ui: &mut egui::Ui, objects: &[kadakas_api::feedback_api::excerpt::ObjectBrief]) {
    let (w, h) = (360.0f32, 150.0f32);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for o in objects {
        for i in 0..2 {
            lo[i] = lo[i].min(o.min[i]);
            hi[i] = hi[i].max(o.max[i]);
        }
    }
    if objects.is_empty() || !lo[0].is_finite() {
        return;
    }
    let span = [(hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0)];
    let scale = (f64::from(w - 8.0) / span[0]).min(f64::from(h - 8.0) / span[1]);
    let map = |x: f64, y: f64| {
        egui::pos2(
            rect.min.x + 4.0 + ((x - lo[0]) * scale) as f32,
            rect.max.y - 4.0 - ((y - lo[1]) * scale) as f32,
        )
    };
    let stroke = egui::Stroke::new(1.0, ui.visuals().text_color());
    for o in objects {
        let r = egui::Rect::from_two_pos(map(o.min[0], o.min[1]), map(o.max[0], o.max[1]));
        if r.width() < 1.5 && r.height() < 1.5 {
            painter.circle_filled(r.center(), 1.5, stroke.color);
        } else {
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        }
    }
}

/// The report form for an error message of the app, with the last command.
pub fn error_form(status: &str, last_command: &str) -> Form {
    Form::from_error(status, last_command)
}

/// The browser keeps the outbox in `localStorage`, in small capped entries.
#[cfg(target_arch = "wasm32")]
pub mod web_store {
    use base64::Engine;
    use kadakas_feedback::Storage;

    const PREFIX: &str = "kadakas.feedback/";
    /// The most that the outbox takes of the roughly 5 MB of a site.
    const CAP: usize = 2_000_000;

    pub struct WebStorage;

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    fn total(s: &web_sys::Storage) -> usize {
        (0..s.length().unwrap_or(0))
            .filter_map(|i| s.key(i).ok().flatten())
            .filter(|k| k.starts_with(PREFIX))
            .filter_map(|k| s.get_item(&k).ok().flatten())
            .map(|v| v.len())
            .sum()
    }

    impl Storage for WebStorage {
        fn read(&self, name: &str) -> Option<Vec<u8>> {
            if !kadakas_feedback::store::safe_name(name) {
                return None;
            }
            let t = storage()?.get_item(&format!("{PREFIX}{name}")).ok()??;
            base64::engine::general_purpose::STANDARD.decode(t).ok()
        }

        fn write(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
            if !kadakas_feedback::store::safe_name(name) {
                return Err(format!("not a feedback file name: {name}"));
            }
            let s = storage().ok_or("the browser storage is not available")?;
            let key = format!("{PREFIX}{name}");
            let text = base64::engine::general_purpose::STANDARD.encode(data);
            let old = s.get_item(&key).ok().flatten().map_or(0, |v| v.len());
            if total(&s) - old + text.len() > CAP {
                return Err("the feedback store is full".into());
            }
            s.set_item(&key, &text)
                .map_err(|_| "the browser storage is full".to_string())
        }

        fn remove(&mut self, name: &str) {
            if let Some(s) = storage()
                && kadakas_feedback::store::safe_name(name)
            {
                let _ = s.remove_item(&format!("{PREFIX}{name}"));
            }
        }

        fn persistent(&self) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_feedback::Draft;

    fn png() -> Vec<u8> {
        kadakas_render::raster::Image::new(8, 6, [10, 20, 30, 255]).to_png()
    }

    #[test]
    fn the_help_commands_open_the_right_thing() {
        let mut ui = FeedbackUi::default();
        ui.open(Open::ReportBug);
        assert_eq!(ui.form().unwrap().kind, Kind::Bug);
        ui.open(Open::RequestFeature);
        assert_eq!(ui.form().unwrap().kind, Kind::FeatureRequest);
        assert!(!ui.is_window_open());
        ui.open(Open::Window);
        assert!(ui.is_window_open());
    }

    #[test]
    fn the_button_opens_the_window_when_a_review_waits_else_the_form() {
        let mut ui = FeedbackUi::default();
        let mut hub = Hub::in_memory();
        ui.on_click(Click::Button);
        assert!(ui.is_form_open() && !ui.is_window_open());
        ui = FeedbackUi::default();
        let mut d = Draft::new(Kind::Bug, "From an agent", "b", Origin::Agent);
        d.method = "m".into();
        hub.submit(
            d,
            vec![kadakas_feedback::AttachmentData {
                kind: AttachmentKind::Render,
                bytes: png(),
                objects: None,
            }],
            0,
        )
        .unwrap();
        ui.refresh_now(&mut hub);
        assert_eq!(ui.badge(), 1);
        ui.on_click(Click::Button);
        assert!(ui.is_window_open() && !ui.is_form_open());
    }

    #[test]
    fn the_form_files_a_report_and_the_consent_follows() {
        let mut ui = FeedbackUi::default();
        let mut hub = Hub::in_memory();
        let mut f = Form::from_error("Error: radius must be positive", "edit.fillet");
        f.body = "It happened twice at /Users/ann/x/garden.kad".into();
        let s = ui.send_form(&mut hub, &f).unwrap();
        assert_eq!(s.id, "fb-1");
        // The person has not been asked: the dialog is due, nothing goes out.
        assert!(ui.consent_is_due());
        assert!(hub.outgoing(true, 5, 1_000).is_empty());
        ui.answer_consent(&mut hub, Some(true));
        assert!(!ui.consent_is_due());
        assert_eq!(hub.outgoing(true, 5, 1_000).len(), 1);
        let r = hub.get("fb-1").unwrap();
        assert_eq!(r.method, "edit.fillet");
        assert!(r.body.contains("<path>/garden.kad") && !r.body.contains("ann"));
        assert_eq!(r.origin, Origin::User);
    }

    #[test]
    fn a_report_of_an_agent_in_the_outbox_triggers_the_consent_dialog_too() {
        let mut ui = FeedbackUi::default();
        let mut hub = Hub::in_memory();
        ui.refresh_now(&mut hub);
        assert!(!ui.consent_is_due());
        hub.submit(
            Draft::new(Kind::Question, "q", "", Origin::Agent),
            vec![],
            0,
        )
        .unwrap();
        ui.refresh_now(&mut hub);
        assert!(ui.consent_is_due());
        ui.answer_consent(&mut hub, Some(false));
        assert!(!ui.consent_is_due() && !hub.send_enabled());
    }

    #[test]
    fn the_window_sends_through_the_dispatcher_and_records_the_result() {
        use kadakas_feedback::dispatch::Dispatcher;
        use kadakas_feedback::{FakeSender, NoSleep, RetryPolicy};
        let fake = std::sync::Arc::new(FakeSender::new(vec![]));
        let mut ui = FeedbackUi::default();
        ui.set_dispatcher(Dispatcher::new(
            fake.clone(),
            std::sync::Arc::new(NoSleep::default()),
            RetryPolicy {
                attempts: 1,
                base_ms: 1,
                max_ms: 1,
            },
        ));
        let mut hub = Hub::in_memory();
        // Without the consent nothing goes out.
        hub.submit(Draft::new(Kind::Question, "q", "", Origin::User), vec![], 0)
            .unwrap();
        ui.refresh_now(&mut hub);
        ui.finish(&mut hub);
        assert!(fake.requests().is_empty());
        hub.set_send(Some(true)).unwrap();
        ui.refresh_now(&mut hub);
        ui.finish(&mut hub);
        assert_eq!(fake.requests().len(), 1);
        ui.refresh_now(&mut hub);
        assert_eq!(hub.get("fb-1").unwrap().status, Status::Sent);
    }

    #[test]
    fn the_name_helpers_cover_every_value() {
        for k in [Kind::Bug, Kind::FeatureRequest, Kind::Question] {
            assert!(!kind_name(k).is_empty());
        }
        for s in [
            Status::Pending,
            Status::NeedsReview,
            Status::Approved,
            Status::Sent,
            Status::Withdrawn,
        ] {
            assert!(!status_name(s).is_empty());
        }
        for o in [Origin::Agent, Origin::User, Origin::Crash] {
            assert!(!origin_name(o).is_empty());
        }
        assert_eq!(error_form("Error: x", "cmd").method, "cmd");
        assert!(unix_now() > 0);
    }
}
