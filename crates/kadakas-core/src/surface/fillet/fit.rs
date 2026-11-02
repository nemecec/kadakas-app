//! The checks of a plan: a changed face keeps its sides in their directions, its loops do not
//! cross and its holes stay inside; a corner piece is a simple polygon.

use super::super::arrange;
use super::super::index::BoxTree;
use super::*;

/// Checks the new `loops` of the face `id` (`f` before, `subs` the points that replace its
/// corners). The error is the reason.
pub(super) fn face(
    b: &Builder<'_>,
    id: FaceId,
    f: &ShapeFace,
    subs: &BTreeMap<VertexId, Vec<Pt>>,
    loops: &[Vec<Pt>],
) -> Result<(), String> {
    let tol = b.tol;
    // Every side keeps its direction and a length.
    for l in &f.loops {
        for i in 0..l.len() {
            let (x, y) = (l[i], l[(i + 1) % l.len()]);
            let from = subs.get(&x).and_then(|s| s.last()).copied();
            let to = subs.get(&y).and_then(|s| s.first()).copied();
            let (a, c) = (
                b.pos(from.unwrap_or(Pt::Old(x))),
                b.pos(to.unwrap_or(Pt::Old(y))),
            );
            let old = b.pos(Pt::Old(y)) - b.pos(Pt::Old(x));
            if (c - a).dot(old) <= 0.0 || a.distance(c) <= tol {
                return Err(format!("a side of face {id} would turn around or vanish"));
            }
        }
    }
    for s in subs.values() {
        for w in s.windows(2) {
            if b.pos(w[0]).distance(b.pos(w[1])) <= tol {
                return Err(format!(
                    "a new side of face {id} would be shorter than the length tolerance"
                ));
            }
        }
    }
    let rings: Vec<Vec<DVec2>> = loops
        .iter()
        .map(|l| l.iter().map(|p| flat(&f.plane, b.pos(*p))).collect())
        .collect();
    if crosses(&rings, tol) {
        return Err(format!("the outline of face {id} would cross itself"));
    }
    if geom::area2(&rings[0]) <= 0.0 || rings[1..].iter().any(|h| geom::area2(h) >= 0.0) {
        return Err(format!("face {id} would turn inside out"));
    }
    let outer = [rings[0].clone()];
    for h in &rings[1..] {
        if !arrange::inside(h[0], &outer) {
            return Err(format!("a hole of face {id} would leave the face"));
        }
    }
    Ok(())
}

/// True if two sides of the rings that are not neighbours come closer than `tol`.
fn crosses(rings: &[Vec<DVec2>], tol: f64) -> bool {
    let mut items: Vec<(DVec3, DVec3, u32)> = Vec::new();
    let mut sides: Vec<(usize, usize)> = Vec::new();
    for (li, r) in rings.iter().enumerate() {
        for i in 0..r.len() {
            let (a, c) = (r[i], r[(i + 1) % r.len()]);
            let pad = DVec2::splat(tol);
            items.push((
                (a.min(c) - pad).extend(0.0),
                (a.max(c) + pad).extend(0.0),
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
        let (a, c) = seg((li, i));
        let mut bad = false;
        tree.query(*lo, *hi, &mut |x| {
            let x = x as usize;
            if x <= k || bad {
                return;
            }
            let (lj, j) = sides[x];
            if lj == li && (j == (i + 1) % len || i == (j + 1) % len) {
                return;
            }
            bad = gap(a, c, seg((lj, j))) <= tol;
        });
        if bad {
            return true;
        }
    }
    false
}

/// The distance between the segment `a`–`c` and the segment `s`.
fn gap(a: DVec3, c: DVec3, s: (DVec3, DVec3)) -> f64 {
    match geom::segment_segment(a, c, s.0, s.1) {
        Some((_, _, d)) => d,
        None => [
            geom::point_segment(a, s.0, s.1).0,
            geom::point_segment(c, s.0, s.1).0,
            geom::point_segment(s.0, a, c).0,
            geom::point_segment(s.1, a, c).0,
        ]
        .into_iter()
        .fold(f64::INFINITY, f64::min),
    }
}

/// Checks a corner piece: its sides are longer than `tol`, sides that are not neighbours stay
/// apart, a side does not fold back on its neighbour, and it looks the way of `normal`.
pub(super) fn piece(pos: &[DVec3], normal: DVec3, tol: f64) -> Result<(), String> {
    let k = pos.len();
    if k < 3 {
        return Err("would have no area".into());
    }
    for i in 0..k {
        let (a, c) = (pos[i], pos[(i + 1) % k]);
        if a.distance(c) <= tol {
            return Err("would have a side shorter than the length tolerance".into());
        }
        // The far end of the next side stays off this side.
        let d = pos[(i + 2) % k];
        if geom::point_segment(d, a, c).0 <= tol {
            return Err("would fold back on itself".into());
        }
        for j in i + 2..k {
            if (j + 1) % k == i {
                continue;
            }
            if gap(a, c, (pos[j], pos[(j + 1) % k])) <= tol {
                return Err("would cross itself".into());
            }
        }
    }
    if geom::newell(pos).dot(normal) <= 0.0 {
        return Err("would look the wrong way".into());
    }
    Ok(())
}
