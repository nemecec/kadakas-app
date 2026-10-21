//! The spatial index of one edit: box trees of the vertices, the edges and the faces, kept in step
//! with the model while the edit runs (it only grows: what a query finds is tested again against
//! the model). It replaces the scans of all vertices, edges and faces that each new edge (and each
//! merge of an erase) needs, so that an edit of `k` edges in a model of `n` takes about
//! `O(n log n + k log n)` time and not `O(k n)`. Each tree is built when it is first asked.
//!
//! The index only finds candidates; every test of the edit stays as it is, on the same numbers, in
//! the same order (candidates in id order, as the scans went). So the result is the same as with
//! the scans, to the last bit. A query must therefore never miss an element that the scan would
//! accept: each query box is grown by the largest length tolerance in the model, and the boxes of
//! edges and faces are grown so that a query in the axes of a plane finds them (see
//! [`Near::lifted`]).

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use super::super::arrange;
use super::super::geom::{self, Plane};
use super::super::index::{BoxTree, Boxes};
use super::super::topo::Topo;
use super::super::{EdgeId, Face, FaceId, SurfaceModel, VertexId};

pub(super) struct Near {
    vertices: OnceCell<Boxes>,
    edges: OnceCell<Boxes>,
    faces: OnceCell<Boxes>,
    /// The largest size (largest absolute coordinate) of a vertex so far.
    size: f64,
    /// The vertex uses in the loops of all faces (for the limits).
    pub loops: usize,
}

impl Near {
    pub fn of(m: &SurfaceModel) -> Near {
        Near {
            vertices: OnceCell::new(),
            edges: OnceCell::new(),
            faces: OnceCell::new(),
            size: m
                .vertices
                .values()
                .map(|p| geom::size(*p))
                .fold(0.0, f64::max),
            loops: m.faces.values().map(Face::loop_len).sum(),
        }
    }

    /// The largest length tolerance of any vertex so far and of a point of size `size`. Every
    /// tolerance of a test is at most this.
    pub fn tol(&self, size: f64) -> f64 {
        geom::tolerance_at(self.size.max(size))
    }

    // A tree that is not built yet needs no change: its build will see the model as it is then.

    pub fn add_vertex(&mut self, id: VertexId, p: DVec3) {
        self.size = self.size.max(geom::size(p));
        if let Some(t) = self.vertices.get_mut() {
            t.insert(p, p, id.0);
        }
    }

    /// Records the current box of an edge (a new edge, or one that changed its ends).
    pub fn add_edge(&mut self, id: EdgeId, a: DVec3, b: DVec3) {
        if let Some(t) = self.edges.get_mut() {
            let (lo, hi) = edge_box(a, b);
            t.insert(lo, hi, id.0);
        }
    }

    /// Records the current box of a face (a new face, or one that changed its loops).
    pub fn add_face(&mut self, id: FaceId, f: &Face, pos: impl Fn(VertexId) -> DVec3) {
        if let Some(t) = self.faces.get_mut()
            && let Some((lo, hi)) = face_box(f, pos)
        {
            t.insert(lo, hi, id.0);
        }
    }

    /// The vertices that may lie in the box `lo`–`hi`, in id order.
    pub fn vertices_in(&self, m: &SurfaceModel, lo: DVec3, hi: DVec3) -> Vec<VertexId> {
        let t = self
            .vertices
            .get_or_init(|| Boxes::new(m.vertices.iter().map(|(id, p)| (*p, *p, id.0)).collect()));
        t.ids_meeting(lo, hi).into_iter().map(VertexId).collect()
    }

    /// The edges that may meet the box `lo`–`hi`, in id order (some may not exist any more).
    pub fn edges_in(&self, m: &SurfaceModel, lo: DVec3, hi: DVec3) -> Vec<EdgeId> {
        let t = self.edges.get_or_init(|| {
            Boxes::new(
                m.edges
                    .iter()
                    .map(|(id, e)| {
                        let (lo, hi) = edge_box(m.vertices[&e.v[0]], m.vertices[&e.v[1]]);
                        (lo, hi, id.0)
                    })
                    .collect(),
            )
        });
        t.ids_meeting(lo, hi).into_iter().map(EdgeId).collect()
    }

    /// The edges that may lie in `plane` with a 2D box (in the axes of the plane) that meets the box
    /// `lo`–`hi` grown by `eps`, in id order (some may not exist any more).
    pub fn edges_in_plane(
        &self,
        m: &SurfaceModel,
        plane: &Plane,
        lo: DVec2,
        hi: DVec2,
        eps: f64,
    ) -> Vec<EdgeId> {
        let (a, b) = self.lifted(plane, lo, hi, eps);
        self.edges_in(m, a, b)
    }

    /// The faces that may lie in `plane` with a 2D box that meets the box `lo`–`hi` grown by `eps`,
    /// in id order (some may not exist any more).
    pub fn faces_in_plane(
        &self,
        m: &SurfaceModel,
        plane: &Plane,
        lo: DVec2,
        hi: DVec2,
        eps: f64,
    ) -> Vec<FaceId> {
        let (a, b) = self.lifted(plane, lo, hi, eps);
        let t = self.faces.get_or_init(|| {
            Boxes::new(
                m.faces
                    .iter()
                    .filter_map(|(id, f)| {
                        let (lo, hi) = face_box(f, |v| m.vertices[&v])?;
                        Some((lo, hi, id.0))
                    })
                    .collect(),
            )
        });
        t.ids_meeting(a, b).into_iter().map(FaceId).collect()
    }

    /// The 3D box of the 2D box `lo`–`hi` (grown by `eps`) of the axes of `plane`, lifted onto the
    /// plane and grown by the tolerance (for rounding).
    ///
    /// Why a grown element box meets it (see [`grown`]): let the points of an element lie in a box
    /// with the centre `c` and the half diagonal `r`, each within the plane tolerance `pt` of the
    /// plane. Seen in the axes, every point lies within `r` of the projection of `c` (a projection
    /// does not make distances longer), so a 2D box of the points that meets the query box has a
    /// point `q` of the query box within `√2 r` of it. `q` lifted onto the plane lies in the lifted
    /// box and within `√(2r² + (r + pt)²) ≤ √3 r + pt` of `c`, so inside the element box grown by
    /// its diagonal `2r` and twice the tolerance.
    fn lifted(&self, plane: &Plane, lo: DVec2, hi: DVec2, eps: f64) -> (DVec3, DVec3) {
        let (u, v) = plane.basis();
        let o = plane.normal * plane.offset;
        let (lo, hi) = (lo - DVec2::splat(eps), hi + DVec2::splat(eps));
        let corners = [
            DVec2::new(lo.x, lo.y),
            DVec2::new(hi.x, lo.y),
            DVec2::new(lo.x, hi.y),
            DVec2::new(hi.x, hi.y),
        ]
        .map(|c| u * c.x + v * c.y + o);
        let (a, b) = geom::bounds(corners.into_iter()).expect("four corners");
        let pad = DVec3::splat(self.tol(geom::size(a).max(geom::size(b))));
        (a - pad, b + pad)
    }
}

/// The box of `lo`–`hi` grown by its diagonal and twice the tolerance at its size, so that a query
/// in the axes of any plane through the points finds it (see [`Near::lifted`]).
fn grown(lo: DVec3, hi: DVec3) -> (DVec3, DVec3) {
    let g = lo.distance(hi) + 2.0 * geom::tolerance_at(geom::size(lo).max(geom::size(hi)));
    (lo - DVec3::splat(g), hi + DVec3::splat(g))
}

fn edge_box(a: DVec3, b: DVec3) -> (DVec3, DVec3) {
    grown(a.min(b), a.max(b))
}

/// The grown box of the outer loop of a face (the loop that the 2D tests of faces use).
fn face_box(f: &Face, pos: impl Fn(VertexId) -> DVec3) -> Option<(DVec3, DVec3)> {
    let (lo, hi) = geom::bounds(f.outer.iter().map(|v| pos(*v)))?;
    Some(grown(lo, hi))
}

/// The points of a set, for the planes through them: which points lie on a plane.
pub(super) struct Points {
    tree: BoxTree,
    /// The largest length tolerance of the points.
    tol: f64,
}

impl Points {
    pub fn new(points: &[DVec3]) -> Points {
        let size = points.iter().map(|p| geom::size(*p)).fold(0.0, f64::max);
        let items = points
            .iter()
            .enumerate()
            .map(|(i, p)| (*p, *p, u32::try_from(i).expect("fewer points than u32")))
            .collect();
        Points {
            tree: BoxTree::new(items),
            tol: geom::tolerance_at(size),
        }
    }

    /// The indices of the points that may lie on the plane (within more than the plane tolerance;
    /// the caller tests each one), in index order.
    pub fn near_plane(&self, plane: &Plane) -> Vec<usize> {
        let (n, o, t) = (plane.normal, plane.offset, self.tol);
        let mut out = Vec::new();
        self.tree.query_by(
            &|lo, hi| {
                let c = (lo + hi) / 2.0;
                let r = n.abs().dot((hi - lo) / 2.0);
                let d = n.dot(c) - o;
                d - r <= t && d + r >= -t
            },
            &mut |i| out.push(i as usize),
        );
        out.sort_unstable();
        out
    }
}

/// The straight runs of edges found so far in one solve (the edges do not change while it runs):
/// a piece on a run that is already known gets its vertices beside the run without the walk.
///
/// The walk of a piece `u`–`v` tests each vertex next to the run against the line through `u`
/// and `v`. A run found from another piece `a`–`b` gives the same result when no test can come
/// out differently: the line of the new piece lies within `dev` of the line `a`–`b` over the whole
/// run (its ends lie within `d1`, `d2` of that line, and a line through two points is within
/// `(d1 + d2)(1 + 2D / (length - d1 - d2))` of it at a distance `D` from them), every vertex of
/// the run is closer than half the smallest tolerance and every vertex beside it is farther than
/// twice the largest. Otherwise the piece is walked as before.
#[derive(Default)]
pub(super) struct Runs {
    runs: Vec<Run>,
    /// The runs through each vertex.
    at: BTreeMap<VertexId, Vec<usize>>,
}

struct Run {
    a: DVec3,
    b: DVec3,
    beside: BTreeSet<VertexId>,
    /// The largest distance of a vertex of the run from the line `a`–`b`.
    on: f64,
    /// The smallest distance of a vertex beside the run from the line.
    off: f64,
    /// The largest distance of a vertex of the run or beside it from `a`.
    reach: f64,
    /// The smallest length tolerance of a vertex of the run, and the largest of all its vertices
    /// and the vertices beside it.
    tol_lo: f64,
    tol_hi: f64,
}

impl Runs {
    /// A known run that gives the walk of `u`–`v`.
    pub fn find(&self, u: VertexId, v: VertexId, pu: DVec3, pv: DVec3) -> Option<usize> {
        let ks = self.at.get(&u)?;
        ks.iter().copied().find(|&k| {
            let r = &self.runs[k];
            if !self.at.get(&v).is_some_and(|kv| kv.contains(&k)) {
                return false;
            }
            let (d1, d2) = (
                geom::point_line(pu, r.a, r.b),
                geom::point_line(pv, r.a, r.b),
            );
            let len = pu.distance(pv) - d1 - d2;
            if len <= 0.0 {
                return false;
            }
            let reach = r.reach + pu.distance(r.a);
            // The rounding of the distances is far below the margins; it is added anyway.
            let dev =
                (d1 + d2) * (1.0 + 2.0 * reach / len) + 1e-12 * (reach + r.a.abs().max_element());
            r.on + dev <= 0.5 * r.tol_lo && r.off - dev >= 2.0 * r.tol_hi
        })
    }

    /// Records the run found by the walk of the piece `pu`–`pv`.
    pub fn add(
        &mut self,
        pu: DVec3,
        pv: DVec3,
        run: BTreeSet<VertexId>,
        beside: BTreeSet<VertexId>,
        pos: impl Fn(VertexId) -> DVec3,
    ) -> usize {
        let k = self.runs.len();
        let dist = |v: &VertexId| geom::point_line(pos(*v), pu, pv);
        let tol = |v: &VertexId| geom::tolerance_at(geom::size(pos(*v)));
        let r = Run {
            a: pu,
            b: pv,
            on: run.iter().map(dist).fold(0.0, f64::max),
            off: beside.iter().map(dist).fold(f64::INFINITY, f64::min),
            reach: run
                .iter()
                .chain(&beside)
                .map(|v| pos(*v).distance(pu))
                .fold(0.0, f64::max),
            tol_lo: run.iter().map(tol).fold(f64::INFINITY, f64::min),
            tol_hi: run.iter().chain(&beside).map(tol).fold(0.0, f64::max),
            beside,
        };
        for v in run {
            self.at.entry(v).or_default().push(k);
        }
        self.runs.push(r);
        k
    }

    pub fn beside(&self, k: usize) -> &BTreeSet<VertexId> {
        &self.runs[k].beside
    }
}

/// The cycles walked so far in one solve, for each plane (to the bit): the walk on the left of a
/// directed edge (`arrange::closes_on_left`) that lies on a known cycle reads the cycle instead of
/// walking it again. The ring then starts at the same vertex as the walk would start, so its area
/// is summed in the same order and comes out the same to the bit.
#[derive(Default)]
pub(super) struct Walks {
    planes: BTreeMap<[u64; 4], PlaneWalks>,
}

#[derive(Default)]
struct PlaneWalks {
    /// For each directed edge on a known cycle: the cycle and the place of the edge in it.
    of: BTreeMap<(VertexId, VertexId), (usize, usize)>,
    /// The 2D points of each cycle, in walk order.
    rings: Vec<Vec<DVec2>>,
}

impl Walks {
    /// The same as `arrange::closes_on_left(m, topo, plane, a, b)`.
    pub fn closes_on_left(
        &mut self,
        m: &SurfaceModel,
        topo: &Topo,
        plane: &Plane,
        a: VertexId,
        b: VertexId,
    ) -> Option<(DVec2, DVec2)> {
        let (n, o) = (plane.normal, plane.offset);
        let pw = self
            .planes
            .entry([n.x, n.y, n.z, o].map(f64::to_bits))
            .or_default();
        let rotated;
        let ring = match pw.of.get(&(a, b)) {
            Some(&(c, 0)) => &pw.rings[c],
            Some(&(c, k)) => {
                let r = &pw.rings[c];
                rotated = r[k..].iter().chain(&r[..k]).copied().collect::<Vec<_>>();
                &rotated
            }
            None => {
                let cycle = arrange::left_cycle(m, topo, plane, a, b)?;
                let c = pw.rings.len();
                let len = cycle.len();
                for (i, x) in cycle.iter().enumerate() {
                    pw.of.insert((*x, cycle[(i + 1) % len]), (c, i));
                }
                pw.rings
                    .push(cycle.iter().map(|v| plane.project(m.vertices[v])).collect());
                &pw.rings[c]
            }
        };
        (geom::area2(ring) > 0.0).then(|| arrange::box_of(ring))
    }
}
