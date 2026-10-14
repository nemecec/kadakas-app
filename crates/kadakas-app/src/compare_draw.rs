//! Drawing the compare overlay (REV-03) on the plan canvas: tints, a pattern and a mark for each
//! status, the ghosts of removed objects, and the legend with the counts. Pure drawing, no state.
//!
//! The colors are never the only sign. A tinted object gets a box around it with a line pattern
//! (added solid, removed dashed, changed dotted) and a mark in a corner (`+`, `-`, `~`), the
//! same patterns and marks as the legend, the SVG and the list of the Compare window.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape as EShape, Stroke};
use glam::DVec2;
use kadakas_core::model::{Document, Object, ObjectId};
use kadakas_l10n::tr;
use kadakas_review::overlay::{CompareOverlay, CompareStyle, Pattern, Status, UnchangedMode};

use crate::canvas::Style;
use crate::view::{View, ViewExt as _};

/// The most boxes with marks that one frame draws, so that a diff of 100 000 objects stays quick.
pub const MAX_HALOS: usize = 4000;

/// What the canvas draws of a compare in one frame.
pub struct CompareDraw<'a> {
    pub overlay: &'a CompareOverlay,
    pub style: &'a CompareStyle,
    /// The ghosts to draw: removed objects, and the old shapes of changed ones.
    pub ghosts: &'a [(Object, &'static str)],
    /// The object that the person clicked in the list. A ghost with this ID gets a frame.
    pub focus: Option<ObjectId>,
}

/// What the compare does with one object of the drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObjectLook {
    Normal,
    Hidden,
    /// Drawn faint, with this opacity.
    Dim(f32),
    Status(Status),
}

impl CompareDraw<'_> {
    pub fn look(&self, id: ObjectId) -> ObjectLook {
        match self.overlay.status_of(id) {
            Some(s) => ObjectLook::Status(s),
            None => match self.style.unchanged {
                UnchangedMode::Show => ObjectLook::Normal,
                UnchangedMode::Dim => ObjectLook::Dim(self.style.dim),
                UnchangedMode::Hide => ObjectLook::Hidden,
            },
        }
    }
}

pub fn status_color(style: &CompareStyle, status: Status) -> Color32 {
    let [r, g, b] = style.color(status);
    Color32::from_rgb(r, g, b)
}

/// The style of an object that the compare tints: its pen and fill take the color of the status,
/// and the line is thicker.
pub fn tinted(base: Style, status: Status, style: &CompareStyle) -> Style {
    let c = status_color(style, status);
    let weight = style.tint(status).weight / 2.0;
    Style {
        pen: c,
        fill: if base.fill.a() == 0 {
            Color32::TRANSPARENT
        } else {
            c.gamma_multiply(0.35)
        },
        width: (base.width * (1.0 + weight)).clamp(1.5, 24.0),
    }
}

fn dashes(pattern: Pattern) -> Option<(f32, f32)> {
    match pattern {
        Pattern::Solid => None,
        Pattern::Dashed => Some((6.0, 4.0)),
        Pattern::Dotted => Some((1.5, 3.0)),
    }
}

/// A rectangle outline in a pattern.
fn pattern_box(painter: &Painter, r: Rect, stroke: Stroke, pattern: Pattern) {
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    match dashes(pattern) {
        None => {
            painter.add(EShape::line(corners.to_vec(), stroke));
        }
        Some((dash, gap)) => {
            painter.extend(EShape::dashed_line(&corners, stroke, dash, gap));
        }
    }
}

/// The box with the pattern of the status and the mark of the status in its top right corner.
/// Nothing is drawn for an object that is too small to see.
pub fn halo(
    painter: &Painter,
    view: &View,
    doc: &Document,
    o: &Object,
    status: Status,
    style: &CompareStyle,
) {
    let (lo, hi) = doc.object_bounds(o);
    halo_box(painter, view, lo, hi, status, style);
}

fn halo_box(
    painter: &Painter,
    view: &View,
    lo: DVec2,
    hi: DVec2,
    status: Status,
    style: &CompareStyle,
) {
    if !lo.is_finite() || !hi.is_finite() {
        return;
    }
    let (a, b) = (view.to_screen(lo), view.to_screen(hi));
    let r = Rect::from_two_pos(a, b).expand(4.0);
    if r.width() < 6.0 || r.height() < 6.0 {
        return;
    }
    let t = style.tint(status);
    let color = status_color(style, status);
    pattern_box(painter, r, Stroke::new(1.5, color), t.pattern);
    painter.text(
        Pos2::new(r.right() + 2.0, r.top()),
        Align2::LEFT_BOTTOM,
        t.symbol,
        FontId::proportional(14.0),
        color,
    );
}

/// The style of a ghost: the color of removed objects, see-through.
pub fn ghost_style(base: Style, style: &CompareStyle, previous: bool) -> Style {
    let c = status_color(
        style,
        if previous {
            Status::Changed
        } else {
            Status::Removed
        },
    );
    let k = if previous {
        style.ghost * 0.5
    } else {
        style.ghost
    };
    Style {
        pen: c.gamma_multiply(k.max(0.2)),
        fill: if base.fill.a() == 0 {
            Color32::TRANSPARENT
        } else {
            c.gamma_multiply(0.3 * k)
        },
        width: base.width,
    }
}

/// The frame of a ghost that the person clicked in the list.
pub fn focus_frame(painter: &Painter, view: &View, lo: DVec2, hi: DVec2, style: &CompareStyle) {
    if !lo.is_finite() || !hi.is_finite() {
        return;
    }
    let r = Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi)).expand(8.0);
    let c = status_color(style, Status::Removed);
    painter.rect_stroke(r, 2.0, Stroke::new(2.5, c), egui::StrokeKind::Middle);
}

/// The boxes and marks of the ghosts, and the frame of the object that the person clicked in the
/// list (a ghost, or an object of the drawing that is shown). The canvas draws the ghost objects
/// themselves, because a plant or a tag needs the document.
pub fn ghost_marks(painter: &Painter, view: &View, doc: &Document, c: &CompareDraw) {
    let mut drawn = 0usize;
    for (o, kind) in c.ghosts {
        if *kind == "removed" && drawn < MAX_HALOS {
            halo(painter, view, doc, o, Status::Removed, c.style);
            drawn += 1;
        }
    }
    if let Some(id) = c.focus {
        let object = c
            .ghosts
            .iter()
            .find(|(o, kind)| o.id == id && *kind == "removed")
            .map(|(o, _)| o)
            .or_else(|| doc.object(id));
        if let Some(o) = object {
            let (lo, hi) = doc.object_bounds(o);
            focus_frame(painter, view, lo, hi, c.style);
        }
    }
}

/// The legend: a swatch with the pattern and the mark of each status, and its count. It sits at
/// the top left corner of `rect`.
pub fn legend(painter: &Painter, rect: Rect, c: &CompareDraw, dark: bool) {
    let counts = c.overlay.counts();
    let rows = [
        (
            Status::Added,
            counts.added,
            tr!("compare-ui-legend-added", count = counts.added),
        ),
        (
            Status::Removed,
            counts.removed,
            tr!("compare-ui-legend-removed", count = counts.removed),
        ),
        (
            Status::Changed,
            counts.changed,
            tr!("compare-ui-legend-changed", count = counts.changed),
        ),
    ];
    let notes = c.overlay.notes();
    let (bg, fg) = if dark {
        (Color32::from_black_alpha(190), Color32::from_gray(230))
    } else {
        (Color32::from_white_alpha(225), Color32::from_gray(30))
    };
    let font = FontId::proportional(13.0);
    let line = 20.0;
    let height = line * (rows.len() + notes.len()) as f32 + 12.0;
    let r = Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(230.0, height));
    painter.rect_filled(r, 4.0, bg);
    painter.rect_stroke(
        r,
        4.0,
        Stroke::new(1.0, fg.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );
    for (i, (status, _, text)) in rows.iter().enumerate() {
        let y = r.top() + 6.0 + line * i as f32 + line / 2.0;
        let t = c.style.tint(*status);
        let color = status_color(c.style, *status);
        let (x0, x1) = (r.left() + 8.0, r.left() + 36.0);
        let stroke = Stroke::new(2.5, color);
        match dashes(t.pattern) {
            None => {
                painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], stroke);
            }
            Some((d, g)) => {
                painter.extend(EShape::dashed_line(
                    &[Pos2::new(x0, y), Pos2::new(x1, y)],
                    stroke,
                    d,
                    g,
                ));
            }
        }
        painter.text(
            Pos2::new(x1 + 6.0, y),
            Align2::LEFT_CENTER,
            t.symbol,
            FontId::proportional(15.0),
            color,
        );
        painter.text(
            Pos2::new(x1 + 24.0, y),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            fg,
        );
    }
    for (i, note) in notes.iter().enumerate() {
        let y = r.top() + 6.0 + line * (rows.len() + i) as f32 + line / 2.0;
        painter.text(
            Pos2::new(r.left() + 8.0, y),
            Align2::LEFT_CENTER,
            note_text(note),
            FontId::proportional(11.0),
            fg.gamma_multiply(0.8),
        );
    }
}

/// The text of a note about what the overlay does not show completely.
pub fn note_text(note: &kadakas_review::overlay::Note) -> String {
    use kadakas_review::overlay::Note;
    match *note {
        Note::GhostsCapped { drawn, omitted } => {
            tr!(
                "compare-ui-note-ghosts",
                drawn = drawn,
                total = drawn + omitted
            )
        }
        Note::ShapesCapped { omitted } => tr!("compare-ui-note-shapes", omitted = omitted),
        Note::StatusesCapped { untracked } => {
            tr!("compare-ui-note-statuses", untracked = untracked)
        }
        Note::Partial => tr!("compare-ui-note-partial"),
    }
}
