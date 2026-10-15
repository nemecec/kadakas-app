//! The logic of the issue UI (REV-05), without egui: what the Issues palette lists, the form of a
//! new issue, the pins on the plan and in 3D, and what opening an issue does.
//!
//! The data and the commands are in `kadakas_core::issue`, the filters in
//! `kadakas_review::issues`. A UI program draws what this module says and runs the commands that
//! it returns, so every change is one undo step and works in collaboration.
//!
//! - [`Filters`] and [`rows`]: the list of the palette (status, assignee, label, text).
//! - [`pins`], [`pick_pin`] and [`badge`]: where the pins are, which one a click hits, and the count
//!   on a pin that is about many objects. [`status_rgb`] gives the colors; they are from the
//!   Okabe and Ito palette, which stays apart for the common kinds of color blindness, and each
//!   status also has its own mark ([`status_mark`]), so color is never the only sign.
//! - [`Draft`]: the form of a new issue, from the selection, a click or the context menu.
//! - [`capture`] and [`open_plan`]: a viewpoint from the current view, and the commands, the view
//!   and the selection that opening a viewpoint needs.
//! - [`plan_spec`] and [`plan_view`]: the plan view as a viewpoint view and back.

use std::collections::BTreeSet;

use glam::DVec2;
use kadakas_core::change::Origin;
use kadakas_core::command::Command;
use kadakas_core::issue::{
    self, Issue, IssueId, IssueStatus, MAX_LABEL_CHARS, MAX_LABELS, MAX_TITLE_CHARS, ViewSpec,
    Viewpoint, ViewpointId,
};
use kadakas_core::model::{Document, ObjectId};
use kadakas_l10n::msg;
use kadakas_review::issues::{Filter, capture_visibility, pin_position, select, time};

use crate::view::{MAX_ZOOM, MIN_ZOOM, View};

/// The color of a status, in red, green and blue.
pub fn status_rgb(s: IssueStatus) -> [u8; 3] {
    match s {
        IssueStatus::Open => [213, 94, 0],
        IssueStatus::InProgress => [0, 114, 178],
        IssueStatus::Resolved => [0, 158, 115],
        IssueStatus::Closed => [120, 120, 120],
    }
}

/// The mark inside a pin, so a status has a sign that is not a color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A dot.
    Dot,
    /// Half a disc.
    Half,
    /// A check.
    Check,
    /// A bar.
    Bar,
}

pub fn status_mark(s: IssueStatus) -> Mark {
    match s {
        IssueStatus::Open => Mark::Dot,
        IssueStatus::InProgress => Mark::Half,
        IssueStatus::Resolved => Mark::Check,
        IssueStatus::Closed => Mark::Bar,
    }
}

/// The id of the message for a status, in `uic-issues.ftl`.
pub fn status_message(s: IssueStatus) -> &'static str {
    match s {
        IssueStatus::Open => msg!("uic-issues-status-open"),
        IssueStatus::InProgress => msg!("uic-issues-status-in-progress"),
        IssueStatus::Resolved => msg!("uic-issues-status-resolved"),
        IssueStatus::Closed => msg!("uic-issues-status-closed"),
    }
}

// ----- the list -----

/// What the palette shows. No status chosen shows all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filters {
    pub statuses: BTreeSet<IssueStatus>,
    pub assignee: Option<String>,
    pub label: Option<String>,
    pub text: String,
    /// Show the resolved and closed issues too (when no status is chosen).
    pub show_done: bool,
}

impl Filters {
    pub fn to_filter(&self) -> Filter {
        Filter {
            status: self.statuses.iter().copied().collect(),
            assignee: self.assignee.clone(),
            label: self.label.clone(),
            text: Some(self.text.trim().to_string()).filter(|t| !t.is_empty()),
            open_only: self.statuses.is_empty() && !self.show_done,
            ..Filter::default()
        }
    }

    /// True if some filter is on.
    pub fn is_active(&self) -> bool {
        !self.statuses.is_empty()
            || self.assignee.is_some()
            || self.label.is_some()
            || !self.text.trim().is_empty()
            || self.show_done
    }

    pub fn toggle_status(&mut self, s: IssueStatus) {
        if !self.statuses.remove(&s) {
            self.statuses.insert(s);
        }
    }

    pub fn clear(&mut self) {
        *self = Filters::default();
    }
}

/// How close a due date is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    None,
    Later,
    /// In three days or less.
    Soon,
    Overdue,
}

/// The state of a due date at a time. A finished issue is never late.
pub fn due_state(due: Option<&str>, status: IssueStatus, now_ms: i64) -> Due {
    let Some(start) = due.and_then(time::from_date) else {
        return Due::None;
    };
    if status.is_done() {
        return Due::Later;
    }
    let end = start + 86_400_000;
    if now_ms >= end {
        Due::Overdue
    } else if now_ms >= start - 2 * 86_400_000 {
        Due::Soon
    } else {
        Due::Later
    }
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: IssueId,
    pub title: String,
    pub status: IssueStatus,
    pub priority: String,
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    pub comments: usize,
    pub objects: usize,
    pub due: Option<String>,
    pub due_state: Due,
    pub origin: Origin,
    pub modified: i64,
}

/// The lines of the list: issues that are not done first, then by the time of the last change,
/// newest first.
pub fn rows(doc: &Document, filters: &Filters, now_ms: i64) -> Vec<Row> {
    let mut list = select(&doc.issues, &filters.to_filter());
    list.sort_by_key(|i| (i.status.is_done(), std::cmp::Reverse(i.modified), i.id));
    list.into_iter()
        .map(|i| Row {
            id: i.id,
            title: i.title.clone(),
            status: i.status,
            priority: i.priority.clone(),
            assignee: i.assignee.clone(),
            labels: i.labels.clone(),
            comments: doc.issues.comment_count(i.id),
            objects: i.objects.len(),
            due: i.due.clone(),
            due_state: due_state(i.due.as_deref(), i.status, now_ms),
            origin: i.origin,
            modified: i.modified,
        })
        .collect()
}

// ----- pins -----

/// A pin on the plan or in 3D.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub id: IssueId,
    /// The model point (mm).
    pub at: [f64; 3],
    pub status: IssueStatus,
    /// How many objects the issue is about.
    pub objects: usize,
}

/// The pins of the issues that match. An issue shows where its point is, else at its first
/// object, else at the center of its first plan viewpoint. One that has none of these has no pin.
pub fn pins(doc: &Document, filters: &Filters) -> Vec<Pin> {
    let position = |o: ObjectId| {
        doc.object(o).map(|ob| {
            let (lo, hi) = ob.shape.bounds();
            let c = (lo + hi) / 2.0;
            let z = doc.layer(ob.layer).map_or(0.0, |l| l.elevation);
            [c.x, c.y, z]
        })
    };
    select(&doc.issues, &filters.to_filter())
        .into_iter()
        .filter_map(|i| {
            pin_position(i, &position).map(|at| Pin {
                id: i.id,
                at,
                status: i.status,
                objects: i.objects.len(),
            })
        })
        .collect()
}

/// The text on a pin that is about many objects: the count, up to `99+`.
pub fn badge(objects: usize) -> Option<String> {
    match objects {
        0 | 1 => None,
        2..=99 => Some(objects.to_string()),
        _ => Some("99+".into()),
    }
}

/// The pin that a click at `p` (px) hits: the nearest of the pins within `radius` px. `screens`
/// gives the position of each pin on the screen.
pub fn pick_pin(screens: &[(IssueId, DVec2)], p: DVec2, radius: f64) -> Option<IssueId> {
    screens
        .iter()
        .map(|(id, s)| (*id, s.distance(p)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

// ----- the form of a new issue -----

/// The form of a new issue. The fields are what the person typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub title: String,
    pub description: String,
    pub kind: String,
    pub priority: String,
    pub assignee: String,
    /// Labels, separated by commas.
    pub labels: String,
    /// `YYYY-MM-DD`, or empty.
    pub due: String,
    pub objects: Vec<ObjectId>,
    pub point: Option<[f64; 3]>,
    /// Keep a viewpoint of the current view with the issue.
    pub with_viewpoint: bool,
    /// Keep a snapshot picture of the view with the viewpoint.
    pub with_snapshot: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Draft {
            title: String::new(),
            description: String::new(),
            kind: issue::DEFAULT_TYPE.into(),
            priority: issue::DEFAULT_PRIORITY.into(),
            assignee: String::new(),
            labels: String::new(),
            due: String::new(),
            objects: Vec::new(),
            point: None,
            with_viewpoint: true,
            with_snapshot: true,
        }
    }
}

/// What is wrong with a form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftError {
    TitleEmpty,
    TitleTooLong,
    DueNotDate,
    TooManyLabels,
    LabelTooLong,
    /// A rule of the core that the form does not know. The text is English.
    Other(String),
}

impl DraftError {
    /// The id of the message in `uic-issues.ftl`. The `Other` message takes the English text of
    /// the core as `{ $e }`.
    pub fn message(&self) -> &'static str {
        match self {
            DraftError::TitleEmpty => msg!("uic-issues-error-title-empty"),
            DraftError::TitleTooLong => msg!("uic-issues-error-title-long"),
            DraftError::DueNotDate => msg!("uic-issues-error-due"),
            DraftError::TooManyLabels => msg!("uic-issues-error-labels"),
            DraftError::LabelTooLong => msg!("uic-issues-error-label-long"),
            DraftError::Other(_) => msg!("uic-issues-error-other"),
        }
    }
}

impl Draft {
    /// A form for the selected objects. The pin goes to the first of them.
    pub fn for_objects(objects: &[ObjectId]) -> Draft {
        Draft {
            objects: objects.to_vec(),
            ..Draft::default()
        }
    }

    /// A form for a point of the model (a click).
    pub fn at_point(p: [f64; 3]) -> Draft {
        Draft {
            point: Some(p),
            ..Draft::default()
        }
    }

    /// The labels of the form: split at commas, trimmed, no empty ones, no repeats.
    pub fn label_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in self.labels.split(',') {
            let l = l.trim();
            if !l.is_empty() && !out.iter().any(|x| x.to_lowercase() == l.to_lowercase()) {
                out.push(l.to_string());
            }
        }
        out
    }

    /// Checks the form.
    pub fn check(&self) -> Result<(), DraftError> {
        if self.title.trim().is_empty() {
            return Err(DraftError::TitleEmpty);
        }
        if self.title.chars().count() > MAX_TITLE_CHARS {
            return Err(DraftError::TitleTooLong);
        }
        let due = self.due.trim();
        if !due.is_empty() && !issue::is_date(due) {
            return Err(DraftError::DueNotDate);
        }
        let labels = self.label_list();
        if labels.len() > MAX_LABELS {
            return Err(DraftError::TooManyLabels);
        }
        if labels.iter().any(|l| l.chars().count() > MAX_LABEL_CHARS) {
            return Err(DraftError::LabelTooLong);
        }
        self.issue("", Origin::User, 0, vec![])
            .validate()
            .map_err(DraftError::Other)
    }

    /// The issue of the form.
    pub fn issue(
        &self,
        author: &str,
        origin: Origin,
        now_ms: i64,
        viewpoints: Vec<Viewpoint>,
    ) -> Issue {
        let text = |t: &str, default: &str| {
            let t = t.trim();
            if t.is_empty() { default } else { t }.to_string()
        };
        Issue {
            title: self.title.trim().to_string(),
            description: self.description.trim().to_string(),
            kind: text(&self.kind, issue::DEFAULT_TYPE),
            priority: text(&self.priority, issue::DEFAULT_PRIORITY),
            assignee: Some(self.assignee.trim().to_string()).filter(|a| !a.is_empty()),
            labels: self.label_list(),
            due: Some(self.due.trim().to_string()).filter(|d| !d.is_empty()),
            author: author.to_string(),
            origin,
            created: now_ms,
            modified: now_ms,
            point: self.point,
            objects: self.objects.clone(),
            viewpoints,
            ..Issue::default()
        }
    }
}

// ----- viewpoints -----

/// A viewpoint of the current view: the view, the selection and the visible layers and classes.
pub fn capture(
    doc: &Document,
    view: ViewSpec,
    selection: &[ObjectId],
    now_ms: i64,
    name: &str,
) -> Viewpoint {
    let (layers, classes) = capture_visibility(doc);
    Viewpoint {
        name: name.to_string(),
        time: now_ms,
        view: Some(view),
        layers,
        classes,
        selection: selection.to_vec(),
        ..Viewpoint::default()
    }
}

/// What opening an issue does.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenPlan {
    /// The commands that restore the visible layers and classes (one undo step when run as a
    /// batch). Empty when nothing differs.
    pub commands: Vec<Command>,
    /// The view to go to, if the viewpoint has one.
    pub view: Option<ViewSpec>,
    /// The objects to select: those of the viewpoint (else those of the issue) that are in the
    /// document.
    pub selection: Vec<ObjectId>,
    /// How many objects of the issue are not in the document.
    pub missing: usize,
}

/// The viewpoint that opening an issue uses: the one asked for, else the first with a view, else
/// the first.
pub fn viewpoint_of(issue: &Issue, id: Option<ViewpointId>) -> Option<&Viewpoint> {
    id.and_then(|id| issue.viewpoints.iter().find(|v| v.id == id))
        .or_else(|| issue.viewpoints.iter().find(|v| v.view.is_some()))
        .or_else(|| issue.viewpoints.first())
}

/// What to do to open an issue with a viewpoint (or without one: then the objects of the issue
/// are selected and nothing else changes).
pub fn open_plan(doc: &Document, issue: &Issue, vp: Option<&Viewpoint>) -> OpenPlan {
    let wanted: &[ObjectId] = match vp {
        Some(v) if !v.selection.is_empty() => &v.selection,
        _ => &issue.objects,
    };
    let selection: Vec<ObjectId> = wanted
        .iter()
        .copied()
        .filter(|o| doc.object(*o).is_some())
        .collect();
    let missing = issue
        .objects
        .iter()
        .filter(|o| doc.object(**o).is_none())
        .count();
    OpenPlan {
        commands: vp.map_or_else(Vec::new, |v| {
            kadakas_review::issues::visibility_commands(doc, v)
        }),
        view: vp.and_then(|v| v.view.clone()),
        selection,
        missing,
    }
}

/// The plan view as a viewpoint view: the model point in the middle of the screen rectangle and
/// the zoom.
pub fn plan_spec(view: &View, rect_min: DVec2, rect_max: DVec2) -> ViewSpec {
    let c = view.model_of((rect_min + rect_max) / 2.0);
    ViewSpec::Plan {
        center: [c.x, c.y],
        zoom: view.zoom,
    }
}

/// The plan view that shows a viewpoint view. `None` for a camera.
pub fn plan_view(spec: &ViewSpec, rect_min: DVec2, rect_max: DVec2) -> Option<View> {
    let ViewSpec::Plan { center, zoom } = spec else {
        return None;
    };
    let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let mid = (rect_min + rect_max) / 2.0;
    Some(View {
        origin: DVec2::new(mid.x - center[0] * zoom, mid.y + center[1] * zoom),
        zoom,
    })
}

#[cfg(test)]
mod tests;
