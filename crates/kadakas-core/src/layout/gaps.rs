//! Gap center lines of a bond, clipped to a region.

use glam::DVec2;

use super::bond::pattern_lines;
use super::{Pattern, Placement, Region, Segment};

/// Most gap lines that the on-screen drawing makes. A pattern with more lines is left out.
pub const MAX_DRAW_SEGMENTS: usize = 30_000;
/// Most edge tests that one pattern may make. A pattern that needs more is drawn coarser.
pub const WORK_BUDGET: u64 = 20_000_000;

/// The gap lines of a pattern inside a region. `truncated` is true when the pattern would have
/// too many lines: nothing is returned then.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gaps {
    pub segments: Vec<Segment>,
    pub truncated: bool,
    /// The units were made four times larger to fit the limits (only when asked for).
    pub coarsened: bool,
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

/// The edges of a region, sorted into strips along both axes, so that a horizontal or vertical
/// line meets only the edges near it.
struct EdgeIndex<'a> {
    edges: &'a [(DVec2, DVec2)],
    lo: DVec2,
    inv: DVec2,
    bins: usize,
    /// Edges by strip: `strips[0]` along x (for vertical lines), `strips[1]` along y.
    strips: [Vec<Vec<u32>>; 2],
}

impl<'a> EdgeIndex<'a> {
    fn new(edges: &'a [(DVec2, DVec2)], lo: DVec2, hi: DVec2) -> Self {
        let bins = (edges.len() / 2).clamp(16, 2048);
        let size = (hi - lo).max(DVec2::splat(1e-9));
        let inv = DVec2::splat(bins as f64) / size;
        let mut ix = EdgeIndex {
            edges,
            lo,
            inv,
            bins,
            strips: [vec![Vec::new(); bins], vec![Vec::new(); bins]],
        };
        for (i, (p, q)) in edges.iter().enumerate() {
            for axis in 0..2 {
                let (a, b) = (
                    ix.bin(axis, p[axis].min(q[axis])),
                    ix.bin(axis, p[axis].max(q[axis])),
                );
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

    /// The parts of the axis-parallel line inside the region. `fixed_axis` is the axis of the
    /// fixed coordinate (1 for a horizontal line at height `fixed`). `from` and `to` are the ends
    /// along the other axis. `work` counts the edges that were tested.
    fn clip(
        &self,
        fixed_axis: usize,
        fixed: f64,
        from: f64,
        to: f64,
        work: &mut u64,
    ) -> Vec<(f64, f64)> {
        let along = 1 - fixed_axis;
        let strip = &self.strips[fixed_axis][self.bin(fixed_axis, fixed)];
        *work += strip.len() as u64;
        let mut hits: Vec<f64> = Vec::new();
        for &i in strip {
            let (p, q) = self.edges[i as usize];
            if (p[fixed_axis] > fixed) != (q[fixed_axis] > fixed) {
                hits.push(
                    p[along]
                        + (fixed - p[fixed_axis]) * (q[along] - p[along])
                            / (q[fixed_axis] - p[fixed_axis]),
                );
            }
        }
        hits.sort_by(f64::total_cmp);
        hits.as_chunks::<2>()
            .0
            .iter()
            .filter(|w| w[1] > from && w[0] < to)
            .map(|w| (w[0].max(from), w[1].min(to)))
            .filter(|(a, b)| b > a)
            .collect()
    }
}

/// The gap center lines of `pattern` inside the region. The work is limited. With
/// `coarse_fallback`, a pattern that is too fine for the limits is made with units four times
/// larger (the on-screen drawing does this). Without it, the pattern is left out and `truncated`
/// is true: an export or a report never changes the units on its own.
pub fn gap_lines(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    max_segments: usize,
    coarse_fallback: bool,
) -> Gaps {
    if pattern.validate().is_err() || region.shapes.is_empty() {
        return Gaps::default();
    }
    let (origin, angle) = place.resolve(region);
    let turn = DVec2::from_angle(angle.to_radians());
    let unturn = DVec2::from_angle(-angle.to_radians());
    let mut edges: Vec<(DVec2, DVec2)> = Vec::new();
    for ring in region.rings() {
        let n = ring.len();
        for i in 0..n {
            edges.push((
                unturn.rotate(ring[i] - origin),
                unturn.rotate(ring[(i + 1) % n] - origin),
            ));
        }
    }
    let (mut llo, mut lhi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for (a, _) in &edges {
        llo = llo.min(*a);
        lhi = lhi.max(*a);
    }
    let index = EdgeIndex::new(&edges, llo, lhi);
    let tries: &[f64] = if coarse_fallback { &[1.0, 4.0] } else { &[1.0] };
    for &coarse in tries {
        let p = Pattern {
            unit_length: pattern.unit_length * coarse,
            unit_width: pattern.unit_width * coarse,
            ..*pattern
        };
        let Some(lines) = pattern_lines(&p, llo, lhi, max_segments) else {
            continue;
        };
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
                let (c, d) = if axis == 1 {
                    (DVec2::new(s, fixed), DVec2::new(e, fixed))
                } else {
                    (DVec2::new(fixed, s), DVec2::new(fixed, e))
                };
                segments.push((origin + turn.rotate(c), origin + turn.rotate(d)));
            }
        }
        if within_budget && segments.len() <= max_segments {
            return Gaps {
                segments,
                truncated: false,
                coarsened: coarse > 1.0,
            };
        }
    }
    Gaps {
        segments: Vec::new(),
        truncated: true,
        coarsened: false,
    }
}

/// Total length of gap lines.
pub fn length(segments: &[Segment]) -> f64 {
    segments.iter().map(|(a, b)| a.distance(*b)).sum()
}
