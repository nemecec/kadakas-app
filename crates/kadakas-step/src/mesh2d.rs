//! The triangles of one face. The boundary points are fixed (the neighbouring faces use the
//! same ones). The region in the parameters of the surface (UV) is cut by ear clipping, with
//! the holes joined to the outline by bridges. Then triangles that are too coarse for the
//! surface are split at an edge or at the middle, and edges are flipped to keep the triangles
//! round. Boundary edges are never split, so the faces fit together.

use std::collections::HashMap;

use glam::DVec3;

use kadakas_core::exact::Surface;

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
            if self.scale[0] > 0.0 {
                self.scale[0]
            } else {
                1.0
            },
            if self.scale[1] > 0.0 {
                self.scale[1]
            } else {
                1.0
            },
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

/// The signed area of a loop (positive when it runs counterclockwise). The numbers are taken
/// from the first point of the loop: far from the origin the sum of products of big numbers
/// would lose all the digits of a small loop.
pub fn area(uv: &[[f64; 2]], l: &[usize]) -> f64 {
    let Some(&first) = l.first() else {
        return 0.0;
    };
    let o = uv[first];
    let n = l.len();
    (0..n)
        .map(|i| {
            let (a, b) = (uv[l[i]], uv[l[(i + 1) % n]]);
            (a[0] - o[0]) * (b[1] - o[1]) - (b[0] - o[0]) * (a[1] - o[1])
        })
        .sum::<f64>()
        / 2.0
}

/// Why a polygon was not cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// The polygon cannot be cut into triangles.
    Cannot,
    /// The joining of the holes took more steps than its budget (`fuel`).
    Fuel(u64),
}

/// Triangles for the outline `loops[0]` (counterclockwise) and the holes `loops[1..]`
/// (clockwise), as indices of `uv`, and the steps of work that the joining of the holes took.
/// `fuel` is the most steps that it may take: a polygon over it is not cut.
pub fn ear_clip(
    uv: &[[f64; 2]],
    xyz: &[DVec3],
    scale: [f64; 2],
    loops: &[Vec<usize>],
    fuel: u64,
) -> Result<(Vec<[u32; 3]>, u64), Cut> {
    let outline = loops.first().ok_or(Cut::Cannot)?;
    let total: usize = loops.iter().map(Vec::len).sum();
    if outline.len() < 3 || total > MAX_POLYGON_POINTS {
        return Err(Cut::Cannot);
    }
    // Holes, the one with the largest x first.
    let mut holes: Vec<&Vec<usize>> = loops[1..].iter().filter(|h| h.len() >= 3).collect();
    let max_x = |h: &Vec<usize>| {
        h.iter()
            .map(|i| uv[*i][0])
            .fold(f64::NEG_INFINITY, f64::max)
    };
    holes.sort_by(|a, b| max_x(b).total_cmp(&max_x(a)));
    let (poly, ops) = if holes.is_empty() {
        (outline.clone(), 0)
    } else {
        let mut m = Merger::new(uv, outline, &holes, fuel);
        for k in 0..holes.len() {
            m.join(k)?;
        }
        (m.polygon(), m.ops)
    };
    clip(uv, xyz, scale, &poly)
        .map(|t| (t, ops))
        .ok_or(Cut::Cannot)
}

fn cross0(u: [f64; 2], v: [f64; 2]) -> f64 {
    u[0] * v[1] - u[1] * v[0]
}

fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// Does the direction `d` from the corner `v` run into the inside of a counterclockwise polygon
/// (not along an edge)? `next` and `prev` are the neighbours of the corner.
fn in_wedge(v: [f64; 2], next: [f64; 2], prev: [f64; 2], target: [f64; 2]) -> bool {
    let (a, b, d) = (sub2(next, v), sub2(prev, v), sub2(target, v));
    let turn = cross0(a, b);
    if turn > 0.0 {
        cross0(a, d) > 0.0 && cross0(d, b) > 0.0
    } else if turn < 0.0 {
        !(cross0(b, d) >= 0.0 && cross0(d, a) >= 0.0)
    } else if a[0] * b[0] + a[1] * b[1] < 0.0 {
        // A straight corner: the inside is on the left of the edge.
        cross0(a, d) > 0.0
    } else {
        false
    }
}

fn on_segment(p: [f64; 2], q: [f64; 2], r: [f64; 2]) -> bool {
    r[0] >= p[0].min(q[0])
        && r[0] <= p[0].max(q[0])
        && r[1] >= p[1].min(q[1])
        && r[1] <= p[1].max(q[1])
}

/// Do the segments pq and ab share a point (also one end on the other, or an overlap)?
fn segments_meet(p: [f64; 2], q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    if p[0].max(q[0]) < a[0].min(b[0])
        || a[0].max(b[0]) < p[0].min(q[0])
        || p[1].max(q[1]) < a[1].min(b[1])
        || a[1].max(b[1]) < p[1].min(q[1])
    {
        return false;
    }
    let o1 = cross(p, q, a);
    let o2 = cross(p, q, b);
    let o3 = cross(a, b, p);
    let o4 = cross(a, b, q);
    if ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
        && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
    {
        return true;
    }
    (o1 == 0.0 && on_segment(p, q, a))
        || (o2 == 0.0 && on_segment(p, q, b))
        || (o3 == 0.0 && on_segment(a, b, p))
        || (o4 == 0.0 && on_segment(a, b, q))
}

/// A segment of the outline, a hole or a bridge.
struct Seg {
    a: [f64; 2],
    b: [f64; 2],
    /// The indices of the ends in `uv`.
    va: usize,
    vb: usize,
    /// True once the segment is an edge of the merged polygon (not of a hole that waits).
    merged: bool,
}

/// The merged polygon as a ring of corners (a corner that a bridge doubles is two nodes), with
/// all segments in a grid, so that a hole finds its neighbours without looking at the rest.
/// A hole is joined by a bridge between its rightmost corner and a corner of the polygon that
/// it can see. The corner comes from the ray to the right and the corners inside the triangle
/// of the ray, as usual. Every choice is checked: the bridge must run inside the polygon at both
/// ends and must not meet any segment (the merged polygon with its bridges, the holes that
/// wait, the hole itself). A corner that the polygon has twice is taken at the copy that has the
/// bridge inside its angle. When the usual corner does not fit, the nearest one that fits is
/// taken. The cost of a hole is the cells around its bridge, not the size of the polygon.
struct Merger<'a> {
    uv: &'a [[f64; 2]],
    holes: &'a [&'a Vec<usize>],
    /// The ring: the index in `uv` of each node and its neighbours.
    vert: Vec<usize>,
    next: Vec<u32>,
    prev: Vec<u32>,
    /// The nodes of each corner: the first, and the next of the same corner.
    first: HashMap<usize, u32>,
    same: Vec<u32>,
    segs: Vec<Seg>,
    lo: [f64; 2],
    size: [f64; 2],
    n: usize,
    cells: Vec<Vec<u32>>,
    /// Steps taken (cells visited, segments tested) and the most that are allowed.
    pub ops: u64,
    fuel: u64,
    /// The start node of the ring.
    head: u32,
    /// The segments of each hole are `hole_segs[k]..hole_segs[k + 1]`.
    hole_segs: Vec<usize>,
}

const NONE: u32 = u32::MAX;

impl<'a> Merger<'a> {
    fn new(uv: &'a [[f64; 2]], outline: &[usize], holes: &'a [&'a Vec<usize>], fuel: u64) -> Self {
        let edges = outline.len() + holes.iter().map(|h| h.len()).sum::<usize>();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for &i in outline.iter().chain(holes.iter().flat_map(|h| h.iter())) {
            for k in 0..2 {
                lo[k] = lo[k].min(uv[i][k]);
                hi[k] = hi[k].max(uv[i][k]);
            }
        }
        let n = ((edges as f64).sqrt().ceil() as usize).clamp(1, 1024);
        let size = [
            ((hi[0] - lo[0]) / n as f64).max(1e-300),
            ((hi[1] - lo[1]) / n as f64).max(1e-300),
        ];
        let mut m = Merger {
            uv,
            holes,
            vert: Vec::with_capacity(edges + 2 * holes.len() + 2),
            next: Vec::new(),
            prev: Vec::new(),
            first: HashMap::new(),
            same: Vec::new(),
            segs: Vec::with_capacity(edges + holes.len()),
            lo,
            size,
            n,
            cells: vec![Vec::new(); n * n],
            ops: 0,
            fuel,
            head: 0,
            hole_segs: Vec::new(),
        };
        let o = m.ring(outline);
        m.head = o;
        m.add_ring_segments(outline, true);
        for h in holes {
            m.hole_segs.push(m.segs.len());
            m.add_ring_segments(h, false);
        }
        m.hole_segs.push(m.segs.len());
        m
    }

    /// Makes the nodes of a loop in a ring and returns the first.
    fn ring(&mut self, l: &[usize]) -> u32 {
        let base = self.vert.len() as u32;
        let k = l.len() as u32;
        for (j, &v) in l.iter().enumerate() {
            let id = base + j as u32;
            self.vert.push(v);
            self.next.push(base + (j as u32 + 1) % k);
            self.prev.push(base + (j as u32 + k - 1) % k);
            self.same.push(NONE);
            self.link_same(v, id);
        }
        base
    }

    fn link_same(&mut self, v: usize, id: u32) {
        let head = self.first.entry(v).or_insert(NONE);
        self.same[id as usize] = *head;
        *head = id;
    }

    fn node(&mut self, v: usize) -> u32 {
        let id = self.vert.len() as u32;
        self.vert.push(v);
        self.next.push(NONE);
        self.prev.push(NONE);
        self.same.push(NONE);
        self.link_same(v, id);
        id
    }

    fn add_ring_segments(&mut self, l: &[usize], merged: bool) {
        for j in 0..l.len() {
            let (va, vb) = (l[j], l[(j + 1) % l.len()]);
            self.add_seg(va, vb, merged);
        }
    }

    fn cell_of(&self, p: [f64; 2]) -> (usize, usize) {
        let f = |k: usize| {
            (((p[k] - self.lo[k]) / self.size[k]).floor().max(0.0) as usize).min(self.n - 1)
        };
        (f(0), f(1))
    }

    /// Calls `f` with each cell on the way from `a` to `b` (a grid walk). `f` says when to stop.
    fn walk(&self, a: [f64; 2], b: [f64; 2], mut f: impl FnMut(usize, usize) -> bool) {
        let (mut cx, mut cy) = self.cell_of(a);
        let (ex, ey) = self.cell_of(b);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let step = |d: f64| if d > 0.0 { 1isize } else { -1 };
        let (sx, sy) = (step(dx), step(dy));
        let tmax = |c: usize, s: isize, d: f64, k: usize| {
            if d == 0.0 {
                f64::INFINITY
            } else {
                let edge = self.lo[k] + (c as isize + isize::from(s > 0)) as f64 * self.size[k];
                (edge - a[k]) / d
            }
        };
        let mut tx = tmax(cx, sx, dx, 0);
        let mut ty = tmax(cy, sy, dy, 1);
        let tdx = if dx == 0.0 {
            f64::INFINITY
        } else {
            self.size[0] / dx.abs()
        };
        let tdy = if dy == 0.0 {
            f64::INFINITY
        } else {
            self.size[1] / dy.abs()
        };
        let mut left = (cx.abs_diff(ex) + cy.abs_diff(ey)) + 2;
        loop {
            if !f(cx, cy) || (cx == ex && cy == ey) || left == 0 {
                break;
            }
            left -= 1;
            if tx < ty {
                let nx = cx as isize + sx;
                if nx < 0 || nx >= self.n as isize {
                    break;
                }
                cx = nx as usize;
                tx += tdx;
            } else {
                let ny = cy as isize + sy;
                if ny < 0 || ny >= self.n as isize {
                    break;
                }
                cy = ny as usize;
                ty += tdy;
            }
        }
    }

    fn add_seg(&mut self, va: usize, vb: usize, merged: bool) {
        let id = self.segs.len() as u32;
        let (a, b) = (self.uv[va], self.uv[vb]);
        self.segs.push(Seg {
            a,
            b,
            va,
            vb,
            merged,
        });
        // In the cells on its way and their neighbours: a segment that only touches the edge
        // of a cell is still found from the next cell.
        let mut cells: Vec<usize> = Vec::new();
        let n = self.n;
        self.walk(a, b, |cx, cy| {
            for y in cy.saturating_sub(1)..=(cy + 1).min(n - 1) {
                for x in cx.saturating_sub(1)..=(cx + 1).min(n - 1) {
                    cells.push(y * n + x);
                }
            }
            true
        });
        cells.sort_unstable();
        cells.dedup();
        self.ops += cells.len() as u64;
        for c in cells {
            self.cells[c].push(id);
        }
    }

    fn spend(&mut self, n: u64) -> Result<(), Cut> {
        self.ops += n;
        if self.ops > self.fuel {
            Err(Cut::Fuel(self.ops))
        } else {
            Ok(())
        }
    }

    /// Does the open segment from `m` to `v` meet a segment of the polygon, of a waiting hole
    /// or of the hole itself, other than the ones that end at `m` or `v`?
    fn blocked(&mut self, m: [f64; 2], v: [f64; 2]) -> Result<bool, Cut> {
        let mut hit = false;
        let mut cells: Vec<usize> = Vec::new();
        let n = self.n;
        self.walk(m, v, |cx, cy| {
            cells.push(cy * n + cx);
            true
        });
        self.spend(cells.len() as u64)?;
        for c in cells {
            for k in 0..self.cells[c].len() {
                let s = &self.segs[self.cells[c][k] as usize];
                let (a, b) = (s.a, s.b);
                if a == m || b == m || a == v || b == v {
                    continue;
                }
                if segments_meet(m, v, a, b) {
                    hit = true;
                    break;
                }
            }
            self.ops += self.cells[c].len() as u64;
            if hit {
                break;
            }
        }
        self.spend(0)?;
        Ok(hit)
    }

    /// Can the hole corner `m` (with neighbours `hp` and `hn` in its loop) be joined to the node
    /// `k`? The bridge must run into the inside at both ends and meet no segment.
    fn fits(&mut self, k: u32, m: [f64; 2], hp: [f64; 2], hn: [f64; 2]) -> Result<bool, Cut> {
        let v = self.uv[self.vert[k as usize]];
        if v == m {
            return Ok(false);
        }
        let (pp, pn) = (
            self.uv[self.vert[self.prev[k as usize] as usize]],
            self.uv[self.vert[self.next[k as usize] as usize]],
        );
        if !in_wedge(v, pn, pp, m) || !in_wedge(m, hn, hp, v) {
            return Ok(false);
        }
        Ok(!self.blocked(m, v)?)
    }

    fn nodes_of(&self, v: usize) -> Vec<u32> {
        let mut out = Vec::new();
        let mut at = self.first.get(&v).copied().unwrap_or(NONE);
        while at != NONE {
            out.push(at);
            at = self.same[at as usize];
        }
        out
    }

    /// The first segment of the merged polygon that the ray to the right of `m` meets, by
    /// the grid walk: its crossing and its end with the larger x.
    fn ray(&mut self, m: [f64; 2]) -> Result<Option<(f64, usize)>, Cut> {
        let far = [self.lo[0] + self.size[0] * (self.n as f64 + 1.0), m[1]];
        let mut best: Option<(f64, usize)> = None;
        let mut visited: Vec<(usize, usize)> = Vec::new();
        self.walk(m, far, |cx, cy| {
            visited.push((cx, cy));
            true
        });
        for (cx, cy) in visited {
            let right = self.lo[0] + (cx + 1) as f64 * self.size[0];
            self.spend(1 + self.cells[cy * self.n + cx].len() as u64)?;
            for &id in &self.cells[cy * self.n + cx] {
                let s = &self.segs[id as usize];
                let (a, b) = (s.a, s.b);
                if !s.merged || (a[1] - m[1]) * (b[1] - m[1]) > 0.0 || a[1] == b[1] {
                    continue;
                }
                let x = a[0] + (m[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if x >= m[0] - 1e-12 && x <= right && best.is_none_or(|(bx, _)| x < bx) {
                    best = Some((x, id as usize));
                }
            }
            if best.is_some() {
                break;
            }
        }
        Ok(best)
    }

    /// Joins the hole `k` to the polygon.
    fn join(&mut self, k: usize) -> Result<(), Cut> {
        let hole = self.holes[k];
        let h = hole.len();
        let (mi, _) = hole
            .iter()
            .enumerate()
            .max_by(|a, b| self.uv[*a.1][0].total_cmp(&self.uv[*b.1][0]))
            .ok_or(Cut::Cannot)?;
        let m = self.uv[hole[mi]];
        let (hp, hn) = (self.uv[hole[(mi + h - 1) % h]], self.uv[hole[(mi + 1) % h]]);
        let mut tries: Vec<usize> = Vec::new();
        if let Some((ix, si)) = self.ray(m)? {
            let s = &self.segs[si];
            let (p, pv) = if s.a[0] > s.b[0] {
                (s.a, s.va)
            } else {
                (s.b, s.vb)
            };
            let ipt = [ix, m[1]];
            // The corners inside the triangle (M, I, P) block the view: the one with the
            // smallest angle to the ray is the candidate.
            let mut blockers: Vec<(f64, f64, usize)> = Vec::new();
            let (lo, hi) = (
                self.cell_of([m[0], m[1].min(p[1])]),
                self.cell_of([ix.max(p[0]), m[1].max(p[1])]),
            );
            if (hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1) <= 4096 {
                self.spend(((hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1)) as u64)?;
                let mut seen: Vec<usize> = Vec::new();
                for cy in lo.1..=hi.1 {
                    for cx in lo.0..=hi.0 {
                        for &id in &self.cells[cy * self.n + cx] {
                            let s = &self.segs[id as usize];
                            if !s.merged {
                                continue;
                            }
                            for (q, qv) in [(s.a, s.va), (s.b, s.vb)] {
                                if qv != pv && in_triangle(m, ipt, p, q) && !seen.contains(&qv) {
                                    seen.push(qv);
                                    let ang = (q[1] - m[1]).abs().atan2(q[0] - m[0]);
                                    blockers.push((ang, dist2(m, q), qv));
                                }
                            }
                        }
                    }
                }
            }
            blockers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            tries.extend(blockers.iter().take(8).map(|b| b.2));
            tries.push(pv);
        }
        let mut found: Option<u32> = None;
        'usual: for v in tries {
            for node in self.nodes_of(v) {
                if self.fits(node, m, hp, hn)? {
                    found = Some(node);
                    break 'usual;
                }
            }
        }
        if found.is_none() {
            found = self.nearest_fit(m, hp, hn)?;
        }
        let node = found.ok_or(Cut::Cannot)?;
        self.splice(node, hole, mi);
        // The hole's own segments are part of the polygon from now on.
        for s in self.hole_segs[k]..self.hole_segs[k + 1] {
            self.segs[s].merged = true;
        }
        // The bridge is two edges of the polygon.
        let v = self.vert[node as usize];
        self.add_seg(v, hole[mi], true);
        self.spend(0)
    }

    /// The nearest corner of the polygon that the corner `m` can be joined to: rings of cells
    /// around `m`, each ring by distance.
    fn nearest_fit(&mut self, m: [f64; 2], hp: [f64; 2], hn: [f64; 2]) -> Result<Option<u32>, Cut> {
        let (cx, cy) = self.cell_of(m);
        let n = self.n;
        let mut tested: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for r in 0..=n {
            let mut ring: Vec<(f64, usize)> = Vec::new();
            for y in cy.saturating_sub(r)..=(cy + r).min(n - 1) {
                for x in cx.saturating_sub(r)..=(cx + r).min(n - 1) {
                    if x.abs_diff(cx).max(y.abs_diff(cy)) != r {
                        continue;
                    }
                    for &id in &self.cells[y * n + x] {
                        let s = &self.segs[id as usize];
                        if !s.merged {
                            continue;
                        }
                        for (q, qv) in [(s.a, s.va), (s.b, s.vb)] {
                            if tested.insert(qv) {
                                ring.push((dist2(m, q), qv));
                            }
                        }
                    }
                }
            }
            self.spend(1 + ring.len() as u64)?;
            ring.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, v) in ring {
                for node in self.nodes_of(v) {
                    if self.fits(node, m, hp, hn)? {
                        return Ok(Some(node));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Puts the hole into the ring after `node`: the node, the hole from its corner `mi` round
    /// to the corner again, and the node again.
    fn splice(&mut self, node: u32, hole: &[usize], mi: usize) {
        let h = hole.len();
        let after = self.next[node as usize];
        let rotated: Vec<usize> = (0..=h).map(|j| hole[(mi + j) % h]).collect();
        let mut prev = node;
        for v in rotated {
            let id = self.node(v);
            self.next[prev as usize] = id;
            self.prev[id as usize] = prev;
            prev = id;
        }
        let again = self.node(self.vert[node as usize]);
        self.next[prev as usize] = again;
        self.prev[again as usize] = prev;
        self.next[again as usize] = after;
        self.prev[after as usize] = again;
    }

    /// The corners of the ring in order, from the head.
    fn polygon(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.vert.len());
        let mut at = self.head;
        loop {
            out.push(self.vert[at as usize]);
            at = self.next[at as usize];
            if at == self.head {
                break;
            }
        }
        out
    }
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
    let inside_any =
        |a: usize, b: usize, c: usize, prev: &[usize], next: &[usize], alive: &[bool]| -> bool {
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
    let (u, v) = (
        [pa[0] - pb[0], pa[1] - pb[1]],
        [pc[0] - pb[0], pc[1] - pb[1]],
    );
    let (lu, lv) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
    if lu == 0.0 || lv == 0.0 {
        return 0.0;
    }
    ((u[0] * v[0] + u[1] * v[1]) / (lu * lv))
        .clamp(-1.0, 1.0)
        .acos()
}

/// What the refinement needs to know.
pub struct Refine<'s> {
    pub surface: &'s Surface,
    pub tol: f64,
    pub angle: f64,
    pub max_triangles: usize,
    /// The most steps that the joining of the holes may take (see [`ear_clip`]).
    pub fuel: u64,
}

/// Cuts the face: the triangles of the polygon, then refined. Returns the triangles and
/// whether the refinement stopped at the limit.
pub fn mesh_face(
    pts: &mut Points,
    loops: &[Vec<usize>],
    refine: &Refine,
) -> Result<(Vec<[u32; 3]>, bool, u64), Cut> {
    let (tris, ops) = ear_clip(&pts.uv, &pts.xyz, pts.scale, loops, refine.fuel)?;
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
        let mut stack: Vec<u32> = (0..topo.tris.len() as u32)
            .filter(|t| topo.alive[*t as usize])
            .collect();
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
    Ok((tris, truncated, ops))
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
                let between =
                    (up[0] - um[0]) * (uq[0] - um[0]) + (up[1] - um[1]) * (uq[1] - um[1]) < 0.0;
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

/// The most steps of work that the seeding of one face may take (grid points times boundary
/// points, for the test of each grid point against the loops). A face over it is not seeded.
pub const SEED_MAX_COST: u64 = 30_000_000;

/// The steps of work that seeding a face takes, about.
pub fn seed_cost(pts: &Points, loops: &[Vec<usize>]) -> u64 {
    let Some(outline) = loops.first() else {
        return 0;
    };
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &i in outline {
        for k in 0..2 {
            lo[k] = lo[k].min(pts.uv[i][k]);
            hi[k] = hi[k].max(pts.uv[i][k]);
        }
    }
    let Some((_, n)) = grid_plan(pts, loops, lo, hi) else {
        return 0;
    };
    let boundary: u64 = loops.iter().map(|l| l.len() as u64).sum();
    (n[0] as u64 * n[1] as u64).saturating_mul(boundary + 1)
}

/// The steps and the counts of the seed grid, or `None` when the grid is too big.
fn grid_plan(
    pts: &Points,
    loops: &[Vec<usize>],
    lo: [f64; 2],
    hi: [f64; 2],
) -> Option<([f64; 2], [usize; 2])> {
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
    (n[0] * n[1] <= 60_000).then_some((step, n))
}

/// Points in the region on a grid in the parameters, so that no triangle is a long needle
/// before the refinement starts. The grid steps are those of the boundary: the median
/// distance between neighbouring boundary points in each direction.
fn seed_grid(topo: &mut Topo, pts: &mut Points, loops: &[Vec<usize>], r: &Refine) {
    if seed_cost(pts, loops) > SEED_MAX_COST {
        return;
    }
    let outline = &loops[0];
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &i in outline {
        for k in 0..2 {
            lo[k] = lo[k].min(pts.uv[i][k]);
            hi[k] = hi[k].max(pts.uv[i][k]);
        }
    }
    let Some((step, n)) = grid_plan(pts, loops, lo, hi) else {
        return;
    };
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
            if !area2.is_finite() || area2 <= 0.0 || w.iter().any(|x| !x.is_finite() || *x < 1e-6) {
                continue;
            }
            pts.uv.push(p);
            pts.xyz.push(xyz);
            let m = (pts.uv.len() - 1) as u32;
            topo.remove(t);
            let nt = [
                topo.add([tri[0], tri[1], m]),
                topo.add([tri[1], tri[2], m]),
                topo.add([tri[2], tri[0], m]),
            ];
            last = nt[0];
            let _ = legalize(
                topo,
                pts,
                vec![(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])],
                r.surface,
            );
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
    let longest_uv = (0..3)
        .map(|k| dist2(uv[k], uv[(k + 1) % 3]))
        .fold(0.0, f64::max);
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
            let c = tri
                .iter()
                .copied()
                .find(|v| *v != a && *v != b)
                .unwrap_or(a);
            let (ua, ub) = (pts.uv[a as usize], pts.uv[b as usize]);
            // A corner that lies on the edge (a flat neighbour) is the split point.
            let on_edge = |p: u32| {
                let up = pts.uv[p as usize];
                let l2 = dist2(ua, ub);
                cross(ua, ub, up).abs() <= 1e-9 * l2
                    && (up[0] - ua[0]) * (ub[0] - ua[0]) + (up[1] - ua[1]) * (ub[1] - ua[1])
                        > 1e-6 * l2
                    && (up[0] - ub[0]) * (ua[0] - ub[0]) + (up[1] - ub[1]) * (ua[1] - ub[1])
                        > 1e-6 * l2
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
                return r
                    .into_iter()
                    .chain(made)
                    .filter(|t| topo.alive[*t as usize])
                    .collect();
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
            new.into_iter()
                .chain(made)
                .filter(|t| topo.alive[*t as usize])
                .collect()
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
            new.into_iter()
                .chain(made)
                .filter(|t| topo.alive[*t as usize])
                .collect()
        }
    }
}

/// Flips edges where it makes the triangles better, as far as the two triangles make a
/// convex quadrilateral in UV. On a plane the better pair has the larger smallest angle
/// (Delaunay: opposite angles add up to less than a half turn). On a curved surface the
/// better pair leans less from the surface.
fn legalize(
    topo: &mut Topo,
    pts: &Points,
    mut stack: Vec<(u32, u32)>,
    _surface: &Surface,
) -> Vec<u32> {
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
        let c = tri
            .iter()
            .copied()
            .find(|v| *v != a && *v != b)
            .unwrap_or(a);
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

    /// The triangles of a polygon with holes, with no limit on the steps.
    fn clip_polygon(
        uv: &[[f64; 2]],
        xyz: &[DVec3],
        scale: [f64; 2],
        loops: &[Vec<usize>],
    ) -> Option<Vec<[u32; 3]>> {
        ear_clip(uv, xyz, scale, loops, u64::MAX).ok().map(|r| r.0)
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
        let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &[l]).unwrap();
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
        let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &[l]).unwrap();
        assert!((area_of(&uv, &t) - 12.0).abs() < 1e-9);
        assert!(clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &[]).is_none());
        assert!(clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &[vec![0, 1]]).is_none());
    }

    /// A rectangle with a grid of square holes (clockwise), some of them jittered or left out.
    /// Returns the points, the loops and the area that must be covered.
    fn grid_holes(
        nx: usize,
        ny: usize,
        jitter: f64,
        seed: u64,
        skip: u64,
    ) -> (Vec<[f64; 2]>, Vec<Vec<usize>>, f64) {
        let mut state = seed | 1;
        let mut rnd = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 10_000) as f64 / 10_000.0 - 0.5
        };
        let (w, h) = (nx as f64 * 2.0 + 1.0, ny as f64 * 2.0 + 1.0);
        let mut uv = vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
        let mut loops = vec![vec![0, 1, 2, 3]];
        let mut want = w * h;
        for j in 0..ny {
            for i in 0..nx {
                if skip > 0 && ((i * 7 + j * 13) as u64 + seed).is_multiple_of(skip) {
                    continue;
                }
                let (cx, cy) = (2.0 * i as f64 + 1.5, 2.0 * j as f64 + 1.5);
                let base = uv.len();
                let corners = [[-0.5, -0.5], [-0.5, 0.5], [0.5, 0.5], [0.5, -0.5]];
                let mut pts = Vec::new();
                for c in corners {
                    pts.push([cx + c[0] + rnd() * jitter, cy + c[1] + rnd() * jitter]);
                }
                let a = area(&pts, &[0, 1, 2, 3]);
                want += a;
                uv.extend(pts);
                loops.push((base..base + 4).collect());
            }
        }
        (uv, loops, want)
    }

    /// The triangles cover the region once: positive areas, the area of the region, and every
    /// edge is used by one triangle on each side, except the edges of the loops.
    fn assert_covers(uv: &[[f64; 2]], loops: &[Vec<usize>], want: f64, t: &[[u32; 3]]) {
        assert_covers_to(1e-9, uv, loops, want, t);
    }

    fn assert_covers_to(
        tol: f64,
        uv: &[[f64; 2]],
        loops: &[Vec<usize>],
        want: f64,
        t: &[[u32; 3]],
    ) {
        let got = area_of(uv, t);
        assert!(
            (got - want).abs() <= tol * want.abs().max(1.0),
            "{got} {want}"
        );
        let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
        for tri in t {
            assert!(
                cross(
                    uv[tri[0] as usize],
                    uv[tri[1] as usize],
                    uv[tri[2] as usize]
                ) >= -1e-12,
                "a triangle looks the wrong way"
            );
            for k in 0..3 {
                *directed.entry((tri[k], tri[(k + 1) % 3])).or_default() += 1;
            }
        }
        let boundary: std::collections::HashSet<(u32, u32)> = loops
            .iter()
            .flat_map(|l| (0..l.len()).map(move |i| (l[i] as u32, l[(i + 1) % l.len()] as u32)))
            .collect();
        for (&(a, b), &n) in &directed {
            assert_eq!(n, 1, "the edge {a}-{b} is used {n} times in one direction");
            if boundary.contains(&(a, b)) {
                assert!(
                    !directed.contains_key(&(b, a)),
                    "a loop edge has a triangle on both sides"
                );
            } else {
                assert!(
                    directed.contains_key(&(b, a)),
                    "the edge {a}-{b} has no neighbour"
                );
            }
        }
        for &(a, b) in &boundary {
            if area_of_edge_is_real(uv, a, b) {
                assert!(
                    directed.contains_key(&(a, b)),
                    "the loop edge {a}-{b} has no triangle"
                );
            }
        }
    }

    fn area_of_edge_is_real(uv: &[[f64; 2]], a: u32, b: u32) -> bool {
        uv[a as usize] != uv[b as usize]
    }

    #[test]
    fn grids_of_holes_in_columns_rows_and_blocks_are_cut() {
        for (nx, ny) in [
            (1, 2),
            (1, 3),
            (1, 4),
            (3, 1),
            (2, 3),
            (3, 3),
            (4, 4),
            (10, 10),
        ] {
            for jitter in [0.0, 0.3] {
                for seed in [1u64, 7] {
                    let (uv, loops, want) = grid_holes(nx, ny, jitter, seed, 0);
                    let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops)
                        .unwrap_or_else(|| panic!("{nx}x{ny} jitter {jitter}"));
                    assert_covers(&uv, &loops, want, &t);
                }
            }
        }
    }

    #[test]
    fn polygons_far_from_the_origin_are_cut_like_the_ones_near_it() {
        for (dx, dy) in [(1e7, 3.7e6), (-2.5e8, 4e8), (1e9, 1e9)] {
            let (mut uv, loops, want) = grid_holes(4, 3, 0.2, 5, 0);
            let (s, c) = (0.5f64, 0.866_025_403_784_438_6f64);
            for p in &mut uv {
                *p = [c * p[0] - s * p[1] + dx, s * p[0] + c * p[1] + dy];
            }
            let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops)
                .unwrap_or_else(|| panic!("offset {dx} {dy}"));
            // The numbers have 7 digits after the point at 1e9: the areas agree to 1e-6.
            assert_covers_to(1e-6, &uv, &loops, want, &t);
        }
    }

    #[test]
    fn joining_holes_stops_at_its_fuel() {
        let (uv, loops, _) = grid_holes(10, 10, 0.0, 1, 0);
        let r = ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &loops, 50);
        assert!(
            matches!(r, Err(Cut::Fuel(n)) if n > 50),
            "{:?}",
            r.map(|x| x.1)
        );
        // With enough fuel the steps are counted.
        let (_, ops) = ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &loops, u64::MAX).unwrap();
        assert!(ops > 100 && ops < 1_000_000, "{ops}");
    }

    #[test]
    fn a_big_grid_of_holes_is_cut() {
        let (uv, loops, want) = grid_holes(45, 45, 0.2, 3, 0);
        let start = std::time::Instant::now();
        let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops).expect("45x45 holes");
        assert_covers(&uv, &loops, want, &t);
        assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
    }

    #[test]
    fn a_hole_that_touches_nothing_and_holes_in_a_concave_outline() {
        // A C-shaped outline with two holes in the arms and one in the middle of the back.
        let mut uv = vec![
            [0.0, 0.0],
            [12.0, 0.0],
            [12.0, 4.0],
            [4.0, 4.0],
            [4.0, 8.0],
            [12.0, 8.0],
            [12.0, 12.0],
            [0.0, 12.0],
        ];
        let mut loops = vec![(0..8).collect::<Vec<usize>>()];
        let mut want = area(&uv, &loops[0]);
        for (cx, cy, s) in [
            (8.0, 2.0, 0.8),
            (8.0, 10.0, 0.8),
            (2.0, 6.0, 1.0),
            (2.0, 2.0, 0.5),
            (2.0, 10.0, 0.5),
        ] {
            let base = uv.len();
            uv.extend([
                [cx - s, cy - s],
                [cx - s, cy + s],
                [cx + s, cy + s],
                [cx + s, cy - s],
            ]);
            want -= 4.0 * s * s;
            loops.push((base..base + 4).collect());
        }
        let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops).unwrap();
        assert_covers(&uv, &loops, want, &t);
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
        #[test]
        fn random_grids_are_covered_once(
            nx in 1usize..8,
            ny in 1usize..8,
            jitter in 0.0f64..0.45,
            seed in 1u64..1_000_000,
            skip in 0u64..5,
        ) {
            let (uv, loops, want) = grid_holes(nx, ny, jitter, seed, skip);
            let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops);
            proptest::prop_assert!(t.is_some(), "{}x{} jitter {} seed {} skip {}", nx, ny, jitter, seed, skip);
            assert_covers(&uv, &loops, want, &t.unwrap());
        }
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
        let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops).unwrap();
        let want = 100.0 - 4.0 - 9.0 - 2.56;
        assert!(
            (area_of(&uv, &t) - want).abs() < 1e-9,
            "{}",
            area_of(&uv, &t)
        );
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
        let t2 = clip_polygon(&uv2, &flat3(&uv2), [1.0, 1.0], &l2).unwrap();
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
        assert!(
            clip_polygon(
                &uv3,
                &flat3(&uv3),
                [1.0, 1.0],
                &[vec![0, 1, 2, 3], vec![4, 5, 6, 7]]
            )
            .is_none()
        );
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
            ring.push([std::f64::consts::PI * k as f64 / n as f64, 0.0]);
        }
        for k in 0..n {
            ring.push([std::f64::consts::PI, 10.0 * k as f64 / n as f64]);
        }
        for k in 0..n {
            ring.push([std::f64::consts::PI * (n - k) as f64 / n as f64, 10.0]);
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
            fuel: u64::MAX,
        };
        let (t, cut, _) = mesh_face(&mut pts, std::slice::from_ref(&ids), &r).unwrap();
        assert!(!cut);
        assert!(t.len() >= 20, "{}", t.len());
        // All points lie on the cylinder.
        for p in &pts.xyz {
            assert!((p.truncate().length() - 5.0).abs() < 1e-9);
        }
        // Every triangle is counterclockwise in UV.
        for tri in &t {
            assert!(
                cross(
                    pts.uv[tri[0] as usize],
                    pts.uv[tri[1] as usize],
                    pts.uv[tri[2] as usize]
                ) > 0.0
            );
        }
        // The count limit stops the refinement.
        let mut pts2 = Points::default();
        for p in &ring {
            pts2.uv.push(*p);
            pts2.xyz.push(s.eval(p[0], p[1]));
        }
        let r2 = Refine {
            max_triangles: 10,
            fuel: u64::MAX,
            ..r
        };
        let (_, cut2, _) = mesh_face(&mut pts2, &[ids], &r2).unwrap();
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
        pts.uv
            .push([std::f64::consts::TAU, std::f64::consts::FRAC_PI_2]);
        pts.xyz.push(s.eval(0.0, std::f64::consts::FRAC_PI_2));
        pts.uv.push([0.0, std::f64::consts::FRAC_PI_2]);
        pts.xyz.push(s.eval(0.0, std::f64::consts::FRAC_PI_2));
        let l: Vec<usize> = (0..n + 3).collect();
        let r = Refine {
            surface: &s,
            tol: 0.1,
            angle: 0.3,
            max_triangles: 20_000,
            fuel: u64::MAX,
        };
        let (t, _, _) = mesh_face(&mut pts, &[l], &r).unwrap();
        assert!(t.len() > n);
    }
}
