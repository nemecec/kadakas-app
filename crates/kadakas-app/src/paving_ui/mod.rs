//! The window part of the paving features (PAT-06 to PAT-12): Create Objects from Shapes, the
//! zones, courses and counts of a hardscape in Object Info, the Split Hardscape into Zones tool
//! and the pattern check. The logic is in `kadakas_ui_core::paving`; this module draws it and
//! feeds it the pointer. Every action is an API call through `Session::handle`, the same one an
//! agent uses, so the drawing, the report and the export see the same data.
//!
//! - `windows`: the dialogs (Create Objects from Shapes, Split, Check).
//! - `info`: the sections of Object Info (pattern, zones, courses, counts).
//! - `paint`: what the dialogs draw on the plan (previews, marks).
//! - `jobs`: the slow work (counts, zones, courses, the check) on a thread, by revision.

use glam::DVec2;
use kadakas_api::Session;
use kadakas_core::curve::Vertex;
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::resource::ResourceId;
use kadakas_l10n::tr;
use kadakas_ui_core::paving::check::CheckPanel;
use kadakas_ui_core::paving::counts::UnitCounts;
use kadakas_ui_core::paving::course::{CourseEditor, CourseRow};
use kadakas_ui_core::paving::curves::CurvesDialog;
use kadakas_ui_core::paving::split::{self, SplitDialog, ZoneEdit, ZoneSettings, ZoneShape};
use kadakas_ui_core::paving::{Backend, pick};
use serde_json::{Value, json};

mod backend;
mod info;
mod jobs;
mod paint;
mod windows;

pub use backend::{Scratch, SessionBackend};
pub use info::hardscape_section;
pub use paint::paint;

#[cfg(test)]
mod tests;

/// What a click in the drawing is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    /// The start point of the pattern of the hardscape.
    Origin,
    /// An edge of the outline: the pattern starts there and runs along it.
    AlignEdge,
    /// The start point of the pattern of a zone.
    ZoneOrigin(usize),
    /// An edge of a zone.
    ZoneAlignEdge(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub target: ObjectId,
    pub kind: PickKind,
    /// The revision of the document when the pick began. A pick of a zone ends when the document
    /// changes: the zone may be another one then.
    pub rev: u64,
}

impl Pick {
    fn wants_edge(self) -> bool {
        matches!(self.kind, PickKind::AlignEdge | PickKind::ZoneAlignEdge(_))
    }

    fn of_zone(self) -> bool {
        matches!(
            self.kind,
            PickKind::ZoneOrigin(_) | PickKind::ZoneAlignEdge(_)
        )
    }
}

/// A request for the rest of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Open the joint pattern editor of a hardscape style.
    EditJoints(ResourceId),
}

/// One copy of the document per revision, for the jobs of one frame (counts, zones, courses and
/// the check read it on their threads; each would otherwise copy the whole document).
#[derive(Default)]
pub struct SharedDoc(std::cell::RefCell<Option<(u64, std::sync::Arc<Document>)>>);

impl SharedDoc {
    pub fn get(&self, doc: &Document, revision: u64) -> std::sync::Arc<Document> {
        let mut c = self.0.borrow_mut();
        match &*c {
            Some((r, d)) if *r == revision => d.clone(),
            _ => {
                let d = std::sync::Arc::new(doc.clone());
                *c = Some((revision, d.clone()));
                d
            }
        }
    }

    /// How many copies were made (for the tests): the revision of the one held.
    #[cfg(test)]
    pub fn held(&self) -> Option<u64> {
        self.0.borrow().as_ref().map(|(r, _)| *r)
    }
}

#[derive(Default)]
pub struct PavingUi {
    pub(crate) shared: SharedDoc,
    pub(crate) curves: Option<CurvesDialog>,
    pub(crate) split: Option<SplitDialog>,
    /// Clicks of the plan add points to the split line.
    pub(crate) drawing: bool,
    pub(crate) check: CheckPanel,
    pub(crate) check_open: bool,
    pub(crate) check_job: jobs::Job<Value>,
    /// The check starts in the next frame of its window.
    pub(crate) check_start: bool,
    pub(crate) pick: Option<Pick>,
    /// The pointer on the plan, for the rubber band and the edge under it.
    pub(crate) hover: DVec2,
    pub(crate) course: Option<CourseEditor>,
    pub(crate) zone_edit: Option<ZoneEdit>,
    pub(crate) zones: jobs::Job<Vec<ZoneShape>>,
    pub(crate) courses: jobs::Job<Vec<CourseRow>>,
    pub(crate) counts: jobs::Job<UnitCounts>,
    /// A hardscape whose units are counted although it is big: the person asked.
    pub(crate) count_asked: Option<ObjectId>,
    /// The hardscape whose split lines the person is asked to remove.
    pub(crate) confirm_clear: Option<ObjectId>,
    /// The zone of Object Info that is open.
    pub(crate) open_zone: Option<usize>,
    /// Why the document cannot be changed now (read only, or the first snapshot of a shared
    /// document is on its way). Set in each frame by the app.
    lock: Option<String>,
    zoom: Option<(DVec2, DVec2)>,
    requests: Vec<Request>,
}

impl PavingUi {
    /// Forgets everything (a new document).
    pub fn reset(&mut self) {
        *self = PavingUi::default();
    }

    /// Called in each frame by the app: the lock, and the picks that the document left behind.
    pub fn frame(&mut self, revision: u64, lock: Option<String>, status: &mut String) {
        self.lock = lock;
        if self.pick.is_some_and(|p| p.of_zone() && p.rev != revision) {
            self.pick = None;
            *status = tr!("paving-ui-the-drawing-changed-the-pick-ended");
        }
    }

    /// True (with a message in the status line) when the document cannot be changed now. Every
    /// action that changes it asks first, as the app does for its own commands.
    pub(crate) fn blocked(&self, status: &mut String) -> bool {
        match &self.lock {
            Some(m) => {
                *status = m.clone();
                true
            }
            None => false,
        }
    }

    /// Asks the plan to show this box.
    pub(crate) fn zoom_to(&mut self, lo: DVec2, hi: DVec2) {
        self.zoom = Some((lo, hi));
    }

    /// A box to zoom to, once.
    pub fn take_zoom(&mut self) -> Option<(DVec2, DVec2)> {
        self.zoom.take()
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn request(&mut self, r: Request) {
        self.requests.push(r);
    }

    /// Landscape > Create Objects from Shapes.
    pub fn open_curves(&mut self, session: &mut Session, status: &mut String) {
        match CurvesDialog::open(&SessionBackend(session)) {
            Ok(d) => self.curves = Some(d),
            Err(e) => *status = e,
        }
    }

    /// Landscape > Split Hardscape into Zones: on the selected hardscape.
    pub fn open_split(&mut self, session: &mut Session, status: &mut String) {
        let doc = session.editor.doc();
        let target = session.selection.iter().copied().find(|id| {
            doc.object(*id)
                .is_some_and(|o| matches!(o.shape, Shape::Hardscape { .. }))
        });
        match target {
            Some(id) => self.open_split_for(session, id, status),
            None => *status = tr!("paving-ui-select-a-hardscape"),
        }
    }

    pub fn open_split_for(&mut self, session: &mut Session, id: ObjectId, status: &mut String) {
        match SplitDialog::open(&SessionBackend(session), id) {
            Ok(d) => {
                self.split = Some(d);
                self.drawing = true;
                self.pick = None;
            }
            Err(e) => *status = e,
        }
    }

    /// Landscape > Check Unit Patterns: opens the panel and runs the check on the selected
    /// hardscapes (all of them when none is selected).
    pub fn run_check(&mut self, session: &mut Session) {
        self.check_open = true;
        self.check.set_scope(&SessionBackend(session));
        self.check_start = true;
    }

    /// The windows of the dialogs, in each frame.
    pub fn windows(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        windows::curves(self, ctx, session, status);
        windows::split(self, ctx, session, status);
        windows::check_window(self, ctx, session, status);
    }

    /// True while a click on the plan belongs to this module.
    pub fn captures(&self) -> bool {
        self.pick.is_some() || (self.split.is_some() && self.drawing)
    }

    /// The pointer is over the plan at `p`.
    pub fn set_hover(&mut self, p: DVec2) {
        self.hover = p;
    }

    /// A press on the plan at `p` (snapped). `tol` is the pick distance in mm. Returns true if the
    /// press was used.
    pub fn press(
        &mut self,
        session: &mut Session,
        p: DVec2,
        tol: f64,
        status: &mut String,
    ) -> bool {
        if let Some(pick) = self.pick {
            if self.blocked(status) {
                self.pick = None;
                return true;
            }
            self.finish_pick(session, pick, p, tol, status);
            return true;
        }
        if let Some(d) = &mut self.split
            && self.drawing
        {
            d.draft.click(p);
            return true;
        }
        false
    }

    /// A double click on the plan ends the split line that is being drawn.
    pub fn double_click(&mut self) -> bool {
        match &mut self.split {
            Some(d) if self.drawing => {
                // The first click of the double click added the point already.
                d.draft.finish_line();
                true
            }
            _ => false,
        }
    }

    /// Enter ends the line.
    pub fn finish_line(&mut self) -> bool {
        match &mut self.split {
            Some(d) if self.drawing => d.draft.finish_line(),
            _ => false,
        }
    }

    /// Escape: ends a pick, the line that is being drawn, or the drawing of lines. Returns true if
    /// it did something.
    pub fn cancel(&mut self) -> bool {
        if self.pick.take().is_some() {
            return true;
        }
        if let Some(d) = &mut self.split
            && self.drawing
        {
            if !d.draft.open.is_empty() {
                d.draft.open.clear();
            } else {
                self.drawing = false;
            }
            return true;
        }
        false
    }

    /// Starts a pick. Another one, and the drawing of split lines, stop.
    pub(crate) fn begin_pick(
        &mut self,
        session: &Session,
        target: ObjectId,
        kind: PickKind,
        status: &mut String,
    ) {
        if self.blocked(status) {
            return;
        }
        let pick = Pick {
            target,
            kind,
            rev: session.editor.revision(),
        };
        self.pick = Some(pick);
        self.drawing = false;
        *status = if pick.wants_edge() {
            tr!("paving-ui-click-an-edge")
        } else {
            tr!("paving-ui-click-a-point")
        };
    }

    fn finish_pick(
        &mut self,
        session: &mut Session,
        pick: Pick,
        p: DVec2,
        tol: f64,
        status: &mut String,
    ) {
        match apply_pick(session, pick, p, tol) {
            Ok(true) => {
                self.pick = None;
                *status = tr!("paving-ui-done");
            }
            // No edge near the click: stay in the pick and say so.
            Ok(false) => *status = tr!("paving-ui-no-edge-there"),
            Err(e) => {
                self.pick = None;
                *status = tr!("paving-ui-error", e = e);
            }
        }
    }
}

/// The outline of a hardscape.
pub(crate) fn outline_of(doc: &Document, id: ObjectId) -> Option<Vec<Vertex>> {
    match &doc.object(id)?.shape {
        Shape::Hardscape { boundary, .. } => Some(boundary.clone()),
        _ => None,
    }
}

/// Does what a click picked, through the API. `Ok(false)`: nothing was near enough.
fn apply_pick(session: &mut Session, pick: Pick, p: DVec2, tol: f64) -> Result<bool, String> {
    let max_units = session.limits().pattern_units;
    let doc = session.editor.doc();
    match pick.kind {
        PickKind::Origin => {
            SessionBackend(session).call(
                "hardscape.set",
                json!({"id": pick.target, "origin": [p.x, p.y]}),
            )?;
            Ok(true)
        }
        PickKind::AlignEdge => {
            let outline =
                outline_of(doc, pick.target).ok_or_else(|| tr!("paving-ui-select-a-hardscape"))?;
            let Some(n) = pick::nearest_edge(&outline, p, tol) else {
                return Ok(false);
            };
            SessionBackend(session).call(
                "hardscape.set",
                json!({"id": pick.target, "align_to_edge": n}),
            )?;
            Ok(true)
        }
        PickKind::ZoneOrigin(zi) | PickKind::ZoneAlignEdge(zi) => {
            let zones = split::zone_shapes(doc, pick.target, max_units)?;
            let zone = zones.get(zi).ok_or_else(|| tr!("paving-ui-no-such-zone"))?;
            let o = doc
                .object(pick.target)
                .ok_or_else(|| tr!("paving-ui-select-a-hardscape"))?;
            let base = ZoneSettings::read(doc, o, zone);
            let mut edit = ZoneEdit::new(pick.target, zi, base);
            match pick.kind {
                PickKind::ZoneOrigin(_) => edit.settings.origin = Some(p),
                _ => {
                    let ring = zone
                        .shapes
                        .first()
                        .and_then(|sh| sh.first())
                        .ok_or_else(|| tr!("paving-ui-no-such-zone"))?;
                    let Some(n) = pick::nearest_ring_edge(ring, p, tol) else {
                        return Ok(false);
                    };
                    edit.settings.align_edge(n);
                }
            }
            split::store_zone(&mut SessionBackend(session), &edit)?;
            Ok(true)
        }
    }
}
