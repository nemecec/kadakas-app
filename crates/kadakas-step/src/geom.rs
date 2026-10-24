//! Geometry entities of a STEP file: points, directions, placements, curves and surfaces.

use std::f64::consts::TAU;
use std::rc::Rc;

use glam::{DAffine3, DMat3, DVec3};

use crate::ctx::{Ctx, Fail, R, attr, enum_name, flag, int, list, num, reals, reference, refs, skip};
use crate::curve::Curve;
use crate::nurbs::{BSplineCurve, BSplineSurface, Knots, MAX_DEGREE};
use crate::part21::{COMPLEX, Entity, Value};
use crate::surface::Surface;

/// The largest number in a coordinate or a size, in file units.
const BIG: f64 = 1.0e12;

fn finite3(v: DVec3, what: &str) -> R<DVec3> {
    if v.is_finite() && v.abs().max_element() <= BIG {
        Ok(v)
    } else {
        skip(format!("{what} is not a usable number"))
    }
}

/// A frame from an origin, a Z axis and a reference direction for X. A missing or parallel
/// reference gives any perpendicular direction.
pub fn frame(o: DVec3, z: Option<DVec3>, x: Option<DVec3>) -> DAffine3 {
    let z = z.filter(|z| z.length() > 1e-12).map_or(DVec3::Z, |z| z.normalize());
    let mut xr = x.unwrap_or(DVec3::X);
    xr -= z * xr.dot(z);
    if xr.length() < 1e-9 {
        xr = z.any_orthonormal_vector();
    }
    let x = xr.normalize();
    let y = z.cross(x);
    DAffine3 {
        matrix3: DMat3::from_cols(x, y, z),
        translation: o,
    }
}

impl<'a> Ctx<'a> {
    pub fn point(&mut self, id: u64) -> R<DVec3> {
        let e = self.ent(id)?;
        if e.name != "CARTESIAN_POINT" {
            return skip(format!("#{id} is a {}, not a CARTESIAN_POINT", e.name));
        }
        let c = reals(attr(e, 1)?).ok_or(()).or_else(|()| skip("a point has no numbers"))?;
        if c.is_empty() || c.len() > 3 {
            return skip("a point needs 1 to 3 coordinates");
        }
        finite3(
            DVec3::new(c[0], *c.get(1).unwrap_or(&0.0), *c.get(2).unwrap_or(&0.0)),
            "a coordinate",
        )
    }

    /// A direction as a unit vector.
    pub fn direction(&mut self, id: u64) -> R<DVec3> {
        let e = self.ent(id)?;
        if e.name != "DIRECTION" {
            return skip(format!("#{id} is a {}, not a DIRECTION", e.name));
        }
        let c = reals(attr(e, 1)?).ok_or(()).or_else(|()| skip("a direction has no numbers"))?;
        if c.len() < 2 || c.len() > 3 {
            return skip("a direction needs 2 or 3 numbers");
        }
        let d = DVec3::new(c[0], c[1], *c.get(2).unwrap_or(&0.0));
        if !d.is_finite() || d.length() < 1e-12 || d.abs().max_element() > BIG {
            return skip("a direction has no length");
        }
        Ok(d.normalize())
    }

    /// A VECTOR: the direction times the magnitude.
    pub fn vector(&mut self, id: u64) -> R<DVec3> {
        let e = self.ent(id)?;
        if e.name != "VECTOR" {
            return skip(format!("#{id} is a {}, not a VECTOR", e.name));
        }
        let d = self.direction(reference(attr(e, 1)?).unwrap_or(0))?;
        let m = num(attr(e, 2)?).filter(|m| m.abs() <= BIG);
        let m = m.ok_or(()).or_else(|()| skip("a vector has no magnitude"))?;
        Ok(d * m)
    }

    /// An AXIS2_PLACEMENT_3D as a frame.
    pub fn axis2(&mut self, id: u64) -> R<DAffine3> {
        let e = self.ent(id)?;
        if e.name != "AXIS2_PLACEMENT_3D" {
            return skip(format!("#{id} is a {}, not an AXIS2_PLACEMENT_3D", e.name));
        }
        let o = self.point(reference(attr(e, 1)?).unwrap_or(0))?;
        let z = match reference(attr(e, 2)?) {
            Some(r) => Some(self.direction(r)?),
            None => None,
        };
        let x = match reference(attr(e, 3)?) {
            Some(r) => Some(self.direction(r)?),
            None => None,
        };
        Ok(frame(o, z, x))
    }

    /// An AXIS1_PLACEMENT as an origin and a unit axis.
    pub fn axis1(&mut self, id: u64) -> R<(DVec3, DVec3)> {
        let e = self.ent(id)?;
        if e.name != "AXIS1_PLACEMENT" {
            return skip(format!("#{id} is a {}, not an AXIS1_PLACEMENT", e.name));
        }
        let o = self.point(reference(attr(e, 1)?).unwrap_or(0))?;
        let z = match reference(attr(e, 2)?) {
            Some(r) => self.direction(r)?,
            None => DVec3::Z,
        };
        Ok((o, z))
    }

    fn radius(&self, v: &Value, what: &str) -> R<f64> {
        match num(v) {
            Some(r) if r > 0.0 && r <= BIG => Ok(r),
            _ => skip(format!("{what} is not a positive size")),
        }
    }

    // Curves.

    pub fn curve(&mut self, id: u64) -> R<Rc<Curve>> {
        if let Some(c) = self.curves.get(&id) {
            return Ok(c.clone());
        }
        if self.depth > 24 {
            return skip("curves are nested too deep (a cycle?)");
        }
        self.depth += 1;
        let r = self.curve_inner(id);
        self.depth -= 1;
        let c = Rc::new(r?);
        self.curves.insert(id, c.clone());
        Ok(c)
    }

    fn curve_inner(&mut self, id: u64) -> R<Curve> {
        let e = self.ent(id)?;
        let name = e.name.as_str();
        match name {
            "LINE" => {
                let o = self.point(reference(attr(e, 1)?).unwrap_or(0))?;
                let d = self.vector(reference(attr(e, 2)?).unwrap_or(0))?;
                if d.length() < 1e-12 {
                    return skip("a line has a vector of length 0");
                }
                Ok(Curve::Line { o, d })
            }
            "CIRCLE" => {
                let f = self.axis2(reference(attr(e, 1)?).unwrap_or(0))?;
                let r = self.radius(attr(e, 2)?, "the radius of a circle")?;
                Ok(Curve::Circle { f, r })
            }
            "ELLIPSE" => {
                let f = self.axis2(reference(attr(e, 1)?).unwrap_or(0))?;
                let a = self.radius(attr(e, 2)?, "a semi-axis of an ellipse")?;
                let b = self.radius(attr(e, 3)?, "a semi-axis of an ellipse")?;
                Ok(Curve::Ellipse { f, a, b })
            }
            "POLYLINE" => {
                let ids = refs(attr(e, 1)?);
                if ids.len() < 2 {
                    return skip("a polyline needs two points");
                }
                if ids.len() > self.limits.max_curve_points {
                    return skip(format!("a polyline with {} points is over the limit", ids.len()));
                }
                let mut pts = Vec::with_capacity(ids.len());
                for p in ids {
                    pts.push(self.point(p)?);
                }
                Ok(Curve::Poly(pts))
            }
            "TRIMMED_CURVE" => self.trimmed(e),
            "COMPOSITE_CURVE" => {
                let segs = refs(attr(e, 1)?);
                if segs.is_empty() || segs.len() > 100_000 {
                    return skip("a composite curve has no segments or too many");
                }
                let mut out = Vec::with_capacity(segs.len());
                for s in segs {
                    let se = self.ent(s)?;
                    if se.name != "COMPOSITE_CURVE_SEGMENT" {
                        return skip(format!("a composite curve holds a {}", se.name));
                    }
                    let same = flag(attr(se, 1)?).unwrap_or(true);
                    let c = self.curve(reference(attr(se, 2)?).unwrap_or(0))?;
                    out.push((c, same));
                }
                Ok(Curve::Composite(out))
            }
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE" => {
                // The curve in space is used. The curves in the parameters of the surfaces are not.
                let c = self.curve(reference(attr(e, 1)?).unwrap_or(0))?;
                Ok((*c).clone())
            }
            "B_SPLINE_CURVE_WITH_KNOTS" | "B_SPLINE_CURVE" | "BEZIER_CURVE" | "QUASI_UNIFORM_CURVE"
            | "UNIFORM_CURVE" => self.spline_curve(e),
            COMPLEX if e.is("B_SPLINE_CURVE") => self.spline_curve(e),
            other => skip(format!("the curve type {other} is not supported")),
        }
    }

    /// A trim of a TRIMMED_CURVE: a parameter or a point.
    fn trim_value(&mut self, v: &Value, basis: &Curve, units: f64) -> R<(Option<f64>, Option<DVec3>)> {
        let mut par = None;
        let mut pt = None;
        for item in list(v).unwrap_or(std::slice::from_ref(v)) {
            match item {
                Value::Ref(r) => pt = Some(self.point(*r)?),
                Value::Typed(_, inner) => par = num(inner).map(|x| {
                    if basis.period().is_some() { x * units } else { x }
                }),
                other => par = num(other),
            }
        }
        Ok((par, pt))
    }

    fn trimmed(&mut self, e: &Entity) -> R<Curve> {
        let basis = self.curve(reference(attr(e, 1)?).unwrap_or(0))?;
        let sense = flag(attr(e, 4)?).unwrap_or(true);
        let master = enum_name(attr(e, 5)?).unwrap_or("UNSPECIFIED");
        let units = self.angle_unit;
        let (p1, q1) = self.trim_value(attr(e, 2)?, &basis, units)?;
        let (p2, q2) = self.trim_value(attr(e, 3)?, &basis, units)?;
        let use_points = (master == "CARTESIAN" || p1.is_none() || p2.is_none())
            && q1.is_some()
            && q2.is_some();
        let (mut t0, mut t1) = if use_points {
            (basis.param_of(q1.unwrap_or_default()), basis.param_of(q2.unwrap_or_default()))
        } else if let (Some(a), Some(b)) = (p1, p2) {
            (a, b)
        } else if let Some((a, b)) = basis.natural_range() {
            (a, b)
        } else {
            return skip("a trimmed curve has no usable trims");
        };
        if let Some(period) = basis.period() {
            if sense {
                t1 = t0 + (t1 - t0).rem_euclid(period);
                if t1 - t0 < 1e-12 {
                    t1 = t0 + period;
                }
            } else {
                t1 = t0 - (t0 - t1).rem_euclid(period);
                if t0 - t1 < 1e-12 {
                    t1 = t0 - period;
                }
            }
        } else if !sense && t1 > t0 {
            std::mem::swap(&mut t0, &mut t1);
        } else if sense && t1 < t0 {
            std::mem::swap(&mut t0, &mut t1);
        }
        if !(t0.is_finite() && t1.is_finite()) || (t1 - t0).abs() > BIG {
            return skip("a trimmed curve has trims that are not usable");
        }
        Ok(Curve::Trimmed { basis, t0, t1 })
    }

    fn spline_curve(&mut self, e: &Entity) -> R<Curve> {
        let complex = e.name == COMPLEX;
        // `base` starts at the degree, `knot_part` at the multiplicities.
        let (base, knot_part, rational, kind) = if complex {
            let b = e.part("B_SPLINE_CURVE").ok_or(()).or_else(|()| skip("a spline curve without its attributes"))?;
            let wk = e.part("B_SPLINE_CURVE_WITH_KNOTS");
            let kind = if wk.is_some() { "B_SPLINE_CURVE_WITH_KNOTS" } else { "B_SPLINE_CURVE" };
            (b, wk, e.part("RATIONAL_B_SPLINE_CURVE"), kind)
        } else {
            let wk = (e.name == "B_SPLINE_CURVE_WITH_KNOTS").then(|| e.attrs.get(6..).unwrap_or(&[]));
            (e.attrs.get(1..).unwrap_or(&[]), wk, None, e.name.as_str())
        };
        let degree = int(attr_s(base, 0)?)
            .filter(|d| *d >= 1)
            .ok_or(())
            .or_else(|()| skip("the degree of a spline is missing"))? as usize;
        if degree > MAX_DEGREE {
            return skip(format!("the degree {degree} of a spline is over the limit {MAX_DEGREE}"));
        }
        let ids = refs(attr_s(base, 1)?);
        if ids.len() > self.limits.max_control_points {
            return skip(format!("a spline with {} control points is over the limit", ids.len()));
        }
        let mut points = Vec::with_capacity(ids.len());
        for p in &ids {
            points.push(self.point(*p)?);
        }
        let count = points.len();
        let knots = match knot_part {
            Some(kp) => {
                let mult: Vec<i64> = list(attr_s(kp, 0)?).map(|l| l.iter().filter_map(int).collect()).unwrap_or_default();
                let kn = reals(attr_s(kp, 1)?).unwrap_or_default();
                Knots::expand(degree, count, &mult, &kn)
            }
            None if kind == "UNIFORM_CURVE" => Knots::uniform(degree, count),
            None => Knots::clamped(degree, count),
        }
        .map_err(|m| Fail::Skip(format!("spline curve: {m}")))?;
        let weights = match rational {
            Some(w) => {
                let w = reals(attr_s(w, 0)?).unwrap_or_default();
                if w.len() != count || w.iter().any(|x| *x <= 0.0 || !x.is_finite()) {
                    return skip("the weights of a spline curve are not usable");
                }
                Some(w)
            }
            None => None,
        };
        Ok(Curve::Spline(BSplineCurve { knots, points, weights }))
    }

    // Surfaces.

    pub fn surface(&mut self, id: u64) -> R<Rc<Surface>> {
        if let Some(s) = self.surfaces.get(&id) {
            return Ok(s.clone());
        }
        if self.depth > 24 {
            return skip("surfaces are nested too deep (a cycle?)");
        }
        self.depth += 1;
        let r = self.surface_inner(id);
        self.depth -= 1;
        let s = Rc::new(r?);
        self.surfaces.insert(id, s.clone());
        Ok(s)
    }

    fn surface_inner(&mut self, id: u64) -> R<Surface> {
        let e = self.ent(id)?;
        let pos = |ctx: &mut Ctx, i: usize| -> R<DAffine3> {
            ctx.axis2(reference(attr(e, i)?).unwrap_or(0))
        };
        match e.name.as_str() {
            "PLANE" => Ok(Surface::Plane { f: pos(self, 1)? }),
            "CYLINDRICAL_SURFACE" => {
                let f = pos(self, 1)?;
                let r = self.radius(attr(e, 2)?, "the radius of a cylinder")?;
                Ok(Surface::Cylinder { f, r })
            }
            "CONICAL_SURFACE" => {
                let f = pos(self, 1)?;
                let r = num(attr(e, 2)?).filter(|r| *r >= 0.0 && *r <= BIG).ok_or(()).or_else(|()| skip("the radius of a cone is not usable"))?;
                let a = num(attr(e, 3)?).ok_or(()).or_else(|()| skip("the angle of a cone is missing"))? * self.angle_unit;
                if !(a.abs() > 1e-9 && a.abs() < TAU / 4.0 - 1e-6) {
                    return skip("the half angle of a cone is not between 0 and 90 degrees");
                }
                Ok(Surface::Cone { f, r, angle: a })
            }
            "SPHERICAL_SURFACE" => {
                let f = pos(self, 1)?;
                let r = self.radius(attr(e, 2)?, "the radius of a sphere")?;
                Ok(Surface::Sphere { f, r })
            }
            "TOROIDAL_SURFACE" => {
                let f = pos(self, 1)?;
                let big = self.radius(attr(e, 2)?, "the major radius of a torus")?;
                let small = self.radius(attr(e, 3)?, "the minor radius of a torus")?;
                if small >= big {
                    return skip("a torus with a minor radius that is not smaller than the major radius is not supported");
                }
                Ok(Surface::Torus { f, big, small })
            }
            "SURFACE_OF_REVOLUTION" => {
                let curve = self.curve(reference(attr(e, 1)?).unwrap_or(0))?;
                let (o, axis) = self.axis1(reference(attr(e, 2)?).unwrap_or(0))?;
                Ok(Surface::Revolution { curve, o, axis })
            }
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                let curve = self.curve(reference(attr(e, 1)?).unwrap_or(0))?;
                let v = self.vector(reference(attr(e, 2)?).unwrap_or(0))?;
                if v.length() < 1e-12 {
                    return skip("an extrusion has a vector of length 0");
                }
                if curve.natural_range().is_none() {
                    return skip("an extrusion of a line without ends is not supported");
                }
                Ok(Surface::Extrusion { curve, dir: v.normalize() })
            }
            "RECTANGULAR_TRIMMED_SURFACE" => {
                // The bounds of the faces cut the surface, so the basis surface is what counts.
                let s = self.surface(reference(attr(e, 1)?).unwrap_or(0))?;
                Ok((*s).clone())
            }
            "B_SPLINE_SURFACE_WITH_KNOTS" | "B_SPLINE_SURFACE" | "BEZIER_SURFACE"
            | "QUASI_UNIFORM_SURFACE" | "UNIFORM_SURFACE" => self.spline_surface(e),
            COMPLEX if e.is("B_SPLINE_SURFACE") => self.spline_surface(e),
            other => skip(format!("the surface type {other} is not supported")),
        }
    }

    fn spline_surface(&mut self, e: &Entity) -> R<Surface> {
        let complex = e.name == COMPLEX;
        // `base` starts at the u degree, `knot_part` at the u multiplicities.
        let (base, knot_part, rational, kind) = if complex {
            let b = e.part("B_SPLINE_SURFACE").ok_or(()).or_else(|()| skip("a spline surface without its attributes"))?;
            let wk = e.part("B_SPLINE_SURFACE_WITH_KNOTS");
            let kind = if wk.is_some() { "B_SPLINE_SURFACE_WITH_KNOTS" } else { "B_SPLINE_SURFACE" };
            (b, wk, e.part("RATIONAL_B_SPLINE_SURFACE"), kind)
        } else {
            let wk = (e.name == "B_SPLINE_SURFACE_WITH_KNOTS").then(|| e.attrs.get(8..).unwrap_or(&[]));
            (e.attrs.get(1..).unwrap_or(&[]), wk, None, e.name.as_str())
        };
        let degree = |i: usize| -> R<usize> {
            int(attr_s(base, i)?)
                .filter(|d| *d >= 1)
                .map(|d| d as usize)
                .ok_or(())
                .or_else(|()| skip("the degree of a spline surface is missing"))
        };
        let (du, dv) = (degree(0)?, degree(1)?);
        if du > MAX_DEGREE || dv > MAX_DEGREE {
            return skip(format!("the degrees {du} and {dv} of a spline surface are over the limit {MAX_DEGREE}"));
        }
        let rows = list(attr_s(base, 2)?).ok_or(()).or_else(|()| skip("a spline surface has no control points"))?;
        let nu = rows.len();
        let nv = rows.first().and_then(list).map_or(0, |r| r.len());
        if nu == 0 || nv == 0 || nu.saturating_mul(nv) > self.limits.max_control_points {
            return skip(format!(
                "a spline surface with {nu} x {nv} control points is empty or over the limit {}",
                self.limits.max_control_points
            ));
        }
        let mut points = Vec::with_capacity(nu * nv);
        for row in rows {
            let ids = list(row)
                .filter(|r| r.len() == nv)
                .ok_or(())
                .or_else(|()| skip("the rows of control points differ in length"))?;
            for p in ids {
                points.push(self.point(reference(p).unwrap_or(0))?);
            }
        }
        let ints = |v: &Value| -> Vec<i64> { list(v).map(|l| l.iter().filter_map(int).collect()).unwrap_or_default() };
        let (ku, kv) = match knot_part {
            Some(kp) => (
                Knots::expand(du, nu, &ints(attr_s(kp, 0)?), &reals(attr_s(kp, 2)?).unwrap_or_default()),
                Knots::expand(dv, nv, &ints(attr_s(kp, 1)?), &reals(attr_s(kp, 3)?).unwrap_or_default()),
            ),
            None if kind == "UNIFORM_SURFACE" => (Knots::uniform(du, nu), Knots::uniform(dv, nv)),
            None => (Knots::clamped(du, nu), Knots::clamped(dv, nv)),
        };
        let u = ku.map_err(|m| Fail::Skip(format!("spline surface: {m}")))?;
        let v = kv.map_err(|m| Fail::Skip(format!("spline surface: {m}")))?;
        let weights = match rational {
            Some(w) => {
                let rows = list(attr_s(w, 0)?).ok_or(()).or_else(|()| skip("the weights are missing"))?;
                let mut out = Vec::with_capacity(nu * nv);
                for r in rows {
                    out.extend(reals(r).unwrap_or_default());
                }
                if out.len() != nu * nv || out.iter().any(|x| *x <= 0.0 || !x.is_finite()) {
                    return skip("the weights of a spline surface are not usable");
                }
                Some(out)
            }
            None => None,
        };
        Ok(Surface::Spline(BSplineSurface { u, v, points, weights }))
    }
}

fn attr_s(a: &[Value], i: usize) -> R<&Value> {
    a.get(i).map_or_else(|| skip("an attribute is missing"), Ok)
}
