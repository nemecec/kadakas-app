//! Tape Measure (T, MOD-05): a headless state machine like the drawing tools. It measures a
//! distance and makes guides.
//!
//! # Measure
//!
//! Click the start (the inference snaps), move, click the end. The distance shows live in the
//! Measurements box and stays there after the second click. Hovering an edge shows its length.
//! Ctrl (Option on macOS) toggles between making guides (the default) and measuring only.
//!
//! # Guides
//!
//! - A click **on an edge** (or on a guide line), then a click away from it, makes a guide line
//!   **parallel** to that line through the second click. The live value is the offset (the
//!   distance of the pointer from the line). A typed distance sets the offset, on the side where
//!   the pointer is. A negative one takes the other side.
//! - A click **on a point** (or on a face, or in the air), then a click in a direction, makes a
//!   guide line through the point towards the second click. It has no ends. A typed distance
//!   makes a finite guide line of that length instead. A second click on the same point makes a
//!   guide point.
//!
//! Each guide is one undo step. Esc cancels what is in progress.
//!
//! # Resize the model
//!
//! After a measure (Ctrl pressed, so no guides), a typed length asks to resize the model: the
//! answer carries the factor (typed length over measured length) and the first point. The window
//! asks "Resize the model?" and runs the scale. The tool itself changes nothing.

use glam::DVec3;
use kadakas_l10n::tr;
use kadakas_units::Units;

use super::exec::{Action, GuideSpec};
use super::scenes::World;
use super::tools::{LiveLabel, MIN_LEN, Out, Pointer, Preview};
use crate::infer3d::{Constraint, Kind, TagKind};
use crate::measure_box::{Phase, Value};

/// A resize can scale by a factor between these (a typo of a unit must not blow the model up).
pub const RESIZE_RANGE: (f64, f64) = (1e-3, 1e3);

/// What the tool is asking for: the whole model scaled by `factor` about `about` (world
/// coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resize {
    pub factor: f64,
    pub about: DVec3,
    /// The length that was measured and the length that was typed, for the question.
    pub measured: f64,
    pub typed: f64,
}

/// What the first click was on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Origin {
    /// A point: an endpoint, a midpoint, a face, the air.
    Point,
    /// An edge or a guide line: a line with a direction (unit).
    Line { dir: DVec3 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Run {
    Idle,
    From { start: DVec3, origin: Origin },
}

/// The state of the Tape Measure tool.
#[derive(Clone, Debug)]
pub(super) struct Tape {
    run: Run,
    /// Guides are made. Ctrl turns it off (measure only).
    creating: bool,
    cursor: Option<DVec3>,
    /// The last measure, for a typed length that resizes the model.
    measured: Option<(DVec3, DVec3)>,
    /// The length of the edge under the pointer, while the tool waits for the first click.
    hover_length: Option<f64>,
}

impl Default for Tape {
    fn default() -> Self {
        Tape {
            run: Run::Idle,
            creating: true,
            cursor: None,
            measured: None,
            hover_length: None,
        }
    }
}

/// The part of `v` that is perpendicular to the unit vector `dir`.
fn perpendicular(v: DVec3, dir: DVec3) -> DVec3 {
    v - dir * v.dot(dir)
}

impl Tape {
    /// The tool was chosen: a fresh start that makes guides.
    pub fn choose(&mut self) {
        *self = Tape::default();
    }

    pub fn abort(&mut self) {
        self.run = Run::Idle;
        self.cursor = None;
        self.hover_length = None;
    }

    /// Ends what is in progress. True if something was.
    pub fn cancel(&mut self) -> bool {
        let was = !matches!(self.run, Run::Idle);
        self.run = Run::Idle;
        self.cursor = None;
        was
    }

    pub fn in_progress(&self) -> bool {
        !matches!(self.run, Run::Idle)
    }

    /// The first point, while the tool waits for the second one.
    pub fn reference(&self) -> Option<DVec3> {
        match self.run {
            Run::From { start, .. } => Some(start),
            Run::Idle => None,
        }
    }

    /// Ctrl: guides on, or measure only.
    pub fn toggle_ctrl(&mut self) {
        self.creating = !self.creating;
    }

    pub fn creating(&self) -> bool {
        self.creating
    }

    pub fn phase(&self) -> Phase {
        match self.run {
            Run::From { .. } => Phase::Step(1),
            Run::Idle if self.measured.is_some() => Phase::Done,
            Run::Idle => Phase::Step(0),
        }
    }

    /// The distance now: from the start to the pointer, or the offset from the line of an edge.
    fn distance(&self) -> Option<f64> {
        let Run::From { start, origin } = self.run else {
            return None;
        };
        let c = self.cursor?;
        Some(match origin {
            Origin::Line { dir } if self.creating => perpendicular(c - start, dir).length(),
            _ => c.distance(start),
        })
    }

    /// The value for the Measurements box: while the tool runs, the distance; after a measure,
    /// that distance; over an edge, its length.
    pub fn live(&self, units: &Units) -> Option<(LiveLabel, String)> {
        let d = self
            .distance()
            .or_else(|| self.measured.map(|(a, b)| a.distance(b)))
            .or(self.hover_length)?;
        Some((LiveLabel::Distance, units.length_text(d)))
    }

    pub fn moved(&mut self, w: &World, p: &Pointer) {
        self.cursor = Some(p.inf.point);
        self.hover_length = None;
        if matches!(self.run, Run::Idle)
            && p.inf.kind == Kind::OnEdge
            && p.inf.source.kind() == TagKind::Edge
            && let Some(model) = w.scenes.object(p.inf.source)
            && let Some([a, b]) = w.edge_ends(model, kadakas_core::surface::EdgeId(p.inf.source.id()))
        {
            self.hover_length = Some(a.distance(b));
        }
    }

    pub fn click(&mut self, p: &Pointer) -> Out {
        let pt = p.inf.point;
        let Run::From { start, origin } = self.run else {
            // The first click.
            self.measured = None;
            let origin = match (p.inf.kind, p.inf.constraint) {
                (Kind::OnEdge | Kind::OnGuide, Some(Constraint::Line { dir, .. })) => {
                    dir.try_normalize().map_or(Origin::Point, |dir| Origin::Line { dir })
                }
                _ => Origin::Point,
            };
            self.run = Run::From { start: pt, origin };
            self.cursor = Some(pt);
            self.hover_length = None;
            return Out::default();
        };
        self.run = Run::Idle;
        if !self.creating {
            if pt.distance(start) < MIN_LEN {
                return Out::msg(tr!("uic-modeling-too-small"));
            }
            self.measured = Some((start, pt));
            return Out::default();
        }
        match origin {
            Origin::Line { dir } => {
                if perpendicular(pt - start, dir).length() < MIN_LEN {
                    return Out::msg(tr!("uic-modeling-guide-on-the-line"));
                }
                guide(GuideSpec::line(pt, dir))
            }
            Origin::Point => match (pt - start).try_normalize() {
                Some(dir) if pt.distance(start) >= MIN_LEN => guide(GuideSpec::line(start, dir)),
                _ => guide(GuideSpec::point(start)),
            },
        }
    }

    /// A typed length.
    pub fn apply(&mut self, v: Value) -> Out {
        let Value::Length(d) = v else {
            return Out::default();
        };
        if !d.is_finite() || d == 0.0 {
            return Out::msg(tr!("uic-modeling-too-small"));
        }
        match self.run {
            Run::From { start, origin } => {
                let cursor = self.cursor.unwrap_or(start + DVec3::X);
                self.run = Run::Idle;
                if !self.creating {
                    // The end of the measure, along the pointer.
                    let dir = (cursor - start).try_normalize().unwrap_or(DVec3::X);
                    self.measured = Some((start, start + dir * d));
                    return Out::default();
                }
                match origin {
                    Origin::Line { dir } => {
                        // The side of the pointer; a negative length takes the other side.
                        let side = perpendicular(cursor - start, dir).try_normalize().unwrap_or_else(|| {
                            let axis = if dir.cross(DVec3::Z).length() > 1e-6 {
                                DVec3::Z
                            } else {
                                DVec3::X
                            };
                            dir.cross(axis).normalize()
                        });
                        guide(GuideSpec::line(start + side * d, dir))
                    }
                    Origin::Point => {
                        let dir = (cursor - start).try_normalize().unwrap_or(DVec3::X);
                        let dir = if d < 0.0 { -dir } else { dir };
                        guide(GuideSpec::segment(start, dir, d.abs()))
                    }
                }
            }
            Run::Idle => match self.measured {
                Some((a, b)) => {
                    let measured = a.distance(b);
                    let typed = d.abs();
                    let factor = typed / measured;
                    if !(RESIZE_RANGE.0..=RESIZE_RANGE.1).contains(&factor) {
                        return Out::msg(tr!("uic-modeling-resize-range"));
                    }
                    Out {
                        resize: Some(Resize {
                            factor,
                            about: a,
                            measured,
                            typed,
                        }),
                        ..Out::default()
                    }
                }
                None => Out::msg(tr!("uic-modeling-need-start")),
            },
        }
    }

    pub fn preview(&self, pv: &mut Preview) {
        let Run::From { start, origin } = self.run else {
            return;
        };
        let Some(c) = self.cursor else {
            return;
        };
        pv.shape.push([start, c]);
        if !self.creating {
            return;
        }
        match origin {
            Origin::Line { dir } => {
                let off = perpendicular(c - start, dir);
                if off.length() >= MIN_LEN {
                    pv.guides.push(PreviewGuide::line(start + off, dir));
                    pv.shape.clear();
                    pv.shape.push([start, start + off]);
                }
            }
            Origin::Point => {
                if let Some(dir) = (c - start).try_normalize() {
                    pv.guides.push(PreviewGuide::line(start, dir));
                }
            }
        }
    }
}

fn guide(spec: GuideSpec) -> Out {
    Out::act(Action::AddGuides {
        guides: vec![spec],
    })
}

/// A guide that the window draws while the tool runs, in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewGuide {
    pub at: DVec3,
    pub dir: DVec3,
    /// Finite length, else a line without ends.
    pub length: Option<f64>,
}

impl PreviewGuide {
    fn line(at: DVec3, dir: DVec3) -> PreviewGuide {
        PreviewGuide {
            at,
            dir,
            length: None,
        }
    }
}
