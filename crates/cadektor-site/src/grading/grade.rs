//! Grade objects: a line with elevations at both ends, or an elevation at the start and a slope.
//!
//! The object is a line (`Shape::Line`) with settings under `Object::data["grade_object"]`. The
//! slope, the end elevation and the label follow from the line and the settings each time. So they
//! update when an end point moves. With `snap` the elevations come live from the site model.

use std::fmt::Write;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use cadektor_core::model::{MAX_COORD, Object, ObjectId, Shape};

use super::{Follow, Ground, esc, finite, meters, read};

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
        Ok(())
    }
}

/// The result for a grade object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    pub a: DVec2,
    pub b: DVec2,
    pub length: f64,
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
    let length = a.distance(b);
    if !(length > 0.0 && length.is_finite()) {
        return Err("the end points of a grade must be apart".into());
    }
    let (za, zb) = match (d.snap, ground) {
        (Some(f), Some(g)) => match (g(a, f), g(b, f)) {
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
    if !matches!(obj.shape, Shape::Line { .. }) {
        return None;
    }
    read(obj, KEY)
}

/// The grade of an object, if it is a grade object that can be solved.
pub fn evaluate(obj: &Object, ground: Option<Ground<'_>>) -> Option<(GradeData, Grade)> {
    let Shape::Line { a, b } = &obj.shape else {
        return None;
    };
    let d = data(obj)?;
    let g = solve(*a, *b, &d, ground).ok()?;
    Some((d, g))
}

/// SVG of a grade object: an arrow that points downhill at the middle of the line, the slope
/// label along the line and the end elevations. `scale` is the drawing scale of the layer.
pub fn svg(id: ObjectId, d: &GradeData, g: &Grade, scale: f64) -> String {
    let mut out = String::new();
    let dir = (g.b - g.a) / g.length;
    let n = DVec2::new(-dir.y, dir.x);
    let mid = (g.a + g.b) / 2.0;
    let size = 3.0 * scale;
    let height = 2.5 * scale;
    if d.label {
        // The arrow: a head at the middle, in the direction of the fall. Level ground has a bar.
        if g.rise.abs() > 1e-9 {
            let d2 = if g.falls() { dir } else { -dir };
            let tip = mid + d2 * size / 2.0;
            let (p, q) = (
                mid - d2 * size / 2.0 + n * size / 2.5,
                mid - d2 * size / 2.0 - n * size / 2.5,
            );
            let _ = writeln!(
                out,
                r#"<polygon points="{},{} {},{} {},{}" fill="black" data-kind="grade-arrow" data-grade="{id}"/>"#,
                tip.x, -tip.y, p.x, -p.y, q.x, -q.y
            );
        } else {
            let (p, q) = (mid + n * size / 2.0, mid - n * size / 2.0);
            let _ = writeln!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="black" data-kind="grade-arrow" data-grade="{id}"/>"#,
                p.x, -p.y, q.x, -q.y
            );
        }
        let at = mid + n * height * 0.8;
        let mut angle = (-dir.y).atan2(dir.x).to_degrees();
        if angle > 90.0 {
            angle -= 180.0;
        } else if angle < -90.0 {
            angle += 180.0;
        }
        let _ = writeln!(
            out,
            r#"<text x="{}" y="{}" font-size="{height}" font-family="sans-serif" text-anchor="middle" fill="black" transform="rotate({angle} {} {})" data-kind="grade-label" data-grade="{id}">{}</text>"#,
            at.x,
            -at.y,
            at.x,
            -at.y,
            esc(&g.label())
        );
    }
    if d.end_tags {
        for (p, z) in [(g.a, g.elevation_a), (g.b, g.elevation_b)] {
            let at = p - dir * height * 0.5 - n * height * 1.2;
            let _ = writeln!(
                out,
                r#"<text x="{}" y="{}" font-size="{height}" font-family="sans-serif" fill="black" data-kind="grade-tag" data-grade="{id}">{}</text>"#,
                at.x,
                -at.y,
                meters(z, 2)
            );
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
        let svg = svg(ObjectId(7), &d, &g, 100.0);
        assert!(
            svg.contains("grade-arrow")
                && svg.contains("grade-label")
                && svg.matches("grade-tag").count() == 2
        );
        assert!(svg.contains("2.0% (1:50)") && !svg.contains(">1<") && !svg.contains(">1.2<"));
        assert!(svg.contains(">1.00<") && svg.contains(">1.20<"));
        // The arrow points downhill: for a rise from a to b it points to a (left).
        let tip: Vec<&str> = svg
            .lines()
            .next()
            .unwrap()
            .split('"')
            .nth(1)
            .unwrap()
            .split(' ')
            .collect();
        assert!(tip[0].split(',').next().unwrap().parse::<f64>().unwrap() < 5000.0);
        let down = GradeData {
            elevation_b: Some(800.0),
            ..d.clone()
        };
        let g2 = solve(a(), b(10_000.0), &down, None).unwrap();
        let s2 = super::svg(ObjectId(7), &down, &g2, 100.0);
        let tip2: f64 = s2
            .lines()
            .next()
            .unwrap()
            .split('"')
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .split(',')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(tip2 > 5000.0);
        // Level ground has a bar. Rotation keeps the text upright for lines that run to the left.
        let level = GradeData {
            elevation_b: Some(1000.0),
            label: true,
            end_tags: false,
            ..d.clone()
        };
        let g3 = solve(b(10_000.0), a(), &level, None).unwrap();
        let s3 = super::svg(ObjectId(7), &level, &g3, 100.0);
        assert!(s3.contains("<line") && !s3.contains("grade-tag") && s3.contains("rotate(0"));
        let s4 = super::svg(
            ObjectId(7),
            &GradeData {
                label: false,
                end_tags: false,
                ..d.clone()
            },
            &g,
            100.0,
        );
        assert!(s4.is_empty());
        // Steep lines in both directions rotate by less than 90 degrees.
        for (p, q) in [
            (DVec2::new(0.0, 0.0), DVec2::new(1.0, 5.0)),
            (DVec2::new(0.0, 0.0), DVec2::new(-1.0, 5.0)),
            (DVec2::new(0.0, 5.0), DVec2::new(1.0, 0.0)),
            (DVec2::new(0.0, 5.0), DVec2::new(-1.0, 0.0)),
        ] {
            let gg = solve(p, q, &d, None).unwrap();
            let s = super::svg(ObjectId(1), &d, &gg, 100.0);
            let r: f64 = s
                .split("rotate(")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert!(r.abs() <= 90.0);
        }
    }
}
