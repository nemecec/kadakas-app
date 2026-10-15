//! Issues in the app (REV-05): the Issues palette, the pins in the plan and in 3D, the window of
//! a new issue, and File > BCF Issues...
//!
//! The logic is in `kadakas_ui_core::issues` (what the list shows, the form, what opening an issue
//! does). This module draws it and calls the `issue.*` methods of the API, so the app and an agent
//! use one code path and every change is one undo step. What needs the view of the app (open an
//! issue, make an issue from the current view) goes to the app as a [`Request`].

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Ui, vec2};
use glam::DVec2;
use kadakas_api::{RpcError, Session};
use kadakas_core::change::Origin;
use kadakas_core::issue::{
    self, Comment, Issue, IssueId, IssueStatus, MAX_TEXT_CHARS, Viewpoint, ViewpointId,
};
use kadakas_core::model::ObjectId;
use kadakas_l10n::{tr, translate};
use kadakas_review::issues::{consent_reason, facets};
use kadakas_ui_core::issues::{
    self as logic, Draft, Due, Filters, Mark, Pin, Row, badge, pick_pin, status_mark,
    status_message, status_rgb,
};
use kadakas_ui_core::versions::time_parts;
use serde_json::{Value, json};

/// How far from the middle of a pin a click still hits it, in points.
const PIN_HIT: f64 = 13.0;
/// The height of one row of the list.
const ROW_HEIGHT: f32 = 38.0;

/// What the app does for the Issues UI. They need the view, which the app owns.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Open an issue: restore the viewpoint and select the objects.
    Open(IssueId, Option<ViewpointId>),
    /// Make the issue of a form, with a viewpoint of the current view if the form asks for it.
    Create(Draft),
    /// Add the current view to an issue as a new viewpoint.
    AddViewpoint(IssueId),
}

/// A question that waits for the person.
#[derive(Clone, Debug, PartialEq)]
enum Confirm {
    /// Delete an issue. The text says why the person is asked, for work of somebody else.
    Delete(IssueId, String, Option<String>),
}

/// The window of a new issue.
#[derive(Clone, Debug, PartialEq)]
struct NewIssue {
    draft: Draft,
    error: Option<String>,
}

/// The pins that were found for one revision of the document and one set of filters.
struct PinsCache {
    revision: u64,
    filters: Filters,
    pins: Vec<Pin>,
}

/// State of the Issues UI.
#[derive(Default)]
pub struct IssuesUi {
    pub filters: Filters,
    pub selected: Option<IssueId>,
    hide_pins: bool,
    pick: bool,
    new_issue: Option<NewIssue>,
    comment: String,
    confirm: Option<Confirm>,
    #[cfg(not(target_arch = "wasm32"))]
    bcf: Option<crate::bcf_dialog::BcfDialog>,
    /// Where the pins were drawn in the last frame, for clicks.
    screens: Vec<(IssueId, DVec2)>,
    requests: Vec<Request>,
    pins: Option<PinsCache>,
}

// ----- text -----

fn origin_text(o: Origin) -> String {
    match o {
        Origin::User => tr!("issues-ui-origin-user"),
        Origin::Agent => tr!("issues-ui-origin-agent"),
        Origin::Script => tr!("issues-ui-origin-script"),
        Origin::Remote => tr!("issues-ui-origin-remote"),
    }
}

pub fn status_text(s: IssueStatus) -> String {
    translate(status_message(s), &[])
}

/// The time of a change, in the date format of the person, with the time of day (UTC).
fn time_text(ms: i64) -> String {
    match time_parts(u64::try_from(ms).unwrap_or(0)) {
        None => tr!("issues-ui-unknown-time"),
        Some((secs, h, m)) => format!(
            "{} {h:02}:{m:02}",
            kadakas_units::current().date_text(kadakas_units::Date::from_unix_secs(secs))
        ),
    }
}

fn type_text(kind: &str) -> String {
    match kind {
        "issue" => tr!("issues-ui-type-issue"),
        "error" => tr!("issues-ui-type-error"),
        "warning" => tr!("issues-ui-type-warning"),
        "request" => tr!("issues-ui-type-request"),
        "clash" => tr!("issues-ui-type-clash"),
        "remark" => tr!("issues-ui-type-remark"),
        other => other.to_string(),
    }
}

fn priority_text(p: &str) -> String {
    match p {
        "low" => tr!("issues-ui-priority-low"),
        "normal" => tr!("issues-ui-priority-normal"),
        "high" => tr!("issues-ui-priority-high"),
        "critical" => tr!("issues-ui-priority-critical"),
        other => other.to_string(),
    }
}

fn color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// The name of the person for the author of an issue: the user of the computer.
fn author_name() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        for key in ["USER", "USERNAME"] {
            if let Ok(v) = std::env::var(key)
                && !v.trim().is_empty()
            {
                return v
                    .trim()
                    .chars()
                    .take(kadakas_core::issue::MAX_NAME_CHARS)
                    .collect();
            }
        }
    }
    tr!("issues-ui-author-default")
}

pub(crate) fn now_ms() -> i64 {
    crate::collab_ui::now_ms() as i64
}

fn call(session: &mut Session, method: &str, p: Value) -> Result<Value, RpcError> {
    session.handle(method, p)
}

// ----- the pin -----

/// Draws a pin with its tip at `tip`: a drop in the color of the status, a mark inside it that
/// says the status without a color, and a count for an issue about many objects.
pub fn paint_pin(
    painter: &Painter,
    tip: Pos2,
    status: IssueStatus,
    count: Option<&str>,
    selected: bool,
    scale: f32,
) {
    let r = 8.0 * scale;
    let c = Pos2::new(tip.x, tip.y - 2.2 * r);
    let fill = color(status_rgb(status));
    let outline = Stroke::new(1.5 * scale, Color32::WHITE);
    if selected {
        painter.circle_stroke(
            c,
            r + 4.0 * scale,
            Stroke::new(2.5 * scale, Color32::from_rgb(255, 200, 0)),
        );
    }
    // The point of the drop, then its round head.
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(c.x - r * 0.82, c.y + r * 0.55),
            Pos2::new(c.x + r * 0.82, c.y + r * 0.55),
            tip,
        ],
        fill,
        Stroke::NONE,
    ));
    painter.circle(c, r, fill, outline);
    let white = Stroke::new(1.8 * scale, Color32::WHITE);
    match status_mark(status) {
        Mark::Dot => {
            painter.circle_filled(c, r * 0.32, Color32::WHITE);
        }
        Mark::Half => {
            let n = 8;
            let mut pts: Vec<Pos2> = (0..=n)
                .map(|k| {
                    let a =
                        -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * k as f32 / n as f32;
                    Pos2::new(c.x + r * 0.5 * a.cos(), c.y + r * 0.5 * a.sin())
                })
                .collect();
            pts.push(pts[0]);
            painter.add(Shape::convex_polygon(pts, Color32::WHITE, Stroke::NONE));
        }
        Mark::Check => {
            painter.add(Shape::line(
                vec![
                    Pos2::new(c.x - r * 0.45, c.y),
                    Pos2::new(c.x - r * 0.1, c.y + r * 0.35),
                    Pos2::new(c.x + r * 0.5, c.y - r * 0.35),
                ],
                white,
            ));
        }
        Mark::Bar => {
            painter.line_segment(
                [
                    Pos2::new(c.x - r * 0.45, c.y),
                    Pos2::new(c.x + r * 0.45, c.y),
                ],
                white,
            );
        }
    }
    if let Some(text) = count {
        let b = Pos2::new(c.x + r * 0.95, c.y - r * 0.95);
        let br = 6.5 * scale;
        painter.circle(
            b,
            br,
            Color32::from_rgb(35, 35, 35),
            Stroke::new(1.0, Color32::WHITE),
        );
        painter.text(
            b,
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(8.5 * scale),
            Color32::WHITE,
        );
    }
}

impl IssuesUi {
    // ----- used by the app -----

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// True while the next click in the drawing places a new issue.
    pub fn picking(&self) -> bool {
        self.pick
    }

    pub fn set_picking(&mut self, on: bool) {
        self.pick = on;
    }

    /// The window of a new issue for the selected objects.
    pub fn start_for_selection(&mut self, selection: &[ObjectId]) {
        self.new_issue = Some(NewIssue {
            draft: Draft::for_objects(selection),
            error: None,
        });
        self.pick = false;
    }

    /// The window of a new issue at a point of the model.
    pub fn start_at(&mut self, p: DVec2, z: f64) {
        self.new_issue = Some(NewIssue {
            draft: Draft::at_point([p.x, p.y, z]),
            error: None,
        });
        self.pick = false;
    }

    /// The window again, with the form as it was and the reason why the issue was not made.
    pub fn reopen(&mut self, draft: Draft, why: String) {
        self.new_issue = Some(NewIssue {
            draft,
            error: Some(why),
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_bcf(&mut self) {
        self.bcf.get_or_insert_with(Default::default);
    }

    #[cfg(test)]
    pub fn is_new_issue_open(&self) -> bool {
        self.new_issue.is_some()
    }

    #[cfg(test)]
    pub fn draft_mut(&mut self) -> Option<&mut Draft> {
        self.new_issue.as_mut().map(|n| &mut n.draft)
    }

    /// The pins for the document as it is, with the filters of the palette.
    fn pins(&mut self, session: &Session) -> &[Pin] {
        let revision = session.editor.revision();
        let stale = self
            .pins
            .as_ref()
            .is_none_or(|c| c.revision != revision || c.filters != self.filters);
        if stale {
            self.pins = Some(PinsCache {
                revision,
                filters: self.filters.clone(),
                pins: logic::pins(session.editor.doc(), &self.filters),
            });
        }
        &self.pins.as_ref().expect("made above").pins
    }

    /// Draws the pins on the plan.
    pub fn paint_plan(
        &mut self,
        painter: &Painter,
        rect: Rect,
        view: &crate::view::View,
        session: &Session,
    ) {
        use crate::view::ViewExt as _;
        self.screens.clear();
        if self.hide_pins {
            return;
        }
        let selected = self.selected;
        let pins: Vec<Pin> = self.pins(session).to_vec();
        let painter = painter.with_clip_rect(rect);
        for p in &pins {
            let at = view.to_screen(DVec2::new(p.at[0], p.at[1]));
            if !rect.expand(30.0).contains(at) {
                continue;
            }
            paint_pin(
                &painter,
                at,
                p.status,
                badge(p.objects).as_deref(),
                selected == Some(p.id),
                1.0,
            );
            // The middle of the head, for a click.
            self.screens
                .push((p.id, DVec2::new(at.x as f64, at.y as f64 - 17.6)));
        }
    }

    /// Draws the pins in 3D. `view_proj` takes a model point (mm) to clip space.
    pub fn paint_3d(
        &mut self,
        painter: &Painter,
        rect: Rect,
        view_proj: glam::Mat4,
        session: &Session,
    ) {
        self.screens.clear();
        if self.hide_pins {
            return;
        }
        let selected = self.selected;
        let pins: Vec<Pin> = self.pins(session).to_vec();
        let painter = painter.with_clip_rect(rect);
        for p in &pins {
            let clip =
                view_proj * glam::Vec4::new(p.at[0] as f32, p.at[1] as f32, p.at[2] as f32, 1.0);
            if clip.w.partial_cmp(&1e-6) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            let ndc = clip.truncate() / clip.w;
            if ndc.x.abs() > 1.2 || ndc.y.abs() > 1.2 || !(0.0..=1.0).contains(&ndc.z) {
                continue;
            }
            let at = Pos2::new(
                rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(),
                rect.min.y + (0.5 - ndc.y * 0.5) * rect.height(),
            );
            paint_pin(
                &painter,
                at,
                p.status,
                badge(p.objects).as_deref(),
                selected == Some(p.id),
                1.0,
            );
            self.screens
                .push((p.id, DVec2::new(at.x as f64, at.y as f64 - 17.6)));
        }
    }

    /// The pin at a screen position, from the last frame.
    pub fn pin_at(&self, p: Pos2) -> Option<IssueId> {
        pick_pin(&self.screens, DVec2::new(p.x as f64, p.y as f64), PIN_HIT)
    }

    /// A click on a pin selects its issue and asks the app to open it.
    pub fn click_pin(&mut self, id: IssueId) {
        self.selected = Some(id);
        self.requests.push(Request::Open(id, None));
    }

    // ----- the palette -----

    pub fn palette(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        let doc = session.editor.doc();
        ui.horizontal_wrapped(|ui| {
            let has_selection = !session.selection.is_empty();
            let button =
                crate::icons::button(ui, "issue-pin", &tr!("issues-ui-new-from-selection"));
            if ui.add_enabled(has_selection, button).clicked() {
                let selection = session.selection.clone();
                self.start_for_selection(&selection);
            }
            if ui
                .selectable_label(self.pick, tr!("issues-ui-new-at-click"))
                .on_hover_text(tr!("issues-ui-pick-hint"))
                .clicked()
            {
                self.pick = !self.pick;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if ui.button(tr!("issues-ui-bcf")).clicked() {
                self.open_bcf();
            }
        });
        if self.pick {
            ui.small(tr!("issues-ui-pick-hint"));
        }
        ui.checkbox(&mut self.hide_pins, tr!("issues-ui-hide-pins"));
        ui.separator();
        self.filter_ui(ui, doc);
        let rows = logic::rows(doc, &self.filters, now_ms());
        let total = doc.issues.len();
        if total == 0 {
            ui.label(tr!("issues-ui-none"));
        } else {
            ui.small(tr!("issues-ui-count", shown = rows.len(), total = total));
        }
        if rows.is_empty() && total > 0 {
            ui.label(tr!("issues-ui-none-match"));
        }
        // Only the rows in view are made: the list can hold thousands of issues.
        egui::ScrollArea::vertical()
            .id_salt("issues-list")
            .max_height(240.0)
            .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                for row in &rows[range] {
                    self.row_ui(ui, row);
                }
            });
        // Shared issue data that this document does not show is not lost: say so.
        let hidden = session.collab_hidden_issue_data();
        if !hidden.is_empty() {
            egui::CollapsingHeader::new(tr!("issues-ui-hidden", count = hidden.len()))
                .id_salt("issues-hidden")
                .show(ui, |ui| {
                    ui.small(tr!("issues-ui-hidden-help"));
                    for reason in hidden.iter().take(20) {
                        ui.small(reason);
                    }
                });
        }
        // The selected issue.
        let Some(id) = self.selected else { return };
        let doc = session.editor.doc();
        let Some(issue) = doc.issues.get(id).cloned() else {
            self.selected = None;
            return;
        };
        ui.separator();
        self.detail_ui(ui, session, status, &issue);
    }

    fn filter_ui(&mut self, ui: &mut Ui, doc: &kadakas_core::model::Document) {
        ui.horizontal_wrapped(|ui| {
            for s in IssueStatus::ALL {
                let on = self.filters.statuses.contains(&s);
                let text = egui::RichText::new(status_text(s)).color(color(status_rgb(s)));
                if ui.selectable_label(on, text).clicked() {
                    self.filters.toggle_status(s);
                }
            }
        });
        ui.checkbox(&mut self.filters.show_done, tr!("issues-ui-show-done"));
        let (assignees, labels) = facets(&doc.issues);
        ui.horizontal(|ui| {
            ui.label(tr!("issues-ui-filter-assignee"));
            choose(
                ui,
                "issues-assignee",
                &mut self.filters.assignee,
                &assignees,
            );
            ui.label(tr!("issues-ui-filter-label"));
            choose(ui, "issues-label", &mut self.filters.label, &labels);
        });
        ui.horizontal(|ui| {
            ui.label(tr!("issues-ui-filter-text"));
            ui.add(egui::TextEdit::singleline(&mut self.filters.text).desired_width(140.0));
            if self.filters.is_active() && ui.small_button(tr!("issues-ui-clear-filters")).clicked()
            {
                self.filters.clear();
            }
        });
    }

    fn row_ui(&mut self, ui: &mut Ui, row: &Row) {
        let height = ROW_HEIGHT;
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::click());
        let selected = self.selected == Some(row.id);
        if selected || resp.hovered() {
            let fill = if selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            };
            ui.painter().rect_filled(rect, 3.0, fill);
        }
        let p = ui.painter_at(rect);
        paint_pin(
            &p,
            Pos2::new(rect.min.x + 14.0, rect.max.y - 6.0),
            row.status,
            None,
            false,
            0.8,
        );
        let text_color = ui.visuals().text_color();
        p.text(
            Pos2::new(rect.min.x + 30.0, rect.min.y + 4.0),
            egui::Align2::LEFT_TOP,
            &row.title,
            egui::FontId::proportional(13.5),
            text_color,
        );
        let mut meta = vec![status_text(row.status), priority_text(&row.priority)];
        if let Some(a) = &row.assignee {
            meta.push(a.clone());
        }
        if row.comments > 0 {
            meta.push(tr!("issues-ui-row-comments", count = row.comments));
        }
        if row.objects > 0 {
            meta.push(tr!("issues-ui-row-objects", count = row.objects));
        }
        if let Some(d) = &row.due {
            let text = match row.due_state {
                Due::Overdue => tr!("issues-ui-due-overdue", date = d.clone()),
                Due::Soon => tr!("issues-ui-due-soon", date = d.clone()),
                _ => tr!("issues-ui-due", date = d.clone()),
            };
            meta.push(text);
        }
        let weak = ui.visuals().weak_text_color();
        p.text(
            Pos2::new(rect.min.x + 30.0, rect.min.y + 21.0),
            egui::Align2::LEFT_TOP,
            meta.join(" · "),
            egui::FontId::proportional(11.0),
            if row.due_state == Due::Overdue {
                Color32::from_rgb(200, 60, 40)
            } else {
                weak
            },
        );
        if resp.clicked() {
            self.selected = Some(row.id);
            self.requests.push(Request::Open(row.id, None));
        }
    }

    fn detail_ui(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        status: &mut String,
        issue: &Issue,
    ) {
        let id = issue.id;
        ui.strong(&issue.title);
        ui.small(tr!(
            "issues-ui-detail-by",
            author = if issue.author.is_empty() {
                tr!("issues-ui-author-default")
            } else {
                issue.author.clone()
            },
            origin = origin_text(issue.origin),
            time = time_text(issue.created)
        ));
        // Status, priority and type are changed here: one undo step each.
        let mut patch = serde_json::Map::new();
        egui::Grid::new("issue-fields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(tr!("issues-ui-detail-status"));
                let mut s = issue.status;
                egui::ComboBox::from_id_salt("issue-status")
                    .selected_text(status_text(s))
                    .show_ui(ui, |ui| {
                        for v in IssueStatus::ALL {
                            ui.selectable_value(&mut s, v, status_text(v));
                        }
                    });
                if s != issue.status {
                    patch.insert("status".into(), json!(s.name()));
                }
                ui.end_row();
                ui.label(tr!("issues-ui-detail-priority"));
                let mut p = issue.priority.clone();
                egui::ComboBox::from_id_salt("issue-priority")
                    .selected_text(priority_text(&p))
                    .show_ui(ui, |ui| {
                        for v in issue::PRIORITIES {
                            ui.selectable_value(&mut p, v.to_string(), priority_text(v));
                        }
                    });
                if p != issue.priority {
                    patch.insert("priority".into(), json!(p));
                }
                ui.end_row();
                ui.label(tr!("issues-ui-detail-type"));
                let mut k = issue.kind.clone();
                egui::ComboBox::from_id_salt("issue-type")
                    .selected_text(type_text(&k))
                    .show_ui(ui, |ui| {
                        for v in issue::TYPES {
                            ui.selectable_value(&mut k, v.to_string(), type_text(v));
                        }
                    });
                if k != issue.kind {
                    patch.insert("type".into(), json!(k));
                }
                ui.end_row();
                ui.label(tr!("issues-ui-detail-assignee"));
                ui.label(issue.assignee.clone().unwrap_or_else(|| "-".into()));
                ui.end_row();
                if !issue.labels.is_empty() {
                    ui.label(tr!("issues-ui-detail-labels"));
                    ui.label(issue.labels.join(", "));
                    ui.end_row();
                }
                if let Some(d) = &issue.due {
                    ui.label(tr!("issues-ui-detail-due"));
                    ui.label(d);
                    ui.end_row();
                }
            });
        if !patch.is_empty() {
            patch.insert("id".into(), json!(id));
            patch.insert("now_ms".into(), json!(now_ms()));
            match call(session, "issue.update", Value::Object(patch)) {
                Ok(_) => {}
                Err(e) => *status = tr!("issues-ui-error", e = e.message),
            }
        }
        if !issue.description.is_empty() {
            ui.label(&issue.description);
        }
        let doc = session.editor.doc();
        if !issue.objects.is_empty() {
            let missing = issue
                .objects
                .iter()
                .filter(|o| doc.object(**o).is_none())
                .count();
            let mut line = tr!("issues-ui-objects", count = issue.objects.len());
            if missing > 0 {
                line.push(' ');
                line.push_str(&tr!("issues-ui-objects-missing", count = missing));
            }
            ui.small(line);
        }
        // Viewpoints.
        ui.add_space(4.0);
        ui.strong(tr!("issues-ui-viewpoints"));
        if issue.viewpoints.is_empty() {
            ui.small(tr!("issues-ui-no-viewpoints"));
        }
        for (n, vp) in issue.viewpoints.iter().enumerate() {
            self.viewpoint_row(ui, session, status, issue, n, vp);
        }
        if ui.button(tr!("issues-ui-viewpoint-add")).clicked() {
            self.requests.push(Request::AddViewpoint(id));
        }
        // The thread.
        ui.add_space(4.0);
        ui.strong(tr!("issues-ui-thread"));
        let comments: Vec<Comment> = session
            .editor
            .doc()
            .issues
            .comments_of(id)
            .into_iter()
            .cloned()
            .collect();
        if comments.is_empty() {
            ui.small(tr!("issues-ui-no-comments"));
        }
        for c in &comments {
            ui.group(|ui| {
                let who = if c.author.is_empty() {
                    tr!("issues-ui-author-default")
                } else {
                    c.author.clone()
                };
                ui.small(tr!(
                    "issues-ui-comment-by",
                    author = who,
                    origin = origin_text(c.origin),
                    time = time_text(c.time)
                ));
                ui.label(&c.text);
                if c.edited.is_some() {
                    ui.small(tr!("issues-ui-comment-edited"));
                }
            });
        }
        ui.add(
            egui::TextEdit::multiline(&mut self.comment)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .char_limit(MAX_TEXT_CHARS)
                .hint_text(tr!("issues-ui-comment-hint")),
        );
        ui.horizontal(|ui| {
            let can = !self.comment.trim().is_empty();
            if ui
                .add_enabled(can, egui::Button::new(tr!("issues-ui-comment-send")))
                .clicked()
            {
                let text = std::mem::take(&mut self.comment);
                let r = call(
                    session,
                    "issue.comment",
                    json!({"id": id, "text": text, "author": author_name(), "now_ms": now_ms()}),
                );
                if let Err(e) = r {
                    *status = tr!("issues-ui-error", e = e.message);
                    self.comment = text;
                }
            }
            if !issue.status.is_done() {
                if ui.button(tr!("issues-ui-resolve")).clicked() {
                    let r = call(
                        session,
                        "issue.resolve",
                        json!({"id": id, "author": author_name(), "now_ms": now_ms()}),
                    );
                    if let Err(e) = r {
                        *status = tr!("issues-ui-error", e = e.message);
                    }
                }
            } else if ui.button(tr!("issues-ui-reopen")).clicked() {
                let r = call(
                    session,
                    "issue.update",
                    json!({"id": id, "status": "open", "now_ms": now_ms()}),
                );
                if let Err(e) = r {
                    *status = tr!("issues-ui-error", e = e.message);
                }
            }
            if ui.button(tr!("issues-ui-delete")).clicked() {
                self.ask_delete(session, issue);
            }
        });
    }

    fn viewpoint_row(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        status: &mut String,
        issue: &Issue,
        n: usize,
        vp: &Viewpoint,
    ) {
        ui.horizontal(|ui| {
            let name = if vp.name.trim().is_empty() {
                tr!("issues-ui-viewpoint-default-name", number = n + 1)
            } else {
                vp.name.clone()
            };
            if let Some(img) = vp
                .snapshot
                .as_deref()
                .and_then(|h| session.editor.doc().resources.find_image(h))
                .and_then(|r| session.editor.doc().resources.image(r))
                && let Some((tex, size)) = crate::picture_ui::resolve(ui.ctx(), img)
            {
                crate::picture_ui::paint_square(ui, &tex, size, 40.0);
            }
            if ui
                .button(tr!("issues-ui-viewpoint-go", name = name))
                .clicked()
            {
                self.selected = Some(issue.id);
                self.requests.push(Request::Open(issue.id, Some(vp.id)));
            }
            if ui.small_button(tr!("issues-ui-viewpoint-delete")).clicked() {
                let r = call(
                    session,
                    "execute",
                    json!({"command": {"op": "delete_issue_viewpoint", "issue": issue.id,
                        "viewpoint": vp.id, "now": now_ms()}}),
                );
                if let Err(e) = r {
                    *status = tr!("issues-ui-error", e = e.message);
                }
            }
        });
    }

    /// Asks before an issue goes. An issue of somebody else, or with comments of somebody else,
    /// has its own question that says why.
    fn ask_delete(&mut self, session: &Session, issue: &Issue) {
        let doc = session.editor.doc();
        let comments = doc.issues.comments_of(issue.id);
        let why = consent_reason(issue, &comments, session.editor.origin(), &author_name());
        self.confirm = Some(Confirm::Delete(issue.id, issue.title.clone(), why));
    }

    // ----- windows -----

    /// The window of a new issue, the questions and the BCF dialog.
    pub fn windows(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        self.new_issue_window(ctx, session);
        self.confirm_window(ctx, session, status);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(dialog) = &mut self.bcf
            && !dialog.show(ctx, session, status)
        {
            self.bcf = None;
        }
    }

    fn new_issue_window(&mut self, ctx: &egui::Context, session: &Session) {
        let Some(mut state) = self.new_issue.take() else {
            return;
        };
        let mut open = true;
        let mut create = false;
        let mut cancel = false;
        egui::Window::new(tr!("issues-ui-new-title"))
            .id(egui::Id::new("win-new-issue"))
            .open(&mut open)
            .default_width(380.0)
            .show(ctx, |ui| {
                let d = &mut state.draft;
                let intro = if !d.objects.is_empty() {
                    tr!("issues-ui-new-intro-objects", count = d.objects.len())
                } else if d.point.is_some() {
                    tr!("issues-ui-new-intro-point")
                } else {
                    tr!("issues-ui-new-intro-none")
                };
                ui.small(intro);
                egui::Grid::new("new-issue-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(tr!("issues-ui-field-title"));
                        ui.add(egui::TextEdit::singleline(&mut d.title).desired_width(260.0));
                        ui.end_row();
                        ui.label(tr!("issues-ui-field-description"));
                        ui.add(
                            egui::TextEdit::multiline(&mut d.description)
                                .desired_rows(3)
                                .desired_width(260.0),
                        );
                        ui.end_row();
                        ui.label(tr!("issues-ui-detail-type"));
                        egui::ComboBox::from_id_salt("new-type")
                            .selected_text(type_text(&d.kind))
                            .show_ui(ui, |ui| {
                                for v in issue::TYPES {
                                    ui.selectable_value(&mut d.kind, v.to_string(), type_text(v));
                                }
                            });
                        ui.end_row();
                        ui.label(tr!("issues-ui-detail-priority"));
                        egui::ComboBox::from_id_salt("new-priority")
                            .selected_text(priority_text(&d.priority))
                            .show_ui(ui, |ui| {
                                for v in issue::PRIORITIES {
                                    ui.selectable_value(
                                        &mut d.priority,
                                        v.to_string(),
                                        priority_text(v),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label(tr!("issues-ui-detail-assignee"));
                        ui.add(egui::TextEdit::singleline(&mut d.assignee).desired_width(260.0));
                        ui.end_row();
                        ui.label(tr!("issues-ui-detail-labels"));
                        ui.add(
                            egui::TextEdit::singleline(&mut d.labels)
                                .hint_text(tr!("issues-ui-field-labels-hint"))
                                .desired_width(260.0),
                        );
                        ui.end_row();
                        ui.label(tr!("issues-ui-detail-due"));
                        ui.add(
                            egui::TextEdit::singleline(&mut d.due)
                                .hint_text(tr!("issues-ui-field-due-hint"))
                                .desired_width(120.0),
                        );
                        ui.end_row();
                    });
                ui.checkbox(&mut d.with_viewpoint, tr!("issues-ui-keep-view"));
                ui.add_enabled(
                    d.with_viewpoint,
                    egui::Checkbox::new(&mut d.with_snapshot, tr!("issues-ui-keep-snapshot")),
                );
                if let Some(e) = &state.error {
                    ui.colored_label(Color32::from_rgb(200, 60, 40), e);
                }
                ui.horizontal(|ui| {
                    if ui.button(tr!("issues-ui-create")).clicked() {
                        match d.check() {
                            Ok(()) => create = true,
                            Err(e) => {
                                state.error = Some(match &e {
                                    logic::DraftError::Other(why) => translate(
                                        e.message(),
                                        &[("e", kadakas_l10n::Arg::Text(why.clone()))],
                                    ),
                                    other => translate(other.message(), &[]),
                                });
                            }
                        }
                    }
                    if ui.button(tr!("issues-ui-cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        let _ = session;
        if create {
            self.requests.push(Request::Create(state.draft));
        } else if open && !cancel {
            self.new_issue = Some(state);
        }
    }

    /// The person said yes to the question: the issue goes. An issue of somebody else goes with
    /// `confirm`, which only this answer sends.
    fn delete_confirmed(&mut self, session: &mut Session, status: &mut String) {
        let Some(Confirm::Delete(id, title, why)) = self.confirm.take() else {
            return;
        };
        let r = call(
            session,
            "issue.delete",
            json!({"id": id, "confirm": why.is_some(), "author": author_name()}),
        );
        match r {
            Ok(_) => *status = tr!("issues-ui-deleted", title = title),
            Err(e) => *status = tr!("issues-ui-error", e = e.message),
        }
        if self.selected == Some(id) {
            self.selected = None;
        }
    }

    fn confirm_window(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        let Some(Confirm::Delete(_, title, why)) = self.confirm.clone() else {
            return;
        };
        let mut done = false;
        egui::Window::new(tr!("issues-ui-delete"))
            .id(egui::Id::new("win-delete-issue"))
            .collapsible(false)
            .show(ctx, |ui| {
                match &why {
                    Some(why) => ui.label(tr!(
                        "issues-ui-confirm-delete-others",
                        title = title.clone(),
                        why = why.clone()
                    )),
                    None => ui.label(tr!("issues-ui-confirm-delete", title = title.clone())),
                };
                ui.horizontal(|ui| {
                    if ui.button(tr!("issues-ui-delete")).clicked() {
                        self.delete_confirmed(session, status);
                        done = true;
                    }
                    if ui.button(tr!("issues-ui-cancel")).clicked() {
                        done = true;
                    }
                });
            });
        if done {
            self.confirm = None;
        }
    }

    #[cfg(test)]
    pub fn push_request(&mut self, r: Request) {
        self.requests.push(r);
    }

    #[cfg(test)]
    pub fn pin_screen(&self, id: IssueId) -> Option<Pos2> {
        self.screens
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, p)| Pos2::new(p.x as f32, p.y as f32))
    }

    /// The question about a deletion that waits, and why the person is asked.
    #[cfg(test)]
    pub fn pending_delete(&self) -> Option<(IssueId, Option<String>)> {
        self.confirm
            .as_ref()
            .map(|Confirm::Delete(id, _, why)| (*id, why.clone()))
    }

    #[cfg(test)]
    pub fn ask_delete_for_test(&mut self, session: &Session, id: IssueId) {
        if let Some(i) = session.editor.doc().issues.get(id).cloned() {
            self.ask_delete(session, &i);
        }
    }

    #[cfg(test)]
    pub fn confirm_for_test(&mut self, session: &mut Session, status: &mut String) {
        self.delete_confirmed(session, status);
    }
}

/// A list to choose one value from, or none (all).
fn choose(ui: &mut Ui, id: &str, value: &mut Option<String>, values: &[String]) {
    let text = value.clone().unwrap_or_else(|| tr!("issues-ui-all"));
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(90.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(value.is_none(), tr!("issues-ui-all"))
                .clicked()
            {
                *value = None;
            }
            for v in values {
                if ui
                    .selectable_label(value.as_deref() == Some(v), v)
                    .clicked()
                {
                    *value = Some(v.clone());
                }
            }
        });
}

/// The JSON params of `issue.create` for a form and the view of the app.
pub fn create_params(
    draft: &Draft,
    view: Option<kadakas_core::issue::ViewSpec>,
    snapshot: Option<Value>,
) -> Value {
    let mut p = json!({
        "title": draft.title.trim(),
        "description": draft.description.trim(),
        "type": if draft.kind.trim().is_empty() { issue::DEFAULT_TYPE } else { draft.kind.trim() },
        "priority": if draft.priority.trim().is_empty() { issue::DEFAULT_PRIORITY } else { draft.priority.trim() },
        "labels": draft.label_list(),
        "objects": draft.objects,
        "author": author_name(),
        "now_ms": now_ms(),
    });
    if !draft.assignee.trim().is_empty() {
        p["assignee"] = json!(draft.assignee.trim());
    }
    if !draft.due.trim().is_empty() {
        p["due"] = json!(draft.due.trim());
    }
    if let Some(pt) = draft.point {
        p["point"] = json!(pt);
    }
    if draft.with_viewpoint
        && let Some(view) = view
    {
        let mut vp = json!({"view": view, "selection": draft.objects});
        if draft.with_snapshot
            && let Some(s) = snapshot
        {
            vp["snapshot"] = s;
        }
        p["viewpoint"] = vp;
    }
    p
}
