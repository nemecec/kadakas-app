//! Layout helpers: pure placement math for heads and drip lines.
//!
//! All lengths are in mm (the model unit). Polygons are lists of corner points without a
//! repeated first point. They may be concave but must not have holes.

use cadektor_core::DVec2;

/// Most points one call returns.
pub const MAX_POINTS: usize = 100_000;

/// Most corners an area may have. Every head is tested against every edge.
pub const MAX_VERTICES: usize = 2_000;

/// How heads are arranged in an area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pattern {
    /// Rows and columns `spacing` apart.
    #[default]
    Square,
    /// Rows `spacing * sin 60` apart. Every second row moves by half the spacing, so that each
    /// head has six neighbors at head-to-head distance.
    Triangular,
}

impl Pattern {
    pub fn from_name(s: &str) -> Option<Pattern> {
        match s.trim().to_ascii_lowercase().as_str() {
            "square" => Some(Pattern::Square),
            "triangular" | "triangle" => Some(Pattern::Triangular),
            _ => None,
        }
    }
}

const EPS: f64 = 1e-6;

fn check_polygon(poly: &[DVec2]) -> Result<(), String> {
    if poly.len() < 3 {
        return Err("an area needs 3 or more points".into());
    }
    if poly.len() > MAX_VERTICES {
        return Err(format!(
            "an area has at most {MAX_VERTICES} corners (simplify the outline)"
        ));
    }
    if poly.iter().any(|p| !p.is_finite()) {
        return Err("the area has a point that is not finite".into());
    }
    Ok(())
}

fn check_spacing(spacing: f64) -> Result<(), String> {
    if !spacing.is_finite() || spacing <= 0.0 {
        Err("the spacing must be positive".into())
    } else {
        Ok(())
    }
}

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

/// Distance from `p` to the boundary of the polygon.
pub fn boundary_distance(p: DVec2, poly: &[DVec2]) -> f64 {
    (0..poly.len())
        .map(|i| segment_distance(p, poly[i], poly[(i + 1) % poly.len()]))
        .fold(f64::INFINITY, f64::min)
}

/// True if `p` is inside the polygon or on its boundary (even-odd rule).
pub fn contains(poly: &[DVec2], p: DVec2) -> bool {
    if boundary_distance(p, poly) <= EPS {
        return true;
    }
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Area of a polygon in mm2.
pub fn polygon_area(poly: &[DVec2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| poly[i].perp_dot(poly[(i + 1) % n]))
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Head positions that fill a polygon.
///
/// The pattern is turned by `angle_deg` (degrees counterclockwise). It starts `inset` mm inside
/// the lower left corner of the turned bounding box. Only points that lie in the polygon and at
/// least `inset` from its boundary are kept. `spacing` is the head-to-head distance.
pub fn fill_area(
    polygon: &[DVec2],
    spacing: f64,
    pattern: Pattern,
    angle_deg: f64,
    inset: f64,
) -> Result<Vec<DVec2>, String> {
    check_polygon(polygon)?;
    check_spacing(spacing)?;
    if !angle_deg.is_finite() || !inset.is_finite() || inset < 0.0 {
        return Err("the angle must be finite and the inset must not be negative".into());
    }
    let turn = DVec2::from_angle(angle_deg.to_radians());
    let local: Vec<DVec2> = polygon
        .iter()
        .map(|p| DVec2::new(turn.x, -turn.y).rotate(*p))
        .collect();
    let (lo, hi) = local.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let row_step = match pattern {
        Pattern::Square => spacing,
        Pattern::Triangular => spacing * (std::f64::consts::PI / 3.0).sin(),
    };
    // Bound the work before the loops start.
    let cells = ((hi.x - lo.x) / spacing + 2.0) * ((hi.y - lo.y) / row_step + 2.0);
    if cells > 4.0 * MAX_POINTS as f64 {
        return Err(format!(
            "the spacing is too small for this area (more than {MAX_POINTS} heads)"
        ));
    }
    let mut out = Vec::new();
    let mut row = 0usize;
    loop {
        let y = lo.y + inset + row as f64 * row_step;
        if y > hi.y + EPS {
            break;
        }
        let shift = if pattern == Pattern::Triangular && row % 2 == 1 {
            spacing / 2.0
        } else {
            0.0
        };
        let mut col = 0usize;
        loop {
            let x = lo.x + inset + shift + col as f64 * spacing;
            if x > hi.x + EPS {
                break;
            }
            let p = turn.rotate(DVec2::new(x, y));
            if contains(polygon, p) && boundary_distance(p, polygon) >= inset - EPS {
                out.push(p);
                if out.len() > MAX_POINTS {
                    return Err(format!("more than {MAX_POINTS} heads"));
                }
            }
            col += 1;
        }
        row += 1;
    }
    Ok(out)
}

/// A head on an edge, with the direction of the edge there (radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeHead {
    pub at: DVec2,
    pub tangent: f64,
}

/// Length of a path in mm.
pub fn path_length(path: &[DVec2], closed: bool) -> f64 {
    let n = path.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs).map(|i| path[i].distance(path[(i + 1) % n])).sum()
}

/// Heads along a path. The spacing is the largest one not above `spacing` that fits whole: the
/// path is divided into `ceil(length / spacing)` equal parts. An open path gets a head at both
/// ends. A closed path gets no repeated head at the start.
pub fn along_edge(path: &[DVec2], spacing: f64, closed: bool) -> Result<Vec<EdgeHead>, String> {
    check_spacing(spacing)?;
    if path.len() < 2 {
        return Err("a path needs 2 or more points".into());
    }
    if path.iter().any(|p| !p.is_finite()) {
        return Err("the path has a point that is not finite".into());
    }
    let len = path_length(path, closed);
    if len <= EPS {
        return Err("the path has no length".into());
    }
    let parts = ((len / spacing - 1e-9).ceil() as usize).max(1);
    if parts > MAX_POINTS {
        return Err(format!("more than {MAX_POINTS} heads"));
    }
    let step = len / parts as f64;
    let count = if closed { parts } else { parts + 1 };
    let n = path.len();
    let segs = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(count);
    let (mut seg, mut walked) = (0usize, 0.0f64);
    for k in 0..count {
        let target = k as f64 * step;
        // Move to the segment that holds `target`.
        loop {
            let (a, b) = (path[seg % n], path[(seg + 1) % n]);
            let l = a.distance(b);
            if target <= walked + l + 1e-9 || seg + 1 >= segs {
                let t = if l > 0.0 {
                    ((target - walked) / l).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let dir = if l > 0.0 { (b - a) / l } else { DVec2::X };
                out.push(EdgeHead {
                    at: a + (b - a) * t,
                    tangent: dir.to_angle(),
                });
                break;
            }
            walked += l;
            seg += 1;
        }
    }
    Ok(out)
}

/// Parallel drip lines in a polygon. The lines run in the direction `angle_deg` (0 is along X),
/// `spacing` mm apart, the first one `spacing / 2` from the edge of the turned bounding box. Each
/// line is cut by the polygon, so a concave polygon can give several pieces per line.
/// A piece is shortened by `inset` at both ends. Pieces that become empty are dropped.
/// Every piece runs from the lower to the higher turned coordinate.
pub fn drip_lines(
    polygon: &[DVec2],
    spacing: f64,
    angle_deg: f64,
    inset: f64,
) -> Result<Vec<(DVec2, DVec2)>, String> {
    check_polygon(polygon)?;
    check_spacing(spacing)?;
    if !angle_deg.is_finite() || !inset.is_finite() || inset < 0.0 {
        return Err("the angle must be finite and the inset must not be negative".into());
    }
    let turn = DVec2::from_angle(angle_deg.to_radians());
    let local: Vec<DVec2> = polygon
        .iter()
        .map(|p| DVec2::new(turn.x, -turn.y).rotate(*p))
        .collect();
    let (lo, hi) = local.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    if (hi.y - lo.y) / spacing > MAX_POINTS as f64 {
        return Err("the spacing is too small for this area".into());
    }
    let n = local.len();
    let mut out = Vec::new();
    let mut k = 0usize;
    loop {
        let y = lo.y + spacing / 2.0 + k as f64 * spacing;
        if y >= hi.y {
            break;
        }
        let mut xs: Vec<f64> = (0..n)
            .filter_map(|i| {
                let (a, b) = (local[i], local[(i + 1) % n]);
                // Half-open rule: a vertex on the line counts for one side only.
                ((a.y > y) != (b.y > y)).then(|| a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x))
            })
            .collect();
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks_exact(2) {
            let (x0, x1) = (pair[0] + inset, pair[1] - inset);
            if x1 - x0 > EPS {
                out.push((
                    turn.rotate(DVec2::new(x0, y)),
                    turn.rotate(DVec2::new(x1, y)),
                ));
                if out.len() > MAX_POINTS {
                    return Err(format!("more than {MAX_POINTS} lines"));
                }
            }
        }
        k += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn rect(w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]
    }

    /// A 10 m x 6 m rectangle, spacing 2 m, square pattern: 6 columns x 4 rows = 24 heads.
    #[test]
    fn square_fill_counts_hand_example() {
        let pts = fill_area(&rect(10_000.0, 6_000.0), 2_000.0, Pattern::Square, 0.0, 0.0).unwrap();
        assert_eq!(pts.len(), 24);
        assert_eq!(pts[0], DVec2::ZERO);
        assert_eq!(pts[23], DVec2::new(10_000.0, 6_000.0));
    }

    /// Triangular, spacing 2 m, rows 1.732 m apart: rows at y = 0, 1.732, 3.464, 5.196 (4 rows).
    /// Even rows have 6 heads (x = 0..10 m), odd rows 5 heads (x = 1, 3, .. 9 m): 6+5+6+5 = 22.
    #[test]
    fn triangular_fill_counts_hand_example() {
        let pts = fill_area(
            &rect(10_000.0, 6_000.0),
            2_000.0,
            Pattern::Triangular,
            0.0,
            0.0,
        )
        .unwrap();
        assert_eq!(pts.len(), 22);
        // Neighbors in a row and across rows are 2 m apart.
        let d = pts[0].distance(pts[1]);
        assert!((d - 2_000.0).abs() < 1e-6);
        let across = pts[0].distance(pts[6]);
        assert!((across - 2_000.0).abs() < 1e-6, "{across}");
    }

    #[test]
    fn inset_keeps_heads_off_the_edge() {
        let pts = fill_area(
            &rect(10_000.0, 6_000.0),
            2_000.0,
            Pattern::Square,
            0.0,
            500.0,
        )
        .unwrap();
        // Start at 0.5 m: x = 0.5 .. 8.5 (5 values, 10.5 is outside the 9.5 limit), y = 0.5, 2.5, 4.5.
        assert_eq!(pts.len(), 5 * 3);
        assert!(
            pts.iter()
                .all(|p| boundary_distance(*p, &rect(10_000.0, 6_000.0)) >= 499.999)
        );
    }

    #[test]
    fn rotated_fill_stays_inside() {
        let poly = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(8000.0, 1000.0),
            DVec2::new(6000.0, 7000.0),
            DVec2::new(1000.0, 5000.0),
        ];
        let pts = fill_area(&poly, 1500.0, Pattern::Square, 30.0, 0.0).unwrap();
        assert!(!pts.is_empty());
        assert!(pts.iter().all(|p| contains(&poly, *p)));
    }

    #[test]
    fn concave_fill_skips_the_notch() {
        // An L shape: the upper right 5 m x 5 m square is missing.
        let l = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(10_000.0, 0.0),
            DVec2::new(10_000.0, 5_000.0),
            DVec2::new(5_000.0, 5_000.0),
            DVec2::new(5_000.0, 10_000.0),
            DVec2::new(0.0, 10_000.0),
        ];
        let pts = fill_area(&l, 2_500.0, Pattern::Square, 0.0, 0.0).unwrap();
        assert!(pts.iter().all(|p| contains(&l, *p)));
        assert!(!pts.contains(&DVec2::new(10_000.0, 10_000.0)));
        assert!(pts.contains(&DVec2::new(5_000.0, 5_000.0)));
    }

    #[test]
    fn a_boundary_with_too_many_corners_is_refused_quickly() {
        let n = 20_000;
        let ring: Vec<DVec2> = (0..n)
            .map(|k| {
                let a = k as f64 / n as f64 * std::f64::consts::TAU;
                DVec2::new(a.cos() * 50_000.0, a.sin() * 50_000.0)
            })
            .collect();
        let t = std::time::Instant::now();
        assert!(
            fill_area(&ring, 1000.0, Pattern::Square, 0.0, 0.0)
                .unwrap_err()
                .contains("at most")
        );
        assert!(drip_lines(&ring, 1000.0, 0.0, 0.0).is_err());
        assert!(t.elapsed().as_secs_f64() < 1.0);
        // A ring at the limit works.
        let ring: Vec<DVec2> = ring.into_iter().step_by(10).collect();
        assert_eq!(ring.len(), MAX_VERTICES);
        let t = std::time::Instant::now();
        let pts = fill_area(&ring, 2000.0, Pattern::Triangular, 0.0, 100.0).unwrap();
        assert!(pts.len() > 1000);
        assert!(t.elapsed().as_secs_f64() < 5.0);
    }

    #[test]
    fn fill_errors() {
        let r = rect(1000.0, 1000.0);
        assert!(fill_area(&r[..2], 100.0, Pattern::Square, 0.0, 0.0).is_err());
        assert!(fill_area(&r, 0.0, Pattern::Square, 0.0, 0.0).is_err());
        assert!(fill_area(&r, f64::NAN, Pattern::Square, 0.0, 0.0).is_err());
        assert!(fill_area(&r, 100.0, Pattern::Square, f64::NAN, 0.0).is_err());
        assert!(fill_area(&r, 100.0, Pattern::Square, 0.0, -1.0).is_err());
        let bad = vec![DVec2::ZERO, DVec2::X, DVec2::new(f64::NAN, 1.0)];
        assert!(fill_area(&bad, 100.0, Pattern::Square, 0.0, 0.0).is_err());
        let big = rect(1e9, 1e9);
        assert!(
            fill_area(&big, 1.0, Pattern::Square, 0.0, 0.0)
                .unwrap_err()
                .contains("too small")
        );
        // Many points that pass the cheap bound but not the count limit.
        let wide = rect(300_000.0, 1_000.0);
        assert!(fill_area(&wide, 1.0, Pattern::Square, 0.0, 0.0).is_err());
        assert_eq!(Pattern::from_name("Triangle"), Some(Pattern::Triangular));
        assert_eq!(Pattern::from_name("square"), Some(Pattern::Square));
        assert_eq!(Pattern::from_name("hex"), None);
    }

    /// A path of 10 m with a spacing of 3 m: ceil(3.33) = 4 parts of 2.5 m, 5 heads.
    #[test]
    fn along_edge_hand_example() {
        let path = [DVec2::ZERO, DVec2::new(10_000.0, 0.0)];
        let h = along_edge(&path, 3_000.0, false).unwrap();
        assert_eq!(h.len(), 5);
        assert!((h[1].at.x - 2_500.0).abs() < 1e-9);
        assert_eq!(h[4].at, DVec2::new(10_000.0, 0.0));
        assert!(h.iter().all(|x| x.tangent == 0.0));
    }

    #[test]
    fn along_edge_corners_and_closed() {
        // An L path of 4 m + 3 m, spacing 1 m: 7 parts, heads at every metre.
        let path = [
            DVec2::ZERO,
            DVec2::new(4000.0, 0.0),
            DVec2::new(4000.0, 3000.0),
        ];
        let h = along_edge(&path, 1000.0, false).unwrap();
        assert_eq!(h.len(), 8);
        assert!((h[5].at - DVec2::new(4000.0, 1000.0)).length() < 1e-6);
        assert!((h[5].tangent - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        // A closed square of 4 m sides, spacing 1 m: 16 heads, none repeated.
        let sq = rect(4000.0, 4000.0);
        let h = along_edge(&sq, 1000.0, true).unwrap();
        assert_eq!(h.len(), 16);
        assert!(h.iter().all(|x| boundary_distance(x.at, &sq) < 1e-6));
        // A spacing larger than the path still gives both ends.
        let h = along_edge(&path[..2], 99_000.0, false).unwrap();
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn along_edge_errors() {
        let p = [DVec2::ZERO, DVec2::X];
        assert!(along_edge(&p[..1], 1.0, false).is_err());
        assert!(along_edge(&p, 0.0, false).is_err());
        assert!(along_edge(&[DVec2::ZERO, DVec2::ZERO], 1.0, false).is_err());
        assert!(along_edge(&[DVec2::ZERO, DVec2::new(f64::INFINITY, 0.0)], 1.0, false).is_err());
        assert!(along_edge(&[DVec2::ZERO, DVec2::new(1e9, 0.0)], 1.0, false).is_err());
        // A repeated point inside the path is harmless.
        let q = [DVec2::ZERO, DVec2::ZERO, DVec2::new(1000.0, 0.0)];
        assert_eq!(along_edge(&q, 500.0, false).unwrap().len(), 3);
    }

    /// 10 m x 6 m, lines 2 m apart along X: at y = 1, 3, 5 m, each 10 m long.
    #[test]
    fn drip_lines_hand_example() {
        let lines = drip_lines(&rect(10_000.0, 6_000.0), 2_000.0, 0.0, 0.0).unwrap();
        assert_eq!(lines.len(), 3);
        for (k, (a, b)) in lines.iter().enumerate() {
            assert!((a.y - (1000.0 + 2000.0 * k as f64)).abs() < 1e-9);
            assert_eq!((a.x, b.x), (0.0, 10_000.0));
        }
        let lines = drip_lines(&rect(10_000.0, 6_000.0), 2_000.0, 0.0, 500.0).unwrap();
        assert_eq!(lines[0].0.x, 500.0);
        assert_eq!(lines[0].1.x, 9_500.0);
        // An inset larger than the polygon removes everything.
        assert!(
            drip_lines(&rect(10_000.0, 6_000.0), 2_000.0, 0.0, 6_000.0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn drip_lines_concave_gives_pieces() {
        // A U shape: a 2 m wide gap in the middle of the top half splits the upper lines.
        let u = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(10_000.0, 0.0),
            DVec2::new(10_000.0, 10_000.0),
            DVec2::new(6_000.0, 10_000.0),
            DVec2::new(6_000.0, 4_000.0),
            DVec2::new(4_000.0, 4_000.0),
            DVec2::new(4_000.0, 10_000.0),
            DVec2::new(0.0, 10_000.0),
        ];
        let lines = drip_lines(&u, 1_000.0, 0.0, 0.0).unwrap();
        // 10 lines. Those at y = 0.5 .. 3.5 m are whole (4), the upper 6 are in two pieces.
        assert_eq!(lines.len(), 4 + 6 * 2);
        for (a, b) in &lines {
            assert!(contains(&u, (*a + *b) / 2.0));
        }
    }

    #[test]
    fn drip_lines_rotated_and_errors() {
        let lines = drip_lines(&rect(10_000.0, 10_000.0), 2_000.0, 90.0, 0.0).unwrap();
        // Lines along Y at x = 1, 3, 5, 7, 9 m.
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().all(|(a, b)| (a.x - b.x).abs() < 1e-6));
        let r = rect(1000.0, 1000.0);
        assert!(drip_lines(&r[..2], 100.0, 0.0, 0.0).is_err());
        assert!(drip_lines(&r, -1.0, 0.0, 0.0).is_err());
        assert!(drip_lines(&r, 100.0, f64::NAN, 0.0).is_err());
        assert!(drip_lines(&r, 100.0, 0.0, -1.0).is_err());
        assert!(drip_lines(&rect(1e9, 1e9), 1.0, 0.0, 0.0).is_err());
        // More lines than the limit.
        assert!(drip_lines(&rect(1e3, 4e8), 5_000.0, 0.0, 0.0).is_ok());
        assert!(drip_lines(&rect(1e3, 2e5), 1.0, 0.0, 0.0).is_err());
    }

    #[test]
    fn polygon_helpers() {
        assert_eq!(polygon_area(&rect(3.0, 4.0)), 12.0);
        let r = rect(10.0, 10.0);
        assert!(contains(&r, DVec2::new(5.0, 5.0)));
        assert!(contains(&r, DVec2::new(0.0, 5.0)));
        assert!(!contains(&r, DVec2::new(11.0, 5.0)));
        assert_eq!(boundary_distance(DVec2::new(5.0, 5.0), &r), 5.0);
        // A zero-length edge does not break the distance.
        assert_eq!(
            segment_distance(DVec2::new(3.0, 4.0), DVec2::ZERO, DVec2::ZERO),
            5.0
        );
        assert_eq!(path_length(&r, true), 40.0);
        assert_eq!(path_length(&r, false), 30.0);
    }

    proptest! {
        /// On a rectangle with whole-number sizes the square count is exact:
        /// (floor(W / s) + 1) * (floor(H / s) + 1).
        #[test]
        fn square_count_formula(w in 1u32..40, h in 1u32..40, s in 1u32..10) {
            let (w, h, s) = (w as f64 * 100.0, h as f64 * 100.0, s as f64 * 100.0);
            let pts = fill_area(&rect(w, h), s, Pattern::Square, 0.0, 0.0).unwrap();
            let expect = ((w / s).floor() + 1.0) * ((h / s).floor() + 1.0);
            prop_assert_eq!(pts.len() as f64, expect);
        }

        /// Heads are inside the polygon and no two are closer than the spacing.
        #[test]
        fn fill_points_inside_and_spaced(
            w in 2000.0f64..20000.0, h in 2000.0f64..20000.0,
            s in 500.0f64..3000.0, angle in 0.0f64..180.0, tri in any::<bool>(), inset in 0.0f64..400.0,
        ) {
            let poly = rect(w, h);
            let pattern = if tri { Pattern::Triangular } else { Pattern::Square };
            let pts = fill_area(&poly, s, pattern, angle, inset).unwrap();
            for (i, p) in pts.iter().enumerate() {
                prop_assert!(contains(&poly, *p));
                prop_assert!(boundary_distance(*p, &poly) >= inset - 1e-3);
                for q in &pts[i + 1..] {
                    prop_assert!(p.distance(*q) >= s * (1.0 - 1e-9) - 1e-6);
                }
            }
        }

        /// Along an edge: first and last heads are the ends, neighbors are at most `spacing` apart
        /// and all heads lie on the path.
        #[test]
        fn along_edge_properties(
            pts in prop::collection::vec((-5000.0f64..5000.0, -5000.0f64..5000.0), 2..6),
            s in 100.0f64..2000.0, closed in any::<bool>(),
        ) {
            let path: Vec<DVec2> = pts.iter().map(|(x, y)| DVec2::new(*x, *y)).collect();
            let len = path_length(&path, closed);
            prop_assume!(len > 1.0);
            let heads = along_edge(&path, s, closed).unwrap();
            let expect = (len / s - 1e-9).ceil().max(1.0) as usize + usize::from(!closed);
            prop_assert_eq!(heads.len(), expect);
            prop_assert!(heads[0].at.distance(path[0]) < 1e-6);
            if !closed {
                prop_assert!(heads.last().unwrap().at.distance(*path.last().unwrap()) < 1e-6);
            }
            let step = len / (expect - usize::from(!closed)) as f64;
            prop_assert!(step <= s + 1e-6);
            for h in &heads {
                // On the path: distance to some segment is 0.
                let segs = if closed { path.len() } else { path.len() - 1 };
                let d = (0..segs)
                    .map(|i| segment_distance(h.at, path[i], path[(i + 1) % path.len()]))
                    .fold(f64::INFINITY, f64::min);
                prop_assert!(d < 1e-6);
            }
        }

        /// Drip lines of a rectangle: pieces lie inside, are `spacing` apart, and their total
        /// length times the spacing is the area, up to one row.
        #[test]
        fn drip_line_properties(
            w in 2000.0f64..20000.0, h in 2000.0f64..20000.0, s in 300.0f64..2000.0,
        ) {
            let poly = rect(w, h);
            let lines = drip_lines(&poly, s, 0.0, 0.0).unwrap();
            for (a, b) in &lines {
                prop_assert!(contains(&poly, *a) && contains(&poly, *b));
                prop_assert!(contains(&poly, (*a + *b) / 2.0));
            }
            for pair in lines.windows(2) {
                prop_assert!(((pair[1].0.y - pair[0].0.y) - s).abs() < 1e-6);
            }
            let total: f64 = lines.iter().map(|(a, b)| a.distance(*b)).sum();
            prop_assert!((total * s - w * h).abs() <= w * s + 1e-3);
        }
    }
}
