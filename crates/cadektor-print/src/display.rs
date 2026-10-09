//! The display list of a sheet: what to draw, in paper space (mm, Y up, origin at the lower left).
//! PDF, SVG, PNG and the app canvas all draw from it.

use std::sync::Arc;

use glam::DVec2;

pub type Rgba = [u8; 4];

/// One piece of a path. Arcs are cubic curves, so every output has exact curves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PSeg {
    Move(DVec2),
    Line(DVec2),
    Cubic(DVec2, DVec2, DVec2),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathPrim {
    pub segs: Vec<PSeg>,
    pub closed: bool,
    pub stroke: Option<Rgba>,
    pub fill: Option<Rgba>,
    /// Line weight in mm.
    pub width: f64,
    /// Dash and gap in mm.
    pub dash: Option<[f64; 2]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextPrim {
    /// Start of the baseline (or its middle for centered text).
    pub at: DVec2,
    pub text: String,
    /// Height of capital letters' em in mm: the font size.
    pub height: f64,
    pub color: Rgba,
    pub align: Align,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImagePrim {
    pub min: DVec2,
    pub max: DVec2,
    pub width: u32,
    pub height: u32,
    /// RGB, 8 bits per channel, rows from the top.
    pub rgb: Arc<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Path(PathPrim),
    Text(TextPrim),
    Image(ImagePrim),
    /// Everything up to the matching `PopClip` is cut at this polygon.
    PushClip(Vec<DVec2>),
    PopClip,
}

/// One sheet, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// Page size in mm.
    pub size: DVec2,
    /// "L-101 Planting plan".
    pub title: String,
    pub prims: Vec<Prim>,
}

impl PathPrim {
    pub fn stroked(segs: Vec<PSeg>, closed: bool, color: Rgba, width: f64) -> PathPrim {
        PathPrim { segs, closed, stroke: Some(color), fill: None, width, dash: None }
    }

    /// Move and line pieces through the points.
    pub fn polyline(points: &[DVec2]) -> Vec<PSeg> {
        points.iter().enumerate().map(|(i, p)| if i == 0 { PSeg::Move(*p) } else { PSeg::Line(*p) }).collect()
    }
}

/// Cubic curves for a circular arc, from `start` over `sweep` radians (counterclockwise when
/// positive). The first segment is a `Move` when `with_move` is set.
pub fn arc_segs(center: DVec2, radius: f64, start: f64, sweep: f64, with_move: bool) -> Vec<PSeg> {
    let mut out = Vec::new();
    let at = |a: f64| center + DVec2::from_angle(a) * radius;
    if with_move {
        out.push(PSeg::Move(at(start)));
    }
    let n = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let step = sweep / n as f64;
    // Control point distance for one piece of a circle.
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    for i in 0..n {
        let (a0, a1) = (start + step * i as f64, start + step * (i + 1) as f64);
        let (p0, p1) = (at(a0), at(a1));
        let t0 = DVec2::new(-a0.sin(), a0.cos()) * radius;
        let t1 = DVec2::new(-a1.sin(), a1.cos()) * radius;
        out.push(PSeg::Cubic(p0 + t0 * k, p1 - t1 * k, p1));
    }
    out
}

/// A full circle as four curves.
pub fn circle_segs(center: DVec2, radius: f64) -> Vec<PSeg> {
    arc_segs(center, radius, 0.0, std::f64::consts::TAU, true)
}

/// Points along the curves of a path piece list, for tests and bounds. Cubics are sampled.
pub fn sample(segs: &[PSeg], per_curve: usize) -> Vec<DVec2> {
    let mut out = Vec::new();
    let mut cur = DVec2::ZERO;
    for s in segs {
        match *s {
            PSeg::Move(p) | PSeg::Line(p) => {
                out.push(p);
                cur = p;
            }
            PSeg::Cubic(c1, c2, p) => {
                for i in 1..=per_curve {
                    let t = i as f64 / per_curve as f64;
                    let u = 1.0 - t;
                    out.push(cur * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p * (t * t * t));
                }
                cur = p;
            }
        }
    }
    out
}

/// A rough width of a text in mm, for layout. Real fonts differ a little.
pub fn text_width(text: &str, height: f64) -> f64 {
    text.chars().count() as f64 * height * 0.52
}
