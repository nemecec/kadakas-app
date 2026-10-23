//! Polygons into triangles, for OBJ faces with more than three corners.

/// The most corners that ear clipping works on.
pub(crate) const MAX_EAR_CORNERS: usize = 500;

/// Triangles `[a, b, c]` of the polygon whose corners are `pts` (in order). The triangles keep
/// the winding of the polygon. A convex polygon is a fan. Other polygons are cut by ear
/// clipping in the plane of the polygon (Newell normal). A polygon that cannot be cut (it
/// crosses itself, or is not convex and has more than 500 corners) is cut as a fan, and the second
/// value is `false`.
pub(crate) fn polygon(pts: &[[f64; 3]]) -> (Vec<[usize; 3]>, bool) {
    let n = pts.len();
    if n < 3 {
        return (Vec::new(), true);
    }
    if n == 3 {
        return (vec![[0, 1, 2]], true);
    }
    let fan = || (1..n - 1).map(|i| [0, i, i + 1]).collect::<Vec<_>>();
    // Newell normal.
    let mut nrm = [0.0f64; 3];
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        nrm[0] += (a[1] - b[1]) * (a[2] + b[2]);
        nrm[1] += (a[2] - b[2]) * (a[0] + b[0]);
        nrm[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let len = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt();
    if !(len.is_finite() && len > 0.0) {
        return (fan(), false);
    }
    // Project on the plane by dropping the largest axis of the normal.
    let drop = (0..3)
        .max_by(|&a, &b| nrm[a].abs().total_cmp(&nrm[b].abs()))
        .unwrap_or(2);
    let (u, v) = match drop {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let p2: Vec<[f64; 2]> = pts.iter().map(|p| [p[u], p[v]]).collect();
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    // Orientation of the projected polygon.
    let area: f64 = (0..n)
        .map(|i| {
            let (a, b) = (p2[i], p2[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let ccw = area >= 0.0;
    if !(area.is_finite() && area != 0.0) {
        return (fan(), false);
    }
    // Convex: every turn has the sign of the area, so a fan is right.
    let convex = (0..n).all(|i| {
        let c = cross(p2[i], p2[(i + 1) % n], p2[(i + 2) % n]);
        if ccw { c >= 0.0 } else { c <= 0.0 }
    });
    if convex {
        return (fan(), true);
    }
    // Ear clipping costs up to n cubed: a big polygon that is not convex is cut as a fan.
    if n > MAX_EAR_CORNERS {
        return (fan(), false);
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0usize;
    let mut at = 0usize;
    while idx.len() > 3 {
        let m = idx.len();
        let mut cut = false;
        for k in 0..m {
            let i = (at + k) % m;
            let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let t = cross(p2[a], p2[b], p2[c]);
            if (ccw && t <= 0.0) || (!ccw && t >= 0.0) {
                continue;
            }
            let inside = idx.iter().any(|&q| {
                if q == a || q == b || q == c {
                    return false;
                }
                let (d1, d2, d3) = (
                    cross(p2[a], p2[b], p2[q]),
                    cross(p2[b], p2[c], p2[q]),
                    cross(p2[c], p2[a], p2[q]),
                );
                if ccw {
                    d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
                } else {
                    d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0
                }
            });
            if inside {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(i);
            at = i % idx.len();
            cut = true;
            break;
        }
        guard += 1;
        if !cut || guard > n * n {
            return (fan(), false);
        }
    }
    out.push([idx[0], idx[1], idx[2]]);
    (out, true)
}
