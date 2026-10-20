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
//! A vertex that only has stretch edges (and no other face that is not at a right angle) moves in
//! place. A vertex that has a wall edge stays, and gets a copy at `normal * distance` with a new
//! vertical edge between them. A stretch neighbour of an edge at such a vertex gets the vertical
//! edge in its outline (a step in the wall), so every face stays flat.
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
//! - A result in which a new edge passes through, or ends on, a face that it does not belong to
//!   (a pocket deeper than the solid), or in which the check of the model finds more problems
//!   than before. The error says why, and the model is unchanged.
//!
//! Curves on the wall edges of `F` are copied to the cap, and the new vertical edges between two
//! pieces of a curve get the attribute `smooth: true` (a cylinder is shaded round).
//!
//! Ids are given in a fixed order (the copies of the vertices in loop order, then the vertical
//! edges, the cap edges, the walls, then the bottom), so the same edit gives the same ids.

use super::*;

const PERPENDICULAR: f64 = 1e-6;

pub(super) fn push_pull(
    m: &mut SurfaceModel,
    face: FaceId,
    distance: f64,
) -> Result<(), SurfaceError> {
    let before = m.check().problems.len();
    run(m, face, distance)?;
    let after = m.check();
    if after.problems.len() > before {
        let why = after
            .problems
            .last()
            .map(ToString::to_string)
            .unwrap_or_default();
        return Err(SurfaceError::Unsolved(format!(
            "the push/pull would make the model invalid: {why}"
        )));
    }
    Ok(())
}

/// One boundary edge of the face.
struct Boundary {
    a: VertexId,
    b: VertexId,
    edge: EdgeId,
    /// The wall that follows the edge, if it is a stretch edge.
    stretch: Option<FaceId>,
}

fn run(m: &mut SurfaceModel, face: FaceId, d: f64) -> Result<(), SurfaceError> {
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
    let mut w = Work::new(m);
    let vset: BTreeSet<VertexId> = verts.iter().copied().collect();

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
            [g] if w.m.faces[g].plane.normal.dot(n).abs() < PERPENDICULAR => Some(*g),
            _ => None,
        };
        bounds.push(Boundary {
            a,
            b,
            edge,
            stretch,
        });
    }
    let attached = any_shared;

    // Vertices that stay and get a copy: the ends of wall edges, and the vertices that a face
    // that is not at a right angle uses (it would not stay flat).
    let mut stays: BTreeSet<VertexId> = bounds
        .iter()
        .filter(|b| b.stretch.is_none())
        .flat_map(|b| [b.a, b.b])
        .collect();
    for (id, g) in &w.m.faces {
        if *id != face && g.plane.normal.dot(n).abs() >= PERPENDICULAR {
            stays.extend(g.loops().flatten().filter(|v| vset.contains(v)));
        }
    }

    // A vertex that moves in place may not reach the far end of an edge along the normal.
    let in_face: BTreeSet<(VertexId, VertexId)> = f.uses().map(|(a, b)| key(a, b)).collect();
    for &v in verts.iter().filter(|v| !stays.contains(v)) {
        for e in w.t.edges_at(v).collect::<Vec<_>>() {
            let edge = w.m.edges[&e];
            if in_face.contains(&edge.key()) {
                continue;
            }
            let Some(o) = edge.other(v) else { continue };
            let dir = w.pos(o) - w.pos(v);
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

    // The copies must not land on a vertex of the model.
    let big =
        w.m.vertices
            .values()
            .fold(0.0f64, |m, p| m.max(geom::size(*p)))
            .max(geom::size(pts[0]) + d.abs());
    let window = geom::tolerance_at(big);
    let mut by_x: Vec<(f64, DVec3)> = w.m.vertices.values().map(|p| (p.x, *p)).collect();
    by_x.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut new_pos: BTreeMap<VertexId, DVec3> = BTreeMap::new();
    for &v in &verts {
        let p = w.pos(v) + n * d;
        if !p.is_finite() || geom::size(p) > MAX_COORD {
            return Err(SurfaceError::BadPoint);
        }
        new_pos.insert(v, p);
        // A vertex that stays leaves a copy behind; one that moves leaves its place free.
        if !stays.contains(&v) {
            continue;
        }
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

    // Copies, in loop order. A vertex that moves is its own copy.
    let mut mv: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    for &v in &verts {
        if stays.contains(&v) {
            let id = w.new_vertex(new_pos[&v]);
            mv.insert(v, id);
        } else {
            mv.insert(v, v);
        }
    }
    let mut new_edges: Vec<EdgeId> = Vec::new();
    let mut vertical: BTreeMap<VertexId, EdgeId> = BTreeMap::new();
    for &v in &verts {
        if stays.contains(&v) {
            let e = w.new_edge(v, mv[&v]);
            vertical.insert(v, e);
            new_edges.push(e);
        }
    }
    for &v in &verts {
        if !stays.contains(&v) {
            w.m.vertices.insert(v, new_pos[&v]);
        }
    }
    // The edges between the copies.
    let mut ring: BTreeMap<(VertexId, VertexId), EdgeId> = BTreeMap::new();
    for bd in &bounds {
        let (a2, b2) = (mv[&bd.a], mv[&bd.b]);
        let e = match w.t.edge(a2, b2) {
            Some(e) => e,
            None => {
                let e = w.new_edge(a2, b2);
                new_edges.push(e);
                e
            }
        };
        if bd.stretch.is_none() {
            ring.insert(key(bd.a, bd.b), e);
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
        w.new_face(Face::new(plane, l, Vec::new()));
    }

    // The walls that follow an edge with a vertex that stays get the step in their outline.
    for bd in &bounds {
        let Some(g) = bd.stretch else { continue };
        let (sa, sb) = (stays.contains(&bd.a), stays.contains(&bd.b));
        if !sa && !sb {
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
    w.put_face(face, cap);

    // An edge whose end stayed is replaced by the edge between the copies.
    for bd in &bounds {
        if bd.stretch.is_some() && (stays.contains(&bd.a) || stays.contains(&bd.b)) {
            w.remove_edge(bd.edge);
        }
    }

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
            let id = super::super::CurveId(w.m.alloc());
            w.m.curves.insert(id, up);
        }
    }
    check_limits(w.m)?;
    if pierces(&w, &new_edges) {
        return Err(SurfaceError::Unsolved(
            "the push/pull would cut through other faces (a hole through is a later step)".into(),
        ));
    }
    Ok(())
}

/// True if a new edge passes through a face that it does not belong to, ends on it, or lies in it.
fn pierces(w: &Work, edges: &[EdgeId]) -> bool {
    for &e in edges {
        let Some(edge) = w.m.edges.get(&e) else {
            continue;
        };
        let (a, b) = (w.pos(edge.v[0]), w.pos(edge.v[1]));
        let pt = geom::plane_tol(&[a, b]);
        for g in w.m.faces.values() {
            if g.loops().flatten().any(|v| edge.v.contains(v)) {
                continue;
            }
            let (sa, sb) = (g.plane.distance(a), g.plane.distance(b));
            let p = if sa.abs() <= pt && sb.abs() <= pt {
                (a + b) / 2.0
            } else if (sa > pt && sb > pt) || (sa < -pt && sb < -pt) {
                continue;
            } else {
                let t = if sa.abs() <= pt {
                    0.0
                } else if sb.abs() <= pt {
                    1.0
                } else {
                    sa / (sa - sb)
                };
                a + (b - a) * t
            };
            let rings: Vec<Vec<DVec2>> = g
                .loops()
                .map(|l| l.iter().map(|v| g.plane.project(w.pos(*v))).collect())
                .collect();
            if inside(g.plane.project(p), &rings) {
                return true;
            }
        }
    }
    false
}
