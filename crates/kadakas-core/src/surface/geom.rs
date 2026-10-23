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

/// The triangles of a face whose holes touch the outer loop at a vertex, where a bridge cannot be
/// found: each such hole is joined to the outer loop at the shared vertex (a bridge of length
/// zero), and the other holes are bridged as usual. Indices are into `vertices` (the loops one
/// after the other).
pub(super) fn spliced(
    f: &Face,
    vertices: &[VertexId],
    pos: &impl Fn(VertexId) -> DVec3,
) -> Option<Vec<[u32; 3]>> {
    let mut ring: Vec<VertexId> = f.outer.clone();
    let mut rest: Vec<&Vec<VertexId>> = Vec::new();
    for h in &f.holes {
        let shared = ring
            .iter()
            .enumerate()
            .find_map(|(i, v)| h.iter().position(|w| w == v).map(|j| (i, j)));
        match shared {
            Some((i, j)) => {
                let n = h.len();
                let mut joined: Vec<VertexId> = ring[..=i].to_vec();
                joined.extend((1..=n).map(|k| h[(j + k) % n]));
                joined.extend_from_slice(&ring[i + 1..]);
                ring = joined;
            }
            None => rest.push(h),
        }
    }
    let rings: Vec<Vec<DVec2>> = std::iter::once(&ring)
        .chain(rest.iter().copied())
        .map(|l| l.iter().map(|v| f.plane.project(pos(*v))).collect())
        .collect();
    let mut ids: Vec<VertexId> = ring.clone();
    for h in &rest {
        ids.extend(h.iter().copied());
    }
    let first: std::collections::BTreeMap<VertexId, u32> = vertices
        .iter()
        .enumerate()
        .rev()
        .map(|(i, v)| (*v, i as u32))
        .collect();
    let tris = triangulate_rings(&rings)?;
    Some(
        tris.into_iter()
            .map(|t| t.map(|i| first[&ids[i as usize]]))
            .collect(),
    )
}

/// Flips away the triangles with no area. The triangulation of a polygon with a corner on the line
/// between two others makes one: a triangle `(a, m, b)` with `m` on the segment `ab`. It draws
/// nothing, but the neighbour across `ab` then has a long edge where the other side has two, a
/// crack when the mesh is written to a file. The triangle and its neighbour `(b, a, c)` become
/// `(a, m, c)` and `(m, b, c)`, which cover the same area and share the edges. `p` are the
/// positions of the indices in the plane. A triangle that has no neighbour is dropped.
pub(super) fn flip_slivers(tris: &mut Vec<[u32; 3]>, p: &[DVec2]) {
    let flat = |t: &[u32; 3]| -> Option<usize> {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        let cross = (b - a).perp_dot(c - a).abs();
        let longest = (b - a)
            .length_squared()
            .max((c - b).length_squared())
            .max((a - c).length_squared());
        if cross > 1e-5f64.max(1e-9 * longest) {
            return None;
        }
        // The middle corner: the one whose opposite edge is the longest.
        let lens = [
            (c - b).length_squared(),
            (a - c).length_squared(),
            (b - a).length_squared(),
        ];
        (0..3).max_by(|&i, &j| lens[i].total_cmp(&lens[j]))
    };
    for _ in 0..tris.len() + 8 {
        let Some(i) = tris.iter().position(|t| flat(t).is_some()) else {
            return;
        };
        let t = tris[i];
        let m = flat(&t).unwrap_or(0);
        // Corner `m` is the middle one, the edge `(x, y)` opposite it runs x to y in `t`.
        let (mv, x, y) = (t[m], t[(m + 1) % 3], t[(m + 2) % 3]);
        // The neighbour has the edge the other way round.
        let nb = tris.iter().enumerate().find_map(|(j, n)| {
            if j == i {
                return None;
            }
            (0..3).find_map(|k| (n[k] == y && n[(k + 1) % 3] == x).then_some((j, n[(k + 2) % 3])))
        });
        match nb {
            Some((j, c)) => {
                // The neighbour runs y, x, c: the new triangles run y, m, c and m, x, c.
                tris[i] = [y, mv, c];
                tris[j] = [mv, x, c];
            }
            None => {
                tris.remove(i);
            }
        }
    }
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
    let mut triangles = match triangulate_rings(&rings) {
        Some(t) => t,
        None => spliced(f, &vertices, &pos)?,
    };
    let flat: Vec<DVec2> = rings.iter().flatten().copied().collect();
    flip_slivers(&mut triangles, &flat);
    Some(FaceTriangles {
        face: id,
        normal: f.plane.normal,
        vertices,
        positions,
        triangles,
    })
}
