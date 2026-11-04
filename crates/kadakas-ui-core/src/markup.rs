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
use kadakas_core::markup::draw::{Prim, mark_prims_with};
use kadakas_core::markup::{
    DEFAULT_WIDTH, MAX_MARK_TEXT_CHARS, MAX_POINTS, MAX_WIDTH, MIN_POINT_DISTANCE, MIN_WIDTH, Mark,
    MarkKind, Markup, MarkupId, MarkupViewpoint, PALETTE, Rgb,
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

/// How the frame of a markup sits on the canvas that is on the screen. The place of a relative
/// point on the screen (points from the top left corner of the canvas) is `origin + rel * size`.
///
/// The points of a mark are relative to the frame of the markup, which has the shape of the canvas
/// the markup was started on. The frame is put on the canvas that is shown now so that it keeps
/// its place in the drawing: in the plan it follows the pan and the zoom, and in 3D (the
/// vertical field of view is fixed) it has the same height as the canvas and is centered. A window
/// of another size or shape does not move a mark away from the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameMap {
    pub origin: [f64; 2],
    pub size: [f64; 2],
    /// How much the frame is magnified against the canvas of the markup. Lines scale with it.
    pub scale: f64,
}

impl FrameMap {
    /// The place on the screen of a relative point.
    pub fn to_screen(&self, rel: [f64; 2]) -> [f64; 2] {
        [
            self.origin[0] + rel[0] * self.size[0],
            self.origin[1] + rel[1] * self.size[1],
        ]
    }

    /// The relative point under a place on the screen, held inside the frame: a mark cannot
    /// leave the frame it is stored in.
    pub fn to_rel(&self, screen: [f64; 2]) -> [f64; 2] {
        kadakas_core::markup::round_rel_point([
            ((screen[0] - self.origin[0]) / self.size[0]).clamp(0.0, 1.0),
            ((screen[1] - self.origin[1]) / self.size[1]).clamp(0.0, 1.0),
        ])
    }
}

/// How a markup shows on the view that is on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameState {
    /// The marks have a place: draw and pick through the map.
    Mapped(FrameMap),
    /// The view is not the view of the markup (another camera, or the plan against a 3D view).
    ViewDiffers,
    /// The camera is the camera of the markup, but the canvas is too narrow for its frame.
    DoesNotFit,
}

/// How the frame of `stored` sits on the view `now`.
pub fn frame_state(stored: &MarkupViewpoint, now: &MarkupViewpoint) -> FrameState {
    let close = |a: f64, b: f64, tol: f64| (a - b).abs() <= tol;
    let (w0, h0) = (stored.canvas[0], stored.canvas[1]);
    let (wn, hn) = (now.canvas[0], now.canvas[1]);
    match (&stored.view, &now.view) {
        (
            ViewSpec::Plan {
                center: c0,
                zoom: z0,
            },
            ViewSpec::Plan {
                center: cn,
                zoom: zn,
            },
        ) => {
            let k = zn / z0;
            let size = [w0 * k, h0 * k];
            // Where the middle of the frame is now: the same point of the drawing.
            let mid = [
                wn / 2.0 + (c0[0] - cn[0]) * zn,
                hn / 2.0 - (c0[1] - cn[1]) * zn,
            ];
            FrameState::Mapped(FrameMap {
                origin: [mid[0] - size[0] / 2.0, mid[1] - size[1] / 2.0],
                size,
                scale: k,
            })
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
            let same = q0 == q1
                && close(*y0, *y1, 1e-3)
                && close(*p0, *p1, 1e-3)
                && close(*d0, *d1, d0 * 1e-3)
                && t0.iter().zip(t1).all(|(a, b)| close(*a, *b, d0 * 1e-3))
                && close(stored.fov_degrees(), now.fov_degrees(), 0.05);
            if !same {
                return FrameState::ViewDiffers;
            }
            // The vertical field of view is fixed: the frame has the height of the canvas and
            // the width that the shape of the canvas of the markup gives.
            let size = [hn * w0 / h0, hn];
            if size[0] > wn + 0.5 {
                return FrameState::DoesNotFit;
            }
            FrameState::Mapped(FrameMap {
                origin: [(wn - size[0]) / 2.0, 0.0],
                size,
                scale: hn / h0,
            })
        }
        _ => FrameState::ViewDiffers,
    }
}

/// A mark that is being drawn: the pointer is down. The points are places on the screen.
#[derive(Clone, Debug)]
pub struct Draft {
    tool: Tool,
    brush: Brush,
    points: Vec<[f64; 2]>,
    /// The last place of the pointer.
    tail: [f64; 2],
}

impl Draft {
    /// Starts a mark at a place on the screen. `None` for a tool that does not draw.
    pub fn begin(tool: Tool, brush: Brush, at: [f64; 2]) -> Option<Draft> {
        tool.kind()?;
        Some(Draft {
            tool,
            brush,
            points: vec![at],
            tail: at,
        })
    }

    /// The pointer moves. A freehand stroke keeps a point only when it is at least
    /// [`MIN_POINT_DISTANCE`] points from the last one, so the data stays small. A shape keeps
    /// its first point and the current one.
    pub fn add(&mut self, at: [f64; 2]) {
        self.tail = at;
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

    fn mark(&self, kind: MarkKind, points: &[[f64; 2]], map: &FrameMap) -> Mark {
        let mut m = Mark::new(kind, points.iter().map(|p| map.to_rel(*p)).collect());
        m.color = self.brush.color;
        m.width = (self.brush.width / map.scale).clamp(MIN_WIDTH, MAX_WIDTH);
        m
    }

    /// The marks that the stroke makes, in the frame of the markup. A freehand stroke with more
    /// points than a mark holds is cut into several marks that share the point between them, so a
    /// long scribble loses nothing. A shape with no size (a click without a drag) makes none.
    pub fn finish(&self, map: &FrameMap) -> Vec<Mark> {
        let Some(kind) = self.tool.kind() else {
            return vec![];
        };
        match self.tool {
            Tool::Pen | Tool::Highlighter => {
                // The stroke ends where the pointer was lifted.
                let mut pts = self.points.clone();
                if pts.last() != Some(&self.tail) && pts.len() > 1 {
                    pts.push(self.tail);
                }
                let mut out = Vec::new();
                let mut start = 0;
                loop {
                    let end = (start + MAX_POINTS).min(pts.len());
                    out.push(self.mark(kind.clone(), &pts[start..end], map));
                    if end == pts.len() {
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
                vec![self.mark(kind, &[a, b], map)]
            }
            _ => vec![],
        }
    }

    /// What to paint while the pointer is down.
    pub fn preview(&self, map: &FrameMap) -> Vec<Prim> {
        self.finish(map)
            .iter()
            .flat_map(|m| mark_prims_with(m, &|p| map.to_screen(p), map.scale))
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

/// A note at a place on the screen, with its text.
pub fn note_at(brush: Brush, map: &FrameMap, at: [f64; 2], text: &str) -> Mark {
    let mut m = Mark::new(MarkKind::Note, vec![map.to_rel(at)]);
    m.color = brush.color;
    m.width = (brush.width / map.scale).clamp(MIN_WIDTH, MAX_WIDTH);
    m.text = text.trim().chars().take(MAX_MARK_TEXT_CHARS).collect();
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

/// The distance in points from a place on the screen to a mark, with the frame of the markup
/// put on the screen by `map`. A mark that is not drawable is infinitely far.
pub fn distance_to_mark(mark: &Mark, map: &FrameMap, at: [f64; 2]) -> f64 {
    if !mark.drawable() {
        return f64::INFINITY;
    }
    let p = DVec2::from(at);
    let pts: Vec<DVec2> = mark
        .points
        .iter()
        .map(|q| DVec2::from(map.to_screen(*q)))
        .collect();
    let half = (mark.width.clamp(MIN_WIDTH, MAX_WIDTH) * map.scale).max(1.0) / 2.0
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
            let size = kadakas_core::markup::draw::NOTE_TEXT_SIZE * map.scale;
            let [w, h] = note_box(&mark.text, size);
            let lo = DVec2::new(pts[0].x, pts[0].y - h / 2.0);
            let hi = lo + DVec2::new(w, h);
            if p.cmpge(lo).all() && p.cmple(hi).all() {
                0.0
            } else {
                p.distance(p.clamp(lo, hi))
            }
        }
        _ => f64::INFINITY,
    };
    (d - half).max(0.0)
}

/// The topmost mark within `tol` points of a place on the screen: the one drawn last.
pub fn hit_mark(markup: &Markup, map: &FrameMap, at: [f64; 2], tol: f64) -> Option<u32> {
    markup
        .marks
        .iter()
        .rev()
        .find(|m| distance_to_mark(m, map, at) <= tol)
        .map(|m| m.id)
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
    model: String,
) -> Command {
    let mut m = Markup::new(viewpoint);
    m.model = Some(model);
    m.author = author.trim().to_string();
    m.time = time_ms;
    m.name = name;
    Command::AddMarkup {
        markup: Box::new(m),
    }
}

#[cfg(test)]
mod tests;
