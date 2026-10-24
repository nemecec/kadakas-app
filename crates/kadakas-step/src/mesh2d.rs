//! The triangles of one face. The boundary points are fixed (the neighbouring faces use the
//! same ones). The region in the parameters of the surface (UV) is cut by ear clipping, with
//! the holes joined to the outline by bridges. Then triangles that are too coarse for the
//! surface are split at an edge or at the middle, and edges are flipped to keep the triangles
//! round. Boundary edges are never split, so the faces fit together.

use std::collections::HashMap;

use glam::DVec3;

use crate::surface::Surface;

/// The points of a face: the parameters and the position of each.
#[derive(Clone, Debug, Default)]
pub struct Points {
    pub uv: Vec<[f64; 2]>,
    pub xyz: Vec<DVec3>,
    /// Lengths in space per unit of u and of v on average: the metric of the chart. A zero
    /// means one.
    pub scale: [f64; 2],
}

impl Points {
    /// The position in a plane where lengths are about those in space.
    pub fn flat(&self, i: u32) -> [f64; 2] {
        let s = [
            if self.scale[0] > 0.0 { self.scale[0] } else { 1.0 },
            if self.scale[1] > 0.0 { self.scale[1] } else { 1.0 },
        ];
        let p = self.uv[i as usize];
        [p[0] * s[0], p[1] * s[1]]
    }
}

/// The most polygon points that ear clipping takes for one face.
pub const MAX_POLYGON_POINTS: usize = 400_000;

fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// The signed area of a loop (positive when it runs counterclockwise).
pub fn area(uv: &[[f64; 2]], l: &[usize]) -> f64 {
    let n = l.len();
    (0..n)
        .map(|i| {
            let (a, b) = (uv[l[i]], uv[l[(i + 1) % n]]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

/// Triangles for the outline `loops[0]` (counterclockwise) and the holes `loops[1..]`
/// (clockwise), as indices of `uv`. `None` when the polygon cannot be cut.
pub fn ear_clip(uv: &[[f64; 2]], xyz: &[DVec3], scale: [f64; 2], loops: &[Vec<usize>]) -> Option<Vec<[u32; 3]>> {
    let mut poly: Vec<usize> = loops.first()?.clone();
    let total: usize = loops.iter().map(Vec::len).sum();
    if poly.len() < 3 || total > MAX_POLYGON_POINTS {
        return None;
    }
    // Holes, the one with the largest x first.
    let mut holes: Vec<&Vec<usize>> = loops[1..].iter().filter(|h| h.len() >= 3).collect();
    let max_x = |h: &Vec<usize>| h.iter().map(|i| uv[*i][0]).fold(f64::NEG_INFINITY, f64::max);
    holes.sort_by(|a, b| max_x(b).total_cmp(&max_x(a)));
    for h in holes {
        poly = bridge(uv, poly, h)?;
    }
    clip(uv, xyz, scale, &poly)
}

/// Joins a hole to the polygon by two coincident edges between the rightmost point of the
/// hole and a point of the polygon that it can see.
fn bridge(uv: &[[f64; 2]], poly: Vec<usize>, hole: &[usize]) -> Option<Vec<usize>> {
    let (mi, _) = hole
        .iter()
        .enumerate()
        .max_by(|a, b| uv[*a.1][0].total_cmp(&uv[*b.1][0]))?;
    let m = uv[hole[mi]];
    // The edge that the ray to the right hits first.
    let n = poly.len();
    let mut best: Option<(f64, usize)> = None;
    for i in 0..n {
        let (a, b) = (uv[poly[i]], uv[poly[(i + 1) % n]]);
        if (a[1] - m[1]) * (b[1] - m[1]) > 0.0 || a[1] == b[1] {
            continue;
        }
        let x = a[0] + (m[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
        if x >= m[0] - 1e-12 && best.is_none_or(|(bx, _)| x < bx) {
            best = Some((x, i));
        }
    }
    let (ix, ei) = best?;
    let (a, b) = (poly[ei], poly[(ei + 1) % n]);
    let p_idx = if uv[a][0] > uv[b][0] { ei } else { (ei + 1) % n };
    let ipt = [ix, m[1]];
    let ppt = uv[poly[p_idx]];
    let mut vi = p_idx;
    // A point inside the triangle (M, I, P) that blocks the view: take the one with the
    // smallest angle to the ray.
    {
        let mut best_ang = f64::INFINITY;
        for (k, &q) in poly.iter().enumerate() {
            let t = uv[q];
            if k == p_idx || !in_triangle(m, ipt, ppt, t) {
                continue;
            }
            let ang = (t[1] - m[1]).abs().atan2(t[0] - m[0]);
            let closer = ang < best_ang - 1e-12
                || ((ang - best_ang).abs() <= 1e-12
                    && dist2(m, t) < dist2(m, uv[poly[vi]]));
            if closer {
                best_ang = ang;
                vi = k;
            }
        }
    }
    let mut out = Vec::with_capacity(poly.len() + hole.len() + 2);
    out.extend_from_slice(&poly[..=vi]);
    for k in 0..=hole.len() {
        out.push(hole[(mi + k) % hole.len()]);
    }
    out.push(poly[vi]);
    out.extend_from_slice(&poly[vi + 1..]);
    Some(out)
}

fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

fn in_triangle(a: [f64; 2], b: [f64; 2], c: [f64; 2], p: [f64; 2]) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Ear clipping of a counterclockwise polygon (repeated indices allowed, for bridges). The ear
/// with the shortest longest edge goes first, which gives short diagonals and round triangles.
fn clip(uv: &[[f64; 2]], xyz: &[DVec3], scale: [f64; 2], poly: &[usize]) -> Option<Vec<[u32; 3]>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = poly.len();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut alive = vec![true; n];
    let mut stamp = vec![0u32; n];
    let pt = |i: usize| uv[poly[i]];
    let convex = |p: usize, i: usize, q: usize| cross(pt(p), pt(i), pt(q)) > 0.0;
    // The reflex points, in a grid.
    let reflex0: Vec<usize> = (0..n).filter(|&i| !convex(prev[i], i, next[i])).collect();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &i in &reflex0 {
        for k in 0..2 {
            lo[k] = lo[k].min(pt(i)[k]);
            hi[k] = hi[k].max(pt(i)[k]);
        }
    }
    let cells = ((reflex0.len() as f64 / 2.0).sqrt().ceil() as usize).clamp(1, 512);
    let size = [
        ((hi[0] - lo[0]) / cells as f64).max(1e-300),
        ((hi[1] - lo[1]) / cells as f64).max(1e-300),
    ];
    let cell_of = |p: [f64; 2]| -> (usize, usize) {
        (
            (((p[0] - lo[0]) / size[0]).floor().max(0.0) as usize).min(cells - 1),
            (((p[1] - lo[1]) / size[1]).floor().max(0.0) as usize).min(cells - 1),
        )
    };
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); cells * cells];
    for &i in &reflex0 {
        let (cx, cy) = cell_of(pt(i));
        grid[cy * cells + cx].push(i);
    }
    let inside_any = |a: usize, b: usize, c: usize, prev: &[usize], next: &[usize], alive: &[bool]| -> bool {
        let (pa, pb, pc) = (pt(a), pt(b), pt(c));
        let (x0, x1) = (pa[0].min(pb[0]).min(pc[0]), pa[0].max(pb[0]).max(pc[0]));
        let (y0, y1) = (pa[1].min(pb[1]).min(pc[1]), pa[1].max(pb[1]).max(pc[1]));
        if reflex0.is_empty() || x1 < lo[0] || x0 > hi[0] || y1 < lo[1] || y0 > hi[1] {
            return false;
        }
        let (cx0, cy0) = cell_of([x0, y0]);
        let (cx1, cy1) = cell_of([x1, y1]);
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                for &r in &grid[cy * cells + cx] {
                    if r == a || r == b || r == c || !alive[r] {
                        continue;
                    }
                    let q = pt(r);
                    // The same position as a corner: a bridge copy, not a blocker.
                    if poly[r] == poly[a] || poly[r] == poly[b] || poly[r] == poly[c] {
                        continue;
                    }
                    if q == pa || q == pb || q == pc {
                        continue;
                    }
                    // Still reflex now?
                    if convex(prev[r], r, next[r]) {
                        continue;
                    }
                    if in_triangle(pa, pb, pc, q) {
                        return true;
                    }
                }
            }
        }
        false
    };
    let score = |p: usize, i: usize, q: usize| -> f64 {
        let sc = [
            if scale[0] > 0.0 { scale[0] } else { 1.0 },
            if scale[1] > 0.0 { scale[1] } else { 1.0 },
        ];
        let f = |k: usize| [uv[poly[k]][0] * sc[0], uv[poly[k]][1] * sc[1]];
        let (a, b, c) = (f(p), f(i), f(q));
        let s = dist2(a, b).max(dist2(b, c)).max(dist2(c, a));
        let _ = xyz;
        if s.is_finite() { s } else { f64::MAX }
    };
    let flat = |p: usize, i: usize, q: usize| {
        cross(pt(p), pt(i), pt(q)).abs() <= 1e-18 * (1.0 + pt(i)[0].abs() + pt(i)[1].abs())
    };
    let mut out: Vec<[u32; 3]> = Vec::with_capacity(n);
    let mut left = n;
    // Mode 0: real ears only. Mode 1: flat ears too (a point on a straight line becomes a
    // triangle with no area, which the flips repair). Mode 2: any convex ear.
    let mut mode = 0u8;
    while left > 3 {
        let mut heap: BinaryHeap<Reverse<(u64, usize, u32)>> = BinaryHeap::new();
        let first = (0..n).find(|i| alive[*i]).unwrap_or(0);
        let mut v = first;
        for _ in 0..left {
            let (p, q) = (prev[v], next[v]);
            let is_flat = flat(p, v, q);
            if (is_flat && mode >= 1) || (!is_flat && convex(p, v, q)) {
                heap.push(Reverse((score(p, v, q).to_bits(), v, stamp[v])));
            }
            v = next[v];
        }
        let mut progress = false;
        while let Some(Reverse((_, v, st))) = heap.pop() {
            if !alive[v] || stamp[v] != st || left <= 3 {
                continue;
            }
            let (p, q) = (prev[v], next[v]);
            let is_flat = flat(p, v, q);
            let ok = if is_flat {
                mode >= 1
            } else {
                convex(p, v, q) && (mode >= 2 || !inside_any(p, v, q, &prev, &next, &alive))
            };
            if !ok {
                continue;
            }
            // A bridge seen from both sides collapses: no triangle.
            if poly[p] != poly[q] && poly[p] != poly[v] && poly[v] != poly[q] {
                out.push([poly[p] as u32, poly[v] as u32, poly[q] as u32]);
            }
            alive[v] = false;
            next[p] = q;
            prev[q] = p;
            left -= 1;
            progress = true;
            stamp[p] += 1;
            stamp[q] += 1;
            for w in [p, q] {
                let (a, c) = (prev[w], next[w]);
                let is_flat = flat(a, w, c);
                if (is_flat && mode >= 1) || (!is_flat && convex(a, w, c)) {
                    heap.push(Reverse((score(a, w, c).to_bits(), w, stamp[w])));
                }
            }
        }
        if left <= 3 {
            break;
        }
        if progress {
            mode = 0;
        } else {
            mode += 1;
            if mode > 2 {
                return None;
            }
        }
    }
    // The last three points.
    let v = (0..n).find(|i| alive[*i])?;
    let (p, q) = (prev[v], next[v]);
    if left == 3
        && cross(pt(p), pt(v), pt(q)) >= 0.0
        && poly[p] != poly[q]
        && poly[p] != poly[v]
        && poly[v] != poly[q]
    {
        out.push([poly[p] as u32, poly[v] as u32, poly[q] as u32]);
    }
    // The pieces must cover the polygon: compare the areas.
    let want = polygon_area(uv, poly);
    let got: f64 = out
        .iter()
        .map(|t| cross(uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]) / 2.0)
        .sum();
    ((want - got).abs() <= 1e-6 * want.abs().max(1e-300)).then_some(out)
}

fn polygon_area(uv: &[[f64; 2]], poly: &[usize]) -> f64 {
    area(uv, poly)
}

/// The triangles of a mesh with the neighbours of each edge.
struct Topo {
    tris: Vec<[u32; 3]>,
    alive: Vec<bool>,
    emap: HashMap<(u32, u32), u32>,
    live: usize,
}

impl Topo {
    fn new(tris: Vec<[u32; 3]>) -> Topo {
        let mut t = Topo {
            tris: Vec::with_capacity(tris.len()),
            alive: Vec::new(),
            emap: HashMap::with_capacity(tris.len() * 3),
            live: 0,
        };
        for tri in tris {
            t.add(tri);
        }
        t
    }

    fn add(&mut self, tri: [u32; 3]) -> u32 {
        let id = self.tris.len() as u32;
        self.tris.push(tri);
        self.alive.push(true);
        self.live += 1;
        for k in 0..3 {
            self.emap.insert((tri[k], tri[(k + 1) % 3]), id);
        }
        id
    }

    fn remove(&mut self, id: u32) {
        let tri = self.tris[id as usize];
        for k in 0..3 {
            let key = (tri[k], tri[(k + 1) % 3]);
            if self.emap.get(&key) == Some(&id) {
                self.emap.remove(&key);
            }
        }
        self.alive[id as usize] = false;
        self.live -= 1;
    }

    /// The triangle on the other side of the edge a->b, and its third point.
    fn across(&self, a: u32, b: u32) -> Option<(u32, u32)> {
        let t = *self.emap.get(&(b, a))?;
        let tri = self.tris[t as usize];
        let third = tri.iter().copied().find(|v| *v != a && *v != b)?;
        Some((t, third))
    }
}

fn angle_at(pts: &Points, a: u32, b: u32, c: u32) -> f64 {
    let (pa, pb, pc) = (pts.flat(a), pts.flat(b), pts.flat(c));
    let (u, v) = ([pa[0] - pb[0], pa[1] - pb[1]], [pc[0] - pb[0], pc[1] - pb[1]]);
    let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
    if lu == 0.0 || lv == 0.0 {
        return 0.0;
    }
    ((u[0] * v[0] + u[1] * v[1]) / (lu * lv)).clamp(-1.0, 1.0).acos()
}

/// What the refinement needs to know.
pub struct Refine<'s> {
    pub surface: &'s Surface,
    pub tol: f64,
    pub angle: f64,
    pub max_triangles: usize,
}

/// Cuts the face: the triangles of the polygon, then refined. Returns the triangles and
/// whether the refinement stopped at the limit.
pub fn mesh_face(
    pts: &mut Points,
    loops: &[Vec<usize>],
    refine: &Refine,
) -> Option<(Vec<[u32; 3]>, bool)> {
    let tris = ear_clip(&pts.uv, &pts.xyz, pts.scale, loops)?;
    let mut topo = Topo::new(tris);
    remove_flat(&mut topo, pts);
    // First a pass of flips on every edge.
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for t in &topo.tris {
        for k in 0..3 {
            edges.push((t[k], t[(k + 1) % 3]));
        }
    }
    edges.reverse();
    let _ = legalize(&mut topo, pts, edges, refine.surface);
    let mut truncated = false;
    let curved = !refine.surface.is_flat();
    if curved {
        seed_grid(&mut topo, pts, loops, refine);
        let mut stack: Vec<u32> = (0..topo.tris.len() as u32).filter(|t| topo.alive[*t as usize]).collect();
        let mut guard = 0usize;
        let mut seen: HashMap<[u32; 3], u8> = HashMap::new();
        while let Some(t) = stack.pop() {
            guard += 1;
            if guard > refine.max_triangles.saturating_mul(8) {
                truncated = true;
                break;
            }
            if !topo.alive[t as usize] {
                continue;
            }
            if topo.live >= refine.max_triangles {
                truncated = true;
                break;
            }
            // A triangle with short edges is as flat as it gets.
            let tri = topo.tris[t as usize];
            let longest = (0..3)
                .map(|k| pts.xyz[tri[k] as usize].distance(pts.xyz[tri[(k + 1) % 3] as usize]))
                .fold(0.0, f64::max);
            if longest < refine.tol * 0.25 || !needs_split(&topo, pts, t, refine) {
                continue;
            }
            let before_tri = topo.tris[t as usize];
            // A triangle that comes back again and again is in a loop with its neighbour.
            let mut key = before_tri;
            key.sort_unstable();
            let visits = seen.entry(key).or_insert(0u8);
            *visits = visits.saturating_add(1);
            if *visits > 6 {
                continue;
            }
            let new = split(&mut topo, pts, t, refine.surface);
            stack.extend(new);
        }
    }
    let tris = topo
        .tris
        .iter()
        .zip(&topo.alive)
        .filter(|(_, a)| **a)
        .map(|(t, _)| *t)
        .collect();
    Some((tris, truncated))
}

/// A triangle with no area whose middle point lies on the long side is joined to the triangle
/// across that side: the two become two triangles that use the middle point.
fn remove_flat(topo: &mut Topo, pts: &Points) {
    for _ in 0..8 {
        let mut changed = false;
        for id in 0..topo.tris.len() as u32 {
            if !topo.alive[id as usize] {
                continue;
            }
            let t = topo.tris[id as usize];
            for k in 0..3 {
                let (p, m, q) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                let (up, um, uq) = (pts.uv[p as usize], pts.uv[m as usize], pts.uv[q as usize]);
                let long2 = dist2(up, uq);
                let between = (up[0] - um[0]) * (uq[0] - um[0]) + (up[1] - um[1]) * (uq[1] - um[1]) < 0.0;
                if !(between && cross(up, um, uq).abs() <= 1e-9 * long2) {
                    continue;
                }
                let Some((n, x)) = topo.across(q, p) else {
                    continue;
                };
                let ux = pts.uv[x as usize];
                if cross(up, um, ux) <= 0.0 || cross(um, uq, ux) <= 0.0 {
                    continue;
                }
                topo.remove(id);
                topo.remove(n);
                topo.add([p, m, x]);
                topo.add([m, q, x]);
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Points in the region on a grid in the parameters, so that no triangle is a long needle
/// before the refinement starts. The grid steps are those of the boundary: the median
/// distance between neighbouring boundary points in each direction.
fn seed_grid(topo: &mut Topo, pts: &mut Points, loops: &[Vec<usize>], r: &Refine) {
    let outline = &loops[0];
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &i in outline {
        for k in 0..2 {
            lo[k] = lo[k].min(pts.uv[i][k]);
            hi[k] = hi[k].max(pts.uv[i][k]);
        }
    }
    let mut steps = [Vec::new(), Vec::new()];
    for l in loops {
        for k in 0..l.len() {
            let (a, b) = (pts.uv[l[k]], pts.uv[l[(k + 1) % l.len()]]);
            for d in 0..2 {
                let x = (a[d] - b[d]).abs();
                if x > 1e-9 * (hi[d] - lo[d]).max(1e-300) {
                    steps[d].push(x);
                }
            }
        }
    }
    let mut step = [0.0; 2];
    for d in 0..2 {
        let extent = hi[d] - lo[d];
        steps[d].sort_by(f64::total_cmp);
        let med = steps[d].get(steps[d].len() / 2).copied().unwrap_or(extent);
        step[d] = med.max(extent / 200.0);
    }
    let n = [
        (((hi[0] - lo[0]) / step[0]).ceil() as usize).max(1),
        (((hi[1] - lo[1]) / step[1]).ceil() as usize).max(1),
    ];
    if n[0] * n[1] > 60_000 || n[0] < 1 || n[1] < 1 {
        return;
    }
    // An even-odd test against all loops.
    let inside = |p: [f64; 2], pts: &Points| -> bool {
        let mut c = false;
        for l in loops {
            let m = l.len();
            for k in 0..m {
                let (a, b) = (pts.uv[l[k]], pts.uv[l[(k + 1) % m]]);
                if (a[1] > p[1]) != (b[1] > p[1])
                    && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
                {
                    c = !c;
                }
            }
        }
        c
    };
    let mut last = 0u32;
    // Rows from the bottom, shifted by an irrational part of a cell, so that no point sits
    // exactly on an edge of the polygon.
    for j in 0..n[1] {
        for i in 0..n[0] {
            if topo.live >= r.max_triangles {
                return;
            }
            let p = [
                lo[0] + step[0] * (i as f64 + 0.5 + 0.0618),
                lo[1] + step[1] * (j as f64 + 0.5 + 0.0382),
            ];
            if p[0] >= hi[0] || p[1] >= hi[1] || !inside(p, pts) {
                continue;
            }
            let Some(t) = locate(topo, pts, p, &mut last) else {
                continue;
            };
            let tri = topo.tris[t as usize];
            let xyz = r.surface.eval(p[0], p[1]);
            if !xyz.is_finite() {
                continue;
            }
            // Not too near a corner or an edge of the triangle.
            let u = tri.map(|k| pts.uv[k as usize]);
            let area2 = cross(u[0], u[1], u[2]);
            let w = [
                cross(p, u[1], u[2]) / area2,
                cross(u[0], p, u[2]) / area2,
                cross(u[0], u[1], p) / area2,
            ];
            if !(area2 > 0.0) || w.iter().any(|x| !x.is_finite() || *x < 1e-6) {
                continue;
            }
            pts.uv.push(p);
            pts.xyz.push(xyz);
            let m = (pts.uv.len() - 1) as u32;
            topo.remove(t);
            let nt = [topo.add([tri[0], tri[1], m]), topo.add([tri[1], tri[2], m]), topo.add([tri[2], tri[0], m])];
            last = nt[0];
            let _ = legalize(topo, pts, vec![(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])], r.surface);
            if !topo.alive[last as usize] {
                last = topo.alive.iter().rposition(|a| *a).unwrap_or(0) as u32;
            }
        }
    }
}

/// The triangle that holds the point: a walk from `start`, else a scan.
fn locate(topo: &Topo, pts: &Points, p: [f64; 2], start: &mut u32) -> Option<u32> {
    let holds = |t: u32| -> bool {
        let tri = topo.tris[t as usize];
        let u = tri.map(|k| pts.uv[k as usize]);
        cross(u[0], u[1], p) >= 0.0 && cross(u[1], u[2], p) >= 0.0 && cross(u[2], u[0], p) >= 0.0
    };
    let mut t = if topo.alive.get(*start as usize) == Some(&true) {
        *start
    } else {
        topo.alive.iter().position(|a| *a)? as u32
    };
    for _ in 0..(topo.tris.len() + 8).min(100_000) {
        let tri = topo.tris[t as usize];
        let u = tri.map(|k| pts.uv[k as usize]);
        let mut moved = false;
        for k in 0..3 {
            let (a, b) = (k, (k + 1) % 3);
            if cross(u[a], u[b], p) < 0.0 {
                match topo.across(tri[a], tri[b]) {
                    Some((n, _)) => {
                        t = n;
                        moved = true;
                        break;
                    }
                    None => break,
                }
            }
        }
        if !moved {
            if holds(t) {
                *start = t;
                return Some(t);
            }
            break;
        }
    }
    // The walk was blocked by the boundary: look at every triangle.
    let found = (0..topo.tris.len() as u32).find(|t| topo.alive[*t as usize] && holds(*t))?;
    *start = found;
    Some(found)
}

fn needs_split(topo: &Topo, pts: &Points, t: u32, r: &Refine) -> bool {
    let tri = topo.tris[t as usize];
    let uv = tri.map(|i| pts.uv[i as usize]);
    let xyz = tri.map(|i| pts.xyz[i as usize]);
    // A triangle with two corners at the same point (a pole) has no area: it is left out of
    // the mesh, so it needs no care.
    let scale = 1e-12 * (1.0 + xyz[0].length());
    if (0..3).any(|k| xyz[k].distance(xyz[(k + 1) % 3]) <= scale) {
        return false;
    }
    // A triangle of three points on a line is not refined: its neighbour is.
    let longest_uv = (0..3).map(|k| dist2(uv[k], uv[(k + 1) % 3])).fold(0.0, f64::max);
    if cross(uv[0], uv[1], uv[2]).abs() <= 1e-13 * longest_uv {
        return false;
    }
    // The turn of the normal over the triangle.
    let ns: Vec<Option<DVec3>> = uv.iter().map(|p| r.surface.normal(p[0], p[1])).collect();
    for i in 0..3 {
        for j in i + 1..3 {
            if let (Some(a), Some(b)) = (ns[i], ns[j])
                && a.dot(b).clamp(-1.0, 1.0).acos() > r.angle
            {
                return true;
            }
        }
    }
    // The distance of the surface from the flat triangle at some points inside.
    let weights: [[f64; 3]; 7] = [
        [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
        [0.5, 0.5, 0.0],
        [0.0, 0.5, 0.5],
        [0.5, 0.0, 0.5],
        [0.6, 0.2, 0.2],
        [0.2, 0.6, 0.2],
        [0.2, 0.2, 0.6],
    ];
    for (k, w) in weights.iter().enumerate() {
        // The middle of a boundary edge is part of the edge, not of the surface inside.
        if (1..4).contains(&k) {
            let (a, b) = match k {
                1 => (0, 1),
                2 => (1, 2),
                _ => (2, 0),
            };
            if topo.across(tri[a], tri[b]).is_none() {
                continue;
            }
        }
        let u = w[0] * uv[0][0] + w[1] * uv[1][0] + w[2] * uv[2][0];
        let v = w[0] * uv[0][1] + w[1] * uv[1][1] + w[2] * uv[2][1];
        let flat = xyz[0] * w[0] + xyz[1] * w[1] + xyz[2] * w[2];
        let on = r.surface.eval(u, v);
        if !on.is_finite() {
            continue;
        }
        if on.distance(flat) > r.tol {
            return true;
        }
    }
    false
}

/// Splits triangle `t`. Returns the triangles to look at again.
fn split(topo: &mut Topo, pts: &mut Points, t: u32, surface: &Surface) -> Vec<u32> {
    let tri = topo.tris[t as usize];
    let len = |a: u32, b: u32| {
        let (p, q) = (pts.flat(a), pts.flat(b));
        (p[0] - q[0]).hypot(p[1] - q[1])
    };
    let edges = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])];
    let longest = edges.iter().map(|e| len(e.0, e.1)).fold(0.0, f64::max);
    let inner = edges
        .iter()
        .filter(|e| topo.across(e.0, e.1).is_some())
        .max_by(|a, b| len(a.0, a.1).total_cmp(&len(b.0, b.1)))
        .copied();
    let new_point = |pts: &mut Points, uv: [f64; 2]| -> Option<u32> {
        let p = surface.eval(uv[0], uv[1]);
        if !p.is_finite() {
            return None;
        }
        pts.uv.push(uv);
        pts.xyz.push(p);
        Some((pts.uv.len() - 1) as u32)
    };
    match inner {
        Some((a, b)) if len(a, b) >= 0.5 * longest => {
            let Some((t2, d)) = topo.across(a, b) else {
                return Vec::new();
            };
            let c = tri.iter().copied().find(|v| *v != a && *v != b).unwrap_or(a);
            let (ua, ub) = (pts.uv[a as usize], pts.uv[b as usize]);
            // A corner that lies on the edge (a flat neighbour) is the split point.
            let on_edge = |p: u32| {
                let up = pts.uv[p as usize];
                let l2 = dist2(ua, ub);
                cross(ua, ub, up).abs() <= 1e-9 * l2
                    && (up[0] - ua[0]) * (ub[0] - ua[0]) + (up[1] - ua[1]) * (ub[1] - ua[1]) > 1e-6 * l2
                    && (up[0] - ub[0]) * (ua[0] - ub[0]) + (up[1] - ub[1]) * (ua[1] - ub[1]) > 1e-6 * l2
            };
            let m = if on_edge(d) {
                d
            } else if on_edge(c) {
                c
            } else {
                let Some(m) = new_point(pts, [(ua[0] + ub[0]) / 2.0, (ua[1] + ub[1]) / 2.0]) else {
                    return Vec::new();
                };
                m
            };
            if m == c || m == d {
                // The flat triangle goes, the other is cut at the corner of the flat one.
                let (flat, other, o) = if m == d { (t2, t, c) } else { (t, t2, d) };
                let _ = (flat, other);
                topo.remove(t);
                topo.remove(t2);
                let r = if m == d {
                    [topo.add([a, m, o]), topo.add([m, b, o])]
                } else {
                    [topo.add([b, m, o]), topo.add([m, a, o])]
                };
                let made = legalize(topo, pts, vec![(a, o), (o, b), (b, o), (o, a)], surface);
                return r.into_iter().chain(made).filter(|t| topo.alive[*t as usize]).collect();
            }
            topo.remove(t);
            topo.remove(t2);
            let new = [
                topo.add([a, m, c]),
                topo.add([m, b, c]),
                topo.add([b, m, d]),
                topo.add([m, a, d]),
            ];
            let edges = vec![(a, c), (c, b), (b, d), (d, a)];
            let made = legalize(topo, pts, edges, surface);
            new.into_iter().chain(made).filter(|t| topo.alive[*t as usize]).collect()
        }
        _ => {
            let uv = tri.map(|i| pts.uv[i as usize]);
            let Some(m) = new_point(
                pts,
                [
                    (uv[0][0] + uv[1][0] + uv[2][0]) / 3.0,
                    (uv[0][1] + uv[1][1] + uv[2][1]) / 3.0,
                ],
            ) else {
                return Vec::new();
            };
            topo.remove(t);
            let new = [
                topo.add([tri[0], tri[1], m]),
                topo.add([tri[1], tri[2], m]),
                topo.add([tri[2], tri[0], m]),
            ];
            let edges = vec![(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])];
            let made = legalize(topo, pts, edges, surface);
            new.into_iter().chain(made).filter(|t| topo.alive[*t as usize]).collect()
        }
    }
}

/// Flips edges where it makes the triangles better, as far as the two triangles make a
/// convex quadrilateral in UV. On a plane the better pair has the larger smallest angle
/// (Delaunay: opposite angles add up to less than a half turn). On a curved surface the
/// better pair leans less from the surface.
fn legalize(topo: &mut Topo, pts: &Points, mut stack: Vec<(u32, u32)>, _surface: &Surface) -> Vec<u32> {
    let mut made: Vec<u32> = Vec::new();
    let mut budget = 40 * stack.len() + 1000;
    while let Some((a, b)) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        // The edge may have been changed since it was put on the stack.
        let Some(&t1) = topo.emap.get(&(a, b)) else {
            continue;
        };
        let Some((t2, d)) = topo.across(a, b) else {
            continue;
        };
        let tri = topo.tris[t1 as usize];
        let c = tri.iter().copied().find(|v| *v != a && *v != b).unwrap_or(a);
        if c == d {
            continue;
        }
        let uv = |i: u32| pts.uv[i as usize];
        // The new triangles (a, d, c) and (d, b, c) must turn left, with some room.
        let min_area = 1e-12 * (1.0 + uv(a)[0].abs() + uv(a)[1].abs()).powi(2);
        if cross(uv(a), uv(d), uv(c)) <= min_area || cross(uv(d), uv(b), uv(c)) <= min_area {
            continue;
        }
        let sum = angle_at(pts, a, c, b) + angle_at(pts, a, d, b);
        let better = sum > std::f64::consts::PI + 1e-9;
        if !better {
            continue;
        }
        topo.remove(t1);
        topo.remove(t2);
        made.push(topo.add([a, d, c]));
        made.push(topo.add([d, b, c]));
        stack.extend([(a, d), (d, b), (b, c), (c, a)]);
    }
    made.retain(|t| topo.alive[*t as usize]);
    made
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DAffine3;

    fn flat3(uv: &[[f64; 2]]) -> Vec<DVec3> {
        uv.iter().map(|p| DVec3::new(p[0], p[1], 0.0)).collect()
    }

    fn loop_pts(pts: &[[f64; 2]]) -> (Vec<[f64; 2]>, Vec<usize>) {
        (pts.to_vec(), (0..pts.len()).collect())
    }

    fn area_of(uv: &[[f64; 2]], tris: &[[u32; 3]]) -> f64 {
        tris.iter()
            .map(|t| cross(uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]) / 2.0)
            .sum()
    }

    #[test]
    fn square_and_concave_polygons() {
        let (uv, l) = loop_pts(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]);
        let t = ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &[l]).unwrap();
        assert_eq!(t.len(), 2);
        assert!((area_of(&uv, &t) - 16.0).abs() < 1e-9);
        // An L shape, and a point on a straight line.
        let (uv, l) = loop_pts(&[
            [0.0, 0.0],
            [2.0, 0.0],
            [4.0, 0.0],
            [4.0, 2.0],
            [2.0, 2.0],
            [2.0, 4.0],
            [0.0, 4.0],
        ]);
        let t = ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &[l]).unwrap();
        assert!((area_of(&uv, &t) - 12.0).abs() < 1e-9);
        assert!(ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &[]).is_none());
        assert!(ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &[vec![0, 1]]).is_none());
    }

    #[test]
    fn holes_are_bridged() {
        let mut uv = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let outer: Vec<usize> = (0..4).collect();
        let mut loops = vec![outer];
        // Two square holes, clockwise, and a triangle hole.
        for (cx, cy, s) in [(3.0, 3.0, 1.0), (7.0, 6.0, 1.5), (2.0, 7.0, 0.8)] {
            let base = uv.len();
            uv.extend([
                [cx - s, cy - s],
                [cx - s, cy + s],
                [cx + s, cy + s],
                [cx + s, cy - s],
            ]);
            loops.push((base..base + 4).collect());
        }
        let t = ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &loops).unwrap();
        let want = 100.0 - 4.0 - 9.0 - 2.56;
        assert!((area_of(&uv, &t) - want).abs() < 1e-9, "{}", area_of(&uv, &t));
        // A hole in the middle of the way of the ray of another one.
        let uv2 = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [1.0, 4.0],
            [1.0, 6.0],
            [3.0, 6.0],
            [3.0, 4.0],
            [5.0, 4.0],
            [5.0, 6.0],
            [7.0, 6.0],
            [7.0, 4.0],
        ];
        let l2 = vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![8, 9, 10, 11]];
        let t2 = ear_clip(&uv2, &flat3(&uv2), [1.0, 1.0], &l2).unwrap();
        assert!((area_of(&uv2, &t2) - 92.0).abs() < 1e-9);
        // A hole that is not inside: no bridge.
        let uv3 = vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [5.0, 0.5],
            [5.0, 1.0],
            [6.0, 1.0],
            [6.0, 0.5],
        ];
        assert!(ear_clip(&uv3, &flat3(&uv3), [1.0, 1.0], &[vec![0, 1, 2, 3], vec![4, 5, 6, 7]]).is_none());
    }

    #[test]
    fn cylinder_strip_is_refined() {
        // A cylinder side: u from 0 to pi, v from 0 to 10 (radius 5).
        let f = DAffine3::IDENTITY;
        let s = Surface::Cylinder { f, r: 5.0 };
        let mut pts = Points::default();
        // The edges are cut finely, like the edges of a real face.
        let n = 16;
        let mut ring = Vec::new();
        for k in 0..n {
            ring.push([3.14159 * k as f64 / n as f64, 0.0]);
        }
        for k in 0..n {
            ring.push([3.14159, 10.0 * k as f64 / n as f64]);
        }
        for k in 0..n {
            ring.push([3.14159 * (n - k) as f64 / n as f64, 10.0]);
        }
        for k in 0..n {
            ring.push([0.0, 10.0 * (n - k) as f64 / n as f64]);
        }
        let ids: Vec<usize> = (0..ring.len()).collect();
        for p in &ring {
            pts.uv.push(*p);
            pts.xyz.push(s.eval(p[0], p[1]));
        }
        let r = Refine {
            surface: &s,
            tol: 0.05,
            angle: 0.3,
            max_triangles: 10_000,
        };
        let (t, cut) = mesh_face(&mut pts, &[ids.clone()], &r).unwrap();
        assert!(!cut);
        assert!(t.len() >= 20, "{}", t.len());
        // All points lie on the cylinder.
        for p in &pts.xyz {
            assert!((p.truncate().length() - 5.0).abs() < 1e-9);
        }
        // Every triangle is counterclockwise in UV.
        for tri in &t {
            assert!(cross(pts.uv[tri[0] as usize], pts.uv[tri[1] as usize], pts.uv[tri[2] as usize]) > 0.0);
        }
        // The count limit stops the refinement.
        let mut pts2 = Points::default();
        for p in &ring {
            pts2.uv.push(*p);
            pts2.xyz.push(s.eval(p[0], p[1]));
        }
        let r2 = Refine {
            max_triangles: 10,
            ..r
        };
        let (_, cut2) = mesh_face(&mut pts2, &[ids], &r2).unwrap();
        assert!(cut2);
    }

    #[test]
    fn sphere_cap_with_a_pole() {
        // The north cap of a sphere: the loop is a circle at v = 1.0, and the pole is in it.
        let s = Surface::Sphere {
            f: DAffine3::IDENTITY,
            r: 10.0,
        };
        let mut pts = Points::default();
        let n = 24;
        for k in 0..n {
            let u = std::f64::consts::TAU * k as f64 / n as f64;
            pts.uv.push([u, 1.0]);
            pts.xyz.push(s.eval(u, 1.0));
        }
        // Close the loop through the pole in UV.
        pts.uv.push([std::f64::consts::TAU, 1.0]);
        pts.xyz.push(s.eval(0.0, 1.0));
        pts.uv.push([std::f64::consts::TAU, std::f64::consts::FRAC_PI_2]);
        pts.xyz.push(s.eval(0.0, std::f64::consts::FRAC_PI_2));
        pts.uv.push([0.0, std::f64::consts::FRAC_PI_2]);
        pts.xyz.push(s.eval(0.0, std::f64::consts::FRAC_PI_2));
        let l: Vec<usize> = (0..n + 3).collect();
        let r = Refine {
            surface: &s,
            tol: 0.1,
            angle: 0.3,
            max_triangles: 20_000,
        };
        let (t, _) = mesh_face(&mut pts, &[l], &r).unwrap();
        assert!(t.len() > n);
    }
}
