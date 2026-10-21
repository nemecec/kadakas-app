//! The validity check of a surface model.
//!
//! `quick` runs on every load and after every change: ids, references, limits, numbers, planes,
//! the sides of the loops, curves, and coplanar faces that look the same way and overlap at an
//! edge (faces back to back are a warning of `full`, not a problem), in `O(n log n)`
//! time (holes: the size of the hole times the size of the outer loop). The plane test is the one
//! that the edits use, so a face is valid exactly when the edits see it in its plane. `full` adds the distances (no two vertices, no vertex and edge, no two edges closer than
//! half the length tolerance unless they meet), the coplanar faces that overlap (see `clash`),
//! and the manifold state of each edge, which the solid tools (MOD-09) need. An edge that passes
//! through a face in another plane is a warning (`NotIntersected`), not a problem: a sketch may
//! hold it until Intersect Faces makes the edges, but a closed solid may not (`is_closed`). The
//! fast checks never look at it, so the two checks agree. The geometric checks need a model whose references hold; an edit checks the
//! geometry that it touched with `near` and `clash` in any case.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use super::arrange::inside;
use super::clash;
use super::geom::{self, Plane};
use super::index::BoxTree;
use super::{
    EdgeId, FaceId, MAX_EDGES, MAX_FACES, MAX_LOOP_VERTICES, MAX_VERTICES, SurfaceModel, VertexId,
    key,
};
use crate::model::MAX_COORD;

/// One thing that is wrong with a model.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, thiserror::Error)]
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
    /// A warning, not an error (see [`Report::warnings`]): the edge passes through the inside of
    /// a face in another plane, and nothing splits them there. A sketch may hold this, as in other
    /// modelers; Intersect Faces makes the edges. A closed solid may not hold it
    /// ([`Report::is_closed`]).
    #[error(
        "edge {0} passes through face {1} at {2} and is not intersected with it (Intersect Faces \
         makes the edges there)"
    )]
    NotIntersected(EdgeId, FaceId, At),
    #[error("edge {0} of a face lies inside face {1}, which is in the same plane")]
    EdgeInFace(EdgeId, FaceId),
    #[error("faces {0} and {1} lie in one plane and overlap")]
    FacesOverlap(FaceId, FaceId),
    /// A warning, not an error (see [`Report::warnings`]).
    #[error(
        "faces {0} and {1} lie back to back in one plane (they look opposite ways and share an \
         area: an inner face)"
    )]
    BackToBack(FaceId, FaceId),
    #[error("curve {0} names an edge that does not exist or is in another curve, or is no chain")]
    BadCurve(super::CurveId),
    #[error("edge {0} has attributes but does not exist")]
    EdgeAttrs(EdgeId),
}

/// A point of a problem in µm (whole numbers, so that problems compare and sort exactly).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct At(pub [i64; 3]);

impl At {
    /// The point rounded to µm.
    pub fn of(p: DVec3) -> At {
        // In range: coordinates are at most `MAX_COORD` (1e9 mm), so 1e12 µm fits an i64.
        At((p * 1000.0).round().to_array().map(|x| x as i64))
    }

    /// The point in mm.
    pub fn point(&self) -> DVec3 {
        DVec3::from(self.0.map(|x| x as f64)) / 1000.0
    }
}

impl std::fmt::Display for At {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mm = |x: i64| {
            let s = if x < 0 { "-" } else { "" };
            let a = x.unsigned_abs();
            format!("{s}{}.{:03}", a / 1000, a % 1000)
        };
        write!(
            f,
            "({}, {}, {})",
            mm(self.0[0]),
            mm(self.0[1]),
            mm(self.0[2])
        )
    }
}

/// The result of [`SurfaceModel::check`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Things that are valid but worth a look: two coplanar faces back to back
    /// ([`Problem::BackToBack`], a box standing on a slab with its own vertices; the solid tools
    /// (MOD-09) can treat such a pair as an inner face), and edges that pass through a face in
    /// another plane without an edge there ([`Problem::NotIntersected`]: a sketch may hold them,
    /// Intersect Faces resolves them, a closed solid may not hold them).
    pub warnings: Vec<Problem>,
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

    /// The number of [`Problem::NotIntersected`] warnings: places where Intersect Faces would make
    /// edges.
    pub fn not_intersected(&self) -> usize {
        self.warnings
            .iter()
            .filter(|p| matches!(p, Problem::NotIntersected(..)))
            .count()
    }

    /// True for a valid model whose faces close a volume: every edge has exactly two faces that
    /// use it in opposite directions, and no edge passes through a face without an edge there
    /// (faces that cross without being intersected close no volume).
    pub fn is_closed(&self) -> bool {
        self.is_valid()
            && self.not_intersected() == 0
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
        let c = clash::through_and_overlaps(m, window(m));
        r.warnings.extend(
            c.through
                .into_iter()
                .map(|(e, f, at)| Problem::NotIntersected(e, f, At::of(at))),
        );
        r.problems.extend(
            c.lies_in
                .into_iter()
                .map(|(e, f)| Problem::EdgeInFace(e, f)),
        );
        r.problems.extend(
            c.overlaps
                .into_iter()
                .map(|(f, g)| Problem::FacesOverlap(f, g)),
        );
        let mut back: BTreeSet<Problem> = c
            .back_to_back
            .into_iter()
            .map(|(f, g)| Problem::BackToBack(f, g))
            .collect();
        let none = BTreeMap::new();
        for (k, list) in &uses {
            back.extend(overlap(m, *k, list, &none, true));
        }
        r.warnings.extend(back);
        r.warnings.sort();
    }
    r
}

/// All problems of the fast checks, as a set.
pub(super) fn structure_problems(m: &SurfaceModel) -> BTreeSet<Problem> {
    let mut out = BTreeSet::new();
    structure(m, &mut |p| {
        out.insert(p);
        true
    });
    out
}

/// A point where a distance problem is: the vertex, the middle of the two vertices or of the
/// short edge, or the nearest point of two edges. `None` for the other problems or when the items
/// are gone.
pub(super) fn place(m: &SurfaceModel, p: &Problem) -> Option<DVec3> {
    let v = |v: &VertexId| m.vertices.get(v).copied();
    let e = |e: &EdgeId| {
        let ed = m.edges.get(e)?;
        Some((v(&ed.v[0])?, v(&ed.v[1])?))
    };
    match p {
        Problem::VertexOnEdge(a, _) => v(a),
        Problem::CloseVertices(a, b) => Some((v(a)? + v(b)?) / 2.0),
        Problem::ShortEdge(x) => e(x).map(|(a, b)| (a + b) / 2.0),
        Problem::Crossing(x, y) => {
            let ((a, b), (c, d)) = (e(x)?, e(y)?);
            match geom::segment_segment(a, b, c, d) {
                Some((s, t, _)) => Some((a + (b - a) * s + c + (d - c) * t) / 2.0),
                None => Some((a + b) / 2.0),
            }
        }
        _ => None,
    }
}

/// A bound for every length tolerance in the model.
pub(super) fn window(m: &SurfaceModel) -> f64 {
    let biggest = m
        .vertices
        .values()
        .map(|p| geom::size(*p))
        .fold(0.0, f64::max);
    geom::tolerance_at(biggest)
}

/// The distance problems (as in `full`) of pairs in which one item is in `edges` or `verts`: the
/// vertices of `verts` against all vertices and edges, and the edges of `edges` against all
/// vertices and edges. The references of the model must hold for these items.
pub(super) fn near(
    m: &SurfaceModel,
    edges: &BTreeSet<EdgeId>,
    verts: &BTreeSet<VertexId>,
) -> BTreeSet<Problem> {
    let window = window(m);
    let pad = DVec3::splat(window);
    let ok = |e: &super::Edge| e.v.iter().all(|v| m.vertices.contains_key(v));
    let tree = BoxTree::new(
        m.edges
            .iter()
            .filter(|(_, e)| ok(e))
            .map(|(id, e)| {
                let (a, b) = (m.vertices[&e.v[0]], m.vertices[&e.v[1]]);
                (a.min(b) - pad, a.max(b) + pad, id.0)
            })
            .collect(),
    );
    let half = |ps: &[DVec3]| geom::tol(ps) / 2.0;
    let mut out = BTreeSet::new();
    let vertex_against = |v: VertexId, p: DVec3, out: &mut BTreeSet<Problem>| {
        tree.query(p, p, &mut |c| {
            let c = EdgeId(c);
            let ce = m.edges[&c];
            let (x, y) = (m.vertices[&ce.v[0]], m.vertices[&ce.v[1]]);
            for (w, q) in [(ce.v[0], x), (ce.v[1], y)] {
                if w != v && p.distance(q) <= half(&[p, q]) {
                    out.insert(Problem::CloseVertices(v.min(w), v.max(w)));
                }
            }
            if !ce.v.contains(&v) && geom::point_segment(p, x, y).0 <= half(&[x, y, p]) {
                out.insert(Problem::VertexOnEdge(v, c));
            }
        });
    };
    for &v in verts {
        if let Some(p) = m.vertices.get(&v) {
            vertex_against(v, *p, &mut out);
        }
    }
    for &id in edges {
        let Some(e) = m.edges.get(&id).filter(|e| ok(e)) else {
            continue;
        };
        let (a, b) = (m.vertices[&e.v[0]], m.vertices[&e.v[1]]);
        if a.distance(b) <= half(&[a, b]) {
            out.insert(Problem::ShortEdge(id));
        }
        for (v, p) in [(e.v[0], a), (e.v[1], b)] {
            vertex_against(v, p, &mut out);
        }
        tree.query(a.min(b), a.max(b), &mut |c| {
            let c = EdgeId(c);
            if c == id {
                return;
            }
            let ce = m.edges[&c];
            let (x, y) = (m.vertices[&ce.v[0]], m.vertices[&ce.v[1]]);
            for (w, q) in [(ce.v[0], x), (ce.v[1], y)] {
                if !e.v.contains(&w) && geom::point_segment(q, a, b).0 <= half(&[a, b, q]) {
                    out.insert(Problem::VertexOnEdge(w, id));
                }
            }
            if e.v.iter().any(|v| ce.v.contains(v)) {
                return;
            }
            if let Some((_, _, dist)) = geom::segment_segment(a, b, x, y)
                && dist <= half(&[a, b, x, y])
            {
                out.insert(Problem::Crossing(id.min(c), id.max(c)));
            }
        });
    }
    out
}

/// Calls `found` for each problem of the fast checks; stops when it returns false.
// `clean` is set by every report; after the reports that end the check it is not read.
#[allow(unused_assignments)]
fn structure(m: &SurfaceModel, found: &mut dyn FnMut(Problem) -> bool) {
    let mut clean = true;
    macro_rules! report {
        ($p:expr) => {
            clean = false;
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
        if count > max * super::limit_scale() {
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
        .chain(m.faces.keys().map(|f| f.0))
        .chain(m.curves.keys().map(|c| c.0));
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
        // The same test as the edits use, so a face is valid exactly when the edits see it in
        // its plane.
        if let Some(v) = f
            .loops()
            .flatten()
            .find(|v| !geom::on_plane(&f.plane, m.vertices[v]))
        {
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
    if let Some(e) = m.edge_attrs.keys().find(|e| !m.edges.contains_key(e)) {
        report!(Problem::EdgeAttrs(*e));
    }
    if let Some(p) = curves(m) {
        report!(p);
    }
    if clean {
        let mut uses: BTreeMap<(VertexId, VertexId), Vec<(FaceId, bool)>> = BTreeMap::new();
        for (&id, f) in &m.faces {
            for (a, b) in f.uses() {
                uses.entry(key(a, b)).or_default().push((id, a < b));
            }
        }
        for (k, list) in &uses {
            if let Some(p) = overlap(m, *k, list, &pairs, false) {
                report!(p);
            }
        }
    }
}

/// The first problem of the curves: an edge that does not exist, an edge in two curves or twice,
/// an empty curve, or edges that are not a chain.
pub(super) fn curves(m: &SurfaceModel) -> Option<Problem> {
    let mut seen: BTreeSet<EdgeId> = BTreeSet::new();
    for (&id, es) in &m.curves {
        let bad = Some(Problem::BadCurve(id));
        if es.is_empty()
            || es
                .iter()
                .any(|e| !m.edges.contains_key(e) || !seen.insert(*e))
        {
            return bad;
        }
        if es.windows(2).any(|w| {
            !m.edges[&w[0]]
                .v
                .iter()
                .any(|v| m.edges[&w[1]].v.contains(v))
        }) {
            return bad;
        }
    }
    None
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

/// Two coplanar faces on the same side of an edge share an area. When they look the same way
/// they overlap ([`Problem::Overlap`], an error); when they look opposite ways they lie back to
/// back ([`Problem::BackToBack`], a warning). `back` chooses which of the two kinds is looked for.
fn overlap(
    m: &SurfaceModel,
    k: (VertexId, VertexId),
    list: &[(FaceId, bool)],
    pairs: &BTreeMap<(VertexId, VertexId), EdgeId>,
    back: bool,
) -> Option<Problem> {
    let on = |p: &Plane, f: FaceId| {
        m.faces[&f]
            .loops()
            .flatten()
            .all(|v| geom::on_plane(p, m.vertices[v]))
    };
    for (i, &(f, fa)) in list.iter().enumerate() {
        let pf = m.faces[&f].plane;
        for &(g, ga) in &list[i + 1..] {
            let pg = m.faces[&g].plane;
            let same_way = pf.normal.dot(pg.normal) > 0.0;
            let same_side = (fa == ga) == same_way;
            if same_side && same_way != back && on(&pf, g) {
                return Some(if back {
                    Problem::BackToBack(f.min(g), f.max(g))
                } else {
                    Problem::Overlap(f, g, pairs[&k])
                });
            }
        }
    }
    None
}
