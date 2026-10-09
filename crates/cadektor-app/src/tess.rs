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
    let mut idx: Vec<usize> = if area2 >= 0.0 { (0..n).collect() } else { (0..n).rev().collect() };
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
                k != a && k != b && k != c && ![pa, pb, pc].contains(&pts[k]) && in_triangle(pts[k], pa, pb, pc)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn area(pts: &[DVec2], tris: &[[usize; 3]]) -> f64 {
        tris.iter().map(|t| ((pts[t[1]] - pts[t[0]]).perp_dot(pts[t[2]] - pts[t[0]])).abs() / 2.0).sum()
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
        let l = [(0., 0.), (2., 0.), (2., 1.), (1., 1.), (1., 2.), (0., 2.)].map(|(x, y)| DVec2::new(x, y));
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        assert!((area(&l, &t) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn too_few_points() {
        assert!(triangulate(&[DVec2::ZERO, DVec2::X]).is_empty());
    }
}
