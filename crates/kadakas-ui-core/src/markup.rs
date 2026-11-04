//! The logic of markup on the view (REV-11), without egui: the tools, the stroke that is being
//! drawn, the hit test of the eraser and the select tool, and the test that tells whether the
//! view is the one a markup was drawn in.
//!
//! The data and the commands are in `kadakas_core::markup`. A UI program turns pointer events into
//! calls of [`Draft`], runs the commands that it builds, and paints the primitives of
//! `kadakas_core::markup::draw`. One drawn mark is one `AddMark` command, so it is one undo step.

use glam::DVec2;
use kadakas_core::command::Command;
use kadakas_core::geom::dist_to_segment;
use kadakas_core::issue::{ViewSpec, Viewpoint};
use kadakas_core::markup::draw::{Prim, mark_prims};
use kadakas_core::markup::{
    DEFAULT_WIDTH, MAX_POINTS, MIN_POINT_DISTANCE, Mark, MarkKind, Markup, MarkupId,
    MarkupViewpoint, PALETTE, Rgb, rel_of,
};
use kadakas_core::model::Document;

use crate::view::{MAX_ZOOM, MIN_ZOOM};

/// What the pointer does on the canvas while the Markup mode is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Pen,
    Highlighter,
    Arrow,
    Rectangle,
    Ellipse,
    Note,
    /// Picks a mark: then the colour, the width and Delete change it.
    Select,
    /// Removes the mark under the pointer.
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Pen,
        Tool::Highlighter,
        Tool::Arrow,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Note,
        Tool::Select,
        Tool::Eraser,
    ];

    /// The kind of mark the tool draws. `None` for the tools that do not draw.
    pub fn kind(self) -> Option<MarkKind> {
        Some(match self {
            Tool::Pen => MarkKind::Pen,
            Tool::Highlighter => MarkKind::Highlighter,
            Tool::Arrow => MarkKind::Arrow,
            Tool::Rectangle => MarkKind::Rectangle,
            Tool::Ellipse => MarkKind::Ellipse,
            Tool::Note => MarkKind::Note,
            Tool::Select | Tool::Eraser => return None,
        })
    }

    /// The key that chooses the tool while the Markup mode is on (1 to 8).
    pub fn digit(self) -> u8 {
        Tool::ALL
            .iter()
            .position(|t| *t == self)
            .map_or(0, |i| i as u8 + 1)
    }
}

/// The colour and the width that new marks get.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub color: Rgb,
    pub width: f64,
}

impl Default for Brush {
    fn default() -> Self {
        Brush {
            color: PALETTE[0],
            width: DEFAULT_WIDTH,
        }
    }
}

/// A mark that is being drawn: the pointer is down. The points are in points of the canvas.
#[derive(Clone, Debug)]
pub struct Draft {
    tool: Tool,
    brush: Brush,
    canvas: [f64; 2],
    points: Vec<[f64; 2]>,
}

impl Draft {
    /// Starts a mark at a point of the canvas. `None` for a tool that does not draw.
    pub fn begin(tool: Tool, brush: Brush, canvas: [f64; 2], at: [f64; 2]) -> Option<Draft> {
        tool.kind()?;
        Some(Draft {
            tool,
            brush,
            canvas,
            points: vec![at],
        })
    }

    /// The pointer moves. A freehand stroke keeps a point only when it is at least
    /// [`MIN_POINT_DISTANCE`] points from the last one, so the data stays small. A shape keeps
    /// its first point and the current one.
    pub fn add(&mut self, at: [f64; 2]) {
        match self.tool {
            Tool::Pen | Tool::Highlighter => {
                let last = self.points[self.points.len() - 1];
                if (at[0] - last[0]).hypot(at[1] - last[1]) >= MIN_POINT_DISTANCE {
                    self.points.push(at);
                }
            }
            Tool::Arrow | Tool::Rectangle | Tool::Ellipse => {
                self.points.truncate(1);
                self.points.push(at);
            }
            _ => {}
        }
    }

    /// The number of points kept.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    fn mark(&self, kind: MarkKind, points: &[[f64; 2]]) -> Mark {
        let mut m = Mark::new(
            kind,
            points.iter().map(|p| rel_of(*p, self.canvas)).collect(),
        );
        m.color = self.brush.color;
        m.width = self.brush.width;
        m
    }

    /// The marks that the stroke makes. A freehand stroke with more points than a mark holds is
    /// cut into several marks that share the point between them, so a long scribble loses
    /// nothing. A shape with no size (a click without a drag) makes none.
    pub fn finish(&self) -> Vec<Mark> {
        let Some(kind) = self.tool.kind() else {
            return vec![];
        };
        match self.tool {
            Tool::Pen | Tool::Highlighter => {
                let mut out = Vec::new();
                let mut start = 0;
                loop {
                    let end = (start + MAX_POINTS).min(self.points.len());
                    out.push(self.mark(kind.clone(), &self.points[start..end]));
                    if end == self.points.len() {
                        break;
                    }
                    start = end - 1;
                }
                out
            }
            Tool::Arrow | Tool::Rectangle | Tool::Ellipse => {
                if self.points.len() < 2 {
                    return vec![];
                }
                let (a, b) = (self.points[0], self.points[1]);
                if (a[0] - b[0]).hypot(a[1] - b[1]) < MIN_POINT_DISTANCE {
                    return vec![];
                }
                vec![self.mark(kind, &[a, b])]
            }
            _ => vec![],
        }
    }

    /// What to paint while the pointer is down, on a canvas of this size.
    pub fn preview(&self) -> Vec<Prim> {
        self.finish()
            .iter()
            .flat_map(|m| mark_prims(m, self.canvas, 1.0))
            .collect()
    }
}

/// The command that adds a mark, one undo step.
pub fn add_mark(markup: MarkupId, mark: Mark) -> Command {
    Command::AddMark {
        markup,
        mark: Box::new(mark),
    }
}

/// A note at a point of the canvas, with its text.
pub fn note_at(brush: Brush, canvas: [f64; 2], at: [f64; 2], text: &str) -> Mark {
    let mut m = Mark::new(MarkKind::Note, vec![rel_of(at, canvas)]);
    m.color = brush.color;
    m.width = brush.width;
    m.text = text.trim().to_string();
    m
}

/// The size of the box of a note in points, for the hit test: the text is wider than a line.
fn note_box(text: &str, size: f64) -> [f64; 2] {
    let lines: Vec<&str> = text.lines().collect();
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1);
    [
        longest as f64 * size * 0.55 + size,
        lines.len().max(1) as f64 * size * 1.3 + size * 0.8,
    ]
}

/// The distance in points from a point of the canvas to a mark drawn on a canvas of `canvas`.
/// A mark of an unknown kind is infinitely far.
pub fn distance_to_mark(mark: &Mark, canvas: [f64; 2], scale: f64, at: [f64; 2]) -> f64 {
    let p = DVec2::from(at);
    let pts: Vec<DVec2> = mark
        .points
        .iter()
        .map(|q| DVec2::new(q[0] * canvas[0], q[1] * canvas[1]))
        .collect();
    let half = (mark.width * scale).max(1.0) / 2.0
        * if mark.kind == MarkKind::Highlighter {
            kadakas_core::markup::HIGHLIGHT_FACTOR
        } else {
            1.0
        };
    let segs = |closed: bool, pts: &[DVec2]| -> f64 {
        let n = pts.len();
        let count = if closed { n } else { n.saturating_sub(1) };
        (0..count)
            .map(|i| dist_to_segment(p, pts[i], pts[(i + 1) % n]))
            .fold(f64::INFINITY, f64::min)
    };
    let d = match &mark.kind {
        MarkKind::Pen | MarkKind::Highlighter => match pts.len() {
            0 => f64::INFINITY,
            1 => p.distance(pts[0]),
            _ => segs(false, &pts),
        },
        MarkKind::Arrow if pts.len() == 2 => segs(false, &pts),
        MarkKind::Rectangle if pts.len() == 2 => {
            let (a, b) = (pts[0], pts[1]);
            segs(true, &[a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)])
        }
        MarkKind::Ellipse if pts.len() == 2 => {
            let (c, r) = ((pts[0] + pts[1]) / 2.0, (pts[1] - pts[0]).abs() / 2.0);
            let ring: Vec<DVec2> = (0..64)
                .map(|i| {
                    let t = f64::from(i) / 64.0 * std::f64::consts::TAU;
                    DVec2::new(c.x + r.x * t.cos(), c.y + r.y * t.sin())
                })
                .collect();
            segs(true, &ring)
        }
        MarkKind::Note if pts.len() == 1 => {
            let size = kadakas_core::markup::draw::NOTE_TEXT_SIZE * scale;
            let [w, h] = note_box(&mark.text, size);
            let lo = DVec2::new(pts[0].x, pts[0].y - h / 2.0);
            let hi = lo + DVec2::new(w, h);
            let inside = p.cmpge(lo).all() && p.cmple(hi).all();
            if inside {
                0.0
            } else {
                let q = p.clamp(lo, hi);
                p.distance(q)
            }
        }
        _ => f64::INFINITY,
    };
    (d - half).max(0.0)
}

/// The topmost mark within `tol` points of a point of the canvas: the one drawn last.
pub fn hit_mark(markup: &Markup, canvas: [f64; 2], at: [f64; 2], tol: f64) -> Option<u32> {
    let scale = canvas[0] / markup.viewpoint.canvas[0];
    markup
        .marks
        .iter()
        .rev()
        .find(|m| distance_to_mark(m, canvas, scale, at) <= tol)
        .map(|m| m.id)
}

/// True if the view now is the view a markup was drawn in: the same plan area or the same
/// camera. The size of the canvas may differ a little: the marks are relative to it.
pub fn view_matches(stored: &MarkupViewpoint, now: &MarkupViewpoint) -> bool {
    let close = |a: f64, b: f64, tol: f64| (a - b).abs() <= tol;
    match (&stored.view, &now.view) {
        (
            ViewSpec::Plan {
                center: c0,
                zoom: z0,
            },
            ViewSpec::Plan {
                center: c1,
                zoom: z1,
            },
        ) => {
            // The width of the part of the drawing that the canvas shows.
            let (w0, w1) = (stored.canvas[0] / z0, now.canvas[0] / z1);
            close(w0, w1, w0 * 0.005)
                && close(c0[0], c1[0], w0 * 0.005)
                && close(c0[1], c1[1], w0 * 0.005)
        }
        (
            ViewSpec::Camera {
                target: t0,
                yaw: y0,
                pitch: p0,
                distance: d0,
                perspective: q0,
            },
            ViewSpec::Camera {
                target: t1,
                yaw: y1,
                pitch: p1,
                distance: d1,
                perspective: q1,
            },
        ) => {
            q0 == q1
                && close(*y0, *y1, 1e-3)
                && close(*p0, *p1, 1e-3)
                && close(*d0, *d1, d0 * 1e-3)
                && t0.iter().zip(t1).all(|(a, b)| close(*a, *b, d0 * 1e-3))
                && close(stored.fov_degrees(), now.fov_degrees(), 0.05)
        }
        _ => false,
    }
}

/// The zoom of the plan view that shows what the markup showed, on a canvas of `width` points:
/// the same part of the drawing across the width.
pub fn plan_zoom_for(stored: &MarkupViewpoint, zoom: f64, width: f64) -> f64 {
    (zoom * width / stored.canvas[0]).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// The viewpoint of an issue-style open: the commands that show the layers and classes as they
/// were when the markup was drawn.
pub fn visibility_commands(doc: &Document, vp: &MarkupViewpoint) -> Vec<Command> {
    let shape = Viewpoint {
        layers: vp.layers.clone(),
        classes: vp.classes.clone(),
        ..Viewpoint::default()
    };
    kadakas_review::issues::visibility_commands(doc, &shape)
}

/// The command that makes a new, empty markup for the viewpoint.
pub fn new_markup(
    viewpoint: MarkupViewpoint,
    author: &str,
    time_ms: i64,
    name: Option<String>,
) -> Command {
    let mut m = Markup::new(viewpoint);
    m.author = author.trim().to_string();
    m.time = time_ms;
    m.name = name;
    Command::AddMarkup {
        markup: Box::new(m),
    }
}

#[cfg(test)]
mod tests;
