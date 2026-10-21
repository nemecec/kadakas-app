//! Intersect Faces (MOD-08): edges where faces cross.
//!
//! Drawing does not split faces that cross in other planes, and an edge drawn through a face in
//! another plane is not split there (as in other modelers of this kind): the check of the model
//! warns (`NotIntersected`). This edit makes the edges.
//!
//! - **Faces.** For each pair of faces in two planes whose boxes meet (a box tree, no scan of all
//!   pairs), the line where the two planes meet is cut by the sides of both faces. The pieces of
//!   the line that lie in both faces (holes are no part of a face; a border counts) become new
//!   edges. They are drawn like `add_edges` draws: an end on an edge splits the edge (so an edge
//!   that passes through the other face is split at the point where it passes through), pieces
//!   weld to vertices and split at crossings, and both planes are solved again, so both faces
//!   split. A part of a split face keeps the attributes of the face; split edges keep their
//!   attributes and their place in a curve (the curve stays one chain).
//! - **Edges.** An edge that passes through a face and is not split by the new edges (a line with
//!   no face, a guide) is split at the point where it passes through. Its two parts then stand on
//!   the face, as a line drawn from a face does (a note says so).
//! - **Not done.** Faces of one plane that overlap are a problem of the model, not a crossing.
//!   They are left alone. A piece shorter than the length tolerance is left out with a note, and a
//!   crossing that stays is named in a note.
//!
//! The pieces that existing edges already cover are left out, so a second run changes nothing.
//! The order is fixed (pairs in id order), so the same model gives the same ids.

use super::super::check;
use super::super::clash::{self, Shapes};
use super::*;
use crate::surface::At;

/// Below this many sides a face is cut side by side, above it through a box tree.
const SMALL: usize = 24;

/// The sides of a face in 3D, with a box tree for a face with many sides.
struct Sides {
    sides: Vec<(DVec3, DVec3)>,
    tree: Option<BoxTree>,
}

impl Sides {
    fn new(m: &SurfaceModel, f: &Face) -> Sides {
        let sides: Vec<(DVec3, DVec3)> = f
            .uses()
            .map(|(a, b)| (m.vertices[&a], m.vertices[&b]))
            .collect();
        let tree = (sides.len() > SMALL).then(|| {
            BoxTree::new(
                sides
                    .iter()
                    .enumerate()
                    .map(|(i, (a, b))| {
                        let i = u32::try_from(i).expect("fewer sides than the loop limit");
                        (a.min(*b), a.max(*b), i)
                    })
                    .collect(),
            )
        });
        Sides { sides, tree }
    }

    /// Calls `f` with each side whose box meets `lo`–`hi` (every side without a tree).
    fn each(&self, lo: DVec3, hi: DVec3, f: &mut dyn FnMut(DVec3, DVec3)) {
        match &self.tree {
            Some(t) => t.query(lo, hi, &mut |i| {
                let (a, b) = self.sides[i as usize];
                f(a, b);
            }),
            None => {
                for &(a, b) in &self.sides {
                    f(a, b);
                }
            }
        }
    }
}

/// The line where two planes meet: a point `o` and the unit direction `d`.
struct Line {
    o: DVec3,
    d: DVec3,
}

impl Line {
    /// The line of the planes `p` and `q`, with `o` next to `near`. `None` for parallel planes.
    fn of(p: &Plane, q: &Plane, near: DVec3) -> Option<Line> {
        let d = p.normal.cross(q.normal);
        let len2 = d.length_squared();
        // Parallel to 1e-9 radians: no line that the tolerances could hold.
        if len2 <= 1e-18 {
            return None;
        }
        let o = (q.normal.cross(d) * p.offset + d.cross(p.normal) * q.offset) / len2;
        let d = d / len2.sqrt();
        Some(Line {
            o: o + d * (near - o).dot(d),
            d,
        })
    }

    fn t(&self, p: DVec3) -> f64 {
        (p - self.o).dot(self.d)
    }
}

/// The points where the sides of a face (those whose box meets `lo`–`hi`) meet the plane `p`, as
/// (place along the line, point): a vertex on the plane, or the crossing of a side whose ends are
/// on the two sides of it.
fn cuts(s: &Sides, lo: DVec3, hi: DVec3, p: &Plane, line: &Line, out: &mut Vec<(f64, DVec3)>) {
    s.each(lo, hi, &mut |a, b| {
        let (sa, sb) = (p.distance(a), p.distance(b));
        if on_plane(p, a) {
            out.push((line.t(a), a));
        } else if !on_plane(p, b) && (sa > 0.0) != (sb > 0.0) {
            let x = a + (b - a) * (sa / (sa - sb));
            out.push((line.t(x), x));
        }
    });
}

/// The cuts in order, those within the length tolerance of the one before left out.
fn sorted(mut cuts: Vec<(f64, DVec3)>) -> Vec<(f64, DVec3)> {
    cuts.sort_by(|x, y| {
        x.0.total_cmp(&y.0)
            .then(x.1.x.total_cmp(&y.1.x))
            .then(x.1.y.total_cmp(&y.1.y))
            .then(x.1.z.total_cmp(&y.1.z))
    });
    let mut out: Vec<(f64, DVec3)> = Vec::with_capacity(cuts.len());
    for c in cuts {
        match out.last() {
            Some(l) if c.0 - l.0 <= geom::tol(&[l.1, c.1]) => {}
            _ => out.push(c),
        }
    }
    out
}

/// The pieces between consecutive cuts whose middle is `inside`, the pieces that meet joined.
fn pieces(cuts: &[(f64, DVec3)], inside: &mut dyn FnMut(DVec3) -> bool) -> Vec<[(f64, DVec3); 2]> {
    let mut out: Vec<[(f64, DVec3); 2]> = Vec::new();
    let mut joined = false;
    for w in cuts.windows(2) {
        let mid = (w[0].1 + w[1].1) / 2.0;
        if !inside(mid) {
            joined = false;
            continue;
        }
        match out.last_mut() {
            Some(last) if joined => last[1] = w[1],
            _ => out.push([w[0], w[1]]),
        }
        joined = true;
    }
    out
}

/// The pieces of the line where the planes of `a` and `b` meet that lie in both faces (their
/// closures). `a` is the face with fewer sides: it is cut by all its sides, `b` only by the sides
/// near the pieces of `a`.
fn crossing(
    m: &SurfaceModel,
    (ia, sa): (FaceId, &Sides),
    (ib, sb): (FaceId, &Sides),
    shapes: &mut Shapes,
) -> Vec<(DVec3, DVec3)> {
    let (fa, fb) = (&m.faces[&ia], &m.faces[&ib]);
    let Some(line) = Line::of(&fa.plane, &fb.plane, m.vertices[&fa.outer[0]]) else {
        return Vec::new();
    };
    let all = (DVec3::NEG_INFINITY, DVec3::INFINITY);
    let mut ca = Vec::new();
    cuts(sa, all.0, all.1, &fb.plane, &line, &mut ca);
    let ca = sorted(ca);
    let in_face = |shapes: &mut Shapes, id: FaceId, f: &Face, p: DVec3| {
        shapes
            .get(id)
            .in_closure(f.plane.project(p), geom::tol(&[p]) / 2.0)
    };
    let along_a = pieces(&ca, &mut |p| in_face(shapes, ia, fa, p));
    let mut out = Vec::new();
    for [(s0, p0), (s1, p1)] in along_a {
        let pad = DVec3::splat(geom::tol(&[p0, p1]));
        let mut cb = Vec::new();
        cuts(
            sb,
            p0.min(p1) - pad,
            p0.max(p1) + pad,
            &fa.plane,
            &line,
            &mut cb,
        );
        cb.retain(|c| c.0 > s0 && c.0 < s1);
        cb.push((s0, p0));
        cb.push((s1, p1));
        // The ends of the piece of `a` first, so that they stay when a cut of `b` falls on them.
        let cb = sorted(cb);
        for [(_, q0), (_, q1)] in pieces(&cb, &mut |p| in_face(shapes, ib, fb, p)) {
            if q0.distance(q1) > geom::tol(&[q0, q1]) {
                out.push((q0, q1));
            }
        }
    }
    out
}

/// The parts of the segment `p`–`q` that no edge of the model covers (an edge along the same line,
/// within the length tolerance), each longer than the tolerance.
fn uncovered(m: &SurfaceModel, edges: &BoxTree, p: DVec3, q: DVec3) -> Vec<(DVec3, DVec3)> {
    let tol = geom::tol(&[p, q]);
    let len = p.distance(q);
    let u = (q - p) / len;
    let pad = DVec3::splat(tol);
    let mut cover: Vec<(f64, f64)> = Vec::new();
    edges.query(p.min(q) - pad, p.max(q) + pad, &mut |e| {
        let Some(ed) = m.edges.get(&EdgeId(e)) else {
            return;
        };
        let (c, d) = (m.vertices[&ed.v[0]], m.vertices[&ed.v[1]]);
        let t = geom::tol(&[p, q, c, d]);
        if geom::point_line(c, p, q) <= t && geom::point_line(d, p, q) <= t {
            let (tc, td) = ((c - p).dot(u), (d - p).dot(u));
            cover.push((tc.min(td), tc.max(td)));
        }
    });
    cover.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
    let mut out = Vec::new();
    let mut at = 0.0;
    for (lo, hi) in cover {
        if lo - at > tol && at < len {
            out.push((p + u * at, p + u * lo.min(len)));
        }
        at = f64::max(at, hi);
    }
    if len - at > tol {
        out.push((p + u * at, q));
    }
    out.retain(|(a, b)| a.distance(*b) > tol);
    out
}

/// Runs Intersect Faces on `m` (see the module documentation).
pub(super) fn intersect_faces(
    m: &mut SurfaceModel,
    scope: IntersectScope,
    faces: &[FaceId],
    edges: &[EdgeId],
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    let chosen_faces = existing_faces(m, faces)?;
    let chosen_edges: BTreeSet<EdgeId> = edges.iter().copied().collect();
    if let Some(id) = chosen_edges.iter().find(|id| !m.edges.contains_key(id)) {
        return Err(SurfaceError::NoEdge(*id));
    }
    let (before_edges, before_faces) = (m.edges.len(), m.faces.len());
    let window = check::window(m);
    // The pairs of faces in two planes whose boxes meet, in id order.
    let tree = clash::face_tree(m, window, &|_| true);
    let firsts: Vec<FaceId> = match scope {
        IntersectScope::Model => m.faces.keys().copied().collect(),
        IntersectScope::Selection => chosen_faces.iter().copied().collect(),
    };
    let mut pairs: BTreeSet<(FaceId, FaceId)> = BTreeSet::new();
    for f in firsts {
        let face = &m.faces[&f];
        let (lo, hi) = clash::face_box(m, face, window);
        tree.query(lo, hi, &mut |g| {
            let g = FaceId(g);
            if g != f
                && (scope == IntersectScope::Selection || g > f)
                && clash::coplanar(m, face, &m.faces[&g]).is_none()
            {
                pairs.insert((f.min(g), f.max(g)));
            }
        });
    }
    let mut shapes = Shapes::new(m);
    let mut sides: BTreeMap<FaceId, Sides> = BTreeMap::new();
    let edge_tree = clash::edge_tree(m, window);
    let mut segs: Vec<(DVec3, DVec3)> = Vec::new();
    for (f, g) in pairs {
        for id in [f, g] {
            sides
                .entry(id)
                .or_insert_with(|| Sides::new(m, &m.faces[&id]));
        }
        // The face with fewer sides is cut whole, the other one near its pieces.
        let (a, b) = if m.faces[&g].loop_len() < m.faces[&f].loop_len() {
            (g, f)
        } else {
            (f, g)
        };
        for (p, q) in crossing(m, (a, &sides[&a]), (b, &sides[&b]), &mut shapes) {
            segs.extend(uncovered(m, &edge_tree, p, q));
        }
    }
    if segs.len() > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit",
            max: MAX_EDIT_EDGES,
            count: segs.len(),
        });
    }
    // The edges that pass through a face, and where.
    let pierces = match scope {
        IntersectScope::Model => clash::through(m, window, None, None),
        IntersectScope::Selection => {
            clash::through(m, window, Some(&chosen_edges), Some(&chosen_faces))
        }
    };
    let mut w = Work::indexed(m);
    let wire: BTreeSet<EdgeId> = pierces
        .iter()
        .filter(|(e, _, _)| w.t.faces_of(*e).next().is_none())
        .map(|(e, _, _)| *e)
        .collect();
    let mut drawn = Vec::new();
    let mut touched = BTreeSet::new();
    let mut short = 0usize;
    for (p, q) in segs {
        match w.insert_segment(p, q, &mut drawn, &mut touched) {
            Ok(()) => {}
            // A piece that would be shorter than the tolerance next to other geometry.
            Err(SurfaceError::TooShort { .. } | SurfaceError::TooClose { .. }) => short += 1,
            Err(e) => return Err(e),
        }
        w.check_limits()?;
    }
    for (e, g, x) in &pierces {
        let was = w.m.vertices.len();
        let made = w.snap_existing(*x, &mut touched);
        if wire.contains(e) && made.is_some() && w.m.vertices.len() > was {
            notes.push(format!(
                "edge {e} passed through face {g} at {}: it is split there, and its parts stand \
                 on the face",
                At::of(*x)
            ));
        }
    }
    w.solve_after_add(&drawn, &touched)?;
    w.check_limits()?;
    if short > 0 {
        notes.push(format!(
            "{short} piece(s) where faces cross were left out: they would be shorter than the \
             length tolerance next to other geometry"
        ));
    }
    let left = match scope {
        IntersectScope::Model => clash::through(m, check::window(m), None, None),
        // Crossings of faces that were not chosen stay by design.
        IntersectScope::Selection => Vec::new(),
    };
    for (e, g, x) in left.iter().take(5) {
        notes.push(format!(
            "edge {e} still passes through face {g} at {}: the place is within the length \
             tolerance of other geometry, so no edge can be made there",
            At::of(*x)
        ));
    }
    if left.len() > 5 {
        notes.push(format!(
            "{} more places where an edge passes through a face",
            left.len() - 5
        ));
    }
    let (made_edges, made_faces) = (
        m.edges.len().saturating_sub(before_edges),
        m.faces.len().saturating_sub(before_faces),
    );
    notes.push(if made_edges == 0 && made_faces == 0 {
        "Intersect Faces: nothing to intersect".to_string()
    } else {
        format!("Intersect Faces: {made_edges} new edges, {made_faces} new faces")
    });
    Ok(())
}
