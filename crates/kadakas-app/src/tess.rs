//! Ear-clipping triangulation for filled (possibly concave) polygons.

use glam::DVec2;

fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let d1 = (b - a).perp_dot(p - a);
    let d2 = (c - b).perp_dot(p - b);
    let d3 = (a - c).perp_dot(p - c);
    d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
}

/// Triangles as index triples into `pts`.
pub fn triangulate(pts: &[DVec2]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let area2: f64 = (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum();
    let mut idx: Vec<usize> = if area2 >= 0.0 {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::with_capacity(n - 2);
    while idx.len() > 3 {
        let m = idx.len();
        let ear = (0..m).find(|&i| {
            let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (pa, pb, pc) = (pts[a], pts[b], pts[c]);
            if (pb - pa).perp_dot(pc - pb) <= 1e-12 {
                return false;
            }
            !idx.iter().any(|&k| {
                k != a
                    && k != b
                    && k != c
                    && ![pa, pb, pc].contains(&pts[k])
                    && in_triangle(pts[k], pa, pb, pc)
            })
        });
        match ear {
            Some(i) => {
                let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
                out.push([a, b, c]);
                idx.remove(i);
            }
            None => {
                // Degenerate or self-intersecting input: fan the rest.
                for w in 1..m - 1 {
                    out.push([idx[0], idx[w], idx[w + 1]]);
                }
                return out;
            }
        }
    }
    out.push([idx[0], idx[1], idx[2]]);
    out
}

fn orient(ring: &[DVec2], ccw: bool) -> Vec<DVec2> {
    let area2: f64 = (0..ring.len())
        .map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()]))
        .sum();
    let mut r = ring.to_vec();
    if (area2 >= 0.0) != ccw {
        r.reverse();
    }
    r
}

fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (d1, d2) = ((b - a).perp_dot(c - a), (b - a).perp_dot(d - a));
    let (d3, d4) = ((d - c).perp_dot(a - c), (d - c).perp_dot(b - c));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// A polygon with holes as one ring: each hole is joined to the outline by a bridge (a pair of
/// equal edges), from its rightmost point to a vertex that it can see. The outline is rings[0].
fn bridged(rings: &[Vec<DVec2>]) -> Vec<DVec2> {
    let mut outer = orient(&rings[0], true);
    let mut holes: Vec<Vec<DVec2>> = rings[1..]
        .iter()
        .filter(|h| h.len() >= 3)
        .map(|h| orient(h, false))
        .collect();
    // The hole with the rightmost point first.
    holes.sort_by(|a, b| {
        let mx = |h: &Vec<DVec2>| h.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        mx(b).total_cmp(&mx(a))
    });
    for hole in holes {
        let (mi, m) = hole
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.x.total_cmp(&b.1.x))
            .expect("a hole has points");
        let mut order: Vec<usize> = (0..outer.len()).collect();
        order.sort_by(|&a, &b| outer[a].distance(m).total_cmp(&outer[b].distance(m)));
        let sees = |v: DVec2| {
            let blocked = |ring: &[DVec2]| {
                (0..ring.len()).any(|i| segments_cross(m, v, ring[i], ring[(i + 1) % ring.len()]))
            };
            !blocked(&outer) && !blocked(&hole)
        };
        let vi = order
            .iter()
            .copied()
            .find(|&i| sees(outer[i]))
            .unwrap_or(order[0]);
        let mut merged = Vec::with_capacity(outer.len() + hole.len() + 2);
        merged.extend_from_slice(&outer[..=vi]);
        for k in 0..=hole.len() {
            merged.push(hole[(mi + k) % hole.len()]);
        }
        merged.push(outer[vi]);
        merged.extend_from_slice(&outer[vi + 1..]);
        outer = merged;
    }
    outer
}

/// Triangles of a polygon with holes: `rings[0]` is the outline, the others are holes. Returns the
/// points that the triangles index (the rings joined into one, so some points repeat) and the
/// triangles.
pub fn triangulate_with_holes(rings: &[Vec<DVec2>]) -> (Vec<DVec2>, Vec<[usize; 3]>) {
    if rings.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let pts = if rings.len() == 1 {
        rings[0].clone()
    } else {
        bridged(rings)
    };
    let tris = triangulate(&pts);
    (pts, tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(pts: &[DVec2], tris: &[[usize; 3]]) -> f64 {
        tris.iter()
            .map(|t| ((pts[t[1]] - pts[t[0]]).perp_dot(pts[t[2]] - pts[t[0]])).abs() / 2.0)
            .sum()
    }

    #[test]
    fn square_both_windings() {
        let mut sq = vec![DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y];
        assert_eq!(area(&sq, &triangulate(&sq)), 1.0);
        sq.reverse();
        assert_eq!(area(&sq, &triangulate(&sq)), 1.0);
    }

    #[test]
    fn concave_l_shape() {
        let l = [(0., 0.), (2., 0.), (2., 1.), (1., 1.), (1., 2.), (0., 2.)]
            .map(|(x, y)| DVec2::new(x, y));
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        assert!((area(&l, &t) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn too_few_points() {
        assert!(triangulate(&[DVec2::ZERO, DVec2::X]).is_empty());
    }

    #[test]
    fn a_polygon_with_a_hole_is_filled_around_the_hole() {
        let sq = |x0: f64, y0: f64, x1: f64, y1: f64| {
            vec![
                DVec2::new(x0, y0),
                DVec2::new(x1, y0),
                DVec2::new(x1, y1),
                DVec2::new(x0, y1),
            ]
        };
        let (pts, tris) =
            triangulate_with_holes(&[sq(0.0, 0.0, 10.0, 10.0), sq(3.0, 3.0, 6.0, 7.0)]);
        let area: f64 = tris
            .iter()
            .map(|t| ((pts[t[1]] - pts[t[0]]).perp_dot(pts[t[2]] - pts[t[0]])).abs() / 2.0)
            .sum();
        assert!((area - (100.0 - 12.0)).abs() < 1e-9, "{area}");
        // No triangle has its center in the hole.
        for t in &tris {
            let c = (pts[t[0]] + pts[t[1]] + pts[t[2]]) / 3.0;
            assert!(!(c.x > 3.0 && c.x < 6.0 && c.y > 3.0 && c.y < 7.0), "{c:?}");
        }
        let (p, t) = triangulate_with_holes(&[]);
        assert!(p.is_empty() && t.is_empty());
    }
}
