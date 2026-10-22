//! Move, Rotate and Scale of geometry, and copies of it (MOD-05, MOD-08).
//!
//! # `TransformVertices`
//!
//! The moved set is the given vertices, all vertices of the given faces and the ends of the given
//! edges. Each moves by the affine map `matrix` (a move, a rotation, a scale, or any mix; a map
//! that flattens is refused: the ratio of its smallest to its largest singular value is below
//! the relative tolerance, see [`MIN_SINGULAR_RATIO`]). Then, in this order:
//!
//! 1. **Snap.** Where the moved geometry lands within the length tolerance of geometry that did
//!    not move (a moved vertex on a vertex or on the inside of an edge, a moved edge over a
//!    vertex), and all these landings agree on one offset within the plane tolerance, the moved
//!    vertices are shifted by that offset, so they land exactly. A vertex that lands on the inside
//!    of an edge that did not move is then put onto the edge. So a landing within the tolerance
//!    never tilts a face out of its plane.
//! 2. **Weld.** A moved vertex that lands within the length tolerance of another vertex becomes
//!    that vertex (the vertex that was there stays; of two that moved, the lower id; the moved
//!    ones go). Edges that fall together become one, loops and faces that lose their area go
//!    (the weld of Push/Pull, with its notes). A vertex that lands on the inside of an edge, and
//!    an edge that is moved over a vertex, split there (a welded T-junction), so the model has no
//!    vertex on an edge; parts of edges that then join the same two vertices (an edge slid along
//!    another) become one edge (the lower id stays and takes the attributes of the other, with a
//!    note for a value that differs). A vertex that would land on an edge of its own face is
//!    refused (the face would touch itself).
//! 3. **Planes.** Every face that has a moved vertex is looked at again. A face whose vertices
//!    were all moved is mapped exactly (an affine map keeps planes: the new normal is the old one
//!    times the inverse transpose of the matrix, and the loops turn around when the map is a
//!    mirror), so it never folds. Any other face keeps its plane if all its vertices still lie on
//!    it; else it is refitted (Newell normal, mean offset) and is flat if all its vertices lie
//!    within the plane tolerance of the fit.
//! 4. **Fold.** A face that is no longer flat is cut into triangles (see the module `fold`: the
//!    cut depends on the geometry only; a quad along the diagonal that bends the least, any other
//!    face Delaunay-like in its best-fit plane), so that every face stays planar. The new edges
//!    between the triangles have the attributes `soft: true` and `smooth: true`; an edge that was
//!    there already is used as it is. Every piece keeps the attributes and the unknown fields of
//!    the face (so its material). **Ids:** the pieces are ordered by area, the largest first
//!    (equal areas: the lower sorted vertex ids first); the first piece keeps the id of the face
//!    (the lowest id of all pieces), the others get new ids in that order, and new edges get ids
//!    in the order of the pieces. The same model and the same edit always give the same ids.
//! 5. **Merge.** A face that is now coplanar with a neighbour that it was not coplanar with before
//!    merges with it by the rules of Erase (the lower id stays; the look of the larger face wins,
//!    with a note for each dropped value: rule C). Neighbours that were coplanar before (the two
//!    halves of a split face) stay apart, and the pieces of a fold stay triangles.
//!
//! Refused, with the model unchanged: a loop of a face that would cross itself or touch itself
//! (the reason names the face), a face that would collapse to zero area without a weld, a map that
//! flattens the model, faces that would overlap in one plane (an edge of a face inside a coplanar
//! face that looks the same way, or one face over another: the coplanar overlaps of `check`, which
//! Push/Pull refuses too), and a result with a new problem of the model (a face off its plane, two
//! edges that cross, a vertex too near an edge, a hole in another hole). Geometry that is moved
//! through faces in other planes is accepted: an edge through such a face is the warning
//! `NotIntersected` of `check`, as for drawing; Intersect Faces makes the edges there.
//!
//! The work looks only at the geometry near what moved: box trees of the vertices and edges in
//! that region, a topology index of the faces that use a moved vertex and their neighbours, and
//! checks of what changed (the rest of the model is as it was). Moving one vertex of a model of
//! 80,000 edges takes a few milliseconds plus the linear scans; see `docs/modeler.md`.
//!
//! # `CopyGeometry`
//!
//! The chosen faces and edges (with the edges of the faces and whole curves, as for groups) are
//! drawn again, `count` times, placed by `matrix`, `matrix` twice, and so on, with the rules of
//! [`SurfaceModel::absorb`]: new ids, a copy that lands on a vertex or an edge of the model welds
//! and splits there, and the faces take their side, attributes and curves from the originals.
//! Copies that cross other faces stay unsplit (Intersect Faces). Copies are limited like the
//! edges of one edit ([`MAX_EDIT_EDGES`] over all copies). A copy that would move no vertex by
//! the length tolerance (it would lie on the original) is refused.

use glam::{DAffine3, DMat3};

use super::super::RELATIVE_TOLERANCE;
use super::super::arrange::{canonical, reversed};
use super::super::check::{self, Problem};
use super::super::clash;
use super::super::geom::{newell, point_segment};
use super::super::part::Absorber;
use super::pushpull::{new_distance_problem, weld};
use super::*;

mod fold;

/// The smallest ratio of the smallest to the largest singular value of the 3x3 part of a map
/// that is applied. It is the relative tolerance of the model (1e-9): a map below it squashes
/// some direction to less than the relative tolerance of the sizes in the others, so a face of
/// any size turned across that direction ends thinner than its own length tolerance (flat by the
/// rules of the model), and the map of the normals (the inverse transpose) loses more than nine
/// of the sixteen digits of a number. The ratio does not depend on the size: a uniform scale of
/// 1e-5 (a site model brought to a sketch) is a ratio of 1 and is applied; a scale of (1, 1,
/// 1e-10) is refused. (A bound on the determinant refused the first, a determinant of 1e-15, and
/// accepted a scale of (1e4, 1e4, 1e-7), a determinant of 10 that squashes one direction 1e11
/// times more than the others.)
const MIN_SINGULAR_RATIO: f64 = RELATIVE_TOLERANCE;

fn bad(msg: impl Into<String>) -> SurfaceError {
    SurfaceError::Refused(msg.into())
}

/// The map of a matrix of 12 numbers, if it does not flatten (see [`MIN_SINGULAR_RATIO`]).
fn affine(matrix: &[f64; 12]) -> Result<DAffine3, SurfaceError> {
    let xf = DAffine3::from_cols_array(matrix);
    if !xf.is_finite() {
        return Err(SurfaceError::BadPoint);
    }
    if singular_ratio(xf.matrix3) < MIN_SINGULAR_RATIO {
        return Err(bad(
            "the transform flattens the geometry (its matrix cannot be inverted)",
        ));
    }
    Ok(xf)
}

/// The ratio of the smallest to the largest singular value of `a` (0 when it cannot be
/// inverted): `1 / (|a| |a^-1|)` with the spectral norms. Each norm is the largest singular value,
/// which the largest eigenvalue of `aᵀa` gives accurately (a small singular value from `aᵀa`
/// would lose half the digits).
fn singular_ratio(a: DMat3) -> f64 {
    let det = a.determinant();
    if !det.is_finite() || det == 0.0 {
        return 0.0;
    }
    let inv = a.inverse();
    if !inv.is_finite() {
        return 0.0;
    }
    let r = 1.0 / (largest_singular(a) * largest_singular(inv));
    if r.is_finite() { r } else { 0.0 }
}

/// The largest singular value of `a`: the square root of the largest eigenvalue of the symmetric
/// `aᵀa`, by the closed form for symmetric 3x3 matrices (Smith, 1961).
fn largest_singular(a: DMat3) -> f64 {
    let s = a.transpose() * a;
    let (m00, m11, m22) = (s.x_axis.x, s.y_axis.y, s.z_axis.z);
    let (m01, m02, m12) = (s.y_axis.x, s.z_axis.x, s.z_axis.y);
    let p1 = m01 * m01 + m02 * m02 + m12 * m12;
    let q = (m00 + m11 + m22) / 3.0;
    let p2 = (m00 - q).powi(2) + (m11 - q).powi(2) + (m22 - q).powi(2) + 2.0 * p1;
    let top = if p2 <= f64::MIN_POSITIVE {
        m00.max(m11).max(m22)
    } else {
        let p = (p2 / 6.0).sqrt();
        let b = (s - DMat3::from_diagonal(DVec3::splat(q))) * (1.0 / p);
        let r = (b.determinant() / 2.0).clamp(-1.0, 1.0);
        q + 2.0 * p * (r.acos() / 3.0).cos()
    };
    top.max(0.0).sqrt()
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
    // Landings within the tolerance: the moved geometry is put exactly onto what it lands on.
    if let Some(off) = landing_offset(m, &s) {
        for v in &s {
            *m.vertices.get_mut(v).expect("moved") += off;
        }
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
    // A moved vertex on an edge that did not move goes exactly onto the edge.
    for (e, vs) in &splits {
        let ends = m.edges[e].v;
        if ends.iter().any(|v| s.contains(v)) {
            continue;
        }
        let (a, b) = (m.vertices[&ends[0]], m.vertices[&ends[1]]);
        for v in vs {
            if s.contains(v) {
                let (_, t) = point_segment(m.vertices[v], a, b);
                m.vertices.insert(*v, a + (b - a) * t);
            }
        }
    }
    let seeds: Vec<VertexId> = survivors
        .iter()
        .copied()
        .chain(splits.keys().flat_map(|e| m.edges[e].v))
        .collect();
    let mut w = local_work(m, seeds.into_iter());
    for (e, vs) in &splits {
        for v in vs {
            if let Some(f) =
                w.t.faces_of(*e)
                    .find(|f| w.m.faces[f].loops().flatten().any(|x| x == v))
            {
                if collapsed(w.m, &w.m.faces[&f]) {
                    return Err(bad(format!(
                        "face {f} would collapse to zero area (all its vertices would lie on a \
                         line)"
                    )));
                }
                return Err(bad(format!(
                    "vertex {v} would land on edge {e} of its own face {f}, so the face would \
                     touch itself"
                )));
            }
        }
    }
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
    join_double_edges(&mut w, &touched_pairs, notes);
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
                let tris = fold::fold(w.m, &face, id)?;
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
    if !merge.is_empty() {
        let list: Vec<EdgeId> = merge.into_iter().collect();
        if reference::mode() == reference::Mode::Index {
            w.near = Some(Near::of(w.m));
        }
        super::erase_edges_in(&mut w, &list, notes)?;
    }
    drop(w);
    finish(before, m, &survivors, &fold_verts)
}

/// A work on `m` whose topology index holds only the part of the model around `seeds` (the
/// vertices that moved and the ends of the edges they split): every edge at a vertex of a face
/// with a seed vertex or at a neighbour of such a vertex, and every face that uses one of these
/// edges. That is all the edit asks the index: the edges and faces of the faces that it looks at
/// (splits, folds, merges and the heal of an erase), so the answers are the same as those of an
/// index of the whole model, which would cost a pass that builds maps of all edges and faces.
fn local_work<'a>(m: &'a mut SurfaceModel, seeds: impl Iterator<Item = VertexId>) -> Work<'a> {
    let n = m.next_id as usize + 1;
    let mut core = vec![false; n];
    let mark = |mask: &mut Vec<bool>, v: VertexId| {
        if let Some(x) = mask.get_mut(v.0 as usize) {
            *x = true;
        }
    };
    let has = |mask: &[bool], v: &VertexId| mask.get(v.0 as usize).copied().unwrap_or(false);
    for v in seeds {
        mark(&mut core, v);
    }
    let mut near = core.clone();
    for f in m.faces.values() {
        if f.loops().flatten().any(|v| has(&core, v)) {
            for v in f.loops().flatten() {
                mark(&mut near, *v);
            }
        }
    }
    let mut wide = near.clone();
    for e in m.edges.values() {
        if e.v.iter().any(|v| has(&near, v)) {
            for v in e.v {
                mark(&mut wide, v);
            }
        }
    }
    let mut t = Topo::default();
    for (id, e) in &m.edges {
        if e.v.iter().any(|v| has(&wide, v)) {
            t.add_edge(*id, *e);
        }
    }
    for (id, f) in &m.faces {
        if f.loops().flatten().any(|v| has(&wide, v)) {
            t.add_face(*id, f);
        }
    }
    Work { m, t, near: None }
}

/// Makes one edge of each set of edges that join the same two vertices among `pairs` (the parts
/// of an edge slid along another, split at each other's ends): the lowest id stays and takes the
/// attributes of the others (a value that differs is dropped with a note) and their place in a
/// curve when it is in none (else the others leave their curves), as in the weld.
fn join_double_edges(
    w: &mut Work<'_>,
    pairs: &BTreeSet<(VertexId, VertexId)>,
    notes: &mut Vec<String>,
) {
    let mut recheck: BTreeSet<super::super::CurveId> = BTreeSet::new();
    for &(a, b) in pairs {
        let k = key(a, b);
        let ids: Vec<EdgeId> =
            w.t.edges_at(a)
                .filter(|e| w.m.edges.get(e).is_some_and(|x| x.key() == k))
                .collect();
        if ids.len() < 2 {
            continue;
        }
        let keep = ids[0];
        let mut faces: Vec<FaceId> = ids.iter().flat_map(|e| w.t.faces_of(*e)).collect();
        faces.sort_unstable();
        faces.dedup();
        let faces: Vec<(FaceId, Face)> = faces
            .into_iter()
            .map(|f| (f, w.m.faces[&f].clone()))
            .collect();
        for (f, face) in &faces {
            w.t.remove_face(*f, face);
        }
        for &gone in &ids[1..] {
            let e = w.m.edges[&gone];
            w.t.remove_edge(gone, e);
            w.m.edges.remove(&gone);
            if let Some(attrs) = w.m.edge_attrs.remove(&gone) {
                let into = w.m.edge_attrs.entry(keep).or_default();
                for (name, v) in attrs {
                    match into.get(&name) {
                        Some(own) if *own != v => notes.push(format!(
                            "edge {gone} fell onto edge {keep}: its attribute {name} = {v} was \
                             dropped (edge {keep} keeps {own})"
                        )),
                        Some(_) => {}
                        None => {
                            into.insert(name, v);
                        }
                    }
                }
            }
            let kept_curved = w.m.curves.values().any(|es| es.contains(&keep));
            for (c, es) in &mut w.m.curves {
                if let Some(i) = es.iter().position(|e| *e == gone) {
                    if kept_curved {
                        es.remove(i);
                    } else {
                        es[i] = keep;
                    }
                    recheck.insert(*c);
                }
            }
            w.m.curves.retain(|_, es| !es.is_empty());
        }
        // The kept edge again, with no stale uses, then the faces.
        let ke = w.m.edges[&keep];
        w.t.remove_edge(keep, ke);
        w.t.add_edge(keep, ke);
        for (f, face) in &faces {
            w.t.add_face(*f, face);
        }
    }
    super::rechain(w.m, &recheck);
}

/// The offset that puts the moved vertices `s` exactly onto the geometry that did not move and
/// that they land on within the length tolerance: a moved vertex near a vertex (the nearest one)
/// or else near the inside of an edge with no moved end, and an edge with both ends moved near a
/// vertex that did not move. `None` when nothing lands, or when the landings do not agree on one
/// offset within the plane tolerance (then each one welds or splits as it is).
fn landing_offset(m: &SurfaceModel, s: &BTreeSet<VertexId>) -> Option<DVec3> {
    let pad = DVec3::splat(check::window(m));
    let (lo, hi) = geom::bounds(s.iter().map(|v| m.vertices[v]))?;
    let (lo, hi) = (lo - pad, hi + pad);
    let outside = |a: DVec3, b: DVec3| b.cmplt(lo).any() || a.cmpgt(hi).any();
    let vtree = BoxTree::new(
        m.vertices
            .iter()
            .filter(|(v, p)| !s.contains(v) && !outside(**p, **p))
            .map(|(v, p)| (*p, *p, v.0))
            .collect(),
    );
    let mut moved_edges: Vec<(DVec3, DVec3)> = Vec::new();
    let etree = BoxTree::new(
        m.edges
            .iter()
            .filter_map(|(id, e)| {
                let moved = e.v.map(|v| s.contains(&v));
                let (a, b) = (m.vertices[&e.v[0]], m.vertices[&e.v[1]]);
                if moved[0] && moved[1] {
                    moved_edges.push((a, b));
                }
                (!moved[0] && !moved[1] && !outside(a.min(b), a.max(b))).then_some((
                    a.min(b) - pad,
                    a.max(b) + pad,
                    id.0,
                ))
            })
            .collect(),
    );
    let interior = |p: DVec3, a: DVec3, b: DVec3| {
        let (d, t) = point_segment(p, a, b);
        let tol = geom::tol(&[p, a, b]);
        let len = a.distance(b);
        (d <= tol && t * len > tol && (1.0 - t) * len > tol).then_some((d, a + (b - a) * t))
    };
    let mut offs: Vec<DVec3> = Vec::new();
    for v in s {
        let p = m.vertices[v];
        let mut best: Option<(f64, u32, DVec3)> = None;
        vtree.query(p - pad, p + pad, &mut |c| {
            let q = m.vertices[&VertexId(c)];
            let d = p.distance(q);
            if d <= geom::tol(&[p, q]) && best.is_none_or(|b| (d, c) < (b.0, b.1)) {
                best = Some((d, c, q));
            }
        });
        if best.is_none() {
            etree.query(p, p, &mut |e| {
                let ed = m.edges[&EdgeId(e)];
                let (a, b) = (m.vertices[&ed.v[0]], m.vertices[&ed.v[1]]);
                if let Some((d, x)) = interior(p, a, b)
                    && best.is_none_or(|b| (d, e) < (b.0, b.1))
                {
                    best = Some((d, e, x));
                }
            });
        }
        if let Some((_, _, q)) = best {
            offs.push(q - p);
        }
    }
    for (a, b) in moved_edges {
        vtree.query(a.min(b) - pad, a.max(b) + pad, &mut |c| {
            let q = m.vertices[&VertexId(c)];
            if let Some((_, x)) = interior(q, a, b) {
                offs.push(q - x);
            }
        });
    }
    let first = *offs.first()?;
    let agree = geom::plane_tol(&[lo, hi]);
    if offs.iter().any(|o| o.distance(first) > agree) {
        return None;
    }
    let mean = offs.iter().sum::<DVec3>() / offs.len() as f64;
    (mean != DVec3::ZERO).then_some(mean)
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
    if fa.plane.normal.dot(fb.plane.normal) <= 0.0
        || !fb
            .loops()
            .flatten()
            .all(|v| on_plane(&fa.plane, before.vertices[v]))
    {
        return false;
    }
    let uses: BTreeSet<(VertexId, VertexId)> = fb.uses().collect();
    fa.uses().any(|(p, q)| uses.contains(&(q, p)))
}

/// The keys of the entries that differ between two maps (in one only, or with other values).
fn changed<K: Ord + Copy, V: PartialEq>(a: &BTreeMap<K, V>, b: &BTreeMap<K, V>) -> Vec<K> {
    let mut out = Vec::new();
    let (mut x, mut y) = (a.iter().peekable(), b.iter().peekable());
    loop {
        match (x.peek(), y.peek()) {
            (None, None) => break,
            (Some((k, _)), None) => {
                out.push(**k);
                x.next();
            }
            (None, Some((k, _))) => {
                out.push(**k);
                y.next();
            }
            (Some((ka, va)), Some((kb, vb))) => match ka.cmp(kb) {
                std::cmp::Ordering::Less => {
                    out.push(**ka);
                    x.next();
                }
                std::cmp::Ordering::Greater => {
                    out.push(**kb);
                    y.next();
                }
                std::cmp::Ordering::Equal => {
                    if va != vb {
                        out.push(**ka);
                    }
                    x.next();
                    y.next();
                }
            },
        }
    }
    out
}

/// The checks of the result: the faces and the limits, then new structural problems, then new
/// distance problems around what moved, then new coplanar overlaps. Only what changed is looked
/// at (the rest of the model is as it was); when the quick look finds a structural problem, the
/// full checks of both models decide, so a problem that the model had before stays allowed.
fn finish(
    before: &SurfaceModel,
    m: &mut SurfaceModel,
    survivors: &BTreeSet<VertexId>,
    folded: &BTreeSet<VertexId>,
) -> Result<(), SurfaceError> {
    check_limits(m)?;
    let invalid = |p: &Problem| bad(format!("the move would make the model invalid: {p}"));
    let cv = changed(&before.vertices, &m.vertices);
    let ce = changed(&before.edges, &m.edges);
    let cf = changed(&before.faces, &m.faces);
    let mut seeds: BTreeSet<VertexId> = cv.iter().copied().collect();
    let mut ids: BTreeSet<u32> = BTreeSet::new();
    for e in &ce {
        for x in [before, &*m] {
            if let Some(ed) = x.edges.get(e) {
                seeds.extend(ed.v);
            }
        }
        if m.edges.contains_key(e) {
            ids.insert(e.0);
        }
    }
    for f in &cf {
        for x in [before, &*m] {
            if let Some(face) = x.faces.get(f) {
                seeds.extend(face.loops().flatten());
            }
        }
        if m.faces.contains_key(f) {
            ids.insert(f.0);
        }
    }
    ids.extend(
        cv.iter()
            .filter(|v| m.vertices.contains_key(v))
            .map(|v| v.0),
    );
    if check::structure_near(m, &seeds, &ids) {
        let after = check::structure_problems(m);
        if !after.is_empty() {
            let old = check::structure_problems(before);
            if let Some(p) = after.difference(&old).next() {
                return Err(invalid(p));
            }
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
    new_overlap(before, m, &cv, &ce, &cf)?;
    if let Some(p) = check::curves(m) {
        return Err(invalid(&p));
    }
    Ok(())
}

/// Refuses a coplanar overlap that the move makes (one that the model before did not have between
/// the same faces, or the same edge and face): an edge of a face that would lie in a face of the
/// same plane looking the same way, or two such faces that would share an area. The items looked
/// at are the changed edges and faces and those at a changed vertex.
fn new_overlap(
    before: &SurfaceModel,
    m: &SurfaceModel,
    cv: &[VertexId],
    ce: &[EdgeId],
    cf: &[FaceId],
) -> Result<(), SurfaceError> {
    let moved: BTreeSet<VertexId> = cv
        .iter()
        .copied()
        .filter(|v| m.vertices.contains_key(v))
        .collect();
    let items = |x: &SurfaceModel| {
        let mut edges: BTreeSet<EdgeId> = ce
            .iter()
            .copied()
            .filter(|e| x.edges.contains_key(e))
            .collect();
        edges.extend(
            x.edges
                .iter()
                .filter(|(_, e)| e.v.iter().any(|v| moved.contains(v)))
                .map(|(id, _)| *id),
        );
        let mut faces: BTreeSet<FaceId> = cf
            .iter()
            .copied()
            .filter(|f| x.faces.contains_key(f))
            .collect();
        faces.extend(
            x.faces
                .iter()
                .filter(|(_, f)| f.loops().flatten().any(|v| moved.contains(v)))
                .map(|(id, _)| *id),
        );
        (edges, faces)
    };
    let window = check::window(m).max(check::window(before));
    let (edges, faces) = items(m);
    let (lies, over) = clash::coplanar_near(m, &edges, &faces, window);
    if lies.is_empty() && over.is_empty() {
        return Ok(());
    }
    let (be, bf) = items(before);
    let (old_lies, old_over) = clash::coplanar_near(before, &be, &bf, window);
    if let Some((e, g)) = lies.iter().find(|x| !old_lies.contains(x)) {
        return Err(bad(format!(
            "the move would make faces overlap: edge {e} would lie inside face {g}, which is in \
             the same plane and looks the same way"
        )));
    }
    if let Some((f, g)) = over.iter().find(|x| !old_over.contains(x)) {
        return Err(bad(format!(
            "the move would make faces {f} and {g} overlap: they would lie in one plane, look the \
             same way and share an area"
        )));
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
    // A copy whose every vertex stays within the length tolerance of where it was would weld
    // onto the original and change nothing.
    let lands_on_itself = part.vertices.values().all(|p| {
        let q = xf.transform_point3(*p);
        p.distance(q) <= geom::tol(&[*p, q])
    });
    if is_identity(&xf) || lands_on_itself {
        return Err(bad(
            "the copy would lie on the original (the move is shorter than the length tolerance)",
        ));
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
