//! What the paving dialogs draw on the plan: the regions of Create Objects from Shapes with the
//! gaps and dead ends, the zones and split lines, the preview of a course, the edge under the
//! pointer while one is picked, and the findings of the check. None of it is part of the document.

use egui::epaint::PathStroke;
use egui::{Color32, Painter, Pos2, Rect, Shape as Sh, Stroke};
use glam::DVec2;
use kadakas_core::curve;
use kadakas_core::model::Document;
use kadakas_ui_core::paving::curves::{Found, NoteKind};
use kadakas_ui_core::paving::pick;

use super::{PavingUi, PickKind, outline_of};
use crate::view::{View, ViewExt as _};

/// The color of zone `i` (a palette that also works for the color blind: it differs in lightness).
pub fn zone_color(i: usize, alpha: u8) -> Color32 {
    const PALETTE: [[u8; 3]; 6] = [
        [0, 114, 178],
        [230, 159, 0],
        [0, 158, 115],
        [204, 121, 167],
        [86, 180, 233],
        [213, 94, 0],
    ];
    let [r, g, b] = PALETTE[i % PALETTE.len()];
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

const ACCENT: [u8; 3] = [0, 114, 178];
const WARN: Color32 = Color32::from_rgb(213, 94, 0);

fn accent(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(ACCENT[0], ACCENT[1], ACCENT[2], alpha)
}

fn screen(view: &View, pts: &[DVec2]) -> Vec<Pos2> {
    pts.iter().map(|p| view.to_screen(*p)).collect()
}

fn polygon(painter: &Painter, pts: Vec<Pos2>, fill: Color32, stroke: Stroke) {
    if pts.len() >= 3 {
        painter.add(egui::epaint::PathShape {
            points: pts,
            closed: true,
            fill,
            stroke: PathStroke::new(stroke.width, stroke.color),
        });
    }
}

/// A cross of two lines.
fn cross(painter: &Painter, at: Pos2, r: f32, stroke: Stroke) {
    painter.line_segment([at + egui::vec2(-r, -r), at + egui::vec2(r, r)], stroke);
    painter.line_segment([at + egui::vec2(-r, r), at + egui::vec2(r, -r)], stroke);
}

/// Draws all previews and marks that the paving module has. `cursor` is the pointer on the plan.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    view: &View,
    doc: &Document,
    selection: &[kadakas_core::model::ObjectId],
    p: &PavingUi,
    dark: bool,
) {
    let painter = painter.with_clip_rect(rect);
    if let Some(Ok(found)) = p.curves.as_ref().map(|d| &d.found) {
        let on = |r: &kadakas_ui_core::paving::curves::Region| {
            p.curves.as_ref().is_some_and(|d| d.form.is_on(r))
        };
        curves(&painter, view, found, &on, dark);
    }
    if let Some(d) = &p.split {
        split(&painter, view, doc, d, p.drawing, p.hover);
    }
    // The preview of a course is drawn while its hardscape is selected.
    if let Some(Ok(pv)) = p
        .course
        .as_ref()
        .filter(|c| selection.contains(&c.target))
        .map(|c| &c.preview)
    {
        for u in &pv.units {
            polygon(
                &painter,
                screen(view, u),
                accent(110),
                Stroke::new(1.0, accent(255)),
            );
        }
    }
    if let Some(pick) = p.pick {
        pick_hint(&painter, view, doc, p, pick);
    }
    if p.check_open
        && let Some(r) = p.check.result()
    {
        for f in r.findings().filter(|f| p.check.shows(&f.kind)) {
            let at = view.to_screen(f.at);
            painter.circle_stroke(at, 5.0, Stroke::new(1.5, WARN));
        }
        if let Some(f) = p.check.current() {
            let at = view.to_screen(f.at);
            painter.circle_stroke(at, 14.0, Stroke::new(3.0, Color32::WHITE));
            painter.circle_stroke(at, 14.0, Stroke::new(1.5, WARN));
            cross(&painter, at, 5.0, Stroke::new(2.0, WARN));
        }
    }
}

fn curves(
    painter: &Painter,
    view: &View,
    found: &Found,
    on: &dyn Fn(&kadakas_ui_core::paving::curves::Region) -> bool,
    dark: bool,
) {
    let tol = (0.25 / view.zoom).max(1e-3);
    let hole = if dark {
        Color32::from_rgba_unmultiplied(30, 30, 30, 235)
    } else {
        Color32::from_white_alpha(235)
    };
    for r in &found.regions {
        let (fill, line) = if on(r) {
            (accent(70), accent(255))
        } else {
            (Color32::from_rgba_unmultiplied(128, 128, 128, 25), Color32::GRAY)
        };
        let outer = screen(view, &curve::flatten(&r.outline, true, tol));
        polygon(painter, outer, fill, Stroke::new(1.5, line));
        for h in &r.holes {
            let ring = screen(view, &curve::flatten(h, true, tol));
            polygon(painter, ring, hole, Stroke::new(1.0, line));
        }
    }
    for m in &found.marks {
        let at = view.to_screen(m.at);
        match m.kind {
            NoteKind::GapClosed => {
                painter.circle_stroke(at, 7.0, Stroke::new(2.0, Color32::from_rgb(230, 159, 0)));
            }
            NoteKind::DeadEnd => cross(painter, at, 6.0, Stroke::new(2.0, Color32::from_rgb(213, 94, 0))),
            NoteKind::Stub => cross(painter, at, 4.0, Stroke::new(1.5, Color32::from_rgb(150, 150, 150))),
            _ => {
                painter.circle_filled(at, 3.0, Color32::GRAY);
            }
        }
    }
}

fn split(
    painter: &Painter,
    view: &View,
    doc: &Document,
    d: &kadakas_ui_core::paving::split::SplitDialog,
    drawing: bool,
    hover: DVec2,
) {
    if let Ok(zones) = &d.zones {
        for z in zones {
            for shape in &z.shapes {
                let mut rings = shape.iter();
                if let Some(outer) = rings.next() {
                    polygon(
                        painter,
                        screen(view, outer),
                        zone_color(z.index, 55),
                        Stroke::new(1.0, zone_color(z.index, 255)),
                    );
                }
                for h in rings {
                    polygon(
                        painter,
                        screen(view, h),
                        Color32::from_white_alpha(120),
                        Stroke::new(1.0, zone_color(z.index, 255)),
                    );
                }
            }
        }
    }
    let line = Stroke::new(2.0, Color32::from_rgb(213, 94, 0));
    let lines = d.draft.polylines(doc);
    for l in &lines {
        let pts = screen(view, l);
        painter.extend(Sh::dashed_line(&pts, line, 8.0, 4.0));
    }
    if drawing && let Some(last) = d.draft.open.last() {
        let a = view.to_screen(*last);
        let b = view.to_screen(hover);
        painter.extend(Sh::dashed_line(&[a, b], Stroke::new(1.0, line.color), 4.0, 4.0));
    }
    for pt in &d.draft.open {
        painter.circle_filled(view.to_screen(*pt), 3.0, line.color);
    }
}

fn pick_hint(painter: &Painter, view: &View, doc: &Document, p: &PavingUi, pick_req: super::Pick) {
    let tol = 12.0 / view.zoom;
    let line = Stroke::new(3.0, Color32::from_rgb(213, 94, 0));
    match pick_req.kind {
        PickKind::AlignEdge => {
            if let Some(outline) = outline_of(doc, pick_req.target)
                && let Some(n) = pick::nearest_edge(&outline, p.hover, tol)
            {
                let pts = screen(view, &pick::edge_points(&outline, n, tol));
                painter.add(Sh::line(pts, line));
            }
        }
        PickKind::ZoneAlignEdge(zi) => {
            let ring = p
                .zones
                .as_ref()
                .filter(|(_, id, _)| *id == pick_req.target)
                .and_then(|(_, _, z)| z.as_ref().ok())
                .and_then(|z| z.get(zi))
                .and_then(|z| z.shapes.first())
                .and_then(|s| s.first());
            if let Some(ring) = ring {
                let verts = curve::corners(ring);
                if let Some(n) = pick::nearest_edge(&verts, p.hover, tol) {
                    let pts = screen(view, &pick::edge_points(&verts, n, tol));
                    painter.add(Sh::line(pts, line));
                }
            }
        }
        PickKind::Origin | PickKind::ZoneOrigin(_) => {
            let at = view.to_screen(p.hover);
            painter.circle_stroke(at, 8.0, line);
            cross(painter, at, 5.0, line);
        }
    }
}
