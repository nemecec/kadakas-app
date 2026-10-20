//! A construction plane: where a rectangle, a circle or an arc is drawn.

use glam::{DVec2, DVec3};

use crate::infer3d::Ray;

/// A plane with two unit axes in it. `u × v = n`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane3 {
    pub origin: DVec3,
    pub n: DVec3,
    pub u: DVec3,
    pub v: DVec3,
}

impl Plane3 {
    /// The ground plane at height `z`: the axes are the red and the green axis.
    pub fn ground(z: f64) -> Plane3 {
        Plane3::through(DVec3::new(0.0, 0.0, z), DVec3::Z).expect("a unit normal")
    }

    /// The plane through `origin` with the normal `n`. A horizontal plane is axis aligned (`u` is
    /// X); any other plane has a horizontal `u` and a `v` that points up in it, so a rectangle on
    /// a wall has a horizontal and a vertical side. `None` for a zero normal.
    pub fn through(origin: DVec3, n: DVec3) -> Option<Plane3> {
        let n = n.try_normalize()?;
        let u = if n.z.abs() > 0.999_999 {
            DVec3::X
        } else {
            DVec3::Z.cross(n).normalize()
        };
        let v = n.cross(u);
        Some(Plane3 { origin, n, u, v })
    }

    /// The point where the ray meets the plane, if it does (in front of the ray origin).
    pub fn intersect(&self, ray: &Ray) -> Option<DVec3> {
        let den = self.n.dot(ray.dir);
        if den.abs() < 1e-9 {
            return None;
        }
        let t = self.n.dot(self.origin - ray.origin) / den;
        (t.is_finite() && t > 0.0).then(|| ray.origin + ray.dir * t)
    }

    /// The nearest point of the plane.
    pub fn project(&self, p: DVec3) -> DVec3 {
        p - self.n * self.n.dot(p - self.origin)
    }

    /// The coordinates of a point along `u` and `v`, from `origin`.
    pub fn coords(&self, p: DVec3) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.dot(self.u), d.dot(self.v))
    }

    /// The point at `(x, y)` along the axes from `origin`.
    pub fn at(&self, origin: DVec3, x: f64, y: f64) -> DVec3 {
        origin + self.u * x + self.v * y
    }

    /// The same plane with the origin moved to `p` (a point of the plane or not).
    pub fn with_origin(&self, p: DVec3) -> Plane3 {
        Plane3 { origin: p, ..*self }
    }
}
