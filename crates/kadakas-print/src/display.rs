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
        PathPrim {
            segs,
            closed,
            stroke: Some(color),
            fill: None,
            width,
            dash: None,
        }
    }

    /// Move and line pieces through the points.
    pub fn polyline(points: &[DVec2]) -> Vec<PSeg> {
        points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i == 0 {
                    PSeg::Move(*p)
                } else {
                    PSeg::Line(*p)
                }
            })
            .collect()
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
                    out.push(
                        cur * (u * u * u)
                            + c1 * (3.0 * u * u * t)
                            + c2 * (3.0 * u * t * t)
                            + p * (t * t * t),
                    );
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

impl Prim {
    /// The primitive moved by `d` mm.
    fn shifted(&self, d: DVec2) -> Prim {
        let seg = |s: &PSeg| match *s {
            PSeg::Move(p) => PSeg::Move(p + d),
            PSeg::Line(p) => PSeg::Line(p + d),
            PSeg::Cubic(a, b, p) => PSeg::Cubic(a + d, b + d, p + d),
        };
        match self {
            Prim::Path(p) => Prim::Path(PathPrim {
                segs: p.segs.iter().map(seg).collect(),
                ..p.clone()
            }),
            Prim::Text(t) => Prim::Text(TextPrim {
                at: t.at + d,
                ..t.clone()
            }),
            Prim::Image(i) => Prim::Image(ImagePrim {
                min: i.min + d,
                max: i.max + d,
                ..i.clone()
            }),
            Prim::PushClip(poly) => Prim::PushClip(poly.iter().map(|p| *p + d).collect()),
            Prim::PopClip => Prim::PopClip,
        }
    }
}

impl Page {
    /// Two pages side by side on one page: `self` at the left, `other` at the right, a gap with a
    /// thin line between them, and a strip above each with its label. The sheet compare
    /// (REV-03) uses it to show a sheet of version A next to the same sheet of version B.
    pub fn beside(&self, other: &Page, gap: f64, labels: [&str; 2]) -> Page {
        const STRIP: f64 = 8.0;
        const TEXT: f64 = 4.0;
        let height = self.size.y.max(other.size.y) + STRIP;
        let size = DVec2::new(self.size.x + gap + other.size.x, height);
        let right = DVec2::new(self.size.x + gap, 0.0);
        let mut prims: Vec<Prim> = self.prims.clone();
        prims.extend(other.prims.iter().map(|p| p.shifted(right)));
        let line = |x: f64| {
            Prim::Path(PathPrim::stroked(
                PathPrim::polyline(&[DVec2::new(x, 0.0), DVec2::new(x, height)]),
                false,
                [160, 160, 160, 255],
                0.3,
            ))
        };
        prims.push(line(self.size.x + gap / 2.0));
        for (x, label) in [(2.0, labels[0]), (right.x + 2.0, labels[1])] {
            prims.push(Prim::Text(TextPrim {
                at: DVec2::new(x, height - STRIP + 2.0),
                text: label.to_string(),
                height: TEXT,
                color: [40, 40, 40, 255],
                align: Align::Left,
            }));
        }
        Page {
            size,
            title: format!("{} | {}", labels[0], labels[1]),
            prims,
        }
    }
}

#[cfg(test)]
mod beside_tests {
    use super::*;

    fn page(w: f64, h: f64) -> Page {
        let clip = vec![DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, h)];
        Page {
            size: DVec2::new(w, h),
            title: "A".into(),
            prims: vec![
                Prim::PushClip(clip),
                Prim::Path(PathPrim::stroked(
                    vec![
                        PSeg::Move(DVec2::new(1.0, 1.0)),
                        PSeg::Cubic(DVec2::splat(2.0), DVec2::splat(3.0), DVec2::splat(4.0)),
                        PSeg::Line(DVec2::new(5.0, 5.0)),
                    ],
                    false,
                    [0, 0, 0, 255],
                    0.2,
                )),
                Prim::PopClip,
                Prim::Text(TextPrim {
                    at: DVec2::new(1.0, 2.0),
                    text: "t".into(),
                    height: 3.0,
                    color: [0, 0, 0, 255],
                    align: Align::Left,
                }),
                Prim::Image(ImagePrim {
                    min: DVec2::ZERO,
                    max: DVec2::splat(2.0),
                    width: 1,
                    height: 1,
                    rgb: Arc::new(vec![1, 2, 3]),
                }),
            ],
        }
    }

    #[test]
    fn two_pages_stand_side_by_side_with_labels_and_a_divider() {
        let (a, b) = (page(100.0, 50.0), page(80.0, 70.0));
        let both = a.beside(&b, 10.0, ["Before", "After"]);
        assert_eq!(both.size, DVec2::new(190.0, 78.0));
        assert_eq!(both.title, "Before | After");
        // The left page is as it was; the right page is moved by its width and the gap.
        assert_eq!(&both.prims[..a.prims.len()], &a.prims[..]);
        let moved = &both.prims[a.prims.len()..a.prims.len() + b.prims.len()];
        assert_eq!(
            moved[0],
            Prim::PushClip(vec![
                DVec2::new(110.0, 0.0),
                DVec2::new(190.0, 0.0),
                DVec2::new(190.0, 70.0),
            ])
        );
        let Prim::Path(p) = &moved[1] else { panic!() };
        assert_eq!(p.segs[0], PSeg::Move(DVec2::new(111.0, 1.0)));
        assert_eq!(
            p.segs[1],
            PSeg::Cubic(
                DVec2::new(112.0, 2.0),
                DVec2::new(113.0, 3.0),
                DVec2::new(114.0, 4.0)
            )
        );
        assert_eq!(p.segs[2], PSeg::Line(DVec2::new(115.0, 5.0)));
        assert_eq!(moved[2], Prim::PopClip);
        let Prim::Text(t) = &moved[3] else { panic!() };
        assert_eq!(t.at, DVec2::new(111.0, 2.0));
        let Prim::Image(i) = &moved[4] else { panic!() };
        assert_eq!((i.min.x, i.max.x), (110.0, 112.0));
        // The two labels come last.
        let texts: Vec<&str> = both
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text(t) if t.height == 4.0 => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["Before", "After"]);
        // Both outputs draw it.
        assert!(crate::svg::page_svg(&both).contains("Before"));
        assert!(crate::png::page_png(&both, 20.0).is_ok());
    }
}
