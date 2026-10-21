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
//!   stray edge on a face is valid; an edge of another face in the same plane that lies in the face
//!   is not (the two faces overlap), and an edit must not move an edge into a face.
//!
//! Two coplanar faces **overlap** when they share an area: an edge of one lies in the other, or a
//! point inside one is inside the other (one face in the other, or the same face twice). Faces
//! that only touch along an edge or at a vertex do not overlap.
//!
//! A face is looked at through its [`Shape`]: the rings in the 2D axes of its plane with a box tree
//! of their sides, and its vertices and edges as sets, so that a test against a face with
//! thousands of holes takes `O(log n)` time.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use super::arrange::sample_rings;
use super::geom;
use super::index::BoxTree;
use super::{EdgeId, Face, FaceId, SurfaceModel, VertexId, key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Contact {
    Through,
    EndsOn,
    LiesIn,
}

/// Below this many sides a face is tested side by side, above it through the box tree.
const SMALL: usize = 24;

/// A face prepared for the tests.
pub(super) struct Shape {
    pub rings: Vec<Vec<DVec2>>,
    sides: Vec<(DVec2, DVec2)>,
    tree: Option<BoxTree>,
    verts: BTreeSet<VertexId>,
    keys: BTreeSet<(VertexId, VertexId)>,
}

fn z0(p: DVec2) -> DVec3 {
    DVec3::new(p.x, p.y, 0.0)
}

impl Shape {
    pub fn new(m: &SurfaceModel, f: &Face) -> Shape {
        let rings: Vec<Vec<DVec2>> = f
            .loops()
            .map(|l| l.iter().map(|v| f.plane.project(m.vertices[v])).collect())
            .collect();
        let sides: Vec<(DVec2, DVec2)> = rings
            .iter()
            .flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()])))
            .collect();
        let tree = (sides.len() > SMALL).then(|| {
            BoxTree::new(
                sides
                    .iter()
                    .enumerate()
                    .map(|(i, (a, b))| {
                        (
                            z0(a.min(*b)),
                            z0(a.max(*b)),
                            u32::try_from(i).expect("fewer sides than the loop limit"),
                        )
                    })
                    .collect(),
            )
        });
        Shape {
            rings,
            sides,
            tree,
            verts: f.loops().flatten().copied().collect(),
            keys: f.uses().map(|(a, b)| key(a, b)).collect(),
        }
    }

    /// Calls `f` with each side whose box meets the box `lo`–`hi`.
    fn sides_in(&self, lo: DVec2, hi: DVec2, f: &mut dyn FnMut(DVec2, DVec2)) {
        match &self.tree {
            Some(t) => t.query(z0(lo), z0(hi), &mut |i| {
                let (a, b) = self.sides[i as usize];
                f(a, b);
            }),
            None => {
                for &(a, b) in &self.sides {
                    if !(a.max(b).cmplt(lo).any() || a.min(b).cmpgt(hi).any()) {
                        f(a, b);
                    }
                }
            }
        }
    }

    /// Inside the area (even-odd rule: inside the outer ring, outside the holes).
    pub fn inside(&self, p: DVec2) -> bool {
        let mut odd = false;
        self.sides_in(p, DVec2::new(f64::INFINITY, p.y), &mut |a, b| {
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    odd = !odd;
                }
            }
        });
        odd
    }

    /// True if a side is within `margin` of `p`.
    fn near_border(&self, p: DVec2, margin: f64) -> bool {
        let mut near = false;
        let m = DVec2::splat(margin);
        self.sides_in(p - m, p + m, &mut |a, b| {
            let d = b - a;
            let len2 = d.length_squared();
            let t = if len2 > 0.0 {
                ((p - a).dot(d) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            near |= (a + d * t).distance(p) <= margin;
        });
        near
    }

    /// Inside and farther than `margin` from the border.
    pub fn strictly_inside(&self, p: DVec2, margin: f64) -> bool {
        self.inside(p) && !self.near_border(p, margin)
    }

    /// Inside or within `margin` of the border.
    pub fn in_closure(&self, p: DVec2, margin: f64) -> bool {
        self.inside(p) || self.near_border(p, margin)
    }

    pub fn has_vertex(&self, v: VertexId) -> bool {
        self.verts.contains(&v)
    }

    pub fn has_edge(&self, a: VertexId, b: VertexId) -> bool {
        self.keys.contains(&key(a, b))
    }

    /// The middle of a piece of the segment `pa`–`pb` (in the plane) that lies inside, farther
    /// than `margin` from the border, if there is one. The pieces are cut where the segment
    /// crosses a side.
    fn piece_inside(&self, pa: DVec2, pb: DVec2, margin: f64) -> Option<DVec2> {
        let d = pb - pa;
        let mut ts = vec![0.0, 1.0];
        self.sides_in(pa.min(pb), pa.max(pb), &mut |c, e| {
            let f = e - c;
            let den = d.perp_dot(f);
            if den.abs() <= 1e-12 * d.length() * f.length() {
                return;
            }
            let t = (c - pa).perp_dot(f) / den;
            let s = (c - pa).perp_dot(d) / den;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s) {
                ts.push(t);
            }
        });
        ts.sort_by(f64::total_cmp);
        ts.windows(2).find_map(|w| {
            let p = pa + d * ((w[0] + w[1]) / 2.0);
            (w[1] > w[0] && self.strictly_inside(p, margin)).then_some(p)
        })
    }
}

/// The point of the plane of `g` at the 2D point `p`.
fn lift(g: &Face, p: DVec2) -> DVec3 {
    let (u, v) = g.plane.basis();
    u * p.x + v * p.y + g.plane.normal * g.plane.offset
}

/// How the segment `a`–`b` (the edge with the end vertices `ends`) meets the face `g`, and where
/// (a point of the contact), or `None`. With `strict` false only `Through` is looked for (the
/// check of a model); with `strict` true also `EndsOn` and `LiesIn` (an edit), and a crossing on
/// the border of `g` counts as `Through`. The ends of the edge that are vertices of `g` may lie on
/// it, and an edge of `g` meets it nowhere.
pub(super) fn contact(
    a: DVec3,
    b: DVec3,
    ends: [VertexId; 2],
    g: &Face,
    s: &Shape,
    strict: bool,
) -> Option<(Contact, DVec3)> {
    let (sa, sb) = (g.plane.distance(a), g.plane.distance(b));
    let far = geom::size(a).max(geom::size(b));
    let tol = geom::tolerance_at(far);
    let pt = geom::plane_tolerance_at(far);
    let half = tol / 2.0;
    let (on_a, on_b) = (sa.abs() <= pt, sb.abs() <= pt);
    if !on_a && !on_b {
        if (sa > 0.0) == (sb > 0.0) {
            return None;
        }
        let x = a + (b - a) * (sa / (sa - sb));
        let p = g.plane.project(x);
        let hit = if strict {
            s.in_closure(p, half)
        } else {
            s.strictly_inside(p, half)
        };
        return hit.then_some((Contact::Through, x));
    }
    if !strict {
        return None;
    }
    let (pa, pb) = (g.plane.project(a), g.plane.project(b));
    if on_a != on_b {
        let (v, p, x) = if on_a {
            (ends[0], pa, a)
        } else {
            (ends[1], pb, b)
        };
        return (!s.has_vertex(v) && s.in_closure(p, half)).then_some((Contact::EndsOn, x));
    }
    // In the plane.
    if s.has_edge(ends[0], ends[1]) {
        return None;
    }
    for (v, p, x) in [(ends[0], pa, a), (ends[1], pb, b)] {
        if !s.has_vertex(v) && s.in_closure(p, half) {
            return Some((Contact::EndsOn, x));
        }
    }
    lies_in(a, b, ends, g, s).map(|x| (Contact::LiesIn, x))
}

/// A point of a piece of the segment `a`–`b` that lies inside `g`, when the segment lies in the
/// plane of `g` and is not an edge of it.
fn lies_in(a: DVec3, b: DVec3, ends: [VertexId; 2], g: &Face, s: &Shape) -> Option<DVec3> {
    let far = geom::size(a).max(geom::size(b));
    let pt = geom::plane_tolerance_at(far);
    if g.plane.distance(a).abs() > pt
        || g.plane.distance(b).abs() > pt
        || s.has_edge(ends[0], ends[1])
    {
        return None;
    }
    let half = geom::tolerance_at(far) / 2.0;
    s.piece_inside(g.plane.project(a), g.plane.project(b), half)
        .map(|p| lift(g, p))
}

/// The box of a face, grown by `pad`.
pub(super) fn face_box(m: &SurfaceModel, f: &Face, pad: f64) -> (DVec3, DVec3) {
    let (lo, hi) = geom::bounds(f.outer.iter().map(|v| m.vertices[v])).unwrap_or_default();
    (lo - DVec3::splat(pad), hi + DVec3::splat(pad))
}

/// The box tree of the faces for which `keep` is true (ids are face ids), grown by `pad`.
pub(super) fn face_tree(m: &SurfaceModel, pad: f64, keep: &dyn Fn(&Face) -> bool) -> BoxTree {
    BoxTree::new(
        m.faces
            .iter()
            .filter(|(_, f)| keep(f))
            .map(|(id, f)| {
                let (lo, hi) = face_box(m, f, pad);
                (lo, hi, id.0)
            })
            .collect(),
    )
}

/// The box tree of the edges whose vertices exist (ids are edge ids), grown by `pad`.
pub(super) fn edge_tree(m: &SurfaceModel, pad: f64) -> BoxTree {
    let pad = DVec3::splat(pad);
    BoxTree::new(
        m.edges
            .iter()
            .filter_map(|(id, e)| {
                let (a, b) = (m.vertices.get(&e.v[0])?, m.vertices.get(&e.v[1])?);
                Some((a.min(*b) - pad, a.max(*b) + pad, id.0))
            })
            .collect(),
    )
}

/// Shapes made when they are first needed.
pub(super) struct Shapes<'a> {
    m: &'a SurfaceModel,
    made: BTreeMap<FaceId, Shape>,
}

impl<'a> Shapes<'a> {
    pub fn new(m: &'a SurfaceModel) -> Self {
        Shapes {
            m,
            made: BTreeMap::new(),
        }
    }

    pub fn get(&mut self, g: FaceId) -> &Shape {
        let m = self.m;
        self.made
            .entry(g)
            .or_insert_with(|| Shape::new(m, &m.faces[&g]))
    }
}

/// True if the segment `a`–`b` has its ends on the two sides of the plane of `g`, beyond the plane
/// tolerance: the only case in which it can pass through `g`.
fn crosses_plane(a: DVec3, b: DVec3, g: &Face) -> bool {
    let pt = geom::plane_tol(&[a, b]);
    let (sa, sb) = (g.plane.distance(a), g.plane.distance(b));
    (sa > pt && sb < -pt) || (sa < -pt && sb > pt)
}

/// True if the two faces lie in one plane (either side to the front).
fn coplanar(m: &SurfaceModel, f: &Face, g: &Face) -> bool {
    f.plane.normal.dot(g.plane.normal).abs() >= 1.0 - 1e-9
        && geom::on_plane(&g.plane, m.vertices[&f.outer[0]])
}

/// The result of [`through_and_overlaps`].
pub(super) struct Clashes {
    /// Edges that pass through a face.
    pub through: Vec<(EdgeId, FaceId)>,
    /// Edges of a face that lie in another face of the same plane.
    pub lies_in: Vec<(EdgeId, FaceId)>,
    /// Coplanar faces that overlap.
    pub overlaps: Vec<(FaceId, FaceId)>,
}

/// The edges that pass through a face, the edges of a face that lie in a coplanar face, and the
/// pairs of coplanar faces that overlap, of a model whose references hold. Each pair once, in id
/// order. A stray edge (of no face) in a face is valid and not reported.
pub(super) fn through_and_overlaps(m: &SurfaceModel, window: f64) -> Clashes {
    let tree = face_tree(m, window, &|_| true);
    let mut shapes = Shapes::new(m);
    let mut faces_of: BTreeMap<(VertexId, VertexId), Vec<FaceId>> = BTreeMap::new();
    for (&id, f) in &m.faces {
        for (a, b) in f.uses() {
            faces_of.entry(key(a, b)).or_default().push(id);
        }
    }
    let mut out = Clashes {
        through: Vec::new(),
        lies_in: Vec::new(),
        overlaps: Vec::new(),
    };
    let mut cands: Vec<(FaceId, bool)> = Vec::new();
    for (&id, e) in &m.edges {
        let (a, b) = (m.vertices[&e.v[0]], m.vertices[&e.v[1]]);
        let own = faces_of.get(&e.key()).map_or(&[][..], Vec::as_slice);
        cands.clear();
        tree.query(a.min(b), a.max(b), &mut |g| {
            let g = FaceId(g);
            let gf = &m.faces[&g];
            if crosses_plane(a, b, gf) {
                cands.push((g, true));
            } else if !own.contains(&g) && own.iter().any(|h| coplanar(m, &m.faces[h], gf)) {
                cands.push((g, false));
            }
        });
        cands.sort_unstable();
        for &(g, crossing) in &cands {
            let gf = &m.faces[&g];
            let s = shapes.get(g);
            if crossing {
                if contact(a, b, e.v, gf, s, false).is_some() {
                    out.through.push((id, g));
                }
            } else if lies_in(a, b, e.v, gf, s).is_some() {
                out.lies_in.push((id, g));
                for h in own {
                    if coplanar(m, &m.faces[h], gf) {
                        out.overlaps.push((g.min(*h), g.max(*h)));
                    }
                }
            }
        }
    }
    // One face inside the other with no edge inside (the same area twice).
    let mut near: Vec<FaceId> = Vec::new();
    for (&id, f) in &m.faces {
        let (lo, hi) = face_box(m, f, 0.0);
        near.clear();
        tree.query(lo, hi, &mut |g| {
            let g = FaceId(g);
            if g != id && coplanar(m, f, &m.faces[&g]) {
                near.push(g);
            }
        });
        if near.is_empty() {
            continue;
        }
        let Some(p) = sample_rings(&shapes.get(id).rings).map(|s| lift(f, s)) else {
            continue;
        };
        let half = geom::tol(&[p]) / 2.0;
        for &g in &near {
            let gf = &m.faces[&g];
            if shapes.get(g).strictly_inside(gf.plane.project(p), half) {
                out.overlaps.push((id.min(g), id.max(g)));
            }
        }
    }
    out.overlaps.sort_unstable();
    out.overlaps.dedup();
    out
}
