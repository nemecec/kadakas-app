//! Offset (MOD-08): a copy of the loops of a face at a distance, in the plane of the face.
//!
//! Every side of every loop of the face moves by `distance` to the inside of the face (`outward`
//! false) or to the outside (`outward` true). The outer loop shrinks or grows and the holes grow
//! or shrink with it. Two neighbour sides meet at the crossing of their moved lines (a miter
//! join, no rounded corners), so a concave corner works as well as a convex one. The new loops are
//! drawn as edges in the plane of the face (with the rules of `add_edges`): an inward offset
//! splits the face into a ring along each loop and the face inside (the pieces keep the
//! attributes of the face, the largest keeps its id); an outward offset splits the coplanar face
//! around it where there is one, or adds a ring face in the empty plane around the face. Faces in
//! other planes are not cut (as in other modelers; the check warns where a new edge passes
//! through one).
//!
//! A run of sides of one curve (an arc or a circle, see `add_curve`) gives a run of new edges that
//! is one curve again, so the offset of a circle is a circle.
//!
//! Refused with a reason, and the model unchanged: a distance that is not finite or not larger
//! than the length tolerance (a negative or zero distance; the direction is `outward`), and an
//! offset that does not fit: a new side would turn around or vanish (an inward offset larger than
//! the face allows, the hole that would close), a new loop would cross itself or another new loop,
//! or a loop would leave the area of the other loops. The message gives the largest distance that
//! fits (found by halving the interval), so the person can type it. A loop with a turn of 180
//! degrees (a spike) has no offset.
//!
//! Ids: the new vertices and edges are made loop by loop (the outer loop, then the holes in their
//! order), side by side from the first vertex of each loop, so the same edit gives the same ids.

use super::*;

/// Offsets the loops of `face` by `distance` mm, inward or outward.
pub(super) fn offset_face(
    m: &mut SurfaceModel,
    face: FaceId,
    distance: f64,
    outward: bool,
) -> Result<(), SurfaceError> {
    let moved = outline(m, face, distance, outward)?;
    let f = &m.faces[&face];
    let lines: Vec<(Vec<VertexId>, Vec<DVec3>)> = f.loops().cloned().zip(moved).collect();
    draw(m, &lines, true)
}

/// The new loops of an offset of `face` (local coordinates, outer loop first), or the refusal.
pub(in crate::surface) fn outline(
    m: &SurfaceModel,
    face: FaceId,
    distance: f64,
    outward: bool,
) -> Result<Vec<Vec<DVec3>>, SurfaceError> {
    let f = m.faces.get(&face).ok_or(SurfaceError::NoFace(face))?;
    let loops: Vec<Vec<DVec3>> = f.loops().map(|l| m.loop_points(l)).collect();
    check_distance(&loops, distance)?;
    let s = if outward { -distance } else { distance };
    fitted(&loops, f.plane, s, true, &format!("face {face}"))
}

/// Offset Edges (MOD-08): a chain of edges in one plane (open, or closed as a loop) offset by
/// `distance` mm to the side of the point `toward`. The plane is that of the chain (a straight
/// chain lies in the plane of a face it bounds, else in the horizontal plane). The ends of an open
/// chain move at a right angle to the end sides.
pub(super) fn offset_edges(
    m: &mut SurfaceModel,
    edges: &[EdgeId],
    distance: f64,
    toward: DVec3,
) -> Result<(), SurfaceError> {
    if let Some(e) = edges.iter().find(|e| !m.edges.contains_key(e)) {
        return Err(SurfaceError::NoEdge(*e));
    }
    if !toward.is_finite() {
        return Err(SurfaceError::BadPoint);
    }
    let (chain, closed, moved) = chain_line(m, edges, distance, toward)?;
    let lines = vec![(chain, moved)];
    draw(m, &lines, closed)
}

/// The new line of an offset of a chain of edges (local coordinates), with the chain and whether
/// it is closed, or the refusal.
pub(in crate::surface) fn chain_line(
    m: &SurfaceModel,
    edges: &[EdgeId],
    distance: f64,
    toward: DVec3,
) -> Result<(Vec<VertexId>, bool, Vec<DVec3>), SurfaceError> {
    if let Some(e) = edges.iter().find(|e| !m.edges.contains_key(e)) {
        return Err(SurfaceError::NoEdge(*e));
    }
    if !toward.is_finite() {
        return Err(SurfaceError::BadPoint);
    }
    let (chain, closed) = chain_of(m, edges)?;
    let pts = m.loop_points(&chain);
    check_distance(std::slice::from_ref(&pts), distance)?;
    let plane = chain_plane(m, edges, &pts)?;
    let n = plane.normal;
    // The side of `toward`: the side of the nearest side of the chain.
    let k = pts.len();
    let count = if closed { k } else { k - 1 };
    let (_, i) = (0..count)
        .map(|i| (geom::point_segment(toward, pts[i], pts[(i + 1) % k]).0, i))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("a chain has a side");
    let (a, b) = (pts[i], pts[(i + 1) % k]);
    let left = n.cross(b - a).normalize_or_zero().dot(toward - a);
    if left.abs() <= geom::tol(&[a, b, toward]) {
        return Err(SurfaceError::Refused(
            "the point that gives the side of the offset lies on the edges".into(),
        ));
    }
    let s = distance * left.signum();
    let moved = fitted(std::slice::from_ref(&pts), plane, s, closed, "the edges")?;
    Ok((chain, closed, moved.into_iter().next().expect("one line")))
}

/// Refuses a distance that is not finite or not larger than the length tolerance.
fn check_distance(loops: &[Vec<DVec3>], distance: f64) -> Result<(), SurfaceError> {
    let all: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let tol = geom::tol(&all);
    if !distance.is_finite() {
        return Err(SurfaceError::BadPoint);
    }
    if distance <= tol {
        return Err(SurfaceError::Refused(format!(
            "the offset distance must be larger than the length tolerance ({tol} mm); choose \
             inward or outward for the direction"
        )));
    }
    Ok(())
}

/// The lines moved by `s`, or the refusal with the largest distance that fits.
fn fitted(
    loops: &[Vec<DVec3>],
    plane: Plane,
    s: f64,
    closed: bool,
    what: &str,
) -> Result<Vec<Vec<DVec3>>, SurfaceError> {
    let n = plane.normal;
    let all: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let tol = geom::tol(&all);
    let moved = match offset_loops(loops, n, &plane, s, closed) {
        Ok(l) => l,
        Err(why) => {
            let most = largest_fit(loops, n, &plane, s.signum(), s.abs(), closed);
            let fits = if most > tol {
                format!("at most about {} mm fits", (most * 100.0).floor() / 100.0)
            } else {
                "there is no room for an offset that way".to_string()
            };
            return Err(SurfaceError::Refused(format!(
                "the offset of {} mm does not fit {what}: {why}; {fits}",
                s.abs()
            )));
        }
    };
    let count: usize = moved.iter().map(Vec::len).sum();
    if count > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit",
            max: MAX_EDIT_EDGES,
            count,
        });
    }
    if moved
        .iter()
        .flatten()
        .any(|p| !p.is_finite() || geom::size(*p) > MAX_COORD)
    {
        return Err(SurfaceError::BadPoint);
    }
    Ok(moved)
}

/// The edges as one chain of vertices in order, and whether it is closed. An open chain starts
/// at its end with the lower id; a closed one at its lowest vertex, toward the edge with the lower
/// id.
pub(super) fn chain_of(
    m: &SurfaceModel,
    edges: &[EdgeId],
) -> Result<(Vec<VertexId>, bool), SurfaceError> {
    let ids: BTreeSet<EdgeId> = edges.iter().copied().collect();
    let not_chain = || {
        SurfaceError::Refused(
            "the edges to offset must be one connected chain without branches".into(),
        )
    };
    if ids.is_empty() {
        return Err(not_chain());
    }
    let mut at: BTreeMap<VertexId, Vec<(EdgeId, VertexId)>> = BTreeMap::new();
    for e in &ids {
        let ed = m.edges[e];
        at.entry(ed.v[0]).or_default().push((*e, ed.v[1]));
        at.entry(ed.v[1]).or_default().push((*e, ed.v[0]));
    }
    if at.values().any(|v| v.len() > 2) {
        return Err(not_chain());
    }
    let open = at.iter().find(|(_, v)| v.len() == 1).map(|(k, _)| *k);
    let first = open.unwrap_or(*at.keys().next().expect("an edge"));
    let mut out = vec![first];
    let mut used: BTreeSet<EdgeId> = BTreeSet::new();
    let mut cur = first;
    loop {
        let Some(&(e, o)) = at[&cur].iter().filter(|(e, _)| !used.contains(e)).min() else {
            break;
        };
        used.insert(e);
        if o == first {
            break;
        }
        out.push(o);
        cur = o;
    }
    if used.len() != ids.len() {
        return Err(not_chain());
    }
    Ok((out, open.is_none()))
}

/// The plane of a chain of points: through its points; a straight chain lies in the plane of a
/// face of one of its edges, else in the horizontal plane (a vertical straight chain is refused).
/// It faces up (+Z; a vertical plane +Y, then +X).
fn chain_plane(m: &SurfaceModel, edges: &[EdgeId], pts: &[DVec3]) -> Result<Plane, SurfaceError> {
    let p0 = pts[0];
    let tol = geom::tol(pts);
    let far = pts
        .iter()
        .copied()
        .max_by(|a, b| a.distance(p0).total_cmp(&b.distance(p0)))
        .expect("points");
    let dir = (far - p0).normalize_or_zero();
    let off = pts
        .iter()
        .copied()
        .map(|p| (geom::point_line(p, p0, far), p))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .expect("points");
    let normal = if off.0 > tol {
        dir.cross(off.1 - p0).normalize_or_zero()
    } else {
        let from_face = edges
            .iter()
            .find_map(|e| m.faces_of_edge(*e).first().map(|f| m.faces[f].plane.normal));
        match from_face {
            Some(n) => n,
            None if dir.z.abs() < 1.0 - 1e-9 => DVec3::Z,
            None => {
                return Err(SurfaceError::Refused(
                    "a vertical straight line has no plane to offset in; offset a face instead"
                        .into(),
                ));
            }
        }
    };
    let plane = Plane::new(p0, normal).ok_or(SurfaceError::TooShort { tol })?;
    if !pts.iter().all(|p| on_plane(&plane, *p)) {
        return Err(SurfaceError::Refused(
            "the edges to offset are not in one plane".into(),
        ));
    }
    Ok(upward(&plane))
}

/// Draws the moved lines (each the vertices of the line it comes from and the new points) with the
/// rules of `add_edges`, and makes a curve of each run of new edges whose sides were one curve.
fn draw(
    m: &mut SurfaceModel,
    lines: &[(Vec<VertexId>, Vec<DVec3>)],
    closed: bool,
) -> Result<(), SurfaceError> {
    // The curve of each side, for the runs of new edges that are curves again.
    let curve_of: BTreeMap<(VertexId, VertexId), super::super::CurveId> = m
        .curves
        .iter()
        .flat_map(|(c, es)| es.iter().map(move |e| (*c, *e)))
        .filter_map(|(c, e)| m.edges.get(&e).map(|ed| (ed.key(), c)))
        .collect();
    let mut w = Work::indexed(m);
    let mut drawn = Vec::new();
    let mut touched = BTreeSet::new();
    // Each run of sides of one curve: the spans of its new edges (start and end vertex).
    let mut runs: Vec<Vec<(VertexId, VertexId)>> = Vec::new();
    for (l, pts) in lines {
        let k = l.len();
        let sides = if closed { k } else { k - 1 };
        let side_curve = |i: usize| curve_of.get(&key(l[i], l[(i + 1) % k])).copied();
        // A closed line starts at a side that begins a run (its curve differs from the side
        // before), so that a run never wraps around the start; one that is all one curve starts
        // at 0.
        let start = if closed {
            (0..k)
                .find(|&i| side_curve(i) != side_curve((i + k - 1) % k))
                .unwrap_or(0)
        } else {
            0
        };
        let mut run: Option<(super::super::CurveId, Vec<(VertexId, VertexId)>)> = None;
        for j in 0..sides {
            let i = (start + j) % k;
            let (a, b) = (pts[i], pts[(i + 1) % k]);
            let from = drawn.len();
            w.insert_segment(a, b, &mut drawn, &mut touched)?;
            w.check_limits()?;
            let span = (drawn[from].0, drawn[drawn.len() - 1].1);
            match (side_curve(i), &mut run) {
                (Some(c), Some((rc, spans))) if *rc == c => spans.push(span),
                (c, r) => {
                    if let Some((_, spans)) = r.take() {
                        runs.push(spans);
                    }
                    *r = c.map(|c| (c, vec![span]));
                }
            }
        }
        if let Some((_, spans)) = run {
            runs.push(spans);
        }
    }
    w.solve_after_add(&drawn, &touched)?;
    let used: BTreeSet<EdgeId> = w.m.curves.values().flatten().copied().collect();
    for spans in runs {
        let mut chain: Vec<EdgeId> = Vec::new();
        let mut ok = true;
        for (x, y) in spans {
            match w.chain_between(x, y) {
                Some(es) => chain.extend(es),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        let unique: BTreeSet<EdgeId> = chain.iter().copied().collect();
        if ok
            && !chain.is_empty()
            && unique.len() == chain.len()
            && chain.iter().all(|e| !used.contains(e))
        {
            let id = super::super::CurveId(w.m.alloc());
            w.m.curves.insert(id, chain);
        }
    }
    w.check_limits()
}

/// The largest distance in (0, `upto`] in the direction `sign` (+1 inward) that fits, by halving.
fn largest_fit(
    loops: &[Vec<DVec3>],
    n: DVec3,
    plane: &Plane,
    sign: f64,
    upto: f64,
    closed: bool,
) -> f64 {
    let (mut lo, mut hi) = (0.0, upto);
    for _ in 0..48 {
        let mid = (lo + hi) / 2.0;
        if offset_loops(loops, n, plane, sign * mid, closed).is_ok() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The loops moved by `s` to the left of each side (seen from the front, the inside of the face;
/// a negative `s` moves to the right), or the reason why they do not fit.
pub(super) fn offset_loops(
    loops: &[Vec<DVec3>],
    n: DVec3,
    plane: &Plane,
    s: f64,
    closed: bool,
) -> Result<Vec<Vec<DVec3>>, String> {
    let all: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let tol = geom::tol(&all);
    let mut out: Vec<Vec<DVec3>> = Vec::with_capacity(loops.len());
    for l in loops {
        out.push(offset_ring(l, n, s, closed)?);
    }
    let sides_of = |k: usize| if closed { k } else { k - 1 };
    // Each new side keeps its direction and a length.
    for (l, o) in loops.iter().zip(&out) {
        let k = l.len();
        for i in 0..sides_of(k) {
            let (a, b) = (l[i], l[(i + 1) % k]);
            let (c, d) = (o[i], o[(i + 1) % k]);
            if (d - c).dot(b - a) <= 0.0 || c.distance(d) <= tol {
                return Err("a side would turn around or vanish".into());
            }
        }
    }
    // No new side meets another one, other than its neighbours in its loop.
    let rings: Vec<Vec<DVec2>> = out
        .iter()
        .map(|o| o.iter().map(|p| plane.project(*p)).collect())
        .collect();
    let mut items: Vec<(DVec3, DVec3, u32)> = Vec::new();
    let mut sides: Vec<(usize, usize)> = Vec::new();
    for (li, r) in rings.iter().enumerate() {
        for i in 0..sides_of(r.len()) {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            let pad = DVec2::splat(tol);
            items.push((
                (a.min(b) - pad).extend(0.0),
                (a.max(b) + pad).extend(0.0),
                u32::try_from(sides.len()).unwrap_or(u32::MAX),
            ));
            sides.push((li, i));
        }
    }
    let tree = BoxTree::new(items.clone());
    let seg = |(li, i): (usize, usize)| {
        let r = &rings[li];
        (r[i].extend(0.0), r[(i + 1) % r.len()].extend(0.0))
    };
    for (k, (lo, hi, _)) in items.iter().enumerate() {
        let (li, i) = sides[k];
        let len = rings[li].len();
        let (a, b) = seg((li, i));
        let mut bad = false;
        tree.query(*lo, *hi, &mut |x| {
            let x = x as usize;
            if x <= k || bad {
                return;
            }
            let (lj, j) = sides[x];
            if lj == li && ((closed && (j == (i + 1) % len || i == (j + 1) % len)) || j == i + 1) {
                return;
            }
            let (c, d) = seg((lj, j));
            let dist = match geom::segment_segment(a, b, c, d) {
                Some((_, _, dist)) => dist,
                None => [
                    geom::point_segment(a, c, d).0,
                    geom::point_segment(b, c, d).0,
                    geom::point_segment(c, a, b).0,
                    geom::point_segment(d, a, b).0,
                ]
                .into_iter()
                .fold(f64::INFINITY, f64::min),
            };
            bad = dist <= tol;
        });
        if bad {
            return Err("a new loop would cross itself or another new loop".into());
        }
    }
    // The holes stay inside the outer loop and outside each other.
    if let Some((outer, holes)) = rings.split_first().filter(|_| closed) {
        let outer_ring = [outer.clone()];
        for (hi, h) in holes.iter().enumerate() {
            if !super::arrange::inside(h[0], &outer_ring) {
                return Err("a hole would leave the face".into());
            }
            if super::arrange::inside(outer[0], &[h.clone()]) {
                return Err("a hole would swallow the face".into());
            }
            for (hj, g) in holes.iter().enumerate() {
                if hi != hj && super::arrange::inside(g[0], &[h.clone()]) {
                    return Err("a hole would swallow another hole".into());
                }
            }
        }
    }
    Ok(out)
}

/// One loop moved by `s` to the left of each side (`closed`), with miter joins: the new corner is
/// where the two moved neighbour sides cross.
pub(super) fn offset_ring(
    l: &[DVec3],
    n: DVec3,
    s: f64,
    closed: bool,
) -> Result<Vec<DVec3>, String> {
    let k = l.len();
    if k < 2 {
        return Err("a loop needs two points".into());
    }
    let left = |a: DVec3, b: DVec3| n.cross(b - a).try_normalize();
    let mut out = Vec::with_capacity(k);
    for i in 0..k {
        let prev = if closed || i > 0 {
            Some((l[(i + k - 1) % k], l[i]))
        } else {
            None
        };
        let next = if closed || i + 1 < k {
            Some((l[i], l[(i + 1) % k]))
        } else {
            None
        };
        let l0 = prev.map(|(a, b)| left(a, b));
        let l1 = next.map(|(a, b)| left(a, b));
        let dir = match (l0, l1) {
            (Some(Some(a)), Some(Some(b))) => {
                let den = 1.0 + a.dot(b);
                if den <= 1e-9 {
                    return Err("a loop turns back on itself (a spike)".into());
                }
                (a + b) / den
            }
            (Some(Some(a)), None) | (None, Some(Some(a))) => a,
            _ => return Err("a side has no length".into()),
        };
        out.push(l[i] + dir * s);
    }
    Ok(out)
}
