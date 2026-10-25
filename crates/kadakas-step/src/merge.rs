//! Joins the holes of a face to its outline, so that one polygon is left for ear clipping.
//!
//! The merged polygon is a ring of linked corners (a corner that a bridge doubles is two nodes),
//! and all segments (the outline, the holes and the bridges) are in a quadtree. A hole costs
//! the cells around its bridge and not the size of the polygon, also when the holes are packed
//! in a band. Every step is counted (`ops`), and the joining stops at a limit (`fuel`).
//!
//! A hole is joined by a bridge between its rightmost corner and a corner of the polygon that
//! it can see: the nearest corners first, then the corner of the ray to the right as usual.
//! Every choice is checked: the bridge must run inside the polygon at both ends and must not
//! meet any segment (the merged polygon with its bridges, the holes that wait, the hole
//! itself). A corner that the polygon has twice is taken at the copy that has the bridge
//! inside its angle. A hole that touches the polygon at a corner or on an edge is joined at the
//! point where it touches, with no bridge.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::mesh2d::{Cut, dist2};

/// Most segments that a leaf of the tree holds before it is cut in four.
const LEAF_MAX: usize = 12;
const MAX_DEPTH: u8 = 24;
/// The nearest corners that are tried for one hole before the ray is used.
const TRY_NEAR: usize = 24;
const NONE: u32 = u32::MAX;

fn cross0(u: [f64; 2], v: [f64; 2]) -> f64 {
    u[0] * v[1] - u[1] * v[0]
}

fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross3(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Does the direction from the corner `v` to `target` run into the inside of a counterclockwise
/// polygon (not along an edge)? `next` and `prev` are the neighbours of the corner.
pub(crate) fn in_wedge(v: [f64; 2], next: [f64; 2], prev: [f64; 2], target: [f64; 2]) -> bool {
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
pub(crate) fn segments_meet(p: [f64; 2], q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    if p[0].max(q[0]) < a[0].min(b[0])
        || a[0].max(b[0]) < p[0].min(q[0])
        || p[1].max(q[1]) < a[1].min(b[1])
        || a[1].max(b[1]) < p[1].min(q[1])
    {
        return false;
    }
    let o1 = cross3(p, q, a);
    let o2 = cross3(p, q, b);
    let o3 = cross3(a, b, p);
    let o4 = cross3(a, b, q);
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
    /// The node of the ring where the edge starts, for an edge of a loop (a bridge has none).
    node: u32,
}

struct Node {
    lo: [f64; 2],
    hi: [f64; 2],
    kids: [u32; 4],
    segs: Vec<u32>,
    depth: u8,
}

/// A quadtree of segments. A segment is in every leaf that it touches.
struct QTree {
    nodes: Vec<Node>,
    eps: f64,
}

/// Does the segment ab touch the box (a little wider than it is)?
fn seg_in_box(lo: [f64; 2], hi: [f64; 2], eps: f64, a: [f64; 2], b: [f64; 2]) -> bool {
    let (lo, hi) = ([lo[0] - eps, lo[1] - eps], [hi[0] + eps, hi[1] + eps]);
    if a[0].max(b[0]) < lo[0]
        || a[0].min(b[0]) > hi[0]
        || a[1].max(b[1]) < lo[1]
        || a[1].min(b[1]) > hi[1]
    {
        return false;
    }
    // The corners of the box are all on one side of the line: the line misses the box.
    let d = sub2(b, a);
    let side = |c: [f64; 2]| cross0(d, sub2(c, a));
    let s = [
        side(lo),
        side([hi[0], lo[1]]),
        side(hi),
        side([lo[0], hi[1]]),
    ];
    !(s.iter().all(|x| *x > 0.0) || s.iter().all(|x| *x < 0.0))
}

impl QTree {
    fn new(lo: [f64; 2], hi: [f64; 2]) -> QTree {
        let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-300);
        QTree {
            nodes: vec![Node {
                lo,
                hi: [lo[0] + size, lo[1] + size],
                kids: [NONE; 4],
                segs: Vec::new(),
                depth: 0,
            }],
            eps: size * 1e-9,
        }
    }

    fn is_leaf(&self, ni: usize) -> bool {
        self.nodes[ni].kids[0] == NONE
    }

    fn split(&mut self, ni: usize, segs: &[Seg]) {
        let (lo, hi, depth) = {
            let n = &self.nodes[ni];
            (n.lo, n.hi, n.depth)
        };
        let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let base = self.nodes.len() as u32;
        for k in 0..4 {
            let (xh, yh) = (k % 2, k / 2);
            self.nodes.push(Node {
                lo: [
                    if xh == 0 { lo[0] } else { mid[0] },
                    if yh == 0 { lo[1] } else { mid[1] },
                ],
                hi: [
                    if xh == 0 { mid[0] } else { hi[0] },
                    if yh == 0 { mid[1] } else { hi[1] },
                ],
                kids: [NONE; 4],
                segs: Vec::new(),
                depth: depth + 1,
            });
        }
        let old = std::mem::take(&mut self.nodes[ni].segs);
        self.nodes[ni].kids = [base, base + 1, base + 2, base + 3];
        for id in old {
            let s = &segs[id as usize];
            for k in 0..4 {
                let kid = (base + k) as usize;
                if seg_in_box(self.nodes[kid].lo, self.nodes[kid].hi, self.eps, s.a, s.b) {
                    self.nodes[kid].segs.push(id);
                }
            }
        }
        for k in 0..4 {
            let kid = (base + k) as usize;
            if self.nodes[kid].segs.len() > LEAF_MAX && depth + 1 < MAX_DEPTH {
                self.split(kid, segs);
            }
        }
    }

    /// Puts segment `id` in the leaves that it touches. Returns the nodes that were looked at.
    fn insert(&mut self, segs: &[Seg], id: u32) -> u64 {
        let (a, b) = (segs[id as usize].a, segs[id as usize].b);
        let mut stack = vec![0usize];
        let mut seen = 0u64;
        while let Some(ni) = stack.pop() {
            seen += 1;
            let (lo, hi) = (self.nodes[ni].lo, self.nodes[ni].hi);
            if !seg_in_box(lo, hi, self.eps, a, b) {
                continue;
            }
            if self.is_leaf(ni) {
                self.nodes[ni].segs.push(id);
                if self.nodes[ni].segs.len() > LEAF_MAX && self.nodes[ni].depth < MAX_DEPTH {
                    self.split(ni, segs);
                }
            } else {
                stack.extend(self.nodes[ni].kids.iter().map(|k| *k as usize));
            }
        }
        seen
    }

    /// Calls `f` with the segments of the leaves that the segment ab touches, until `f` says
    /// stop. Returns whether it stopped and the steps taken.
    fn visit_segment(
        &self,
        a: [f64; 2],
        b: [f64; 2],
        f: &mut dyn FnMut(u32) -> bool,
    ) -> (bool, u64) {
        let mut stack = vec![0usize];
        let mut ops = 0u64;
        while let Some(ni) = stack.pop() {
            ops += 1;
            let n = &self.nodes[ni];
            if !seg_in_box(n.lo, n.hi, self.eps, a, b) {
                continue;
            }
            if n.kids[0] == NONE {
                for &id in &n.segs {
                    ops += 1;
                    if f(id) {
                        return (true, ops);
                    }
                }
            } else {
                stack.extend(n.kids.iter().map(|k| *k as usize));
            }
        }
        (false, ops)
    }

    /// The first segment of the merged polygon that a ray to the right of `m` meets: its
    /// crossing and its id. Leaves are visited from the left, so the first leaf with a hit has
    /// the nearest one.
    fn ray(
        &self,
        segs: &[Seg],
        ni: usize,
        m: [f64; 2],
        best: &mut Option<(f64, u32)>,
        ops: &mut u64,
    ) -> bool {
        let n = &self.nodes[ni];
        *ops += 1;
        if m[1] < n.lo[1] - self.eps || m[1] > n.hi[1] + self.eps || n.hi[0] < m[0] - self.eps {
            return false;
        }
        if n.kids[0] == NONE {
            for &id in &n.segs {
                *ops += 1;
                let s = &segs[id as usize];
                let (a, b) = (s.a, s.b);
                if !s.merged || (a[1] - m[1]) * (b[1] - m[1]) > 0.0 || a[1] == b[1] {
                    continue;
                }
                let x = a[0] + (m[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if x >= m[0] - 1e-12 && x <= n.hi[0] + self.eps && best.is_none_or(|(bx, _)| x < bx)
                {
                    *best = Some((x, id));
                }
            }
            return best.is_some();
        }
        for xh in 0..2 {
            let mut found = false;
            for yh in 0..2 {
                found |= self.ray(segs, n.kids[yh * 2 + xh] as usize, m, best, ops);
            }
            if found {
                return true;
            }
        }
        false
    }
}

/// The merged polygon, the holes that wait and the tree of all the segments.
pub(crate) struct Merger<'a> {
    uv: &'a [[f64; 2]],
    holes: &'a [&'a Vec<usize>],
    /// The ring: the index in `uv` of each node and its neighbours.
    vert: Vec<usize>,
    next: Vec<u32>,
    prev: Vec<u32>,
    /// The nodes of each corner: the last made, and the one made before it.
    first: HashMap<usize, u32>,
    same: Vec<u32>,
    segs: Vec<Seg>,
    tree: QTree,
    /// Which corner was met in a search, to meet it once.
    stamp: Vec<u32>,
    epoch: u32,
    /// Steps taken (nodes visited, segments tested) and the most that are allowed.
    pub ops: u64,
    fuel: u64,
    /// The start node of the ring.
    head: u32,
    /// The segments of each hole are `hole_segs[k]..hole_segs[k + 1]`.
    hole_segs: Vec<usize>,
}

impl<'a> Merger<'a> {
    pub fn new(
        uv: &'a [[f64; 2]],
        outline: &[usize],
        holes: &'a [&'a Vec<usize>],
        fuel: u64,
    ) -> Self {
        let edges = outline.len() + holes.iter().map(|h| h.len()).sum::<usize>();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for &i in outline.iter().chain(holes.iter().flat_map(|h| h.iter())) {
            for k in 0..2 {
                lo[k] = lo[k].min(uv[i][k]);
                hi[k] = hi[k].max(uv[i][k]);
            }
        }
        let mut m = Merger {
            uv,
            holes,
            vert: Vec::with_capacity(edges + 2 * holes.len() + 2),
            next: Vec::new(),
            prev: Vec::new(),
            first: HashMap::new(),
            same: Vec::new(),
            segs: Vec::with_capacity(edges + holes.len()),
            tree: QTree::new(lo, hi),
            stamp: vec![0; uv.len()],
            epoch: 0,
            ops: 0,
            fuel,
            head: 0,
            hole_segs: Vec::new(),
        };
        m.head = m.ring(outline);
        m.add_ring_segments(outline, true, m.head);
        for h in holes {
            m.hole_segs.push(m.segs.len());
            m.add_ring_segments(h, false, NONE);
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

    /// The segments of a loop. `first_node` is the node where the first one starts (the nodes
    /// of a ring are in order), or none.
    fn add_ring_segments(&mut self, l: &[usize], merged: bool, first_node: u32) {
        for j in 0..l.len() {
            let (va, vb) = (l[j], l[(j + 1) % l.len()]);
            let node = if first_node == NONE {
                NONE
            } else {
                first_node + j as u32
            };
            self.add_seg(va, vb, merged, node);
        }
    }

    fn add_seg(&mut self, va: usize, vb: usize, merged: bool, node: u32) -> u32 {
        let id = self.segs.len() as u32;
        let (a, b) = (self.uv[va], self.uv[vb]);
        self.segs.push(Seg {
            a,
            b,
            va,
            vb,
            merged,
            node,
        });
        let seen = self.tree.insert(&self.segs, id);
        self.ops += seen;
        id
    }

    /// Counts steps and stops at the limit.
    fn spend(&mut self, n: u64) -> Result<(), Cut> {
        self.ops += n;
        if self.ops > self.fuel {
            Err(Cut::Fuel(self.ops))
        } else {
            Ok(())
        }
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

    fn new_stamp(&mut self) -> u32 {
        self.epoch += 1;
        self.epoch
    }

    /// Does the segment from `m` to `v` meet a segment of the polygon, of a waiting hole or of
    /// the hole itself, other than the ones that end at `m` or `v`?
    fn blocked(&mut self, m: [f64; 2], v: [f64; 2]) -> Result<bool, Cut> {
        let segs = &self.segs;
        let (hit, ops) = self.tree.visit_segment(m, v, &mut |id| {
            let s = &segs[id as usize];
            let (a, b) = (s.a, s.b);
            if a == m || b == m || a == v || b == v {
                return false;
            }
            segments_meet(m, v, a, b)
        });
        self.spend(ops)?;
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

    /// The `count` nearest corners of the merged polygon to `m`, nearest first, by a search of
    /// the tree from the nearest box.
    fn nearest(&mut self, m: [f64; 2], count: usize) -> Result<Vec<usize>, Cut> {
        // Entries: a box (kind 0) or a corner (kind 1), by distance.
        let mut heap: BinaryHeap<Reverse<(u64, u8, usize)>> = BinaryHeap::new();
        let box_d2 = |n: &Node| {
            let dx = (n.lo[0] - m[0]).max(0.0).max(m[0] - n.hi[0]);
            let dy = (n.lo[1] - m[1]).max(0.0).max(m[1] - n.hi[1]);
            dx * dx + dy * dy
        };
        heap.push(Reverse((box_d2(&self.tree.nodes[0]).to_bits(), 0, 0)));
        let epoch = self.new_stamp();
        let mut out = Vec::new();
        let mut ops = 0u64;
        while let Some(Reverse((_, kind, idx))) = heap.pop() {
            ops += 1;
            if kind == 1 {
                out.push(idx);
                if out.len() >= count {
                    break;
                }
                continue;
            }
            let n = &self.tree.nodes[idx];
            if n.kids[0] == NONE {
                for &id in &n.segs {
                    ops += 1;
                    let s = &self.segs[id as usize];
                    if !s.merged {
                        continue;
                    }
                    for (q, v) in [(s.a, s.va), (s.b, s.vb)] {
                        if self.stamp[v] != epoch {
                            self.stamp[v] = epoch;
                            heap.push(Reverse((dist2(m, q).to_bits(), 1, v)));
                        }
                    }
                }
            } else {
                for &k in &n.kids {
                    heap.push(Reverse((
                        box_d2(&self.tree.nodes[k as usize]).to_bits(),
                        0,
                        k as usize,
                    )));
                }
            }
            if ops > self.fuel.saturating_sub(self.ops) {
                break;
            }
        }
        self.spend(ops)?;
        Ok(out)
    }

    /// The first node among the corners `vs` that the hole corner can be joined to.
    fn first_fit(
        &mut self,
        vs: &[usize],
        m: [f64; 2],
        hp: [f64; 2],
        hn: [f64; 2],
    ) -> Result<Option<u32>, Cut> {
        for &v in vs {
            for node in self.nodes_of(v) {
                if self.fits(node, m, hp, hn)? {
                    return Ok(Some(node));
                }
            }
        }
        Ok(None)
    }

    /// The ends of the segment that the ray to the right of `m` hits first: the one with the
    /// larger x is the corner of the usual rule, the other one is tried too.
    fn ray_corners(&mut self, m: [f64; 2]) -> Result<Vec<usize>, Cut> {
        let mut best: Option<(f64, u32)> = None;
        let mut ops = 0u64;
        self.tree.ray(&self.segs, 0, m, &mut best, &mut ops);
        self.spend(ops)?;
        let Some((_, si)) = best else {
            return Ok(Vec::new());
        };
        let s = &self.segs[si as usize];
        Ok(if s.a[0] > s.b[0] {
            vec![s.va, s.vb]
        } else {
            vec![s.vb, s.va]
        })
    }

    /// Joins the hole `k` to the polygon.
    pub fn join(&mut self, k: usize) -> Result<(), Cut> {
        let hole = self.holes[k];
        let h = hole.len();
        if self.touch(k)? {
            return Ok(());
        }
        let (mi, _) = hole
            .iter()
            .enumerate()
            .max_by(|a, b| self.uv[*a.1][0].total_cmp(&self.uv[*b.1][0]))
            .ok_or(Cut::Cannot)?;
        let m = self.uv[hole[mi]];
        let (hp, hn) = (self.uv[hole[(mi + h - 1) % h]], self.uv[hole[(mi + 1) % h]]);
        let near = self.nearest(m, TRY_NEAR)?;
        let mut found = self.first_fit(&near, m, hp, hn)?;
        if found.is_none() {
            let ray = self.ray_corners(m)?;
            found = self.first_fit(&ray, m, hp, hn)?;
        }
        if found.is_none() {
            // All the corners, nearest first. The steps are counted, so this ends.
            let all = self.nearest(m, usize::MAX)?;
            found = self.first_fit(&all[near.len().min(all.len())..], m, hp, hn)?;
        }
        let node = found.ok_or(Cut::Cannot)?;
        self.splice(node, hole, mi, Some(k));
        // The bridge is two edges of the polygon.
        let v = self.vert[node as usize];
        self.add_seg(v, hole[mi], true, NONE);
        self.merge_hole_segments(k);
        self.spend(0)
    }

    /// The hole's own segments are part of the polygon from now on.
    fn merge_hole_segments(&mut self, k: usize) {
        for s in self.hole_segs[k]..self.hole_segs[k + 1] {
            self.segs[s].merged = true;
        }
    }

    /// A hole that touches the polygon at one of its corners, at a corner of the polygon or in
    /// the middle of an edge, is joined there, with no bridge. True when it was joined.
    fn touch(&mut self, k: usize) -> Result<bool, Cut> {
        let hole = self.holes[k];
        let h = hole.len();
        for j in 0..h {
            let p = self.uv[hole[j]];
            let (hp, hn) = (self.uv[hole[(j + h - 1) % h]], self.uv[hole[(j + 1) % h]]);
            // The merged segments that p is on.
            let mut on: Vec<u32> = Vec::new();
            let segs = &self.segs;
            let (_, ops) = self.tree.visit_segment(p, p, &mut |id| {
                let s = &segs[id as usize];
                if s.merged && cross3(s.a, s.b, p) == 0.0 && on_segment(s.a, s.b, p) {
                    on.push(id);
                }
                false
            });
            self.spend(ops)?;
            on.sort_unstable();
            on.dedup();
            for id in on {
                let (a, b, va, vb, node) = {
                    let s = &self.segs[id as usize];
                    (s.a, s.b, s.va, s.vb, s.node)
                };
                // At a corner of the polygon: the nodes that are there.
                let corner = if a == p {
                    Some(va)
                } else if b == p {
                    Some(vb)
                } else {
                    None
                };
                let at = match corner {
                    Some(v) => self
                        .nodes_of(v)
                        .into_iter()
                        .find(|&n| self.inside_both(n, hp, hn)),
                    None if node != NONE && self.next[node as usize] != NONE => {
                        // In the middle of an edge of the ring: a new corner is made there.
                        let x = self.node(hole[j]);
                        let after = self.next[node as usize];
                        self.next[node as usize] = x;
                        self.prev[x as usize] = node;
                        self.next[x as usize] = after;
                        self.prev[after as usize] = x;
                        self.segs[id as usize].node = NONE;
                        let vx = hole[j];
                        let first = self.add_seg(va, vx, true, node);
                        let _ = first;
                        self.add_seg(vx, vb, true, x);
                        if self.inside_both(x, hp, hn) {
                            Some(x)
                        } else {
                            // The hole is on the other side: the corner stays, harmless.
                            None
                        }
                    }
                    None => None,
                };
                if let Some(node) = at {
                    self.touching_splice(node, hole, j, k);
                    self.merge_hole_segments(k);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Do both neighbours of a hole corner at this node lie inside the polygon there?
    fn inside_both(&self, n: u32, hp: [f64; 2], hn: [f64; 2]) -> bool {
        let v = self.uv[self.vert[n as usize]];
        let (pp, pn) = (
            self.uv[self.vert[self.prev[n as usize] as usize]],
            self.uv[self.vert[self.next[n as usize] as usize]],
        );
        in_wedge(v, pn, pp, hn) && in_wedge(v, pn, pp, hp)
    }

    /// Puts the hole into the ring after `node`: the node, the hole from its corner `j` round
    /// to the corner again, and the node again (a bridge). The edges of the hole learn the
    /// nodes where they start.
    fn splice(&mut self, node: u32, hole: &[usize], j: usize, k: Option<usize>) {
        let h = hole.len();
        let after = self.next[node as usize];
        let mut prev = node;
        for i in 0..=h {
            let id = self.node(hole[(j + i) % h]);
            self.next[prev as usize] = id;
            self.prev[id as usize] = prev;
            prev = id;
            if let (Some(k), true) = (k, i < h) {
                let seg = self.hole_segs[k] + (j + i) % h;
                self.segs[seg].node = id;
            }
        }
        let again = self.node(self.vert[node as usize]);
        self.next[prev as usize] = again;
        self.prev[again as usize] = prev;
        self.next[again as usize] = after;
        self.prev[after as usize] = again;
    }

    /// Puts the hole into the ring where it touches the polygon at the corner `j`: the node,
    /// the hole from the corner after `j` round to the corner before it, and the node again.
    fn touching_splice(&mut self, node: u32, hole: &[usize], j: usize, k: usize) {
        let h = hole.len();
        let after = self.next[node as usize];
        let mut prev = node;
        // The edge that starts at the corner j starts at the joint.
        let first = self.hole_segs[k];
        self.segs[first + j].node = node;
        for i in 1..h {
            let id = self.node(hole[(j + i) % h]);
            self.next[prev as usize] = id;
            self.prev[id as usize] = prev;
            prev = id;
            self.segs[first + (j + i) % h].node = id;
        }
        let again = self.node(self.vert[node as usize]);
        self.next[prev as usize] = again;
        self.prev[again as usize] = prev;
        self.next[again as usize] = after;
        self.prev[after as usize] = again;
    }

    /// The corners of the ring in order, from the head.
    pub fn polygon(&self) -> Vec<usize> {
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
