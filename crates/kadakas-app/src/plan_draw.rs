//! The pattern plan of a hardscape on the canvas: the joint lines of every group (the field, the
//! zones, the courses), the cut units with a light fill, the zone edges, and, on request, the
//! findings of the pattern check.
//!
//! The plan comes from `kadakas_core::hardscape::draw`, the same source as `render.svg`, the sheets
//! and the PDF, laid by the same engine as the report and the DXF export. The canvas keeps the laid
//! plan by a content key (the global cache of that module) and, for each object, the part of it
//! that it drew last: a window round the visible part, so a pan or a small zoom costs nothing.
//! The window is drawn again when the view leaves it, or when the view got much smaller than it.
//!
//! Level of detail: a window with at most `CANVAS_BOUND` units is drawn with its real units. A
//! bigger one is a preview of the whole plan, with the label [`PREVIEW_LABEL`] (translated).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use egui::{Color32, Painter, Pos2, Rect, Stroke};
use glam::DVec2;
use kadakas_core::hardscape::draw::{self, BBox, DrawOptions, Drawn, Prepared};
use kadakas_core::layout::check::Kind;
use kadakas_core::model::{Document, Object, ObjectId};
use kadakas_l10n::tr;

use crate::view::{View, ViewExt as _};

/// How far the window reaches round the visible part, as a share of its size.
const MARGIN: f64 = 0.25;
/// A window that is more than this many times the area of what is visible is drawn again.
const MAX_WINDOW_RATIO: f64 = 9.0;
/// Units smaller than this on the screen (px) are not drawn: they would only make a gray area.
pub const MIN_UNIT_PX: f64 = 3.0;

/// The window that an object drew last.
struct Held {
    key: u64,
    window: BBox,
    drawn: Rc<Drawn>,
}

thread_local! {
    /// Tests that measure set the bound of the window; `None`: [`draw::CANVAS_BOUND`].
    static BOUND: Cell<Option<usize>> = const { Cell::new(None) };
    static HELD: RefCell<HashMap<ObjectId, Held>> = RefCell::new(HashMap::new());
    /// The content key of an object for a document revision.
    static KEYS: RefCell<HashMap<ObjectId, (u64, u64)>> = RefCell::new(HashMap::new());
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

/// Show the findings of the pattern check on the plan.
pub fn show_check() -> bool {
    SHOW_CHECK.with(Cell::get)
}

pub fn set_show_check(on: bool) {
    SHOW_CHECK.with(|s| s.set(on));
}

/// How often a window was laid and cut (not taken from what the object drew before).
#[cfg(test)]
pub fn redraws() -> u64 {
    REDRAWS.with(Cell::get)
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

/// The content key of the plan of an object. With a revision of the document it is made once for
/// the revision; the same revision names the same document state.
fn key_of(doc: &Document, o: &Object, revision: Option<u64>) -> u64 {
    if let Some(rev) = revision
        && let Some(k) = KEYS.with(|k| {
            k.borrow()
                .get(&o.id)
                .filter(|(r, _)| *r == rev)
                .map(|e| e.1)
        })
    {
        return k;
    }
    let key = draw::content_key(doc, o, kadakas_core::layout::TOLERANCE);
    if let Some(rev) = revision {
        KEYS.with(|k| {
            let mut k = k.borrow_mut();
            if k.len() > 4000 {
                k.clear();
            }
            k.insert(o.id, (rev, key));
        });
    }
    key
}

/// The drawing of an object for a visible box: kept while the view stays in its window.
fn drawn_for(prepared: &Prepared, id: ObjectId, visible: BBox) -> Rc<Drawn> {
    let bounds = prepared.bounds();
    let need = (visible.0.max(bounds.0), visible.1.min(bounds.1));
    let held = HELD.with(|h| {
        h.borrow()
            .get(&id)
            .filter(|h| {
                h.key == prepared.key
                    && contains(h.window, need)
                    && area(h.window) <= MAX_WINDOW_RATIO * area(need).max(1.0)
            })
            .map(|h| h.drawn.clone())
    });
    if let Some(d) = held {
        return d;
    }
    REDRAWS.with(|r| r.set(r.get() + 1));
    let size = need.1 - need.0;
    let window = (
        (need.0 - size * MARGIN).max(bounds.0),
        (need.1 + size * MARGIN).min(bounds.1),
    );
    let drawn = Rc::new(prepared.draw(
        Some(window),
        &DrawOptions {
            bound: bound(),
            max_segments: max_segments(),
            ..DrawOptions::canvas()
        },
    ));
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        if h.len() > 500 {
            h.clear();
        }
        h.insert(
            id,
            Held {
                key: prepared.key,
                window,
                drawn: drawn.clone(),
            },
        );
    });
    drawn
}

fn finding_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Overlap | Kind::OutsideArea => Color32::from_rgb(220, 30, 30),
        Kind::JointTooNarrow | Kind::JointTooWide => Color32::from_rgb(235, 130, 20),
        Kind::SmallPiece | Kind::WrongSize => Color32::from_rgb(160, 40, 200),
        Kind::Coverage => Color32::from_rgb(20, 110, 220),
    }
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
    let key = key_of(doc, o, revision);
    let Some(prepared) = draw::prepared_with_key(doc, o, key) else {
        return;
    };
    let rect = painter.clip_rect();
    let at = |p: DVec2| view.to_screen(p + offset);
    let note_at = |p: DVec2| {
        let p = at(p);
        Pos2::new(
            p.x.clamp(rect.min.x + 4.0, rect.max.x - 4.0),
            p.y.clamp(rect.min.y + 4.0, rect.max.y - 16.0),
        )
    };
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            // The pattern cannot be laid: say why, where the object is.
            let (lo, hi) = doc.object_bounds(o);
            if lo.is_finite() && hi.is_finite() {
                painter.text(
                    note_at(DVec2::new(lo.x, lo.y)),
                    egui::Align2::LEFT_BOTTOM,
                    tr!("plan-draw-not-drawn", reason = e),
                    egui::FontId::proportional(11.0),
                    color,
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
    let drawn = drawn_for(&prepared, o.id, visible);
    let stroke = Stroke::new(1.0, color);
    let fill = color.gamma_multiply(0.22);
    // What the view shows: the part of the window that is on the screen.
    let on_screen = |a: DVec2, b: DVec2| meet((a.min(b), a.max(b)), visible);
    let mut shapes: Vec<egui::Shape> = Vec::new();
    for g in &drawn.groups {
        for (a, b) in &g.joints {
            if on_screen(*a, *b) {
                shapes.push(egui::Shape::line_segment([at(*a), at(*b)], stroke));
            }
        }
        for u in &g.cut {
            let Some(outer) = u.first() else { continue };
            let (lo, hi) = bbox(outer);
            if !meet((lo, hi), visible) {
                continue;
            }
            let pts: Vec<Pos2> = outer.iter().map(|p| at(*p)).collect();
            if u.len() == 1 && convex(&pts) {
                shapes.push(egui::Shape::convex_polygon(pts, fill, stroke));
            } else if u.len() == 1 {
                // A concave piece: filled as triangles.
                let tri = kadakas_core::mesh::triangulate(outer);
                let mut mesh = egui::epaint::Mesh::default();
                for p in &pts {
                    mesh.colored_vertex(*p, fill);
                }
                for t in tri {
                    mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
                }
                shapes.push(egui::Shape::mesh(mesh));
                shapes.push(egui::Shape::closed_line(pts, stroke));
            } else {
                // A piece with a hole in it: the outlines only.
                for r in u {
                    shapes.push(egui::Shape::closed_line(
                        r.iter().map(|p| at(*p)).collect(),
                        stroke,
                    ));
                }
            }
        }
    }
    let zone_stroke = Stroke::new(1.0, pen.gamma_multiply(0.8));
    for ring in &drawn.zone_edges {
        let (lo, hi) = bbox(ring);
        if !meet((lo, hi), visible) {
            continue;
        }
        let mut pts: Vec<Pos2> = ring.iter().map(|p| at(*p)).collect();
        pts.extend(pts.first().copied());
        shapes.extend(egui::Shape::dashed_line(&pts, zone_stroke, 6.0, 4.0));
    }
    painter.extend(shapes);
    if let draw::Detail::Preview { truncated, .. } = drawn.detail {
        let (lo, hi) = (visible.0.max(blo), visible.1.min(bhi));
        let text = if truncated {
            tr!(
                "plan-draw-preview-too-fine",
                label = tr!("plan-draw-preview")
            )
        } else {
            tr!("plan-draw-preview")
        };
        painter.text(
            note_at(DVec2::new(lo.x, lo.y.min(hi.y))),
            egui::Align2::LEFT_BOTTOM,
            text,
            egui::FontId::proportional(11.0),
            color,
        );
    }
    if show_check() {
        draw_check(painter, view, &prepared, visible, offset, rect);
    }
}

fn bbox(ring: &[DVec2]) -> BBox {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in ring {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo, hi)
}

/// A mark for each finding of the check that is in view: a ring, in the color of its kind.
fn draw_check(
    painter: &Painter,
    view: &View,
    prepared: &Prepared,
    visible: BBox,
    offset: DVec2,
    rect: Rect,
) {
    match prepared.findings() {
        Err(e) => {
            painter.text(
                Pos2::new(rect.min.x + 6.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                tr!("plan-draw-check-failed", reason = e.clone()),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(220, 30, 30),
            );
        }
        Ok(report) => {
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
    }
}

#[cfg(test)]
mod tests;
