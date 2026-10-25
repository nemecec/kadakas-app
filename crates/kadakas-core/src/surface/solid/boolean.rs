//! The boolean operations of two closed solids (see the module `solid`).
//!
//! 1. **Check.** Both inputs must be solids ([`ShellState::Solid`]) that are not stuck to other
//!    geometry of the model. Nothing is changed before this is known.
//! 2. **Copy.** A model that holds only the faces of the two solids (the same ids, so the pieces
//!    can be followed) is cut with Intersect Faces. Every face of a solid that the other one
//!    crosses is split along the crossing. A crossing too small to be cut is a refusal.
//! 3. **Locate.** Each piece lies wholly in, outside or on the surface of the other solid, because
//!    no face crosses the surface any more. A few points of the piece (the centroids of its
//!    triangles) are located by a ray cast; if they disagree, the piece was not cut and the
//!    operation is refused.
//! 4. **Keep.** The rules of the operation say which pieces stay and which turn over (see
//!    [`fates`]). A piece on the surface of the other solid is told apart by its direction.
//! 5. **Tidy.** Neighbours in one plane, with the same direction and paint, merge. Edges and
//!    vertices that no kept face uses go.
//! 6. **Verify.** Each solid of the result must be closed and have the volume that the operation
//!    gives (checked with the volumes of the inputs). The model must have no problem that it did
//!    not have before, and must be within the limits.
//! 7. **Put in.** The result replaces the geometry of the two solids in the model.
//!
//! Everything is done in a copy (`SurfaceModel::apply`), so a refusal leaves the model as it was.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;
use serde_json::Value;

use super::super::arrange::reversed;
use super::super::check::Problem;
use super::super::edit::{self, ATTR_BACK_MATERIAL, ATTR_MATERIAL, rechain};
use super::super::geom::{self, on_plane};
use super::super::index::BoxTree;
use super::super::topo::Topo;
use super::super::{
    CurveId, Edge, EdgeId, Edit, Face, FaceId, IntersectScope, SurfaceError, SurfaceModel,
    VertexId, key,
};
use super::locate::{Soup, Where, samples};
use super::shell::{ShellState, crossings_of, measure, state_with};
use super::{Operand, SolidError, SolidOp};

/// The attribute that tells a piece which solid it came from, in the copy only.
const SIDE: &str = "solid_side";

fn cut(why: impl Into<String>) -> SurfaceError {
    SurfaceError::Solid(SolidError::Cut(why.into()))
}

/// What an operation made: the volumes of its solids in mm³ (see `SurfaceModel::solid_preview`).
pub(super) struct Outcome {
    pub volumes: Vec<f64>,
}

/// The edit `Edit::Solid`.
pub(in crate::surface) fn solid_edit(
    m: &mut SurfaceModel,
    op: SolidOp,
    first: &[FaceId],
    second: &[FaceId],
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    run(m, op, first, second, notes).map(|_| ())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    A,
    B,
}

/// Where a piece is against the other solid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    In = 0,
    Out = 1,
    /// On the surface of the other solid, looking the same way.
    OnSame = 2,
    /// On the surface of the other solid, looking the opposite way.
    OnOpp = 3,
}

/// What becomes of a piece of the solid `side`: the solids of the result it goes into (`0` is
/// the result of Union, Subtract, Intersect and Trim; Split has `0` (first less second), `1` (the
/// common part) and `2` (second less first); Trim has `1` for the second solid, which stays), and
/// whether it turns over.
fn fates(op: SolidOp, side: Side, at: Class) -> Vec<(usize, bool)> {
    use Class::{In, OnOpp, OnSame, Out};
    let keep = |g: usize| vec![(g, false)];
    match (op, side) {
        (SolidOp::Union, Side::A) => match at {
            Out | OnSame => keep(0),
            _ => vec![],
        },
        (SolidOp::Union, Side::B) => match at {
            Out => keep(0),
            _ => vec![],
        },
        (SolidOp::Intersect, Side::A) => match at {
            In | OnSame => keep(0),
            _ => vec![],
        },
        (SolidOp::Intersect, Side::B) => match at {
            In => keep(0),
            _ => vec![],
        },
        (SolidOp::Subtract, Side::A) | (SolidOp::Trim, Side::A) => match at {
            Out | OnOpp => keep(0),
            _ => vec![],
        },
        (SolidOp::Subtract, Side::B) => match at {
            In => vec![(0, true)],
            _ => vec![],
        },
        (SolidOp::Trim, Side::B) => match at {
            In => vec![(1, false), (0, true)],
            _ => keep(1),
        },
        (SolidOp::Split, Side::A) => match at {
            Out | OnOpp => keep(0),
            In => vec![(1, false), (2, true)],
            OnSame => keep(1),
        },
        (SolidOp::Split, Side::B) => match at {
            In => vec![(1, false), (0, true)],
            Out | OnOpp => keep(2),
            OnSame => vec![],
        },
    }
}

fn groups_of(op: SolidOp) -> usize {
    match op {
        SolidOp::Union | SolidOp::Subtract | SolidOp::Intersect => 1,
        SolidOp::Trim => 2,
        SolidOp::Split => 3,
    }
}

fn existing(
    m: &SurfaceModel,
    ids: &[FaceId],
    which: Operand,
) -> Result<BTreeSet<FaceId>, SurfaceError> {
    if ids.is_empty() {
        return Err(SurfaceError::Solid(SolidError::Empty(which)));
    }
    if let Some(id) = ids.iter().find(|id| !m.faces.contains_key(id)) {
        return Err(SurfaceError::NoFace(*id));
    }
    Ok(ids.iter().copied().collect())
}

/// Refuses an input that is not a solid.
fn must_be_solid(which: Operand, s: &super::Shell) -> Result<(), SurfaceError> {
    let e = match s.state {
        ShellState::Solid => return Ok(()),
        ShellState::Open => {
            let first = s.open_edges[0];
            SolidError::Open {
                which,
                count: s.open_edges.len(),
                first: first.edge,
                kind: first.kind,
            }
        }
        ShellState::SelfCrossing => SolidError::SelfCrossing {
            which,
            edge: s.crossings[0].0,
            face: s.crossings[0].1,
        },
        ShellState::Inverted => SolidError::Inverted(which),
        ShellState::Flat => SolidError::Flat(which),
    };
    Err(SurfaceError::Solid(e))
}

/// Refuses a solid whose edges or vertices other geometry uses.
fn must_be_free(
    m: &SurfaceModel,
    topo: &Topo,
    a: &BTreeSet<FaceId>,
    b: &BTreeSet<FaceId>,
) -> Result<(), SurfaceError> {
    let mut edges: BTreeMap<EdgeId, Operand> = BTreeMap::new();
    let mut verts: BTreeMap<VertexId, Operand> = BTreeMap::new();
    for (set, which) in [(a, Operand::First), (b, Operand::Second)] {
        for id in set {
            for (x, y) in m.faces[id].uses() {
                if let Some(e) = topo.edge(x, y) {
                    edges.insert(e, which);
                }
                verts.insert(x, which);
            }
        }
    }
    for (&e, &which) in &edges {
        if topo.faces_of(e).any(|f| !a.contains(&f) && !b.contains(&f)) {
            return Err(SurfaceError::Solid(SolidError::Attached(which)));
        }
    }
    for (&v, &which) in &verts {
        if topo.edges_at(v).any(|e| !edges.contains_key(&e)) {
            return Err(SurfaceError::Solid(SolidError::Attached(which)));
        }
    }
    Ok(())
}

/// A model with only the faces of the two solids, their edges and vertices (the same ids), and the
/// curves that lie wholly in it. Every face carries the mark of its side.
fn copy_of(
    m: &SurfaceModel,
    topo: &Topo,
    a: &BTreeSet<FaceId>,
    b: &BTreeSet<FaceId>,
) -> SurfaceModel {
    let mut faces = BTreeMap::new();
    let mut vertices = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for id in a.union(b) {
        let mut f = m.faces[id].clone();
        let side = match (a.contains(id), b.contains(id)) {
            (true, true) => "ab",
            (true, false) => "a",
            _ => "b",
        };
        f.attrs.insert(SIDE.into(), Value::from(side));
        for (x, y) in f.uses() {
            vertices.insert(x, m.vertices[&x]);
            if let Some(e) = topo.edge(x, y) {
                edges.insert(e, m.edges[&e]);
            }
        }
        faces.insert(*id, f);
    }
    let mut sub = SurfaceModel::from_parts(vertices, edges, faces, m.next_id);
    for id in sub.edges.keys() {
        if let Some(at) = m.edge_attrs.get(id) {
            sub.edge_attrs.insert(*id, at.clone());
        }
    }
    for (c, es) in &m.curves {
        if es.iter().all(|e| sub.edges.contains_key(e)) {
            sub.curves.insert(*c, es.clone());
        }
    }
    sub
}

/// Which solids a piece belongs to: (first, second). A face that both solids use is in both.
fn roles_of(f: &Face) -> (bool, bool) {
    match f.attrs.get(SIDE).and_then(Value::as_str) {
        Some("a") => (true, false),
        Some("b") => (false, true),
        _ => (true, true),
    }
}

/// The share of the area of a piece that the most common place must have. A piece that the cut
/// left whole lies on one side. A sliver next to the cut can have one thin triangle within the
/// plane tolerance of the other surface, which is no reason to refuse.
const SHARE: f64 = 0.8;

/// Where a piece is against the soup of the other solid: the place that most of its area has.
fn class_of(sub: &SurfaceModel, id: FaceId, other: &Soup) -> Result<Class, SurfaceError> {
    let f = &sub.faces[&id];
    let (lo, hi) = other.bounds();
    let pad = geom::plane_tol(&[lo, hi]) * 2.0;
    let pts =
        samples(sub, id).ok_or_else(|| cut(format!("face {id} cannot be cut into triangles")))?;
    let mut area = [0.0f64; 4];
    for (p, a) in &pts {
        let at = if p.cmplt(lo - pad).any() || p.cmpgt(hi + pad).any() {
            Class::Out
        } else {
            match other.locate(*p).ok_or_else(|| {
                cut(format!(
                    "face {id} lies too close to the other solid to tell where it is"
                ))
            })? {
                Where::In => Class::In,
                Where::Out => Class::Out,
                Where::On(n) if n.dot(f.plane.normal) > 0.0 => Class::OnSame,
                Where::On(_) => Class::OnOpp,
            }
        };
        area[at as usize] += a;
    }
    vote(area).ok_or_else(|| {
        cut(format!(
            "face {id} lies partly in and partly out of the other solid and could not be cut there"
        ))
    })
}

/// The place that holds at least [`SHARE`] of the area (`area` is by place: in, out, on the same
/// way, on the opposite way).
fn vote(area: [f64; 4]) -> Option<Class> {
    let total: f64 = area.iter().sum();
    let (best, most) = area
        .iter()
        .enumerate()
        .fold((0, 0.0), |b, (i, a)| if *a > b.1 { (i, *a) } else { b });
    (total > 0.0 && most >= SHARE * total)
        .then(|| [Class::In, Class::Out, Class::OnSame, Class::OnOpp][best])
}

/// The face seen from the other side.
fn turned(f: &Face) -> Face {
    let mut g = f.clone();
    g.plane = g.plane.flipped();
    g.outer = reversed(&g.outer);
    let mut holes: Vec<Vec<VertexId>> = g.holes.iter().map(|h| reversed(h)).collect();
    holes.sort();
    g.holes = holes;
    edit::swap_paint(&mut g.attrs);
    g
}

/// Removes the edges that no face uses, and the vertices that no edge uses. A curve that loses
/// edges stays as the chains that are left (the rule of MOD-06).
fn prune(res: &mut SurfaceModel) {
    let used: BTreeSet<(VertexId, VertexId)> = res
        .faces
        .values()
        .flat_map(|f| f.uses().map(|(a, b)| key(a, b)).collect::<Vec<_>>())
        .collect();
    let gone: BTreeSet<EdgeId> = res
        .edges
        .iter()
        .filter(|(_, e)| !used.contains(&e.key()))
        .map(|(id, _)| *id)
        .collect();
    let mut broken: BTreeSet<CurveId> = BTreeSet::new();
    for (c, es) in res.curves.iter_mut() {
        if es.iter().any(|e| gone.contains(e)) {
            es.retain(|e| !gone.contains(e));
            broken.insert(*c);
        }
    }
    for id in &gone {
        res.edges.remove(id);
        res.edge_attrs.remove(id);
    }
    rechain(res, &broken);
    let ends: BTreeSet<VertexId> = res.edges.values().flat_map(|e: &Edge| e.v).collect();
    res.vertices.retain(|v, _| ends.contains(v));
}

/// True if two faces lie in one plane and look the same way.
fn flush(res: &SurfaceModel, f: &Face, g: &Face) -> bool {
    f.plane.normal.dot(g.plane.normal) > 0.0
        && g.outer.iter().all(|v| on_plane(&f.plane, res.vertices[v]))
        && f.outer.iter().all(|v| on_plane(&g.plane, res.vertices[v]))
}

fn same_paint(f: &Face, g: &Face) -> bool {
    [ATTR_MATERIAL, ATTR_BACK_MATERIAL]
        .iter()
        .all(|k| f.attrs.get(*k) == g.attrs.get(*k))
}

/// The edges of the solids as they were, to tell the lines of the person from the cut.
struct Original {
    segs: Vec<(DVec3, DVec3)>,
    tree: BoxTree,
    /// The ids from here up were made by the cut.
    start: u32,
}

impl Original {
    fn of(sub: &SurfaceModel) -> Original {
        let segs: Vec<(DVec3, DVec3)> = sub
            .edges
            .values()
            .map(|e| (sub.vertices[&e.v[0]], sub.vertices[&e.v[1]]))
            .collect();
        let tree = BoxTree::new(
            segs.iter()
                .enumerate()
                .map(|(i, (a, b))| (a.min(*b), a.max(*b), i as u32))
                .collect(),
        );
        Original {
            segs,
            tree,
            start: sub.next_id,
        }
    }

    /// True if the point lies on an edge that the solids had (a line, a circle, a border).
    fn holds(&self, p: DVec3) -> bool {
        let pad = DVec3::splat(geom::tol(&[p]) * 2.0);
        let mut found = false;
        self.tree.query(p - pad, p + pad, &mut |i| {
            let (a, b) = self.segs[i as usize];
            found |= geom::point_segment(p, a, b).0 <= geom::tol(&[a, b, p]);
        });
        found
    }
}

/// Merges neighbours of one solid that lie in one plane, look the same way and have the same
/// paint, along the seams that the cut made. An edge that the solids had before (a line or a
/// circle of the person, also where the cut split it) is never erased, and a seam used by more
/// than two faces stays.
fn tidy(
    res: &mut SurfaceModel,
    group: &mut BTreeMap<FaceId, usize>,
    original: &Original,
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    let mut by_edge: BTreeMap<(VertexId, VertexId), Vec<FaceId>> = BTreeMap::new();
    for (&id, f) in &res.faces {
        for (a, b) in f.uses() {
            by_edge.entry(key(a, b)).or_default().push(id);
        }
    }
    let topo = Topo::of(res);
    let mut seams: Vec<EdgeId> = Vec::new();
    for (k, fs) in &by_edge {
        let [f, g] = fs.as_slice() else { continue };
        let (ff, gf) = (&res.faces[f], &res.faces[g]);
        if group[f] != group[g] || !same_paint(ff, gf) || !flush(res, ff, gf) {
            continue;
        }
        let Some(e) = topo.edge(k.0, k.1) else {
            continue;
        };
        let mid = (res.vertices[&k.0] + res.vertices[&k.1]) / 2.0;
        if e.0 >= original.start && !original.holds(mid) {
            seams.push(e);
        }
    }
    if seams.is_empty() {
        return Ok(());
    }
    let n = res.apply_with_notes(&Edit::EraseEdges { edges: seams })?;
    notes.extend(n);
    group.retain(|id, _| res.faces.contains_key(id));
    Ok(())
}

/// Why the crossing of the edge `e` through the face `f` could not be cut. When an end of the
/// edge is a vertex that lies off the plane of the face by more than the plane tolerance but not
/// by more than the length tolerance, the crossing piece is too short to draw and the vertex is
/// too far off to count as on the face: the text says so and how to move the solid.
fn not_cut_reason(sub: &SurfaceModel, e: EdgeId, f: FaceId, at: &str) -> String {
    let (Some(edge), Some(face)) = (sub.edges.get(&e), sub.faces.get(&f)) else {
        return format!(
            "edge {e} still passes through face {f} at {at}: the crossing is too small to cut"
        );
    };
    let near = edge
        .v
        .iter()
        .map(|v| (*v, face.plane.distance(sub.vertices[v]).abs()))
        .filter(|(v, d)| *d <= geom::tolerance_at(geom::size(sub.vertices[v])))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    match near {
        Some((v, d)) => {
            let tol = geom::tolerance_at(geom::size(sub.vertices[&v]));
            format!(
                "vertex {v} lies {d:.4} mm from the plane of face {f} at {at}: that is too far to count as on the face and too close to cut it. Move one solid by at least {tol} mm along the normal of the face, or set the vertex exactly on the face"
            )
        }
        None => format!(
            "edge {e} still passes through face {f} at {at}: the crossing is too small to cut"
        ),
    }
}

/// Runs an operation on `m` (see the module documentation).
pub(super) fn run(
    m: &mut SurfaceModel,
    op: SolidOp,
    first: &[FaceId],
    second: &[FaceId],
    notes: &mut Vec<String>,
) -> Result<Outcome, SurfaceError> {
    // 1. Check.
    let a = existing(m, first, Operand::First)?;
    let b = existing(m, second, Operand::Second)?;
    let before = m.check();
    let topo = Topo::of(m);
    let crossings = crossings_of(&before.warnings);
    let (sa, sb) = (
        state_with(m, &topo, &a, &crossings),
        state_with(m, &topo, &b, &crossings),
    );
    must_be_solid(Operand::First, &sa)?;
    must_be_solid(Operand::Second, &sb)?;
    must_be_free(m, &topo, &a, &b)?;

    // 2. Copy and cut.
    let mut sub = copy_of(m, &topo, &a, &b);
    let original = Original::of(&sub);
    match sub.apply(&Edit::IntersectFaces {
        scope: IntersectScope::Model,
        faces: Vec::new(),
        edges: Vec::new(),
    }) {
        Ok(()) => {}
        Err(e @ SurfaceError::Limit { .. }) => return Err(e),
        Err(e) => return Err(cut(e.to_string())),
    }
    let report = sub.check();
    if let Some(p) = report.problems.first() {
        return Err(cut(p.to_string()));
    }
    if let Some(Problem::NotIntersected(e, f, at)) = report
        .warnings
        .iter()
        .find(|p| matches!(p, Problem::NotIntersected(..)))
    {
        return Err(cut(not_cut_reason(&sub, *e, *f, &at.to_string())));
    }

    // 3. Locate. A face that both solids use lies on the surface of both, looking the same way.
    let ids_of = |side: Side| -> Vec<FaceId> {
        sub.faces
            .iter()
            .filter(|(_, f)| {
                let (in_a, in_b) = roles_of(f);
                if side == Side::A { in_a } else { in_b }
            })
            .map(|(id, _)| *id)
            .collect()
    };
    let (pa, pb) = (ids_of(Side::A), ids_of(Side::B));
    let soup = |ids: &[FaceId]| {
        Soup::of(&sub, ids.iter().copied())
            .ok_or_else(|| cut("a face cannot be cut into triangles"))
    };
    let (soup_a, soup_b) = (soup(&pa)?, soup(&pb)?);
    let mut entries: Vec<(FaceId, usize, bool)> = Vec::new();
    // The surface of the common volume: the pieces of the first solid in or on the second, and
    // the pieces of the second in the first.
    let mut common: BTreeSet<FaceId> = BTreeSet::new();
    for (ids, side, other) in [(&pa, Side::A, &soup_b), (&pb, Side::B, &soup_a)] {
        for &id in ids {
            let at = if roles_of(&sub.faces[&id]) == (true, true) {
                Class::OnSame
            } else {
                class_of(&sub, id, other)?
            };
            if matches!(
                (side, at),
                (Side::A, Class::In | Class::OnSame) | (Side::B, Class::In)
            ) {
                common.insert(id);
            }
            for (g, flip) in fates(op, side, at) {
                entries.push((id, g, flip));
            }
        }
    }
    let common_volume = measure(&sub, &common).0;
    entries.sort_by_key(|(id, g, flip)| (*id, *g, *flip));
    entries.dedup();

    // 4. Keep.
    let mut res = sub.clone();
    let mut faces: BTreeMap<FaceId, Face> = BTreeMap::new();
    let mut group: BTreeMap<FaceId, usize> = BTreeMap::new();
    let mut used: BTreeSet<FaceId> = BTreeSet::new();
    for (piece, g, flip) in entries {
        let id = if used.insert(piece) {
            piece
        } else {
            FaceId(res.alloc())
        };
        let mut f = sub.faces[&piece].clone();
        f.attrs.remove(SIDE);
        if flip {
            f = turned(&f);
        }
        faces.insert(id, f);
        group.insert(id, g);
    }
    res.faces = faces;
    prune(&mut res);

    // 5. Tidy.
    tidy(&mut res, &mut group, &original, notes)?;

    // 6. Verify.
    let topo_r = Topo::of(&res);
    let mut volumes = Vec::new();
    for g in 0..groups_of(op) {
        let set: BTreeSet<FaceId> = group
            .iter()
            .filter(|(_, x)| **x == g)
            .map(|(id, _)| *id)
            .collect();
        if set.is_empty() {
            let needed = if op == SolidOp::Split { g == 1 } else { g == 0 };
            if needed {
                return Err(SurfaceError::Solid(SolidError::NoResult(op)));
            }
            volumes.push(0.0);
            continue;
        }
        let s = state_with(&res, &topo_r, &set, &[]);
        if s.state != ShellState::Solid {
            return Err(SurfaceError::Solid(SolidError::ResultOpen(
                match s.open_edges.first() {
                    Some(o) => format!("edge {} is open ({})", o.edge, o.kind),
                    None => format!("{:?}", s.state).to_lowercase(),
                },
            )));
        }
        volumes.push(s.volume);
    }
    expect_volumes(op, sa.volume, sb.volume, common_volume, &volumes)?;

    // 7. Put in.
    put_in(m, &topo, &a, &b, res);
    edit::check_limits(m)?;
    if let Some(p) = m.problem() {
        return Err(SurfaceError::Solid(SolidError::ResultInvalid(p)));
    }
    let after = m.check();
    if let Some(p) = after.problems.iter().find(|p| !before.problems.contains(p)) {
        return Err(SurfaceError::Solid(SolidError::ResultInvalid(
            p.to_string(),
        )));
    }
    Ok(Outcome { volumes })
}

/// The volumes of the result must be the exact ones. With `i` the common volume (from the pieces
/// in both solids, found before the rules of the operation were applied): the common part is
/// `i`, a union is `va + vb - i`, a subtract is `va - i`, and Split adds up the same way. `i` must
/// be between 0 and the smaller input. If they differ, a piece was kept or dropped wrongly and
/// the result is not used.
fn expect_volumes(op: SolidOp, va: f64, vb: f64, i: f64, got: &[f64]) -> Result<(), SurfaceError> {
    let tol = 1e-6 * (va + vb);
    let near = |x: f64, want: f64| (x - want).abs() <= tol;
    let ok = (-tol..=va.min(vb) + tol).contains(&i)
        && match op {
            SolidOp::Union => near(got[0], va + vb - i),
            SolidOp::Intersect => near(got[0], i),
            SolidOp::Subtract => near(got[0], va - i),
            SolidOp::Trim => near(got[0], va - i) && near(got[1], vb),
            SolidOp::Split => near(got[0], va - i) && near(got[1], i) && near(got[2], vb - i),
        };
    if ok {
        Ok(())
    } else {
        Err(SurfaceError::Solid(SolidError::ResultVolume))
    }
}

/// Replaces the geometry of the two solids in `m` by the result. The solids share no edge or
/// vertex with other geometry (checked before), so their edges and vertices go whole.
fn put_in(
    m: &mut SurfaceModel,
    topo: &Topo,
    a: &BTreeSet<FaceId>,
    b: &BTreeSet<FaceId>,
    res: SurfaceModel,
) {
    let mut old_edges: BTreeSet<EdgeId> = BTreeSet::new();
    let mut old_vertices: BTreeSet<VertexId> = BTreeSet::new();
    for id in a.union(b) {
        let f = m.faces.remove(id).expect("checked");
        for (x, y) in f.uses() {
            old_vertices.insert(x);
            if let Some(e) = topo.edge(x, y) {
                old_edges.insert(e);
            }
        }
    }
    for e in &old_edges {
        m.edges.remove(e);
        m.edge_attrs.remove(e);
    }
    m.curves
        .retain(|_, es| !es.iter().any(|e| old_edges.contains(e)));
    for v in &old_vertices {
        m.vertices.remove(v);
    }
    m.vertices.extend(res.vertices);
    m.edges.extend(res.edges);
    m.edge_attrs.extend(res.edge_attrs);
    m.faces.extend(res.faces);
    m.curves.extend(res.curves);
    m.next_id = m.next_id.max(res.next_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::ShellState;

    #[test]
    fn the_rules_of_the_operations() {
        use Class::{In, OnOpp, OnSame, Out};
        let all = [In, Out, OnSame, OnOpp];
        let keep = |op, side| -> Vec<Class> {
            all.iter()
                .copied()
                .filter(|c| fates(op, side, *c) == vec![(0, false)])
                .collect()
        };
        assert_eq!(keep(SolidOp::Union, Side::A), vec![Out, OnSame]);
        assert_eq!(keep(SolidOp::Union, Side::B), vec![Out]);
        assert_eq!(keep(SolidOp::Intersect, Side::A), vec![In, OnSame]);
        assert_eq!(keep(SolidOp::Intersect, Side::B), vec![In]);
        assert_eq!(keep(SolidOp::Subtract, Side::A), vec![Out, OnOpp]);
        assert_eq!(keep(SolidOp::Trim, Side::A), vec![Out, OnOpp]);
        // The second solid of Subtract turns over where it is inside the first.
        assert_eq!(fates(SolidOp::Subtract, Side::B, In), vec![(0, true)]);
        for c in [Out, OnSame, OnOpp] {
            assert!(fates(SolidOp::Subtract, Side::B, c).is_empty());
        }
        // Trim keeps the whole second solid and also turns over the part inside.
        for c in [Out, OnSame, OnOpp] {
            assert_eq!(fates(SolidOp::Trim, Side::B, c), vec![(1, false)]);
        }
        assert_eq!(
            fates(SolidOp::Trim, Side::B, In),
            vec![(1, false), (0, true)]
        );
        // Split: the common part is in group 1 and its faces turn over for the two remainders.
        assert_eq!(
            fates(SolidOp::Split, Side::A, In),
            vec![(1, false), (2, true)]
        );
        assert_eq!(
            fates(SolidOp::Split, Side::B, In),
            vec![(1, false), (0, true)]
        );
        assert_eq!(fates(SolidOp::Split, Side::A, Out), vec![(0, false)]);
        assert_eq!(fates(SolidOp::Split, Side::A, OnOpp), vec![(0, false)]);
        assert_eq!(fates(SolidOp::Split, Side::A, OnSame), vec![(1, false)]);
        assert_eq!(fates(SolidOp::Split, Side::B, Out), vec![(2, false)]);
        assert_eq!(fates(SolidOp::Split, Side::B, OnOpp), vec![(2, false)]);
        assert!(fates(SolidOp::Split, Side::B, OnSame).is_empty());
        // Every group number is below the number of groups of the operation.
        for op in SolidOp::ALL {
            for side in [Side::A, Side::B] {
                for c in all {
                    assert!(fates(op, side, c).iter().all(|(g, _)| *g < groups_of(op)));
                }
            }
        }
    }

    #[test]
    fn volumes_that_do_not_fit_are_refused() {
        let (va, vb, i) = (8.0, 8.0, 4.0);
        assert!(expect_volumes(SolidOp::Union, va, vb, i, &[12.0]).is_ok());
        assert!(expect_volumes(SolidOp::Union, va, vb, i, &[13.0]).is_err());
        assert!(expect_volumes(SolidOp::Union, va, vb, i, &[11.0]).is_err());
        assert!(expect_volumes(SolidOp::Intersect, va, vb, i, &[4.0]).is_ok());
        assert!(expect_volumes(SolidOp::Intersect, va, vb, i, &[5.0]).is_err());
        assert!(expect_volumes(SolidOp::Subtract, va, vb, i, &[4.0]).is_ok());
        assert!(expect_volumes(SolidOp::Subtract, va, vb, i, &[5.0]).is_err());
        assert!(expect_volumes(SolidOp::Trim, va, vb, i, &[4.0, 8.0]).is_ok());
        assert!(expect_volumes(SolidOp::Trim, va, vb, i, &[4.0, 7.0]).is_err());
        assert!(expect_volumes(SolidOp::Split, va, vb, i, &[4.0, 4.0, 4.0]).is_ok());
        let e = expect_volumes(SolidOp::Split, va, vb, i, &[4.0, 5.0, 4.0]).unwrap_err();
        assert_eq!(e, SurfaceError::Solid(SolidError::ResultVolume));
        // The common volume is between 0 and the smaller input.
        assert!(expect_volumes(SolidOp::Union, va, vb, 9.0, &[7.0]).is_err());
        assert!(expect_volumes(SolidOp::Union, va, vb, -1.0, &[17.0]).is_err());
        assert!(expect_volumes(SolidOp::Union, va, vb, 0.0, &[16.0]).is_ok());
    }

    fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Edit {
        let c = |i: usize| -> [f64; 3] {
            std::array::from_fn(|k| if i >> k & 1 == 0 { lo[k] } else { hi[k] })
        };
        let mut edges = Vec::new();
        for i in 0..8usize {
            for bit in [1usize, 2, 4] {
                if i & bit == 0 {
                    edges.push([c(i), c(i | bit)]);
                }
            }
        }
        Edit::AddEdges { edges }
    }

    #[test]
    fn a_piece_that_was_not_cut_is_not_given_a_place() {
        let mut m = SurfaceModel::new();
        m.apply(&cuboid([0.0; 3], [1000.0; 3])).unwrap();
        m.apply(&cuboid([500.0; 3], [1500.0; 3])).unwrap();
        let inward: Vec<FaceId> = m
            .solid_report()
            .shells
            .iter()
            .filter(|s| s.state == ShellState::Inverted)
            .flat_map(|s| s.faces.clone())
            .collect();
        m.apply(&Edit::ReverseFaces { faces: inward }).unwrap();
        let r = m.solid_report();
        let solids: Vec<_> = r.solids().collect();
        let (a, b): (BTreeSet<FaceId>, BTreeSet<FaceId>) = (
            solids[0].faces.iter().copied().collect(),
            solids[1].faces.iter().copied().collect(),
        );
        let topo = Topo::of(&m);
        let sub = copy_of(&m, &topo, &a, &b);
        let soup_b = Soup::of(
            &sub,
            sub.faces
                .iter()
                .filter(|(_, f)| roles_of(f) == (false, true))
                .map(|(id, _)| *id),
        )
        .unwrap();
        // The faces of the first box, not cut: the wall x = 1000 is partly in the second box.
        let mut classes = Vec::new();
        for id in &a {
            classes.push(class_of(&sub, *id, &soup_b));
        }
        let errors: Vec<String> = classes
            .iter()
            .filter_map(|c| c.as_ref().err().map(ToString::to_string))
            .collect();
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(
            errors
                .iter()
                .all(|e| e.contains("partly in and partly out"))
        );
        // The three faces away from the second box are outside it.
        assert_eq!(
            classes
                .iter()
                .filter(|c| matches!(c, Ok(Class::Out)))
                .count(),
            3
        );
    }

    #[test]
    fn a_thin_sliver_on_the_surface_does_not_decide_but_a_big_share_does() {
        // A piece next to the cut can have one thin triangle within the plane tolerance of the
        // other surface. The rest decides.
        assert_eq!(vote([97.0, 0.0, 3.0, 0.0]), Some(Class::In));
        assert_eq!(vote([0.0, 90.0, 0.0, 10.0]), Some(Class::Out));
        assert_eq!(vote([0.0, 0.0, 5.0, 95.0]), Some(Class::OnOpp));
        assert_eq!(vote([0.0, 0.0, 100.0, 0.0]), Some(Class::OnSame));
        // A face that the cut missed lies on both sides, or half on the surface.
        assert_eq!(vote([75.0, 25.0, 0.0, 0.0]), None);
        assert_eq!(vote([50.0, 0.0, 50.0, 0.0]), None);
        assert_eq!(vote([0.0; 4]), None);
    }

    #[test]
    fn turning_a_face_twice_gives_the_face_again() {
        let mut m = SurfaceModel::new();
        m.apply(&cuboid([0.0; 3], [1000.0; 3])).unwrap();
        for (_, f) in m.faces() {
            let mut f = f.clone();
            f.attrs.insert(ATTR_MATERIAL.into(), Value::from("m"));
            let t = turned(&f);
            assert_eq!(t.plane, f.plane.flipped());
            assert_eq!(t.attrs.get(ATTR_BACK_MATERIAL), Some(&Value::from("m")));
            assert_eq!(turned(&t), f);
        }
    }
}
