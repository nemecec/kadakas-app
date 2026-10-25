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
    let (poly, mut ops) = if holes.is_empty() {
        (outline.clone(), 0)
    } else {
        let mut m = crate::merge::Merger::new(uv, outline, &holes, fuel);
        for k in 0..holes.len() {
            m.join(k)?;
        }
        (m.polygon(), m.ops)
    };
    let tris = clip(uv, xyz, scale, &poly, fuel, &mut ops)?;
    Ok((tris, ops))
}

pub(crate) fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
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

/// A quadtree of points, each with a number. It answers "is there a point in this box that
/// passes this test" and visits only the places near the box.
struct PointTree {
    nodes: Vec<PNode>,
}

struct PNode {
    lo: [f64; 2],
    hi: [f64; 2],
    kids: [u32; 4],
    points: Vec<([f64; 2], usize)>,
}

impl PointTree {
    const LEAF: usize = 16;
    const DEPTH: u32 = 40;

    fn build(points: &[([f64; 2], usize)]) -> PointTree {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for (p, _) in points {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if points.is_empty() {
            lo = [0.0; 2];
            hi = [0.0; 2];
        }
        let mut t = PointTree {
            nodes: vec![PNode {
                lo,
                hi,
                kids: [u32::MAX; 4],
                points: points.to_vec(),
            }],
        };
        t.split(0, 0);
        t
    }

    fn split(&mut self, ni: usize, depth: u32) {
        if self.nodes[ni].points.len() <= Self::LEAF || depth >= Self::DEPTH {
            return;
        }
        let (lo, hi) = (self.nodes[ni].lo, self.nodes[ni].hi);
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        if mid[0] <= lo[0] && mid[1] <= lo[1] {
            return;
        }
        let pts = std::mem::take(&mut self.nodes[ni].points);
        let mut parts: [Vec<([f64; 2], usize)>; 4] = Default::default();
        for p in pts {
            let k = usize::from(p.0[0] > mid[0]) + 2 * usize::from(p.0[1] > mid[1]);
            parts[k].push(p);
        }
        let base = self.nodes.len() as u32;
        for (k, part) in parts.into_iter().enumerate() {
            let (xh, yh) = (k % 2, k / 2);
            self.nodes.push(PNode {
                lo: [
                    if xh == 0 { lo[0] } else { mid[0] },
                    if yh == 0 { lo[1] } else { mid[1] },
                ],
                hi: [
                    if xh == 0 { mid[0] } else { hi[0] },
                    if yh == 0 { mid[1] } else { hi[1] },
                ],
                kids: [u32::MAX; 4],
                points: part,
            });
        }
        self.nodes[ni].kids = [base, base + 1, base + 2, base + 3];
        for k in 0..4 {
            self.split((base + k) as usize, depth + 1);
        }
    }

    /// Is there a point in the triangle abc (ends included) for which `test` says yes? Only the
    /// boxes that the triangle touches are looked at, so a long thin triangle in a crowd of
    /// points costs what is near it. `steps` counts the nodes and the points looked at.
    fn any_in_triangle(
        &self,
        tri: [[f64; 2]; 3],
        steps: &mut u64,
        test: &mut dyn FnMut(usize) -> bool,
    ) -> bool {
        let orient = cross(tri[0], tri[1], tri[2]);
        let sign = if orient < 0.0 { -1.0 } else { 1.0 };
        let (lo, hi) = (
            [
                tri[0][0].min(tri[1][0]).min(tri[2][0]),
                tri[0][1].min(tri[1][1]).min(tri[2][1]),
            ],
            [
                tri[0][0].max(tri[1][0]).max(tri[2][0]),
                tri[0][1].max(tri[1][1]).max(tri[2][1]),
            ],
        );
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            *steps += 1;
            let n = &self.nodes[ni];
            if n.hi[0] < lo[0] || n.lo[0] > hi[0] || n.hi[1] < lo[1] || n.lo[1] > hi[1] {
                continue;
            }
            // The box is outside the triangle when all its corners are outside one edge.
            let corners = [n.lo, [n.hi[0], n.lo[1]], n.hi, [n.lo[0], n.hi[1]]];
            let outside = (0..3).any(|e| {
                corners
                    .iter()
                    .all(|c| sign * cross(tri[e], tri[(e + 1) % 3], *c) < 0.0)
            });
            if outside {
                continue;
            }
            if n.kids[0] == u32::MAX {
                for &(p, id) in &n.points {
                    *steps += 1;
                    if in_triangle(tri[0], tri[1], tri[2], p) && test(id) {
                        return true;
                    }
                }
            } else {
                stack.extend(n.kids.iter().map(|k| *k as usize));
            }
        }
        false
    }
}

/// Steps that one push on the heap of the ears or one pop is worth.
const HEAP_STEPS: u64 = 30;

/// Ear clipping of a counterclockwise polygon (repeated indices allowed, for bridges). The ear
/// with the shortest longest edge goes first, which gives short diagonals and round triangles.
fn clip(
    uv: &[[f64; 2]],
    xyz: &[DVec3],
    scale: [f64; 2],
    poly: &[usize],
    fuel: u64,
    ops: &mut u64,
) -> Result<Vec<[u32; 3]>, Cut> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = poly.len();
    // The steps taken: ears tried and corners looked at.
    let work = std::cell::Cell::new(*ops);
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut alive = vec![true; n];
    let mut stamp = vec![0u32; n];
    let pt = |i: usize| uv[poly[i]];
    let convex = |p: usize, i: usize, q: usize| cross(pt(p), pt(i), pt(q)) > 0.0;
    // The reflex points, in a quadtree: a cluster of holes has many of them in a small place.
    let reflex0: Vec<usize> = (0..n).filter(|&i| !convex(prev[i], i, next[i])).collect();
    let reflex_tree = PointTree::build(&reflex0.iter().map(|&i| (pt(i), i)).collect::<Vec<_>>());
    work.set(work.get() + reflex0.len() as u64 * 4);
    let inside_any =
        |a: usize, b: usize, c: usize, prev: &[usize], next: &[usize], alive: &[bool]| -> bool {
            let (pa, pb, pc) = (pt(a), pt(b), pt(c));
            let mut steps = 0u64;
            let found = reflex_tree.any_in_triangle([pa, pb, pc], &mut steps, &mut |r| {
                if r == a || r == b || r == c || !alive[r] {
                    return false;
                }
                let q = pt(r);
                // The same position as a corner: a bridge copy, not a blocker.
                if poly[r] == poly[a] || poly[r] == poly[b] || poly[r] == poly[c] {
                    return false;
                }
                if q == pa || q == pb || q == pc {
                    return false;
                }
                // Still reflex now?
                !convex(prev[r], r, next[r])
            });
            work.set(work.get() + steps);
            found
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
        work.set(work.get() + left as u64 * HEAP_STEPS);
        if work.get() > fuel {
            return Err(Cut::Fuel(work.get()));
        }
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
            work.set(work.get() + HEAP_STEPS);
            if !alive[v] || stamp[v] != st || left <= 3 {
                continue;
            }
            if work.get() > fuel {
                return Err(Cut::Fuel(work.get()));
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
                    work.set(work.get() + HEAP_STEPS);
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
                return Err(Cut::Cannot);
            }
        }
    }
    // The last three points.
    let v = (0..n).find(|i| alive[*i]).ok_or(Cut::Cannot)?;
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
    *ops = work.get();
    if (want - got).abs() <= 1e-6 * want.abs().max(1e-300) {
        Ok(out)
    } else {
        Err(Cut::Cannot)
    }
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

    /// The triangles have positive areas that add up to the area of the region: they cover it
    /// once. Where holes touch, the edges of the triangles do not match by number.
    fn assert_area_once(uv: &[[f64; 2]], want: f64, t: &[[u32; 3]]) {
        let got = area_of(uv, t);
        assert!(
            (got - want).abs() <= 1e-9 * want.abs().max(1.0),
            "{got} {want}"
        );
        for tri in t {
            assert!(
                cross(
                    uv[tri[0] as usize],
                    uv[tri[1] as usize],
                    uv[tri[2] as usize]
                ) >= -1e-12,
                "a triangle looks the wrong way"
            );
        }
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

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(128))]
        #[test]
        fn random_touching_rows_are_covered_once(
            nx in 1usize..9,
            ny in 1usize..7,
            ri in 0usize..4,
            gi in 0usize..4,
            skip in 0u64..6,
        ) {
            // Sizes that add up exactly in binary, so that corners that should touch do.
            let (r, gap) = ([0.25, 0.5, 1.0, 2.0][ri], [0.125, 0.25, 0.5, 1.0][gi]);
            let (uv, loops, want) = touching_rows(nx, ny, r, gap, skip);
            let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops);
            proptest::prop_assert!(t.is_some(), "{} pairs x {} rows r {} gap {} skip {}", nx, ny, r, gap, skip);
            assert_area_once(&uv, want, &t.unwrap());
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

    /// A named polygon: its points and its loops.
    type Shape = (String, Vec<[f64; 2]>, Vec<Vec<usize>>);

    /// The polygons that the reviewers used to find the cost of the cutting, as (points, loops).
    fn shapes(big: bool) -> Vec<Shape> {
        let mut out = Vec::new();
        let holes_along =
            |name: &str, n: usize, pos: &dyn Fn(usize) -> (f64, f64, f64), outline: [f64; 4]| {
                let mut uv = vec![
                    [outline[0], outline[1]],
                    [outline[2], outline[1]],
                    [outline[2], outline[3]],
                    [outline[0], outline[3]],
                ];
                let mut loops = vec![vec![0, 1, 2, 3]];
                for i in 0..n {
                    let (x, y, side) = pos(i);
                    let base = uv.len();
                    uv.extend([[x, y], [x, y + side], [x + side, y + side], [x + side, y]]);
                    loops.push((base..base + 4).collect());
                }
                (name.to_string(), uv, loops)
            };
        let diagonals: &[usize] = if big {
            &[1000, 3000, 10_000, 30_000, 90_000]
        } else {
            &[100, 300]
        };
        for &n in diagonals {
            let s = 1000.0 / n as f64;
            out.push(holes_along(
                &format!("diagonal {n}"),
                n,
                &|i| (i as f64 * s, i as f64 * s, s / 2.0),
                [-1.0, -1.0, 1001.0, 1001.0],
            ));
        }
        out.push(holes_along(
            "row",
            if big { 10_000 } else { 200 },
            &|i| (i as f64 * 0.1 + 0.025, 0.5, 0.05),
            [0.0, 0.0, if big { 1000.0 } else { 20.0 }, 1.0],
        ));
        // A comb: teeth up from a base. n points.
        let comb = |teeth: usize| {
            let mut uv = vec![[0.0, 0.0], [teeth as f64 * 2.0, 0.0]];
            for t in (0..teeth).rev() {
                let x = t as f64 * 2.0;
                uv.push([x + 2.0, 10.0]);
                uv.push([x + 1.0, 10.0]);
                uv.push([x + 1.0, 1.0]);
                uv.push([x + 0.5, 1.0]);
                uv.push([x + 0.5, 10.0]);
                uv.push([x, 10.0]);
            }
            let n = uv.len();
            (uv, vec![(0..n).collect::<Vec<usize>>()])
        };
        let (uv, l) = comb(if big { 30_000 } else { 300 });
        out.push(("comb".to_string(), uv, l));
        // Holes of sizes from a thousandth to the cell, one in each cell of a grid.
        {
            let mut state = 0x9E37_79B9_7F4A_7C15u64;
            let mut rnd = move || {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 11) as f64 / (1u64 << 53) as f64
            };
            let (g, cell) = (32usize, 10_000.0 / 32.0);
            let mut uv = vec![
                [0.0, 0.0],
                [10_000.0, 0.0],
                [10_000.0, 10_000.0],
                [0.0, 10_000.0],
            ];
            let mut loops = vec![vec![0, 1, 2, 3]];
            for j in 0..g {
                for i in 0..g {
                    let side = (1e-3f64).powf(1.0 - rnd()) * (cell * 0.8f64).min(800.0);
                    let (x, y) = (
                        i as f64 * cell + rnd() * (cell - side),
                        j as f64 * cell + rnd() * (cell - side),
                    );
                    let base = uv.len();
                    uv.extend([[x, y], [x, y + side], [x + side, y + side], [x + side, y]]);
                    loops.push((base..base + 4).collect());
                }
            }
            out.push(("random 1024".to_string(), uv, loops));
        }
        let n = if big { 390_000usize } else { 3_900 };
        let ring = |f: &dyn Fn(usize) -> [f64; 2]| -> (Vec<[f64; 2]>, Vec<Vec<usize>>) {
            ((0..n).map(f).collect(), vec![(0..n).collect()])
        };
        let (uv, l) = ring(&|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [100.0 * a.cos(), 100.0 * a.sin()]
        });
        out.push(("circle".into(), uv, l));
        let (uv, l) = ring(&|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let r = if i % 2 == 0 { 100.0 } else { 60.0 };
            [r * a.cos(), r * a.sin()]
        });
        out.push(("star".into(), uv, l));
        let (uv, l) = ring(&|i| {
            // Zigzag teeth along the top of a long strip, counterclockwise: the bottom from
            // left to right, then the teeth from right to left.
            let half = n / 2;
            if i < half {
                [i as f64, -5.0]
            } else {
                let j = n - 1 - i;
                [j as f64, if j.is_multiple_of(2) { 0.0 } else { 1.0 }]
            }
        });
        out.push(("zigzag".into(), uv, l));
        out
    }

    /// Rows of diamond holes. In each row the first touches the left edge of the outline with
    /// a corner, and the others are in pairs that touch each other with a corner. Every hole
    /// touches one thing at one point, and the gaps keep the face in one piece.
    fn touching_rows(
        pairs: usize,
        ny: usize,
        r: f64,
        gap: f64,
        skip: u64,
    ) -> (Vec<[f64; 2]>, Vec<Vec<usize>>, f64) {
        let row_w = 2.0 * r + gap + pairs as f64 * (4.0 * r + gap);
        let (w, h) = (row_w, (2.0 * r + gap) * ny as f64 + gap);
        let mut uv = vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
        let mut loops = vec![vec![0, 1, 2, 3]];
        let mut want = w * h;
        for j in 0..ny {
            let cy = gap + r + (2.0 * r + gap) * j as f64;
            let mut centres = vec![r];
            for k in 0..pairs {
                if skip > 0 && ((k * 5 + j * 11) as u64).is_multiple_of(skip) {
                    continue;
                }
                let left = 2.0 * r + gap + k as f64 * (4.0 * r + gap);
                centres.push(left + r);
                centres.push(left + 3.0 * r);
            }
            for cx in centres {
                let base = uv.len();
                uv.extend([[cx - r, cy], [cx, cy + r], [cx + r, cy], [cx, cy - r]]);
                want -= 2.0 * r * r;
                loops.push((base..base + 4).collect());
            }
        }
        (uv, loops, want)
    }

    #[test]
    fn holes_that_touch_each_other_and_the_outline_are_cut() {
        // The case that the review shrank: a corner of the hole on a straight corner of the
        // outline.
        let mut uv = vec![
            [0.0, 0.0],
            [30.25, 0.0],
            [100.0, 0.0],
            [100.0, 40.75],
            [100.0, 100.0],
            [0.0, 100.0],
        ];
        let mut loops = vec![(0..6).collect::<Vec<usize>>()];
        let base = uv.len();
        uv.extend([[90.96, 36.23], [90.96, 45.27], [100.0, 40.75]]);
        loops.push((base..base + 3).collect());
        let want = 10_000.0 - 0.5 * 9.04 * 9.04;
        let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops).expect("the shrunk case");
        assert_area_once(&uv, want, &t);
        // Rows of diamonds.
        for (nx, ny, r, gap, skip) in [
            (1, 1, 1.0, 1.0, 0),
            (3, 2, 1.0, 1.0, 0),
            (6, 4, 0.5, 0.7, 0),
            (10, 10, 2.0, 1.0, 0),
            (8, 6, 1.0, 0.5, 3),
        ] {
            let (uv, loops, want) = touching_rows(nx, ny, r, gap, skip);
            let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops)
                .unwrap_or_else(|| panic!("diamonds {nx}x{ny}"));
            assert_area_once(&uv, want, &t);
        }
    }

    #[test]
    fn a_row_of_holes_is_cut() {
        for n in [2usize, 3, 5, 10, 50, 300] {
            let mut uv = vec![
                [0.0, 0.0],
                [n as f64 * 0.1, 0.0],
                [n as f64 * 0.1, 1.0],
                [0.0, 1.0],
            ];
            let mut loops = vec![vec![0, 1, 2, 3]];
            let mut want = n as f64 * 0.1;
            for i in 0..n {
                let (x, y, s) = (i as f64 * 0.1 + 0.025, 0.5, 0.05);
                let base = uv.len();
                uv.extend([[x, y], [x, y + s], [x + s, y + s], [x + s, y]]);
                want -= s * s;
                loops.push((base..base + 4).collect());
            }
            let t = clip_polygon(&uv, &flat3(&uv), [1.0, 1.0], &loops)
                .unwrap_or_else(|| panic!("row of {n}"));
            assert_covers(&uv, &loops, want, &t);
        }
    }

    /// Measures the steps and the time of the cutting. In a release build the shapes are the
    /// big ones of the reviews (`cargo test --release -p kadakas-step --lib measure_cutting --
    /// --nocapture` prints the table of `docs/interop.md`). Elsewhere they are small.
    #[test]
    fn measure_cutting() {
        for (name, uv, loops) in shapes(!cfg!(debug_assertions)) {
            let start = std::time::Instant::now();
            let r = ear_clip(&uv, &flat3(&uv), [1.0, 1.0], &loops, u64::MAX);
            let t = start.elapsed();
            let pts: usize = loops.iter().map(Vec::len).sum();
            let (tris, ops) = r.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert!(ops > 0, "{name}");
            println!(
                "{name}: {pts} points, {} holes: {t:?}, {ops} steps, {} triangles, {:.1} ns/step",
                loops.len() - 1,
                tris.len(),
                t.as_nanos() as f64 / ops as f64
            );
        }
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
