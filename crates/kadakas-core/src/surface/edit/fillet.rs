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

use super::super::FaceTriangles;
use super::super::clash::{self, Contact};
use super::super::fillet::{self, FilletError, Profile, Pt, Shape, ShapeFace};
use super::super::index::BoxTree;
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

/// The edit: rounds (`segments` given or the default) or cuts the `edges`. A size that does not
/// fit is refused with the largest size that the whole edit accepts (see [`largest_working`]).
pub(super) fn fillet(
    m: &mut SurfaceModel,
    before: &SurfaceModel,
    edges: &[EdgeId],
    size: f64,
    round: Option<Option<u32>>,
    smooth: bool,
) -> Result<(), SurfaceError> {
    match fillet_once(m, before, edges, size, round, smooth) {
        Err(SurfaceError::Fillet(FilletError::DoesNotFit {
            noun,
            asked,
            why,
            most,
            gaps,
            above,
        })) => {
            let most = largest_working(before, edges, asked, most.unwrap_or(0.0), round, smooth);
            // Larger sizes that work are named only when the whole edit takes both ends.
            let takes = |x: f64| {
                let mut w = before.clone();
                fillet_once(&mut w, before, edges, x, round, smooth).is_ok()
            };
            let above = above
                .filter(|_| most <= 0.0)
                .filter(|[a, b]| takes(*a) && takes(*b));
            Err(FilletError::DoesNotFit {
                noun,
                asked,
                why,
                most: Some(most),
                gaps,
                above,
            }
            .into())
        }
        other => other,
    }
}

/// The largest size below `asked` with which the whole edit works (the plan and every check of
/// the result), rounded down to two significant digits, from the guess of the planner. Each try
/// runs the edit on a copy. A guess that works is pushed up to the next two-digit values that
/// still work. A guess that does not work is halved between 0 and the guess (at most 10 tries)
/// and stepped down until a try works. 0 when no size works.
fn largest_working(
    before: &SurfaceModel,
    edges: &[EdgeId],
    asked: f64,
    guess: f64,
    round: Option<Option<u32>>,
    smooth: bool,
) -> f64 {
    let works = |x: f64| {
        let mut w = before.clone();
        x > 0.0 && x < asked && fillet_once(&mut w, before, edges, x, round, smooth).is_ok()
    };
    let up = |c: f64| {
        let mut c = c;
        for _ in 0..3 {
            let next = round_down(c + step_of(c));
            if next <= c || !works(next) {
                break;
            }
            c = next;
        }
        c
    };
    let c = round_down(guess);
    if works(c) {
        return up(c);
    }
    let (mut lo, mut hi) = (0.0, if c > 0.0 { c } else { asked });
    for _ in 0..10 {
        let mid = (lo + hi) / 2.0;
        if works(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mut c = round_down(lo);
    for _ in 0..6 {
        if c <= 0.0 {
            return 0.0;
        }
        if works(c) {
            return up(c);
        }
        c = round_down(c * 0.8);
    }
    0.0
}

/// The step of the second significant digit of `x` (0.01 for 0.47, 1 for 12).
fn step_of(x: f64) -> f64 {
    10f64.powf(x.log10().floor() - 1.0)
}

/// `x` rounded down to two significant digits (8.49 to 8.4, 299.99 to 290).
pub(super) fn round_down(x: f64) -> f64 {
    if x.is_nan() || x <= 0.0 || !x.is_finite() {
        return 0.0;
    }
    let f = 10f64.powf(x.log10().floor() - 1.0);
    let r = (x / f).floor() * f;
    // Clean the digits of the multiplication.
    (r * 1e9).round() / 1e9
}

/// One try of the edit (see [`fillet`]). A size that the planner refuses comes back with the
/// planner's own guess of the largest size in `most`.
fn fillet_once(
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
    // Each edge lies between two faces of a closed solid. The shells are found once.
    let shells = solid::quick_shells(m);
    let mut shell_of: BTreeMap<FaceId, usize> = BTreeMap::new();
    for (i, sh) in shells.iter().enumerate() {
        for f in &sh.faces {
            shell_of.entry(*f).or_insert(i);
        }
    }
    let mut touched: BTreeSet<usize> = BTreeSet::new();
    let (mut max_phi, mut min_phi): (f64, f64) = (0.0, std::f64::consts::PI);
    for &(e, _, _) in &list {
        let Some((phi, _)) = kind_of(m, &t, e) else {
            return Err(FilletError::NotSolidEdge { edge: e }.into());
        };
        max_phi = max_phi.max(phi);
        min_phi = min_phi.min(phi);
        let f = t.faces_of(e).next().expect("two faces");
        if let Some(i) = shell_of.get(&f) {
            touched.insert(*i);
        }
    }
    let set: BTreeSet<FaceId> = touched
        .iter()
        .flat_map(|i| shells[*i].faces.iter().copied())
        .collect();
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
    // The profile of a size: given segments, or the default for the largest angle that keeps
    // segments of four length tolerances on the smallest angle too.
    let profile_at = |x: f64| match round {
        Some(segments) => Profile::Round {
            radius: x,
            segments: segments.unwrap_or_else(|| {
                fillet::default_segments(x, max_phi).min(fillet::fitting_segments(x, min_phi))
            }),
        },
        None => Profile::Flat { distance: x },
    };
    let profile = profile_at(size);
    let plan = match fillet::plan(&shape, &list, &profile) {
        Ok(p) => p,
        Err(FilletError::DoesNotFit {
            noun, asked, why, ..
        }) => {
            let scan = fillet::size_scan(&shape, &list, &profile_at, asked);
            return Err(FilletError::DoesNotFit {
                noun,
                asked,
                why,
                most: Some(scan.most),
                gaps: scan.gaps,
                above: scan.above,
            }
            .into());
        }
        Err(e) => return Err(e.into()),
    };
    // The new edges, counted from the plan before anything changes.
    let count = fillet::new_edges(&plan);
    if count > MAX_EDIT_EDGES {
        let most_segments = match profile {
            Profile::Round { segments, .. } => {
                let fits = |k: u32| {
                    let p = Profile::Round {
                        radius: size,
                        segments: k,
                    };
                    fillet::plan(&shape, &list, &p)
                        .is_ok_and(|p| fillet::new_edges(&p) <= MAX_EDIT_EDGES)
                };
                // The count grows with the segments: halve between 1 and the asked number.
                let (mut good, mut bad) = (0u32, segments);
                while bad - good > 1 {
                    let mid = (good + bad) / 2;
                    if fits(mid) {
                        good = mid;
                    } else {
                        bad = mid;
                    }
                }
                (good > 0 && fits(good)).then_some(good)
            }
            Profile::Flat { .. } => None,
        };
        return Err(FilletError::TooManyEdges {
            count,
            max: MAX_EDIT_EDGES,
            most_segments,
        }
        .into());
    }
    if plan
        .points
        .iter()
        .any(|p| !p.is_finite() || geom::size(*p) > MAX_COORD)
    {
        return Err(SurfaceError::BadPoint);
    }
    let curves: BTreeSet<CurveId> = ids.iter().filter_map(|e| m.curve_of(*e)).collect();
    let made = apply(m, before, &plan, &ids, smooth)?;
    let mut curves = curves;
    curves.extend(made.curves.iter().copied());
    super::rechain(m, &curves);
    check_result(before, m, &plan, &set, &made, state.volume)
}

/// What the edit made, for the checks.
struct Made {
    /// Curves that lost a trimmed edge.
    curves: BTreeSet<CurveId>,
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
    let mut cut_curves: BTreeSet<CurveId> = BTreeSet::new();
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
        // An edge of an exact circle that loses its end leaves the circle: the rest of the
        // curve keeps its description (shortened or cut, with a note).
        let in_curve =
            w.m.curves
                .iter()
                .find(|(_, es)| es.contains(e))
                .map(|(c, _)| *c);
        if let Some(c) = in_curve
            && w.m.analytic.contains_key(&c)
        {
            if let Some(es) = w.m.curves.get_mut(&c) {
                es.retain(|x| x != e);
            }
            cut_curves.insert(c);
        }
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
    let none: BTreeSet<(Pt, Pt)> = BTreeSet::new();
    let (blend, rails) = if smooth {
        (&plan.blend, &plan.rails)
    } else {
        (&none, &none)
    };
    let seam_flags: &[&str] = if smooth {
        &[ATTR_SOFT, ATTR_SMOOTH]
    } else {
        &[ATTR_SOFT]
    };
    {
        for (set, flags) in [
            (blend, &[ATTR_SOFT, ATTR_SMOOTH][..]),
            (rails, &[ATTR_SOFT][..]),
            (&plan.seams, seam_flags),
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
        curves: cut_curves,
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
    if check::curves(m).is_some() {
        return Err(invalid("a curve of the model would break".into()));
    }
    let after = check::structure_problems(m);
    if !after.is_empty() {
        let old = check::structure_problems(before);
        if let Some(p) = after.difference(&old).next() {
            return Err(invalid(plain(m, p)));
        }
    }
    let mut faces = made.faces.clone();
    faces.extend(made.changed.iter().copied());
    // The new pieces of one corner touch each other by design: the closed-solid check below
    // finds them when they pass through each other. Here only the old geometry counts.
    let start = before.next_id();
    let both_new = |e: EdgeId, g: FaceId| e.0 >= start && g.0 >= start;
    if let Some((e, _, c)) = contact::new_contact(before, m, &made.edges, &faces, &both_new) {
        let how = match c {
            Contact::Through => "cut through",
            Contact::EndsOn => "touch",
            Contact::LiesIn => "lie in",
        };
        return Err(invalid(format!(
            "the new faces would {how} another face {}",
            near_edge(m, e)
        )));
    }
    if let Some(p) = contact::new_distance_problem(before, m, &made.edges, &made.verts) {
        return Err(invalid(plain(m, &p)));
    }
    // No new edge shorter than the length tolerance.
    let tol = geom::tol(&[]);
    for e in &made.edges {
        if let Some(ed) = m.edges.get(e)
            && m.vertices[&ed.v[0]].distance(m.vertices[&ed.v[1]]) <= tol
        {
            return Err(invalid(format!(
                "an edge would be shorter than the length tolerance (0.01 mm) {}",
                near_edge(m, *e)
            )));
        }
    }
    let mut own: BTreeSet<FaceId> = shell.clone();
    own.extend(made.faces.iter().copied());
    if let Some((x, g)) = near_touch(m, made, 0.5 * tol) {
        let whom = if own.contains(&g) {
            "the solid would touch itself"
        } else {
            "the new faces would touch another solid"
        };
        return Err(invalid(format!("{whom} {}", near(x))));
    }
    let mut set: Vec<FaceId> = shell
        .iter()
        .copied()
        .filter(|f| m.faces.contains_key(f))
        .collect();
    set.extend(made.faces.iter().copied());
    let state = m
        .solid_state(&set)
        .ok_or_else(|| invalid("faces of the solid would be missing".into()))?;
    if !state.is_solid() {
        let why = match (state.open_edges.first(), state.crossings.first()) {
            (Some(o), _) => format!("the solid would be open {}", near_edge(m, o.edge)),
            (None, Some((e, _))) => {
                format!("the solid would pass through itself {}", near_edge(m, *e))
            }
            _ => "it would not close a volume".to_string(),
        };
        return Err(invalid(why));
    }
    // A concave round adds material: no other solid of the model may end up inside it.
    if plan.concave_bound > 0.0 {
        let now: Vec<FaceId> = own
            .iter()
            .copied()
            .filter(|f| m.faces.contains_key(f))
            .collect();
        let was: Vec<FaceId> = shell.iter().copied().collect();
        for other in solid::quick_shells(before) {
            if other.faces.iter().any(|f| shell.contains(f)) {
                continue;
            }
            let Some(p) = other
                .faces
                .iter()
                .find_map(|f| m.faces.get(f).and_then(|g| g.outer.first()))
                .and_then(|v| m.vertices.get(v).copied())
            else {
                continue;
            };
            if solid::inside(m, &now, p) == Some(true)
                && solid::inside(before, &was, p) != Some(true)
            {
                return Err(invalid(format!(
                    "the round would swallow another solid {}",
                    near(p)
                )));
            }
        }
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

/// "near (x, y, z)": the middle of an edge, in local mm with one decimal.
fn near_edge(m: &SurfaceModel, e: EdgeId) -> String {
    match m.edges.get(&e) {
        Some(ed) => near(m.vertices[&ed.v[0]].midpoint(m.vertices[&ed.v[1]])),
        None => String::new(),
    }
}

fn near(p: DVec3) -> String {
    format!("near ({:.1}, {:.1}, {:.1})", p.x, p.y, p.z)
}

/// A problem of the model in plain words, with the place and without ids.
fn plain(m: &SurfaceModel, p: &check::Problem) -> String {
    use check::Problem as P;
    let what = match p {
        P::OffPlane(..) | P::BadNormal(_) => "a face would not be flat",
        P::CloseVertices(..) | P::VertexOnEdge(..) | P::Crossing(..) | P::ShortEdge(_) => {
            "new edges would come closer to each other than the length tolerance (0.01 mm)"
        }
        P::Overlap(..) => "two faces in one plane would overlap",
        P::OuterSide(_) | P::HoleSide(_) | P::HoleOutside(_) | P::HoleInHole(_) => {
            "the outline of a face would turn around"
        }
        _ => "the model would not be valid",
    };
    match check::place(m, p) {
        Some(x) => format!("{what} {}", near(x)),
        None => what.to_string(),
    }
}

/// The first place where a vertex comes closer than `close` to a face that it is not a corner of,
/// for the new vertices and for the vertices near the new and changed faces. The place and the
/// face.
fn near_touch(m: &SurfaceModel, made: &Made, close: f64) -> Option<(DVec3, FaceId)> {
    let mut fresh: BTreeSet<FaceId> = made.faces.clone();
    fresh.extend(made.changed.iter().copied());
    let faces = clash::face_tree(m, close, &|_| true);
    let verts = BoxTree::new(m.vertices.iter().map(|(v, p)| (*p, *p, v.0)).collect());
    let mut tris: BTreeMap<FaceId, Option<FaceTriangles>> = BTreeMap::new();
    let mut gap = |v: VertexId, g: FaceId| -> Option<f64> {
        let f = m.faces.get(&g)?;
        if f.loops().flatten().any(|x| *x == v) {
            return None;
        }
        let p = m.vertices[&v];
        if f.plane.distance(p).abs() >= close {
            return None;
        }
        let t = tris
            .entry(g)
            .or_insert_with(|| m.triangulate_face(g).ok())
            .as_ref()?;
        t.triangles
            .iter()
            .map(|k| {
                let q = [
                    t.positions[k[0] as usize],
                    t.positions[k[1] as usize],
                    t.positions[k[2] as usize],
                ];
                solid::closest_on_triangle(p, &q).distance(p)
            })
            .reduce(f64::min)
    };
    let pad = DVec3::splat(close);
    let mut pairs: Vec<(VertexId, FaceId)> = Vec::new();
    for v in &made.verts {
        let Some(p) = m.vertices.get(v) else { continue };
        faces.query(*p - pad, *p + pad, &mut |g| pairs.push((*v, FaceId(g))));
    }
    for g in &fresh {
        let Some(f) = m.faces.get(g) else { continue };
        let (lo, hi) = clash::face_box(m, f, close);
        verts.query(lo, hi, &mut |v| pairs.push((VertexId(v), *g)));
    }
    pairs.sort();
    pairs.dedup();
    for (v, g) in pairs {
        if gap(v, g).is_some_and(|d| d < close) {
            return Some((m.vertices[&v], g));
        }
    }
    None
}
