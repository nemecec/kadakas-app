//! The check of a pattern (PAT-10): the units of a hardscape, with its zones and courses, tested
//! for the faults that cost money on site.
//!
//! The check lays the units of every group ([`super::plan::Group`]) and tests them as they are,
//! not by trusting how they were made:
//!
//! - **overlap**: two units whose shapes share an area. A unit that the trim kept whole may reach
//!   over an edge by less than the trim, so a thickness of the shared area below `overlap_depth`
//!   (mm, default a little above the trim) does not count. Units of different groups are tested
//!   against each other: a course against the field, a zone against a zone.
//! - **joint too narrow / too wide**: the distance between two units that face each other, found
//!   with a ray from the middle of every edge of a unit, straight out. Units of two zones that meet
//!   at the zone edge have no joint by design and are not reported. A wide joint is only a joint
//!   where the stretch between the units lies inside the area (a hole of the area is no joint).
//! - **small piece**: a cut unit under a share of a full unit, or narrower than a width.
//! - **wrong size**: a whole unit whose area (and, for a rectangle, whose sides) differ from its
//!   unit type.
//! - **outside**: a unit that reaches out of the area of its group by more than the trim, for
//!   example over a zone edge.
//! - **coverage**: the area of the units against the area of the group, and the share of joints
//!   against what the pattern should have. A group whose units cover more than its area is
//!   an error; a group with much more open area than its joints explain has missing units.
//!
//! Every finding has a position, so that an agent can draw the place. The counts are exact; the
//! list of one kind is cut at `max_findings`.

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

use super::plan::{Body, Group, Role};
use super::units::{LayoutError, Limits, Unit};
use super::{Region, noise_area};

/// Most units that one check lays and tests. Measured (`measure_check`): 233,000 units take 0.5 s
/// and 120 MB (1.7 s on a pattern turned by 45 degrees, where the boxes of neighbours overlap), so
/// 500,000 units are about 260 MB and 4 s at most.
pub const MAX_UNITS: usize = 500_000;

/// The options of a check.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckOptions {
    /// An overlap smaller than this (mm2) does not count.
    pub overlap_mm2: f64,
    /// An overlap that is thinner than this (mm) does not count. A unit that crosses an edge by
    /// less than the trim (0.1 mm) is kept whole, so the default is a little more than that.
    pub overlap_depth_mm: f64,
    /// A joint narrower than this (mm) is reported. `None`: the gap of the group less 1 mm.
    pub min_joint_mm: Option<f64>,
    /// A joint wider than this (mm) is reported. `None`: the widest joint of the group plus 1 mm.
    pub max_joint_mm: Option<f64>,
    /// A cut unit with less than this share of a full unit is a small piece.
    pub min_share: f64,
    /// A cut unit narrower than this (mm) is a small piece. 0: off.
    pub min_width_mm: f64,
    /// A whole unit whose sides differ from the unit type by more than this (mm) has the wrong size.
    pub size_tol_mm: f64,
    /// A group whose open area is more than this share (0 to 1) above what the joints explain is
    /// reported.
    pub coverage_tol: f64,
    /// The trim of the layout (mm).
    pub trim: f64,
    /// At most this many findings of one kind are listed. The counts are exact.
    pub max_findings: usize,
    /// At most this many units are laid in all groups together.
    pub max_units: usize,
}

impl Default for CheckOptions {
    fn default() -> Self {
        CheckOptions {
            overlap_mm2: 1.0,
            overlap_depth_mm: 0.15,
            min_joint_mm: None,
            max_joint_mm: None,
            min_share: 0.05,
            min_width_mm: 0.0,
            size_tol_mm: 0.05,
            coverage_tol: 0.02,
            trim: super::units::DEFAULT_TRIM,
            max_findings: 200,
            max_units: MAX_UNITS,
        }
    }
}

/// The kind of a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Overlap,
    JointTooNarrow,
    JointTooWide,
    SmallPiece,
    WrongSize,
    OutsideArea,
    Coverage,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Overlap,
        Kind::JointTooNarrow,
        Kind::JointTooWide,
        Kind::SmallPiece,
        Kind::WrongSize,
        Kind::OutsideArea,
        Kind::Coverage,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Overlap => "overlap",
            Kind::JointTooNarrow => "joint_too_narrow",
            Kind::JointTooWide => "joint_too_wide",
            Kind::SmallPiece => "small_piece",
            Kind::WrongSize => "wrong_size",
            Kind::OutsideArea => "outside_area",
            Kind::Coverage => "coverage",
        }
    }
}

/// One fault, with its place.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    pub kind: Kind,
    /// The group of the unit.
    pub group: String,
    /// The group of the other unit, for an overlap or a joint between two groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_group: Option<String>,
    /// Where it is, in drawing coordinates (mm).
    pub at: [f64; 2],
    /// The size: the overlap area in mm2, the joint width in mm, the share of a full unit, the
    /// area outside in mm2, the difference in mm2, or the share of open area.
    pub size: f64,
    pub message: String,
}

/// The areas of one group.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Coverage {
    pub group: String,
    pub role: super::plan::Role,
    pub region_area_m2: f64,
    pub units_area_m2: f64,
    pub units: usize,
    /// The share of the area that no unit covers.
    pub open_share: f64,
    /// The share that the joints of the pattern should leave open.
    pub expected_open_share: Option<f64>,
}

/// The result of a check.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub units: usize,
    /// The number of findings of each kind (exact).
    pub counts: std::collections::BTreeMap<String, usize>,
    /// The findings, cut at `max_findings` for each kind.
    pub findings: Vec<Finding>,
    /// Kinds whose list was cut.
    pub truncated: Vec<String>,
    pub coverage: Vec<Coverage>,
    /// The area of the whole area, in m2, and the part of it that the units cover.
    pub area_m2: f64,
    pub units_area_m2: f64,
    pub ok: bool,
}

/// The edges of the area of a group, in a grid.
type EdgeGrid = (Grid, Vec<(DVec2, DVec2)>);

struct U {
    group: usize,
    kind: usize,
    whole: bool,
    area: f64,
    share: f64,
    width: f64,
    /// The outline, counterclockwise.
    ring: Vec<DVec2>,
    lo: DVec2,
    hi: DVec2,
    convex: bool,
    /// The index of the unit among the units of its group.
    nth: usize,
}

/// A uniform grid of boxes, to find the boxes near a box.
struct Grid {
    lo: DVec2,
    cell: f64,
    nx: usize,
    ny: usize,
    start: Vec<u32>,
    items: Vec<u32>,
    marks: std::cell::RefCell<(Vec<u32>, u32)>,
}

impl Grid {
    fn build(boxes: &[(DVec2, DVec2)], pad: f64) -> Grid {
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        let mut ext: f64 = 0.0;
        for (a, b) in boxes {
            lo = lo.min(*a);
            hi = hi.max(*b);
            ext = ext.max((*b - *a).max_element());
        }
        if boxes.is_empty() {
            lo = DVec2::ZERO;
            hi = DVec2::ZERO;
        }
        lo -= DVec2::splat(pad);
        hi += DVec2::splat(pad);
        let mut cell = (ext + 2.0 * pad).max(1.0);
        let span = hi - lo;
        while (span.x / cell + 1.0) * (span.y / cell + 1.0) > 8_000_000.0 {
            cell *= 2.0;
        }
        let nx = (span.x / cell).floor() as usize + 1;
        let ny = (span.y / cell).floor() as usize + 1;
        let mut g = Grid {
            lo,
            cell,
            nx,
            ny,
            start: vec![0; nx * ny + 1],
            items: Vec::new(),
            marks: std::cell::RefCell::new((vec![0; boxes.len()], 0)),
        };
        let range = |g: &Grid, a: DVec2, b: DVec2| {
            g.cell_range(a - DVec2::splat(pad), b + DVec2::splat(pad))
        };
        let mut counts = vec![0u32; nx * ny];
        for (a, b) in boxes {
            let ((x0, y0), (x1, y1)) = range(&g, *a, *b);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    counts[y * nx + x] += 1;
                }
            }
        }
        for (c, n) in counts.iter().enumerate() {
            g.start[c + 1] = g.start[c] + n;
        }
        g.items = vec![0; g.start[nx * ny] as usize];
        let mut fill = g.start.clone();
        for (i, (a, b)) in boxes.iter().enumerate() {
            let ((x0, y0), (x1, y1)) = range(&g, *a, *b);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    g.items[fill[y * nx + x] as usize] = i as u32;
                    fill[y * nx + x] += 1;
                }
            }
        }
        g
    }

    fn cell_range(&self, a: DVec2, b: DVec2) -> ((usize, usize), (usize, usize)) {
        let c = |p: DVec2| {
            (
                (((p.x - self.lo.x) / self.cell).floor().max(0.0) as usize).min(self.nx - 1),
                (((p.y - self.lo.y) / self.cell).floor().max(0.0) as usize).min(self.ny - 1),
            )
        };
        (c(a), c(b))
    }

    /// The items whose boxes (grown by the pad) meet the box `a`..`b`, each once.
    fn query(&self, a: DVec2, b: DVec2, out: &mut Vec<u32>) {
        out.clear();
        let ((x0, y0), (x1, y1)) = self.cell_range(a, b);
        let mut m = self.marks.borrow_mut();
        m.1 += 1;
        let now = m.1;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = y * self.nx + x;
                for &i in &self.items[self.start[c] as usize..self.start[c + 1] as usize] {
                    if m.0[i as usize] != now {
                        m.0[i as usize] = now;
                        out.push(i);
                    }
                }
            }
        }
    }
}

fn is_convex(r: &[DVec2]) -> bool {
    let n = r.len();
    n >= 3
        && (0..n).all(|i| {
            let (a, b, c) = (r[i], r[(i + 1) % n], r[(i + 2) % n]);
            (b - a).perp_dot(c - b) >= -1e-9
        })
}

fn polygon_perimeter(r: &[DVec2]) -> f64 {
    (0..r.len())
        .map(|i| r[i].distance(r[(i + 1) % r.len()]))
        .sum()
}

/// The depth by which two convex polygons overlap: 0 if they are apart or only touch.
fn sat_depth(a: &[DVec2], b: &[DVec2]) -> f64 {
    let mut depth = f64::INFINITY;
    for poly in [a, b] {
        let n = poly.len();
        for i in 0..n {
            let e = poly[(i + 1) % n] - poly[i];
            let Some(axis) = e.perp().try_normalize() else {
                continue;
            };
            let proj = |p: &[DVec2]| {
                p.iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| {
                        let d = q.dot(axis);
                        (lo.min(d), hi.max(d))
                    })
            };
            let (pa, pb) = (proj(a), proj(b));
            let overlap = pa.1.min(pb.1) - pa.0.max(pb.0);
            if overlap <= 0.0 {
                return 0.0;
            }
            depth = depth.min(overlap);
        }
    }
    if depth.is_finite() { depth } else { 0.0 }
}

fn centroid(r: &[DVec2]) -> DVec2 {
    r.iter().copied().sum::<DVec2>() / r.len().max(1) as f64
}

fn shapes_of(a: &[DVec2], b: &[DVec2]) -> Vec<Vec<Vec<[f64; 2]>>> {
    let p = |r: &[DVec2]| vec![r.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()];
    p(a).overlay(&p(b), OverlayRule::Intersect, FillRule::NonZero)
}

/// What a ray from the edge of a unit meets.
enum Hit {
    /// The nearest edge of another unit: the distance and the unit.
    Edge(f64, usize),
    /// The ray starts inside a unit of its own group, so the units touch or overlap.
    Touch(usize),
    /// The ray starts inside a unit of another group (a trimmed unit over a zone edge).
    Skip,
}

/// The nearest hit of a ray from `p` along `n` (a unit vector) on the edges of the rings of the
/// units in `cand`, other than `skip`, up to `max` mm.
fn ray_hit(p: DVec2, n: DVec2, max: f64, cand: &[u32], skip: usize, us: &[U]) -> Option<Hit> {
    let mut best: Option<(f64, usize)> = None;
    let start = p + n * 0.01;
    for &j in cand {
        let j = j as usize;
        if j == skip {
            continue;
        }
        let r = &us[j].ring;
        if start.x >= us[j].lo.x
            && start.x <= us[j].hi.x
            && start.y >= us[j].lo.y
            && start.y <= us[j].hi.y
            && crate::geom::point_in_polygon(start, r)
        {
            return Some(if us[j].group == us[skip].group {
                Hit::Touch(j)
            } else {
                Hit::Skip
            });
        }
        let m = r.len();
        for k in 0..m {
            let (a, b) = (r[k], r[(k + 1) % m]);
            let e = b - a;
            let den = n.perp_dot(e);
            if den.abs() < 1e-12 {
                continue;
            }
            let w = a - p;
            let t = w.perp_dot(e) / den;
            let s = w.perp_dot(n) / den;
            if t >= -1e-9 && t <= max && (-1e-9..=1.0 + 1e-9).contains(&s) {
                let t = t.max(0.0);
                if best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, j));
                }
            }
        }
    }
    best.map(|(t, j)| Hit::Edge(t, j))
}

/// Checks the units of the groups. `area` is the whole area of the hardscape (where the stretch
/// between two units must lie for a wide joint to count).
pub fn check(groups: &[Group], area: &Region, opts: &CheckOptions) -> Result<Report, LayoutError> {
    check_cancellable(groups, area, opts, &AtomicBool::new(false))
}

/// The message of the error that [`check_cancellable`] gives when `cancel` was set.
pub const CANCELLED: &str = "the check was cancelled";

/// Like [`check`]. The flag `cancel` is read between the steps and in the long loops; when it is
/// set the check stops and gives `LayoutError::Invalid(CANCELLED)`. A caller that runs the check
/// in the background sets it when the result is not wanted any more.
pub fn check_cancellable(
    groups: &[Group],
    area: &Region,
    opts: &CheckOptions,
    cancel: &AtomicBool,
) -> Result<Report, LayoutError> {
    let stopped = || cancel.load(Ordering::Relaxed);
    let cancelled = || LayoutError::Invalid(CANCELLED.to_string());
    // Lay every group.
    let mut us: Vec<U> = Vec::new();
    let mut per_group: Vec<(usize, f64)> = vec![(0, 0.0); groups.len()];
    let mut budget = opts.max_units;
    let mut kind_full: Vec<Vec<f64>> = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        if stopped() {
            return Err(cancelled());
        }
        let limits = Limits {
            max_units: budget,
            trim: opts.trim,
        };
        let mut nth = 0usize;
        let stats = g.for_each_unit(&limits, &mut |u: &Unit| {
            if stopped() {
                return;
            }
            let Some(outer) = u.rings.first() else { return };
            let mut ring = outer.points.clone();
            if crate::geom::signed_area(&ring) < 0.0 {
                ring.reverse();
            }
            let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            for q in &ring {
                lo = lo.min(*q);
                hi = hi.max(*q);
            }
            let convex = u.rings.len() == 1 && is_convex(&ring);
            us.push(U {
                group: gi,
                kind: u.kind,
                whole: u.whole,
                area: u.area,
                share: u.share,
                width: u.width,
                ring,
                lo,
                hi,
                convex,
                nth,
            });
            nth += 1;
        })?;
        budget = budget.saturating_sub(stats.units);
        per_group[gi] = (nth, us[us.len() - nth..].iter().map(|u| u.area).sum());
        kind_full.push(stats.kinds.iter().map(|k| k.1).collect());
    }
    let names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
    let mut report = Report {
        units: us.len(),
        area_m2: area.area / 1e6,
        units_area_m2: us.iter().map(|u| u.area).sum::<f64>() / 1e6,
        ..Default::default()
    };
    let mut counts = [0usize; 7];
    let idx = |k: Kind| Kind::ALL.iter().position(|x| *x == k).unwrap_or(0);
    let mut findings: Vec<Finding> = Vec::new();
    // Every kind keeps its own count of listed findings.
    let mut add = |k: Kind, f: Finding, counts: &mut [usize; 7]| {
        counts[idx(k)] += 1;
        if counts[idx(k)] <= opts.max_findings {
            findings.push(f);
        }
    };
    let boxes: Vec<(DVec2, DVec2)> = us.iter().map(|u| (u.lo, u.hi)).collect();
    // The widest joint a ray looks for.
    let max_joint_of = |g: usize| {
        opts.max_joint_mm
            .unwrap_or(groups[g].widest_joint() + 1.0)
            .max(0.0)
    };
    let largest = us
        .iter()
        .map(|u| (u.hi - u.lo).max_element())
        .fold(0.0, f64::max);
    let probe = (2.0 * largest).max(20.0);
    let grid = Grid::build(&boxes, 0.0);
    let mut cand: Vec<u32> = Vec::new();
    // Overlaps.
    for i in 0..us.len() {
        if i % 512 == 0 && stopped() {
            return Err(cancelled());
        }
        let a = &us[i];
        grid.query(a.lo, a.hi, &mut cand);
        for &jj in &cand {
            let j = jj as usize;
            if j <= i {
                continue;
            }
            let (ua, ub) = (&us[i], &us[j]);
            let ox = ua.hi.x.min(ub.hi.x) - ua.lo.x.max(ub.lo.x);
            let oy = ua.hi.y.min(ub.hi.y) - ua.lo.y.max(ub.lo.y);
            if ox <= opts.overlap_depth_mm.min(1e-3) || oy <= opts.overlap_depth_mm.min(1e-3) {
                continue;
            }
            if ua.convex && ub.convex && sat_depth(&ua.ring, &ub.ring) <= opts.overlap_depth_mm {
                continue;
            }
            // Exact: the shape that the two share.
            let mut area_sum = 0.0;
            let mut thick: f64 = 0.0;
            let mut at = DVec2::ZERO;
            for s in shapes_of(&ua.ring, &ub.ring) {
                let Some(outer) = s.first() else { continue };
                let pts: Vec<DVec2> = outer.iter().map(|q| DVec2::new(q[0], q[1])).collect();
                let a_ = crate::geom::polygon_area(&pts);
                let p_ = polygon_perimeter(&pts);
                if p_ > 0.0 {
                    let t = 2.0 * a_ / p_;
                    if t > thick {
                        thick = t;
                        at = centroid(&pts);
                    }
                }
                area_sum += a_;
            }
            if area_sum > opts.overlap_mm2 && thick > opts.overlap_depth_mm {
                let (ga, gb) = (ua.group, ub.group);
                add(
                    Kind::Overlap,
                    Finding {
                        kind: Kind::Overlap,
                        group: names[ga].clone(),
                        other_group: (ga != gb).then(|| names[gb].clone()),
                        at: [at.x, at.y],
                        size: area_sum,
                        message: format!(
                            "unit {} of '{}' and unit {} of '{}' overlap by {:.1} mm2 (about {:.2} mm thick)",
                            ua.nth + 1,
                            names[ga],
                            ub.nth + 1,
                            names[gb],
                            area_sum,
                            thick
                        ),
                    },
                    &mut counts,
                );
            }
        }
    }
    // Per unit: small pieces, wrong sizes, outside the area of the group.
    let mut edge_grids: Vec<Option<EdgeGrid>> = Vec::new();
    for g in groups {
        let mut edges: Vec<(DVec2, DVec2)> = Vec::new();
        for r in g.region.rings() {
            for k in 0..r.len() {
                edges.push((r[k], r[(k + 1) % r.len()]));
            }
        }
        let bx: Vec<(DVec2, DVec2)> = edges.iter().map(|(a, b)| (a.min(*b), a.max(*b))).collect();
        edge_grids.push(Some((Grid::build(&bx, 0.0), edges)));
    }
    for (n, u) in us.iter().enumerate() {
        if n % 512 == 0 && stopped() {
            return Err(cancelled());
        }
        let g = &groups[u.group];
        let c = centroid(&u.ring);
        // Small pieces.
        if !u.whole
            && (u.share < opts.min_share
                || (opts.min_width_mm > 0.0 && u.width < opts.min_width_mm))
        {
            add(
                Kind::SmallPiece,
                Finding {
                    kind: Kind::SmallPiece,
                    group: g.name.clone(),
                    other_group: None,
                    at: [c.x, c.y],
                    size: u.share,
                    message: format!(
                        "unit {} of '{}' is a piece of {:.1} percent of a full unit, {:.1} mm wide",
                        u.nth + 1,
                        g.name,
                        u.share * 100.0,
                        u.width
                    ),
                },
                &mut counts,
            );
        }
        // Wrong size.
        if u.whole {
            let full = kind_full[u.group].get(u.kind).copied().unwrap_or(u.area);
            let mut bad = (u.area - full).abs() > 1e-6 * full.max(1.0);
            if !bad && u.ring.len() == 4 {
                let (s0, s1) = (u.ring[0].distance(u.ring[1]), u.ring[1].distance(u.ring[2]));
                let expect = match &g.body {
                    // A generator has no unit size of its own: its cells have the sizes the script gave.
                    Body::Pattern { pattern, .. }
                        if pattern.module.is_none()
                            && pattern.bond != crate::layout::Bond::Generator =>
                    {
                        Some((pattern.unit_length, pattern.unit_width))
                    }
                    Body::Course(cb) if u.kind == 0 => Some((cb.along, cb.depth)),
                    _ => None,
                };
                if let Some((l, w)) = expect {
                    let (mut got, mut want) = ([s0, s1], [l, w]);
                    got.sort_by(f64::total_cmp);
                    want.sort_by(f64::total_cmp);
                    bad = (got[0] - want[0]).abs() > opts.size_tol_mm
                        || (got[1] - want[1]).abs() > opts.size_tol_mm;
                }
            }
            if bad {
                add(
                    Kind::WrongSize,
                    Finding {
                        kind: Kind::WrongSize,
                        group: g.name.clone(),
                        other_group: None,
                        at: [c.x, c.y],
                        size: (u.area - full).abs(),
                        message: format!(
                            "whole unit {} of '{}' has an area of {:.1} mm2, not {:.1} mm2",
                            u.nth + 1,
                            g.name,
                            u.area,
                            full
                        ),
                    },
                    &mut counts,
                );
            }
        }
        // Outside the area of the group: only units near an edge of it need the exact test.
        if let Some((eg, edges)) = &edge_grids[u.group] {
            let mut near: Vec<u32> = Vec::new();
            eg.query(u.lo, u.hi, &mut near);
            let touches = near.iter().any(|&e| {
                let (a, b) = edges[e as usize];
                let (elo, ehi) = (a.min(b), a.max(b));
                elo.x <= u.hi.x && ehi.x >= u.lo.x && elo.y <= u.hi.y && ehi.y >= u.lo.y
            });
            let inside_probe = || {
                g.region.shapes.iter().any(|s| {
                    s.first()
                        .is_some_and(|o| crate::geom::point_in_polygon(c, o))
                })
            };
            if touches {
                let outside = outside_area(&u.ring, &g.region);
                let perimeter = polygon_perimeter(&u.ring);
                if outside > (opts.trim + 0.02) * perimeter + 1e-6 {
                    add(
                        Kind::OutsideArea,
                        Finding {
                            kind: Kind::OutsideArea,
                            group: g.name.clone(),
                            other_group: None,
                            at: [c.x, c.y],
                            size: outside,
                            message: format!(
                                "unit {} of '{}' reaches {:.1} mm2 out of its area",
                                u.nth + 1,
                                g.name,
                                outside
                            ),
                        },
                        &mut counts,
                    );
                }
            } else if !inside_probe() {
                add(
                    Kind::OutsideArea,
                    Finding {
                        kind: Kind::OutsideArea,
                        group: g.name.clone(),
                        other_group: None,
                        at: [c.x, c.y],
                        size: u.area,
                        message: format!(
                            "unit {} of '{}' lies outside its area",
                            u.nth + 1,
                            g.name
                        ),
                    },
                    &mut counts,
                );
            }
        }
    }
    // Joints: a ray from the middle of every edge, straight out.
    let group_edges_near = |gi: usize, p: DVec2| -> bool {
        edge_grids[gi].as_ref().is_some_and(|(eg, edges)| {
            let mut near: Vec<u32> = Vec::new();
            eg.query(p - DVec2::splat(0.3), p + DVec2::splat(0.3), &mut near);
            near.iter().any(|&e| {
                let (a, b) = edges[e as usize];
                crate::geom::dist_to_segment(p, a, b) < 0.2
            })
        })
    };
    let mut told: std::collections::HashSet<(usize, usize)> = Default::default();
    for (i, u) in us.iter().enumerate() {
        if i % 512 == 0 && stopped() {
            return Err(cancelled());
        }
        let n = u.ring.len();
        let (min_j, max_j) = (
            opts.min_joint_mm
                .unwrap_or((groups[u.group].gap() - 1.0).max(0.0)),
            max_joint_of(u.group),
        );
        for k in 0..n {
            let (a, b) = (u.ring[k], u.ring[(k + 1) % n]);
            let e = b - a;
            if e.length() < 1.0 {
                continue;
            }
            // Counterclockwise ring: the outside is on the right.
            let Some(out) = (-e.perp()).try_normalize() else {
                continue;
            };
            // Three rays along the edge: a ray may run down the cross joint of the next row, so
            // the neighbour of the edge is the nearest unit any of them meets.
            let mut best: Option<(f64, usize, DVec2)> = None;
            let mut skip_edge = false;
            for frac in [0.2, 0.5, 0.8] {
                let mid = a + e * frac;
                let reach = mid + out * probe;
                grid.query(mid.min(reach), mid.max(reach), &mut cand);
                let (d, j) = match ray_hit(mid, out, probe, &cand, i, &us) {
                    None => continue,
                    Some(Hit::Skip) => {
                        skip_edge = true;
                        break;
                    }
                    Some(Hit::Touch(j)) => (0.0, j),
                    Some(Hit::Edge(d, j)) => (d, j),
                };
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, j, mid));
                }
            }
            let Some((d, j, mid)) = best.filter(|_| !skip_edge) else {
                continue;
            };
            // The other unit may find the same joint from its side: a joint is told once.
            if !told.insert((i.min(j), i.max(j))) {
                continue;
            }
            let other = &us[j];
            // Two groups meet with the joint of the wider one.
            let (min_j, max_j) = if u.group == other.group {
                (min_j, max_j)
            } else {
                (
                    min_j.min(
                        opts.min_joint_mm
                            .unwrap_or((groups[other.group].gap() - 1.0).max(0.0)),
                    ),
                    max_j.max(max_joint_of(other.group)),
                )
            };
            let at = mid + out * (d / 2.0);
            if d < min_j {
                // Units of two groups that meet at the edge of a zone have no joint by design.
                let meets_by_design = |r: Role| matches!(r, Role::Zone | Role::Field);
                let abut = u.group != other.group
                    && meets_by_design(groups[u.group].role)
                    && meets_by_design(groups[other.group].role)
                    && d < 0.2
                    && group_edges_near(u.group, mid);
                if !abut {
                    add(
                        Kind::JointTooNarrow,
                        Finding {
                            kind: Kind::JointTooNarrow,
                            group: names[u.group].clone(),
                            other_group: (u.group != other.group)
                                .then(|| names[other.group].clone()),
                            at: [at.x, at.y],
                            size: d,
                            message: format!(
                                "the joint between unit {} of '{}' and unit {} of '{}' is {:.2} mm, narrower than {:.2} mm",
                                u.nth + 1,
                                names[u.group],
                                other.nth + 1,
                                names[other.group],
                                d,
                                min_j
                            ),
                        },
                        &mut counts,
                    );
                }
            } else if d > max_j {
                // A stretch that lies outside the area (across a hole) is no joint.
                let inside = area.shapes.iter().any(|s| {
                    s.first()
                        .is_some_and(|o| crate::geom::point_in_polygon(at, o))
                        && !s
                            .iter()
                            .skip(1)
                            .any(|h| crate::geom::point_in_polygon(at, h))
                });
                if inside {
                    add(
                        Kind::JointTooWide,
                        Finding {
                            kind: Kind::JointTooWide,
                            group: names[u.group].clone(),
                            other_group: (u.group != other.group)
                                .then(|| names[other.group].clone()),
                            at: [at.x, at.y],
                            size: d,
                            message: format!(
                                "the joint between unit {} of '{}' and unit {} of '{}' is {:.2} mm, wider than {:.2} mm",
                                u.nth + 1,
                                names[u.group],
                                other.nth + 1,
                                names[other.group],
                                d,
                                max_j
                            ),
                        },
                        &mut counts,
                    );
                }
            }
        }
    }
    // Coverage per group.
    for (gi, g) in groups.iter().enumerate() {
        let (n, laid) = per_group[gi];
        let region = g.area();
        let open = if region > 0.0 {
            (1.0 - laid / region).max(0.0)
        } else {
            0.0
        };
        let expected = match &g.body {
            Body::Pattern { pattern, .. } => pattern.compile().ok().map(|c| {
                let covered: f64 =
                    c.kinds.iter().map(|k| k.area).sum::<f64>() / c.det.abs().max(1e-9);
                (1.0 - covered).max(0.0)
            }),
            Body::Course(cb) => Some(cb.gap / (cb.along + cb.gap)),
        };
        // A course lies inside the area of the hardscape and covers a part of it: its share is
        // not meaningful against the whole area.
        let meaningful = !matches!(g.body, Body::Course(_));
        report.coverage.push(Coverage {
            group: g.name.clone(),
            role: g.role,
            region_area_m2: region / 1e6,
            units_area_m2: laid / 1e6,
            units: n,
            open_share: if meaningful { open } else { 0.0 },
            expected_open_share: expected,
        });
        // A zone that owns its units by their centers may hold units that reach over its edge.
        let ring = match &g.owner {
            Some(o) => &o.zones[o.index].shapes[0][0],
            None => &g.region.shapes[0][0],
        };
        let c = centroid(ring);
        if laid > region * (1.0 + 1e-6) + noise_area(ring) && meaningful && g.owner.is_none() {
            add(
                Kind::Coverage,
                Finding {
                    kind: Kind::Coverage,
                    group: g.name.clone(),
                    other_group: None,
                    at: [c.x, c.y],
                    size: (laid - region) / 1e6,
                    message: format!(
                        "the units of '{}' cover {:.3} m2, more than its area of {:.3} m2",
                        g.name,
                        laid / 1e6,
                        region / 1e6
                    ),
                },
                &mut counts,
            );
        } else if let Some(exp) = expected.filter(|_| meaningful)
            && n > 0
            && open - exp > opts.coverage_tol
        {
            add(
                Kind::Coverage,
                Finding {
                    kind: Kind::Coverage,
                    group: g.name.clone(),
                    other_group: None,
                    at: [c.x, c.y],
                    size: open,
                    message: format!(
                        "'{}' is {:.1} percent open, but its joints explain {:.1} percent: units may be missing",
                        g.name,
                        open * 100.0,
                        exp * 100.0
                    ),
                },
                &mut counts,
            );
        }
    }
    // A course must fill its band: a hole in it is area that the field gave up and no unit has.
    for g in groups {
        let Body::Course(cb) = &g.body else { continue };
        if cb.band.is_empty() {
            continue;
        }
        let ring = |r: &Vec<DVec2>| r.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
        let band: Vec<Vec<[f64; 2]>> = cb.band.iter().flatten().map(ring).collect();
        let reg: Vec<Vec<[f64; 2]>> = g.region.rings().map(ring).collect();
        let inside = band.overlay(&reg, OverlayRule::Intersect, FillRule::EvenOdd);
        let margin = cb.gap * 1.02 + 0.05;
        let dilated: Vec<Vec<[f64; 2]>> = cb
            .pieces
            .iter()
            .map(|p| {
                let mut d = super::course::dilate(&p.poly, margin);
                if crate::geom::signed_area(&d) < 0.0 {
                    d.reverse();
                }
                d.iter().map(|q| [q.x, q.y]).collect()
            })
            .collect();
        let empty: Vec<Vec<[f64; 2]>> = Vec::new();
        let covered = dilated.overlay(&empty, OverlayRule::Union, FillRule::NonZero);
        let covered: Vec<Vec<[f64; 2]>> = covered.into_iter().flatten().collect();
        let subject: Vec<Vec<[f64; 2]>> = inside.into_iter().flatten().collect();
        let left = subject.overlay(&covered, OverlayRule::Difference, FillRule::EvenOdd);
        // A hole is a piece of the band that no unit or joint covers and that is more than a
        // twentieth of a unit. The limit does not depend on the length of the course.
        let limit = 0.05 * cb.along * cb.depth;
        for s in &left {
            let mut a = 0.0;
            let mut c = DVec2::ZERO;
            for (i, r) in s.iter().enumerate() {
                let pts: Vec<DVec2> = r.iter().map(|q| DVec2::new(q[0], q[1])).collect();
                let ar = crate::geom::polygon_area(&pts);
                if i == 0 {
                    a += ar;
                    c = centroid(&pts);
                } else {
                    a -= ar;
                }
            }
            if a > limit {
                add(
                    Kind::Coverage,
                    Finding {
                        kind: Kind::Coverage,
                        group: g.name.clone(),
                        other_group: None,
                        at: [c.x, c.y],
                        size: a,
                        message: format!(
                            "the course '{}' has a hole of {:.0} mm2 without a unit or a joint",
                            g.name, a
                        ),
                    },
                    &mut counts,
                );
            }
        }
    }
    for k in Kind::ALL {
        let c = counts[idx(k)];
        report.counts.insert(k.id().into(), c);
        if c > opts.max_findings {
            report.truncated.push(k.id().into());
        }
    }
    report.ok = counts.iter().all(|c| *c == 0);
    report.findings = findings;
    Ok(report)
}

/// The area of the unit outside the region, in mm2.
fn outside_area(ring: &[DVec2], region: &Region) -> f64 {
    let subject = vec![ring.iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()];
    let clip: Vec<Vec<[f64; 2]>> = region
        .rings()
        .map(|r| r.iter().map(|q| [q.x, q.y]).collect())
        .collect();
    subject
        .overlay(&clip, OverlayRule::Difference, FillRule::EvenOdd)
        .iter()
        .map(|s| {
            s.iter()
                .enumerate()
                .map(|(i, r)| {
                    let pts: Vec<DVec2> = r.iter().map(|q| DVec2::new(q[0], q[1])).collect();
                    let a = crate::geom::polygon_area(&pts);
                    if i == 0 { a } else { -a }
                })
                .sum::<f64>()
        })
        .sum()
}

#[cfg(test)]
mod tests;
