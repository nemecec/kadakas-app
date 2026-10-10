//! The objects beyond the cut plane, as visible lines: silhouettes, creases and borders of the
//! meshes, with hidden parts removed.
//!
//! Positions are (u, v, w): u runs along the section to the right, v is the height, and w is
//! the distance from the plane in the direction of the view. Nothing in front of the plane
//! (w below zero) is drawn, and nothing hides what lies behind it.

use std::collections::HashMap;

use glam::{DVec2, DVec3};

use cadektor_core::model::ObjectId;

use super::{CutFace, Edge};

/// Edges meeting at less than this angle (the dot product of the normals is above its cosine) are smooth.
const CREASE_COS: f64 = 0.866_025_4;
/// Most edges in a drawing.
pub const MAX_EDGES: usize = 300_000;
/// Most pieces that one edge is split into for the hidden-line test.
const MAX_PIECES: usize = 64;

/// A triangle in section space.
#[derive(Clone, Debug)]
pub struct Tri {
    pub object: ObjectId,
    pub p: [DVec3; 3],
    /// The original vertices (bits of the scene position), to find the edges that triangles share.
    pub key: [[u32; 3]; 3],
    /// A plant: only its silhouette is drawn.
    pub plant: bool,
}

/// A line of the scene in section space.
#[derive(Clone, Debug)]
pub struct Seg {
    pub object: ObjectId,
    pub a: DVec3,
    pub b: DVec3,
}

/// The region that shows: `0 <= u <= len` and `0 <= w <= depth`.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub len: f64,
    pub depth: f64,
}

fn normal(t: &Tri) -> DVec3 {
    (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0])
}

/// Clips the segment `a`-`b` to the region. Returns the parameters of the part that shows.
fn clip(a: DVec3, b: DVec3, r: &Region) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = b - a;
    for (p, q) in [
        (-d.x, a.x),
        (d.x, r.len - a.x),
        (-d.z, a.z),
        (d.z, r.depth - a.z),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then_some((t0, t1))
}

/// A grid of triangles over the (u, v) plane, for the hidden-line test.
struct Grid<'a> {
    tris: Vec<&'a Tri>,
    lo: DVec2,
    cell: DVec2,
    n: usize,
    cells: Vec<Vec<u32>>,
}

impl<'a> Grid<'a> {
    fn new(tris: Vec<&'a Tri>) -> Grid<'a> {
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for t in &tris {
            for p in &t.p {
                lo = lo.min(DVec2::new(p.x, p.y));
                hi = hi.max(DVec2::new(p.x, p.y));
            }
        }
        if tris.is_empty() {
            lo = DVec2::ZERO;
            hi = DVec2::ONE;
        }
        let n = ((tris.len() as f64).sqrt() / 1.5).clamp(4.0, 200.0) as usize;
        let size = (hi - lo).max(DVec2::splat(1e-6));
        let cell = size / n as f64;
        let mut cells = vec![Vec::new(); n * n];
        for (i, t) in tris.iter().enumerate() {
            let (tl, th) = t.p.iter().fold(
                (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                |(l, h), p| (l.min(DVec2::new(p.x, p.y)), h.max(DVec2::new(p.x, p.y))),
            );
            let (c0, c1) = (
                (((tl - lo) / cell).floor().max(DVec2::ZERO)).min(DVec2::splat(n as f64 - 1.0)),
                (((th - lo) / cell).floor().max(DVec2::ZERO)).min(DVec2::splat(n as f64 - 1.0)),
            );
            for cy in c0.y as usize..=c1.y as usize {
                for cx in c0.x as usize..=c1.x as usize {
                    cells[cy * n + cx].push(i as u32);
                }
            }
        }
        Grid {
            tris,
            lo,
            cell,
            n,
            cells,
        }
    }

    /// True if a triangle in front of depth `w` covers the point (u, v).
    fn hides(&self, p: DVec2, w: f64, tol: f64) -> bool {
        let c = ((p - self.lo) / self.cell).floor();
        if c.x < 0.0 || c.y < 0.0 || c.x >= self.n as f64 || c.y >= self.n as f64 {
            return false;
        }
        for &i in &self.cells[c.y as usize * self.n + c.x as usize] {
            let t = self.tris[i as usize];
            let (a, b, c3) = (t.p[0], t.p[1], t.p[2]);
            let det = (b.x - a.x) * (c3.y - a.y) - (c3.x - a.x) * (b.y - a.y);
            if det.abs() < 1e-9 {
                continue;
            }
            let l1 = ((p.x - a.x) * (c3.y - a.y) - (c3.x - a.x) * (p.y - a.y)) / det;
            let l2 = ((b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)) / det;
            let l0 = 1.0 - l1 - l2;
            let e = -1e-9;
            if l0 < e || l1 < e || l2 < e {
                continue;
            }
            let wt = l0 * a.z + l1 * b.z + l2 * c3.z;
            if wt >= -tol && wt < w - tol {
                return true;
            }
        }
        false
    }
}

fn inside_ring(p: DVec2, ring: &[DVec2]) -> bool {
    let mut c = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

/// An edge of the meshes, with the triangles that share it.
struct Shared {
    a: DVec3,
    b: DVec3,
    object: ObjectId,
    plant: bool,
    /// The facing and the normal of up to two triangles.
    sides: Vec<(bool, DVec3)>,
}

/// The visible lines beyond the plane. `covers` are the cut faces that hide what is behind them.
/// `tol` is the distance in mm below which depths count as equal.
pub fn edges(
    tris: &[Tri],
    segs: &[Seg],
    region: &Region,
    covers: &[&CutFace],
    tol: f64,
) -> Vec<Edge> {
    // Only triangles that reach into the region matter.
    let near: Vec<&Tri> = tris
        .iter()
        .filter(|t| {
            let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            for p in &t.p {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            hi.z >= 0.0 && lo.z <= region.depth && hi.x >= 0.0 && lo.x <= region.len
        })
        .collect();
    // The edges of the meshes, keyed by their end points.
    let mut shared: HashMap<([u32; 3], [u32; 3]), usize> = HashMap::new();
    let mut list: Vec<Shared> = Vec::new();
    for t in &near {
        let n = normal(t);
        let front = n.z > 1e-9 * n.length();
        let n = n.normalize_or_zero();
        for k in 0..3 {
            let (i, j) = (k, (k + 1) % 3);
            let key = if t.key[i] <= t.key[j] {
                (t.key[i], t.key[j])
            } else {
                (t.key[j], t.key[i])
            };
            let slot = *shared.entry(key).or_insert_with(|| {
                list.push(Shared {
                    a: t.p[i],
                    b: t.p[j],
                    object: t.object,
                    plant: t.plant,
                    sides: Vec::new(),
                });
                list.len() - 1
            });
            list[slot].sides.push((front, n));
        }
    }
    let mut candidates: Vec<(ObjectId, DVec3, DVec3)> = Vec::new();
    for s in &list {
        let feature = match s.sides.as_slice() {
            [_] => true,
            [(f0, n0), (f1, n1)] => f0 != f1 || (!s.plant && n0.dot(*n1) < CREASE_COS),
            _ => true,
        };
        if feature {
            candidates.push((s.object, s.a, s.b));
        }
    }
    for s in segs {
        candidates.push((s.object, s.a, s.b));
    }
    // Triangles that can hide something: the near ones. Plants hide too.
    let grid = Grid::new(near);
    let mut out = Vec::new();
    let (mut vlo, mut vhi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (_, a, b) in &candidates {
        vlo = vlo.min(a.y).min(b.y);
        vhi = vhi.max(a.y).max(b.y);
    }
    // Pieces of about a hundredth of the drawing.
    let step = region.len.max(vhi - vlo).max(1.0) / 150.0;
    for (object, a, b) in candidates {
        let Some((t0, t1)) = clip(a, b, region) else {
            continue;
        };
        let (p, q) = (a.lerp(b, t0), a.lerp(b, t1));
        let len2 = DVec2::new(q.x - p.x, q.y - p.y).length();
        if len2 < 1e-6 {
            continue;
        }
        let pieces = ((len2 / step).ceil() as usize).clamp(1, MAX_PIECES);
        let mut start: Option<DVec3> = None;
        for k in 0..pieces {
            let (f0, f1) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
            let (s0, s1) = (p.lerp(q, f0), p.lerp(q, f1));
            let mid = p.lerp(q, (f0 + f1) / 2.0);
            let pt = DVec2::new(mid.x, mid.y);
            let hidden = grid.hides(pt, mid.z, tol)
                || covers.iter().any(|f| {
                    f.rings
                        .iter()
                        .filter(|r| r.len() >= 3)
                        .fold(false, |acc, r| acc ^ inside_ring(pt, r))
                });
            match (hidden, start) {
                (false, None) => start = Some(s0),
                (true, Some(s)) => {
                    push(&mut out, object, s, s0);
                    start = None;
                }
                _ => {}
            }
            if k + 1 == pieces
                && let Some(s) = start
            {
                push(&mut out, object, s, s1);
            }
        }
        if out.len() >= MAX_EDGES {
            break;
        }
    }
    out
}

fn push(out: &mut Vec<Edge>, object: ObjectId, a: DVec3, b: DVec3) {
    out.push(Edge {
        object,
        a: DVec2::new(a.x, a.y),
        b: DVec2::new(b.x, b.y),
        depth: (a.z + b.z) / 2.0,
    });
}
