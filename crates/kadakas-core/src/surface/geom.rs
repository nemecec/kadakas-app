//! Geometry of surface models: tolerances, planes, distances, areas and the triangles of a face.

use glam::{DVec2, DVec3};

use super::{
    Face, FaceId, FaceTriangles, LENGTH_TOLERANCE, PLANE_TOLERANCE_FACTOR, RELATIVE_TOLERANCE,
    VertexId,
};

/// The length tolerance in mm for points whose largest coordinate (absolute) is `size` mm.
pub fn tolerance_at(size: f64) -> f64 {
    LENGTH_TOLERANCE.max(RELATIVE_TOLERANCE * size)
}

/// The plane tolerance in mm for points whose largest coordinate (absolute) is `size` mm.
pub fn plane_tolerance_at(size: f64) -> f64 {
    tolerance_at(size) * PLANE_TOLERANCE_FACTOR
}

/// The size of a point for its tolerance: its largest coordinate in absolute value.
pub(super) fn size(p: DVec3) -> f64 {
    p.abs().max_element()
}

/// The length tolerance for a set of points.
pub(super) fn tol(points: &[DVec3]) -> f64 {
    tolerance_at(points.iter().map(|p| size(*p)).fold(0.0, f64::max))
}

/// The plane tolerance for a set of points.
pub(super) fn plane_tol(points: &[DVec3]) -> f64 {
    tol(points) * PLANE_TOLERANCE_FACTOR
}

/// A plane: the points `p` with `normal · p = offset`. `normal` has unit length and points to the
/// front.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: DVec3,
    pub offset: f64,
}

impl Plane {
    /// The plane through `p` with the normal `n` (made unit length). `None` for a zero normal.
    pub fn new(p: DVec3, n: DVec3) -> Option<Plane> {
        let normal = n.try_normalize()?;
        Some(Plane {
            normal,
            offset: normal.dot(p),
        })
    }

    /// The signed distance of a point (positive in front).
    pub fn distance(&self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.offset
    }

    /// The same plane seen from the other side.
    pub fn flipped(&self) -> Plane {
        Plane {
            normal: -self.normal,
            offset: -self.offset,
        }
    }

    /// Two unit axes `u`, `v` in the plane with `u × v = normal`, so counterclockwise in `(u, v)`
    /// is counterclockwise seen from the front. The same plane always gives the same axes.
    pub fn basis(&self) -> (DVec3, DVec3) {
        let n = self.normal;
        let a = n.abs();
        // The world axis that is most nearly in the plane.
        let axis = if a.x <= a.y && a.x <= a.z {
            DVec3::X
        } else if a.y <= a.z {
            DVec3::Y
        } else {
            DVec3::Z
        };
        let u = (axis - n * axis.dot(n)).normalize();
        (u, n.cross(u))
    }

    /// A point in the 2D axes of [`Plane::basis`].
    pub fn project(&self, p: DVec3) -> DVec2 {
        let (u, v) = self.basis();
        DVec2::new(u.dot(p), v.dot(p))
    }
}

/// True if `p` lies on the plane within the plane tolerance at `p`.
pub fn on_plane(plane: &Plane, p: DVec3) -> bool {
    plane.distance(p).abs() <= plane_tolerance_at(size(p))
}

/// Twice the vector area of a closed loop (Newell's method). Its direction is the normal of a
/// counterclockwise loop.
pub(super) fn newell(points: &[DVec3]) -> DVec3 {
    let n = points.len();
    if n == 0 {
        return DVec3::ZERO;
    }
    // Relative to the first point, which keeps the rounding small far from the origin.
    let o = points[0];
    (0..n)
        .map(|i| (points[i] - o).cross(points[(i + 1) % n] - o))
        .sum()
}

/// Twice the signed area of a 2D ring (positive counterclockwise).
pub(super) fn area2(points: &[DVec2]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let o = points[0];
    (1..n - 1)
        .map(|i| (points[i] - o).perp_dot(points[i + 1] - o))
        .sum()
}

/// The area of a face in mm²: the outer loop less the holes, measured in its plane.
pub(super) fn face_area(f: &Face, pos: impl Fn(VertexId) -> DVec3) -> f64 {
    let n = f.plane.normal;
    let ring =
        |l: &Vec<VertexId>| newell(&l.iter().map(|v| pos(*v)).collect::<Vec<_>>()).dot(n) / 2.0;
    let outer = ring(&f.outer);
    let holes: f64 = f.holes.iter().map(|h| -ring(h)).sum();
    outer - holes
}

/// The box around points, or `None` without points.
pub(super) fn bounds(points: impl Iterator<Item = DVec3>) -> Option<(DVec3, DVec3)> {
    points.fold(None, |acc, p| match acc {
        None => Some((p, p)),
        Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
    })
}

/// The distance from `p` to the segment `a`–`b`, and the parameter (0 at `a`, 1 at `b`) of the
/// nearest point.
pub(super) fn point_segment(p: DVec3, a: DVec3, b: DVec3) -> (f64, f64) {
    let d = b - a;
    let len2 = d.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(d) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a + d * t).distance(p), t)
}

/// The nearest points of the segments `p1`–`q1` and `p2`–`q2` as parameters `(s, t)` and their
/// distance. `None` when the segments are parallel (an overlap is found from the end points).
pub(super) fn segment_segment(
    p1: DVec3,
    q1: DVec3,
    p2: DVec3,
    q2: DVec3,
) -> Option<(f64, f64, f64)> {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    let (b, c) = (d1.dot(d2), d1.dot(r));
    let denom = a * e - b * b;
    // Parallel (or a zero segment): sin² of the angle below 1e-18.
    if a == 0.0 || e == 0.0 || denom <= 1e-18 * a * e {
        return None;
    }
    let mut s = ((b * f - c * e) / denom).clamp(0.0, 1.0);
    let mut t = (b * s + f) / e;
    if t < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    }
    let dist = (p1 + d1 * s).distance(p2 + d2 * t);
    Some((s, t, dist))
}

/// The distance from `p` to the line through `a` and `b`.
pub(super) fn point_line(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let d = b - a;
    let len = d.length();
    if len == 0.0 {
        return p.distance(a);
    }
    (p - a).cross(d).length() / len
}

/// Triangles of a polygon with holes in 2D, as indices into the rings written one after the other.
/// The outer ring must run counterclockwise and the holes clockwise.
pub(super) fn triangulate_rings(rings: &[Vec<DVec2>]) -> Option<Vec<[u32; 3]>> {
    crate::solid3d::cap_triangles(rings)
}

/// The triangles of a face, counterclockwise seen from the front.
pub(super) fn triangulate_face(
    id: FaceId,
    f: &Face,
    pos: impl Fn(VertexId) -> DVec3,
) -> Option<FaceTriangles> {
    let vertices: Vec<VertexId> = f.loops().flatten().copied().collect();
    let positions: Vec<DVec3> = vertices.iter().map(|v| pos(*v)).collect();
    let rings: Vec<Vec<DVec2>> = f
        .loops()
        .map(|l| l.iter().map(|v| f.plane.project(pos(*v))).collect())
        .collect();
    let triangles = triangulate_rings(&rings)?;
    Some(FaceTriangles {
        face: id,
        normal: f.plane.normal,
        vertices,
        positions,
        triangles,
    })
}
