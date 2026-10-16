//! The pattern plan of a hardscape on the canvas: the joint lines of every group (the field, the
//! zones, the courses), the cut units with a light fill, the zone edges, the outlines of courses and,
//! on request, the findings of the pattern check.
//!
//! The plan comes from `kadakas_core::hardscape::draw`, the same source as `render.svg`, the sheets
//! and the PDF, laid by the same engine as the report and the DXF export. The rules of the canvas:
//!
//! - It never waits. A plan is laid, and a check is made, by the background worker
//!   ([`crate::plan_worker`]); until the new plan is ready the canvas draws the last one it had and
//!   says "updating". A check shows only the findings of the plan that is on the screen.
//! - It does not lay again what did not change. Each object keeps the plan of its content key, and
//!   the part of it that it drew last: a window round the visible part, so a pan or a small zoom
//!   costs nothing. The window is drawn again when the view leaves it, when the view got much
//!   smaller than it, or when a group of units changes between visible and hidden.
//! - It says what it simplifies. Above the bound of units the drawing is a preview (the whole plan
//!   as gap lines) with the label "preview: zoom in for the real pattern"; a group whose units are
//!   smaller than 3 px is hidden with a label, but the zone edges, the outlines of courses and the
//!   findings are always drawn.
//! - A compare draws the plan of the old object (a ghost) with its own place, so it never takes the
//!   plan of the live object that has the same ID.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Painter, Pos2, Stroke};
use glam::DVec2;
use kadakas_core::hardscape::draw::{self, BBox, DrawOptions, Drawn, Prepared};
use kadakas_core::layout::check::Kind;
use kadakas_core::model::{Document, Object, ObjectId};
use kadakas_l10n::tr;

use crate::plan_worker::{Slot, slot as slot_of, worker};
use crate::view::{View, ViewExt as _};

/// How far the window reaches round the visible part, as a share of its size.
const MARGIN: f64 = 0.25;
/// A window that is more than this many times the area of what is visible is drawn again.
const MAX_WINDOW_RATIO: f64 = 9.0;
/// A preview is looked at again when the visible part got this much smaller (area): it may hold
/// few enough units for the real ones.
const PREVIEW_SHRINK: f64 = 0.85;
/// Units smaller than this on the screen (px) are hidden: they would only make a gray area.
pub const MIN_UNIT_PX: f64 = 3.0;
/// The most memory that the kept plans of the canvas hold (estimated, bytes).
const HELD_BYTES: usize = 512 * 1024 * 1024;

/// What an object keeps between frames.
#[derive(Default)]
struct Held {
    /// The content key of the plan that `prepared` is for.
    key: u64,
    prepared: Option<Arc<Prepared>>,
    /// Why the plan of `key` cannot be laid.
    error: Option<String>,
    /// The key that the worker is asked for.
    asked: Option<u64>,
    /// The key of the plan that the check is asked for.
    check_asked: Option<u64>,
    /// What was drawn last, and for which plan, window and zoom.
    drawn: Option<Arc<Drawn>>,
    drawn_key: u64,
    window: Option<BBox>,
    need_area: f64,
    /// The window that the worker is asked to cut: the request, and what it is for.
    cut_asked: Option<(u64, BBox, f64)>,
    /// In the browser the check runs in the window, and the page pauses: the label is painted one
    /// frame before the check starts.
    check_deferred: bool,
    /// The frame that used this last.
    used: u64,
}

impl Held {
    fn bytes(&self) -> usize {
        self.prepared.as_ref().map_or(0, |p| p.approx_bytes()) + 2048
    }
}

thread_local! {
    /// Tests that measure set the bound of the window; `None`: [`draw::CANVAS_BOUND`].
    static BOUND: Cell<Option<usize>> = const { Cell::new(None) };
    static HELD: RefCell<HashMap<Slot, Held>> = RefCell::new(HashMap::new());
    /// The content key of an object for a document revision.
    static KEYS: RefCell<HashMap<ObjectId, (u64, usize, u64)>> = RefCell::new(HashMap::new());
    /// The revision of the document that the entries were last compared with the objects of.
    static PRUNED: Cell<Option<u64>> = const { Cell::new(None) };
    static GHOST: Cell<bool> = const { Cell::new(false) };
    static SHOW_CHECK: Cell<bool> = const { Cell::new(false) };
    /// How often a window was drawn again (tests count the work).
    static REDRAWS: Cell<u64> = const { Cell::new(0) };
}

/// Most units in the window for the real units.
fn bound() -> usize {
    BOUND.with(Cell::get).unwrap_or(draw::CANVAS_BOUND)
}

/// Most gap lines in the window for the real units.
fn max_segments() -> usize {
    BOUND.with(Cell::get).unwrap_or(draw::CANVAS_MAX_SEGMENTS)
}

/// Sets the bound of the window (`None`: the default). For measurements and tests.
#[cfg(test)]
pub fn set_bound(bound: Option<usize>) {
    BOUND.with(|b| b.set(bound));
}

/// The objects that are drawn now are ghosts of a compare: the old objects. A ghost has the ID of
/// the live object and its own plan.
pub fn set_ghost(ghost: bool) {
    GHOST.with(|g| g.set(ghost));
}

/// Show the findings of the pattern check on the plan.
pub fn show_check() -> bool {
    SHOW_CHECK.with(Cell::get)
}

pub fn set_show_check(on: bool) {
    SHOW_CHECK.with(|s| s.set(on));
    if !on {
        // The checks that wait or run are not wanted.
        HELD.with(|h| {
            for (slot, held) in h.borrow_mut().iter_mut() {
                worker().cancel_check(*slot);
                held.check_asked = None;
            }
        });
    }
}

/// How often a window was laid and cut (not taken from what the object drew before).
#[cfg(test)]
pub fn redraws() -> u64 {
    REDRAWS.with(Cell::get)
}

/// The content key of the plan that the canvas keeps for a place.
#[cfg(test)]
pub fn held_key(id: ObjectId, ghost: bool) -> Option<u64> {
    HELD.with(|h| h.borrow().get(&slot_of(id, ghost)).map(|e| e.key))
}

/// The plan that the canvas keeps for a place.
#[cfg(test)]
pub fn held_prepared(id: ObjectId, ghost: bool) -> Option<Arc<Prepared>> {
    HELD.with(|h| {
        h.borrow()
            .get(&slot_of(id, ghost))
            .and_then(|e| e.prepared.clone())
    })
}

/// How many objects the canvas keeps a plan for.
#[cfg(test)]
pub fn held() -> usize {
    HELD.with(|h| h.borrow().len())
}

fn area(b: BBox) -> f64 {
    ((b.1.x - b.0.x) * (b.1.y - b.0.y)).max(0.0)
}

fn contains(outer: BBox, inner: BBox) -> bool {
    outer.0.x <= inner.0.x
        && outer.0.y <= inner.0.y
        && outer.1.x >= inner.1.x
        && outer.1.y >= inner.1.y
}

fn meet(a: BBox, b: BBox) -> bool {
    a.1.x >= b.0.x && a.0.x <= b.1.x && a.1.y >= b.0.y && a.0.y <= b.1.y
}

fn bbox(ring: &[DVec2]) -> BBox {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in ring {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo, hi)
}

/// The content key of the plan of an object. With a revision of the document it is made once for
/// the revision (the same revision names the same document state). A ghost is of another document
/// than the one the revision names, so its key is made every time.
fn key_of(doc: &Document, o: &Object, revision: Option<u64>, ghost: bool) -> u64 {
    if !ghost
        && let Some(rev) = revision
        && let Some(k) = KEYS.with(|k| {
            k.borrow()
                .get(&o.id)
                .filter(|(r, units, _)| *r == rev && *units == draw::pattern_units())
                .map(|e| e.2)
        })
    {
        return k;
    }
    let key = draw::content_key(doc, o, kadakas_core::layout::TOLERANCE);
    if !ghost && let Some(rev) = revision {
        KEYS.with(|k| {
            k.borrow_mut()
                .insert(o.id, (rev, draw::pattern_units(), key));
        });
    }
    key
}

/// Forgets what the canvas kept for objects that are gone, once for each revision of the document,
/// and the oldest plans when they hold too much.
fn prune(doc: &Document, revision: Option<u64>, frame: u64) {
    if revision.is_some() && PRUNED.with(Cell::get) != revision {
        PRUNED.with(|p| p.set(revision));
        HELD.with(|h| {
            h.borrow_mut().retain(|(id, ghost, _), held| {
                if *ghost {
                    // A ghost not drawn in the last frames is gone with the compare.
                    held.used + 2 >= frame
                } else {
                    doc.object(*id).is_some()
                }
            })
        });
        KEYS.with(|k| k.borrow_mut().retain(|id, _| doc.object(*id).is_some()));
    }
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let mut total: usize = h.values().map(Held::bytes).sum();
        if total <= HELD_BYTES {
            return;
        }
        // The least recently used go first; what the last frame drew stays.
        let mut order: Vec<(u64, Slot)> = h.iter().map(|(s, e)| (e.used, *s)).collect();
        order.sort_by_key(|e| e.0);
        for (used, slot) in order {
            if total <= HELD_BYTES || used + 1 >= frame {
                break;
            }
            if let Some(e) = h.remove(&slot) {
                total = total.saturating_sub(e.bytes());
            }
        }
    });
}

fn finding_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Overlap | Kind::OutsideArea => Color32::from_rgb(220, 30, 30),
        Kind::JointTooNarrow | Kind::JointTooWide => Color32::from_rgb(235, 130, 20),
        Kind::SmallPiece | Kind::WrongSize => Color32::from_rgb(160, 40, 200),
        Kind::Coverage => Color32::from_rgb(20, 110, 220),
    }
}

/// What the plan of an object is, this frame.
enum Plan {
    /// The plan of the current content, ready.
    Ready(Arc<Prepared>),
    /// The plan is being laid; the last one (if any) is drawn meanwhile.
    Laying,
    /// The plan of the current content cannot be laid.
    Failed(String),
}

/// Finds the plan for `key` in the entry, the shared cache or the worker.
fn plan_for(
    held: &mut Held,
    slot: Slot,
    doc: &Document,
    o: &Object,
    key: u64,
    ctx: &egui::Context,
) -> Plan {
    if held.key == key {
        if let Some(p) = &held.prepared {
            return Plan::Ready(p.clone());
        }
        if let Some(e) = &held.error {
            return Plan::Failed(e.clone());
        }
    }
    let mut got = draw::cached(key);
    if got.is_none()
        && held.asked == Some(key)
        && let Some(done) = worker().take(slot, key)
    {
        got = Some(done.plan);
    }
    match got {
        Some(Ok(p)) => {
            held.key = key;
            held.prepared = Some(p.clone());
            held.error = None;
            held.asked = None;
            Plan::Ready(p)
        }
        Some(Err(e)) => {
            held.key = key;
            held.prepared = None;
            held.error = Some(e.clone());
            held.asked = None;
            Plan::Failed(e)
        }
        None => {
            // Ask when nothing was asked, or when the job went (its result was dropped and the
            // shared cache no longer has the plan).
            if held.asked != Some(key) || !worker().pending_plan(slot, key) {
                match draw::prepare_job(doc, o, key) {
                    Some(job) => {
                        held.asked = Some(key);
                        worker().prepare(slot, job, ctx);
                    }
                    None => return Plan::Failed("the object has no joint pattern".into()),
                }
            }
            Plan::Laying
        }
    }
}

/// The drawing of a plan for a visible box: kept while the view stays in its window. A new window
/// is cut by the worker; until it is there the window that is held is returned, and the flag says
/// that it is being updated.
fn drawn_for(
    held: &mut Held,
    slot: Slot,
    prepared: &Arc<Prepared>,
    visible: BBox,
    zoom: f64,
    ctx: &egui::Context,
) -> (Option<Arc<Drawn>>, bool) {
    let bounds = prepared.bounds();
    let need = (visible.0.max(bounds.0), visible.1.min(bounds.1));
    if need.0.x > need.1.x || need.0.y > need.1.y {
        return (None, false);
    }
    let min_unit = MIN_UNIT_PX / zoom;
    if let (Some(d), Some(w)) = (&held.drawn, held.window)
        && held.drawn_key == prepared.key
        && contains(w, need)
        && area(w) <= MAX_WINDOW_RATIO * area(need).max(1.0)
        // The groups that are hidden are the groups that are too small now.
        && d.groups.iter().all(|g| g.hidden == (g.unit_mm < min_unit))
        // A preview is looked at again when the view got smaller.
        && (!d.is_preview() || area(need) >= PREVIEW_SHRINK * held.need_area)
    {
        return (Some(d.clone()), false);
    }
    let size = need.1 - need.0;
    let window = (
        (need.0 - size * MARGIN).max(bounds.0),
        (need.1 + size * MARGIN).min(bounds.1),
    );
    let opts = DrawOptions {
        bound: bound(),
        max_segments: max_segments(),
        ..DrawOptions::canvas()
    }
    .hiding_units_under(min_unit);
    // The request: this plan, this window, this limit of units.
    let id = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        prepared.key.hash(&mut h);
        for x in [window.0.x, window.0.y, window.1.x, window.1.y, min_unit] {
            x.to_bits().hash(&mut h);
        }
        (opts.bound, opts.max_segments).hash(&mut h);
        h.finish()
    };
    if let Some((asked, w, need_area)) = held.cut_asked
        && asked == id
    {
        if let Some(cut) = worker().take_cut(slot, id) {
            let d = Arc::new(cut.drawn);
            held.drawn = Some(d.clone());
            held.drawn_key = prepared.key;
            held.window = Some(w);
            held.need_area = need_area;
            held.cut_asked = None;
            return (Some(d), false);
        }
        if worker().pending_cut(slot, id) {
            return (held.drawn.clone(), true);
        }
    }
    REDRAWS.with(|r| r.set(r.get() + 1));
    held.cut_asked = Some((id, window, area(need)));
    worker().draw(slot, id, prepared.clone(), window, opts, ctx);
    (held.drawn.clone(), true)
}

/// What the canvas draws for one hardscape: `pen` is the pen of the object, `color` the color of
/// the pattern lines of its style (the pen, half strength, when the style has none). `offset`
/// moves the drawing (an object that is dragged).
#[allow(clippy::too_many_arguments)]
pub fn draw_plan(
    painter: &Painter,
    view: &View,
    doc: &Document,
    o: &Object,
    revision: Option<u64>,
    color: Color32,
    pen: Color32,
    offset: DVec2,
) {
    let ghost = GHOST.with(Cell::get);
    let slot: Slot = slot_of(o.id, ghost);
    let ctx = painter.ctx();
    let frame = ctx.cumulative_frame_nr();
    if !ghost {
        prune(doc, revision, frame);
    }
    let key = key_of(doc, o, revision, ghost);
    let rect = painter.clip_rect();
    let at = |p: DVec2| view.to_screen(p + offset);
    // Notes stand one above the other at the lower left of the object, kept on the screen.
    let (olo, ohi) = doc.object_bounds(o);
    let note_at = |line: usize| {
        let p = if olo.is_finite() && ohi.is_finite() {
            at(DVec2::new(olo.x, olo.y))
        } else {
            rect.left_bottom()
        };
        Pos2::new(
            p.x.clamp(rect.min.x + 4.0, rect.max.x - 4.0),
            p.y.clamp(rect.min.y + 16.0, rect.max.y - 4.0) - 13.0 * line as f32,
        )
    };
    let say = |line: usize, text: String| {
        painter.text(
            note_at(line),
            egui::Align2::LEFT_BOTTOM,
            text,
            egui::FontId::proportional(11.0),
            color,
        );
    };
    let mut lines = 0usize;
    let mut note = |text: String| {
        say(lines, text);
        lines += 1;
    };

    // The plan of this content, and what to draw while it is laid.
    let (plan, drawing) = HELD.with(|h| {
        let mut h = h.borrow_mut();
        let held = h.entry(slot).or_default();
        held.used = frame;
        let plan = plan_for(held, slot, doc, o, key, ctx);
        // The drawing of the last plan stays while the new one is laid.
        let stale = (held.drawn_key != key || held.prepared.as_ref().is_none_or(|p| p.key != key))
            .then(|| held.drawn.clone())
            .flatten();
        (plan, stale)
    });
    let prepared = match plan {
        Plan::Ready(p) => p,
        Plan::Failed(e) => {
            note(tr!("plan-draw-not-drawn", reason = e));
            return;
        }
        Plan::Laying => {
            note(if drawing.is_some() {
                tr!("plan-draw-updating")
            } else {
                tr!("plan-draw-laying")
            });
            if let Some(d) = drawing {
                let (vlo, vhi) = view.visible_model(rect);
                paint_drawn(
                    painter,
                    view,
                    &d,
                    (vlo - offset, vhi - offset),
                    offset,
                    color,
                    pen,
                );
            }
            return;
        }
    };
    let (vlo, vhi) = view.visible_model(rect);
    let visible: BBox = (vlo - offset, vhi - offset);
    let (blo, bhi) = prepared.bounds();
    if !meet(visible, (blo, bhi)) {
        return;
    }
    let (drawn, updating) = HELD.with(|h| {
        let mut h = h.borrow_mut();
        h.get_mut(&slot)
            .map(|held| drawn_for(held, slot, &prepared, visible, view.zoom, ctx))
            .unwrap_or((None, false))
    });
    if updating {
        // The window is being cut by the worker; the one that is held stays meanwhile.
        note(if drawn.is_some() {
            tr!("plan-draw-updating")
        } else {
            tr!("plan-draw-laying")
        });
    }
    if let Some(d) = &drawn {
        paint_drawn(painter, view, d, visible, offset, color, pen);
        if d.has_hidden() {
            note(tr!("plan-draw-hidden"));
        }
        if d.is_preview() {
            let text = match d.detail {
                draw::Detail::Preview {
                    truncated: true, ..
                } => {
                    tr!(
                        "plan-draw-preview-too-fine",
                        label = tr!("plan-draw-preview")
                    )
                }
                _ => tr!("plan-draw-preview"),
            };
            note(text);
        }
    }
    if show_check() {
        match prepared.findings_ready() {
            Some(Err(e)) => note(tr!("plan-draw-check-failed", reason = e.clone())),
            Some(Ok(report)) => {
                let (total, listed, cut) = draw::findings_summary(report);
                note(if cut.is_empty() {
                    tr!("plan-draw-check-summary", total = total, listed = listed)
                } else {
                    tr!(
                        "plan-draw-check-summary-cut",
                        total = total,
                        listed = listed,
                        kinds = cut.join(", ")
                    )
                });
                for f in &report.findings {
                    let p = DVec2::new(f.at[0], f.at[1]);
                    if !meet((p, p), visible) {
                        continue;
                    }
                    let c = finding_color(f.kind);
                    let s = view.to_screen(p + offset);
                    painter.circle_stroke(s, 6.0, Stroke::new(1.5, c));
                    painter.circle_filled(s, 1.5, c);
                }
            }
            None => {
                // The findings are made in the background; none of an older plan are shown.
                // In the browser there is no second thread: the check runs in the page and the page
                // pauses, so the label says so and is painted one frame before the check starts.
                let browser = cfg!(target_arch = "wasm32");
                note(if browser {
                    tr!("plan-draw-checking-pauses")
                } else {
                    tr!("plan-draw-checking")
                });
                HELD.with(|h| {
                    if let Some(held) = h.borrow_mut().get_mut(&slot)
                        && held.check_asked != Some(prepared.key)
                    {
                        if browser && !held.check_deferred {
                            held.check_deferred = true;
                            ctx.request_repaint();
                        } else {
                            held.check_deferred = false;
                            held.check_asked = Some(prepared.key);
                            worker().check(slot, prepared.clone(), ctx);
                        }
                    }
                });
            }
        }
    }
}

/// Paints a drawing: the joint lines, the cut units, the outlines of hidden courses and the zone
/// edges that are in `visible`.
fn paint_drawn(
    painter: &Painter,
    view: &View,
    d: &Drawn,
    visible: BBox,
    offset: DVec2,
    color: Color32,
    pen: Color32,
) {
    let at = |p: DVec2| view.to_screen(p + offset);
    let stroke = Stroke::new(1.0, color);
    let fill = color.gamma_multiply(0.22);
    let mut shapes: Vec<egui::Shape> = Vec::new();
    for g in &d.groups {
        if g.hidden {
            // The units are too small to see, but a course is still a band on the ground.
            for ring in g.outline.iter().filter(|r| meet(bbox(r), visible)) {
                shapes.push(egui::Shape::closed_line(
                    ring.iter().map(|p| at(*p)).collect(),
                    stroke,
                ));
            }
            continue;
        }
        for (a, b) in &g.joints {
            if meet((a.min(*b), a.max(*b)), visible) {
                shapes.push(egui::Shape::line_segment([at(*a), at(*b)], stroke));
            }
        }
        for u in &g.cut {
            let Some(outer) = u.first() else { continue };
            if !meet(bbox(outer), visible) {
                continue;
            }
            shapes.extend(cut_unit(u, &at, fill, stroke));
        }
    }
    let zone_stroke = Stroke::new(1.0, pen.gamma_multiply(0.8));
    for ring in &d.zone_edges {
        if !meet(bbox(ring), visible) {
            continue;
        }
        let mut pts: Vec<Pos2> = ring.iter().map(|p| at(*p)).collect();
        pts.extend(pts.first().copied());
        shapes.extend(egui::Shape::dashed_line(&pts, zone_stroke, 6.0, 4.0));
    }
    painter.extend(shapes);
}

fn convex(pts: &[Pos2]) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        let cross = (b - a).x * (c - b).y - (b - a).y * (c - b).x;
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// The shapes of one cut unit: filled, with its holes open, and outlined.
fn cut_unit(
    rings: &[Vec<DVec2>],
    at: &dyn Fn(DVec2) -> Pos2,
    fill: Color32,
    stroke: Stroke,
) -> Vec<egui::Shape> {
    let mut out = Vec::new();
    let outer: Vec<Pos2> = rings[0].iter().map(|p| at(*p)).collect();
    if rings.len() == 1 && convex(&outer) {
        out.push(egui::Shape::convex_polygon(outer, fill, stroke));
        return out;
    }
    let (pts, tris) = crate::tess::triangulate_with_holes(rings);
    let mut mesh = egui::epaint::Mesh::default();
    for p in &pts {
        mesh.colored_vertex(at(*p), fill);
    }
    for t in tris {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    out.push(egui::Shape::mesh(mesh));
    for r in rings {
        out.push(egui::Shape::closed_line(
            r.iter().map(|p| at(*p)).collect(),
            stroke,
        ));
    }
    out
}

/// Waits until the worker has nothing to do (tests).
#[cfg(test)]
pub fn wait_for_worker() {
    let t = std::time::Instant::now();
    while worker().busy() && t.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests;
