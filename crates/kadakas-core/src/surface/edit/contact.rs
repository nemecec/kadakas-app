//! The checks of new or changed geometry against the rest of the model: contacts of edges with
//! faces, and distance problems, each only when the model before did not have it at the same
//! place. Push/Pull, Follow Me, Move, Set segments and Fillet use them.

use super::super::check::{self, Problem};
use super::super::clash::{self, Contact};
use super::*;

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

/// The first contact (in id order) of one of the `edges` with a face, or of one of the `faces` with
/// an edge, that the model before did not have at the same place. A pair that `skip` names is
/// left out.
pub(super) fn new_contact(
    before: &SurfaceModel,
    m: &SurfaceModel,
    edges: &BTreeSet<EdgeId>,
    faces: &BTreeSet<FaceId>,
    skip: &dyn Fn(EdgeId, FaceId) -> bool,
) -> Option<(EdgeId, FaceId, Contact)> {
    let whole =
        |m: &SurfaceModel, f: &Face| f.loops().flatten().all(|v| m.vertices.contains_key(v));
    let ends = |m: &SurfaceModel, e: EdgeId| {
        let ed = m.edges.get(&e)?;
        Some((*m.vertices.get(&ed.v[0])?, *m.vertices.get(&ed.v[1])?, ed.v))
    };
    let pad = check::window(m);
    let face_tree = clash::face_tree(m, pad, &|f| whole(m, f));
    let mut shapes = clash::Shapes::new(m);
    let mut found: BTreeSet<(EdgeId, FaceId)> = BTreeSet::new();
    let mut hits: Vec<(EdgeId, FaceId, Contact, DVec3)> = Vec::new();
    let mut test = |e: EdgeId, g: FaceId, hits: &mut Vec<(EdgeId, FaceId, Contact, DVec3)>| {
        if skip(e, g) || !found.insert((e, g)) {
            return;
        }
        let Some((a, b, vs)) = ends(m, e) else { return };
        if let Some((c, at)) = clash::contact(a, b, vs, &m.faces[&g], shapes.get(g), true) {
            hits.push((e, g, c, at));
        }
    };
    for &e in edges {
        let Some((a, b, _)) = ends(m, e) else {
            continue;
        };
        let mut cands = Vec::new();
        face_tree.query(a.min(b), a.max(b), &mut |g| cands.push(FaceId(g)));
        for g in cands {
            test(e, g, &mut hits);
        }
    }
    let edge_tree = clash::edge_tree(m, 0.0);
    for &g in faces {
        let Some(gf) = m.faces.get(&g).filter(|f| whole(m, f)) else {
            continue;
        };
        let (lo, hi) = clash::face_box(m, gf, pad);
        let mut cands = Vec::new();
        edge_tree.query(lo, hi, &mut |e| cands.push(EdgeId(e)));
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
