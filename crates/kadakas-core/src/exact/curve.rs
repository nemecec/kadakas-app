//! Curves of a STEP file in space, and their cut into points.

use std::f64::consts::TAU;
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use super::Similarity;
use super::nurbs::BSplineCurve;

/// The most points that one curve is cut into.
pub const MAX_CURVE_POINTS: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Curve {
    /// `o + t * d`. The direction `d` has the length of the vector of the file.
    Line {
        o: DVec3,
        d: DVec3,
    },
    /// The parameter is the angle in the plane of the frame.
    Circle {
        f: DAffine3,
        r: f64,
    },
    Ellipse {
        f: DAffine3,
        a: f64,
        b: f64,
    },
    Spline(BSplineCurve),
    /// The parameter is the index along the points.
    Poly {
        points: Vec<DVec3>,
    },
    /// Part of another curve, from parameter `t0` to parameter `t1` of it (`t1` may be the
    /// smaller: then the part runs against the direction of the curve).
    Trimmed {
        basis: Arc<Curve>,
        t0: f64,
        t1: f64,
    },
    /// Segments in order, each with its sense.
    Composite {
        segments: Vec<(Arc<Curve>, bool)>,
    },
}

/// The angle step of a circle of radius `r` for a chord error `tol` and a largest angle `ang`.
pub fn arc_step(r: f64, tol: f64, ang: f64) -> f64 {
    let by_chord = if r > tol {
        2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos()
    } else {
        ang
    };
    by_chord.min(ang).max(TAU / 4096.0)
}

impl Curve {
    /// Checks a curve from a file: finite numbers, valid splines, nesting not too deep.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_at(0)
    }

    fn validate_at(&self, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err("curves are nested too deep".into());
        }
        let finite = |v: DVec3| v.is_finite() && v.abs().max_element() < 1e12;
        let frame_ok = |f: &DAffine3| f.matrix3.is_finite() && f.translation.is_finite();
        let size_ok = |x: f64| x.is_finite() && x > 0.0 && x < 1e12;
        let ok = match self {
            Curve::Line { o, d } => finite(*o) && finite(*d) && d.length() > 0.0,
            Curve::Circle { f, r } => frame_ok(f) && size_ok(*r),
            Curve::Ellipse { f, a, b } => frame_ok(f) && size_ok(*a) && size_ok(*b),
            Curve::Spline(s) => return s.check(),
            Curve::Poly { points } => {
                points.len() >= 2 && points.len() <= 1_000_000 && points.iter().all(|p| finite(*p))
            }
            Curve::Trimmed { basis, t0, t1 } => {
                if !(t0.is_finite() && t1.is_finite()) {
                    return Err("a trimmed curve has a trim that is not finite".into());
                }
                return basis.validate_at(depth + 1);
            }
            Curve::Composite { segments } => {
                if segments.is_empty() || segments.len() > 100_000 {
                    return Err("a composite curve has no segments or too many".into());
                }
                for (c, _) in segments {
                    c.validate_at(depth + 1)?;
                }
                true
            }
        };
        if ok {
            Ok(())
        } else {
            Err("a curve has a number that is not usable".into())
        }
    }

    /// The curve moved by a similarity.
    pub fn transformed(&self, m: &Similarity) -> Curve {
        match self {
            Curve::Line { o, d } => Curve::Line {
                o: m.point(*o),
                d: m.vector(*d),
            },
            Curve::Circle { f, r } => Curve::Circle {
                f: m.frame(f),
                r: r * m.scale,
            },
            Curve::Ellipse { f, a, b } => Curve::Ellipse {
                f: m.frame(f),
                a: a * m.scale,
                b: b * m.scale,
            },
            Curve::Spline(s) => {
                let mut s = s.clone();
                s.points = s.points.iter().map(|p| m.point(*p)).collect();
                Curve::Spline(s)
            }
            Curve::Poly { points } => Curve::Poly {
                points: points.iter().map(|p| m.point(*p)).collect(),
            },
            Curve::Trimmed { basis, t0, t1 } => Curve::Trimmed {
                basis: Arc::new(basis.transformed(m)),
                t0: *t0,
                t1: *t1,
            },
            Curve::Composite { segments } => Curve::Composite {
                segments: segments
                    .iter()
                    .map(|(c, s)| (Arc::new(c.transformed(m)), *s))
                    .collect(),
            },
        }
    }

    pub fn period(&self) -> Option<f64> {
        match self {
            Curve::Circle { .. } | Curve::Ellipse { .. } => Some(TAU),
            _ => None,
        }
    }

    pub fn eval(&self, t: f64) -> DVec3 {
        match self {
            Curve::Line { o, d } => *o + *d * t,
            Curve::Circle { f, r } => f.transform_point3(DVec3::new(r * t.cos(), r * t.sin(), 0.0)),
            Curve::Ellipse { f, a, b } => {
                f.transform_point3(DVec3::new(a * t.cos(), b * t.sin(), 0.0))
            }
            Curve::Spline(s) => s.eval(t),
            Curve::Poly { points: p } => {
                let n = p.len();
                if n == 0 {
                    return DVec3::ZERO;
                }
                let t = t.clamp(0.0, (n - 1) as f64);
                let i = (t.floor() as usize).min(n.saturating_sub(2));
                if n == 1 {
                    return p[0];
                }
                p[i].lerp(p[i + 1], t - i as f64)
            }
            Curve::Trimmed { basis, .. } => basis.eval(t),
            Curve::Composite { segments: s } => s.first().map_or(DVec3::ZERO, |(c, _)| c.eval(t)),
        }
    }

    /// The parameter range that the curve has on its own (`None` for a line or a composite).
    pub fn natural_range(&self) -> Option<(f64, f64)> {
        match self {
            Curve::Circle { .. } | Curve::Ellipse { .. } => Some((0.0, TAU)),
            Curve::Spline(s) => Some(s.domain()),
            Curve::Poly { points: p } => Some((0.0, (p.len().max(1) - 1) as f64)),
            Curve::Trimmed { t0, t1, .. } => Some((*t0, *t1)),
            _ => None,
        }
    }

    /// The parameter of the point of the curve that is nearest to `p`. A circle gives an angle
    /// in (-pi, pi]. A spline or a polyline is searched by sampling and refined.
    pub fn param_of(&self, p: DVec3) -> f64 {
        match self {
            Curve::Line { o, d } => (p - *o).dot(*d) / d.length_squared().max(1e-300),
            Curve::Circle { f, .. } => {
                let q = f.inverse().transform_point3(p);
                q.y.atan2(q.x)
            }
            Curve::Ellipse { f, a, b } => {
                let q = f.inverse().transform_point3(p);
                (q.y / b.max(1e-300)).atan2(q.x / a.max(1e-300))
            }
            Curve::Trimmed { basis, .. } => basis.param_of(p),
            Curve::Composite { .. } => 0.0,
            Curve::Spline(_) | Curve::Poly { .. } => {
                let Some((a, b)) = self.natural_range() else {
                    return 0.0;
                };
                self.closest(p, a, b)
            }
        }
    }

    /// The points of the whole curve in its own direction: the whole range of a curve that has
    /// one, all segments of a composite. `None` for a line, which has no ends of its own.
    pub fn sample_all(&self, tol: f64, ang: f64) -> Option<Vec<DVec3>> {
        if let Curve::Composite { segments } = self {
            let mut out: Vec<DVec3> = Vec::new();
            for (c, same) in segments {
                let mut pts = c.sample_all(tol, ang)?;
                if !same {
                    pts.reverse();
                }
                if let (Some(last), Some(first)) = (out.last(), pts.first())
                    && last.distance(*first) <= tol * 1e-3 + 1e-9
                {
                    pts.remove(0);
                }
                out.extend(pts);
                if out.len() > MAX_CURVE_POINTS {
                    return None;
                }
            }
            return (out.len() >= 2).then_some(out);
        }
        let (a, b) = self.natural_range()?;
        Some(self.sample(a, b, tol, ang))
    }

    /// The parameter in [a, b] with the nearest point to `p`.
    pub fn closest(&self, p: DVec3, a: f64, b: f64) -> f64 {
        self.closest_by(a, b, &|c| c.distance_squared(p))
    }

    /// The parameter in [a, b] where `cost` of the point is the smallest. The cost should be
    /// smooth: the search samples the range, then narrows down around the best sample.
    pub fn closest_by(&self, a: f64, b: f64, cost: &dyn Fn(DVec3) -> f64) -> f64 {
        let n = match self {
            Curve::Spline(s) => (s.points.len() * s.knots.degree * 2).clamp(32, 4096),
            Curve::Poly { points: q } => q.len().clamp(2, 100_000) * 4,
            _ => 256,
        };
        let mut best = (f64::INFINITY, a);
        for k in 0..=n {
            let t = a + (b - a) * k as f64 / n as f64;
            let d = cost(self.eval(t));
            if d < best.0 {
                best = (d, t);
            }
        }
        // Golden-section refinement around the best sample.
        let h = (b - a) / n as f64;
        let (mut lo, mut hi) = ((best.1 - h).max(a), (best.1 + h).min(b));
        let g = 0.618_033_988_749_895;
        for _ in 0..40 {
            let m1 = hi - (hi - lo) * g;
            let m2 = lo + (hi - lo) * g;
            if cost(self.eval(m1)) < cost(self.eval(m2)) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let t = (lo + hi) / 2.0;
        if cost(self.eval(t)) <= best.0 {
            t
        } else {
            best.1
        }
    }

    /// Points along the curve from parameter `ts` to `te` (either may be the larger), including
    /// both ends. The chord error is at most `tol` and the turn of the direction between two
    /// points at most `ang` radians, as far as `MAX_CURVE_POINTS` allows.
    pub fn sample(&self, ts: f64, te: f64, tol: f64, ang: f64) -> Vec<DVec3> {
        match self {
            Curve::Line { .. } => vec![self.eval(ts), self.eval(te)],
            Curve::Circle { r, .. } | Curve::Ellipse { a: r, .. } => {
                let big = match self {
                    Curve::Ellipse { a, b, .. } => a.max(*b),
                    _ => *r,
                };
                let step = arc_step(big, tol, ang);
                let n = (((te - ts).abs() / step).ceil() as usize).clamp(1, MAX_CURVE_POINTS);
                (0..=n)
                    .map(|k| self.eval(ts + (te - ts) * k as f64 / n as f64))
                    .collect()
            }
            Curve::Poly { points: p } => {
                // The vertices between the two parameters.
                let (lo, hi) = (ts.min(te), ts.max(te));
                let mut out = vec![self.eval(lo)];
                let first = lo.floor() as i64 + 1;
                let last = hi.ceil() as i64 - 1;
                for i in first..=last {
                    if i >= 0 && (i as usize) < p.len() && (i as f64) > lo && (i as f64) < hi {
                        out.push(p[i as usize]);
                    }
                }
                out.push(self.eval(hi));
                if ts > te {
                    out.reverse();
                }
                out
            }
            Curve::Spline(s) => {
                let (lo, hi) = (ts.min(te), ts.max(te));
                let base = (s.points.len().max(s.knots.degree * 2)).clamp(4, 512);
                let mut ts_list: Vec<f64> = (0..=base)
                    .map(|k| lo + (hi - lo) * k as f64 / base as f64)
                    .collect();
                // Cut at the distinct knots too.
                for k in &s.knots.values {
                    if *k > lo && *k < hi {
                        ts_list.push(*k);
                    }
                }
                ts_list.sort_by(f64::total_cmp);
                ts_list.dedup_by(|a, b| (*a - *b).abs() < (hi - lo) * 1e-12);
                let mut out = Vec::new();
                let pts: Vec<DVec3> = ts_list.iter().map(|t| self.eval(*t)).collect();
                out.push(pts[0]);
                for w in 0..ts_list.len() - 1 {
                    self.refine(
                        ts_list[w],
                        ts_list[w + 1],
                        pts[w],
                        pts[w + 1],
                        tol,
                        ang,
                        0,
                        &mut out,
                    );
                    out.push(pts[w + 1]);
                    if out.len() > MAX_CURVE_POINTS {
                        break;
                    }
                }
                if ts > te {
                    out.reverse();
                }
                out
            }
            Curve::Trimmed { basis, .. } => basis.sample(ts, te, tol, ang),
            Curve::Composite { .. } => Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn refine(
        &self,
        ta: f64,
        tb: f64,
        pa: DVec3,
        pb: DVec3,
        tol: f64,
        ang: f64,
        depth: u32,
        out: &mut Vec<DVec3>,
    ) {
        if depth >= 12 || out.len() > MAX_CURVE_POINTS {
            return;
        }
        let tm = (ta + tb) / 2.0;
        let pm = self.eval(tm);
        let chord = pb - pa;
        let sag = if chord.length_squared() > 0.0 {
            let t = ((pm - pa).dot(chord) / chord.length_squared()).clamp(0.0, 1.0);
            pm.distance(pa + chord * t)
        } else {
            pm.distance(pa)
        };
        // The turn between the halves.
        let (d1, d2) = (pm - pa, pb - pm);
        let turn = if d1.length() > 0.0 && d2.length() > 0.0 {
            d1.angle_between(d2)
        } else {
            0.0
        };
        if sag > tol || turn > ang {
            self.refine(ta, tm, pa, pm, tol, ang, depth + 1, out);
            out.push(pm);
            self.refine(tm, tb, pm, pb, tol, ang, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exact::nurbs::Knots;

    #[test]
    fn circle_samples_stay_within_tolerance() {
        let c = Curve::Circle {
            f: DAffine3::IDENTITY,
            r: 50.0,
        };
        let pts = c.sample(0.0, TAU, 0.05, 1.0);
        assert!(pts.len() > 20);
        assert!((pts[0] - pts[pts.len() - 1]).length() < 1e-9);
        for w in pts.windows(2) {
            let mid = (w[0] + w[1]) / 2.0;
            assert!(50.0 - mid.length() <= 0.05 + 1e-9);
        }
        assert!((c.param_of(DVec3::new(0.0, 50.0, 0.0)) - TAU / 4.0).abs() < 1e-12);
        let e = Curve::Ellipse {
            f: DAffine3::IDENTITY,
            a: 4.0,
            b: 2.0,
        };
        assert!((e.param_of(DVec3::new(0.0, 2.0, 0.0)) - TAU / 4.0).abs() < 1e-12);
        assert!(e.sample(0.0, TAU, 0.01, 0.5).len() > 10);
        assert_eq!(e.period(), Some(TAU));
        assert_eq!(arc_step(0.001, 1.0, 0.3), 0.3);
    }

    #[test]
    fn line_poly_and_spline() {
        let l = Curve::Line {
            o: DVec3::ZERO,
            d: DVec3::X,
        };
        assert_eq!(l.param_of(DVec3::new(3.0, 1.0, 0.0)), 3.0);
        assert_eq!(l.sample(0.0, 5.0, 0.1, 0.1).len(), 2);
        assert!(l.natural_range().is_none() && l.period().is_none());
        let p = Curve::Poly {
            points: vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0)],
        };
        assert_eq!(p.natural_range(), Some((0.0, 2.0)));
        assert_eq!(p.sample(0.0, 2.0, 0.1, 0.1).len(), 3);
        assert_eq!(p.sample(2.0, 0.0, 0.1, 0.1)[0], DVec3::new(1.0, 1.0, 0.0));
        assert!((p.param_of(DVec3::new(1.0, 0.5, 0.0)) - 1.5).abs() < 1e-6);
        let s = Curve::Spline(BSplineCurve {
            knots: Knots::clamped(3, 6).unwrap(),
            points: (0..6)
                .map(|i| DVec3::new(i as f64 * 10.0, (i as f64).sin() * 10.0, 0.0))
                .collect(),
            weights: None,
        });
        let (a, b) = s.natural_range().unwrap();
        let pts = s.sample(a, b, 0.05, 0.3);
        assert!(pts.len() > 8);
        let back = s.sample(b, a, 0.05, 0.3);
        assert_eq!(back[0], pts[pts.len() - 1]);
        let t = s.param_of(s.eval(a + (b - a) * 0.37));
        assert!((t - (a + (b - a) * 0.37)).abs() < 1e-4);
        let tr = Curve::Trimmed {
            basis: Arc::new(Curve::Circle {
                f: DAffine3::IDENTITY,
                r: 1.0,
            }),
            t0: 0.0,
            t1: 1.0,
        };
        assert_eq!(tr.natural_range(), Some((0.0, 1.0)));
        assert!(tr.sample(0.0, 1.0, 0.01, 0.3).len() > 2);
        assert!((tr.param_of(DVec3::Y) - TAU / 4.0).abs() < 1e-9);
        assert!((tr.eval(0.0) - DVec3::X).length() < 1e-12);
        assert_eq!(
            tr.sample_all(0.01, 0.3).unwrap().len(),
            tr.sample(0.0, 1.0, 0.01, 0.3).len()
        );
        assert!(l.sample_all(0.1, 0.1).is_none());
        let comp = Curve::Composite {
            segments: vec![
                (
                    Arc::new(Curve::Poly {
                        points: vec![DVec3::ZERO, DVec3::X],
                    }),
                    true,
                ),
                (
                    Arc::new(Curve::Poly {
                        points: vec![DVec3::new(1.0, 1.0, 0.0), DVec3::X],
                    }),
                    false,
                ),
            ],
        };
        let all = comp.sample_all(0.1, 0.1).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[2], DVec3::new(1.0, 1.0, 0.0));
        let bad = Curve::Composite {
            segments: vec![(Arc::new(l.clone()), true)],
        };
        assert!(bad.sample_all(0.1, 0.1).is_none());
        let c = Curve::Composite { segments: vec![] };
        assert!(c.sample_all(0.1, 0.1).is_none());
        assert!(c.sample(0.0, 1.0, 0.1, 0.1).is_empty());
        assert_eq!(c.eval(0.0), DVec3::ZERO);
        assert_eq!(c.param_of(DVec3::X), 0.0);
        assert!(c.natural_range().is_none());
    }
}
