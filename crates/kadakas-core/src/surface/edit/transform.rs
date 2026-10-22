//! Move, Rotate and Scale of geometry, and copies of it (MOD-05, MOD-08).
//!
//! # `TransformVertices`
//!
//! The moved set is the given vertices, all vertices of the given faces and the ends of the given
//! edges. Each moves by the affine map `matrix` (a move, a rotation, a scale, or any mix; a map
//! that cannot be inverted is refused). Then, in this order:
//!
//! 1. **Weld.** A moved vertex that lands within the length tolerance of another vertex becomes
//!    that vertex (the vertex that was there stays; of two that moved, the lower id; the moved
//!    ones go). Edges that fall together become one, loops and faces that lose their area go
//!    (the weld of Push/Pull, with its notes). A vertex that lands on the inside of an edge, and
//!    an edge that is moved over a vertex, split there (a welded T-junction), so the model has no
//!    vertex on an edge.
//! 2. **Planes.** Every face that has a moved vertex is looked at again. A face whose vertices
//!    were all moved is mapped exactly (an affine map keeps planes: the new normal is the old one
//!    times the inverse transpose of the matrix, and the loops turn around when the map is a
//!    mirror), so it never folds. Any other face keeps its plane if all its vertices still lie on
//!    it; else it is refitted (Newell normal, mean offset) and is flat if all its vertices lie
//!    within the plane tolerance of the fit.
//! 3. **Fold.** A face that is no longer flat is cut into triangles (ear cutting in the best-fit
//!    plane, holes bridged), so that every face stays planar. The new edges between the triangles
//!    have the attributes `soft: true` and `smooth: true`; an edge that was there already is used
//!    as it is. Every piece keeps the attributes and the unknown fields of the face (so its
//!    material). **Ids:** the pieces are ordered by area, the largest first (equal areas: the
//!    lower sorted vertex ids first); the first piece keeps the id of the face (the lowest id of
//!    all pieces), the others get new ids in that order, and new edges get ids in the order of the
//!    pieces. The same model and the same edit always give the same ids.
//! 4. **Merge.** A face that is now coplanar with a neighbour that it was not coplanar with before
//!    merges with it by the rules of Erase (the lower id stays; the look of the larger face wins,
//!    with a note for each dropped value: rule C). Neighbours that were coplanar before (the two
//!    halves of a split face) stay apart, and the pieces of a fold stay triangles.
//!
//! Refused, with the model unchanged: a loop of a face that would cross itself or touch itself
//! (the reason names the face), a face that would collapse to zero area without a weld, a map that
//! flattens the model, and a result with a new problem of the model (a face off its plane, two
//! edges that cross, a vertex too near an edge). Geometry that is moved through other faces is
//! accepted: an edge through a face in another plane is the warning `NotIntersected` of `check`,
//! as for drawing; Intersect Faces makes the edges there.
//!
//! The work uses box trees of the vertices and edges near the moved geometry and the faces that
//! use a moved vertex, so moving 1,000 vertices of a model of 80,000 edges takes a fraction of a
//! second.
//!
//! # `CopyGeometry`
//!
//! The chosen faces and edges (with the edges of the faces and whole curves, as for groups) are
//! drawn again, `count` times, placed by `matrix`, `matrix` twice, and so on, with the rules of
//! [`SurfaceModel::absorb`]: new ids, a copy that lands on a vertex or an edge of the model welds
//! and splits there, and the faces take their side, attributes and curves from the originals.
//! Copies that cross other faces stay unsplit (Intersect Faces). Copies are limited like the
//! edges of one edit ([`MAX_EDIT_EDGES`] over all copies).

use glam::{DAffine3, DMat3};

use super::super::arrange::{canonical, reversed};
use super::super::check::{self, Problem};
use super::super::geom::{newell, point_segment};
use super::super::part::Absorber;
use super::pushpull::{new_distance_problem, weld};
use super::*;

/// Smallest determinant (of the 3x3 part) of a map that is applied.
const MIN_DET: f64 = 1e-12;

fn bad(msg: impl Into<String>) -> SurfaceError {
    SurfaceError::Refused(msg.into())
}

/// The map of a matrix of 12 numbers, if it can be inverted.
fn affine(matrix: &[f64; 12]) -> Result<DAffine3, SurfaceError> {
    let xf = DAffine3::from_cols_array(matrix);
    if !xf.is_finite() {
        return Err(SurfaceError::BadPoint);
    }
    if xf.matrix3.determinant().abs() < MIN_DET {
        return Err(bad(
            "the transform flattens the geometry (its matrix cannot be inverted)",
        ));
    }
    Ok(xf)
}

fn is_identity(xf: &DAffine3) -> bool {
    xf.matrix3.abs_diff_eq(DMat3::IDENTITY, 1e-14) && xf.translation.abs().max_element() < 1e-14
}

/// The vertices that move: the given vertices, the vertices of the faces, the ends of the edges.
fn moved_set(
    m: &SurfaceModel,
    vertices: &[VertexId],
    faces: &[FaceId],
    edges: &[EdgeId],
) -> Result<BTreeSet<VertexId>, SurfaceError> {
    let mut s = BTreeSet::new();
    for v in vertices {
        if !m.vertices.contains_key(v) {
            return Err(SurfaceError::NoVertex(*v));
        }
        s.insert(*v);
    }
    for f in faces {
        let face = m.faces.get(f).ok_or(SurfaceError::NoFace(*f))?;
        s.extend(face.loops().flatten().copied());
    }
    for e in edges {
        let edge = m.edges.get(e).ok_or(SurfaceError::NoEdge(*e))?;
        s.extend(edge.v);
    }
    Ok(s)
}

fn pts(m: &SurfaceModel, l: &[VertexId]) -> Vec<DVec3> {
    l.iter().map(|v| m.vertices[v]).collect()
}

/// The plane through the loops of a face: the Newell normal of the outer loop (so the outer loop
/// runs counterclockwise around it) and the mean offset of all vertices. `None` when the outer
/// loop has no area.
fn fit(m: &SurfaceModel, f: &Face) -> Option<Plane> {
    let outer = pts(m, &f.outer);
    let n = newell(&outer).try_normalize()?;
    let all: Vec<DVec3> = f.loops().flatten().map(|v| m.vertices[v]).collect();
    let o = all[0];
    let mean = all.iter().map(|p| n.dot(*p - o)).sum::<f64>() / all.len() as f64;
    Some(Plane {
        normal: n,
        offset: n.dot(o) + mean,
    })
}

fn flat(m: &SurfaceModel, plane: &Plane, f: &Face) -> bool {
    f.loops().flatten().all(|v| on_plane(plane, m.vertices[v]))
}

/// Twice the area of the outer loop and its length round, for the test of a collapse.
fn collapsed(m: &SurfaceModel, f: &Face) -> bool {
    let p = pts(m, &f.outer);
    let round: f64 = (0..p.len())
        .map(|i| p[i].distance(p[(i + 1) % p.len()]))
        .sum();
    newell(&p).length() <= geom::tol(&p) * round
}

/// The smallest distance of two 2D segments (0 if they cross).
fn gap2(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> f64 {
    let cross = |o: DVec2, p: DVec2, q: DVec2| (p - o).perp_dot(q - o);
    let (d1, d2, d3, d4) = (
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    );
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return 0.0;
    }
    let ps = |p: DVec2, s: DVec2, e: DVec2| {
        let v = e - s;
        let l2 = v.length_squared();
        let t = if l2 > 0.0 {
            ((p - s).dot(v) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (s + v * t).distance(p)
    };
    ps(a, c, d)
        .min(ps(b, c, d))
        .min(ps(c, a, b))
        .min(ps(d, a, b))
}

/// True if two segments of the loops of a face cross, or touch without sharing a vertex. The
/// loops are looked at in the axes of `plane`.
fn crosses_itself(m: &SurfaceModel, f: &Face, plane: &Plane) -> bool {
    let mut segs: Vec<(VertexId, VertexId, DVec2, DVec2)> = Vec::new();
    for l in f.loops() {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            segs.push((
                a,
                b,
                plane.project(m.vertices[&a]),
                plane.project(m.vertices[&b]),
            ));
        }
    }
    let all: Vec<DVec3> = f.loops().flatten().map(|v| m.vertices[v]).collect();
    let tol = geom::tol(&all);
    let tree = BoxTree::new(
        segs.iter()
            .enumerate()
            .map(|(i, s)| {
                let (lo, hi) = (
                    s.2.min(s.3) - DVec2::splat(tol),
                    s.2.max(s.3) + DVec2::splat(tol),
                );
                (lo.extend(0.0), hi.extend(0.0), i as u32)
            })
            .collect(),
    );
    for (i, s) in segs.iter().enumerate() {
        let (lo, hi) = (
            (s.2.min(s.3) - DVec2::splat(tol)).extend(0.0),
            (s.2.max(s.3) + DVec2::splat(tol)).extend(0.0),
        );
        let mut hit = false;
        tree.query(lo, hi, &mut |j| {
            let j = j as usize;
            if j <= i || hit {
                return;
            }
            let t = &segs[j];
            if s.0 == t.0 || s.0 == t.1 || s.1 == t.0 || s.1 == t.1 {
                return;
            }
            if gap2(s.2, s.3, t.2, t.3) <= tol {
                hit = true;
            }
        });
        if hit {
            return true;
        }
    }
    false
}

/// The triangles of a face that is not flat, in the best-fit plane: vertex triples, each
/// counterclockwise around the normal of the fit.
fn fold(m: &SurfaceModel, f: &Face, id: FaceId) -> Result<Vec<[VertexId; 3]>, SurfaceError> {
    let cannot = || bad(format!("face {id} cannot be folded into triangles"));
    let plane = fit(m, f).ok_or_else(cannot)?;
    let ring = |l: &Vec<VertexId>, want_ccw: bool| -> (Vec<VertexId>, Vec<DVec2>) {
        let mut ids = l.clone();
        let mut p: Vec<DVec2> = ids.iter().map(|v| plane.project(m.vertices[v])).collect();
        if (geom::area2(&p) > 0.0) != want_ccw {
            ids.reverse();
            p.reverse();
        }
        (ids, p)
    };
    let mut ids: Vec<VertexId> = Vec::new();
    let mut rings: Vec<Vec<DVec2>> = Vec::new();
    let (oi, op) = ring(&f.outer, true);
    ids.extend(oi);
    rings.push(op);
    for h in &f.holes {
        let (hi, hp) = ring(h, false);
        ids.extend(hi);
        rings.push(hp);
    }
    let flat_pts: Vec<DVec2> = rings.iter().flatten().copied().collect();
    let tris = geom::triangulate_rings(&rings).ok_or_else(cannot)?;
    let mut tris: Vec<[usize; 3]> = tris.into_iter().map(|t| t.map(|i| i as usize)).collect();
    let all3: Vec<DVec3> = f.loops().flatten().map(|v| m.vertices[v]).collect();
    let tol = geom::tol(&all3);
    let area = |t: &[usize; 3]| {
        (flat_pts[t[1]] - flat_pts[t[0]]).perp_dot(flat_pts[t[2]] - flat_pts[t[0]])
    };
    // The pieces must make up the polygon.
    let want = geom::area2(&rings[0]) + rings[1..].iter().map(|r| geom::area2(r)).sum::<f64>();
    let got: f64 = tris.iter().map(|t| area(t).abs()).sum();
    if (want - got).abs() > 1e-6 * want.abs().max(1.0) {
        return Err(bad(format!(
            "a loop of face {id} would cross itself, so it cannot be folded"
        )));
    }
    // A piece with no area (three points on one line, a vertex in a straight side) is turned
    // away by flipping the diagonal it shares with the next piece.
    let thin = |t: &[usize; 3], flat_pts: &[DVec2]| {
        let longest = (0..3)
            .map(|k| flat_pts[t[k]].distance(flat_pts[t[(k + 1) % 3]]))
            .fold(0.0, f64::max);
        area(t).abs() <= tol * longest
    };
    for _ in 0..(8 * tris.len() + 8) {
        let Some(i) = (0..tris.len()).find(|&i| thin(&tris[i], &flat_pts)) else {
            break;
        };
        let t = tris[i];
        // The side of the thin piece that is the long diagonal: opposite the middle point.
        let long = (0..3)
            .max_by(|&a, &b| {
                let l = |k: usize| flat_pts[t[k]].distance(flat_pts[t[(k + 1) % 3]]);
                l(a).total_cmp(&l(b))
            })
            .expect("three sides");
        let (x, z, y) = (t[long], t[(long + 1) % 3], t[(long + 2) % 3]);
        let same = |u: usize, v: usize| ids[u] == ids[x] && ids[v] == ids[z];
        let other = (0..tris.len()).find(|&j| {
            j != i
                && (0..3).any(|k| {
                    let (a, b) = (tris[j][k], tris[j][(k + 1) % 3]);
                    same(a, b) || same(b, a)
                })
        });
        let Some(j) = other else {
            return Err(cannot());
        };
        let c = *tris[j]
            .iter()
            .find(|&&q| ids[q] != ids[x] && ids[q] != ids[z])
            .expect("a third vertex");
        let mut a = [x, y, c];
        let mut b = [y, z, c];
        for t in [&mut a, &mut b] {
            if area(t) < 0.0 {
                t.swap(1, 2);
            }
        }
        tris[i] = a;
        tris[j] = b;
    }
    if tris.iter().any(|t| thin(t, &flat_pts) || area(t) <= 0.0) {
        return Err(cannot());
    }
    Ok(tris.iter().map(|t| t.map(|i| ids[i])).collect())
}

/// What happens to the faces that use moved vertices.
struct Faces {
    /// All vertices of the face moved.
    full: BTreeSet<FaceId>,
    /// The plane of each face with a moved vertex, before the move.
    old: BTreeMap<FaceId, Plane>,
}

pub(super) fn transform_vertices(
    m: &mut SurfaceModel,
    vertices: &[VertexId],
    faces: &[FaceId],
    edges: &[EdgeId],
    matrix: &[f64; 12],
    before: &SurfaceModel,
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    let xf = affine(matrix)?;
    let s = moved_set(m, vertices, faces, edges)?;
    if s.is_empty() || is_identity(&xf) {
        return Ok(());
    }
    let mirror = xf.matrix3.determinant() < 0.0;
    let normal_map = xf.matrix3.inverse().transpose();
    // The new positions.
    let mut newp: Vec<(VertexId, DVec3)> = Vec::with_capacity(s.len());
    for v in &s {
        let q = xf.transform_point3(m.vertices[v]);
        if !q.is_finite() || geom::size(q) > MAX_COORD {
            return Err(SurfaceError::BadPoint);
        }
        newp.push((*v, q));
    }
    // The faces that use a moved vertex, and the faces that move whole.
    let mut st = Faces {
        full: BTreeSet::new(),
        old: BTreeMap::new(),
    };
    for (id, f) in &m.faces {
        let (mut moved, mut all) = (0usize, 0usize);
        for v in f.loops().flatten() {
            all += 1;
            moved += usize::from(s.contains(v));
        }
        if moved > 0 {
            st.old.insert(*id, f.plane);
            if moved == all {
                st.full.insert(*id);
            }
        }
    }
    for (v, q) in &newp {
        m.vertices.insert(*v, *q);
    }
    // Faces that moved whole: the plane is mapped, and a mirror turns the loops around.
    for id in &st.full {
        let mut f = m.faces[id].clone();
        let n = (normal_map * f.plane.normal).normalize();
        let all: Vec<DVec3> = f.loops().flatten().map(|v| m.vertices[v]).collect();
        let o = all[0];
        let mean = all.iter().map(|p| n.dot(*p - o)).sum::<f64>() / all.len() as f64;
        f.plane = Plane {
            normal: n,
            offset: n.dot(o) + mean,
        };
        if mirror {
            f.outer = reversed(&f.outer);
            let mut holes: Vec<Vec<VertexId>> = f.holes.iter().map(|h| reversed(h)).collect();
            holes.sort();
            f.holes = holes;
        }
        m.faces.insert(*id, f);
    }
    // The vertices that land on other vertices.
    let welds = weld_map(m, &s);
    if !welds.is_empty() {
        // A rough plane for the faces that are cut apart by the weld.
        let partial: Vec<FaceId> = st
            .old
            .keys()
            .copied()
            .filter(|id| !st.full.contains(id))
            .collect();
        for id in partial {
            let f = &m.faces[&id];
            if let Some(mut p) = fit(m, f) {
                if p.normal.dot(st.old[&id].normal) < 0.0 {
                    p = p.flipped();
                }
                m.faces.get_mut(&id).expect("listed").plane = p;
            }
        }
        weld(m, &welds, notes);
    }
    let alive = |v: &VertexId| m.vertices.contains_key(v);
    let survivors: BTreeSet<VertexId> = s
        .iter()
        .map(|v| *welds.get(v).unwrap_or(v))
        .filter(alive)
        .collect();
    // Vertices on edges, and edges over vertices.
    let splits = t_junctions(m, &survivors);
    let mut w = Work::new(m);
    let mut touched_pairs = BTreeSet::new();
    let mut made_edges = 0usize;
    for (e, vs) in splits {
        // From the first end of the edge on, nearest first, so each part is split again.
        let mut cur = e;
        for v in vs {
            w.split_edge_at(cur, v, &mut touched_pairs);
            made_edges += 1;
            // The part from the new vertex on is the new edge: the one with the highest id.
            cur = *w.m.edges.keys().next_back().expect("an edge");
        }
    }
    // The faces to look at: every face with a surviving moved vertex.
    let mut to_check: Vec<FaceId> =
        w.m.faces
            .iter()
            .filter(|(_, f)| f.loops().flatten().any(|v| survivors.contains(v)))
            .map(|(id, _)| *id)
            .collect();
    to_check.sort_unstable();
    let mut folded: BTreeSet<FaceId> = BTreeSet::new();
    let mut flat_changed: BTreeSet<FaceId> = BTreeSet::new();
    let mut fold_verts: BTreeSet<VertexId> = BTreeSet::new();
    for id in to_check {
        let face = w.m.faces[&id].clone();
        let is_full = st.full.contains(&id);
        // The plane the face had, if it has it still (a face that moved whole has the mapped one).
        let current = if is_full {
            face.plane
        } else {
            st.old.get(&id).copied().unwrap_or(face.plane)
        };
        let keep = Some(current).filter(|p| flat(w.m, p, &face));
        let plane = keep.or_else(|| fit(w.m, &face).filter(|p| flat(w.m, p, &face)));
        if !is_full {
            let axes = plane.or_else(|| fit(w.m, &face)).unwrap_or(face.plane);
            if crosses_itself(w.m, &face, &axes) {
                return Err(bad(format!(
                    "a loop of face {id} would cross itself or touch itself"
                )));
            }
        }
        if collapsed(w.m, &face) {
            return Err(bad(format!(
                "face {id} would collapse to zero area (all its vertices would lie on a line)"
            )));
        }
        match plane {
            Some(p) => {
                if p != face.plane {
                    w.m.faces.get_mut(&id).expect("exists").plane = p;
                }
                if !is_full {
                    flat_changed.insert(id);
                }
            }
            None => {
                let tris = fold(w.m, &face, id)?;
                let fp = fit(w.m, &face).expect("fold found a fit");
                fold_verts.extend(face.loops().flatten().copied());
                let mut pieces: Vec<(f64, [VertexId; 3], Plane)> = Vec::new();
                for t in tris {
                    let (a, b, c) = (w.pos(t[0]), w.pos(t[1]), w.pos(t[2]));
                    let n = (b - a).cross(c - a);
                    let Some(mut pl) = Plane::new((a + b + c) / 3.0, n) else {
                        return Err(bad(format!("face {id} cannot be folded into triangles")));
                    };
                    if pl.normal.dot(fp.normal) < 0.0 {
                        pl = pl.flipped();
                    }
                    pieces.push((n.length() / 2.0, canonical_tri(t), pl));
                }
                // The largest first; equal areas by vertex ids.
                pieces.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
                // Edges between the pieces.
                for (_, t, _) in &pieces {
                    for k in 0..3 {
                        let (a, b) = (t[k], t[(k + 1) % 3]);
                        if w.t.edge(a, b).is_none() {
                            let e = w.new_edge(a, b);
                            made_edges += 1;
                            let at = w.m.edge_attrs.entry(e).or_default();
                            at.insert("soft".into(), true.into());
                            at.insert("smooth".into(), true.into());
                        }
                    }
                }
                for (k, (_, t, pl)) in pieces.into_iter().enumerate() {
                    let mut piece = Face::new(pl, t.to_vec(), Vec::new());
                    piece.attrs = face.attrs.clone();
                    piece.extra = face.extra.clone();
                    if k == 0 {
                        w.put_face(id, piece);
                        folded.insert(id);
                    } else {
                        folded.insert(w.new_face(piece));
                    }
                }
            }
        }
        if made_edges > MAX_EDIT_EDGES {
            return Err(SurfaceError::Limit {
                what: "edges in one edit",
                max: MAX_EDIT_EDGES,
                count: made_edges,
            });
        }
    }
    // Faces that are now coplanar with a neighbour that they were not coplanar with.
    let mut merge: BTreeSet<EdgeId> = BTreeSet::new();
    for &id in &flat_changed {
        let Some(f) = w.m.faces.get(&id) else {
            continue;
        };
        for (a, b) in f.uses() {
            let Some(e) = w.t.edge(a, b) else { continue };
            let fs: Vec<FaceId> = w.t.faces_of(e).collect();
            let [x, y] = fs[..] else { continue };
            let g = if x == id { y } else { x };
            if folded.contains(&g) || merge.contains(&e) {
                continue;
            }
            let gf = &w.m.faces[&g];
            let f = &w.m.faces[&id];
            let opposite = gf.uses().any(|u| u == (b, a));
            if opposite
                && f.plane.normal.dot(gf.plane.normal) > 0.0
                && w.face_on(&f.plane, gf)
                && !coplanar_before(before, id, g)
            {
                merge.insert(e);
            }
        }
    }
    drop(w);
    if !merge.is_empty() {
        let list: Vec<EdgeId> = merge.into_iter().collect();
        super::erase_edges(m, &list, notes)?;
    }
    finish(before, m, &survivors, &fold_verts)
}

/// A triangle's vertices as a loop that starts at its smallest id.
fn canonical_tri(t: [VertexId; 3]) -> [VertexId; 3] {
    let c = canonical(&t);
    [c[0], c[1], c[2]]
}

/// True if both faces were in the model before, shared an edge and were in one plane looking the
/// same way.
fn coplanar_before(before: &SurfaceModel, a: FaceId, b: FaceId) -> bool {
    let (Some(fa), Some(fb)) = (before.faces.get(&a), before.faces.get(&b)) else {
        return false;
    };
    fa.plane.normal.dot(fb.plane.normal) > 0.0
        && fb
            .loops()
            .flatten()
            .all(|v| on_plane(&fa.plane, before.vertices[v]))
        && fa.uses().any(|(p, q)| fb.uses().any(|u| u == (q, p)))
}

/// The checks of the result: the faces and the limits, then new structural problems, then new
/// distance problems around what moved.
fn finish(
    before: &SurfaceModel,
    m: &mut SurfaceModel,
    survivors: &BTreeSet<VertexId>,
    folded: &BTreeSet<VertexId>,
) -> Result<(), SurfaceError> {
    check_limits(m)?;
    let invalid = |p: &Problem| bad(format!("the move would make the model invalid: {p}"));
    let after = check::structure_problems(m);
    if !after.is_empty() {
        let old = check::structure_problems(before);
        if let Some(p) = after.difference(&old).next() {
            return Err(invalid(p));
        }
    }
    let verts: BTreeSet<VertexId> = survivors
        .iter()
        .chain(folded.iter())
        .copied()
        .filter(|v| m.vertices.contains_key(v))
        .collect();
    let edges: BTreeSet<EdgeId> = m
        .edges
        .iter()
        .filter(|(_, e)| e.v.iter().any(|v| verts.contains(v)))
        .map(|(id, _)| *id)
        .collect();
    if let Some(p) = new_distance_problem(before, m, &edges, &verts) {
        return Err(invalid(&p));
    }
    if let Some(p) = check::curves(m) {
        return Err(invalid(&p));
    }
    Ok(())
}

/// For each moved vertex, the vertex it lands on: the lower of the vertices within the tolerance,
/// preferring one that did not move. Maps the vertex that goes to the vertex that stays.
fn weld_map(m: &SurfaceModel, s: &BTreeSet<VertexId>) -> BTreeMap<VertexId, VertexId> {
    let pad = DVec3::splat(check::window(m));
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for v in s {
        let p = m.vertices[v];
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let (lo, hi) = (lo - pad, hi + pad);
    let tree = BoxTree::new(
        m.vertices
            .iter()
            .filter(|(_, p)| !(p.cmplt(lo).any() || p.cmpgt(hi).any()))
            .map(|(v, p)| (*p, *p, v.0))
            .collect(),
    );
    let mut parent: BTreeMap<VertexId, VertexId> = BTreeMap::new();
    fn root(parent: &mut BTreeMap<VertexId, VertexId>, v: VertexId) -> VertexId {
        let p = *parent.entry(v).or_insert(v);
        if p == v {
            return v;
        }
        let r = root(parent, p);
        parent.insert(v, r);
        r
    }
    for &v in s {
        let p = m.vertices[&v];
        let mut near = Vec::new();
        tree.query(p - pad, p + pad, &mut |c| near.push(VertexId(c)));
        for c in near {
            if c == v {
                continue;
            }
            let q = m.vertices[&c];
            if p.distance(q) <= geom::tol(&[p, q]) {
                let (a, b) = (root(&mut parent, v), root(&mut parent, c));
                if a != b {
                    let (x, y) = if a < b { (a, b) } else { (b, a) };
                    parent.insert(y, x);
                }
            }
        }
    }
    let mut clusters: BTreeMap<VertexId, Vec<VertexId>> = BTreeMap::new();
    let keys: Vec<VertexId> = parent.keys().copied().collect();
    for v in keys {
        let r = root(&mut parent, v);
        clusters.entry(r).or_default().push(v);
    }
    let mut out = BTreeMap::new();
    for (_, members) in clusters {
        if members.len() < 2 {
            continue;
        }
        // The vertex that stays: the lowest that did not move, else the lowest.
        let keep = members
            .iter()
            .copied()
            .find(|v| !s.contains(v))
            .unwrap_or(members[0]);
        for v in members {
            if v != keep {
                out.insert(v, keep);
            }
        }
    }
    out
}

/// The vertices that lie on the inside of an edge they do not belong to, because they moved onto
/// it or the edge moved over them: for each edge, the vertices in order from its first end.
fn t_junctions(
    m: &SurfaceModel,
    survivors: &BTreeSet<VertexId>,
) -> BTreeMap<EdgeId, Vec<VertexId>> {
    let mut out: BTreeMap<EdgeId, Vec<(f64, VertexId)>> = BTreeMap::new();
    if survivors.is_empty() {
        return BTreeMap::new();
    }
    let pad = DVec3::splat(check::window(m));
    let pos = |v: VertexId| m.vertices[&v];
    // The box around the moved vertices and the edges that end at them.
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    let mut moved_edges: Vec<EdgeId> = Vec::new();
    for (id, e) in &m.edges {
        if e.v.iter().any(|v| survivors.contains(v)) {
            moved_edges.push(*id);
            for v in e.v {
                lo = lo.min(pos(v));
                hi = hi.max(pos(v));
            }
        }
    }
    let (lo, hi) = (lo - pad, hi + pad);
    let etree = BoxTree::new(
        m.edges
            .iter()
            .filter_map(|(id, e)| {
                let (a, b) = (pos(e.v[0]), pos(e.v[1]));
                let (elo, ehi) = (a.min(b), a.max(b));
                (!(ehi.cmplt(lo).any() || elo.cmpgt(hi).any())).then_some((
                    elo - pad,
                    ehi + pad,
                    id.0,
                ))
            })
            .collect(),
    );
    let vtree = BoxTree::new(
        m.vertices
            .iter()
            .filter(|(_, p)| !(p.cmplt(lo).any() || p.cmpgt(hi).any()))
            .map(|(v, p)| (*p, *p, v.0))
            .collect(),
    );
    let test = |v: VertexId, e: EdgeId, out: &mut BTreeMap<EdgeId, Vec<(f64, VertexId)>>| {
        let edge = m.edges[&e];
        if edge.v.contains(&v) {
            return;
        }
        let (p, a, b) = (pos(v), pos(edge.v[0]), pos(edge.v[1]));
        let (d, t) = point_segment(p, a, b);
        let tol = geom::tol(&[p, a, b]);
        let len = a.distance(b);
        if d <= tol && t * len > tol && (1.0 - t) * len > tol {
            let list = out.entry(e).or_default();
            if !list.iter().any(|(_, x)| *x == v) {
                list.push((t, v));
            }
        }
    };
    for &v in survivors {
        let p = pos(v);
        let mut es = Vec::new();
        etree.query(p, p, &mut |e| es.push(EdgeId(e)));
        for e in es {
            test(v, e, &mut out);
        }
    }
    for e in moved_edges {
        let edge = m.edges[&e];
        let (a, b) = (pos(edge.v[0]), pos(edge.v[1]));
        let mut vs = Vec::new();
        vtree.query(a.min(b), a.max(b), &mut |v| vs.push(VertexId(v)));
        for v in vs {
            test(v, e, &mut out);
        }
    }
    out.into_iter()
        .map(|(e, mut vs)| {
            vs.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            (e, vs.into_iter().map(|(_, v)| v).collect())
        })
        .collect()
}

pub(super) fn copy_geometry(
    m: &mut SurfaceModel,
    faces: &[FaceId],
    edges: &[EdgeId],
    matrix: &[f64; 12],
    count: u32,
) -> Result<(), SurfaceError> {
    let xf = affine(matrix)?;
    if count == 0 {
        return Err(bad("a copy needs a count of at least 1"));
    }
    let sel = m.select(faces, edges)?;
    if sel.is_empty() {
        return Err(bad("nothing is chosen to copy"));
    }
    let part = m.extract(&sel);
    let total = part.edge_count().saturating_mul(count as usize);
    if total > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit",
            max: MAX_EDIT_EDGES,
            count: total,
        });
    }
    if is_identity(&xf) {
        return Err(bad("the copy would lie on the original (the move is zero)"));
    }
    let mut a = Absorber::new(m.clone());
    let mut at = DAffine3::IDENTITY;
    for _ in 0..count {
        at = xf * at;
        a.absorb(&part, Some(at))?;
    }
    let work = a.finish();
    check_limits(&work)?;
    *m = work;
    Ok(())
}
