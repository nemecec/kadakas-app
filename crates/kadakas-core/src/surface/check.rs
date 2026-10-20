//! The validity check of a surface model.
//!
//! `quick` runs on every load and after every change: ids, references, limits, numbers, planes and
//! the sides of the loops, in linear time (holes: the size of the hole times the size of the outer
//! loop). `full` adds the distances (no two vertices, no vertex and edge, no two edges closer than
//! half the length tolerance unless they meet), overlapping faces, and the manifold state of each
//! edge, which the solid tools (MOD-09) need.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use super::arrange::inside;
use super::geom::{self, Plane};
use super::{
    EdgeId, FaceId, MAX_EDGES, MAX_FACES, MAX_LOOP_VERTICES, MAX_VERTICES, SurfaceModel, VertexId,
    key,
};
use crate::model::MAX_COORD;

/// One thing that is wrong with a model.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Problem {
    #[error("a number is not finite or out of range")]
    Range,
    #[error("the transform cannot be inverted")]
    Transform,
    #[error("a surface model has at most {max} {what} (it has {count})")]
    Limit {
        what: &'static str,
        max: usize,
        count: usize,
    },
    #[error("the id {0} is 0, used twice, or not below next_id")]
    BadId(u32),
    #[error("edge {0} names a vertex that does not exist")]
    EdgeVertex(EdgeId),
    #[error("edge {0} has the same vertex at both ends")]
    LoopEdge(EdgeId),
    #[error("edges {0} and {1} join the same two vertices")]
    DoubleEdge(EdgeId, EdgeId),
    #[error("vertex {0} has no edge")]
    LooseVertex(VertexId),
    #[error("face {0} has a loop with fewer than 3 vertices")]
    ShortLoop(FaceId),
    #[error("face {0} names a vertex that does not exist")]
    FaceVertex(FaceId),
    #[error("face {0} has no edge between vertices {1} and {2}")]
    MissingEdge(FaceId, VertexId, VertexId),
    #[error("a loop of face {0} visits vertex {1} twice")]
    RepeatedVertex(FaceId, VertexId),
    #[error("face {0} uses edge {1} twice")]
    RepeatedEdge(FaceId, EdgeId),
    #[error("the normal of face {0} is not of unit length")]
    BadNormal(FaceId),
    #[error("vertex {1} of face {0} is off the plane of the face")]
    OffPlane(FaceId, VertexId),
    #[error("the outer loop of face {0} does not run counterclockwise around its normal")]
    OuterSide(FaceId),
    #[error("a hole of face {0} does not run clockwise around its normal")]
    HoleSide(FaceId),
    #[error("a hole of face {0} lies outside its outer loop")]
    HoleOutside(FaceId),
    #[error("vertices {0} and {1} are closer than half the length tolerance")]
    CloseVertices(VertexId, VertexId),
    #[error("vertex {0} lies on edge {1}")]
    VertexOnEdge(VertexId, EdgeId),
    #[error("edges {0} and {1} cross or touch")]
    Crossing(EdgeId, EdgeId),
    #[error("edge {0} is shorter than half the length tolerance")]
    ShortEdge(EdgeId),
    #[error("faces {0} and {1} overlap at edge {2}")]
    Overlap(FaceId, FaceId, EdgeId),
}

/// The result of [`SurfaceModel::check`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Edges that no face uses.
    pub wire_edges: Vec<EdgeId>,
    /// Edges that one face uses: the border of an open surface.
    pub border_edges: Vec<EdgeId>,
    /// Edges that three or more faces use.
    pub nonmanifold_edges: Vec<EdgeId>,
    /// Edges that two faces use in the same direction: the two faces disagree about front and
    /// back (one of them needs Reverse Faces).
    pub reversed_edges: Vec<EdgeId>,
}

impl Report {
    pub fn is_valid(&self) -> bool {
        self.problems.is_empty()
    }

    /// True for a valid model whose faces close a volume: every edge has exactly two faces that
    /// use it in opposite directions.
    pub fn is_closed(&self) -> bool {
        self.is_valid()
            && self.wire_edges.is_empty()
            && self.border_edges.is_empty()
            && self.nonmanifold_edges.is_empty()
            && self.reversed_edges.is_empty()
    }
}

/// The first problem of the fast checks.
pub(super) fn quick(m: &SurfaceModel) -> Option<Problem> {
    let mut first = None;
    structure(m, &mut |p| {
        first = Some(p);
        false
    });
    first
}

/// All problems and the edge report.
pub(super) fn full(m: &SurfaceModel) -> Report {
    let mut r = Report::default();
    structure(m, &mut |p| {
        r.problems.push(p);
        true
    });
    // The distance checks need a model whose references hold.
    let ok = r.problems.is_empty();
    let mut uses: BTreeMap<(VertexId, VertexId), Vec<(FaceId, bool)>> = BTreeMap::new();
    for (&id, f) in &m.faces {
        for (a, b) in f.uses() {
            uses.entry(key(a, b)).or_default().push((id, a < b));
        }
    }
    for (&id, e) in &m.edges {
        let list = uses.get(&e.key()).map_or(&[][..], Vec::as_slice);
        match list {
            [] => r.wire_edges.push(id),
            [_] => r.border_edges.push(id),
            [(_, a), (_, b)] if a != b => {}
            [_, _] => r.reversed_edges.push(id),
            _ => r.nonmanifold_edges.push(id),
        }
    }
    if ok {
        distances(m, &mut r.problems);
        overlaps(m, &uses, &mut r.problems);
    }
    r
}

/// Calls `found` for each problem of the fast checks; stops when it returns false.
fn structure(m: &SurfaceModel, found: &mut dyn FnMut(Problem) -> bool) {
    macro_rules! report {
        ($p:expr) => {
            if !found($p) {
                return;
            }
        };
    }
    let loops: usize = m.faces.values().map(super::Face::loop_len).sum();
    for (what, max, count) in [
        ("vertices", MAX_VERTICES, m.vertices.len()),
        ("edges", MAX_EDGES, m.edges.len()),
        ("faces", MAX_FACES, m.faces.len()),
        ("vertex uses in face loops", MAX_LOOP_VERTICES, loops),
    ] {
        if count > max {
            report!(Problem::Limit { what, max, count });
            return;
        }
    }
    if !m.is_finite() || m.vertices.values().any(|p| geom::size(*p) > MAX_COORD) {
        report!(Problem::Range);
        return;
    }
    if m.transform.matrix3.determinant().abs() <= 1e-12 {
        report!(Problem::Transform);
    }
    if m.bounds()
        .is_some_and(|(lo, hi)| geom::size(lo).max(geom::size(hi)) > MAX_COORD)
    {
        report!(Problem::Range);
    }
    let mut ids: BTreeSet<u32> = BTreeSet::new();
    let all = m
        .vertices
        .keys()
        .map(|v| v.0)
        .chain(m.edges.keys().map(|e| e.0))
        .chain(m.faces.keys().map(|f| f.0));
    for id in all {
        if id == 0 || id >= m.next_id || !ids.insert(id) {
            report!(Problem::BadId(id));
        }
    }
    let mut pairs: BTreeMap<(VertexId, VertexId), EdgeId> = BTreeMap::new();
    let mut degree: BTreeMap<VertexId, usize> = BTreeMap::new();
    for (&id, e) in &m.edges {
        if e.v.iter().any(|v| !m.vertices.contains_key(v)) {
            report!(Problem::EdgeVertex(id));
            continue;
        }
        if e.v[0] == e.v[1] {
            report!(Problem::LoopEdge(id));
            continue;
        }
        if let Some(other) = pairs.insert(e.key(), id) {
            report!(Problem::DoubleEdge(other, id));
        }
        for v in e.v {
            *degree.entry(v).or_default() += 1;
        }
    }
    for &v in m.vertices.keys() {
        if !degree.contains_key(&v) {
            report!(Problem::LooseVertex(v));
        }
    }
    for (&id, f) in &m.faces {
        let n = f.plane.normal;
        if (n.length() - 1.0).abs() > 1e-9 {
            report!(Problem::BadNormal(id));
            continue;
        }
        if f.loops().any(|l| l.len() < 3) {
            report!(Problem::ShortLoop(id));
            continue;
        }
        if f.loops().flatten().any(|v| !m.vertices.contains_key(v)) {
            report!(Problem::FaceVertex(id));
            continue;
        }
        let mut used: BTreeSet<EdgeId> = BTreeSet::new();
        let mut broken = false;
        for l in f.loops() {
            let mut seen: BTreeSet<VertexId> = BTreeSet::new();
            for (i, &v) in l.iter().enumerate() {
                if !seen.insert(v) {
                    report!(Problem::RepeatedVertex(id, v));
                    broken = true;
                }
                let w = l[(i + 1) % l.len()];
                match pairs.get(&key(v, w)) {
                    None => {
                        report!(Problem::MissingEdge(id, v, w));
                        broken = true;
                    }
                    Some(e) if !used.insert(*e) => {
                        report!(Problem::RepeatedEdge(id, *e));
                        broken = true;
                    }
                    Some(_) => {}
                }
            }
        }
        if broken {
            continue;
        }
        // Twice the plane tolerance: a face that a nearly equal plane split stays valid.
        if let Some(v) = f.loops().flatten().find(|v| {
            let p = m.vertices[v];
            f.plane.distance(p).abs() > 2.0 * geom::plane_tolerance_at(geom::size(p))
        }) {
            report!(Problem::OffPlane(id, *v));
            continue;
        }
        let pts = |l: &Vec<VertexId>| m.loop_points(l);
        if geom::newell(&pts(&f.outer)).dot(n) <= 0.0 {
            report!(Problem::OuterSide(id));
            continue;
        }
        let ring = |l: &Vec<VertexId>| -> Vec<DVec2> {
            l.iter().map(|v| f.plane.project(m.vertices[v])).collect()
        };
        let outer = [ring(&f.outer)];
        for h in &f.holes {
            if geom::newell(&pts(h)).dot(n) >= 0.0 {
                report!(Problem::HoleSide(id));
                break;
            }
            let r = ring(h);
            if !inside((r[0] + r[1]) / 2.0, &outer) {
                report!(Problem::HoleOutside(id));
                break;
            }
        }
    }
}

/// The distance checks, with a sweep along x.
fn distances(m: &SurfaceModel, out: &mut Vec<Problem>) {
    let pos = |v: &VertexId| m.vertices[v];
    let biggest = m
        .vertices
        .values()
        .map(|p| geom::size(*p))
        .fold(0.0, f64::max);
    // A bound for every tolerance in the model; the tests use the tolerance of the points.
    let window = geom::tolerance_at(biggest);
    let half = |ps: &[DVec3]| geom::tol(ps) / 2.0;
    let mut verts: Vec<(VertexId, DVec3)> = m.vertices.iter().map(|(k, v)| (*k, *v)).collect();
    verts.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.0.cmp(&b.0)));
    for i in 0..verts.len() {
        for j in i + 1..verts.len() {
            if verts[j].1.x - verts[i].1.x > window {
                break;
            }
            let (a, b) = (verts[i], verts[j]);
            if a.1.distance(b.1) <= half(&[a.1, b.1]) {
                out.push(Problem::CloseVertices(a.0.min(b.0), a.0.max(b.0)));
            }
        }
    }
    let mut edges: Vec<(EdgeId, DVec3, DVec3)> = m
        .edges
        .iter()
        .map(|(id, e)| (*id, pos(&e.v[0]), pos(&e.v[1])))
        .collect();
    for &(id, a, b) in &edges {
        if a.distance(b) <= half(&[a, b]) {
            out.push(Problem::ShortEdge(id));
        }
    }
    edges.sort_by(|x, y| {
        x.1.x
            .min(x.2.x)
            .total_cmp(&y.1.x.min(y.2.x))
            .then(x.0.cmp(&y.0))
    });
    // Vertices against edges: for each edge, the vertices whose x is in its range.
    for &(id, a, b) in &edges {
        let e = m.edges[&id];
        let (lo, hi) = (
            a.min(b) - DVec3::splat(window),
            a.max(b) + DVec3::splat(window),
        );
        let start = verts.partition_point(|v| v.1.x < lo.x);
        for &(v, p) in &verts[start..] {
            if p.x > hi.x {
                break;
            }
            if e.v.contains(&v) || p.cmplt(lo).any() || p.cmpgt(hi).any() {
                continue;
            }
            if geom::point_segment(p, a, b).0 <= half(&[a, b, p]) {
                out.push(Problem::VertexOnEdge(v, id));
            }
        }
    }
    for i in 0..edges.len() {
        let (ei, a, b) = edges[i];
        let vi = m.edges[&ei].v;
        let hi_x = a.x.max(b.x) + window;
        for &(ej, c, d) in &edges[i + 1..] {
            if c.x.min(d.x) > hi_x {
                break;
            }
            // Many edges share an x range (a column of a grid): test the other axes first.
            let (lo, hi) = (
                a.min(b) - DVec3::splat(window),
                a.max(b) + DVec3::splat(window),
            );
            if c.max(d).cmplt(lo).any() || c.min(d).cmpgt(hi).any() {
                continue;
            }
            let vj = m.edges[&ej].v;
            if vi.iter().any(|v| vj.contains(v)) {
                continue;
            }
            if let Some((_, _, dist)) = geom::segment_segment(a, b, c, d)
                && dist <= half(&[a, b, c, d])
            {
                out.push(Problem::Crossing(ei.min(ej), ei.max(ej)));
            }
        }
    }
}

/// Two coplanar faces on the same side of an edge overlap.
fn overlaps(
    m: &SurfaceModel,
    uses: &BTreeMap<(VertexId, VertexId), Vec<(FaceId, bool)>>,
    out: &mut Vec<Problem>,
) {
    let on = |p: &Plane, f: FaceId| {
        m.faces[&f]
            .loops()
            .flatten()
            .all(|v| geom::on_plane(p, m.vertices[v]))
    };
    for (k, list) in uses {
        for (i, &(f, fa)) in list.iter().enumerate() {
            let pf = m.faces[&f].plane;
            for &(g, ga) in &list[i + 1..] {
                let pg = m.faces[&g].plane;
                let same_side = (fa == ga) == (pf.normal.dot(pg.normal) > 0.0);
                if same_side && on(&pf, g) {
                    let e = m.edge_between(k.0, k.1).expect("a used edge exists");
                    out.push(Problem::Overlap(f, g, e));
                }
            }
        }
    }
}
