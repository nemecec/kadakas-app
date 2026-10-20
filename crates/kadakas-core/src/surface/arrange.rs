//! The regions of the edges in one plane.
//!
//! The edges of a model never cross and never pass through a vertex (an edit splits them), so the
//! edges that lie in a plane form a planar straight-line graph. Seen in the 2D axes of the plane,
//! each directed edge has one region on its left. The walk that keeps the region on its left takes,
//! at each vertex, the edge that turns most sharply to the left: the first edge clockwise from the
//! way back. Every directed edge lies on exactly one such cycle.
//!
//! - An edge with the same cycle on both sides (a stray edge, or a bridge between two loops) is
//!   inside one region. It is taken out and the walk runs again, so loops never hold such edges.
//! - A cycle that visits a vertex twice (two loops that touch at a vertex) is cut there.
//! - A cycle with a positive area (counterclockwise) is the outer loop of a region. A cycle with a
//!   negative area is an inner loop; it belongs to the smallest outer loop around it, or to the
//!   unbounded region when there is none.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec2;

use super::geom::{self, Plane};
use super::topo::Topo;
use super::{SurfaceError, SurfaceModel, VertexId};

/// A bounded region of a plane: the outer loop counterclockwise and the holes clockwise in the 2D
/// axes of the plane. Each loop starts at its smallest vertex id.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Region {
    pub outer: Vec<VertexId>,
    pub holes: Vec<Vec<VertexId>>,
    /// The area of the outer loop less the holes.
    pub area: f64,
}

impl Region {
    pub fn loops(&self) -> impl Iterator<Item = &Vec<VertexId>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }

    /// The directed edges of all loops.
    pub fn half_edges(&self) -> impl Iterator<Item = (VertexId, VertexId)> + '_ {
        self.loops()
            .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
    }
}

/// The regions of one plane and the 2D points of its vertices.
pub(super) struct Arrangement {
    pub pts: BTreeMap<VertexId, DVec2>,
    pub regions: Vec<Region>,
}

/// The 2D graph of the edges of one plane, with the edges at each vertex sorted by angle.
struct Graph {
    adj: BTreeMap<VertexId, Vec<VertexId>>,
    /// The place of `b` in the list of `a`.
    place: BTreeMap<(VertexId, VertexId), usize>,
}

impl Graph {
    fn new(pts: &BTreeMap<VertexId, DVec2>, edges: &BTreeSet<(VertexId, VertexId)>) -> Self {
        let mut adj: BTreeMap<VertexId, Vec<VertexId>> = BTreeMap::new();
        for &(a, b) in edges {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        let mut place = BTreeMap::new();
        for (v, list) in adj.iter_mut() {
            sort_around(pts[v], list, |w| pts[&w]);
            for (i, w) in list.iter().enumerate() {
                place.insert((*v, *w), i);
            }
        }
        Graph { adj, place }
    }

    /// The vertex after `y` on the walk that came from `x`.
    fn next(&self, x: VertexId, y: VertexId) -> VertexId {
        let list = &self.adj[&y];
        let k = self.place[&(y, x)];
        list[(k + list.len() - 1) % list.len()]
    }

    /// Every cycle of the walk, as the list of its vertices (each one goes to the next).
    fn cycles(&self) -> Vec<Vec<VertexId>> {
        let mut seen: BTreeSet<(VertexId, VertexId)> = BTreeSet::new();
        let mut out = Vec::new();
        for (&y, list) in &self.adj {
            for &z in list {
                if seen.contains(&(y, z)) {
                    continue;
                }
                let mut cycle = Vec::new();
                let (mut a, mut b) = (y, z);
                loop {
                    seen.insert((a, b));
                    cycle.push(a);
                    let c = self.next(a, b);
                    (a, b) = (b, c);
                    if (a, b) == (y, z) {
                        break;
                    }
                }
                out.push(cycle);
            }
        }
        out
    }
}

/// Sorts the neighbours of a vertex at `o` by the angle of their direction (counterclockwise from
/// +u), and by id when two angles are equal.
fn sort_around(o: DVec2, list: &mut [VertexId], pos: impl Fn(VertexId) -> DVec2) {
    list.sort_by(|a, b| {
        let (da, db) = (pos(*a) - o, pos(*b) - o);
        da.y.atan2(da.x).total_cmp(&db.y.atan2(db.x)).then(a.cmp(b))
    });
}

/// Cuts a cycle where it visits a vertex twice.
fn split_pinches(cycle: &[VertexId]) -> Vec<Vec<VertexId>> {
    let mut out = Vec::new();
    let mut stack: Vec<VertexId> = Vec::new();
    let mut at: BTreeMap<VertexId, usize> = BTreeMap::new();
    for &v in cycle {
        if let Some(&i) = at.get(&v) {
            let sub = stack.split_off(i);
            for u in &sub {
                at.remove(u);
            }
            out.push(sub);
        }
        at.insert(v, stack.len());
        stack.push(v);
    }
    out.push(stack);
    out.retain(|c| c.len() >= 3);
    out
}

/// The loop turned so that it starts at its smallest vertex id (the direction stays).
pub(super) fn canonical(l: &[VertexId]) -> Vec<VertexId> {
    let Some(k) = (0..l.len()).min_by_key(|i| l[*i]) else {
        return Vec::new();
    };
    l[k..].iter().chain(&l[..k]).copied().collect()
}

/// The loop in the other direction, still starting at its smallest vertex id.
pub(super) fn reversed(l: &[VertexId]) -> Vec<VertexId> {
    let r: Vec<VertexId> = l.iter().rev().copied().collect();
    canonical(&r)
}

/// True if `p` is inside the area of the loops (even-odd rule: inside the outer loop and outside
/// the holes).
pub(super) fn inside(p: DVec2, rings: &[Vec<DVec2>]) -> bool {
    let mut odd = false;
    for ring in rings {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    odd = !odd;
                }
            }
        }
    }
    odd
}

impl Arrangement {
    /// The regions of the edges `edges` (vertex pairs) seen in `plane`.
    pub fn solve(
        m: &SurfaceModel,
        plane: Plane,
        edges: &BTreeSet<(VertexId, VertexId)>,
    ) -> Arrangement {
        let mut pts = BTreeMap::new();
        for &(a, b) in edges {
            for v in [a, b] {
                pts.entry(v)
                    .or_insert_with(|| plane.project(m.vertices[&v]));
            }
        }
        // First walk: find the edges with one cycle on both sides and take them out.
        let first = Graph::new(&pts, edges);
        let cycles = first.cycles();
        let mut cycle_of: BTreeMap<(VertexId, VertexId), usize> = BTreeMap::new();
        for (i, c) in cycles.iter().enumerate() {
            for k in 0..c.len() {
                cycle_of.insert((c[k], c[(k + 1) % c.len()]), i);
            }
        }
        let kept: BTreeSet<(VertexId, VertexId)> = edges
            .iter()
            .copied()
            .filter(|&(a, b)| cycle_of[&(a, b)] != cycle_of[&(b, a)])
            .collect();
        let graph = Graph::new(&pts, &kept);
        let ring = |l: &[VertexId]| l.iter().map(|v| pts[v]).collect::<Vec<DVec2>>();
        let mut outers: Vec<(Vec<VertexId>, f64)> = Vec::new();
        let mut holes: Vec<(Vec<VertexId>, f64)> = Vec::new();
        for c in graph.cycles() {
            for sub in split_pinches(&c) {
                let a = geom::area2(&ring(&sub)) / 2.0;
                let sub = canonical(&sub);
                if a > 0.0 {
                    outers.push((sub, a));
                } else if a < 0.0 {
                    holes.push((sub, a));
                }
            }
        }
        outers.sort_by(|x, y| x.0.cmp(&y.0));
        holes.sort_by(|x, y| x.0.cmp(&y.0));
        let boxes: Vec<(DVec2, DVec2)> = outers.iter().map(|(o, _)| box_of(&ring(o))).collect();
        let outer_edges: Vec<BTreeSet<(VertexId, VertexId)>> = outers
            .iter()
            .map(|(o, _)| {
                (0..o.len())
                    .map(|i| super::key(o[i], o[(i + 1) % o.len()]))
                    .collect()
            })
            .collect();
        let mut owned: Vec<Vec<usize>> = vec![Vec::new(); outers.len()];
        for (h, (hole, _)) in holes.iter().enumerate() {
            // The middle of the first edge of the hole. It lies on no outer loop but the loop of
            // the region inside the hole, which holds that edge and is skipped.
            let probe = (pts[&hole[0]] + pts[&hole[1]]) / 2.0;
            let e = super::key(hole[0], hole[1]);
            let best = (0..outers.len())
                .filter(|&o| {
                    let (lo, hi) = boxes[o];
                    probe.cmpge(lo).all() && probe.cmple(hi).all() && !outer_edges[o].contains(&e)
                })
                .filter(|&o| inside(probe, &[ring(&outers[o].0)]))
                .min_by(|&a, &b| outers[a].1.total_cmp(&outers[b].1).then(a.cmp(&b)));
            if let Some(o) = best {
                owned[o].push(h);
            }
        }
        let regions = outers
            .into_iter()
            .zip(owned)
            .map(|((outer, a), hs)| {
                let area = a + hs.iter().map(|&h| holes[h].1).sum::<f64>();
                Region {
                    outer,
                    holes: hs.iter().map(|&h| holes[h].0.clone()).collect(),
                    area,
                }
            })
            .collect();
        Arrangement { pts, regions }
    }

    pub fn rings(&self, r: &Region) -> Vec<Vec<DVec2>> {
        r.loops()
            .map(|l| l.iter().map(|v| self.pts[v]).collect())
            .collect()
    }

    /// A point well inside a region: the middle of its largest triangle.
    pub fn sample(&self, r: &Region) -> Result<DVec2, SurfaceError> {
        let rings = self.rings(r);
        let flat: Vec<DVec2> = rings.iter().flatten().copied().collect();
        let tris = geom::triangulate_rings(&rings).unwrap_or_default();
        tris.iter()
            .map(|t| {
                let (a, b, c) = (
                    flat[t[0] as usize],
                    flat[t[1] as usize],
                    flat[t[2] as usize],
                );
                ((b - a).perp_dot(c - a), (a + b + c) / 3.0)
            })
            .filter(|(area, _)| *area > 0.0)
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, p)| p)
            .ok_or_else(|| SurfaceError::Unsolved("a region cannot be cut into triangles".into()))
    }
}

/// The box of 2D points.
pub(super) fn box_of(points: &[DVec2]) -> (DVec2, DVec2) {
    points.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    )
}

/// The 2D box of the cycle on the left of `a`→`b` in `plane` when that cycle closes a bounded
/// region (a positive area), else `None`. Only the edges at the vertices on the way are looked at.
pub(super) fn closes_on_left(
    m: &SurfaceModel,
    topo: &Topo,
    plane: &Plane,
    a: VertexId,
    b: VertexId,
) -> Option<(DVec2, DVec2)> {
    let mut pts: BTreeMap<VertexId, DVec2> = BTreeMap::new();
    let mut around: BTreeMap<VertexId, Vec<VertexId>> = BTreeMap::new();
    let point = |v: VertexId, pts: &mut BTreeMap<VertexId, DVec2>| {
        *pts.entry(v)
            .or_insert_with(|| plane.project(m.vertices[&v]))
    };
    let start = (a, b);
    let (mut x, mut y) = start;
    let mut ring = vec![point(x, &mut pts)];
    // Each directed edge comes once at most; this bound only guards against a broken model.
    for _ in 0..=2 * m.edges.len() + 2 {
        if let std::collections::btree_map::Entry::Vacant(slot) = around.entry(y) {
            let mut list: Vec<VertexId> = topo
                .edges_at(y)
                .filter_map(|e| m.edges[&e].other(y))
                .filter(|w| geom::on_plane(plane, m.vertices[w]))
                .collect();
            for w in &list {
                point(*w, &mut pts);
            }
            let o = point(y, &mut pts);
            sort_around(o, &mut list, |w| pts[&w]);
            slot.insert(list);
        }
        let list = &around[&y];
        let k = list.iter().position(|w| *w == x)?;
        let z = list[(k + list.len() - 1) % list.len()];
        (x, y) = (y, z);
        if (x, y) == start {
            return (geom::area2(&ring) > 0.0).then(|| box_of(&ring));
        }
        ring.push(point(x, &mut pts));
    }
    None
}
