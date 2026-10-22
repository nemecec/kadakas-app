//! The fold of `transform_vertices`: the triangles of a face that is no longer flat.
//!
//! The cut depends only on the geometry, never on the ids or on where a loop starts: the loops
//! are first written in a fixed way (the outer loop counterclockwise around the best-fit plane,
//! the holes clockwise, each loop from its lowest point by the coordinates x, then y, then z, and
//! the holes in the order of those points), and every choice below breaks ties by coordinates.
//!
//! - **A quad** (four vertices, no hole) is cut along the diagonal that bends the least: the
//!   smaller angle between the normals of the two triangles. Equal bends (within 1e-9 rad): the
//!   shorter diagonal (within the length tolerance). Still equal: the diagonal whose ends compare
//!   lower by coordinates (the lower end first, then the other).
//! - **Any other face** is first cut into ears (holes bridged, a hole that touches the outer loop
//!   at a vertex joined there), then the diagonals are flipped toward the constrained Delaunay
//!   triangulation in the best-fit plane: a diagonal is flipped when the far corner of the other
//!   triangle lies inside the circle of the first (beyond a relative tolerance), or when one of
//!   the two triangles has no area (three points on a straight side) and the flip gives two with
//!   an area. That keeps the triangles well shaped (the Delaunay triangulation makes the smallest
//!   angle as large as it can be) and needs no choice of ids. The loop sides are never flipped.
//!
//! The flips use a map of the triangle sides, so a face of `t` triangles takes about
//! `O(t log t)`.

use std::collections::{HashMap, HashSet};

use super::*;

/// The triangles of a face that is not flat, in the best-fit plane: vertex triples, each
/// counterclockwise around the normal of the fit.
pub(super) fn fold(
    m: &SurfaceModel,
    f: &Face,
    id: FaceId,
) -> Result<Vec<[VertexId; 3]>, SurfaceError> {
    let cannot = || bad(format!("face {id} cannot be folded into triangles"));
    let plane = fit(m, f).ok_or_else(cannot)?;
    let p3 = |v: VertexId| m.vertices[&v];
    let p2: HashMap<VertexId, DVec2> = f
        .loops()
        .flatten()
        .map(|v| (*v, plane.project(p3(*v))))
        .collect();
    let ring = |l: &[VertexId], ccw: bool| -> Vec<VertexId> {
        let mut ids = l.to_vec();
        let p: Vec<DVec2> = ids.iter().map(|v| p2[v]).collect();
        if (geom::area2(&p) > 0.0) != ccw {
            ids.reverse();
        }
        let start = (0..ids.len())
            .min_by(|&i, &j| lex(p3(ids[i]), p3(ids[j])))
            .expect("a loop has vertices");
        ids.rotate_left(start);
        ids
    };
    let outer = ring(&f.outer, true);
    let mut holes: Vec<Vec<VertexId>> = f.holes.iter().map(|h| ring(h, false)).collect();
    holes.sort_by(|a, b| lex(p3(a[0]), p3(b[0])));
    let all3: Vec<DVec3> = f.loops().flatten().map(|v| p3(*v)).collect();
    let tol = geom::tol(&all3);
    let area = |t: &[VertexId; 3]| (p2[&t[1]] - p2[&t[0]]).perp_dot(p2[&t[2]] - p2[&t[0]]);
    let thin = |t: &[VertexId; 3]| {
        let longest = (0..3)
            .map(|k| p2[&t[k]].distance(p2[&t[(k + 1) % 3]]))
            .fold(0.0, f64::max);
        area(t) <= tol * longest
    };
    let mut tris = if holes.is_empty() && outer.len() == 4 {
        quad(&outer, &p3, &thin).ok_or_else(cannot)?
    } else {
        ears(&plane, &outer, &holes, &p2, &p3).ok_or_else(cannot)?
    };
    // The pieces must make up the polygon.
    let ring_area = |l: &[VertexId]| geom::area2(&l.iter().map(|v| p2[v]).collect::<Vec<_>>());
    let want = ring_area(&outer) + holes.iter().map(|h| ring_area(h)).sum::<f64>();
    let got: f64 = tris.iter().map(|t| area(t).abs()).sum();
    if (want - got).abs() > 1e-6 * want.abs().max(1.0) {
        return Err(bad(format!(
            "a loop of face {id} would cross itself, so it cannot be folded"
        )));
    }
    if tris.len() > 2 || !holes.is_empty() {
        let sides: HashSet<(VertexId, VertexId)> = f.uses().map(|(a, b)| key(a, b)).collect();
        flip_to_delaunay(&mut tris, &p2, &sides, &thin);
    }
    if tris.iter().any(|t| thin(t) || area(t) <= 0.0) {
        return Err(cannot());
    }
    Ok(tris)
}

/// The order of points by x, then y, then z.
fn lex(a: DVec3, b: DVec3) -> std::cmp::Ordering {
    a.x.total_cmp(&b.x)
        .then(a.y.total_cmp(&b.y))
        .then(a.z.total_cmp(&b.z))
}

/// The two triangles of a quad `r` (counterclockwise), cut along the diagonal that bends the
/// least (see the module).
fn quad(
    r: &[VertexId],
    p3: &dyn Fn(VertexId) -> DVec3,
    thin: &dyn Fn(&[VertexId; 3]) -> bool,
) -> Option<Vec<[VertexId; 3]>> {
    let tol = geom::tol(&r.iter().map(|v| p3(*v)).collect::<Vec<_>>());
    // The bend, the length and the ends of the diagonal, and the two triangles.
    type Cut = (f64, f64, [DVec3; 2], Vec<[VertexId; 3]>);
    let mut best: Option<Cut> = None;
    for i in 0..2 {
        let (a, b, c, d) = (r[i], r[i + 1], r[i + 2], r[(i + 3) % 4]);
        let tris = vec![[a, b, c], [c, d, a]];
        if tris.iter().any(thin) {
            continue;
        }
        let n1 = (p3(b) - p3(a)).cross(p3(c) - p3(a));
        let n2 = (p3(d) - p3(c)).cross(p3(a) - p3(c));
        let bend = n1.angle_between(n2);
        let len = p3(a).distance(p3(c));
        let mut ends = [p3(a), p3(c)];
        if lex(ends[1], ends[0]).is_lt() {
            ends.swap(0, 1);
        }
        let better = match &best {
            None => true,
            Some((b0, l0, e0, _)) => {
                if (bend - b0).abs() > 1e-9 {
                    bend < *b0
                } else if (len - l0).abs() > tol {
                    len < *l0
                } else {
                    lex(ends[0], e0[0]).then(lex(ends[1], e0[1])).is_lt()
                }
            }
        };
        if better {
            best = Some((bend, len, ends, tris));
        }
    }
    best.map(|b| b.3)
}

/// A first cut into triangles: ears of the loops in their fixed order, holes bridged; a hole
/// that touches the outer loop at a vertex is joined there.
fn ears(
    plane: &Plane,
    outer: &[VertexId],
    holes: &[Vec<VertexId>],
    p2: &HashMap<VertexId, DVec2>,
    p3: &dyn Fn(VertexId) -> DVec3,
) -> Option<Vec<[VertexId; 3]>> {
    let ids: Vec<VertexId> = outer
        .iter()
        .chain(holes.iter().flatten())
        .copied()
        .collect();
    let rings: Vec<Vec<DVec2>> = std::iter::once(outer)
        .chain(holes.iter().map(Vec::as_slice))
        .map(|l| l.iter().map(|v| p2[v]).collect())
        .collect();
    let t = match geom::triangulate_rings(&rings) {
        Some(t) => t,
        None => {
            let face = Face::new(*plane, outer.to_vec(), holes.to_vec());
            geom::spliced(&face, &ids, &|v| p3(v))?
        }
    };
    // A bridge of length zero (a hole that touches the outer loop at a vertex) leaves a sliver
    // with that vertex twice: it has no area and goes.
    Some(
        t.into_iter()
            .map(|t| t.map(|i| ids[i as usize]))
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect(),
    )
}

/// Flips the diagonals of the triangles toward the constrained Delaunay triangulation (see the
/// module). `sides` are the loop sides, which stay.
fn flip_to_delaunay(
    tris: &mut [[VertexId; 3]],
    p2: &HashMap<VertexId, DVec2>,
    sides: &HashSet<(VertexId, VertexId)>,
    thin: &dyn Fn(&[VertexId; 3]) -> bool,
) {
    // Each directed side of a triangle: the triangle.
    let mut at: HashMap<(VertexId, VertexId), usize> = HashMap::with_capacity(3 * tris.len());
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            if at.insert((t[k], t[(k + 1) % 3]), i).is_some() {
                // Not a plain triangulation (a side twice): leave it as it is.
                return;
            }
        }
    }
    let inner = |a: VertexId, b: VertexId| !sides.contains(&key(a, b));
    let mut stack: Vec<(VertexId, VertexId)> = Vec::new();
    for t in tris.iter() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a < b && inner(a, b) && at.contains_key(&(b, a)) {
                stack.push((a, b));
            }
        }
    }
    stack.reverse();
    let corner = |t: &[VertexId; 3], a: VertexId, b: VertexId| {
        *t.iter()
            .find(|&&v| v != a && v != b)
            .expect("a third corner")
    };
    let ccw = |t: &[VertexId; 3]| (p2[&t[1]] - p2[&t[0]]).perp_dot(p2[&t[2]] - p2[&t[0]]) > 0.0;
    // Enough for the flips (each flip makes the triangulation strictly better); a guard only.
    let mut budget = 64 * tris.len() + 1024;
    while let Some((a, b)) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let (Some(&i), Some(&j)) = (at.get(&(a, b)), at.get(&(b, a))) else {
            continue;
        };
        let (ti, tj) = (tris[i], tris[j]);
        let (c, d) = (corner(&ti, a, b), corner(&tj, a, b));
        if c == d || at.contains_key(&(c, d)) || at.contains_key(&(d, c)) {
            continue;
        }
        let (n1, n2) = ([a, d, c], [d, b, c]);
        if !ccw(&n1) || !ccw(&n2) || thin(&n1) || thin(&n2) {
            continue;
        }
        let was_thin = thin(&ti) || thin(&tj);
        if !was_thin && !in_circle(p2[&a], p2[&b], p2[&c], p2[&d]) {
            continue;
        }
        for t in [ti, tj] {
            for k in 0..3 {
                at.remove(&(t[k], t[(k + 1) % 3]));
            }
        }
        tris[i] = n1;
        tris[j] = n2;
        for (t, idx) in [(n1, i), (n2, j)] {
            for k in 0..3 {
                at.insert((t[k], t[(k + 1) % 3]), idx);
            }
        }
        for (x, y) in [(a, d), (d, b), (b, c), (c, a)] {
            if inner(x, y) {
                stack.push((x.min(y), x.max(y)));
            }
        }
    }
}

/// True if `d` lies inside the circle through the counterclockwise triangle `a`, `b`, `c`, beyond
/// a relative tolerance (points on the circle are not inside, so four points on one circle keep
/// the diagonal they have).
fn in_circle(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (ad, bd, cd) = (a - d, b - d, c - d);
    let (la, lb, lc) = (
        ad.length_squared(),
        bd.length_squared(),
        cd.length_squared(),
    );
    let (x, y, z) = (bd.perp_dot(cd), cd.perp_dot(ad), ad.perp_dot(bd));
    let det = la * x + lb * y + lc * z;
    let bound = la * ((bd.x * cd.y).abs() + (cd.x * bd.y).abs())
        + lb * ((cd.x * ad.y).abs() + (ad.x * cd.y).abs())
        + lc * ((ad.x * bd.y).abs() + (bd.x * ad.y).abs());
    det > 1e-10 * bound
}
