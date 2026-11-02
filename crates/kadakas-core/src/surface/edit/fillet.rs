//! Fillet and chamfer (MOD-21) on a surface model: the plan of `surface::fillet` put into the
//! model as one edit, with the checks of the result.
//!
//! # Ids and look
//!
//! The faces that change (the faces next to the chosen edges and the third face at a corner) keep
//! their ids, planes and attributes. An edge that stays and loses its end at a corner keeps its id,
//! flags and place in a curve; only its end moves. The chosen edges and their end vertices go.
//! The strips and the corner pieces are new faces, with new edges and vertices, made strip by
//! strip in the order of the edge ids, then the corners in the order of the vertex ids. A new face
//! takes the paint of the largest face next to it (a tie goes to the lower id).
//!
//! # Flags
//!
//! With `smooth` (the default of a round) the edges inside a round (between two segments of a
//! strip, the miter of two strips, the triangles of a ball corner) get `soft` and `smooth`, as in
//! Soften Edges, so the round shades as one surface. A rail (where a strip meets a face that stays)
//! gets `soft` only: its line is not drawn, and the big face keeps its own shading. A cut gets no
//! flags.
//!
//! # Curves
//!
//! A chosen edge leaves its curve, and the rest of the curve is cut into chains as an erase does.
//! The exact circle of a curve is kept only where the chain still fits it; else it is dropped or
//! shortened with a note (`analytic::reconcile`).
//!
//! # Refused (the model unchanged)
//!
//! A chosen edge that is not between two faces of a closed solid, a solid that is not closed, a
//! corner that the planner does not support, a size that does not fit (with the largest size that
//! fits), more than [`MAX_EDIT_EDGES`] new edges or a model limit, and a result that is not a
//! closed solid, has a new problem, cuts into other geometry, or changes the volume more than the
//! wedges of the edges allow.

use super::super::fillet::{self, FilletError, Profile, Pt, Shape, ShapeFace};
use super::super::{CurveId, check, solid};
use super::style::{ATTR_SMOOTH, ATTR_SOFT, flagged};
use super::*;

/// Which edges a fillet or a chamfer takes (MOD-21). The result is the union of the given
/// `edges` and the edges that the filter finds among the edges of `faces` (or of the whole model
/// with `all`). The filter keeps an edge between exactly two faces that meet at an angle above
/// `min_angle` degrees (5 by default), is not soft or smooth (an edge of a curved surface), and is
/// convex, or concave with `concave`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeChoice {
    pub edges: Vec<EdgeId>,
    pub faces: Vec<FaceId>,
    pub all: bool,
    pub concave: bool,
    pub min_angle: Option<f64>,
}

impl SurfaceModel {
    /// The edges of a choice (see [`EdgeChoice`]), in id order. A missing edge or face is an
    /// error.
    pub fn fillet_edges(&self, c: &EdgeChoice) -> Result<Vec<EdgeId>, SurfaceError> {
        let t = Topo::of(self);
        let min = c.min_angle.unwrap_or(fillet::DEFAULT_MIN_ANGLE);
        let mut out: BTreeSet<EdgeId> = BTreeSet::new();
        for e in &c.edges {
            if !self.edges.contains_key(e) {
                return Err(SurfaceError::NoEdge(*e));
            }
            out.insert(*e);
        }
        let mut pool: BTreeSet<EdgeId> = BTreeSet::new();
        if c.all {
            pool.extend(self.edges.keys().copied());
        }
        for f in &c.faces {
            let face = self.faces.get(f).ok_or(SurfaceError::NoFace(*f))?;
            pool.extend(face.uses().filter_map(|(a, b)| t.edge(a, b)));
        }
        for e in pool {
            let attrs = self.edge_attrs.get(&e);
            if flagged(attrs, ATTR_SOFT) || flagged(attrs, ATTR_SMOOTH) {
                continue;
            }
            if let Some((phi, convex)) = kind_of(self, &t, e)
                && phi.to_degrees() > min
                && (convex || c.concave)
            {
                out.insert(e);
            }
        }
        Ok(out.into_iter().collect())
    }
}

/// The angle between the face normals of an edge (radians) and whether it is convex, for an edge
/// between exactly two faces that use it in opposite directions.
fn kind_of(m: &SurfaceModel, t: &Topo, e: EdgeId) -> Option<(f64, bool)> {
    let ed = m.edges.get(&e)?;
    let fs: Vec<FaceId> = t.faces_of(e).collect();
    let [f, g] = fs.as_slice() else {
        return None;
    };
    let (ff, gf) = (&m.faces[f], &m.faces[g]);
    let (a, b) = (ed.v[0], ed.v[1]);
    let along = |x: &Face| x.uses().any(|u| u == (a, b));
    let (first, second) = match (along(ff), along(gf)) {
        (true, false) => (ff, gf),
        (false, true) => (gf, ff),
        _ => return None,
    };
    let d = (m.vertices[&b] - m.vertices[&a]).normalize_or_zero();
    Some(fillet::edge_kind(
        first.plane.normal,
        second.plane.normal,
        d,
    ))
}

/// The edit: rounds (`segments` given or the default) or cuts the `edges`.
#[allow(clippy::too_many_arguments)]
pub(super) fn fillet(
    m: &mut SurfaceModel,
    before: &SurfaceModel,
    edges: &[EdgeId],
    size: f64,
    round: Option<Option<u32>>,
    smooth: bool,
) -> Result<(), SurfaceError> {
    let ids: BTreeSet<EdgeId> = edges.iter().copied().collect();
    if ids.is_empty() {
        return Err(FilletError::NoEdges.into());
    }
    if let Some(e) = ids.iter().find(|e| !m.edges.contains_key(e)) {
        return Err(SurfaceError::NoEdge(*e));
    }
    if ids.len() > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit",
            max: MAX_EDIT_EDGES,
            count: ids.len(),
        });
    }
    let t = Topo::of(m);
    let list: Vec<(EdgeId, VertexId, VertexId)> = ids
        .iter()
        .map(|e| (*e, m.edges[e].v[0], m.edges[e].v[1]))
        .collect();
    // Each edge lies between two faces of a closed solid.
    let mut set: BTreeSet<FaceId> = BTreeSet::new();
    let mut max_phi: f64 = 0.0;
    for &(e, _, _) in &list {
        let Some((phi, _)) = kind_of(m, &t, e) else {
            return Err(FilletError::NotSolidEdge { edge: e }.into());
        };
        max_phi = max_phi.max(phi);
        let f = t.faces_of(e).next().expect("two faces");
        if !set.contains(&f) {
            set.extend(m.solid_of(f).unwrap_or_default());
        }
    }
    let state = solid::quick_state(m, &set);
    if !state.is_solid() {
        let why = match state.open_edges.first() {
            Some(o) => format!("edge {} is open ({})", o.edge, o.kind),
            None if state.volume < 0.0 => "its faces look inward".to_string(),
            None => "it holds no volume".to_string(),
        };
        return Err(FilletError::NotSolid {
            edge: list[0].0,
            why,
        }
        .into());
    }
    // The faces at the ends of the edges.
    let ends: BTreeSet<VertexId> = list.iter().flat_map(|x| [x.1, x.2]).collect();
    let mut shape = Shape::default();
    for v in &ends {
        for e in t.edges_at(*v) {
            if t.faces_of(e).next().is_none() {
                return Err(FilletError::Corner {
                    vertex: *v,
                    why: format!("the loose edge {e} meets the vertex"),
                }
                .into());
            }
            for f in t.faces_of(e) {
                if shape.faces.contains_key(&f) {
                    continue;
                }
                let face = &m.faces[&f];
                for x in face.loops().flatten() {
                    shape.points.insert(*x, m.vertices[x]);
                }
                shape.faces.insert(
                    f,
                    ShapeFace {
                        plane: face.plane,
                        loops: face.loops().cloned().collect(),
                    },
                );
            }
        }
    }
    let profile = match round {
        Some(segments) => Profile::Round {
            radius: size,
            segments: segments.unwrap_or_else(|| fillet::default_segments(size, max_phi)),
        },
        None => Profile::Flat { distance: size },
    };
    let plan = match fillet::plan(&shape, &list, &profile) {
        Ok(p) => p,
        Err(FilletError::DoesNotFit {
            noun, asked, why, ..
        }) => {
            let most = fillet::largest_fit(&shape, &list, &profile, asked);
            return Err(FilletError::DoesNotFit {
                noun,
                asked,
                why,
                most: Some(most),
            }
            .into());
        }
        Err(e) => return Err(e.into()),
    };
    if plan
        .points
        .iter()
        .any(|p| !p.is_finite() || geom::size(*p) > MAX_COORD)
    {
        return Err(SurfaceError::BadPoint);
    }
    let curves: BTreeSet<CurveId> = ids.iter().filter_map(|e| m.curve_of(*e)).collect();
    let made = apply(m, before, &plan, &ids, smooth)?;
    super::rechain(m, &curves);
    check_result(before, m, &plan, &set, &made, state.volume)
}

/// What the edit made, for the checks.
struct Made {
    faces: BTreeSet<FaceId>,
    changed: BTreeSet<FaceId>,
    edges: BTreeSet<EdgeId>,
    verts: BTreeSet<VertexId>,
}

/// Puts the plan into the model.
fn apply(
    m: &mut SurfaceModel,
    before: &SurfaceModel,
    plan: &fillet::Plan,
    chosen: &BTreeSet<EdgeId>,
    smooth: bool,
) -> Result<Made, SurfaceError> {
    let mut w = Work::new(m);
    let vids: Vec<VertexId> = plan.points.iter().map(|p| w.new_vertex(*p)).collect();
    let id = |p: Pt| match p {
        Pt::Old(v) => v,
        Pt::New(i) => vids[i],
    };
    let invalid = |why: String| SurfaceError::from(FilletError::Result(why));
    // The edges that get a new end, found before anything changes.
    let mut trims: Vec<(EdgeId, VertexId, VertexId)> = Vec::new();
    for tr in &plan.trims {
        let e =
            w.t.edge(tr.far, tr.old)
                .ok_or_else(|| invalid(format!("no edge from {} to {}", tr.far, tr.old)))?;
        trims.push((e, tr.old, id(tr.to)));
    }
    let mut kept: BTreeMap<FaceId, Face> = BTreeMap::new();
    for f in plan.faces.keys() {
        let face = w
            .take_face(*f)
            .ok_or_else(|| invalid(format!("face {f} is missing")))?;
        kept.insert(*f, face);
    }
    for e in chosen {
        w.remove_edge(*e);
    }
    for (e, old, new) in &trims {
        let Some(ed) = w.m.edges.get(e).copied() else {
            continue;
        };
        w.t.remove_edge(*e, ed);
        let mut ne = ed;
        for x in &mut ne.v {
            if *x == *old {
                *x = *new;
            }
        }
        w.m.edges.insert(*e, ne);
        w.t.add_edge(*e, ne);
    }
    for v in &plan.gone {
        if w.t.degree(*v) != 0 {
            return Err(invalid(format!("vertex {v} keeps an edge")));
        }
        w.m.vertices.remove(v);
    }
    // The edges of the new loops, strips first, then the corners and the changed faces.
    let mut made_edges: BTreeSet<EdgeId> = trims.iter().map(|t| t.0).collect();
    let mut new_count = 0usize;
    let mut ring = |w: &mut Work<'_>, l: &[VertexId], made: &mut BTreeSet<EdgeId>| {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            if w.t.edge(a, b).is_none() {
                made.insert(w.new_edge(a, b));
                new_count += 1;
            }
        }
    };
    let new_loops: Vec<Vec<VertexId>> = plan
        .new_faces
        .iter()
        .map(|f| f.points.iter().map(|p| id(*p)).collect())
        .collect();
    for l in &new_loops {
        ring(&mut w, l, &mut made_edges);
    }
    let face_loops: BTreeMap<FaceId, Vec<Vec<VertexId>>> = plan
        .faces
        .iter()
        .map(|(f, ls)| {
            (
                *f,
                ls.iter()
                    .map(|l| l.iter().map(|p| id(*p)).collect())
                    .collect(),
            )
        })
        .collect();
    for ls in face_loops.values() {
        for l in ls {
            ring(&mut w, l, &mut made_edges);
        }
    }
    if new_count > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit (a round makes about three edges for each segment of each \
                   chosen edge)",
            max: MAX_EDIT_EDGES,
            count: new_count,
        });
    }
    for (f, ls) in face_loops {
        let mut face = kept.remove(&f).expect("taken above");
        face.outer = arrange::canonical(&ls[0]);
        let mut holes: Vec<Vec<VertexId>> = ls[1..].iter().map(|h| arrange::canonical(h)).collect();
        holes.sort();
        face.holes = holes;
        w.put_face(f, face);
    }
    // The new faces, with the paint of the largest face next to them.
    let paint_of = |looks: &[FaceId]| -> Vec<(String, serde_json::Value)> {
        let best = looks.iter().copied().max_by(|a, b| {
            let (x, y) = (
                before.face_area(*a).unwrap_or(0.0),
                before.face_area(*b).unwrap_or(0.0),
            );
            x.total_cmp(&y).then(b.cmp(a))
        });
        let Some(f) = best.and_then(|f| before.faces.get(&f)) else {
            return Vec::new();
        };
        [ATTR_MATERIAL, ATTR_BACK_MATERIAL]
            .iter()
            .filter_map(|k| f.attrs.get(*k).map(|v| ((*k).to_string(), v.clone())))
            .collect()
    };
    let mut faces: BTreeSet<FaceId> = BTreeSet::new();
    for (nf, l) in plan.new_faces.iter().zip(&new_loops) {
        let plane = Plane::new(w.pos(l[0]), nf.normal)
            .ok_or_else(|| invalid("a new face has no direction".into()))?;
        let mut g = Face::new(plane, arrange::canonical(l), Vec::new());
        for (k, v) in paint_of(&nf.looks) {
            g.attrs.insert(k, v);
        }
        faces.insert(w.new_face(g));
    }
    if smooth {
        for (set, flags) in [
            (&plan.blend, &[ATTR_SOFT, ATTR_SMOOTH][..]),
            (&plan.rails, &[ATTR_SOFT][..]),
        ] {
            for (a, b) in set {
                if let Some(e) = w.t.edge(id(*a), id(*b)) {
                    let at = w.m.edge_attrs.entry(e).or_default();
                    for f in flags {
                        at.insert((*f).into(), serde_json::Value::Bool(true));
                    }
                }
            }
        }
    }
    let verts: BTreeSet<VertexId> = vids.iter().copied().collect();
    let edges: BTreeSet<EdgeId> = made_edges;
    Ok(Made {
        faces,
        changed: plan.faces.keys().copied().collect(),
        edges,
        verts,
    })
}

/// The checks of the result: limits, curves, new problems, contacts with other geometry, a closed
/// solid, and a change of volume within the wedges of the edges.
fn check_result(
    before: &SurfaceModel,
    m: &SurfaceModel,
    plan: &fillet::Plan,
    shell: &BTreeSet<FaceId>,
    made: &Made,
    volume_before: f64,
) -> Result<(), SurfaceError> {
    let invalid = |why: String| SurfaceError::from(FilletError::Result(why));
    check_limits(m)?;
    if let Some(c) = check::curves(m) {
        return Err(invalid(c.to_string()));
    }
    let after = check::structure_problems(m);
    if !after.is_empty() {
        let old = check::structure_problems(before);
        if let Some(p) = after.difference(&old).next() {
            return Err(invalid(p.to_string()));
        }
    }
    let mut faces = made.faces.clone();
    faces.extend(made.changed.iter().copied());
    if let Some(why) = pushpull::new_contact_text(before, m, &made.edges, &faces) {
        return Err(invalid(format!("it would cut into other geometry: {why}")));
    }
    if let Some(p) = pushpull::new_distance_problem(before, m, &made.edges, &made.verts) {
        return Err(invalid(p.to_string()));
    }
    let mut set: Vec<FaceId> = shell
        .iter()
        .copied()
        .filter(|f| m.faces.contains_key(f))
        .collect();
    set.extend(made.faces.iter().copied());
    let state = m
        .solid_state(&set)
        .ok_or_else(|| invalid("the faces of the solid are missing".into()))?;
    if !state.is_solid() {
        let why = match (state.open_edges.first(), state.crossings.first()) {
            (Some(o), _) => format!("edge {} would be open ({})", o.edge, o.kind),
            (None, Some((e, f))) => format!("edge {e} would pass through face {f}"),
            _ => "it would not close a volume".to_string(),
        };
        return Err(invalid(why));
    }
    let dv = state.volume - volume_before;
    let slack = 1e-9 * volume_before.abs() + geom::tol(&[]) * state.area;
    if dv < -(plan.convex_bound + slack) || dv > plan.concave_bound + slack {
        return Err(invalid(format!(
            "the volume would change by {dv:.3} mm³, more than the rounded edges allow"
        )));
    }
    if plan.concave_bound == 0.0 && dv > slack {
        return Err(invalid(format!(
            "the volume would grow by {dv:.3} mm³ although only convex edges change"
        )));
    }
    Ok(())
}
