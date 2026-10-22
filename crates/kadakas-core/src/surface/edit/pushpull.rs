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
//! # Flush
//!
//! When a vertex of the face lands on the far end of an edge along the normal (a bump pushed back
//! down, a pocket floor pulled up to the top, the second half of a split top pushed to the level
//! of the first), the walls between collapse: the vertex is welded to the one it meets, edges
//! that fall together become one (the older id stays and takes the attributes and the place in a
//! curve of the other), loops and faces that have no area left go, and the face merges with a
//! face of the same plane and side that it now shares an edge with, by the rules of Erase (the
//! face with the lower id stays). A push that passes a vertex along the normal (the top pushed
//! past a line across a wall) goes in steps: first to that vertex, then on. A face that merged on
//! the way does not go on: the push is refused (a pocket floor with a bump on it, pulled to the
//! top, stops at the bump; the next push goes on). This is by design.
//!
//! The same weld happens when a vertex that moves, or the copy of a vertex that stays, lands on a
//! vertex or inside an edge of a face in the plane that the cap goes to (a square at the foot of a
//! step pulled up to the high side): the edge is split at the landing point first, a welded
//! T-junction. A copy that lands is made and then welded, so the stretched walls next to it go
//! through the same collapse as the walls of a moving vertex; a wall whose loop touches itself
//! after the weld (its middle collapsed) is cut into the parts that keep an area. A push that ends
//! within the length tolerance of such geometry goes exactly to it (`Plan::Snap`), so the welded
//! vertices lie in the plane of the cap.
//!
//! Ids: the vertex that was there stays (the vertex that moved or the copy goes), of edges that
//! fall together the lower id, of faces that merge the lower id. An edge that falls onto an edge
//! of a curve leaves its own curve, so an edge is never in two curves. The attributes of merged
//! faces follow `merge_attrs` (in `edit`), and each change is a note of the edit.
//!
//! # Refused
//!
//! - A move that reaches the opposite face (the face becomes flush with a face that looks the
//!   other way, beside it or back to back under it, or a copy ends on it). The hole through a
//!   slab is a later step.
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
use super::*;

/// The most steps of one push (each passes a vertex along the normal).
const MAX_STEPS: usize = 64;

/// Pushes `face` of `m` by `distance`. `before` is the model before the edit (`m` is its copy).
/// The notes of the merges go to `notes`.
pub(super) fn push_pull(
    m: &mut SurfaceModel,
    before: &SurfaceModel,
    face: FaceId,
    distance: f64,
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    let mut rest = distance;
    // The model before the current step (the first step starts from `before`).
    let mut base: Option<SurfaceModel> = None;
    for _ in 0..MAX_STEPS {
        let from = base.as_ref().unwrap_or(before);
        let plan = match run(m, face, rest, true)? {
            // Nearly flush: the push goes exactly to the geometry it lands on.
            Plan::Snap(s) => {
                rest = s;
                run(m, face, s, false)?
            }
            p => p,
        };
        match plan {
            Plan::Done(t, welds) => {
                finish_and_check(m, from, t, &welds, notes)?;
                return Ok(());
            }
            Plan::Snap(_) => {
                return Err(SurfaceError::Unsolved(
                    "the push/pull does not land exactly on the geometry it meets".into(),
                ));
            }
            Plan::Step(d) => {
                // Nothing changed yet: push to the vertex on the way first.
                let Plan::Done(t, welds) = run(m, face, d, false)? else {
                    return Err(SurfaceError::Unsolved(
                        "the push/pull does not reach the vertex on its way".into(),
                    ));
                };
                if finish_and_check(m, from, t, &welds, notes)? {
                    return Err(SurfaceError::Unsolved(format!(
                        "the face becomes flush with another face after {} mm and merges with \
                         it; a push past that is a later step",
                        d.abs()
                    )));
                }
                rest -= d;
                base = Some(m.clone());
            }
        }
    }
    Err(SurfaceError::Unsolved(
        "the push/pull passes too many vertices on its way".into(),
    ))
}

/// What `run` found: the push is done (and what it touched, and the vertices to weld), it must
/// first go to a vertex on its way (`Step(distance)`, nothing changed), or it lands within the
/// tolerance of geometry and must go exactly there (`Snap(distance)`, nothing changed).
enum Plan {
    Done(Touched, BTreeMap<VertexId, VertexId>),
    Step(f64),
    Snap(f64),
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
    /// The faces that are new or changed.
    faces: BTreeSet<FaceId>,
    /// The face that was pushed (the cap, unless it merged).
    face: FaceId,
}

/// One push of `face` by `d`. With `snap`, a push that lands within the tolerance of geometry but
/// not exactly on it returns `Snap` with the exact distance.
fn run(m: &mut SurfaceModel, face: FaceId, d: f64, snap: bool) -> Result<Plan, SurfaceError> {
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
    let mut merge: BTreeMap<VertexId, EdgeId> = BTreeMap::new();
    // Flush: a vertex that moves onto the far end of the edge is welded to it after the push; the
    // copy of a vertex that stays is that end.
    let mut welds: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    let mut lands: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    // The nearest vertex on the way: the push goes there first.
    let mut step: Option<f64> = None;
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
            if (s - d).abs() <= tol {
                if stays.contains(&v) {
                    lands.insert(v, o);
                } else {
                    welds.insert(v, o);
                }
            } else if s.abs() < d.abs() {
                if step.is_none_or(|x| s.abs() < x.abs()) {
                    step = Some(s);
                }
            } else if stays.contains(&v) {
                // The copy lies on the edge: the edge is split there and its part is the vertical.
                merge.insert(v, e);
            }
        }
    }

    if let Some(s) = step {
        return Ok(Plan::Step(s));
    }

    // Flush in general: a vertex that moves, or the copy of one that stays, that lands on a vertex
    // or inside an edge of the geometry that does not move, is welded to it after the push (the
    // edge is split there first: a welded T-junction). The far end of an edge along the normal
    // (above) is one such case.
    let in_verts: BTreeSet<VertexId> = verts.iter().copied().collect();
    let moving = |x: &VertexId| in_verts.contains(x) && !stays.contains(x);
    let big =
        w.m.vertices
            .values()
            .fold(0.0f64, |m, p| m.max(geom::size(*p)))
            .max(geom::size(pts[0]) + d.abs());
    // Only the edges near the box of the cap: a cheap filter first, so a big model costs one
    // pass over its edges and a small tree.
    let pad = DVec3::splat(geom::tolerance_at(big));
    let (clo, chi) = geom::bounds(new_pos.values().copied()).unwrap_or_default();
    let (clo, chi) = (clo - pad, chi + pad);
    let tree = super::super::index::BoxTree::new(
        w.m.edges
            .iter()
            .filter_map(|(id, e)| {
                let (a, b) = (w.m.vertices[&e.v[0]], w.m.vertices[&e.v[1]]);
                let (lo, hi) = (a.min(b) - pad, a.max(b) + pad);
                (lo.cmple(chi).all() && hi.cmpge(clo).all()).then_some((lo, hi, id.0))
            })
            .collect(),
    );
    let mut on_vertex: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    let mut on_edge: BTreeMap<EdgeId, Vec<(f64, VertexId, DVec3)>> = BTreeMap::new();
    // Where each vertex lands, for the snap.
    let mut landing: Vec<(VertexId, DVec3)> = Vec::new();
    for &v in &verts {
        if let Some(o) = welds.get(&v).or(lands.get(&v)) {
            landing.push((v, w.pos(*o)));
            continue;
        }
        if merge.contains_key(&v) {
            continue;
        }
        let p = new_pos[&v];
        let mut best_v: Option<(f64, VertexId)> = None;
        let mut best_e: Option<(f64, EdgeId, f64, DVec3)> = None;
        tree.query(p, p, &mut |e| {
            let e = EdgeId(e);
            let ed = w.m.edges[&e];
            // Only the geometry of a face in the plane that the cap goes to: the face meets it
            // there (a stray line that the push touches is not welded, the checks refuse it).
            let flush = || {
                w.t.faces_of(e).any(|g| {
                    let gf = &w.m.faces[&g];
                    gf.plane.normal.dot(n).abs() >= 1.0 - 1e-9 && on_plane(&gf.plane, p)
                })
            };
            if ed.v.contains(&v) || ed.v.iter().any(&moving) || !flush() {
                return;
            }
            let (a, b) = (w.pos(ed.v[0]), w.pos(ed.v[1]));
            for (x, q) in [(ed.v[0], a), (ed.v[1], b)] {
                let dist = p.distance(q);
                if !in_verts.contains(&x)
                    && dist <= geom::tol(&[p, q])
                    && best_v.is_none_or(|(bd, bx)| dist < bd || (dist == bd && x < bx))
                {
                    best_v = Some((dist, x));
                }
            }
            let (dist, t) = geom::point_segment(p, a, b);
            let (len, tt) = (a.distance(b), geom::tol(&[p, a, b]));
            if dist <= tt
                && t * len > tt
                && (1.0 - t) * len > tt
                && best_e.is_none_or(|(bd, be, ..)| dist < bd || (dist == bd && e < be))
            {
                best_e = Some((dist, e, t, a + (b - a) * t));
            }
        });
        if let Some((_, q)) = best_v {
            on_vertex.insert(v, q);
            landing.push((v, w.pos(q)));
        } else if let Some((_, e, t, x)) = best_e {
            on_edge.entry(e).or_default().push((t, v, x));
            landing.push((v, x));
        }
    }
    // Nearly flush: go exactly to the first landing point, so that the cap is in the plane of
    // the points it is welded to.
    if snap {
        let fine = geom::plane_tol(&pts) / 4.0;
        if let Some(s) = landing
            .iter()
            .map(|(v, q)| (*q - w.pos(*v)).dot(n))
            .find(|s| (s - d).abs() > fine)
        {
            return Ok(Plan::Snap(s));
        }
    }

    let mut faces_out: BTreeSet<FaceId> = BTreeSet::new();
    // Split the edges that vertices land in. The points of one edge are split from its far end,
    // so each next point lies on the part that keeps the id.
    let mut split_at: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    for (e, mut hits) in on_edge {
        faces_out.extend(w.t.faces_of(e).collect::<Vec<_>>());
        hits.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut last: Option<(DVec3, VertexId)> = None;
        for (_, v, x) in hits {
            let c = match last {
                Some((y, c)) if y.distance(x) <= geom::tol(&[x, y]) => c,
                _ => w.split_edge(e, x, &mut BTreeSet::new()),
            };
            last = Some((x, c));
            split_at.insert(v, c);
        }
    }
    // Copies, in loop order. A vertex that moves is its own copy.
    let mut mv: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    for &v in &verts {
        let target = lands
            .get(&v)
            .or(on_vertex.get(&v))
            .or(split_at.get(&v))
            .copied();
        if !stays.contains(&v) {
            mv.insert(v, v);
            if let Some(o) = target {
                welds.insert(v, o);
            }
        } else if let Some(&e) = merge.get(&v) {
            for g in w.t.faces_of(e).collect::<Vec<_>>() {
                faces_out.insert(g);
            }
            let mut keys = BTreeSet::new();
            let c = w.split_edge(e, new_pos[&v], &mut keys);
            mv.insert(v, c);
        } else {
            // A copy that lands is welded to the vertex it meets after the push, so the walls
            // that collapse there go through the weld.
            let id = w.new_vertex(new_pos[&v]);
            mv.insert(v, id);
            if let Some(o) = target {
                welds.insert(id, o);
            }
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
        faces_out.insert(id);
    }

    // The walls that follow an edge with a vertex that stays get the step in their outline.
    for bd in &bounds {
        let Some(g) = bd.stretch else { continue };
        let (sa, sb) = (stays.contains(&bd.a), stays.contains(&bd.b));
        if !sa && !sb {
            faces_out.insert(g);
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
        faces_out.insert(g);
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
        faces_out.insert(id);
    }
    w.put_face(face, cap);
    faces_out.insert(face);

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
    Ok(Plan::Done(
        Touched {
            verts: tverts,
            edges: tedges,
            faces: faces_out,
            face,
        },
        welds,
    ))
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

/// Welds the flush vertices, merges the faces that became one, and checks the result against
/// `before`. True if the pushed face merged with another face.
fn finish_and_check(
    m: &mut SurfaceModel,
    before: &SurfaceModel,
    t: Touched,
    welds: &BTreeMap<VertexId, VertexId>,
    notes: &mut Vec<String>,
) -> Result<bool, SurfaceError> {
    let to = |v: VertexId| *welds.get(&v).unwrap_or(&v);
    if !welds.is_empty() {
        weld(m, welds, notes);
    }
    let targets: BTreeSet<VertexId> = t.verts.iter().map(|v| to(*v)).collect();
    let merges = flush_merges(m, t.face, &targets)?;
    let merged = !merges.is_empty();
    if merged {
        super::erase_edges(m, &merges, notes)?;
    }
    // What is touched now: the vertices that are left, their edges, and the faces at them.
    let verts: BTreeSet<VertexId> = targets
        .into_iter()
        .filter(|v| m.vertices.contains_key(v))
        .collect();
    let edges: BTreeSet<EdgeId> = m
        .edges
        .iter()
        .filter(|(_, e)| e.v.iter().any(|v| verts.contains(v)))
        .map(|(id, _)| *id)
        .collect();
    let mut faces: BTreeSet<FaceId> = t
        .faces
        .iter()
        .copied()
        .filter(|f| m.faces.contains_key(f))
        .collect();
    if merged {
        faces.extend(
            m.faces
                .iter()
                .filter(|(_, f)| f.loops().flatten().any(|v| verts.contains(v)))
                .map(|(id, _)| *id),
        );
    }
    let t = Touched {
        verts,
        edges,
        faces,
        face: t.face,
    };
    validate(before, m, &t)?;
    Ok(merged)
}

/// Replaces each vertex of `map` by its value (the vertex that was there stays, the one that
/// moved onto it goes): edges that fall on one vertex go, edges that fall together become one
/// (the lower id stays and takes the attributes of the other, and its place in a curve when the
/// kept edge is in no curve; else the other leaves its curve, so an edge is never in two curves),
/// loops that lose their area go, a face whose outer loop loses its area goes, and a face whose
/// loop now touches itself (a wall whose middle collapsed) is cut into the parts that keep an
/// area. A face that goes with attributes, and an edge attribute that is dropped, give a note.
pub(super) fn weld(
    m: &mut SurfaceModel,
    map: &BTreeMap<VertexId, VertexId>,
    notes: &mut Vec<String>,
) {
    let to = |v: VertexId| *map.get(&v).unwrap_or(&v);
    let mut by_key: BTreeMap<(VertexId, VertexId), EdgeId> = BTreeMap::new();
    let mut gone: Vec<(EdgeId, Option<EdgeId>)> = Vec::new();
    let ids: Vec<EdgeId> = m.edges.keys().copied().collect();
    for id in ids {
        let e = m.edges[&id];
        let nv = e.v.map(to);
        if nv[0] == nv[1] {
            gone.push((id, None));
            continue;
        }
        match by_key.entry(key(nv[0], nv[1])) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(id);
                m.edges.insert(id, Edge { v: nv });
            }
            std::collections::btree_map::Entry::Occupied(kept) => {
                gone.push((id, Some(*kept.get())))
            }
        }
    }
    let mut recheck: BTreeSet<CurveId> = BTreeSet::new();
    for (id, kept) in gone {
        m.edges.remove(&id);
        let attrs = m.edge_attrs.remove(&id);
        if let (Some(k), Some(attrs)) = (kept, attrs) {
            let into = m.edge_attrs.entry(k).or_default();
            for (name, v) in attrs {
                match into.get(&name) {
                    Some(own) if *own != v => notes.push(format!(
                        "edge {id} fell onto edge {k}: its attribute {name} = {v} was dropped \
                         (edge {k} keeps {own})"
                    )),
                    Some(_) => {}
                    None => {
                        into.insert(name, v);
                    }
                }
            }
        }
        // The kept edge already in a curve (the ring of a bump pushed back onto the ring it
        // stands on) keeps that place; the other edge leaves its own curve.
        let k_curved = kept.is_some_and(|k| m.curves.values().any(|es| es.contains(&k)));
        for (c, es) in &mut m.curves {
            let Some(i) = es.iter().position(|e| *e == id) else {
                continue;
            };
            match kept {
                Some(k) if !k_curved => es[i] = k,
                _ => {
                    es.remove(i);
                }
            }
            recheck.insert(*c);
        }
    }
    m.curves.retain(|_, es| !es.is_empty());
    for v in map.keys() {
        m.vertices.remove(v);
    }
    let ids: Vec<FaceId> = m
        .faces
        .iter()
        .filter(|(_, f)| f.loops().flatten().any(|v| map.contains_key(v)))
        .map(|(id, _)| *id)
        .collect();
    let gone_note = |id: FaceId, f: &Face, notes: &mut Vec<String>| {
        if !f.attrs.is_empty() || !f.extra.is_empty() {
            notes.push(format!(
                "face {id} lost its area in the flush and went with its attributes"
            ));
        }
    };
    let mut parts: Vec<Face> = Vec::new();
    for id in ids {
        let mut f = m.faces.remove(&id).expect("listed");
        let clean = |l: &Vec<VertexId>| -> Option<Vec<VertexId>> {
            let mut l: Vec<VertexId> = l.iter().map(|v| to(*v)).collect();
            l.dedup();
            while l.len() > 1 && l.first() == l.last() {
                l.pop();
            }
            drop_spikes(&mut l);
            let pts: Vec<DVec3> = l.iter().map(|v| m.vertices[v]).collect();
            let round: f64 = (0..pts.len())
                .map(|i| pts[i].distance(pts[(i + 1) % pts.len()]))
                .sum();
            let area2 = geom::newell(&pts).length();
            (l.len() >= 3 && area2 > geom::tol(&pts) * round).then_some(l)
        };
        let Some(outer) = clean(&f.outer) else {
            gone_note(id, &f, notes);
            continue;
        };
        f.outer = outer;
        f.holes = f.holes.iter().filter_map(&clean).collect();
        f.holes.sort();
        let touches_itself = f.loops().any(|l| {
            let mut seen = BTreeSet::new();
            !l.iter().all(|v| seen.insert(*v))
        });
        if !touches_itself {
            m.faces.insert(id, f);
            continue;
        }
        let mut pieces = cut_apart(m, &f).into_iter();
        match pieces.next() {
            Some(first) => {
                m.faces.insert(id, first);
                parts.extend(pieces);
            }
            None => gone_note(id, &f, notes),
        }
    }
    for f in parts {
        let id = FaceId(m.alloc());
        m.faces.insert(id, f);
    }
    rechain(m, &recheck);
}

/// The parts of a face whose loops touch themselves: its sides solved in its plane (a side that
/// the face uses both ways has the face on both sides and goes), each region on the inner side of
/// the face's own loops a face with the attributes of the face, the largest first.
fn cut_apart(m: &SurfaceModel, f: &Face) -> Vec<Face> {
    let uses: BTreeSet<(VertexId, VertexId)> = f.uses().collect();
    let keys: BTreeSet<(VertexId, VertexId)> = uses.iter().map(|&(a, b)| key(a, b)).collect();
    let arr = Arrangement::solve(m, f.plane, &keys);
    let mut regions: Vec<&Region> = arr
        .regions
        .iter()
        .filter(|r| {
            let n = r.outer.len();
            (0..n).any(|i| uses.contains(&(r.outer[i], r.outer[(i + 1) % n])))
        })
        .collect();
    regions.sort_by(|a, b| b.area.total_cmp(&a.area).then(a.outer.cmp(&b.outer)));
    regions
        .into_iter()
        .map(|r| from_region(r, &f.plane, f.plane, Some(f)))
        .collect()
}

/// The edges at `targets` where the pushed face now meets a face of the same plane: to be erased
/// when the two look the same way (they merge), refused when they look opposite ways (the push
/// reached the opposite face).
fn flush_merges(
    m: &SurfaceModel,
    face: FaceId,
    targets: &BTreeSet<VertexId>,
) -> Result<Vec<EdgeId>, SurfaceError> {
    let Some(f) = m.faces.get(&face) else {
        return Ok(Vec::new());
    };
    let mine: BTreeMap<(VertexId, VertexId), bool> = f
        .uses()
        .filter(|(a, b)| targets.contains(a) || targets.contains(b))
        .map(|(a, b)| (key(a, b), a < b))
        .collect();
    let mut out: BTreeSet<EdgeId> = BTreeSet::new();
    for (&id, g) in &m.faces {
        if id == face
            || g.plane.normal.dot(f.plane.normal).abs() < 1.0 - 1e-9
            || !on_plane(&f.plane, m.vertices[&g.outer[0]])
        {
            continue;
        }
        let same_side = g.plane.normal.dot(f.plane.normal) > 0.0;
        for (a, b) in g.uses() {
            let Some(&dir) = mine.get(&key(a, b)) else {
                continue;
            };
            let same_way = dir == (a < b);
            if same_side && !same_way {
                if let Some(e) = m.edge_between(a, b) {
                    out.insert(e);
                }
            } else if !same_side {
                // A face that looks the other way: beside the face (it uses the edge the same
                // way) or under it (back to back: a slab pushed to zero thickness).
                return Err(SurfaceError::Unsolved(
                    "the face cannot be pushed through to the opposite face yet".into(),
                ));
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// The checks of the result of a push: new structural problems of the model, contacts of the new
/// or moved geometry with other faces and edges, and new distance problems of the touched vertices
/// and edges. A problem or contact counts as old only when the model before had one of the same
/// kind at the same place (so new ids for old geometry do not matter, and old ids at a new place
/// do).
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
    match new_distance_problem(before, m, &t.edges, &t.verts) {
        Some(p) => Err(invalid(&p)),
        None => Ok(()),
    }
}

/// The first new distance problem (see `check::near`) of the vertices `verts` and the edges
/// `edges` of `m`: one that the model `before` did not have at the same place with the same kind.
pub(super) fn new_distance_problem(
    before: &SurfaceModel,
    m: &SurfaceModel,
    edges: &BTreeSet<EdgeId>,
    verts: &BTreeSet<VertexId>,
) -> Option<Problem> {
    let after = check::near(m, edges, verts);
    if after.is_empty() {
        return None;
    }
    let pad = check::window(before).max(check::window(m));
    let tree = clash::edge_tree(before, pad);
    for p in &after {
        let Some(at) = check::place(m, p) else {
            return Some(p.clone());
        };
        // The problems of the model before around the same place.
        let mut es: BTreeSet<EdgeId> = BTreeSet::new();
        tree.query(at, at, &mut |e| {
            es.insert(EdgeId(e));
        });
        let vs: BTreeSet<VertexId> = es.iter().flat_map(|e| before.edges[e].v).collect();
        let tol = geom::tol(&[at]);
        let old = check::near(before, &es, &vs).iter().any(|q| {
            std::mem::discriminant(q) == std::mem::discriminant(p)
                && check::place(before, q).is_some_and(|x| x.distance(at) <= tol)
        });
        if !old {
            return Some(p.clone());
        }
    }
    None
}

/// The first contact (in id order) of a touched edge with a face, or of a touched face with an
/// edge, that the model before did not have at the same place.
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
    let pad = check::window(m);
    let faces = clash::face_tree(m, pad, &|f| whole(m, f));
    let mut shapes = clash::Shapes::new(m);
    let mut found: BTreeSet<(EdgeId, FaceId)> = BTreeSet::new();
    let mut hits: Vec<(EdgeId, FaceId, Contact, DVec3)> = Vec::new();
    let mut test = |e: EdgeId, g: FaceId, hits: &mut Vec<(EdgeId, FaceId, Contact, DVec3)>| {
        if !found.insert((e, g)) {
            return;
        }
        let Some((a, b, vs)) = ends(m, e) else { return };
        if let Some((c, at)) = clash::contact(a, b, vs, &m.faces[&g], shapes.get(g), true) {
            hits.push((e, g, c, at));
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
    let edges = clash::edge_tree(m, 0.0);
    for &g in &t.faces {
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
    if hits.is_empty() {
        return None;
    }
    hits.sort_by_key(|h| (h.0, h.1));
    // The same contact before: an edge along the same line through the place, and a face in the
    // same plane at the place, that meet.
    let bpad = check::window(before).max(pad);
    let bfaces = clash::face_tree(before, bpad, &|f| whole(before, f));
    let bedges = clash::edge_tree(before, bpad);
    let mut bshapes = clash::Shapes::new(before);
    hits.into_iter()
        .find(|&(e, g, _, at)| {
            let (a, b, _) = ends(m, e).expect("a hit has its edge");
            let dir = (b - a).normalize_or_zero();
            let gp = m.faces[&g].plane;
            let tol = geom::tol(&[at]);
            let mut es = Vec::new();
            bedges.query(at, at, &mut |x| es.push(EdgeId(x)));
            let mut gs = Vec::new();
            bfaces.query(at, at, &mut |x| gs.push(FaceId(x)));
            let old = es.iter().any(|&x| {
                let Some((c, d, vs)) = ends(before, x) else {
                    return false;
                };
                if geom::point_segment(at, c, d).0 > tol
                    || (d - c).normalize_or_zero().cross(dir).length() > 1e-6
                {
                    return false;
                }
                gs.iter().any(|&y| {
                    let bg = &before.faces[&y];
                    bg.plane.normal.dot(gp.normal).abs() >= 1.0 - 1e-9
                        && on_plane(&bg.plane, at)
                        && clash::contact(c, d, vs, bg, bshapes.get(y), true).is_some()
                })
            });
            !old
        })
        .map(|(e, g, c, _)| (e, g, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cap_edge_welded_onto_an_edge_of_a_curve_leaves_its_own_curve() {
        // A cylinder of 12 sides pulled 300 out of the top of a box: the ring on the top and its
        // copy on the cap are two curves.
        let mut m = SurfaceModel::new();
        let sq = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0], [0.0, 1000.0]];
        m.apply(&Edit::AddEdges {
            edges: (0..4)
                .map(|i| {
                    let (a, b) = (sq[i], sq[(i + 1) % 4]);
                    [[a[0], a[1], 0.0], [b[0], b[1], 0.0]]
                })
                .collect(),
        })
        .unwrap();
        let base = *m.faces.keys().next().unwrap();
        m.apply(&Edit::PushPull {
            face: base,
            distance: 500.0,
        })
        .unwrap();
        let points: Vec<[f64; 3]> = (0..12)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 12.0;
                [500.0 + 300.0 * a.cos(), 500.0 + 300.0 * a.sin(), 500.0]
            })
            .collect();
        m.apply(&Edit::AddCurve {
            points,
            closed: true,
        })
        .unwrap();
        let disc = *m.faces.iter().find(|(_, f)| f.outer.len() == 12).unwrap().0;
        m.apply(&Edit::PushPull {
            face: disc,
            distance: 300.0,
        })
        .unwrap();
        assert_eq!(m.curves.len(), 2);
        // Weld each vertex of the cap onto the ring under it, as a push back down does.
        let ring: Vec<(VertexId, DVec3)> = m
            .vertices
            .iter()
            .filter(|(_, p)| p.z == 500.0)
            .map(|(id, p)| (*id, *p))
            .collect();
        let map: BTreeMap<VertexId, VertexId> = m.faces[&disc]
            .outer
            .iter()
            .map(|v| {
                let p = m.vertices[v] - DVec3::Z * 300.0;
                let under = ring.iter().find(|(_, q)| q.distance(p) < 1e-6).unwrap().0;
                (*v, under)
            })
            .collect();
        let mut notes = Vec::new();
        weld(&mut m, &map, &mut notes);
        assert_eq!(check::curves(&m), None, "no edge is in two curves");
        assert_eq!(m.curves.len(), 1);
        assert_eq!(m.curves.values().next().unwrap().len(), 12);
        assert!(notes.is_empty(), "{notes:?}");
    }
}
