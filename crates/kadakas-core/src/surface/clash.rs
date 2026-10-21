//! Where an edge meets a face that it does not belong to, and where two coplanar faces cover the
//! same area. The check of a model (`check::full`) reports the cases that are always wrong; an edit
//! (Push/Pull) also refuses the cases that a new or moved edge or face must not make.
//!
//! - **Through**: the edge passes through the face (its ends are on the two sides of the plane and
//!   the crossing point is inside the face). Always wrong: two faces of the model cut each other
//!   without an edge between them.
//! - **Ends on**: an end of the edge that is not a vertex of the face lies on the face (in it or on
//!   its border). A line that stands on a face is valid, but an edit must not make one by moving
//!   geometry onto a face.
//! - **Lies in**: the edge lies in the plane of the face, and a part of it is inside the face. A
//!   stray edge on a face is valid; an edit must not move an edge into a face.
//!
//! Two coplanar faces **overlap** when a point inside one is inside the other. Faces next to each
//! other in one plane (the halves of a split face) do not.

use std::collections::BTreeMap;

use glam::{DVec2, DVec3};

use super::arrange::{inside, sample_rings};
use super::geom;
use super::index::BoxTree;
use super::{Face, FaceId, SurfaceModel, VertexId, key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Contact {
    Through,
    EndsOn,
    LiesIn,
}

/// The 2D rings of a face in the axes of its plane.
pub(super) fn rings(m: &SurfaceModel, f: &Face) -> Vec<Vec<DVec2>> {
    f.loops()
        .map(|l| l.iter().map(|v| f.plane.project(m.vertices[v])).collect())
        .collect()
}

/// The distance from a 2D point to the border of the rings.
fn border_distance(p: DVec2, rings: &[Vec<DVec2>]) -> f64 {
    let mut best = f64::INFINITY;
    for r in rings {
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            let d = b - a;
            let len2 = d.length_squared();
            let t = if len2 > 0.0 {
                ((p - a).dot(d) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            best = best.min((a + d * t).distance(p));
        }
    }
    best
}

/// Inside the rings and farther than `margin` from their border.
fn strictly_inside(p: DVec2, rings: &[Vec<DVec2>], margin: f64) -> bool {
    inside(p, rings) && border_distance(p, rings) > margin
}

/// Inside the rings or within `margin` of their border.
fn in_closure(p: DVec2, rings: &[Vec<DVec2>], margin: f64) -> bool {
    inside(p, rings) || border_distance(p, rings) <= margin
}

/// How the segment `a`–`b` (the edge with the end vertices `ends`) meets the face `g` (with its 2D
/// rings), or `None`. With `strict` false only `Through` is looked for (the check of a model);
/// with `strict` true also `EndsOn` and `LiesIn` (an edit), and a crossing on the border of `g`
/// counts as `Through`. The ends of the edge that are vertices of `g` may lie on it, and an edge of
/// `g` meets it nowhere.
pub(super) fn contact(
    a: DVec3,
    b: DVec3,
    ends: [VertexId; 2],
    g: &Face,
    rings: &[Vec<DVec2>],
    strict: bool,
) -> Option<Contact> {
    let any = rings.iter().flatten().next().copied().unwrap_or_default();
    let far = geom::size(a)
        .max(geom::size(b))
        .max(any.abs().max_element() + g.plane.offset.abs());
    let tol = geom::tolerance_at(far);
    let pt = geom::plane_tolerance_at(far);
    let half = tol / 2.0;
    let (sa, sb) = (g.plane.distance(a), g.plane.distance(b));
    let (on_a, on_b) = (sa.abs() <= pt, sb.abs() <= pt);
    if !on_a && !on_b {
        if (sa > 0.0) == (sb > 0.0) {
            return None;
        }
        let p = g.plane.project(a + (b - a) * (sa / (sa - sb)));
        let hit = if strict {
            in_closure(p, rings, half)
        } else {
            strictly_inside(p, rings, half)
        };
        return hit.then_some(Contact::Through);
    }
    if !strict {
        return None;
    }
    let is_vertex = |v: VertexId| g.loops().flatten().any(|x| *x == v);
    let (pa, pb) = (g.plane.project(a), g.plane.project(b));
    if on_a != on_b {
        let (v, p) = if on_a { (ends[0], pa) } else { (ends[1], pb) };
        return (!is_vertex(v) && in_closure(p, rings, half)).then_some(Contact::EndsOn);
    }
    // In the plane.
    let k = key(ends[0], ends[1]);
    if g.uses().any(|(x, y)| key(x, y) == k) {
        return None;
    }
    for (v, p) in [(ends[0], pa), (ends[1], pb)] {
        if !is_vertex(v) && in_closure(p, rings, half) {
            return Some(Contact::EndsOn);
        }
    }
    // The pieces between the crossings with the border: a piece whose middle is inside lies in g.
    let d = pb - pa;
    let mut ts = vec![0.0, 1.0];
    for r in rings {
        for i in 0..r.len() {
            let (c, e) = (r[i], r[(i + 1) % r.len()]);
            let f = e - c;
            let den = d.perp_dot(f);
            if den.abs() <= 1e-12 * d.length() * f.length() {
                continue;
            }
            let t = (c - pa).perp_dot(f) / den;
            let s = (c - pa).perp_dot(d) / den;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s) {
                ts.push(t);
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.windows(2)
        .any(|w| w[1] > w[0] && strictly_inside(pa + d * ((w[0] + w[1]) / 2.0), rings, half))
        .then_some(Contact::LiesIn)
}

/// The box of a face, grown by `pad`.
pub(super) fn face_box(m: &SurfaceModel, f: &Face, pad: f64) -> (DVec3, DVec3) {
    let (lo, hi) = geom::bounds(f.outer.iter().map(|v| m.vertices[v])).unwrap_or_default();
    (lo - DVec3::splat(pad), hi + DVec3::splat(pad))
}

/// The box tree of all faces (ids are face ids), grown by `pad`.
pub(super) fn face_tree(m: &SurfaceModel, pad: f64) -> BoxTree {
    BoxTree::new(
        m.faces
            .iter()
            .map(|(id, f)| {
                let (lo, hi) = face_box(m, f, pad);
                (lo, hi, id.0)
            })
            .collect(),
    )
}

/// True if the segment `a`–`b` has its ends on the two sides of the plane of `g`, beyond the plane
/// tolerance: the only case in which it can pass through `g`.
fn crosses_plane(a: DVec3, b: DVec3, g: &Face) -> bool {
    let pt = geom::plane_tol(&[a, b]);
    let (sa, sb) = (g.plane.distance(a), g.plane.distance(b));
    (sa > pt && sb < -pt) || (sa < -pt && sb > pt)
}

/// The edges that pass through a face, and the pairs of coplanar faces that overlap, of a model
/// whose references hold. Each pair once, in id order. The rings of a face are made only for the
/// faces that need them.
pub(super) fn through_and_overlaps(m: &SurfaceModel, window: f64) -> Clashes {
    let tree = face_tree(m, window);
    let mut rings_of: BTreeMap<FaceId, Vec<Vec<DVec2>>> = BTreeMap::new();
    let mut through = Vec::new();
    let mut cands: Vec<FaceId> = Vec::new();
    for (&id, e) in &m.edges {
        let (a, b) = (m.vertices[&e.v[0]], m.vertices[&e.v[1]]);
        cands.clear();
        tree.query(a.min(b), a.max(b), &mut |g| {
            let g = FaceId(g);
            if crosses_plane(a, b, &m.faces[&g]) {
                cands.push(g);
            }
        });
        cands.sort_unstable();
        for &g in &cands {
            let gf = &m.faces[&g];
            let r = rings_of.entry(g).or_insert_with(|| rings(m, gf));
            if contact(a, b, e.v, gf, r, false).is_some() {
                through.push((id, g));
            }
        }
    }
    let mut overlaps = Vec::new();
    for (&id, f) in &m.faces {
        let p0 = m.vertices[&f.outer[0]];
        let (lo, hi) = face_box(m, f, 0.0);
        cands.clear();
        tree.query(lo, hi, &mut |g| {
            let g = FaceId(g);
            let other = &m.faces[&g];
            if g != id
                && other.plane.normal.dot(f.plane.normal).abs() >= 1.0 - 1e-9
                && geom::on_plane(&other.plane, p0)
            {
                cands.push(g);
            }
        });
        if cands.is_empty() {
            continue;
        }
        let rf = rings_of.entry(id).or_insert_with(|| rings(m, f)).clone();
        let Some(s) = sample_rings(&rf) else {
            continue;
        };
        let (u, v) = f.plane.basis();
        let p = u * s.x + v * s.y + f.plane.normal * f.plane.offset;
        let half = geom::tol(&[p]) / 2.0;
        for &g in &cands {
            let gf = &m.faces[&g];
            let r = rings_of.entry(g).or_insert_with(|| rings(m, gf));
            if strictly_inside(gf.plane.project(p), r, half) {
                overlaps.push((id.min(g), id.max(g)));
            }
        }
    }
    overlaps.sort_unstable();
    overlaps.dedup();
    Clashes { through, overlaps }
}

/// The result of [`through_and_overlaps`].
pub(super) struct Clashes {
    /// Edges that pass through a face.
    pub through: Vec<(super::EdgeId, FaceId)>,
    /// Coplanar faces that overlap.
    pub overlaps: Vec<(FaceId, FaceId)>,
}
