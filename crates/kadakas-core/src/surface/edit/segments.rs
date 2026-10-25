//! Set curve segments (MOD-15): rebuilds the chain of a curve that has an exact circle with
//! another number of segments, on the same circle.
//!
//! The ends of an arc and the first vertex of a circle stay where they are. The interior
//! vertices are new (at equal angles), the first edges of the chain keep their ids, the extra
//! edges go or are new. An edge that stays keeps its own attributes (soft, smooth, faceted). A new
//! edge gets the attributes of the first edge of the chain. The curve keeps its id and its description.
//!
//! **What is supported.** A vertex inside the chain has only the two chain edges. A face that
//! uses the chain lies in the plane of the circle, and each of its loops runs once along the whole
//! chain: the loop gets the new vertices. So a loose circle or arc, a flat disc, a disc with
//! holes, a ring and a D shape are supported. **What is refused**, with a typed reason
//! ([`CurveError`]): an edge that meets the chain inside (`VertexShared`), a face in another plane
//! on the chain such as the wall of a pushed cylinder (`BordersOtherPlane`), a loop that does not
//! run along the whole chain (`LoopUnclear`), and a result that the model checks refuse
//! (`Invalid`: edges that become shorter than the length tolerance, or crossings).

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;

use super::super::analytic::{self, CurveError};
use super::super::arrange::canonical;
use super::super::{
    CurveId, Edge, EdgeId, FaceId, SurfaceError, SurfaceModel, VertexId, check, geom,
};
use super::pushpull;

/// Fewest segments.
pub const MIN_SEGMENTS: u32 = 3;
/// Most segments.
pub const MAX_SEGMENTS: u32 = 1000;

fn refuse(e: CurveError) -> SurfaceError {
    SurfaceError::Curve(e)
}

pub(super) fn set_curve_segments(
    m: &mut SurfaceModel,
    before: &SurfaceModel,
    curve: CurveId,
    segments: u32,
) -> Result<(), SurfaceError> {
    if !m.curves.contains_key(&curve) {
        return Err(refuse(CurveError::NoCurve(curve)));
    }
    let Some(a) = m.analytic.get(&curve).cloned() else {
        return Err(refuse(CurveError::NoDescription(curve)));
    };
    if !analytic::is_known_kind(&a.kind) {
        return Err(refuse(CurveError::UnknownKind(curve)));
    }
    if !(MIN_SEGMENTS..=MAX_SEGMENTS).contains(&segments) {
        return Err(refuse(CurveError::Segments {
            segments,
            min: MIN_SEGMENTS,
            max: MAX_SEGMENTS,
        }));
    }
    let es = m.curves[&curve].clone();
    let vs = analytic::chain_vertices(m, &es).ok_or(refuse(CurveError::NoDescription(curve)))?;
    let n = es.len();
    let closed = vs.len() >= 4 && vs[0] == vs[n];
    let pts: Vec<DVec3> = vs.iter().map(|v| m.vertices[v]).collect();
    let target = segments as usize;
    if target == n {
        return Ok(());
    }

    // The new vertices on the circle. The direction and the span of an arc come from the stored
    // angles (the chain of one edge cannot tell an arc of more than half a turn from the other
    // side); the chain starts at the end that lies at the start angle, else at the end angle and
    // goes clockwise.
    let (a0, dir, span) = match (closed, a.start, a.end, a.sweep()) {
        (false, Some(s), Some(e), Some(sweep)) => {
            let tol = geom::tol(&pts);
            if a.point_at(s).distance(pts[0]) <= tol {
                (s, 1.0, sweep)
            } else {
                (e, -1.0, sweep)
            }
        }
        _ => {
            let a0 = a.angle_of(pts[0]);
            let dir = analytic::chain_direction(&a, &pts);
            let span = if closed {
                std::f64::consts::TAU
            } else {
                let end = a.angle_of(pts[n]);
                let w = if dir > 0.0 { end - a0 } else { a0 - end };
                let w = w.rem_euclid(std::f64::consts::TAU);
                if w == 0.0 { std::f64::consts::TAU } else { w }
            };
            (a0, dir, span)
        }
    };
    let fresh: Vec<DVec3> = (1..target)
        .map(|k| a.point_at(a0 + dir * span * k as f64 / target as f64))
        .collect();

    // The limits of the model, with a reason of their own.
    let vc = (m.vertices.len() + target).saturating_sub(n);
    let ecount = (m.edges.len() + target).saturating_sub(n);
    if vc > super::super::MAX_VERTICES {
        return Err(SurfaceError::Limit {
            what: "vertices",
            max: super::super::MAX_VERTICES,
            count: vc,
        });
    }
    if ecount > super::super::MAX_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges",
            max: super::super::MAX_EDGES,
            count: ecount,
        });
    }
    // Faces that use the chain: in the plane of the circle, with loops that run along it.
    let chain_pairs: BTreeSet<(VertexId, VertexId)> = es
        .iter()
        .map(|e| {
            let v = m.edges[e].v;
            (v[0].min(v[1]), v[0].max(v[1]))
        })
        .collect();
    let plane_tol = geom::plane_tol(&pts);
    let new_ids: Vec<VertexId> = (1..target).map(|_| VertexId(m.alloc())).collect();
    let mut replaced: Vec<(FaceId, Vec<Vec<VertexId>>)> = Vec::new();
    for (&fid, f) in &m.faces {
        let uses = |l: &Vec<VertexId>| {
            (0..l.len()).any(|i| {
                let (x, y) = (l[i], l[(i + 1) % l.len()]);
                chain_pairs.contains(&(x.min(y), x.max(y)))
            })
        };
        if !f.loops().any(uses) {
            continue;
        }
        if fresh
            .iter()
            .chain(&pts)
            .any(|p| f.plane.distance(*p).abs() > plane_tol)
        {
            return Err(refuse(CurveError::BordersOtherPlane { face: fid }));
        }
        let mut loops = Vec::new();
        for l in f.loops() {
            if uses(l) {
                let new = rebuild_loop(l, &vs, closed, &new_ids)
                    .ok_or(refuse(CurveError::LoopUnclear { face: fid }))?;
                loops.push(new);
            } else {
                loops.push(l.clone());
            }
        }
        replaced.push((fid, loops));
    }

    // Only the two chain edges meet a vertex inside the chain.
    let interior: Vec<VertexId> = vs[1..n].to_vec();
    let inside: BTreeSet<VertexId> = interior.iter().copied().collect();
    let mut degree: BTreeMap<VertexId, usize> = BTreeMap::new();
    for e in m.edges.values() {
        for v in e.v {
            if inside.contains(&v) {
                *degree.entry(v).or_default() += 1;
            }
        }
    }
    if let Some(v) = interior.iter().find(|v| degree.get(v) != Some(&2)) {
        return Err(refuse(CurveError::VertexShared { vertex: *v }));
    }

    // The new chain.
    for (&id, p) in new_ids.iter().zip(&fresh) {
        m.vertices.insert(id, *p);
    }
    for v in &interior {
        m.vertices.remove(v);
    }
    let mut ring: Vec<VertexId> = vec![vs[0]];
    ring.extend(&new_ids);
    if !closed {
        ring.push(vs[n]);
    }
    let attrs = m.edge_attrs.get(&es[0]).cloned();
    let mut chain = Vec::with_capacity(target);
    for k in 0..target {
        let v = [ring[k], ring[(k + 1) % ring.len()]];
        let id = match es.get(k) {
            Some(id) => *id,
            None => {
                let id = EdgeId(m.alloc());
                if let Some(at) = &attrs {
                    m.edge_attrs.insert(id, at.clone());
                }
                id
            }
        };
        m.edges.insert(id, Edge { v });
        chain.push(id);
    }
    for e in es.iter().skip(target) {
        m.edges.remove(e);
        m.edge_attrs.remove(e);
    }
    m.curves.insert(curve, chain);
    for (fid, loops) in replaced {
        let f = m.faces.get_mut(&fid).expect("a face");
        let mut it = loops.into_iter();
        f.outer = canonical(&it.next().expect("outer loop"));
        let mut holes: Vec<Vec<VertexId>> = it.map(|l| canonical(&l)).collect();
        holes.sort();
        f.holes = holes;
    }

    // The same checks as the other edits that change many edges.
    let touched_v: BTreeSet<VertexId> = ring.iter().copied().collect();
    let touched_e: BTreeSet<EdgeId> = m.curves[&curve].iter().copied().collect();
    let bad = |p: &check::Problem| {
        refuse(CurveError::Invalid {
            reason: p.to_string(),
        })
    };
    let after = check::structure_problems(m);
    if !after.is_empty() {
        let old = check::structure_problems(before);
        if let Some(p) = after.difference(&old).next() {
            return Err(bad(p));
        }
    }
    if let Some(p) = pushpull::new_distance_problem(before, m, &touched_e, &touched_v) {
        return Err(bad(&p));
    }
    Ok(())
}

/// The loop with the interior vertices of the chain `vs` replaced by `fresh` (in chain order), or
/// `None` if the loop does not run along the whole chain once.
fn rebuild_loop(
    l: &[VertexId],
    vs: &[VertexId],
    closed: bool,
    fresh: &[VertexId],
) -> Option<Vec<VertexId>> {
    let len = l.len();
    let at = |x: VertexId, y: VertexId| (0..len).find(|&i| l[i] == x && l[(i + 1) % len] == y);
    let forward = at(vs[0], vs[1]);
    let reverse = at(vs[1], vs[0]);
    let (mut seq, flip): (Vec<VertexId>, bool) = match (forward, reverse) {
        (Some(i), None) => (l[i..].iter().chain(&l[..i]).copied().collect(), false),
        (None, Some(_)) => {
            let r: Vec<VertexId> = l.iter().rev().copied().collect();
            let i = (0..len).find(|&i| r[i] == vs[0] && r[(i + 1) % len] == vs[1])?;
            (r[i..].iter().chain(&r[..i]).copied().collect(), true)
        }
        _ => return None,
    };
    let n = vs.len() - 1;
    let run = if closed { n } else { n + 1 };
    if seq.len() < run || seq[..run] != vs[..run] {
        return None;
    }
    let mut out: Vec<VertexId> = vec![vs[0]];
    out.extend(fresh);
    if !closed {
        out.push(vs[n]);
    }
    out.extend(seq.drain(run..));
    if flip {
        out.reverse();
    }
    Some(out)
}
