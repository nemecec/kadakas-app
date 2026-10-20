//! Push/Pull (MOD-08): a face moves along its normal and the faces next to it follow.
//!
//! Two cases, chosen from the faces around the face `F`:
//!
//! 1. **Move.** Every edge of `F` is used by another face, and every face that touches a vertex of
//!    `F` stands at a right angle to `F` (the walls of a prism). Then the vertices of `F` move by
//!    `normal * distance`. The walls stretch or shrink and stay flat. The move may not reach the far
//!    end of a wall edge (pushing through to the opposite face; the hole through a slab is a later
//!    step), that is an error and nothing changes.
//! 2. **Extrude.** Anything else: the vertices of `F` are copied by `normal * distance`, a wall
//!    face is made on each edge of `F`, and `F` (keeping its id) becomes the cap on the copies.
//!    - A face with no neighbour (a lone sketch) becomes a closed solid. A new face keeps the old
//!      position, turned to the outside; for a push to the back, the cap is turned instead.
//!    - A face that sits on other faces (a rectangle drawn on a wall, a bump) leaves the opening
//!      where it was. A pull adds a bump with the walls outward; a push digs a pocket and the walls
//!      face into it (the walls always face the empty side).
//!
//!    Curves on the edges of `F` are copied to the edges of the cap, and the new edges between two
//!    pieces of a curve get the attribute `smooth: true` (a cylinder is shaded round).
//!
//! Ids are given in a fixed order (the copies of the vertices in loop order, then the edges from
//! the vertices up and along the cap, then the walls, then the bottom), so the same edit gives the
//! same ids.

use super::*;

pub(super) fn push_pull(
    m: &mut SurfaceModel,
    face: FaceId,
    distance: f64,
) -> Result<(), SurfaceError> {
    let f = m
        .faces
        .get(&face)
        .cloned()
        .ok_or(SurfaceError::NoFace(face))?;
    if !distance.is_finite() {
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
    if distance.abs() <= tol {
        return Err(SurfaceError::TooShort { tol });
    }
    let mut w = Work::new(m);
    let shared = |w: &Work, a: VertexId, b: VertexId| -> bool {
        w.t.edge(a, b)
            .is_some_and(|e| w.t.faces_of(e).any(|g| g != face))
    };
    let attached = f.uses().any(|(a, b)| shared(&w, a, b));
    let all_shared = f.uses().all(|(a, b)| shared(&w, a, b));
    let vset: BTreeSet<VertexId> = verts.iter().copied().collect();
    let perpendicular =
        w.m.faces
            .iter()
            .filter(|(id, g)| **id != face && g.loops().flatten().any(|v| vset.contains(v)))
            .all(|(_, g)| g.plane.normal.dot(n).abs() < 1e-6);
    if all_shared && perpendicular {
        move_face(&mut w, face, &f, &verts, distance, tol)
    } else {
        extrude(&mut w, face, &f, &verts, distance, attached)
    }
}

fn move_face(
    w: &mut Work,
    face: FaceId,
    f: &Face,
    verts: &[VertexId],
    d: f64,
    tol: f64,
) -> Result<(), SurfaceError> {
    let n = f.plane.normal;
    let in_face: BTreeSet<(VertexId, VertexId)> = f.uses().map(|(a, b)| key(a, b)).collect();
    for &v in verts {
        for e in w.t.edges_at(v).collect::<Vec<_>>() {
            let edge = w.m.edges[&e];
            if in_face.contains(&edge.key()) {
                continue;
            }
            let Some(o) = edge.other(v) else { continue };
            let dir = w.pos(o) - w.pos(v);
            // Only the edges that run along the normal change length.
            if dir.cross(n).length() > 1e-6 * dir.length().max(1.0) {
                continue;
            }
            let s = dir.dot(n);
            let s2 = s - d;
            if s2 * s <= 0.0 || s2.abs() <= tol {
                return Err(SurfaceError::Unsolved(
                    "the face cannot be pushed through to the opposite face yet".into(),
                ));
            }
        }
    }
    for &v in verts {
        let p = w.pos(v) + n * d;
        if !p.is_finite() || geom::size(p) > MAX_COORD {
            return Err(SurfaceError::BadPoint);
        }
        w.m.vertices.insert(v, p);
    }
    let mut g = w.m.faces[&face].clone();
    g.plane.offset += d;
    w.put_face(face, g);
    check_limits(w.m)
}

fn extrude(
    w: &mut Work,
    face: FaceId,
    f: &Face,
    verts: &[VertexId],
    d: f64,
    attached: bool,
) -> Result<(), SurfaceError> {
    let n = f.plane.normal;
    let mut map: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    let mut new_pts = Vec::new();
    for &v in verts {
        let p = w.pos(v) + n * d;
        if !p.is_finite() || geom::size(p) > MAX_COORD {
            return Err(SurfaceError::BadPoint);
        }
        if w.m
            .vertices
            .values()
            .any(|q| p.distance(*q) <= geom::tol(&[p, *q]))
        {
            return Err(SurfaceError::TooClose {
                tol: geom::tol(&[p]),
            });
        }
        new_pts.push((v, p));
    }
    for (v, p) in new_pts {
        let id = w.new_vertex(p);
        map.insert(v, id);
    }
    let mut vertical: BTreeMap<VertexId, EdgeId> = BTreeMap::new();
    for &v in verts {
        let e = w.new_edge(v, map[&v]);
        vertical.insert(v, e);
    }
    let mut ring: BTreeMap<(VertexId, VertexId), EdgeId> = BTreeMap::new();
    for (a, b) in f.uses() {
        let (a2, b2) = (map[&a], map[&b]);
        let e = match w.t.edge(a2, b2) {
            Some(e) => e,
            None => w.new_edge(a2, b2),
        };
        ring.insert(key(a, b), e);
    }
    // The boundary edges of the face, with their ends, before the face changes.
    let edges_of: BTreeMap<EdgeId, (VertexId, VertexId)> = f
        .uses()
        .filter_map(|(a, b)| w.t.edge(a, b).map(|e| (e, (a, b))))
        .collect();
    // Walls.
    for (a, b) in f.uses() {
        let (a2, b2) = (map[&a], map[&b]);
        let outward = attached || d > 0.0;
        let l = if outward {
            vec![a, b, b2, a2]
        } else {
            vec![a, a2, b2, b]
        };
        let (p0, p1, p2) = (w.pos(l[0]), w.pos(l[1]), w.pos(l[2]));
        let plane =
            Plane::new(p0, (p1 - p0).cross(p2 - p1)).ok_or(SurfaceError::TooShort { tol: 0.0 })?;
        w.new_face(Face::new(plane, l, Vec::new()));
    }
    // The cap keeps the id of the face.
    let lift = |l: &Vec<VertexId>| -> Vec<VertexId> { l.iter().map(|v| map[v]).collect() };
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
        if d > 0.0 {
            // The old position becomes the bottom, turned to the outside.
            let mut bottom = f.clone();
            flip(&mut bottom);
            w.new_face(bottom);
        } else {
            // The old face stays; the cap looks to the back.
            w.new_face(f.clone());
            flip(&mut cap);
        }
    }
    // (A face on other faces leaves the opening where it was.)
    w.put_face(face, cap);
    // Curves of the boundary go up to the cap.
    let curves: Vec<Vec<EdgeId>> =
        w.m.curves
            .values()
            .filter(|es| es.iter().all(|e| edges_of.contains_key(e)))
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
            let id = super::super::CurveId(w.m.alloc());
            w.m.curves.insert(id, up);
        }
    }
    check_limits(w.m)
}
