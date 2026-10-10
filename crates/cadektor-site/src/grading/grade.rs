//! Grade objects: a line with elevations at both ends, or an elevation at the start and a slope.
//!
//! The object is a line (`Shape::Line`) or a curve (an open `Shape::Polyline`, arcs allowed) with
//! settings under `Object::data["grade_object"]`. The slope, the end elevation and the label follow
//! from the path and the settings each time. So they update when a point moves. On a curve the
//! elevation changes in step with the distance along the curve. With `snap` the elevations come
//! live from the site model. With `site_modifier` the grade also changes the proposed surface:
//! it makes a strip of the given `width` along the path that follows the grade.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use cadektor_core::model::{MAX_COORD, Object, ObjectId, Shape};

use super::draw::{Drawn, Label, Poly};
use super::{Follow, Ground, finite, meters, read};
use crate::modifier::{Modifier, PadSlope};

/// Width of the strip that a grade makes when it is a site modifier, without a setting.
pub const DEFAULT_WIDTH: f64 = 1500.0;
/// Most pieces of the strip of a grade. A longer path is thinned out.
pub const MAX_PIECES: usize = 400;

/// The key in `Object::data` that holds the settings.
pub const KEY: &str = "grade_object";

/// A slope that is given with the start elevation. A positive value rises from the start to the
/// end and a negative value falls.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeSpec {
    /// Rise over run in percent: 2 is 2 percent.
    Percent(f64),
    /// Run per unit of rise: 50 means 1:50.
    Ratio(f64),
    /// Angle in degrees.
    Angle(f64),
}

impl SlopeSpec {
    /// The slope as a signed percent.
    pub fn percent(self) -> Result<f64, String> {
        match self {
            SlopeSpec::Percent(p) => finite("slope percent", p),
            SlopeSpec::Ratio(r) => {
                finite("slope ratio", r)?;
                if r == 0.0 {
                    Err("a ratio of 0 is not a slope; use 1:N with N other than 0".into())
                } else {
                    Ok(100.0 / r)
                }
            }
            SlopeSpec::Angle(a) => {
                finite("slope angle", a)?;
                if a.abs() >= 90.0 {
                    Err("a slope angle must be between -90 and 90 degrees".into())
                } else {
                    Ok(a.to_radians().tan() * 100.0)
                }
            }
        }
    }
}

/// The settings of a grade object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GradeData {
    /// Elevation at the start point of the line.
    pub elevation_a: f64,
    /// Elevation at the end point. Without it the slope sets the end elevation.
    pub elevation_b: Option<f64>,
    pub slope: Option<SlopeSpec>,
    /// Read both elevations from this surface of the site model, live.
    pub snap: Option<Follow>,
    pub site: Option<ObjectId>,
    /// Show the slope label and the arrow.
    pub label: bool,
    /// Show the elevations at both ends.
    pub end_tags: bool,
    /// The grade changes the proposed surface of the site model: the ground along the path gets
    /// the elevations of the grade, in a strip of `width`. Needs explicit elevations (no `snap`).
    pub site_modifier: bool,
    /// Width of that strip in mm.
    pub width: f64,
}

impl Default for GradeData {
    fn default() -> Self {
        GradeData {
            elevation_a: 0.0,
            elevation_b: None,
            slope: None,
            snap: None,
            site: None,
            label: true,
            end_tags: true,
            site_modifier: false,
            width: DEFAULT_WIDTH,
        }
    }
}

impl GradeData {
    pub fn validate(&self) -> Result<(), String> {
        finite("elevation_a", self.elevation_a)?;
        if let Some(b) = self.elevation_b {
            finite("elevation_b", b)?;
        }
        if self.elevation_a.abs() > MAX_COORD
            || self.elevation_b.is_some_and(|b| b.abs() > MAX_COORD)
        {
            return Err("an elevation is too large".into());
        }
        if let Some(s) = self.slope {
            s.percent()?;
        }
        if self.elevation_b.is_none() && self.slope.is_none() && self.snap.is_none() {
            return Err("give elevation_b, or a slope, or snap to a site model".into());
        }
        finite("width", self.width)?;
        if !(self.width > 0.0 && self.width <= 100_000.0) {
            return Err("width must be more than 0 and at most 100 000 mm".into());
        }
        if self.site_modifier && self.snap.is_some() {
            return Err(
                "a grade that picks up its elevations from the site model cannot also change it. Give the elevations or a slope"
                    .into(),
            );
        }
        Ok(())
    }
}

/// The result for a grade object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    pub a: DVec2,
    pub b: DVec2,
    /// The length along the path (the chord for a straight line).
    pub length: f64,
    /// The point at half the length along the path.
    pub mid: DVec2,
    /// The direction of the path at `mid` (unit length).
    pub tangent: DVec2,
    pub elevation_a: f64,
    pub elevation_b: f64,
    /// `elevation_b - elevation_a`.
    pub rise: f64,
    /// Signed: positive rises from a to b.
    pub percent: f64,
    pub angle_deg: f64,
    /// Run per unit of rise (the 50 of 1:50). `None` on level ground.
    pub ratio: Option<f64>,
}

impl Grade {
    /// True when the ground falls from a to b.
    pub fn falls(&self) -> bool {
        self.rise < 0.0
    }

    /// The label: percent and ratio.
    pub fn label(&self) -> String {
        match self.ratio {
            Some(r) => format!("{:.1}% (1:{:.0})", self.percent.abs(), r),
            None => "0.0% (level)".into(),
        }
    }
}

/// Solves the grade between two points.
pub fn solve(
    a: DVec2,
    b: DVec2,
    d: &GradeData,
    ground: Option<Ground<'_>>,
) -> Result<Grade, String> {
    solve_path(&[a, b], d, ground)
}

/// The point at distance `s` along the path and the direction of the path there.
fn point_along(path: &[DVec2], s: f64) -> (DVec2, DVec2) {
    let mut left = s;
    for w in path.windows(2) {
        let seg = w[0].distance(w[1]);
        if left <= seg || std::ptr::eq(&w[1], path.last().expect("not empty")) {
            let dir = (w[1] - w[0]).normalize_or_zero();
            return (w[0] + dir * left.min(seg), dir);
        }
        left -= seg;
    }
    (path[0], DVec2::X)
}

/// Solves the grade along a path (two or more points). The elevation changes with the distance
/// along the path, so a curve has one slope all along.
pub fn solve_path(
    path: &[DVec2],
    d: &GradeData,
    ground: Option<Ground<'_>>,
) -> Result<Grade, String> {
    let (Some(&a), Some(&b)) = (path.first(), path.last()) else {
        return Err("a grade needs a path".into());
    };
    let length: f64 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    if !(length > 0.0 && length.is_finite()) || a == b && path.len() == 2 {
        return Err("the end points of a grade must be apart".into());
    }
    let (mid, tangent) = point_along(path, length / 2.0);
    let (za, zb) = match (d.snap, ground) {
        (Some(f), Some(g)) => match (g.exact(None, a, f), g.exact(None, b, f)) {
            (Some(za), Some(zb)) => (za, zb),
            _ => {
                return Err(
                    "an end point is outside the site model, so it cannot pick up an elevation"
                        .into(),
                );
            }
        },
        (Some(_), None) => return Err("there is no site model to pick up elevations from".into()),
        (None, _) => {
            let zb = match (d.elevation_b, d.slope) {
                (Some(b), _) => b,
                (None, Some(s)) => d.elevation_a + length * s.percent()? / 100.0,
                (None, None) => return Err("give elevation_b or a slope".into()),
            };
            (d.elevation_a, zb)
        }
    };
    let rise = zb - za;
    Ok(Grade {
        a,
        b,
        length,
        mid,
        tangent,
        elevation_a: za,
        elevation_b: zb,
        rise,
        percent: rise / length * 100.0,
        angle_deg: (rise / length).atan().to_degrees(),
        ratio: (rise.abs() > 1e-9).then(|| length / rise.abs()),
    })
}

/// The settings of a grade object. `None` for other objects.
pub fn data(obj: &Object) -> Option<GradeData> {
    if !matches!(
        obj.shape,
        Shape::Line { .. } | Shape::Polyline { closed: false, .. }
    ) {
        return None;
    }
    read(obj, KEY)
}

/// The path of a grade object in plan: the line, or the curve flattened.
pub fn path_of(obj: &Object, tolerance: f64) -> Option<Vec<DVec2>> {
    match &obj.shape {
        Shape::Line { a, b } => Some(vec![*a, *b]),
        Shape::Polyline {
            points,
            closed: false,
        } if points.len() >= 2 => {
            let mut p = cadektor_core::curve::flatten(points, false, tolerance);
            p.dedup_by(|a, b| a.distance_squared(*b) < 1e-12);
            Some(p)
        }
        _ => None,
    }
}

/// The grade of an object, if it is a grade object that can be solved.
pub fn evaluate(obj: &Object, ground: Option<Ground<'_>>) -> Option<(GradeData, Grade)> {
    let d = data(obj)?;
    let path = path_of(obj, 2.0)?;
    let g = solve_path(&path, &d, ground).ok()?;
    Some((d, g))
}

/// The pads that make a grade a site modifier: a strip of `width` along the path, in pieces.
/// Each piece is a pad that slopes along the path at the grade. Empty unless the grade asks for it
/// and has explicit elevations.
pub fn pads_of(obj: &Object) -> Vec<Modifier> {
    let Some(d) = data(obj).filter(|d| d.site_modifier && d.validate().is_ok()) else {
        return Vec::new();
    };
    let Some(mut path) = path_of(obj, 20.0) else {
        return Vec::new();
    };
    if path.len() > MAX_PIECES + 1 {
        let step = path.len().div_ceil(MAX_PIECES);
        let last = *path.last().expect("not empty");
        path = path.iter().copied().step_by(step).collect();
        if path.last() != Some(&last) {
            path.push(last);
        }
    }
    let Ok(g) = solve_path(&path, &d, None) else {
        return Vec::new();
    };
    let (left, right) = super::wall::offsets(&path, d.width / 2.0);
    let mut run = 0.0;
    let mut out = Vec::new();
    for i in 0..path.len() - 1 {
        let seg = path[i].distance(path[i + 1]);
        if seg < 1e-6 {
            continue;
        }
        let along = (path[i + 1] - path[i]) / seg;
        let z = g.elevation_a + g.rise * (run + seg / 2.0) / g.length;
        run += seg;
        // A pad falls in its direction. Rising along the path means falling backward.
        let fall = if g.rise > 0.0 { -along } else { along };
        out.push(Modifier::Pad {
            boundary: vec![right[i], right[i + 1], left[i + 1], left[i]],
            elevation: z,
            slope: Some(PadSlope {
                direction: fall.y.atan2(fall.x),
                percent: g.percent.abs(),
            }),
        });
    }
    out
}

/// A grade object in plan: an arrow that points downhill at the middle of the line, the slope
/// label along the line and the end elevations. `scale` is the drawing scale of the layer.
pub fn draw(id: ObjectId, d: &GradeData, g: &Grade, scale: f64) -> Drawn {
    let mut out = Drawn::default();
    let dir = g.tangent;
    let n = DVec2::new(-dir.y, dir.x);
    let mid = g.mid;
    let size = 3.0 * scale;
    let height = 2.5 * scale;
    let black = [0, 0, 0, 255];
    if d.label {
        // The arrow: a head at the middle, in the direction of the fall. Level ground has a bar.
        if g.rise.abs() > 1e-9 {
            let d2 = if g.falls() { dir } else { -dir };
            let tip = mid + d2 * size / 2.0;
            let base = mid - d2 * size / 2.0;
            let ring = vec![tip, base + n * size / 2.5, base - n * size / 2.5];
            out.polys
                .push(Poly::new("grade-arrow", id, vec![ring], true).filled(black));
        } else {
            let bar = vec![mid + n * size / 2.0, mid - n * size / 2.0];
            out.polys
                .push(Poly::new("grade-arrow", id, vec![bar], false).stroked(black, 0.25 * scale));
        }
        // The text reads from the left, whichever way the line runs.
        let mut angle = dir.y.atan2(dir.x).to_degrees();
        if angle > 90.0 {
            angle -= 180.0;
        } else if angle < -90.0 {
            angle += 180.0;
        }
        let mut label = Label::new("grade-label", id, mid + n * height * 0.8, g.label(), height);
        label.center = true;
        label.angle_deg = angle;
        out.labels.push(label);
    }
    if d.end_tags {
        for (p, z) in [(g.a, g.elevation_a), (g.b, g.elevation_b)] {
            let at = p - dir * height * 0.5 - n * height * 1.2;
            out.labels
                .push(Label::new("grade-tag", id, at, meters(z, 2), height));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grading::testutil::*;
    use cadektor_core::command::Editor;
    use proptest::prelude::*;
    use serde_json::json;

    fn a() -> DVec2 {
        DVec2::ZERO
    }

    fn b(len: f64) -> DVec2 {
        DVec2::new(len, 0.0)
    }

    #[test]
    fn slope_specs_give_percent() {
        assert_eq!(SlopeSpec::Percent(2.5).percent().unwrap(), 2.5);
        assert!((SlopeSpec::Ratio(50.0).percent().unwrap() - 2.0).abs() < 1e-12);
        assert!((SlopeSpec::Ratio(-20.0).percent().unwrap() + 5.0).abs() < 1e-12);
        assert!((SlopeSpec::Angle(45.0).percent().unwrap() - 100.0).abs() < 1e-9);
        assert!(SlopeSpec::Ratio(0.0).percent().is_err());
        assert!(SlopeSpec::Angle(90.0).percent().is_err());
        assert!(SlopeSpec::Percent(f64::NAN).percent().is_err());
        assert!(SlopeSpec::Ratio(f64::INFINITY).percent().is_err());
        assert!(SlopeSpec::Angle(f64::NAN).percent().is_err());
    }

    #[test]
    fn two_elevations_give_the_slope() {
        let d = GradeData {
            elevation_a: 10_000.0,
            elevation_b: Some(10_500.0),
            ..GradeData::default()
        };
        let g = solve(a(), b(25_000.0), &d, None).unwrap();
        assert_eq!((g.rise, g.elevation_b), (500.0, 10_500.0));
        assert!((g.percent - 2.0).abs() < 1e-12);
        assert!((g.ratio.unwrap() - 50.0).abs() < 1e-9);
        assert!(!g.falls());
        assert_eq!(g.label(), "2.0% (1:50)");
        let down = GradeData {
            elevation_b: Some(9_000.0),
            ..d.clone()
        };
        let g = solve(a(), b(10_000.0), &down, None).unwrap();
        assert!(g.falls() && (g.percent + 10.0).abs() < 1e-12);
        assert_eq!(g.label(), "10.0% (1:10)");
        let level = GradeData {
            elevation_b: Some(10_000.0),
            ..d
        };
        let g = solve(a(), b(10_000.0), &level, None).unwrap();
        assert!(g.ratio.is_none() && g.label().contains("level"));
    }

    #[test]
    fn elevation_and_slope_give_the_end() {
        let d = GradeData {
            elevation_a: 1000.0,
            slope: Some(SlopeSpec::Percent(-4.0)),
            ..GradeData::default()
        };
        let g = solve(a(), b(5000.0), &d, None).unwrap();
        assert!((g.elevation_b - 800.0).abs() < 1e-9);
        // A longer line gets a different end elevation: the grade follows its end points.
        let g = solve(a(), b(10_000.0), &d, None).unwrap();
        assert!((g.elevation_b - 600.0).abs() < 1e-9);
        let ratio = GradeData {
            slope: Some(SlopeSpec::Ratio(20.0)),
            ..d.clone()
        };
        assert!((solve(a(), b(2000.0), &ratio, None).unwrap().rise - 100.0).abs() < 1e-9);
        assert!(
            solve(
                a(),
                b(2000.0),
                &GradeData {
                    slope: Some(SlopeSpec::Ratio(0.0)),
                    ..d.clone()
                },
                None
            )
            .is_err()
        );
        assert!(solve(a(), b(2000.0), &GradeData::default(), None).is_err());
    }

    #[test]
    fn bad_input_is_refused() {
        let d = GradeData {
            elevation_b: Some(1.0),
            ..GradeData::default()
        };
        assert!(solve(a(), a(), &d, None).is_err());
        assert!(solve(a(), DVec2::new(f64::NAN, 0.0), &d, None).is_err());
        assert!(d.validate().is_ok());
        assert!(GradeData::default().validate().is_err());
        assert!(
            GradeData {
                elevation_a: f64::NAN,
                ..d.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            GradeData {
                elevation_b: Some(f64::NAN),
                ..d.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            GradeData {
                elevation_a: 1e12,
                ..d.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            GradeData {
                elevation_b: Some(-1e12),
                ..d.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            GradeData {
                slope: Some(SlopeSpec::Angle(95.0)),
                ..d.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            GradeData {
                snap: Some(Follow::Existing),
                ..GradeData::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn snapping_reads_the_ground() {
        let ground = |p: DVec2, f: Follow| {
            (p.x >= 0.0).then(|| {
                if f == Follow::Existing {
                    p.x / 10.0
                } else {
                    p.x / 5.0
                }
            })
        };
        let d = GradeData {
            snap: Some(Follow::Existing),
            ..GradeData::default()
        };
        let g = solve(a(), b(1000.0), &d, Some(&ground)).unwrap();
        assert_eq!((g.elevation_a, g.elevation_b), (0.0, 100.0));
        let p = GradeData {
            snap: Some(Follow::Proposed),
            ..GradeData::default()
        };
        assert_eq!(
            solve(a(), b(1000.0), &p, Some(&ground))
                .unwrap()
                .elevation_b,
            200.0
        );
        assert!(
            solve(DVec2::new(-5.0, 0.0), b(1000.0), &d, Some(&ground))
                .unwrap_err()
                .contains("outside")
        );
        assert!(
            solve(a(), b(1000.0), &d, None)
                .unwrap_err()
                .contains("no site model")
        );
    }

    proptest! {
        #[test]
        fn slope_math_round_trips(len in 100.0f64..50_000.0, za in -5000.0f64..5000.0, pct in -60.0f64..60.0) {
            let d = GradeData { elevation_a: za, slope: Some(SlopeSpec::Percent(pct)), ..GradeData::default() };
            let g = solve(a(), b(len), &d, None).unwrap();
            prop_assert!((g.percent - pct).abs() < 1e-9);
            prop_assert!((g.angle_deg - (pct / 100.0).atan().to_degrees()).abs() < 1e-9);
            // The same grade from two elevations.
            let e = GradeData { elevation_a: za, elevation_b: Some(g.elevation_b), ..GradeData::default() };
            let h = solve(a(), b(len), &e, None).unwrap();
            prop_assert!((h.percent - g.percent).abs() < 1e-9);
            // The ratio and the angle spec give the same percent.
            if pct.abs() > 0.5 {
                prop_assert!((SlopeSpec::Ratio(100.0 / pct).percent().unwrap() - pct).abs() < 1e-9);
                prop_assert!((SlopeSpec::Angle((pct / 100.0).atan().to_degrees()).percent().unwrap() - pct).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn objects_update_when_the_end_moves() {
        let mut ed = Editor::default();
        let d = GradeData {
            elevation_a: 1000.0,
            slope: Some(SlopeSpec::Percent(2.0)),
            ..GradeData::default()
        };
        let id = add_with(
            &mut ed,
            Shape::Line {
                a: a(),
                b: b(10_000.0),
            },
            KEY,
            serde_json::to_value(&d).unwrap(),
        );
        let (_, g) = evaluate(ed.doc().object(id).unwrap(), None).unwrap();
        assert!((g.elevation_b - 1200.0).abs() < 1e-9);
        ed.execute(&cadektor_core::command::Command::ModifyObject {
            id,
            shape: Some(Shape::Line {
                a: a(),
                b: b(20_000.0),
            }),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        let (_, g) = evaluate(ed.doc().object(id).unwrap(), None).unwrap();
        assert!((g.elevation_b - 1400.0).abs() < 1e-9);
        // Other objects and broken settings give nothing.
        let circle = add(
            &mut ed,
            Shape::Circle {
                center: a(),
                radius: 5.0,
            },
        );
        assert!(evaluate(ed.doc().object(circle).unwrap(), None).is_none());
        assert!(data(ed.doc().object(circle).unwrap()).is_none());
        let broken = add_with(
            &mut ed,
            Shape::Line { a: a(), b: b(10.0) },
            KEY,
            json!({"slope": "steep"}),
        );
        assert!(evaluate(ed.doc().object(broken).unwrap(), None).is_none());
        let none = add_with(&mut ed, Shape::Line { a: a(), b: b(10.0) }, KEY, json!({}));
        assert!(evaluate(ed.doc().object(none).unwrap(), None).is_none());
    }

    #[test]
    fn drawing_the_grade() {
        let d = GradeData {
            elevation_a: 1000.0,
            elevation_b: Some(1200.0),
            ..GradeData::default()
        };
        let g = solve(a(), b(10_000.0), &d, None).unwrap();
        let out = draw(ObjectId(7), &d, &g, 100.0);
        let texts: Vec<&str> = out.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["2.0% (1:50)", "1.00", "1.20"]);
        assert!(out.polys.len() == 1 && out.polys[0].tag == "grade-arrow");
        // The arrow points downhill: for a rise from a to b the tip is left of the middle.
        assert!(out.polys[0].rings[0][0].x < 5000.0);
        let svg = out.svg();
        assert!(
            svg.contains("grade-arrow")
                && svg.contains("grade-label")
                && svg.matches("grade-tag").count() == 2
        );
        let down = GradeData {
            elevation_b: Some(800.0),
            ..d.clone()
        };
        let g2 = solve(a(), b(10_000.0), &down, None).unwrap();
        assert!(draw(ObjectId(7), &down, &g2, 100.0).polys[0].rings[0][0].x > 5000.0);
        // Level ground has a bar.
        let level = GradeData {
            elevation_b: Some(1000.0),
            label: true,
            end_tags: false,
            ..d.clone()
        };
        let g3 = solve(b(10_000.0), a(), &level, None).unwrap();
        let s3 = draw(ObjectId(7), &level, &g3, 100.0);
        assert!(!s3.polys[0].closed && s3.labels.len() == 1 && s3.labels[0].angle_deg == 0.0);
        let none = GradeData {
            label: false,
            end_tags: false,
            ..d.clone()
        };
        assert!(draw(ObjectId(7), &none, &g, 100.0).is_empty());
        // Steep lines in both directions rotate by less than 90 degrees.
        for (p, q) in [
            (DVec2::new(0.0, 0.0), DVec2::new(1.0, 5.0)),
            (DVec2::new(0.0, 0.0), DVec2::new(-1.0, 5.0)),
            (DVec2::new(0.0, 5.0), DVec2::new(1.0, 0.0)),
            (DVec2::new(0.0, 5.0), DVec2::new(-1.0, 0.0)),
        ] {
            let gg = solve(p, q, &d, None).unwrap();
            let r = draw(ObjectId(1), &d, &gg, 100.0).labels[0].angle_deg;
            assert!(r.abs() <= 90.0);
        }
    }

    fn arc_shape(radius: f64) -> Shape {
        use cadektor_core::curve::{Vertex, VertexKind};
        // A half circle from (-r, 0) over the top (0, r) to (r, 0).
        Shape::Polyline {
            points: vec![
                Vertex {
                    p: DVec2::new(-radius, 0.0),
                    kind: VertexKind::Corner,
                },
                Vertex {
                    p: DVec2::new(0.0, radius),
                    kind: VertexKind::PointOnArc,
                },
                Vertex {
                    p: DVec2::new(radius, 0.0),
                    kind: VertexKind::PointOnArc,
                },
            ],
            closed: false,
        }
    }

    #[test]
    fn a_curved_grade_runs_along_the_curve() {
        let mut ed = Editor::default();
        let d = GradeData {
            elevation_a: 1000.0,
            slope: Some(SlopeSpec::Percent(2.0)),
            ..GradeData::default()
        };
        let id = add_with(
            &mut ed,
            arc_shape(5000.0),
            KEY,
            serde_json::to_value(&d).unwrap(),
        );
        let (_, g) = evaluate(ed.doc().object(id).unwrap(), None).unwrap();
        let arc = std::f64::consts::PI * 5000.0;
        assert!((g.length - arc).abs() < 5.0, "{}", g.length);
        assert!((g.elevation_b - (1000.0 + arc * 0.02)).abs() < 0.2);
        assert!((g.percent - 2.0).abs() < 0.01);
        // Half way along the curve is the top of the arc, and the tangent runs along +X there.
        assert!(g.mid.distance(DVec2::new(0.0, 5000.0)) < 5.0, "{:?}", g.mid);
        assert!(g.tangent.dot(DVec2::X) > 0.99);
        let out = draw(id, &d, &g, 100.0);
        assert_eq!(out.labels.len(), 3);
        assert!(out.labels[0].center);
        // A closed curve is not a grade.
        let closed = Shape::Polyline {
            points: match arc_shape(1000.0) {
                Shape::Polyline { points, .. } => points,
                _ => unreachable!(),
            },
            closed: true,
        };
        let cid = add_with(&mut ed, closed, KEY, serde_json::to_value(&d).unwrap());
        assert!(data(ed.doc().object(cid).unwrap()).is_none());
        // A path that returns to its start has no end apart.
        let z = path_of(ed.doc().object(id).unwrap(), 2.0).unwrap();
        assert!(z.len() > 10);
        assert!(solve_path(&[], &d, None).is_err());
        assert!(solve_path(&[a(), a()], &d, None).is_err());
    }

    fn modifier_grade(ed: &mut Editor, shape: Shape, d: GradeData) -> ObjectId {
        add_with(ed, shape, KEY, serde_json::to_value(&d).unwrap())
    }

    #[test]
    fn a_grade_that_is_a_site_modifier_makes_pads_along_the_path() {
        let mut ed = Editor::default();
        let site = site(&mut ed, |_, _| 0.0);
        let d = GradeData {
            elevation_a: 100.0,
            slope: Some(SlopeSpec::Percent(2.0)),
            site_modifier: true,
            width: 1000.0,
            ..GradeData::default()
        };
        let id = modifier_grade(
            &mut ed,
            Shape::Line {
                a: DVec2::new(1000.0, 5000.0),
                b: DVec2::new(9000.0, 5000.0),
            },
            d.clone(),
        );
        let pads = pads_of(ed.doc().object(id).unwrap());
        assert_eq!(pads.len(), 1);
        let site_result = crate::SiteResult::compute(ed.doc(), site).unwrap();
        assert!(site_result.proposed.is_some());
        let z = |x: f64| {
            site_result
                .elevation_at(DVec2::new(x, 5000.0))
                .expect("on the model")
        };
        // 8 m long at 2 percent: 160 mm of rise from 100 mm.
        assert!((z(1500.0) - 110.0).abs() < 1.0, "{}", z(1500.0));
        assert!((z(8500.0) - 250.0).abs() < 1.0, "{}", z(8500.0));
        // Outside the strip the ground stays.
        assert!(
            site_result
                .elevation_at(DVec2::new(5000.0, 7000.0))
                .unwrap()
                .abs()
                < 1e-9
        );
        // The same grade without the switch changes nothing.
        let off = modifier_grade(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X * 5000.0,
            },
            GradeData {
                site_modifier: false,
                ..d.clone()
            },
        );
        assert!(pads_of(ed.doc().object(off).unwrap()).is_empty());
        // A falling curve makes many pieces, each with the grade as slope.
        let curve_id = modifier_grade(
            &mut ed,
            arc_shape(4000.0),
            GradeData {
                slope: Some(SlopeSpec::Percent(-3.0)),
                ..d.clone()
            },
        );
        let pads = pads_of(ed.doc().object(curve_id).unwrap());
        assert!(pads.len() > 10 && pads.len() <= MAX_PIECES);
        for p in &pads {
            let Modifier::Pad { slope, .. } = p else {
                panic!()
            };
            assert!((slope.unwrap().percent - 3.0).abs() < 1e-6);
        }
        // Validation.
        let snapped = GradeData {
            snap: Some(Follow::Existing),
            site_modifier: true,
            ..GradeData::default()
        };
        assert!(snapped.validate().is_err());
        for w in [0.0, -5.0, f64::NAN, 1e9] {
            assert!(
                GradeData {
                    width: w,
                    ..d.clone()
                }
                .validate()
                .is_err()
            );
        }
    }
}
