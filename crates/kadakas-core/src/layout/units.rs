//! Units of a bond, clipped exactly to a region.

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use super::bond::{self, Cell};
use super::{DUST_AREA, Pattern, Placement, Region};

/// One ring of a unit: an outline, or a hole in it.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitRing {
    pub points: Vec<DVec2>,
    pub hole: bool,
}

/// A unit: a whole one, or a cut one. The rings are the outline of the unit first, then holes in
/// it (a hole of the region that lies inside the unit). A unit that the region edge splits in
/// two pieces counts as two units.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub rings: Vec<UnitRing>,
    pub whole: bool,
    /// The area inside the region, in mm2 (holes of the unit taken off).
    pub area: f64,
    /// The area as a part of a full unit: 1 for a whole unit.
    pub share: f64,
}

/// What a layout found, besides the units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub origin: DVec2,
    pub angle: f64,
    pub full_unit_area: f64,
    pub region_area: f64,
    pub units: usize,
    /// Clipped pieces smaller than [`DUST_AREA`]: rounding dust, not counted as units.
    pub dust_pieces: usize,
}

/// More units than the limit allows. Nothing was made.
#[derive(Clone, Debug, PartialEq)]
pub struct TooMany {
    /// About how many units the job makes.
    pub estimated: u64,
    pub max: usize,
}

impl std::fmt::Display for TooMany {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the pattern makes about {} units, more than the limit of {}",
            self.estimated, self.max
        )
    }
}

impl std::error::Error for TooMany {}

/// Counts of units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub whole: usize,
    pub cut: usize,
    /// Cut units with a share below the sliver share.
    pub slivers: usize,
    /// The area of all units, in mm2.
    pub area: f64,
}

impl Tally {
    pub fn add(&mut self, u: &Unit, sliver_share: f64) {
        if u.whole {
            self.whole += 1;
        } else {
            self.cut += 1;
            if u.share < sliver_share {
                self.slivers += 1;
            }
        }
        self.area += u.area;
    }
}

struct Index {
    edges: Vec<(DVec2, DVec2)>,
    lo: DVec2,
    size: f64,
    nx: usize,
    ny: usize,
    start: Vec<u32>,
    items: Vec<u32>,
}

impl Index {
    fn new(edges: Vec<(DVec2, DVec2)>, lo: DVec2, hi: DVec2, size: f64) -> Index {
        let nx = (((hi.x - lo.x) / size).floor() as usize + 1).max(1);
        let ny = (((hi.y - lo.y) / size).floor() as usize + 1).max(1);
        let mut ix = Index {
            edges,
            lo,
            size,
            nx,
            ny,
            start: vec![0; nx * ny + 1],
            items: Vec::new(),
        };
        let cells = nx * ny;
        // Pass 1 counts, pass 2 fills.
        let mut counts = vec![0u32; cells];
        let mut last = vec![u32::MAX; cells];
        for e in 0..ix.edges.len() {
            ix.cells_of(e, &mut |c| {
                if last[c] != e as u32 {
                    last[c] = e as u32;
                    counts[c] += 1;
                }
            });
        }
        for c in 0..cells {
            ix.start[c + 1] = ix.start[c] + counts[c];
        }
        ix.items = vec![0; ix.start[cells] as usize];
        let mut fill = ix.start.clone();
        last.fill(u32::MAX);
        for e in 0..ix.edges.len() {
            let mut hits: Vec<usize> = Vec::new();
            ix.cells_of(e, &mut |c| hits.push(c));
            for c in hits {
                if last[c] != e as u32 {
                    last[c] = e as u32;
                    ix.items[fill[c] as usize] = e as u32;
                    fill[c] += 1;
                }
            }
        }
        ix
    }

    fn cell_xy(&self, p: DVec2) -> (usize, usize) {
        (
            (((p.x - self.lo.x) / self.size).floor().max(0.0) as usize).min(self.nx - 1),
            (((p.y - self.lo.y) / self.size).floor().max(0.0) as usize).min(self.ny - 1),
        )
    }

    /// Cells that edge `e` may touch (a few extra cells, never too few).
    fn cells_of(&self, e: usize, f: &mut dyn FnMut(usize)) {
        let (a, b) = self.edges[e];
        let ((x0, y0), (x1, y1)) = (self.cell_xy(a.min(b)), self.cell_xy(a.max(b)));
        if (x1 - x0 + 1) * (y1 - y0 + 1) <= 16 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    f(y * self.nx + x);
                }
            }
            return;
        }
        let steps = (a.distance(b) / (self.size / 2.0)).ceil() as usize + 1;
        for s in 0..=steps {
            let p = a.lerp(b, s as f64 / steps as f64);
            let (cx, cy) = self.cell_xy(p);
            for y in cy.saturating_sub(1)..=(cy + 1).min(self.ny - 1) {
                for x in cx.saturating_sub(1)..=(cx + 1).min(self.nx - 1) {
                    f(y * self.nx + x);
                }
            }
        }
    }

    fn cell_items(&self, c: usize) -> &[u32] {
        &self.items[self.start[c] as usize..self.start[c + 1] as usize]
    }

    /// True if `p` is inside the region (even-odd, a ray to -X).
    fn inside(&self, p: DVec2, stamp: &mut Stamp) -> bool {
        let (cx, cy) = self.cell_xy(p);
        stamp.next();
        let mut inside = false;
        for x in 0..=cx {
            for &e in self.cell_items(cy * self.nx + x) {
                if stamp.seen(e) {
                    continue;
                }
                let (a, b) = self.edges[e as usize];
                if (a.y > p.y) != (b.y > p.y) {
                    let xi = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                    if xi < p.x {
                        inside = !inside;
                    }
                }
            }
        }
        inside
    }
}

/// Marks of edges seen in the current query.
struct Stamp {
    marks: Vec<u32>,
    now: u32,
}

impl Stamp {
    fn new(n: usize) -> Stamp {
        Stamp {
            marks: vec![0; n],
            now: 0,
        }
    }

    fn next(&mut self) {
        self.now += 1;
    }

    /// True if the edge was seen already in this query. Marks it.
    fn seen(&mut self, e: u32) -> bool {
        let m = &mut self.marks[e as usize];
        let was = *m == self.now;
        *m = self.now;
        was
    }
}

fn seg_hits_rect(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - lo.x),
        (d.x, hi.x - a.x),
        (-d.y, a.y - lo.y),
        (d.y, hi.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return false;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return false;
                }
                t1 = t1.min(r);
            }
        }
    }
    true
}

/// Sutherland-Hodgman: a ring cut to a box.
fn clip_ring(ring: &[DVec2], lo: DVec2, hi: DVec2) -> Vec<DVec2> {
    let mut pts = ring.to_vec();
    for side in 0..4 {
        if pts.is_empty() {
            break;
        }
        let inside = |p: DVec2| match side {
            0 => p.x >= lo.x,
            1 => p.x <= hi.x,
            2 => p.y >= lo.y,
            _ => p.y <= hi.y,
        };
        let cut = |a: DVec2, b: DVec2| match side {
            0 | 1 => {
                let x = if side == 0 { lo.x } else { hi.x };
                DVec2::new(x, a.y + (b.y - a.y) * (x - a.x) / (b.x - a.x))
            }
            _ => {
                let y = if side == 2 { lo.y } else { hi.y };
                DVec2::new(a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y), y)
            }
        };
        let mut out = Vec::with_capacity(pts.len() + 2);
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            match (inside(a), inside(b)) {
                (true, true) => out.push(b),
                (true, false) => out.push(cut(a, b)),
                (false, true) => {
                    out.push(cut(a, b));
                    out.push(b);
                }
                (false, false) => {}
            }
        }
        pts = out;
    }
    pts
}

fn ring_area(r: &[DVec2]) -> f64 {
    (0..r.len())
        .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
        .sum::<f64>()
        / 2.0
}

/// Lays the units of a pattern over a region and calls `f` for each. The units are in the order of
/// the rows of the pattern. If the job makes more than `max_units`, the function returns an error;
/// `f` may have been called before that, so a caller that gets an error discards what it got.
pub fn for_each_unit(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    max_units: usize,
    f: &mut dyn FnMut(&Unit),
) -> Result<Stats, super::units::TooMany> {
    let full = pattern.full_unit_area();
    let (origin, angle) = place.resolve(region);
    let mut stats = Stats {
        origin,
        angle,
        full_unit_area: full,
        region_area: region.area,
        units: 0,
        dust_pieces: 0,
    };
    if pattern.validate().is_err() || full <= 0.0 {
        return Ok(stats);
    }
    let cell_area = {
        let (lp, wp) = (
            pattern.unit_length + pattern.gap,
            pattern.unit_width + pattern.gap,
        );
        lp * wp
    };
    let estimated = (region.area / cell_area.max(1e-9)).ceil() as u64;
    let too_many = || TooMany {
        estimated,
        max: max_units,
    };
    if estimated > max_units as u64 {
        return Err(too_many());
    }
    let turn = DVec2::from_angle(angle.to_radians());
    let unturn = DVec2::from_angle(-angle.to_radians());
    let local: Vec<Vec<DVec2>> = region
        .rings()
        .map(|r| r.iter().map(|p| unturn.rotate(*p - origin)).collect())
        .collect();
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for r in &local {
        for p in r {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    // The job in the box of the region may be larger than the region (a long, thin, diagonal shape).
    let steps = bond::steps_in(pattern, lo, hi);
    if steps > (max_units as f64) * 8.0 + 10_000.0 {
        return Err(TooMany {
            estimated: estimated.max(steps as u64 / 4),
            max: max_units,
        });
    }
    let mut edges = Vec::new();
    let mut ring_box: Vec<(DVec2, DVec2)> = Vec::new();
    for r in &local {
        let (mut rlo, mut rhi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for (i, p) in r.iter().enumerate() {
            edges.push((*p, r[(i + 1) % r.len()]));
            rlo = rlo.min(*p);
            rhi = rhi.max(*p);
        }
        ring_box.push((rlo, rhi));
    }
    let unit_extent = (pattern.unit_length.max(pattern.unit_width) + pattern.gap) * 4.0;
    let span = hi - lo;
    let size = unit_extent
        .max((span.x * span.y / 100_000.0).sqrt())
        .max(span.x.max(span.y) / 1000.0)
        .max(1e-6);
    let index = Index::new(edges, lo, hi, size);
    let mut stamp = Stamp::new(index.edges.len());
    let mut inside_cache: Vec<i8> = vec![-1; index.nx * index.ny];
    let half_gap = pattern.gap / 2.0;
    let mut failed = false;
    let mut visit = |c: Cell| {
        if failed {
            return;
        }
        let (rlo, rhi) = (c.lo + DVec2::splat(half_gap), c.hi - DVec2::splat(half_gap));
        if rhi.x <= rlo.x || rhi.y <= rlo.y {
            return;
        }
        // Quick rejects against the box of the region.
        if rhi.x < lo.x || rlo.x > hi.x || rhi.y < lo.y || rlo.y > hi.y {
            return;
        }
        let rect_area = (rhi.x - rlo.x) * (rhi.y - rlo.y);
        let ((x0, y0), (x1, y1)) = (index.cell_xy(rlo), index.cell_xy(rhi));
        stamp.next();
        let mut candidates: Vec<u32> = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &e in index.cell_items(y * index.nx + x) {
                    if !stamp.seen(e) {
                        candidates.push(e);
                    }
                }
            }
        }
        let center = (rlo + rhi) / 2.0;
        let touching = candidates.iter().any(|&e| {
            let (a, b) = index.edges[e as usize];
            seg_hits_rect(a, b, rlo, rhi)
        });
        let emit_whole = |stats: &mut Stats, f: &mut dyn FnMut(&Unit)| {
            let corners = [rlo, DVec2::new(rhi.x, rlo.y), rhi, DVec2::new(rlo.x, rhi.y)];
            let u = Unit {
                rings: vec![UnitRing {
                    points: corners.iter().map(|p| origin + turn.rotate(*p)).collect(),
                    hole: false,
                }],
                whole: true,
                area: rect_area,
                share: rect_area / full,
            };
            stats.units += 1;
            f(&u);
        };
        if !touching {
            let inside = if candidates.is_empty() {
                let (cx, cy) = index.cell_xy(center);
                let k = cy * index.nx + cx;
                if inside_cache[k] < 0 {
                    inside_cache[k] = index.inside(center, &mut stamp) as i8;
                }
                inside_cache[k] == 1
            } else {
                index.inside(center, &mut stamp)
            };
            if inside {
                if stats.units >= max_units {
                    failed = true;
                    return;
                }
                emit_whole(&mut stats, f);
            }
            return;
        }
        // The unit meets an edge: cut it. Rings far from the unit cannot matter.
        let mut subject: Vec<Vec<[f64; 2]>> = Vec::new();
        for (ri, ring) in local.iter().enumerate() {
            let (blo, bhi) = ring_box[ri];
            if bhi.x < rlo.x || blo.x > rhi.x || bhi.y < rlo.y || blo.y > rhi.y {
                continue;
            }
            let cut = clip_ring(ring, rlo, rhi);
            if cut.len() >= 3 {
                subject.push(cut.iter().map(|p| [p.x, p.y]).collect());
            }
        }
        let shapes = subject.overlay(
            &Vec::<Vec<[f64; 2]>>::new(),
            OverlayRule::Union,
            FillRule::EvenOdd,
        );
        // Every connected piece is a unit of its own: a notch that splits a unit in two makes two
        // cut units.
        let count = shapes.len();
        for shape in shapes {
            let a: f64 = shape
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let pts: Vec<DVec2> = r.iter().map(|p| DVec2::new(p[0], p[1])).collect();
                    let a = ring_area(&pts).abs();
                    if i == 0 { a } else { -a }
                })
                .sum();
            if a < DUST_AREA {
                stats.dust_pieces += 1;
                continue;
            }
            if stats.units >= max_units {
                failed = true;
                return;
            }
            let rings: Vec<UnitRing> = shape
                .iter()
                .enumerate()
                .map(|(i, r)| UnitRing {
                    points: r
                        .iter()
                        .map(|p| origin + turn.rotate(DVec2::new(p[0], p[1])))
                        .collect(),
                    hole: i > 0,
                })
                .collect();
            let whole = count == 1 && rings.len() == 1 && a >= rect_area * (1.0 - 1e-9);
            stats.units += 1;
            f(&Unit {
                rings,
                whole,
                area: a,
                share: a / full,
            });
        }
    };
    bond::for_each_cell(pattern, lo, hi, &mut visit);
    if failed {
        return Err(too_many());
    }
    Ok(stats)
}

/// All units of a layout, in a list. Same limits as [`for_each_unit`].
pub fn lay_units(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    max_units: usize,
) -> Result<(Vec<Unit>, Stats), TooMany> {
    let mut out = Vec::new();
    let stats = for_each_unit(region, pattern, place, max_units, &mut |u| {
        out.push(u.clone())
    })?;
    Ok((out, stats))
}
