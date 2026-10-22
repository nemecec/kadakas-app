//! Where a point lies against a closed surface: inside, outside or on it.
//!
//! The surface is a soup of triangles (the faces cut into triangles, with their front normals) in a
//! box tree. A point within the plane tolerance of a triangle is *on* the surface, and the answer
//! carries the front normal of that face. Any other point is cast a ray; an odd number of hits
//! means inside. A ray that passes an edge or a vertex of a triangle, or runs in the plane of one,
//! is not counted: another direction is tried (six fixed ones), so the answer does not depend on
//! luck. If all fail the answer is `None` and the operation is refused.

use glam::DVec3;

use super::super::geom;
use super::super::index::BoxTree;
use super::super::{FaceId, SurfaceModel};

/// Where a point is against a closed surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Where {
    In,
    Out,
    /// On the surface; the front normal of the face there.
    On(DVec3),
}

struct Tri {
    p: [DVec3; 3],
    /// The front normal of the face.
    n: DVec3,
}

pub(super) struct Soup {
    tris: Vec<Tri>,
    tree: BoxTree,
    lo: DVec3,
    hi: DVec3,
}

/// The directions of the rays. They are not along an axis or a diagonal, so that boxes and other
/// regular shapes do not make a ray run along an edge or a plane.
const RAYS: [[f64; 3]; 6] = [
    [0.267_261_241_912, 0.534_522_483_825, 0.801_783_725_737],
    [-0.371_390_676_354, 0.557_086_014_531, -0.742_781_352_708],
    [0.623_735_040_4, -0.311_867_520_2, 0.717_295_296_5],
    [-0.459_479_8, -0.321_635_86, 0.827_063_64],
    [0.163_340_9, -0.884_460_9, -0.435_575_0],
    [-0.696_526_3, 0.139_305_3, 0.704_104_0],
];

/// What a ray does to a triangle.
enum Hit {
    Miss,
    Yes,
    /// It passes an edge or a vertex, or runs in the plane: this ray cannot be counted.
    Unsure,
}

impl Soup {
    /// The soup of the given faces, or `None` when a face cannot be cut into triangles.
    pub fn of(m: &SurfaceModel, faces: impl Iterator<Item = FaceId>) -> Option<Soup> {
        let mut tris = Vec::new();
        for id in faces {
            let f = m.faces.get(&id)?;
            let t = geom::triangulate_face(id, f, |v| m.vertices[&v])?;
            for tri in &t.triangles {
                tris.push(Tri {
                    p: tri.map(|i| t.positions[i as usize]),
                    n: f.plane.normal,
                });
            }
        }
        Some(Soup::new(tris))
    }

    fn new(tris: Vec<Tri>) -> Soup {
        let items: Vec<(DVec3, DVec3, u32)> = tris
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let lo = t.p[0].min(t.p[1]).min(t.p[2]);
                let hi = t.p[0].max(t.p[1]).max(t.p[2]);
                (lo, hi, i as u32)
            })
            .collect();
        let (lo, hi) = items.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), (a, b, _)| (lo.min(*a), hi.max(*b)),
        );
        Soup {
            tris,
            tree: BoxTree::new(items),
            lo,
            hi,
        }
    }

    /// The box around the soup, with the plane tolerance added.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        (self.lo, self.hi)
    }

    /// Where the point is. `None` if no ray could be counted.
    pub fn locate(&self, p: DVec3) -> Option<Where> {
        let pt = geom::plane_tol(&[p, self.lo, self.hi]);
        let pad = DVec3::splat(2.0 * pt);
        let mut on: Option<(f64, DVec3)> = None;
        self.tree.query(p - pad, p + pad, &mut |i| {
            let t = &self.tris[i as usize];
            let d = distance_to_triangle(p, &t.p);
            if d <= pt && on.is_none_or(|(best, _)| d < best) {
                on = Some((d, t.n));
            }
        });
        if let Some((_, n)) = on {
            return Some(Where::On(n));
        }
        if p.cmplt(self.lo - pad).any() || p.cmpgt(self.hi + pad).any() {
            return Some(Where::Out);
        }
        for r in RAYS {
            let d = DVec3::from(r).normalize();
            let mut hits = 0usize;
            let mut unsure = false;
            let inv = d.recip();
            self.tree.query_by(
                &|lo, hi| ray_meets_box(p, inv, lo - pad, hi + pad),
                &mut |i| match ray_triangle(p, d, &self.tris[i as usize].p, pt) {
                    Hit::Miss => {}
                    Hit::Yes => hits += 1,
                    Hit::Unsure => unsure = true,
                },
            );
            if !unsure {
                return Some(if hits % 2 == 1 { Where::In } else { Where::Out });
            }
        }
        None
    }
}

/// The centroids of the triangles of a face: the points that stand for it.
pub(super) fn centroids(m: &SurfaceModel, id: FaceId) -> Option<Vec<DVec3>> {
    let f = m.faces.get(&id)?;
    let t = geom::triangulate_face(id, f, |v| m.vertices[&v])?;
    Some(
        t.triangles
            .iter()
            .map(|tri| tri.map(|i| t.positions[i as usize]))
            .map(|[a, b, c]| (a + b + c) / 3.0)
            .collect(),
    )
}

/// True if the ray from `o` along the direction with the reciprocal `inv` can meet the box.
fn ray_meets_box(o: DVec3, inv: DVec3, lo: DVec3, hi: DVec3) -> bool {
    let t1 = (lo - o) * inv;
    let t2 = (hi - o) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    tmax >= 0.0 && tmin <= tmax
}

/// Möller–Trumbore, with a margin: a hit near an edge or a vertex of the triangle is `Unsure`.
fn ray_triangle(o: DVec3, d: DVec3, tri: &[DVec3; 3], plane_tol: f64) -> Hit {
    const EDGE: f64 = 1e-9;
    let (e1, e2) = (tri[1] - tri[0], tri[2] - tri[0]);
    let pvec = d.cross(e2);
    let det = e1.dot(pvec);
    let scale = e1.length() * e2.length();
    if scale == 0.0 {
        return Hit::Miss;
    }
    if det.abs() <= 1e-12 * scale {
        // The ray is parallel to the plane: it matters only if it runs in the plane, near the
        // triangle.
        let n = e1.cross(e2).normalize_or_zero();
        let in_plane = n.dot(o - tri[0]).abs() <= plane_tol;
        return if in_plane && line_passes_triangle(o, d, tri) {
            Hit::Unsure
        } else {
            Hit::Miss
        };
    }
    let inv = 1.0 / det;
    let tvec = o - tri[0];
    let u = tvec.dot(pvec) * inv;
    let qvec = tvec.cross(e1);
    let v = d.dot(qvec) * inv;
    let t = e2.dot(qvec) * inv;
    if u < -EDGE || v < -EDGE || u + v > 1.0 + EDGE {
        return Hit::Miss;
    }
    if t < -plane_tol {
        return Hit::Miss;
    }
    if t < plane_tol || u < EDGE || v < EDGE || u + v > 1.0 - EDGE {
        return Hit::Unsure;
    }
    Hit::Yes
}

/// True if the line through `o` along `d` (in the plane of the triangle) meets the triangle.
fn line_passes_triangle(o: DVec3, d: DVec3, tri: &[DVec3; 3]) -> bool {
    let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
    let mut side = [0.0; 3];
    for i in 0..3 {
        side[i] = d.cross(tri[i] - o).dot(n);
    }
    let (lo, hi) = (
        side.iter().copied().fold(f64::INFINITY, f64::min),
        side.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    lo <= 0.0 && hi >= 0.0
}

/// The distance from a point to a triangle (Ericson, Real-Time Collision Detection).
pub(super) fn distance_to_triangle(p: DVec3, t: &[DVec3; 3]) -> f64 {
    let [a, b, c] = *t;
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return p.distance(a);
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return p.distance(b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return p.distance(a + ab * v);
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return p.distance(c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return p.distance(a + ac * w);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return p.distance(b + (c - b) * w);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    p.distance(a + ab * v + ac * w)
}
