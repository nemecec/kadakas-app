//! Push/Pull (MOD-08): a face moves along its normal and the faces next to it follow.
//!
//! The decision is made for each **edge** of the face `F` (all loops), not for the face:
//!
//! - **Stretch.** The edge is shared with exactly one other face `G` that stands at a right angle
//!   to `F` (a wall of a prism). `G` follows the edge: the edge moves with `F`, and the wall gets
//!   longer or shorter. No new wall is made, so no coplanar leftovers remain.
//! - **Wall.** Any other edge (no neighbour, a coplanar neighbour such as the other half of a
//!   split top or the face around a hole, a slanted neighbour, several neighbours): a new wall
//!   face is made between the edge and its copy.
//!
//! "At a right angle" is measured as a distance: the moved edge stays within the plane tolerance of
//! the wall's plane, so a wall that is nearly at a right angle stretches for a short push and gets
//! a new wall beside it for a long one, and no face ever leaves its plane.
//!
//! A vertex that only has stretch edges (and no other face that would leave its plane) moves in
//! place. A vertex that has a wall edge stays, and gets a copy at `normal * distance` with a new
//! vertical edge between them. A stretch neighbour of an edge at such a vertex gets the vertical
//! edge in its outline (a step in the wall), so every face stays flat. When an edge of the model
//! already runs from the vertex along the normal past the copy (the corner of a box under a top
//! split from corner to corner), the copy splits that edge and its part is the vertical edge:
//! coincident edges merge, as in SketchUp, and the step in the walls closes up.
//!
//! A stretched edge whose end stays is replaced by the edge between the copies; the new edge takes
//! its attributes and its place in a curve (a curve that is no longer one chain is cut into
//! chains).
//!
//! Then `F` (keeping its id) is the cap on the moved or copied vertices. A face with no neighbour
//! at all (a lone sketch) becomes a closed solid: the old position is kept as the bottom, turned
//! to the outside, and for a push to the back the cap is turned instead. A face that sits on other
//! faces leaves the opening where it was: a pull adds a bump with the walls outward, a push digs a
//! pocket and the walls face into it (the walls always face the empty side).
//!
//! # Refused
//!
//! - A move that reaches the far end of a wall edge (the push goes through to the opposite face).
//!   The hole through a slab is a later step.
//! - A wall edge with more than one other face, one of which lies where the new wall would go (a
//!   fin: the face is pushed into a wall that it shares an edge with).
//! - A result in which a new or moved edge passes through, ends on, or lies in a face that it does
//!   not belong to, or a new or moved face takes in an edge in the same way (a pocket deeper than
//!   the solid, a stretched wall that cuts a plate). Only contacts that the push makes count.
//! - A result with a problem that the model did not have before: the fast checks of the whole
//!   model, and the distance checks (`check::near`) of the touched vertices and edges, which run
//!   also when the model had a structural problem before. The problems are compared as sets, and
//!   the error names a new one. The model is unchanged.
//!
//! Curves on the wall edges of `F` are copied to the cap, and the new vertical edges between two
//! pieces of a curve get the attribute `smooth: true` (a cylinder is shaded round).
//!
//! Ids are given in a fixed order (the copies of the vertices in loop order, then the vertical
//! edges, the cap edges, the walls, then the bottom), so the same edit gives the same ids.

use super::super::CurveId;
use super::super::check::{self, Problem};
use super::super::clash::{self, Contact};
use super::super::index::BoxTree;
use super::*;

pub(super) fn push_pull(
    m: &mut SurfaceModel,
    face: FaceId,
    distance: f64,
) -> Result<(), SurfaceError> {
    let before = m.clone();
    let touched = run(m, face, distance)?;
    validate(&before, m, &touched)
}

/// One boundary edge of the face.
struct Boundary {
    a: VertexId,
    b: VertexId,
    edge: EdgeId,
    /// The wall that follows the edge, if it is a stretch edge.
    stretch: Option<FaceId>,
}

/// What a push changed, for the checks of the result.
struct Touched {
    /// The vertices that moved, the copies, and the vertices of split edges.
    verts: BTreeSet<VertexId>,
    /// The edges at those vertices.
    edges: BTreeSet<EdgeId>,
    /// The faces that are new or changed, with the face of the model before that was at the same
    /// place (`None` for a new wall).
    faces: BTreeMap<FaceId, Option<FaceId>>,
}

fn run(m: &mut SurfaceModel, face: FaceId, d: f64) -> Result<Touched, SurfaceError> {
    let f = m
        .faces
        .get(&face)
        .cloned()
        .ok_or(SurfaceError::NoFace(face))?;
    if !d.is_finite() {
        return Err(SurfaceError::BadPoint);
    }
    let n = f.plane.normal;
    let verts: Vec<VertexId> = {
        let mut seen = BTreeSet::new();
        f.loops()
            .flatten()
            .copied()
            .filter(|v| seen.insert(*v))
            .collect()
    };
    let pts: Vec<DVec3> = verts.iter().map(|v| m.vertices[v]).collect();
    let tol = geom::tol(&pts);
    if d.abs() <= tol {
        return Err(SurfaceError::TooShort { tol });
    }
    let mut new_pos: BTreeMap<VertexId, DVec3> = BTreeMap::new();
    for &v in &verts {
        let p = m.vertices[&v] + n * d;
        if !p.is_finite() || geom::size(p) > MAX_COORD {
            return Err(SurfaceError::BadPoint);
        }
        new_pos.insert(v, p);
    }
    let mut w = Work::new(m);
    // A face keeps its plane when the vertices `vs` of the face move: each moved point is within
    // the plane tolerance, and so is the move itself (a distance, not an angle, so a long push of
    // a nearly upright wall never takes a vertex off the wall).
    let keeps_plane = |g: &Face, vs: &[VertexId]| {
        vs.iter().all(|v| {
            let p = new_pos[v];
            (d * g.plane.normal.dot(n)).abs() <= geom::plane_tol(&[p]) && on_plane(&g.plane, p)
        })
    };

    // The edges of the face.
    let mut bounds: Vec<Boundary> = Vec::new();
    let mut any_shared = false;
    for (a, b) in f.uses() {
        let edge =
            w.t.edge(a, b)
                .ok_or_else(|| SurfaceError::Unsolved("a face has no edge for a side".into()))?;
        let others: Vec<FaceId> = w.t.faces_of(edge).filter(|g| *g != face).collect();
        any_shared |= !others.is_empty();
        let stretch = match others.as_slice() {
            [g] if keeps_plane(&w.m.faces[g], &[a, b]) => Some(*g),
            _ => None,
        };
        if stretch.is_none() {
            // A face of the edge in the plane that the new wall takes, on the side it goes to (a
            // fin): the new wall would lie on it.
            for g in &others {
                let gf = &w.m.faces[g];
                if !keeps_plane(gf, &[a, b]) {
                    continue;
                }
                let along = if gf.uses().any(|u| u == (a, b)) {
                    w.pos(b) - w.pos(a)
                } else {
                    w.pos(a) - w.pos(b)
                };
                if gf.plane.normal.cross(along).dot(n * d) > 0.0 {
                    return Err(SurfaceError::Unsolved(format!(
                        "the new wall at edge {edge} would lie on face {g}, which shares the \
                         edge (push the other way or erase one of the faces)"
                    )));
                }
            }
        }
        bounds.push(Boundary {
            a,
            b,
            edge,
            stretch,
        });
    }
    let attached = any_shared;

    // Vertices that stay and get a copy: the ends of wall edges, and the vertices of a face that
    // would leave its plane if they moved.
    let mut stays: BTreeSet<VertexId> = bounds
        .iter()
        .filter(|b| b.stretch.is_none())
        .flat_map(|b| [b.a, b.b])
        .collect();
    for &v in &verts {
        let around: BTreeSet<FaceId> =
            w.t.edges_at(v)
                .flat_map(|e| w.t.faces_of(e).collect::<Vec<_>>())
                .collect();
        if around
            .iter()
            .any(|g| *g != face && !keeps_plane(&w.m.faces[g], &[v]))
        {
            stays.insert(v);
        }
    }

    // An edge of the model that runs from a vertex along the normal.
    let in_face: BTreeSet<(VertexId, VertexId)> = f.uses().map(|(a, b)| key(a, b)).collect();
    let through = || {
        SurfaceError::Unsolved("the face cannot be pushed through to the opposite face yet".into())
    };
    let mut merge: BTreeMap<VertexId, EdgeId> = BTreeMap::new();
    for &v in &verts {
        let p = w.pos(v);
        for e in w.t.edges_at(v).collect::<Vec<_>>() {
            let edge = w.m.edges[&e];
            if in_face.contains(&edge.key()) {
                continue;
            }
            let Some(o) = edge.other(v) else { continue };
            let q = w.pos(o);
            if geom::point_line(q, p, p + n) > geom::tol(&[p, q]) {
                continue;
            }
            let s = (q - p).dot(n);
            if s * d <= 0.0 {
                continue;
            }
            if !stays.contains(&v) {
                // A vertex that moves in place may not reach the far end of the edge.
                if (s - d) * s <= 0.0 || (s - d).abs() <= tol {
                    return Err(through());
                }
            } else if s.abs() > d.abs() + tol {
                // The copy lies on the edge: the edge is split there and its part is the vertical.
                merge.insert(v, e);
            } else if s.abs() < d.abs() - tol {
                return Err(through());
            }
        }
    }

    // The copies must not land on a vertex of the model.
    let big =
        w.m.vertices
            .values()
            .fold(0.0f64, |m, p| m.max(geom::size(*p)))
            .max(geom::size(pts[0]) + d.abs());
    let window = geom::tolerance_at(big);
    let mut by_x: Vec<(f64, DVec3)> = w.m.vertices.values().map(|p| (p.x, *p)).collect();
    by_x.sort_by(|p, q| p.0.total_cmp(&q.0));
    for &v in verts.iter().filter(|v| stays.contains(v)) {
        // A vertex that stays leaves a copy behind; one that moves leaves its place free.
        let p = new_pos[&v];
        let start = by_x.partition_point(|(x, _)| *x < p.x - window);
        let hit = by_x[start..]
            .iter()
            .take_while(|(x, _)| *x <= p.x + window)
            .any(|(_, q)| p.distance(*q) <= geom::tol(&[p, *q]));
        if hit {
            return Err(SurfaceError::TooClose {
                tol: geom::tol(&[p]),
            });
        }
    }

    let mut faces_out: BTreeMap<FaceId, Option<FaceId>> = BTreeMap::new();
    // Copies, in loop order. A vertex that moves is its own copy.
    let mut mv: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    for &v in &verts {
        if !stays.contains(&v) {
            mv.insert(v, v);
        } else if let Some(&e) = merge.get(&v) {
            for g in w.t.faces_of(e).collect::<Vec<_>>() {
                faces_out.insert(g, Some(g));
            }
            let mut keys = BTreeSet::new();
            let c = w.split_edge(e, new_pos[&v], &mut keys);
            mv.insert(v, c);
        } else {
            let id = w.new_vertex(new_pos[&v]);
            mv.insert(v, id);
        }
    }
    let mut vertical: BTreeMap<VertexId, EdgeId> = BTreeMap::new();
    for &v in &verts {
        if stays.contains(&v) {
            let e = match w.t.edge(v, mv[&v]) {
                Some(e) => e,
                None => w.new_edge(v, mv[&v]),
            };
            vertical.insert(v, e);
        }
    }
    for &v in &verts {
        if !stays.contains(&v) {
            w.m.vertices.insert(v, new_pos[&v]);
        }
    }
    // The edges between the copies.
    let mut ring: BTreeMap<(VertexId, VertexId), EdgeId> = BTreeMap::new();
    let mut replaced: BTreeMap<EdgeId, EdgeId> = BTreeMap::new();
    for bd in &bounds {
        let (a2, b2) = (mv[&bd.a], mv[&bd.b]);
        let e = match w.t.edge(a2, b2) {
            Some(e) => e,
            None => w.new_edge(a2, b2),
        };
        if bd.stretch.is_none() {
            ring.insert(key(bd.a, bd.b), e);
        } else if e != bd.edge {
            replaced.insert(bd.edge, e);
        }
    }
    // The boundary edges of the face, with their ends, before the face changes.
    let edges_of: BTreeMap<EdgeId, (VertexId, VertexId)> =
        bounds.iter().map(|b| (b.edge, (b.a, b.b))).collect();

    // Walls on the wall edges.
    for bd in bounds.iter().filter(|b| b.stretch.is_none()) {
        let (a, b) = (bd.a, bd.b);
        let (a2, b2) = (mv[&a], mv[&b]);
        let outward = attached || d > 0.0;
        let l = if outward {
            vec![a, b, b2, a2]
        } else {
            vec![a, a2, b2, b]
        };
        let (p0, p1, p2) = (w.pos(l[0]), w.pos(l[1]), w.pos(l[2]));
        let plane =
            Plane::new(p0, (p1 - p0).cross(p2 - p1)).ok_or(SurfaceError::TooShort { tol: 0.0 })?;
        let id = w.new_face(Face::new(plane, l, Vec::new()));
        faces_out.insert(id, None);
    }

    // The walls that follow an edge with a vertex that stays get the step in their outline.
    for bd in &bounds {
        let Some(g) = bd.stretch else { continue };
        let (sa, sb) = (stays.contains(&bd.a), stays.contains(&bd.b));
        if !sa && !sb {
            faces_out.insert(g, Some(g));
            continue;
        }
        let mut wall = w.m.faces[&g].clone();
        let step = |l: &mut Vec<VertexId>| -> bool {
            let len = l.len();
            let Some(i) = (0..len).find(|&i| l[i] == bd.b && l[(i + 1) % len] == bd.a) else {
                return false;
            };
            l.rotate_left(i);
            let mut out = vec![bd.b];
            if sb {
                out.push(mv[&bd.b]);
            }
            if sa {
                out.push(mv[&bd.a]);
            }
            out.push(bd.a);
            out.extend_from_slice(&l[2..]);
            // A vertical that merged with an edge of the wall goes there and back: cut it off.
            drop_spikes(&mut out);
            *l = out;
            true
        };
        let mut done = step(&mut wall.outer);
        if !done {
            for h in &mut wall.holes {
                if step(h) {
                    done = true;
                    break;
                }
            }
        }
        if !done {
            return Err(SurfaceError::Unsolved(
                "a wall next to the face does not use the shared edge".into(),
            ));
        }
        w.put_face(g, wall);
        faces_out.insert(g, Some(g));
    }

    // The cap keeps the id of the face.
    let lift = |l: &Vec<VertexId>| -> Vec<VertexId> { l.iter().map(|v| mv[v]).collect() };
    let cap_plane = Plane {
        normal: n,
        offset: f.plane.offset + d,
    };
    let mut cap = Face::new(
        cap_plane,
        lift(&f.outer),
        f.holes.iter().map(&lift).collect(),
    );
    cap.attrs = f.attrs.clone();
    cap.extra = f.extra.clone();
    let flip = |g: &mut Face| {
        g.plane = g.plane.flipped();
        g.outer = reversed(&g.outer);
        let mut holes: Vec<Vec<VertexId>> = g.holes.iter().map(|h| reversed(h)).collect();
        holes.sort();
        g.holes = holes;
    };
    if !attached {
        let id = if d > 0.0 {
            // The old position becomes the bottom, turned to the outside.
            let mut bottom = f.clone();
            flip(&mut bottom);
            w.new_face(bottom)
        } else {
            // The old face stays; the cap looks to the back.
            flip(&mut cap);
            w.new_face(f.clone())
        };
        faces_out.insert(id, Some(face));
    }
    w.put_face(face, cap);
    faces_out.insert(face, Some(face));

    // A stretched edge whose end stayed is replaced by the edge between the copies, which takes
    // its attributes and its place in a curve.
    let mut recheck: BTreeSet<CurveId> = BTreeSet::new();
    for (&old, &new) in &replaced {
        if let Some(attrs) = w.m.edge_attrs.get(&old).cloned() {
            let to = w.m.edge_attrs.entry(new).or_default();
            for (k, v) in attrs {
                to.entry(k).or_insert(v);
            }
        }
        let taken = w.m.curves.values().flatten().any(|e| *e == new);
        for (id, es) in &mut w.m.curves {
            if let Some(i) = es.iter().position(|e| *e == old) {
                if taken {
                    es.remove(i);
                } else {
                    es[i] = new;
                }
                recheck.insert(*id);
            }
        }
        w.remove_edge(old);
    }
    rechain(w.m, &recheck);

    // Curves of the wall edges go up to the cap.
    let curves: Vec<Vec<EdgeId>> =
        w.m.curves
            .values()
            .filter(|es| {
                es.iter().all(|e| {
                    edges_of
                        .get(e)
                        .is_some_and(|(a, b)| ring.contains_key(&key(*a, *b)))
                })
            })
            .cloned()
            .collect();
    for es in curves {
        let up: Vec<EdgeId> = es
            .iter()
            .filter_map(|e| {
                let (a, b) = edges_of[e];
                ring.get(&key(a, b)).copied()
            })
            .collect();
        let mut at: BTreeMap<VertexId, u32> = BTreeMap::new();
        for e in &es {
            for v in w.m.edges[e].v {
                *at.entry(v).or_default() += 1;
            }
        }
        for (v, c) in at {
            if c == 2
                && let Some(ve) = vertical.get(&v)
            {
                w.m.edge_attrs
                    .entry(*ve)
                    .or_default()
                    .insert("smooth".into(), serde_json::Value::Bool(true));
            }
        }
        let taken = w.m.curves.values().flatten().any(|e| up.contains(e));
        if up.len() == es.len() && !taken {
            let id = CurveId(w.m.alloc());
            w.m.curves.insert(id, up);
        }
    }
    check_limits(w.m)?;

    let tverts: BTreeSet<VertexId> = verts
        .iter()
        .filter(|v| !stays.contains(v))
        .copied()
        .chain(mv.values().copied())
        .collect();
    let tedges: BTreeSet<EdgeId> = tverts.iter().flat_map(|v| w.t.edges_at(*v)).collect();
    Ok(Touched {
        verts: tverts,
        edges: tedges,
        faces: faces_out,
    })
}

/// Takes out the places where a loop goes to a vertex and straight back (`… x, y, x …`).
fn drop_spikes(l: &mut Vec<VertexId>) {
    loop {
        let n = l.len();
        if n < 3 {
            return;
        }
        let Some(i) = (0..n).find(|&i| l[(i + n - 1) % n] == l[(i + 1) % n]) else {
            return;
        };
        let j = (i + 1) % n;
        if j > i {
            l.drain(i..=j);
        } else {
            l.remove(i);
            l.remove(0);
        }
    }
}

/// Cuts each of the curves that is no longer one chain into chains: the first keeps the id, the
/// others get new ids, so every edge stays in a curve.
fn rechain(m: &mut SurfaceModel, ids: &BTreeSet<CurveId>) {
    for id in ids {
        let Some(es) = m.curves.get(id).cloned() else {
            continue;
        };
        if es.is_empty() {
            m.curves.remove(id);
            continue;
        }
        let mut pieces: Vec<Vec<EdgeId>> = vec![vec![es[0]]];
        for pair in es.windows(2) {
            let (p, q) = (m.edges[&pair[0]].v, m.edges[&pair[1]].v);
            if p.iter().any(|v| q.contains(v)) {
                pieces.last_mut().expect("one piece").push(pair[1]);
            } else {
                pieces.push(vec![pair[1]]);
            }
        }
        let mut pieces = pieces.into_iter();
        m.curves.insert(*id, pieces.next().expect("one piece"));
        for p in pieces {
            let c = CurveId(m.alloc());
            m.curves.insert(c, p);
        }
    }
}

/// The checks of the result of a push: new structural problems of the model, contacts of the new
/// or moved geometry with other faces and edges, and new distance problems of the touched vertices
/// and edges. Problems (and contacts) that the model had before do not count.
fn validate(before: &SurfaceModel, m: &SurfaceModel, t: &Touched) -> Result<(), SurfaceError> {
    let invalid = |p: &Problem| {
        SurfaceError::Unsolved(format!("the push/pull would make the model invalid: {p}"))
    };
    let after = check::structure_problems(m);
    if !after.is_empty() {
        let old = check::structure_problems(before);
        if let Some(p) = after.difference(&old).next() {
            return Err(invalid(p));
        }
    }
    if let Some((e, g, c)) = new_contact(before, m, t) {
        let how = match c {
            Contact::Through => "passes through",
            Contact::EndsOn => "ends on",
            Contact::LiesIn => "lies in",
        };
        return Err(SurfaceError::Unsolved(format!(
            "the push/pull would cut through other faces: edge {e} {how} face {g} (a hole \
             through is a later step)"
        )));
    }
    let after = check::near(m, &t.edges, &t.verts);
    if !after.is_empty() {
        let old = check::near(before, &t.edges, &t.verts);
        if let Some(p) = after.difference(&old).next() {
            return Err(invalid(p));
        }
    }
    Ok(())
}

/// The first contact (in id order) of a touched edge with a face, or of a touched face with an
/// edge, that the model before did not have.
fn new_contact(
    before: &SurfaceModel,
    m: &SurfaceModel,
    t: &Touched,
) -> Option<(EdgeId, FaceId, Contact)> {
    let whole =
        |m: &SurfaceModel, f: &Face| f.loops().flatten().all(|v| m.vertices.contains_key(v));
    let ends = |m: &SurfaceModel, e: EdgeId| {
        let ed = m.edges.get(&e)?;
        Some((*m.vertices.get(&ed.v[0])?, *m.vertices.get(&ed.v[1])?, ed.v))
    };
    let pad = geom::tolerance_at(
        m.vertices
            .values()
            .map(|p| geom::size(*p))
            .fold(0.0, f64::max),
    );
    let faces = BoxTree::new(
        m.faces
            .iter()
            .filter(|(_, f)| whole(m, f))
            .map(|(id, f)| {
                let (lo, hi) = clash::face_box(m, f, pad);
                (lo, hi, id.0)
            })
            .collect(),
    );
    let mut rings: BTreeMap<FaceId, Vec<Vec<DVec2>>> = BTreeMap::new();
    let mut found: BTreeSet<(EdgeId, FaceId)> = BTreeSet::new();
    let mut hits: Vec<(EdgeId, FaceId, Contact)> = Vec::new();
    let mut test = |e: EdgeId, g: FaceId, hits: &mut Vec<(EdgeId, FaceId, Contact)>| {
        if !found.insert((e, g)) {
            return;
        }
        let Some((a, b, vs)) = ends(m, e) else { return };
        let gf = &m.faces[&g];
        let r = rings.entry(g).or_insert_with(|| clash::rings(m, gf));
        if let Some(c) = clash::contact(a, b, vs, gf, r, true) {
            hits.push((e, g, c));
        }
    };
    for &e in &t.edges {
        let Some((a, b, _)) = ends(m, e) else {
            continue;
        };
        let mut cands = Vec::new();
        faces.query(a.min(b), a.max(b), &mut |g| cands.push(FaceId(g)));
        for g in cands {
            test(e, g, &mut hits);
        }
    }
    let edges = BoxTree::new(
        m.edges
            .keys()
            .filter_map(|id| {
                let (a, b, _) = ends(m, *id)?;
                Some((a.min(b), a.max(b), id.0))
            })
            .collect(),
    );
    for &g in t.faces.keys() {
        let Some(gf) = m.faces.get(&g).filter(|f| whole(m, f)) else {
            continue;
        };
        let (lo, hi) = clash::face_box(m, gf, pad);
        let mut cands = Vec::new();
        edges.query(lo, hi, &mut |e| cands.push(EdgeId(e)));
        for e in cands {
            test(e, g, &mut hits);
        }
    }
    hits.sort_by_key(|h| (h.0, h.1));
    hits.into_iter().find(|&(e, g, _)| {
        // The same contact at the place before: the edge as it was, and the face that was there.
        let old = match t.faces.get(&g) {
            Some(o) => *o,
            None => Some(g),
        };
        let Some((a, b, vs)) = ends(before, e) else {
            return true;
        };
        let Some(gf) = old
            .and_then(|o| before.faces.get(&o))
            .filter(|f| whole(before, f))
        else {
            return true;
        };
        clash::contact(a, b, vs, gf, &clash::rings(before, gf), true).is_none()
    })
}
