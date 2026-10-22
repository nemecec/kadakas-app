//! Follow Me (MOD-08): a face (the profile) swept along a path of connected edges.
//!
//! # How the sweep goes
//!
//! The path is a chain of edges (open, or closed as a loop), found as in Offset Edges. An open
//! path starts at its end nearer to the middle of the profile; a closed one at its vertex nearest
//! to it. The profile moves along the first side of the path to the plane at the next corner,
//! along the next side to the plane after it, and so on. The plane at a corner halves the angle
//! between the two sides (a miter), and the plane at the end of an open path is at a right angle
//! to the last side. Each point of the profile moves along a side of the path, so each side face
//! of the sweep (one for each side of the profile and each side of the path) is flat by
//! construction, and a profile at any angle to the path works; the frame of the profile turns
//! with the path at each corner (the miter keeps the distances to the path in a flat path).
//!
//! - An **open path** gives a solid with caps: the profile stays in its place as the first cap
//!   (turned to look away from the sweep), and a new face at the end of the path is the second.
//! - A **closed path** gives a ring without caps (a torus-like solid): the profile is moved to
//!   the plane of the corner where the sweep starts (when it is not there, the note says so) and
//!   the profile face goes, because the sweep closes on itself. The path must bring the profile
//!   back to where it started (a flat path does); else the sweep is refused.
//!
//! A point of the profile that does not move from one corner to the next (it lies on the axis of
//! a turn, like the axis of a vase on a circle) stays one vertex: its side faces are triangles.
//!
//! # Look and flags
//!
//! The new faces get the paint of the profile (as the walls of Push/Pull). Where two sides of the
//! path are in one curve (an arc or a circle), the copy of the profile between them is soft and
//! smooth, and where two sides of the profile are in one curve, the edge that their common vertex
//! leaves along the path is soft and smooth: a profile swept along a circle shades round. A curve
//! of the profile gives a curve at each copy, and a curve of the path gives a curve along each
//! point of the profile.
//!
//! # Refused (the model unchanged)
//!
//! - edges that are not one chain without branches, a path that turns back on itself (a corner
//!   of 180 degrees), a face in the path;
//! - a profile whose plane holds the first side of the path (it would sweep into no area), and a
//!   side of the profile that runs along the path;
//! - more than [`MAX_EDIT_EDGES`] new edges or a model over a limit (the message gives the count:
//!   profile sides x path sides faces);
//! - a result with a problem that the model did not have before (the fast checks and the
//!   distance checks of the new geometry), such as a sweep that runs into other geometry within
//!   the tolerance. A sweep that passes through itself or other faces without meeting them in an
//!   edge is accepted with the warning `NotIntersected` of the check, as other edits are.
//!
//! Ids: the vertices of each copy of the profile in the order of its loops, copy by copy along the
//! path, then the edges of each copy and the edges along the path, then the faces, the end cap
//! last; the same edit always gives the same ids.

use super::super::CurveId;
use super::super::check;
use super::*;

/// Sweeps `face` along the chain of `path` edges.
pub(super) fn follow_me(
    m: &mut SurfaceModel,
    before: &SurfaceModel,
    face: FaceId,
    path: &[EdgeId],
    notes: &mut Vec<String>,
) -> Result<(), SurfaceError> {
    let f = m
        .faces
        .get(&face)
        .cloned()
        .ok_or(SurfaceError::NoFace(face))?;
    if let Some(e) = path.iter().find(|e| !m.edges.contains_key(e)) {
        return Err(SurfaceError::NoEdge(*e));
    }
    let (mut chain, closed) = offset::chain_of(m, path).map_err(|_| {
        SurfaceError::Refused(
            "the path of Follow Me must be one connected chain of edges without branches".into(),
        )
    })?;
    let profile_pts: Vec<DVec3> = f.loops().flatten().map(|v| m.vertices[v]).collect();
    let mid = profile_pts.iter().copied().sum::<DVec3>() / profile_pts.len() as f64;
    let pos = |v: &VertexId| m.vertices[v];
    // The start of the path: nearer to the profile.
    if closed {
        let i = (0..chain.len())
            .min_by(|&a, &b| {
                pos(&chain[a])
                    .distance(mid)
                    .total_cmp(&pos(&chain[b]).distance(mid))
                    .then(a.cmp(&b))
            })
            .expect("a chain");
        chain.rotate_left(i);
    } else if pos(chain.last().expect("a chain")).distance(mid) < pos(&chain[0]).distance(mid) {
        chain.reverse();
    }
    let k = chain.len();
    let sides = if closed { k } else { k - 1 };
    let p: Vec<DVec3> = chain.iter().map(pos).collect();
    let dirs: Vec<DVec3> = (0..sides)
        .map(|i| (p[(i + 1) % k] - p[i]).normalize_or_zero())
        .collect();
    // The plane at each corner (`planes[j]` through `p[j]`), for the rings 1..: a miter, and at the
    // end of an open path a right angle to the last side.
    let corner = |j: usize| -> Result<DVec3, SurfaceError> {
        let (a, b) = if closed {
            (dirs[(j + sides - 1) % sides], dirs[j % sides])
        } else if j == 0 {
            (dirs[0], dirs[0])
        } else if j >= sides {
            (dirs[sides - 1], dirs[sides - 1])
        } else {
            (dirs[j - 1], dirs[j])
        };
        (a + b).try_normalize().ok_or_else(|| {
            SurfaceError::Refused(format!(
                "the path of Follow Me turns back on itself at vertex {}",
                chain[j % k]
            ))
        })
    };
    let n = f.plane.normal;
    if n.dot(dirs[0]).abs() < 1e-6 {
        return Err(SurfaceError::Refused(
            "the profile lies along the path: its plane holds the first side of the path, so it \
             would sweep into no area (draw the profile across the path)"
                .into(),
        ));
    }
    if path.iter().any(|e| {
        let ed = m.edges[e];
        f.uses().any(|(a, b)| key(a, b) == ed.key())
    }) {
        return Err(SurfaceError::Refused(
            "the path of Follow Me may not be an edge of the profile".into(),
        ));
    }
    // The loops of the profile turned to run counterclockwise about the first side of the path.
    let along = n.dot(dirs[0]) > 0.0;
    let loops: Vec<Vec<VertexId>> = f
        .loops()
        .map(|l| if along { l.clone() } else { reversed(l) })
        .collect();
    let loop_pts: Vec<Vec<DVec3>> = loops.iter().map(|l| m.loop_points(l)).collect();
    // A side of the profile along the path has no area.
    for l in &loop_pts {
        for i in 0..l.len() {
            let e = l[(i + 1) % l.len()] - l[i];
            if e.normalize_or_zero().cross(dirs[0]).length() < 1e-9 {
                return Err(SurfaceError::Refused(
                    "a side of the profile runs along the path".into(),
                ));
            }
        }
    }
    // The limits: profile sides x path sides faces.
    let psides: usize = loops.iter().map(Vec::len).sum();
    let new_edges = psides * sides * 2 + psides;
    if new_edges > MAX_EDIT_EDGES {
        return Err(SurfaceError::Limit {
            what: "edges in one edit (Follow Me makes about two edges for each side of the \
                   profile and each side of the path)",
            max: MAX_EDIT_EDGES,
            count: new_edges,
        });
    }
    if psides * sides + 1 > MAX_FACES {
        return Err(SurfaceError::Limit {
            what: "faces",
            max: MAX_FACES,
            count: psides * sides,
        });
    }

    // The rings: the points of the profile at each corner.
    let project = |q: DVec3, d: DVec3, at: DVec3, normal: DVec3| -> DVec3 {
        q + d * ((at - q).dot(normal) / d.dot(normal))
    };
    let mut rings: Vec<Vec<Vec<DVec3>>> = Vec::with_capacity(sides + 1);
    let first: Vec<Vec<DVec3>> = if closed {
        let c0 = corner(0)?;
        loop_pts
            .iter()
            .map(|l| l.iter().map(|q| project(*q, dirs[0], p[0], c0)).collect())
            .collect()
    } else {
        loop_pts.clone()
    };
    rings.push(first);
    for j in 1..=sides {
        let c = corner(j)?;
        let at = p[j % k];
        let d = dirs[j - 1];
        let prev = rings.last().expect("a ring");
        let next: Vec<Vec<DVec3>> = prev
            .iter()
            .map(|l| l.iter().map(|q| project(*q, d, at, c)).collect())
            .collect();
        rings.push(next);
    }
    let all: Vec<DVec3> = rings.iter().flatten().flatten().copied().collect();
    if all
        .iter()
        .any(|q| !q.is_finite() || geom::size(*q) > MAX_COORD)
    {
        return Err(SurfaceError::BadPoint);
    }
    let tol = geom::tol(&all);
    if closed {
        let back = rings[sides]
            .iter()
            .flatten()
            .zip(rings[0].iter().flatten())
            .map(|(a, b)| a.distance(*b))
            .fold(0.0, f64::max);
        if back > tol {
            return Err(SurfaceError::Refused(format!(
                "the closed path does not bring the profile back to where it started (it is \
                 {back:.3} mm off; a flat path does): sweep along an open path instead"
            )));
        }
    }

    let curve_of: BTreeMap<(VertexId, VertexId), CurveId> = m
        .curves
        .iter()
        .flat_map(|(c, es)| es.iter().map(move |e| (*c, *e)))
        .filter_map(|(c, e)| m.edges.get(&e).map(|ed| (ed.key(), c)))
        .collect();
    let path_curve: Vec<Option<CurveId>> = (0..sides)
        .map(|i| curve_of.get(&key(chain[i], chain[(i + 1) % k])).copied())
        .collect();
    let first_ring_moved = closed
        && rings[0]
            .iter()
            .flatten()
            .zip(&loop_pts.iter().flatten().copied().collect::<Vec<_>>())
            .any(|(a, b)| a.distance(*b) > tol);
    let first_moved_note = first_ring_moved;
    // The vertices of the model, so that a copy that lands on one (a profile that touches the
    // path) is that vertex.
    let tree = BoxTree::new(m.vertices.iter().map(|(v, q)| (*q, *q, v.0)).collect());
    let existing = |m: &SurfaceModel, q: DVec3| -> Option<VertexId> {
        let pad = DVec3::splat(tol);
        let mut best: Option<(f64, VertexId)> = None;
        tree.query(q - pad, q + pad, &mut |x| {
            let v = VertexId(x);
            if let Some(o) = m.vertices.get(&v) {
                let d = o.distance(q);
                if d <= tol && best.is_none_or(|(bd, bv)| d < bd || (d == bd && v < bv)) {
                    best = Some((d, v));
                }
            }
        });
        best.map(|(_, v)| v)
    };

    let mut w = Work::new(m);
    // The vertices of each ring, loop by loop. Ring 0 of an open path (and of a closed one whose
    // profile is at the start corner) is the profile itself.
    let mut ids: Vec<Vec<Vec<VertexId>>> = Vec::with_capacity(sides + 1);
    if first_ring_moved {
        let r: Vec<Vec<VertexId>> = rings[0]
            .iter()
            .map(|l| {
                l.iter()
                    .map(|q| match existing(w.m, *q) {
                        Some(v) => v,
                        None => w.new_vertex(*q),
                    })
                    .collect()
            })
            .collect();
        ids.push(r);
    } else {
        ids.push(loops.clone());
    }
    for j in 1..=sides {
        if closed && j == sides {
            ids.push(ids[0].clone());
            continue;
        }
        let prev = ids[j - 1].clone();
        let r: Vec<Vec<VertexId>> = rings[j]
            .iter()
            .zip(&prev)
            .map(|(l, pl)| {
                l.iter()
                    .zip(pl)
                    .map(|(q, pv)| {
                        // A point that does not move stays one vertex; one that lands on a
                        // vertex of the model is that vertex.
                        if w.pos(*pv).distance(*q) <= tol {
                            *pv
                        } else if let Some(v) = existing(w.m, *q) {
                            v
                        } else {
                            w.new_vertex(*q)
                        }
                    })
                    .collect()
            })
            .collect();
        ids.push(r);
    }
    let soft = |w: &mut Work<'_>, e: EdgeId| {
        let at = w.m.edge_attrs.entry(e).or_default();
        at.insert(ATTR_SOFT_KEY.into(), serde_json::Value::Bool(true));
        at.insert(ATTR_SMOOTH_KEY.into(), serde_json::Value::Bool(true));
    };
    let edge = |w: &mut Work<'_>, a: VertexId, b: VertexId| -> Option<EdgeId> {
        if a == b {
            return None;
        }
        Some(match w.t.edge(a, b) {
            Some(e) => e,
            None => w.new_edge(a, b),
        })
    };
    // The edges of each ring (the copies of the profile) and along the path.
    let mut ring_edges: Vec<Vec<Vec<Option<EdgeId>>>> = Vec::with_capacity(sides + 1);
    for j in 0..=sides {
        let mut r = Vec::new();
        for l in &ids[j] {
            let len = l.len();
            let es: Vec<Option<EdgeId>> = (0..len)
                .map(|i| edge(&mut w, l[i], l[(i + 1) % len]))
                .collect();
            r.push(es);
        }
        ring_edges.push(r);
    }
    // At an inner corner where the path is one curve, the copy is soft and smooth.
    for j in 1..sides + usize::from(closed) {
        let (a, b) = (path_curve[(j + sides - 1) % sides], path_curve[j % sides]);
        if a.is_some() && a == b {
            for es in &ring_edges[j % (sides + 1)] {
                for e in es.iter().flatten() {
                    soft(&mut w, *e);
                }
            }
        }
    }
    // The edges along the path, for each point of the profile.
    let mut long: Vec<Vec<Vec<Option<EdgeId>>>> = Vec::with_capacity(sides);
    for j in 0..sides {
        let mut r = Vec::new();
        for (li, l) in ids[j].iter().enumerate() {
            let es: Vec<Option<EdgeId>> = (0..l.len())
                .map(|i| edge(&mut w, l[i], ids[j + 1][li][i]))
                .collect();
            r.push(es);
        }
        long.push(r);
    }
    // Where two sides of the profile are one curve, the edge along the path is soft and smooth.
    for (li, l) in loops.iter().enumerate() {
        let len = l.len();
        for i in 0..len {
            let before_side = curve_of.get(&key(l[(i + len - 1) % len], l[i]));
            let after_side = curve_of.get(&key(l[i], l[(i + 1) % len]));
            if before_side.is_some() && before_side == after_side {
                for row in &long {
                    if let Some(e) = row[li][i] {
                        soft(&mut w, e);
                    }
                }
            }
        }
    }
    // The side faces, with the paint of the profile.
    let paint: Vec<(String, serde_json::Value)> = [super::ATTR_MATERIAL, super::ATTR_BACK_MATERIAL]
        .iter()
        .filter_map(|k| f.attrs.get(*k).map(|v| ((*k).to_string(), v.clone())))
        .collect();
    for j in 0..sides {
        for (li, l) in ids[j].iter().enumerate() {
            let len = l.len();
            for i in 0..len {
                let next = &ids[j + 1][li];
                let mut q = vec![l[i], l[(i + 1) % len], next[(i + 1) % len], next[i]];
                q.dedup();
                while q.len() > 1 && q.first() == q.last() {
                    q.pop();
                }
                if q.len() < 3 {
                    continue;
                }
                let pts: Vec<DVec3> = q.iter().map(|v| w.pos(*v)).collect();
                let Some(plane) = Plane::new(pts[0], geom::newell(&pts)) else {
                    continue;
                };
                let mut g = Face::new(plane, canonical(&q), Vec::new());
                for (k2, v) in &paint {
                    g.attrs.insert(k2.clone(), v.clone());
                }
                w.new_face(g);
            }
        }
    }
    // The caps.
    if closed {
        // The profile goes with the edges that no other face uses (an edge on the axis of a turn).
        w.take_face(face);
        let own: Vec<EdgeId> = f
            .uses()
            .filter_map(|(a, b)| w.t.edge(a, b))
            .filter(|e| w.t.faces_of(*e).next().is_none())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for e in own {
            w.remove_edge(e);
        }
        let left: Vec<VertexId> = f
            .loops()
            .flatten()
            .copied()
            .filter(|v| w.t.degree(*v) == 0)
            .collect();
        for v in left {
            w.m.vertices.remove(&v);
        }
    } else {
        // The profile looks away from the sweep.
        if along {
            let mut g = w.take_face(face).expect("the profile");
            g.plane = g.plane.flipped();
            g.outer = reversed(&g.outer);
            let mut holes: Vec<Vec<VertexId>> = g.holes.iter().map(|h| reversed(h)).collect();
            holes.sort();
            g.holes = holes;
            super::swap_paint(&mut g.attrs);
            w.put_face(face, g);
        }
        // The end cap: the last ring, looking along the last side.
        let last = &ids[sides];
        let plane = Plane::new(p[k - 1], dirs[sides - 1]).ok_or(SurfaceError::TooShort { tol })?;
        let mut holes: Vec<Vec<VertexId>> = last[1..].iter().map(|h| canonical(h)).collect();
        holes.sort();
        let mut cap = Face::new(plane, canonical(&last[0]), holes);
        // The end cap has the look of the profile on the same physical sides: the side of the
        // profile that looks into the sweep is the inside (the back) of the cap.
        cap.attrs = f.attrs.clone();
        if along {
            super::swap_paint(&mut cap.attrs);
        }
        w.new_face(cap);
    }

    // Curves: a curve of the profile at each copy, a curve of the path along each point.
    let mut new_curves: Vec<Vec<EdgeId>> = Vec::new();
    for es in m_curves_of_profile(&w, &loops, &curve_of) {
        for (j, ring) in ring_edges.iter().enumerate() {
            if (j == 0 && !first_ring_moved) || (closed && j == sides) {
                continue;
            }
            let c: Option<Vec<EdgeId>> = es.iter().map(|&(li, i)| ring[li][i]).collect();
            if let Some(c) = c {
                new_curves.push(c);
            }
        }
    }
    for run in runs_of(&path_curve, closed) {
        for (li, l) in ids[0].iter().enumerate() {
            for i in 0..l.len() {
                let c: Option<Vec<EdgeId>> = run.iter().map(|&j| long[j][li][i]).collect();
                if let Some(c) = c.filter(|c| !c.is_empty()) {
                    new_curves.push(c);
                }
            }
        }
    }
    let mut used: BTreeSet<EdgeId> = w.m.curves.values().flatten().copied().collect();
    for c in new_curves {
        let unique: BTreeSet<EdgeId> = c.iter().copied().collect();
        if unique.len() == c.len() && c.iter().all(|e| !used.contains(e)) {
            used.extend(c.iter().copied());
            let id = CurveId(w.m.alloc());
            w.m.curves.insert(id, c);
        }
    }
    w.check_limits()?;

    // New faces side by side in one plane (the fan of a turn about an axis, a straight run of the
    // path) become one face, as an erase of the edge between them does.
    let start_id = before.next_id;
    let flat: Vec<EdgeId> =
        w.m.edges
            .keys()
            .copied()
            .filter(|e| e.0 >= start_id || w.t.faces_of(*e).all(|g| g.0 >= start_id))
            .filter(|e| {
                let fs: Vec<FaceId> = w.t.faces_of(*e).collect();
                let [a, b] = fs.as_slice() else { return false };
                if a.0 < start_id || b.0 < start_id {
                    return false;
                }
                let (fa, fb) = (&w.m.faces[a], &w.m.faces[b]);
                fa.plane.normal.dot(fb.plane.normal) >= 1.0 - 1e-9 && w.face_on(&fa.plane, fb)
            })
            .collect();
    let verts: BTreeSet<VertexId> = ids.iter().flatten().flatten().copied().collect();
    drop(w);
    if !flat.is_empty() {
        super::erase_edges(m, &flat, notes)?;
    }
    let verts: BTreeSet<VertexId> = verts
        .into_iter()
        .filter(|v| m.vertices.contains_key(v))
        .collect();
    let edges: BTreeSet<EdgeId> = m
        .edges
        .iter()
        .filter(|(_, e)| e.v.iter().any(|v| verts.contains(v)))
        .map(|(id, _)| *id)
        .collect();
    if let Some(c) = check::curves(m) {
        return Err(SurfaceError::Unsolved(c.to_string()));
    }
    let invalid = |p: &check::Problem| {
        SurfaceError::Refused(format!("Follow Me would make the model invalid: {p}"))
    };
    let after = check::structure_problems(m);
    if !after.is_empty() {
        let old = check::structure_problems(before);
        if let Some(p) = after.difference(&old).next() {
            return Err(invalid(p));
        }
    }
    if let Some(p) = pushpull::new_distance_problem(before, m, &edges, &verts) {
        return Err(invalid(&p));
    }
    if first_moved_note {
        notes.push(format!(
            "the profile (face {face}) was not at a corner of the closed path: the sweep starts \
             at vertex {} and the profile went",
            chain[0]
        ));
    }
    Ok(())
}

const ATTR_SOFT_KEY: &str = "soft";
const ATTR_SMOOTH_KEY: &str = "smooth";

/// The curves of the profile whose edges are all sides of its loops, each as (loop, side) pairs
/// in the order of the curve.
fn m_curves_of_profile(
    w: &Work<'_>,
    loops: &[Vec<VertexId>],
    curve_of: &BTreeMap<(VertexId, VertexId), CurveId>,
) -> Vec<Vec<(usize, usize)>> {
    let mut side_of: BTreeMap<(VertexId, VertexId), (usize, usize)> = BTreeMap::new();
    for (li, l) in loops.iter().enumerate() {
        for i in 0..l.len() {
            side_of.insert(key(l[i], l[(i + 1) % l.len()]), (li, i));
        }
    }
    let ids: BTreeSet<CurveId> = side_of
        .keys()
        .filter_map(|k| curve_of.get(k))
        .copied()
        .collect();
    let mut out = Vec::new();
    for c in ids {
        let Some(es) = w.m.curves.get(&c) else {
            continue;
        };
        let sides: Option<Vec<(usize, usize)>> = es
            .iter()
            .map(|e| {
                w.m.edges
                    .get(e)
                    .and_then(|ed| side_of.get(&ed.key()))
                    .copied()
            })
            .collect();
        if let Some(s) = sides {
            out.push(s);
        }
    }
    out
}

/// The runs of consecutive path sides in one curve (indices of the sides). A closed path whose
/// run wraps around its start gives that run from its first side.
fn runs_of(curves: &[Option<CurveId>], closed: bool) -> Vec<Vec<usize>> {
    let n = curves.len();
    if n == 0 {
        return Vec::new();
    }
    let start = if closed {
        (0..n)
            .find(|&i| curves[i] != curves[(i + n - 1) % n])
            .unwrap_or(0)
    } else {
        0
    };
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut cur: Option<(CurveId, Vec<usize>)> = None;
    for j in 0..n {
        let i = (start + j) % n;
        match (curves[i], &mut cur) {
            (Some(c), Some((cc, run))) if *cc == c => run.push(i),
            (c, slot) => {
                if let Some((_, run)) = slot.take() {
                    out.push(run);
                }
                *slot = c.map(|c| (c, vec![i]));
            }
        }
    }
    if let Some((_, run)) = cur {
        out.push(run);
    }
    out
}
