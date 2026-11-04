//! The drawing of marks as plain primitives (REV-11).
//!
//! The app paints them with egui over the canvas, and `kadakas-print` paints them into a PNG
//! (the render with marks that `markup.get` and the BCF snapshot use). One description of the
//! geometry makes both pictures equal. All coordinates are points of the canvas the primitives
//! are made for (from the top left corner, y down).

use super::{HIGHLIGHT_ALPHA, HIGHLIGHT_FACTOR, Mark, MarkKind, Markup, Rgb, canvas_of};

/// The opacity of the background of a note.
pub const NOTE_BACKGROUND_ALPHA: f64 = 0.88;
/// The size of the text of a note in points.
pub const NOTE_TEXT_SIZE: f64 = 15.0;
/// The segments of the polygon of an ellipse.
const ELLIPSE_SEGMENTS: usize = 64;

/// One thing to paint.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    /// A line of constant width. `round` gives round caps and joins.
    Line {
        points: Vec<[f64; 2]>,
        closed: bool,
        width: f64,
        color: Rgb,
        alpha: f64,
        round: bool,
    },
    /// A filled polygon.
    Fill {
        points: Vec<[f64; 2]>,
        color: Rgb,
        alpha: f64,
    },
    /// A filled circle.
    Dot {
        at: [f64; 2],
        radius: f64,
        color: Rgb,
        alpha: f64,
    },
    /// A note: the text in a box, its top left corner near `at`, and a dot at `at`.
    Note {
        at: [f64; 2],
        text: String,
        size: f64,
        color: Rgb,
    },
}

/// The primitives of one mark on a canvas of `canvas` points. `scale` is how much wider the
/// canvas is than the canvas of the markup (lines keep their share of the width of the view).
/// A mark of a kind this version does not know gives none.
pub fn mark_prims(mark: &Mark, canvas: [f64; 2], scale: f64) -> Vec<Prim> {
    let pts: Vec<[f64; 2]> = mark.points.iter().map(|p| canvas_of(*p, canvas)).collect();
    let width = (mark.width * scale).max(0.5);
    match &mark.kind {
        MarkKind::Pen | MarkKind::Highlighter => {
            let highlight = mark.kind == MarkKind::Highlighter;
            let (w, alpha) = if highlight {
                (width * HIGHLIGHT_FACTOR, HIGHLIGHT_ALPHA)
            } else {
                (width, 1.0)
            };
            match pts.as_slice() {
                [] => vec![],
                [p] => vec![Prim::Dot {
                    at: *p,
                    radius: w / 2.0,
                    color: mark.color,
                    alpha,
                }],
                _ => vec![Prim::Line {
                    points: pts,
                    closed: false,
                    width: w,
                    color: mark.color,
                    alpha,
                    round: !highlight,
                }],
            }
        }
        MarkKind::Arrow if pts.len() == 2 => {
            let (tail, tip) = (pts[0], pts[1]);
            let (dx, dy) = (tip[0] - tail[0], tip[1] - tail[1]);
            let len = dx.hypot(dy);
            if len < 1e-6 {
                return vec![Prim::Dot {
                    at: tip,
                    radius: width,
                    color: mark.color,
                    alpha: 1.0,
                }];
            }
            let (ux, uy) = (dx / len, dy / len);
            let head = (width * 4.0 + 6.0 * scale).min(len * 0.6);
            let base = [tip[0] - ux * head, tip[1] - uy * head];
            let half = head * 0.45;
            let left = [base[0] - uy * half, base[1] + ux * half];
            let right = [base[0] + uy * half, base[1] - ux * half];
            vec![
                Prim::Line {
                    points: vec![tail, base],
                    width,
                    closed: false,
                    color: mark.color,
                    alpha: 1.0,
                    round: true,
                },
                Prim::Fill {
                    points: vec![tip, left, right],
                    color: mark.color,
                    alpha: 1.0,
                },
            ]
        }
        MarkKind::Rectangle if pts.len() == 2 => {
            let (a, b) = (pts[0], pts[1]);
            vec![Prim::Line {
                points: vec![[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]],
                closed: true,
                width,
                color: mark.color,
                alpha: 1.0,
                round: true,
            }]
        }
        MarkKind::Ellipse if pts.len() == 2 => {
            let (a, b) = (pts[0], pts[1]);
            let c = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let (rx, ry) = ((b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0);
            let ring = (0..ELLIPSE_SEGMENTS)
                .map(|i| {
                    let t = i as f64 / ELLIPSE_SEGMENTS as f64 * std::f64::consts::TAU;
                    [c[0] + rx * t.cos(), c[1] + ry * t.sin()]
                })
                .collect();
            vec![Prim::Line {
                points: ring,
                closed: true,
                width,
                color: mark.color,
                alpha: 1.0,
                round: true,
            }]
        }
        MarkKind::Note if pts.len() == 1 => vec![Prim::Note {
            at: pts[0],
            text: mark.text.clone(),
            size: NOTE_TEXT_SIZE * scale,
            color: mark.color,
        }],
        _ => vec![],
    }
}

/// The primitives of all marks of a markup, in the order of their creation.
pub fn markup_prims(markup: &Markup, canvas: [f64; 2]) -> Vec<Prim> {
    let scale = canvas[0] / markup.viewpoint.canvas[0];
    markup
        .marks
        .iter()
        .flat_map(|m| mark_prims(m, canvas, scale))
        .collect()
}
