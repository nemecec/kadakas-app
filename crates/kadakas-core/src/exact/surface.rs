//! Surfaces of a STEP file in space: evaluation, partial derivatives and the parameters of a
//! point.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};

use serde_json::{Value, json};

use super::Similarity;
use super::curve::Curve;
use super::nurbs::BSplineSurface;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Surface {
    Plane {
        f: DAffine3,
    },
    Cylinder {
        f: DAffine3,
        r: f64,
    },
    /// `P(u, v) = O + (R + v sin a)(cos u X + sin u Y) + v cos a Z`.
    Cone {
        f: DAffine3,
        r: f64,
        angle: f64,
    },
    Sphere {
        f: DAffine3,
        r: f64,
    },
    Torus {
        f: DAffine3,
        big: f64,
        small: f64,
    },
    /// The curve turned around the axis: `u` is the angle, `v` the parameter of the curve.
    Revolution {
        curve: Arc<Curve>,
        o: DVec3,
        axis: DVec3,
    },
    /// The curve moved along the direction: `v` is the distance.
    Extrusion {
        curve: Arc<Curve>,
        dir: DVec3,
    },
    Spline(BSplineSurface),
}

fn rotate_about(axis: DVec3, angle: f64, v: DVec3) -> DVec3 {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s + axis * axis.dot(v) * (1.0 - c)
}

impl Surface {
    /// Checks a surface from a file: finite numbers and valid splines.
    pub fn validate(&self) -> Result<(), String> {
        let frame_ok = |f: &DAffine3| f.matrix3.is_finite() && f.translation.is_finite();
        let size_ok = |x: f64| x.is_finite() && x > 0.0 && x < 1e12;
        let ok = match self {
            Surface::Plane { f } => frame_ok(f),
            Surface::Cylinder { f, r } | Surface::Sphere { f, r } => frame_ok(f) && size_ok(*r),
            Surface::Cone { f, r, angle } => {
                frame_ok(f)
                    && r.is_finite()
                    && *r >= 0.0
                    && *r < 1e12
                    && angle.is_finite()
                    && angle.abs() > 1e-9
                    && angle.abs() < TAU / 4.0 - 1e-6
            }
            Surface::Torus { f, big, small } => {
                frame_ok(f) && size_ok(*big) && size_ok(*small) && small < big
            }
            Surface::Revolution { curve, o, axis } => {
                curve.validate()?;
                o.is_finite() && axis.is_finite() && axis.length() > 0.5
            }
            Surface::Extrusion { curve, dir } => {
                curve.validate()?;
                curve.natural_range().is_some() && dir.is_finite() && dir.length() > 0.5
            }
            Surface::Spline(s) => return s.check(),
        };
        if ok {
            Ok(())
        } else {
            Err("a surface has a number that is not usable".into())
        }
    }

    /// The surface moved by a similarity. For a map that mirrors, the faces must turn their
    /// `same_sense` (the payload does that), because the cross product of the derivatives
    /// changes sign.
    pub fn transformed(&self, m: &Similarity) -> Surface {
        match self {
            Surface::Plane { f } => Surface::Plane { f: m.frame(f) },
            Surface::Cylinder { f, r } => Surface::Cylinder {
                f: m.frame(f),
                r: r * m.scale,
            },
            Surface::Cone { f, r, angle } => Surface::Cone {
                f: m.frame(f),
                r: r * m.scale,
                angle: *angle,
            },
            Surface::Sphere { f, r } => Surface::Sphere {
                f: m.frame(f),
                r: r * m.scale,
            },
            Surface::Torus { f, big, small } => Surface::Torus {
                f: m.frame(f),
                big: big * m.scale,
                small: small * m.scale,
            },
            Surface::Revolution { curve, o, axis } => {
                // A mirror turns the sense of the turn, so the axis turns around.
                let a = m.vector(*axis) / m.scale;
                Surface::Revolution {
                    curve: Arc::new(curve.transformed(m)),
                    o: m.point(*o),
                    axis: if m.mirror { -a } else { a },
                }
            }
            Surface::Extrusion { curve, dir } => Surface::Extrusion {
                curve: Arc::new(curve.transformed(m)),
                dir: m.vector(*dir) / m.scale,
            },
            Surface::Spline(s) => {
                let mut s = s.clone();
                s.points = s.points.iter().map(|p| m.point(*p)).collect();
                Surface::Spline(s)
            }
        }
    }

    /// False for a surface whose frame is left-handed (a mirror made it so). Then the normal
    /// `dP/du x dP/dv` points to the other side than for a right-handed frame. A plane does not
    /// depend on its third axis.
    pub fn right_handed(&self) -> bool {
        match self {
            Surface::Cylinder { f, .. }
            | Surface::Cone { f, .. }
            | Surface::Sphere { f, .. }
            | Surface::Torus { f, .. } => f.matrix3.determinant() > 0.0,
            _ => true,
        }
    }

    /// The same surface with a right-handed frame, if the frame is left-handed (the angle `u`
    /// runs the other way). The normal `dP/du x dP/dv` of the new surface is the opposite of
    /// the old one at every point, so the one who cuts the faces turns `same_sense` too.
    pub fn right_handed_form(&self) -> Option<Surface> {
        if self.right_handed() {
            return None;
        }
        let turn = |f: &DAffine3| DAffine3 {
            matrix3: glam::DMat3::from_cols(f.matrix3.x_axis, -f.matrix3.y_axis, f.matrix3.z_axis),
            translation: f.translation,
        };
        Some(match self {
            Surface::Cylinder { f, r } => Surface::Cylinder { f: turn(f), r: *r },
            Surface::Cone { f, r, angle } => Surface::Cone {
                f: turn(f),
                r: *r,
                angle: *angle,
            },
            Surface::Sphere { f, r } => Surface::Sphere { f: turn(f), r: *r },
            Surface::Torus { f, big, small } => Surface::Torus {
                f: turn(f),
                big: *big,
                small: *small,
            },
            other => other.clone(),
        })
    }

    /// Does the normal `dP/du x dP/dv` point away from the inside of the surface (away from the
    /// axis of a cylinder or a revolution, from the centre of a sphere, from the centre of
    /// curvature of a patch)? A face whose normal points the other way is a bore or a hollow.
    /// `None` when the surface has no inside to tell by: a plane, a patch or a revolution that
    /// is flat to the precision of its numbers (a mirror must not make it hollow by rounding).
    pub fn natural_outward(&self) -> Option<bool> {
        // The side of `radial` that the normal is on at some sample, the first one that tells.
        let side = |samples: &mut dyn Iterator<Item = (f64, f64, DVec3)>| -> Option<bool> {
            for (u, v, radial) in samples {
                let (pu, pv) = self.partials(u, v);
                let n = pu.cross(pv);
                let d = n.dot(radial);
                if n.is_finite()
                    && radial.is_finite()
                    && d.abs() > 1e-6 * n.length() * radial.length()
                {
                    return Some(d > 0.0);
                }
            }
            None
        };
        match self {
            Surface::Plane { .. } => None,
            Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. } => Some(self.right_handed()),
            Surface::Revolution { curve, o, axis } => {
                // A line has no natural range: its parameter starts at its point.
                let (a, b) = curve.natural_range().unwrap_or((0.0, 1.0));
                let ax = axis.normalize_or_zero();
                side(&mut [0.5, 0.3, 0.7, 0.1, 0.9].into_iter().map(|k| {
                    let v = a + (b - a) * k;
                    let rel = self.eval(0.0, v) - *o;
                    (0.0, v, rel - ax * rel.dot(ax))
                }))
            }
            Surface::Extrusion { curve, dir } => {
                let (a, b) = curve.natural_range().unwrap_or((0.0, 1.0));
                let d = dir.normalize_or_zero();
                let centre = (0..16)
                    .map(|k| curve.eval(a + (b - a) * (k as f64 + 0.5) / 16.0))
                    .sum::<DVec3>()
                    / 16.0;
                side(&mut [0.5, 0.3, 0.7, 0.1, 0.9].into_iter().map(|k| {
                    let u = a + (b - a) * k;
                    let rel = self.eval(u, 0.0) - centre;
                    (u, 0.0, rel - d * rel.dot(d))
                }))
            }
            Surface::Spline(s) => {
                let ((u0, u1), (v0, v1)) = s.domain();
                let (u, v) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
                let (hu, hv) = ((u1 - u0) * 0.02, (v1 - v0) * 0.02);
                let p = |du: f64, dv: f64| self.eval(u + du, v + dv);
                let p0 = p(0.0, 0.0);
                let pu = (p(hu, 0.0) - p(-hu, 0.0)) / (2.0 * hu);
                let pv = (p(0.0, hv) - p(0.0, -hv)) / (2.0 * hv);
                let puu = (p(hu, 0.0) - 2.0 * p0 + p(-hu, 0.0)) / (hu * hu);
                let pvv = (p(0.0, hv) - 2.0 * p0 + p(0.0, -hv)) / (hv * hv);
                let puv = (p(hu, hv) - p(hu, -hv) - p(-hu, hv) + p(-hu, -hv)) / (4.0 * hu * hv);
                let (e, f, g) = (pu.dot(pu), pu.dot(pv), pv.dot(pv));
                let det = e * g - f * f;
                if det <= 0.0 {
                    return None;
                }
                // The mean curvature vector points to the centre of curvature.
                let h = (g * puu - 2.0 * f * puv + e * pvv) / (2.0 * det);
                let n = pu.cross(pv);
                if !(n.is_finite() && h.is_finite()) {
                    return None;
                }
                // Flat: the curvature times the size of the patch is under a ten-thousandth.
                // The second differences of numbers that are far from zero carry rounding.
                let size = (p(u1 - u, v1 - v) - p(u0 - u, v0 - v)).length();
                if h.length() * size < 1e-4 {
                    return None;
                }
                let d = n.dot(h);
                if d.abs() <= 1e-6 * n.length() * h.length() {
                    return None;
                }
                Some(d <= 0.0)
            }
        }
    }

    /// About how many evaluations one call of `uv_of` takes, for the work budget of an import.
    pub fn uv_cost(&self) -> u64 {
        match self {
            Surface::Revolution { curve, .. } | Surface::Extrusion { curve, .. } => {
                // A search along the curve: samples and a golden section.
                let samples = match curve.as_ref() {
                    Curve::Spline(s) => {
                        (s.points.len() * s.knots.degree * 2).clamp(32, 4096) as u64
                    }
                    Curve::Poly { points } => (points.len().clamp(2, 100_000) * 4) as u64,
                    _ => 256,
                };
                samples + 90
            }
            Surface::Spline(s) => {
                let n = ((s.u.count * 2).clamp(8, 40) * (s.v.count * 2).clamp(8, 40)) as u64;
                n + 40 * 4
            }
            _ => 2,
        }
    }

    /// About how many steps of work one call of `eval` takes, for the work budget of an import
    /// (one step is about 50 ns on a current computer).
    pub fn eval_cost(&self) -> u64 {
        match self {
            Surface::Plane { .. } => 1,
            Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Sphere { .. } => 2,
            Surface::Torus { .. } => 3,
            Surface::Revolution { curve, .. } | Surface::Extrusion { curve, .. } => {
                match curve.as_ref() {
                    Curve::Spline(s) => (s.knots.degree as u64 + 1) * 2 + 10,
                    Curve::Poly { points } => 4 + (points.len().max(2).ilog2() as u64),
                    _ => 5,
                }
            }
            Surface::Spline(s) => {
                let (p, q) = (s.u.degree as u64, s.v.degree as u64);
                (p + 1) * (q + 1) / 2 + 2 * (p + q) + 10
            }
        }
    }

    /// A short name: `plane`, `cylinder`, `cone`, `sphere`, `torus`, `revolution`,
    /// `extrusion`, `spline`.
    pub fn short_kind(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "plane",
            Surface::Cylinder { .. } => "cylinder",
            Surface::Cone { .. } => "cone",
            Surface::Sphere { .. } => "sphere",
            Surface::Torus { .. } => "torus",
            Surface::Revolution { .. } => "revolution",
            Surface::Extrusion { .. } => "extrusion",
            Surface::Spline(_) => "spline",
        }
    }

    /// The true sizes of the surface in the units of the payload, as JSON.
    pub fn describe(&self) -> Value {
        let frame = |f: &DAffine3| {
            json!({
                "origin": f.translation.to_array(),
                "axis": f.matrix3.z_axis.to_array(),
                "x_axis": f.matrix3.x_axis.to_array(),
            })
        };
        let mut v = match self {
            // The normal of the surface is x cross y: for a frame that a mirror made left-handed
            // it is the opposite of the third axis.
            Surface::Plane { f } => json!({
                "point": f.translation.to_array(),
                "normal": f.matrix3.x_axis.cross(f.matrix3.y_axis).normalize_or_zero().to_array(),
            }),
            Surface::Cylinder { f, r } => {
                let mut o = frame(f);
                o["radius"] = json!(r);
                o
            }
            Surface::Cone { f, r, angle } => {
                let mut o = frame(f);
                o["radius"] = json!(r);
                o["half_angle_deg"] = json!(angle.to_degrees());
                o
            }
            Surface::Sphere { f, r } => json!({"center": f.translation.to_array(), "radius": r}),
            Surface::Torus { f, big, small } => {
                let mut o = frame(f);
                o["major_radius"] = json!(big);
                o["minor_radius"] = json!(small);
                o
            }
            Surface::Revolution { o, axis, .. } => {
                json!({"origin": o.to_array(), "axis": axis.to_array()})
            }
            Surface::Extrusion { dir, .. } => json!({"direction": dir.to_array()}),
            Surface::Spline(s) => json!({
                "degree_u": s.u.degree,
                "degree_v": s.v.degree,
                "control_points": [s.u.count, s.v.count],
                "rational": s.weights.is_some(),
            }),
        };
        v["kind"] = json!(self.short_kind());
        v
    }

    /// The name of the kind, for the report.
    pub fn kind(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "PLANE",
            Surface::Cylinder { .. } => "CYLINDRICAL_SURFACE",
            Surface::Cone { .. } => "CONICAL_SURFACE",
            Surface::Sphere { .. } => "SPHERICAL_SURFACE",
            Surface::Torus { .. } => "TOROIDAL_SURFACE",
            Surface::Revolution { .. } => "SURFACE_OF_REVOLUTION",
            Surface::Extrusion { .. } => "SURFACE_OF_LINEAR_EXTRUSION",
            Surface::Spline(_) => "B_SPLINE_SURFACE",
        }
    }

    pub fn is_flat(&self) -> bool {
        matches!(self, Surface::Plane { .. })
    }

    pub fn period_u(&self) -> Option<f64> {
        match self {
            Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
            | Surface::Revolution { .. } => Some(TAU),
            Surface::Extrusion { curve, .. } => curve.period(),
            _ => None,
        }
    }

    pub fn period_v(&self) -> Option<f64> {
        match self {
            Surface::Torus { .. } => Some(TAU),
            Surface::Revolution { curve, .. } => curve.period(),
            _ => None,
        }
    }

    pub fn eval(&self, u: f64, v: f64) -> DVec3 {
        match self {
            Surface::Plane { f } => f.transform_point3(DVec3::new(u, v, 0.0)),
            Surface::Cylinder { f, r } => {
                f.transform_point3(DVec3::new(r * u.cos(), r * u.sin(), v))
            }
            Surface::Cone { f, r, angle } => {
                let rad = r + v * angle.sin();
                f.transform_point3(DVec3::new(rad * u.cos(), rad * u.sin(), v * angle.cos()))
            }
            Surface::Sphere { f, r } => f.transform_point3(DVec3::new(
                r * v.cos() * u.cos(),
                r * v.cos() * u.sin(),
                r * v.sin(),
            )),
            Surface::Torus { f, big, small } => {
                let rad = big + small * v.cos();
                f.transform_point3(DVec3::new(rad * u.cos(), rad * u.sin(), small * v.sin()))
            }
            Surface::Revolution { curve, o, axis } => {
                let rel = curve.eval(v) - *o;
                let along = *axis * rel.dot(*axis);
                *o + along + rotate_about(*axis, u, rel - along)
            }
            Surface::Extrusion { curve, dir } => curve.eval(u) + *dir * v,
            Surface::Spline(s) => s.eval(u, v),
        }
    }

    /// The surface derivatives `(dP/du, dP/dv)`.
    pub fn partials(&self, u: f64, v: f64) -> (DVec3, DVec3) {
        match self {
            Surface::Plane { f } => (f.matrix3.x_axis, f.matrix3.y_axis),
            Surface::Cylinder { f, r } => (
                f.transform_vector3(DVec3::new(-r * u.sin(), r * u.cos(), 0.0)),
                f.matrix3.z_axis,
            ),
            Surface::Cone { f, r, angle } => {
                let rad = r + v * angle.sin();
                (
                    f.transform_vector3(DVec3::new(-rad * u.sin(), rad * u.cos(), 0.0)),
                    f.transform_vector3(DVec3::new(
                        angle.sin() * u.cos(),
                        angle.sin() * u.sin(),
                        angle.cos(),
                    )),
                )
            }
            Surface::Sphere { f, r } => (
                f.transform_vector3(DVec3::new(
                    -r * v.cos() * u.sin(),
                    r * v.cos() * u.cos(),
                    0.0,
                )),
                f.transform_vector3(DVec3::new(
                    -r * v.sin() * u.cos(),
                    -r * v.sin() * u.sin(),
                    r * v.cos(),
                )),
            ),
            Surface::Torus { f, big, small } => {
                let rad = big + small * v.cos();
                (
                    f.transform_vector3(DVec3::new(-rad * u.sin(), rad * u.cos(), 0.0)),
                    f.transform_vector3(DVec3::new(
                        -small * v.sin() * u.cos(),
                        -small * v.sin() * u.sin(),
                        small * v.cos(),
                    )),
                )
            }
            Surface::Extrusion { curve, dir } => {
                let h = 1e-6 * (1.0 + u.abs());
                ((curve.eval(u + h) - curve.eval(u - h)) / (2.0 * h), *dir)
            }
            Surface::Revolution { .. } | Surface::Spline(_) => {
                let hu = 1e-6 * (1.0 + u.abs());
                let hv = 1e-6 * (1.0 + v.abs());
                (
                    (self.eval(u + hu, v) - self.eval(u - hu, v)) / (2.0 * hu),
                    (self.eval(u, v + hv) - self.eval(u, v - hv)) / (2.0 * hv),
                )
            }
        }
    }

    /// The unit normal `dP/du x dP/dv`, or `None` where it is not defined (a pole).
    pub fn normal(&self, u: f64, v: f64) -> Option<DVec3> {
        let (a, b) = self.partials(u, v);
        let n = a.cross(b);
        let l = n.length();
        (l.is_finite() && l > 1e-9 * a.length().max(b.length()) && l > 0.0).then(|| n / l)
    }

    /// True when the point is on the axis of a sphere or at the apex of a cone, where the angle
    /// `u` has no value.
    pub fn at_pole(&self, p: DVec3) -> bool {
        match self {
            Surface::Sphere { f, r } => {
                let q = f.inverse().transform_point3(p);
                q.x.hypot(q.y) < 1e-9 * r
            }
            Surface::Cone { f, r, angle } => {
                let q = f.inverse().transform_point3(p);
                let rad = r + q.z / angle.cos() * angle.sin();
                q.x.hypot(q.y) < 1e-9 * (1.0 + r) && rad.abs() < 1e-9 * (1.0 + r)
            }
            _ => false,
        }
    }

    /// The `v` of the pole on the north side (`north`) or south side of a surface that turns
    /// to a point there: a sphere and a cone.
    pub fn pole_v(&self, north: bool) -> Option<f64> {
        match self {
            Surface::Sphere { .. } => Some(if north { FRAC_PI_2 } else { -FRAC_PI_2 }),
            Surface::Cone { r, angle, .. } => {
                let s = angle.sin();
                if s.abs() < 1e-12 {
                    return None;
                }
                let apex = -r / s;
                // Increasing v is north.
                let toward_north = apex > 0.0;
                (toward_north == north || r.abs() < 1e-12).then_some(apex)
            }
            _ => None,
        }
    }

    /// Where the point `p` of the surface lies in the parameters. `hint` is a nearby `(u, v)`.
    /// An angle comes back in (-pi, pi]. `None` when the search fails.
    pub fn uv_of(&self, p: DVec3, hint: Option<(f64, f64)>) -> Option<(f64, f64)> {
        let r = match self {
            Surface::Plane { f } => {
                let q = f.inverse().transform_point3(p);
                (q.x, q.y)
            }
            Surface::Cylinder { f, .. } => {
                let q = f.inverse().transform_point3(p);
                (q.y.atan2(q.x), q.z)
            }
            Surface::Cone { f, angle, .. } => {
                let q = f.inverse().transform_point3(p);
                (q.y.atan2(q.x), q.z / angle.cos())
            }
            Surface::Sphere { f, .. } => {
                let q = f.inverse().transform_point3(p);
                let rho = q.x.hypot(q.y);
                (q.y.atan2(q.x), q.z.atan2(rho))
            }
            Surface::Torus { f, big, .. } => {
                let q = f.inverse().transform_point3(p);
                let rho = q.x.hypot(q.y);
                (q.y.atan2(q.x), q.z.atan2(rho - big))
            }
            Surface::Revolution { curve, o, axis } => {
                let rel = p - *o;
                let h = rel.dot(*axis);
                let rho = (rel - *axis * h).length();
                let (a, b) = curve.natural_range().or_else(|| match curve.as_ref() {
                    Curve::Line { o, d } => {
                        // Around the nearest point of the line, wide enough for the point.
                        let t = curve.param_of(p);
                        let k = (2.0 * (p - *o).length() + 1.0) / d.length();
                        Some((t - k, t + k))
                    }
                    _ => None,
                })?;
                let cost = |c: DVec3| {
                    let r = c - *o;
                    let ch = r.dot(*axis);
                    let cr = (r - *axis * ch).length();
                    (ch - h) * (ch - h) + (cr - rho) * (cr - rho)
                };
                let v = curve.closest_by(a, b, &cost);
                let c = curve.eval(v) - *o;
                let c = c - *axis * c.dot(*axis);
                let q = rel - *axis * h;
                if c.length() < 1e-12 || q.length() < 1e-12 {
                    (hint.map_or(0.0, |h| h.0), v)
                } else {
                    let (c, q) = (c.normalize(), q.normalize());
                    let u = axis.dot(c.cross(q)).atan2(c.dot(q));
                    (u, v)
                }
            }
            Surface::Extrusion { curve, dir } => {
                let (a, b) = curve.natural_range()?;
                let cost = |c: DVec3| {
                    let d = p - c;
                    d.length_squared() - d.dot(*dir).powi(2)
                };
                let u = curve.closest_by(a, b, &cost);
                (u, (p - curve.eval(u)).dot(*dir))
            }
            Surface::Spline(s) => return spline_uv(s, self, p, hint),
        };
        (r.0.is_finite() && r.1.is_finite()).then_some(r)
    }
}

fn spline_uv(
    s: &BSplineSurface,
    surf: &Surface,
    p: DVec3,
    hint: Option<(f64, f64)>,
) -> Option<(f64, f64)> {
    let ((u0, u1), (v0, v1)) = s.domain();
    let scale = 1.0 + p.length();
    let newton = |mut u: f64, mut v: f64| -> Option<(f64, f64, f64)> {
        let mut err = f64::INFINITY;
        for _ in 0..40 {
            let r = surf.eval(u, v) - p;
            err = r.length();
            if err < 1e-9 * scale {
                break;
            }
            let (su, sv) = surf.partials(u, v);
            let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (g1, g2) = (su.dot(r), sv.dot(r));
            let det = a * c - b * b;
            let damp = 1e-9 * (a + c);
            let det = det + damp * (a + c) + damp * damp;
            if !det.is_finite() || det == 0.0 {
                break;
            }
            let du = -((c + damp) * g1 - b * g2) / det;
            let dv = -((a + damp) * g2 - b * g1) / det;
            let (nu, nv) = ((u + du).clamp(u0, u1), (v + dv).clamp(v0, v1));
            if (nu - u).abs() < 1e-14 && (nv - v).abs() < 1e-14 {
                break;
            }
            u = nu;
            v = nv;
        }
        err.is_finite().then_some((u, v, err))
    };
    if let Some((hu, hv)) = hint
        && let Some((u, v, e)) = newton(hu.clamp(u0, u1), hv.clamp(v0, v1))
        && e < 1e-6 * scale
    {
        return Some((u, v));
    }
    let nu = (s.u.count * 2).clamp(8, 40);
    let nv = (s.v.count * 2).clamp(8, 40);
    let mut best = (f64::INFINITY, u0, v0);
    for i in 0..=nu {
        for j in 0..=nv {
            let u = u0 + (u1 - u0) * i as f64 / nu as f64;
            let v = v0 + (v1 - v0) * j as f64 / nv as f64;
            let d = s.eval(u, v).distance_squared(p);
            if d < best.0 {
                best = (d, u, v);
            }
        }
    }
    newton(best.1, best.2).map(|(u, v, _)| (u, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exact::nurbs::Knots;

    fn mirror_x() -> crate::exact::Similarity {
        crate::exact::Similarity::of(&DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0))).unwrap()
    }

    /// A mirror turns the normal of a surface that keeps its parameters, and the answer of
    /// `natural_outward` follows it, so a face (which turns its sense too) keeps its answer.
    fn assert_follows_mirror(s: &Surface, outward: bool) {
        assert_eq!(s.natural_outward(), Some(outward), "{}", s.kind());
        let m = s.transformed(&mirror_x());
        assert_eq!(m.natural_outward(), Some(!outward), "{} mirrored", s.kind());
    }

    #[test]
    fn the_inside_of_every_kind_of_surface_is_found() {
        let f = DAffine3::from_translation(DVec3::new(1.0, 2.0, 3.0));
        // A cylinder, a cone, a sphere and a torus are outward until a mirror leaves a left frame.
        for s in [
            Surface::Cylinder { f, r: 4.0 },
            Surface::Cone {
                f,
                r: 4.0,
                angle: 0.3,
            },
            Surface::Sphere { f, r: 5.0 },
            Surface::Torus {
                f,
                big: 10.0,
                small: 2.0,
            },
        ] {
            assert_follows_mirror(&s, true);
            let left = s.transformed(&mirror_x());
            assert!(!left.right_handed());
            let right = left.right_handed_form().expect("a right-handed form");
            assert!(right.right_handed() && right.right_handed_form().is_none());
            // The same points, and the normal of the new surface is the opposite one.
            let (u, v) = (0.7, 0.4);
            let p = left.eval(u, v);
            let (u2, v2) = right.uv_of(p, None).unwrap();
            assert!(near(right.eval(u2, v2), p), "{}", s.kind());
            let (n1, n2) = (left.normal(u, v).unwrap(), right.normal(u2, v2).unwrap());
            assert!(near(n1, -n2), "{}", s.kind());
        }
        assert_eq!(Surface::Plane { f }.natural_outward(), None);
        // A revolution of a line that runs along the axis: the line one way or the other.
        let line = |d: DVec3| {
            Arc::new(Curve::Line {
                o: DVec3::new(5.0, 0.0, 0.0),
                d,
            })
        };
        let rev = |d: DVec3| Surface::Revolution {
            curve: line(d),
            o: DVec3::ZERO,
            axis: DVec3::Z,
        };
        assert_follows_mirror(&rev(DVec3::Z), true);
        assert_follows_mirror(&rev(-DVec3::Z), false);
        // An extrusion of a circle along its axis: up or down.
        let circle = Arc::new(Curve::Circle {
            f: DAffine3::IDENTITY,
            r: 5.0,
        });
        assert_follows_mirror(
            &Surface::Extrusion {
                curve: circle.clone(),
                dir: DVec3::Z,
            },
            true,
        );
        assert_follows_mirror(
            &Surface::Extrusion {
                curve: circle,
                dir: -DVec3::Z,
            },
            false,
        );
        // A dome of a patch, seen from above or from below, and flat.
        let patch = |z: f64, swap: bool| {
            let mut pts = Vec::new();
            for i in 0..3 {
                for j in 0..3 {
                    let h = if i == 1 && j == 1 { z } else { 0.0 };
                    let (a, b) = if swap { (j, i) } else { (i, j) };
                    pts.push(DVec3::new(a as f64, b as f64, h));
                }
            }
            let knots = Knots {
                degree: 2,
                values: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                count: 3,
            };
            Surface::Spline(BSplineSurface {
                u: knots.clone(),
                v: knots,
                points: pts,
                weights: None,
            })
        };
        let (up, down) = (patch(2.0, false), patch(-2.0, false));
        assert_ne!(up.natural_outward(), down.natural_outward());
        assert_ne!(up.natural_outward(), patch(2.0, true).natural_outward());
        assert_eq!(
            patch(0.0, false).natural_outward(),
            None,
            "a flat patch has no inside"
        );
        assert_follows_mirror(&up, up.natural_outward().unwrap());
        // A flat patch far from the origin and turned: rounding does not give it an inside.
        let far = |m: &crate::exact::Similarity| match patch(0.0, false) {
            Surface::Spline(mut s) => {
                for p in &mut s.points {
                    *p = m.point(*p + DVec3::new(1e7, 3.7e6, 250.0));
                }
                Surface::Spline(s)
            }
            _ => unreachable!(),
        };
        for mm in [
            crate::exact::Similarity::of(&DAffine3::from_rotation_z(0.5)).unwrap(),
            mirror_x(),
        ] {
            assert_eq!(far(&mm).natural_outward(), None);
        }
        // A flat revolution (a disc) the same.
        let disc = Surface::Revolution {
            curve: Arc::new(Curve::Line {
                o: DVec3::ZERO,
                d: DVec3::X,
            }),
            o: DVec3::ZERO,
            axis: DVec3::Z,
        };
        assert_eq!(disc.natural_outward(), None);
        assert_eq!(disc.transformed(&mirror_x()).natural_outward(), None);
    }

    fn near(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-6
    }

    #[test]
    fn analytic_round_trips() {
        let f = DAffine3::from_translation(DVec3::new(1.0, 2.0, 3.0));
        let surfaces = [
            Surface::Plane { f },
            Surface::Cylinder { f, r: 4.0 },
            Surface::Cone {
                f,
                r: 4.0,
                angle: 0.3,
            },
            Surface::Sphere { f, r: 5.0 },
            Surface::Torus {
                f,
                big: 10.0,
                small: 2.0,
            },
        ];
        for s in &surfaces {
            for (u, v) in [(0.4, 0.3), (2.0, -0.5), (-1.0, 0.9)] {
                let p = s.eval(u, v);
                let (uu, vv) = s.uv_of(p, None).unwrap();
                assert!(near(s.eval(uu, vv), p), "{}", s.kind());
                // The normal is a unit vector or undefined.
                if let Some(n) = s.normal(u, v) {
                    assert!((n.length() - 1.0).abs() < 1e-9);
                }
            }
        }
        assert!(Surface::Plane { f }.is_flat());
        assert!(
            Surface::Sphere { f, r: 1.0 }
                .normal(0.0, FRAC_PI_2)
                .is_none()
        );
        // Poles.
        assert_eq!(surfaces[3].pole_v(true), Some(FRAC_PI_2));
        assert_eq!(surfaces[3].pole_v(false), Some(-FRAC_PI_2));
        assert!(surfaces[2].pole_v(false).is_some());
        assert!(surfaces[1].pole_v(true).is_none());
        assert_eq!(surfaces[4].period_v(), Some(TAU));
        assert_eq!(surfaces[1].period_u(), Some(TAU));
        assert_eq!(surfaces[0].period_u(), None);
    }

    #[test]
    fn revolution_extrusion_and_spline() {
        // A line at x = 3 turned around Z is a cylinder of radius 3.
        let rev = Surface::Revolution {
            curve: Arc::new(Curve::Line {
                o: DVec3::new(3.0, 0.0, 0.0),
                d: DVec3::Z,
            }),
            o: DVec3::ZERO,
            axis: DVec3::Z,
        };
        let p = rev.eval(1.0, 2.0);
        assert!((p.truncate().length() - 3.0).abs() < 1e-9 && (p.z - 2.0).abs() < 1e-9);
        let (u, v) = rev.uv_of(p, None).unwrap();
        assert!(near(rev.eval(u, v), p));
        assert!(rev.normal(1.0, 2.0).is_some());
        assert_eq!(rev.period_u(), Some(TAU));
        // A circle profile turned around Z gives a torus.
        let tor = Surface::Revolution {
            curve: Arc::new(Curve::Circle {
                f: DAffine3::from_translation(DVec3::new(10.0, 0.0, 0.0))
                    * DAffine3::from_rotation_x(FRAC_PI_2),
                r: 2.0,
            }),
            o: DVec3::ZERO,
            axis: DVec3::Z,
        };
        assert_eq!(tor.period_v(), Some(TAU));
        let p = tor.eval(0.7, 1.1);
        let (u, v) = tor.uv_of(p, Some((0.7, 1.1))).unwrap();
        assert!(near(tor.eval(u, v), p));
        let ext = Surface::Extrusion {
            curve: Arc::new(Curve::Circle {
                f: DAffine3::IDENTITY,
                r: 2.0,
            }),
            dir: DVec3::Z,
        };
        let p = ext.eval(0.5, 7.0);
        let (u, v) = ext.uv_of(p, None).unwrap();
        assert!(near(ext.eval(u, v), p));
        assert!(ext.normal(0.5, 7.0).is_some());
        assert_eq!(ext.period_v(), None);
        let line_ext = Surface::Extrusion {
            curve: Arc::new(Curve::Line {
                o: DVec3::ZERO,
                d: DVec3::X,
            }),
            dir: DVec3::Z,
        };
        assert!(line_ext.uv_of(DVec3::ONE, None).is_none());
        // A bumpy patch.
        let mut pts = Vec::new();
        for i in 0..4 {
            for j in 0..4 {
                pts.push(DVec3::new(
                    i as f64 * 10.0,
                    j as f64 * 10.0,
                    ((i + j) % 2) as f64 * 4.0,
                ));
            }
        }
        let sp = Surface::Spline(BSplineSurface {
            u: Knots::clamped(3, 4).unwrap(),
            v: Knots::clamped(3, 4).unwrap(),
            points: pts,
            weights: None,
        });
        for (u, v) in [(0.3, 0.8), (0.0, 0.0), (1.0, 1.0)] {
            let p = sp.eval(u, v);
            let (a, b) = sp.uv_of(p, None).unwrap();
            assert!(near(sp.eval(a, b), p), "{u} {v}");
            let (a, b) = sp.uv_of(p, Some((0.5, 0.5))).unwrap();
            assert!(near(sp.eval(a, b), p));
        }
        assert!(sp.normal(0.5, 0.5).is_some());
        assert_eq!(sp.kind(), "B_SPLINE_SURFACE");
    }
}
