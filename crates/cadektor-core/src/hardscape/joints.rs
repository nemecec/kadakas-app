//! Joint patterns: the lines between paving units, clipped to a polygon.

use std::collections::BTreeSet;

use glam::DVec2;

use super::style::{JointKind, JointPattern};

/// Most joint lines that one hardscape draws. A pattern with more lines is left out.
pub const MAX_SEGMENTS: usize = 30_000;
/// Most grid cells that a herringbone pattern covers.
const MAX_CELLS: f64 = 80_000.0;

/// A straight piece of joint.
pub type Segment = (DVec2, DVec2);

/// The joint lines of a pattern inside `poly`. `truncated` is true when the pattern would have
/// too many lines: nothing is drawn then.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Joints {
    pub segments: Vec<Segment>,
    pub truncated: bool,
}

/// Cuts the segment `a`-`b` to the part inside the polygon.
pub fn clip_segment(a: DVec2, b: DVec2, poly: &[DVec2]) -> Vec<Segment> {
    let n = poly.len();
    let r = b - a;
    let mut ts = vec![0.0, 1.0];
    for i in 0..n {
        let (q0, q1) = (poly[i], poly[(i + 1) % n]);
        let s = q1 - q0;
        let denom = r.perp_dot(s);
        if denom.abs() < 1e-18 {
            continue;
        }
        let t = (q0 - a).perp_dot(s) / denom;
        let u = (q0 - a).perp_dot(r) / denom;
        if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u) {
            ts.push(t);
        }
    }
    ts.sort_by(f64::total_cmp);
    let mut out: Vec<Segment> = Vec::new();
    for w in ts.windows(2) {
        if w[1] - w[0] < 1e-12 {
            continue;
        }
        let mid = a + r * ((w[0] + w[1]) / 2.0);
        if crate::geom::point_in_polygon(mid, poly) {
            let seg = (a + r * w[0], a + r * w[1]);
            match out.last_mut() {
                Some(last) if last.1.distance_squared(seg.0) < 1e-12 => last.1 = seg.1,
                _ => out.push(seg),
            }
        }
    }
    out
}

/// Merges unit edges on a grid (integer coordinates) into long lines.
fn merge_grid_edges(edges: BTreeSet<(bool, i64, i64)>) -> Vec<((i64, i64), (i64, i64))> {
    // An edge is (vertical, x, y): from (x, y) to (x + 1, y) or to (x, y + 1).
    // Horizontal edges are grouped by y and run along x. Vertical edges are grouped by x and run along y.
    let mut horizontals: Vec<(i64, i64)> = Vec::new();
    let mut verticals: Vec<(i64, i64)> = Vec::new();
    for (vertical, x, y) in edges {
        if vertical { verticals.push((x, y)) } else { horizontals.push((y, x)) }
    }
    horizontals.sort_unstable();
    verticals.sort_unstable();
    let mut lines = Vec::new();
    for (list, vertical) in [(horizontals, false), (verticals, true)] {
        let mut i = 0;
        while i < list.len() {
            let (fixed, start) = list[i];
            let mut end = start + 1;
            i += 1;
            while i < list.len() && list[i] == (fixed, end) {
                end += 1;
                i += 1;
            }
            lines.push(if vertical { ((fixed, start), (fixed, end)) } else { ((start, fixed), (end, fixed)) });
        }
    }
    lines
}

fn rect_edges(set: &mut BTreeSet<(bool, i64, i64)>, x0: i64, y0: i64, x1: i64, y1: i64) {
    for x in x0..x1 {
        set.insert((false, x, y0));
        set.insert((false, x, y1));
    }
    for y in y0..y1 {
        set.insert((true, x0, y));
        set.insert((true, x1, y));
    }
}

/// Lines of the pattern in its own frame, over the box `lo`..`hi`.
fn pattern_lines(p: &JointPattern, lo: DVec2, hi: DVec2) -> Option<Vec<Segment>> {
    let (lp, wp) = (p.unit_length + p.spacing, p.unit_width + p.spacing);
    let mut out: Vec<Segment> = Vec::new();
    let span = hi - lo;
    match p.kind {
        JointKind::Stack => {
            let (nx, ny) = ((span.x / lp).ceil() + 1.0, (span.y / wp).ceil() + 1.0);
            if nx + ny > MAX_SEGMENTS as f64 {
                return None;
            }
            let (i0, j0) = ((lo.x / lp).floor(), (lo.y / wp).floor());
            for k in 0..=nx as i64 {
                let x = (i0 + k as f64) * lp;
                out.push((DVec2::new(x, lo.y), DVec2::new(x, hi.y)));
            }
            for k in 0..=ny as i64 {
                let y = (j0 + k as f64) * wp;
                out.push((DVec2::new(lo.x, y), DVec2::new(hi.x, y)));
            }
        }
        JointKind::RunningBond => {
            let (nx, ny) = ((span.x / lp).ceil() + 2.0, (span.y / wp).ceil() + 1.0);
            if nx * ny + ny > MAX_SEGMENTS as f64 {
                return None;
            }
            let j0 = (lo.y / wp).floor() as i64;
            let i0 = (lo.x / lp).floor();
            for jj in 0..=ny as i64 {
                let j = j0 + jj;
                let y = j as f64 * wp;
                out.push((DVec2::new(lo.x, y), DVec2::new(hi.x, y)));
                let shift = if j.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
                for k in 0..=nx as i64 {
                    let x = (i0 + k as f64 + shift) * lp;
                    out.push((DVec2::new(x, y), DVec2::new(x, y + wp)));
                }
            }
        }
        JointKind::Herringbone => {
            let cell = wp;
            let k = ((p.unit_length / p.unit_width).round() as i64).max(2);
            let (nx, ny) = ((span.x / cell).ceil() as i64 + 2 * k + 2, (span.y / cell).ceil() as i64 + 2 * k + 2);
            if nx as f64 * ny as f64 > MAX_CELLS {
                return None;
            }
            let (a0, b0) = ((lo.x / cell).floor() as i64 - k, (lo.y / cell).floor() as i64 - k);
            let mut set = BTreeSet::new();
            for a in a0..a0 + nx {
                for b in b0..b0 + ny {
                    match (a - b).rem_euclid(2 * k) {
                        0 => rect_edges(&mut set, a, b, a + k, b + 1),
                        m if m == k => rect_edges(&mut set, a, b - k + 1, a + 1, b + 1),
                        _ => {}
                    }
                }
            }
            for ((x0, y0), (x1, y1)) in merge_grid_edges(set) {
                out.push((DVec2::new(x0 as f64, y0 as f64) * cell, DVec2::new(x1 as f64, y1 as f64) * cell));
            }
        }
        JointKind::BasketWeave => {
            let block = lp.max(2.0 * wp);
            let (nx, ny) = ((span.x / block).ceil() + 2.0, (span.y / block).ceil() + 2.0);
            if nx * ny + nx + ny > MAX_SEGMENTS as f64 {
                return None;
            }
            let (i0, j0) = ((lo.x / block).floor() as i64, (lo.y / block).floor() as i64);
            for ii in 0..=nx as i64 {
                let x = (i0 + ii) as f64 * block;
                out.push((DVec2::new(x, lo.y), DVec2::new(x, hi.y)));
            }
            for jj in 0..=ny as i64 {
                let y = (j0 + jj) as f64 * block;
                out.push((DVec2::new(lo.x, y), DVec2::new(hi.x, y)));
            }
            for ii in 0..nx as i64 {
                for jj in 0..ny as i64 {
                    let (i, j) = (i0 + ii, j0 + jj);
                    let (x, y) = (i as f64 * block, j as f64 * block);
                    if (i + j).rem_euclid(2) == 0 {
                        out.push((DVec2::new(x, y + block / 2.0), DVec2::new(x + block, y + block / 2.0)));
                    } else {
                        out.push((DVec2::new(x + block / 2.0, y), DVec2::new(x + block / 2.0, y + block)));
                    }
                }
            }
        }
    }
    Some(out)
}

/// Most edge tests that one pattern may make. A pattern that needs more is drawn coarser.
pub const WORK_BUDGET: u64 = 20_000_000;

/// The edges of a polygon, sorted into strips along both axes, so that a horizontal or vertical
/// line meets only the edges near it.
struct EdgeIndex<'a> {
    poly: &'a [DVec2],
    lo: DVec2,
    inv: DVec2,
    bins: usize,
    /// Edges by strip: `strips[0]` along x (for vertical lines), `strips[1]` along y.
    strips: [Vec<Vec<u32>>; 2],
}

impl<'a> EdgeIndex<'a> {
    fn new(poly: &'a [DVec2], lo: DVec2, hi: DVec2) -> Self {
        let bins = (poly.len() / 2).clamp(16, 2048);
        let size = (hi - lo).max(DVec2::splat(1e-9));
        let inv = DVec2::splat(bins as f64) / size;
        let mut ix = EdgeIndex { poly, lo, inv, bins, strips: [vec![Vec::new(); bins], vec![Vec::new(); bins]] };
        let n = poly.len();
        for i in 0..n {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            for axis in 0..2 {
                let (a, b) = (ix.bin(axis, p[axis].min(q[axis])), ix.bin(axis, p[axis].max(q[axis])));
                for strip in a..=b {
                    ix.strips[axis][strip].push(i as u32);
                }
            }
        }
        ix
    }

    fn bin(&self, axis: usize, v: f64) -> usize {
        (((v - self.lo[axis]) * self.inv[axis]).floor().max(0.0) as usize).min(self.bins - 1)
    }

    /// The parts of the axis-parallel line inside the polygon. `fixed_axis` is the axis of the
    /// fixed coordinate (1 for a horizontal line at height `fixed`). `from` and `to` are the ends
    /// along the other axis. `work` counts the edges that were tested.
    fn clip(&self, fixed_axis: usize, fixed: f64, from: f64, to: f64, work: &mut u64) -> Vec<(f64, f64)> {
        let along = 1 - fixed_axis;
        let strip = &self.strips[fixed_axis][self.bin(fixed_axis, fixed)];
        *work += strip.len() as u64;
        let n = self.poly.len();
        let mut hits: Vec<f64> = Vec::new();
        for &i in strip {
            let (p, q) = (self.poly[i as usize], self.poly[(i as usize + 1) % n]);
            if (p[fixed_axis] > fixed) != (q[fixed_axis] > fixed) {
                hits.push(p[along] + (fixed - p[fixed_axis]) * (q[along] - p[along]) / (q[fixed_axis] - p[fixed_axis]));
            }
        }
        hits.sort_by(f64::total_cmp);
        hits.chunks_exact(2).filter(|w| w[1] > from && w[0] < to).map(|w| (w[0].max(from), w[1].min(to))).filter(|(a, b)| b > a).collect()
    }
}

/// The joint lines of `pattern` inside the polygon. The pattern starts at the middle of the
/// box of the polygon, so a rectangle gets a symmetric layout. The work is limited: a pattern
/// that is too fine for the polygon is drawn with larger units, and if that does not help, it
/// is left out and `truncated` is true.
pub fn joint_lines(poly: &[DVec2], pattern: &JointPattern) -> Joints {
    if poly.len() < 3 || pattern.unit_length <= 0.0 || pattern.unit_width <= 0.0 {
        return Joints::default();
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in poly {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let center = (lo + hi) / 2.0;
    let turn = DVec2::from_angle(pattern.angle.to_radians());
    let unturn = DVec2::from_angle(-pattern.angle.to_radians());
    let local: Vec<DVec2> = poly.iter().map(|p| unturn.rotate(*p - center)).collect();
    let (mut llo, mut lhi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in &local {
        llo = llo.min(*p);
        lhi = lhi.max(*p);
    }
    let index = EdgeIndex::new(&local, llo, lhi);
    // First the pattern as asked, then with units four times larger.
    for coarse in [1.0, 4.0] {
        let p = JointPattern { unit_length: pattern.unit_length * coarse, unit_width: pattern.unit_width * coarse, ..*pattern };
        let Some(lines) = pattern_lines(&p, llo, lhi) else { continue };
        let mut work = 0u64;
        let mut segments = Vec::new();
        let mut within_budget = true;
        for (a, b) in lines {
            if work > WORK_BUDGET {
                within_budget = false;
                break;
            }
            let (axis, fixed, from, to) = if (a.y - b.y).abs() <= 1e-9 * (1.0 + a.y.abs()) {
                (1, a.y, a.x.min(b.x), a.x.max(b.x))
            } else {
                (0, a.x, a.y.min(b.y), a.y.max(b.y))
            };
            for (s, e) in index.clip(axis, fixed, from, to, &mut work) {
                let (c, d) = if axis == 1 { (DVec2::new(s, fixed), DVec2::new(e, fixed)) } else { (DVec2::new(fixed, s), DVec2::new(fixed, e)) };
                segments.push((center + turn.rotate(c), center + turn.rotate(d)));
            }
        }
        if within_budget && segments.len() <= MAX_SEGMENTS {
            return Joints { segments, truncated: false };
        }
    }
    Joints { segments: Vec::new(), truncated: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Vec<DVec2> {
        vec![DVec2::new(0.0, 0.0), DVec2::new(w, 0.0), DVec2::new(w, h), DVec2::new(0.0, h)]
    }

    fn pat(kind: JointKind) -> JointPattern {
        JointPattern { kind, unit_length: 200.0, unit_width: 100.0, spacing: 0.0, angle: 0.0, color: None }
    }

    fn total_length(j: &Joints) -> f64 {
        j.segments.iter().map(|(a, b)| a.distance(*b)).sum()
    }

    fn inside_all(j: &Joints, poly: &[DVec2]) -> bool {
        let n = poly.len();
        j.segments.iter().all(|(a, b)| {
            let m = (*a + *b) / 2.0;
            let on_edge = (0..n).any(|i| crate::geom::dist_to_segment(m, poly[i], poly[(i + 1) % n]) < 1e-6);
            on_edge || crate::geom::point_in_polygon(m, poly)
        })
    }

    #[test]
    fn stack_lines_cover_a_rectangle_exactly() {
        let j = joint_lines(&rect(1000.0, 500.0), &pat(JointKind::Stack));
        assert!(!j.truncated);
        // Vertical joints every 200 mm and horizontal every 100 mm, with the layout centered.
        // Edges of the polygon may add lines. Interior length: 4 verticals + 4 horizontals plus the
        // boundary lines that the pattern puts on the edge.
        assert!(inside_all(&j, &rect(1000.0, 500.0)));
        assert!(total_length(&j) >= 4.0 * 500.0 + 4.0 * 1000.0);
    }

    #[test]
    fn running_bond_shifts_alternate_rows() {
        let poly = rect(1000.0, 400.0);
        let j = joint_lines(&poly, &pat(JointKind::RunningBond));
        let verticals: Vec<&Segment> = j.segments.iter().filter(|(a, b)| (a.x - b.x).abs() < 1e-9).collect();
        assert!(!verticals.is_empty());
        let mut rows: std::collections::BTreeMap<i64, Vec<f64>> = Default::default();
        for (a, b) in &verticals {
            rows.entry(((a.y + b.y) / 2.0 / 100.0).floor() as i64).or_default().push(a.x);
        }
        let mut keys = rows.keys();
        let (r0, r1) = (&rows[keys.next().unwrap()], &rows[keys.next().unwrap()]);
        let frac = |x: f64| (x / 200.0).rem_euclid(1.0);
        assert!((frac(r0[0]) - frac(r1[0])).abs().min(1.0 - (frac(r0[0]) - frac(r1[0])).abs()) > 0.4, "rows are shifted by half a unit");
        assert!(inside_all(&j, &poly));
    }

    #[test]
    fn every_kind_stays_inside_a_concave_polygon_and_turns() {
        let l = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(3000.0, 0.0),
            DVec2::new(3000.0, 1000.0),
            DVec2::new(1000.0, 1000.0),
            DVec2::new(1000.0, 3000.0),
            DVec2::new(0.0, 3000.0),
        ];
        for kind in JointKind::ALL {
            for angle in [0.0, 30.0, 90.0, -45.0] {
                let p = JointPattern { angle, spacing: 5.0, ..pat(kind) };
                let j = joint_lines(&l, &p);
                assert!(!j.truncated && !j.segments.is_empty(), "{kind:?} {angle}");
                assert!(inside_all(&j, &l), "{kind:?} {angle}");
            }
        }
        // A turn of 90 degrees maps a stack pattern of a square unit onto itself.
        let sq = JointPattern { unit_length: 100.0, ..pat(JointKind::Stack) };
        let a = joint_lines(&rect(1000.0, 1000.0), &sq);
        let b = joint_lines(&rect(1000.0, 1000.0), &JointPattern { angle: 90.0, ..sq });
        assert!((total_length(&a) - total_length(&b)).abs() < 1e-6);
    }

    #[test]
    fn herringbone_bricks_tile_the_plane_without_overlap() {
        // Joint length per area of a tiling by 2x1 bricks: each brick has perimeter 6 and every edge is shared by two bricks.
        // Inside a large rectangle the length per cell area tends to 3 / 2 per brick area (2 cells), i.e. 1.5 per cell.
        let cell = 100.0;
        let poly = rect(20_000.0, 20_000.0);
        let j = joint_lines(&poly, &pat(JointKind::Herringbone));
        let per_cell = total_length(&j) / (20_000.0 * 20_000.0 / (cell * cell)) / cell;
        assert!((per_cell - 1.5).abs() < 0.05, "{per_cell}");
        assert!(inside_all(&j, &poly));
    }

    #[test]
    fn basket_weave_has_blocks_and_midlines() {
        let poly = rect(800.0, 800.0);
        let j = joint_lines(&poly, &pat(JointKind::BasketWeave));
        assert!(!j.segments.is_empty());
        assert!(inside_all(&j, &poly));
    }

    #[test]
    fn too_many_lines_are_left_out() {
        let tiny = JointPattern { unit_length: 10.0, unit_width: 10.0, spacing: 0.0, ..pat(JointKind::RunningBond) };
        for kind in JointKind::ALL {
            let j = joint_lines(&rect(1.0e7, 1.0e7), &JointPattern { kind, ..tiny });
            assert!(j.truncated && j.segments.is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn degenerate_input_gives_nothing() {
        assert_eq!(joint_lines(&[], &pat(JointKind::Stack)), Joints::default());
        let zero = JointPattern { unit_length: 0.0, ..pat(JointKind::Stack) };
        assert_eq!(joint_lines(&rect(10.0, 10.0), &zero), Joints::default());
    }

    #[test]
    fn clip_keeps_inside_parts_only() {
        let poly = rect(100.0, 100.0);
        let s = clip_segment(DVec2::new(-50.0, 50.0), DVec2::new(150.0, 50.0), &poly);
        assert_eq!(s.len(), 1);
        assert!((s[0].0.x - 0.0).abs() < 1e-9 && (s[0].1.x - 100.0).abs() < 1e-9);
        assert!(clip_segment(DVec2::new(-50.0, 200.0), DVec2::new(150.0, 200.0), &poly).is_empty());
        // A U-shape: a line through both arms gives two pieces.
        let u = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(30.0, 0.0),
            DVec2::new(30.0, 60.0),
            DVec2::new(60.0, 60.0),
            DVec2::new(60.0, 0.0),
            DVec2::new(90.0, 0.0),
            DVec2::new(90.0, 100.0),
            DVec2::new(0.0, 100.0),
        ];
        assert_eq!(clip_segment(DVec2::new(-10.0, 30.0), DVec2::new(100.0, 30.0), &u).len(), 2);
    }
}
