//! Units of a pattern, clipped exactly to a region.

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use super::module::Compiled;
use super::{DUST_AREA, Pattern, Placement, Region};

/// A unit that crosses the region edge by less than this (mm) counts as a whole unit, so that the
/// noise of a drawing (a corner 0.01 mm off) does not make cut units.
pub const DEFAULT_TRIM: f64 = 0.1;

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
    /// The area as a part of a full unit of its kind: 1 for a whole unit.
    pub share: f64,
    /// The unit shape (the index into `Stats::kinds`). Always 0 in the four bonds.
    pub kind: usize,
    /// How wide the piece is, in mm: the smallest width of the convex hull of its outline over
    /// all directions. It is the short side of a rectangle, also when the piece is long. The hull
    /// hides a thin tail or a notch of a concave piece, so the width can be overstated for such a
    /// piece (a piece may be flagged as too narrow less often than it should, never more often).
    pub width: f64,
}

/// What a layout found, besides the units.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub origin: DVec2,
    pub angle: f64,
    /// The area of a full unit (of the first unit shape), in mm2.
    pub full_unit_area: f64,
    /// The name and the area of each unit shape, in mm2.
    pub kinds: Vec<(String, f64)>,
    pub region_area: f64,
    pub units: usize,
    /// Clipped pieces smaller than [`DUST_AREA`]: rounding dust, not counted as units.
    pub dust_pieces: usize,
    /// Units that crossed the edge by less than the trim and were counted as whole.
    pub trimmed: usize,
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

/// Why a layout could not be made.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutError {
    /// More units than the limit.
    TooMany(TooMany),
    /// The pattern or the region is not valid. The text says why.
    Invalid(String),
    /// The region is so long and thin in the frame of the pattern that the scan of its box would
    /// test far more cells than there are units. The text says what to do.
    Scan(String),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::TooMany(t) => t.fmt(f),
            LayoutError::Invalid(s) | LayoutError::Scan(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for LayoutError {}

impl From<TooMany> for LayoutError {
    fn from(t: TooMany) -> Self {
        LayoutError::TooMany(t)
    }
}

/// The limits of a layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Most units. A layout with more is an error.
    pub max_units: usize,
    /// A unit that crosses the region edge (or a hole) by less than this many mm counts as whole.
    /// 0 turns it off.
    pub trim: f64,
}

impl Limits {
    pub fn new(max_units: usize) -> Limits {
        Limits {
            max_units,
            trim: DEFAULT_TRIM,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::new(super::MAX_UNITS)
    }
}

/// Counts of the units of one shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KindTally {
    pub whole: usize,
    pub cut: usize,
    /// Cut units with a share below the sliver share.
    pub slivers: usize,
    /// Cut units too small or too narrow to be laid (see [`Unfillable`]).
    pub unfillable: usize,
    /// The area of all units, in mm2.
    pub area: f64,
    /// The area of the cut units, in mm2.
    pub cut_area: f64,
    /// The sum of the shares of the cut units.
    pub cut_share: f64,
}

/// When a cut unit is too small to be laid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unfillable {
    /// A cut unit with less than this share of a full unit.
    pub share: f64,
    /// A cut unit narrower than this, in mm (0: off).
    pub width: f64,
}

impl Default for Unfillable {
    fn default() -> Self {
        Unfillable {
            share: 0.05,
            width: 0.0,
        }
    }
}

/// Counts of units, per unit shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tally {
    pub kinds: Vec<KindTally>,
}

impl Tally {
    pub fn add(&mut self, u: &Unit, sliver_share: f64, unfillable: &Unfillable) {
        if self.kinds.len() <= u.kind {
            self.kinds.resize(u.kind + 1, KindTally::default());
        }
        let k = &mut self.kinds[u.kind];
        if u.whole {
            k.whole += 1;
        } else {
            k.cut += 1;
            k.cut_area += u.area;
            k.cut_share += u.share;
            if u.share < sliver_share {
                k.slivers += 1;
            }
            if u.share < unfillable.share || (unfillable.width > 0.0 && u.width < unfillable.width)
            {
                k.unfillable += 1;
            }
        }
        k.area += u.area;
    }

    fn sum<T: std::iter::Sum<T>>(&self, f: impl Fn(&KindTally) -> T) -> T {
        self.kinds.iter().map(f).sum()
    }

    pub fn whole(&self) -> usize {
        self.sum(|k| k.whole)
    }

    pub fn cut(&self) -> usize {
        self.sum(|k| k.cut)
    }

    pub fn slivers(&self) -> usize {
        self.sum(|k| k.slivers)
    }

    pub fn unfillable(&self) -> usize {
        self.sum(|k| k.unfillable)
    }

    /// The area of all units, in mm2.
    pub fn area(&self) -> f64 {
        self.sum(|k| k.area)
    }
}

/// The edges of the region, sorted into a grid, so that a unit meets only the edges near it.
pub(super) struct Index {
    pub edges: Vec<(DVec2, DVec2)>,
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
        for (c, n) in counts.iter().enumerate() {
            ix.start[c + 1] = ix.start[c] + n;
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

    pub(super) fn cell_size(&self) -> f64 {
        self.size
    }

    pub(super) fn cell_xy(&self, p: DVec2) -> (usize, usize) {
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

    /// The edges near the box `lo`..`hi` (each once).
    pub(super) fn near(&self, lo: DVec2, hi: DVec2, stamp: &mut Stamp, out: &mut Vec<u32>) {
        out.clear();
        let ((x0, y0), (x1, y1)) = (self.cell_xy(lo), self.cell_xy(hi));
        stamp.next();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &e in self.cell_items(y * self.nx + x) {
                    if !stamp.seen(e) {
                        out.push(e);
                    }
                }
            }
        }
    }

    /// True if `p` is inside the region (even-odd, a ray to -X).
    pub(super) fn inside(&self, p: DVec2, stamp: &mut Stamp) -> bool {
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
pub(super) struct Stamp {
    marks: Vec<u32>,
    now: u32,
}

impl Stamp {
    pub(super) fn new(n: usize) -> Stamp {
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

/// The region in the frame of the pattern, with its edge index.
pub(super) struct Local {
    pub rings: Vec<Vec<DVec2>>,
    ring_box: Vec<(DVec2, DVec2)>,
    pub index: Index,
    pub lo: DVec2,
    pub hi: DVec2,
    pub stamp: Stamp,
}

impl Local {
    pub(super) fn new(region: &Region, origin: DVec2, angle: f64, unit_extent: f64) -> Local {
        let unturn = DVec2::from_angle(-angle.to_radians());
        let rings: Vec<Vec<DVec2>> = region
            .rings()
            .map(|r| r.iter().map(|p| unturn.rotate(*p - origin)).collect())
            .collect();
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for r in &rings {
            for p in r {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        let mut edges = Vec::new();
        let mut ring_box: Vec<(DVec2, DVec2)> = Vec::new();
        for r in &rings {
            let (mut rlo, mut rhi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            for (i, p) in r.iter().enumerate() {
                edges.push((*p, r[(i + 1) % r.len()]));
                rlo = rlo.min(*p);
                rhi = rhi.max(*p);
            }
            ring_box.push((rlo, rhi));
        }
        let span = hi - lo;
        let size = (unit_extent * 4.0)
            .max((span.x * span.y / 100_000.0).sqrt())
            .max(span.x.max(span.y) / 1000.0)
            .max(1e-6);
        let n = edges.len();
        Local {
            rings,
            ring_box,
            index: Index::new(edges, lo, hi, size),
            lo,
            hi,
            stamp: Stamp::new(n),
        }
    }
}

fn segments_touch(p: DVec2, q: DVec2, r: DVec2, s: DVec2) -> bool {
    let o = |a: DVec2, b: DVec2, c: DVec2| (b - a).perp_dot(c - a);
    let (d1, d2, d3, d4) = (o(p, q, r), o(p, q, s), o(r, s, p), o(r, s, q));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    let on = |a: DVec2, b: DVec2, c: DVec2, d: f64| {
        d == 0.0
            && c.x >= a.x.min(b.x)
            && c.x <= a.x.max(b.x)
            && c.y >= a.y.min(b.y)
            && c.y <= a.y.max(b.y)
    };
    on(p, q, r, d1) || on(p, q, s, d2) || on(r, s, p, d3) || on(r, s, q, d4)
}

/// True if the segment touches the polygon: it crosses an edge, or an end lies inside.
fn seg_hits_poly(a: DVec2, b: DVec2, poly: &[DVec2], lo: DVec2, hi: DVec2) -> bool {
    if a.x.max(b.x) < lo.x || a.x.min(b.x) > hi.x || a.y.max(b.y) < lo.y || a.y.min(b.y) > hi.y {
        return false;
    }
    let n = poly.len();
    (0..n).any(|i| segments_touch(a, b, poly[i], poly[(i + 1) % n]))
        || crate::geom::point_in_polygon(a, poly)
        || crate::geom::point_in_polygon(b, poly)
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

fn ring_length(r: &[DVec2]) -> f64 {
    (0..r.len())
        .map(|i| r[i].distance(r[(i + 1) % r.len()]))
        .sum()
}

/// The counts of the lattice cells that the layout visits, for a box in the frame of the pattern.
pub(super) fn lattice_range(c: &Compiled, lo: DVec2, hi: DVec2) -> ((i64, i64), (i64, i64)) {
    let (mut klo, mut khi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for k in &c.kinds {
        klo = klo.min(k.lo);
        khi = khi.max(k.hi);
    }
    // The shifts t for which the box of a cell moved by t meets the box.
    let (tlo, thi) = (lo - khi, hi - klo);
    let corners = [tlo, DVec2::new(thi.x, tlo.y), thi, DVec2::new(tlo.x, thi.y)];
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for p in corners {
        let (u, v) = c.lattice_of(p);
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    (
        (u0.floor() as i64 - 1, u1.ceil() as i64 + 1),
        (v0.floor() as i64 - 1, v1.ceil() as i64 + 1),
    )
}

/// Lays the units of a pattern over a region and calls `f` for each. The units are in the order of
/// the rows of the lattice. If the job makes more than `limits.max_units`, the function returns an
/// error; `f` may have been called before that, so a caller that gets an error discards what it got.
pub fn for_each_unit(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    limits: &Limits,
    f: &mut dyn FnMut(&Unit),
) -> Result<Stats, LayoutError> {
    let max_units = limits.max_units;
    if let Err(e) = pattern.validate() {
        return Err(LayoutError::Invalid(e));
    }
    let comp = pattern.compile().map_err(LayoutError::Invalid)?;
    let (origin, angle) = place.resolve(region);
    let kinds: Vec<(String, f64)> = comp
        .kinds
        .iter()
        .map(|k| (k.name.clone(), k.area))
        .collect();
    let stats = Stats {
        origin,
        angle,
        full_unit_area: kinds.first().map_or(0.0, |k| k.1),
        kinds,
        region_area: region.area,
        units: 0,
        dust_pieces: 0,
        trimmed: 0,
    };
    let pieces: usize = comp.kinds.iter().map(|k| k.pieces.len()).sum();
    let estimated = (region.area / comp.det.abs().max(1e-9) * pieces as f64).ceil() as u64;
    let too_many = |estimated: u64| {
        LayoutError::TooMany(TooMany {
            estimated,
            max: max_units,
        })
    };
    if estimated > max_units as u64 {
        return Err(too_many(estimated));
    }
    let turn = DVec2::from_angle(angle.to_radians());
    let extent = comp
        .kinds
        .iter()
        .map(|k| (k.hi - k.lo).max_element())
        .fold(0.0, f64::max);
    let local = Local::new(region, origin, angle, extent);
    let (lo, hi) = (local.lo, local.hi);
    // The job in the box of the region may be larger than the region (a long, thin, diagonal shape).
    let ((i0, i1), (j0, j1)) = lattice_range(&comp, lo, hi);
    let steps = (i1 - i0 + 1) as f64 * (j1 - j0 + 1) as f64 * pieces as f64;
    if steps > (max_units as f64) * 8.0 + 10_000.0 {
        return Err(LayoutError::Scan(format!(
            "the region is long and thin in the frame of the pattern: its box holds about {} cells to test for about {estimated} units. Turn the pattern (angle, or align_to_edge) to follow the length of the region, or split the region",
            steps as u64
        )));
    }
    // The width of each piece of each unit shape (the same for every copy of it).
    let widths: Vec<Vec<f64>> = comp
        .kinds
        .iter()
        .map(|k| k.pieces.iter().map(|p| min_width(p)).collect())
        .collect();
    let mut placer = Placer::new(
        local,
        origin,
        turn,
        limits.trim.max(0.0),
        max_units,
        estimated,
        stats,
    );
    let mut buf: Vec<DVec2> = Vec::new();
    for j in j0..=j1 {
        for i in i0..=i1 {
            let t = comp.a * i as f64 + comp.b * j as f64;
            for (ki, kind) in comp.kinds.iter().enumerate() {
                // Quick reject with the box of the cell.
                if kind.hi.x + t.x < lo.x
                    || kind.lo.x + t.x > hi.x
                    || kind.hi.y + t.y < lo.y
                    || kind.lo.y + t.y > hi.y
                {
                    continue;
                }
                for (pi, piece) in kind.pieces.iter().enumerate() {
                    buf.clear();
                    buf.extend(piece.iter().map(|p| *p + t));
                    placer.place(&buf, ki, ring_area(piece), kind.area, widths[ki][pi], f)?;
                }
            }
        }
    }
    Ok(placer.stats)
}

/// Puts pieces (cells of a pattern, in the frame of the pattern) into a region: whole, trimmed or
/// cut. The engine and the courses (PAT-08) share it.
pub(super) struct Placer {
    pub local: Local,
    inside_cache: std::collections::HashMap<(usize, usize), bool>,
    near: Vec<u32>,
    origin: DVec2,
    turn: DVec2,
    trim: f64,
    max_units: usize,
    estimated: u64,
    pub stats: Stats,
}

impl Placer {
    pub(super) fn new(
        local: Local,
        origin: DVec2,
        turn: DVec2,
        trim: f64,
        max_units: usize,
        estimated: u64,
        stats: Stats,
    ) -> Placer {
        Placer {
            local,
            inside_cache: Default::default(),
            near: Vec::new(),
            origin,
            turn,
            trim,
            max_units,
            estimated,
            stats,
        }
    }

    fn too_many(&self) -> LayoutError {
        LayoutError::TooMany(TooMany {
            estimated: self.estimated.max(self.stats.units as u64 + 1),
            max: self.max_units,
        })
    }

    /// One piece `buf` (a polygon in the frame of the pattern) of unit shape `ki`, whose full area
    /// is `piece_area` (the whole piece) and `kind_area` (all pieces of the shape). Calls `f` for
    /// each unit that results.
    pub(super) fn place(
        &mut self,
        buf: &[DVec2],
        ki: usize,
        piece_area: f64,
        kind_area: f64,
        width: f64,
        f: &mut dyn FnMut(&Unit),
    ) -> Result<(), LayoutError> {
        let (lo, hi) = (self.local.lo, self.local.hi);
        let (mut plo, mut phi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in buf {
            plo = plo.min(*p);
            phi = phi.max(*p);
        }
        if phi.x < lo.x || plo.x > hi.x || phi.y < lo.y || plo.y > hi.y {
            return Ok(());
        }
        self.local
            .index
            .near(plo, phi, &mut self.local.stamp, &mut self.near);
        let touching = self.near.iter().any(|&e| {
            let (a, b) = self.local.index.edges[e as usize];
            seg_hits_poly(a, b, buf, plo, phi)
        });
        if !touching {
            let p0 = buf[0];
            let inside = if self.near.is_empty() {
                let key = self.local.index.cell_xy(p0);
                match self.inside_cache.get(&key) {
                    Some(v) => *v,
                    None => {
                        let v = self.local.index.inside(p0, &mut self.local.stamp);
                        self.inside_cache.insert(key, v);
                        v
                    }
                }
            } else {
                self.local.index.inside(p0, &mut self.local.stamp)
            };
            if inside {
                if self.stats.units >= self.max_units {
                    return Err(self.too_many());
                }
                self.stats.units += 1;
                let u = whole_unit(
                    buf,
                    self.origin,
                    self.turn,
                    ki,
                    piece_area,
                    kind_area,
                    width,
                );
                f(&u);
            }
            return Ok(());
        }
        // The unit meets an edge: cut it. Rings far from the unit cannot matter.
        let mut clip: Vec<Vec<[f64; 2]>> = Vec::new();
        for (ri, ring) in self.local.rings.iter().enumerate() {
            let (blo, bhi) = self.local.ring_box[ri];
            if bhi.x < plo.x || blo.x > phi.x || bhi.y < plo.y || blo.y > phi.y {
                continue;
            }
            let cut = clip_ring(ring, plo, phi);
            if cut.len() >= 3 {
                clip.push(cut.iter().map(|p| [p.x, p.y]).collect());
            }
        }
        let subject = vec![buf.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()];
        let shapes = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);
        // Noise: the unit crosses the edge by less than the trim. It counts as whole.
        if self.trim > 0.0
            && is_trimmed(buf, &shapes, piece_area, &self.local, &self.near, self.trim)
        {
            if self.stats.units >= self.max_units {
                return Err(self.too_many());
            }
            self.stats.units += 1;
            self.stats.trimmed += 1;
            let u = whole_unit(
                buf,
                self.origin,
                self.turn,
                ki,
                piece_area,
                kind_area,
                width,
            );
            f(&u);
            return Ok(());
        }
        // Every connected piece is a unit of its own: a notch that splits a unit in two makes two
        // cut units.
        let mut made: Vec<Unit> = Vec::new();
        for shape in &shapes {
            let pts: Vec<Vec<DVec2>> = shape
                .iter()
                .map(|r| r.iter().map(|p| DVec2::new(p[0], p[1])).collect())
                .collect();
            let a: f64 = pts
                .iter()
                .enumerate()
                .map(|(n, r)| {
                    let a = ring_area(r).abs();
                    if n == 0 { a } else { -a }
                })
                .sum();
            if a < DUST_AREA {
                self.stats.dust_pieces += 1;
                continue;
            }
            made.push(Unit {
                rings: pts
                    .iter()
                    .enumerate()
                    .map(|(n, r)| UnitRing {
                        points: r
                            .iter()
                            .map(|p| self.origin + self.turn.rotate(*p))
                            .collect(),
                        hole: n > 0,
                    })
                    .collect(),
                whole: false,
                area: a,
                share: a / kind_area,
                kind: ki,
                width: min_width(&pts[0]),
            });
        }
        // Counted after the dust is gone: one piece, no hole, and all of the unit.
        let single = made.len() == 1;
        for mut u in made {
            if single && u.rings.len() == 1 && u.area >= piece_area * (1.0 - 1e-9) {
                u.whole = true;
            }
            if self.stats.units >= self.max_units {
                return Err(self.too_many());
            }
            self.stats.units += 1;
            f(&u);
        }
        Ok(())
    }
}

fn whole_unit(
    buf: &[DVec2],
    origin: DVec2,
    turn: DVec2,
    kind: usize,
    area: f64,
    full: f64,
    width: f64,
) -> Unit {
    Unit {
        rings: vec![UnitRing {
            points: buf.iter().map(|p| origin + turn.rotate(*p)).collect(),
            hole: false,
        }],
        whole: true,
        area,
        share: area / full,
        kind,
        width,
    }
}

/// The smallest width of the convex hull of the ring over all directions (the narrowest strip
/// that holds it). For a concave ring the hull hides a thin tail, so the width is overstated, on
/// the safe side for a test of "too narrow". The inscribed-circle diameter would be smaller for a
/// long piece and needs more work.
pub fn min_width(ring: &[DVec2]) -> f64 {
    let mut pts: Vec<DVec2> = ring.to_vec();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return 0.0;
    }
    // Andrew's monotone chain.
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<DVec2> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &DVec2>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    let n = hull.len();
    if n < 3 {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for i in 0..n {
        let (a, b) = (hull[i], hull[(i + 1) % n]);
        let Some(d) = (b - a).try_normalize() else {
            continue;
        };
        let far = hull
            .iter()
            .map(|p| d.perp_dot(*p - a).abs())
            .fold(0.0, f64::max);
        best = best.min(far);
    }
    if best.is_finite() { best } else { 0.0 }
}

/// True if the unit `buf` sticks out of the region only by less than `trim`, and no corner of
/// the region pokes into it by more than that.
fn is_trimmed(
    buf: &[DVec2],
    shapes: &[Vec<Vec<[f64; 2]>>],
    piece_area: f64,
    local: &Local,
    near: &[u32],
    trim: f64,
) -> bool {
    // Nothing left, or the unit is cut a lot: the area test is the cheap way out.
    let area: f64 = shapes
        .iter()
        .map(|s| {
            s.iter()
                .enumerate()
                .map(|(n, r)| {
                    let pts: Vec<DVec2> = r.iter().map(|p| DVec2::new(p[0], p[1])).collect();
                    let a = ring_area(&pts).abs();
                    if n == 0 { a } else { -a }
                })
                .sum::<f64>()
        })
        .sum();
    let perimeter = ring_length(buf);
    if area >= piece_area || piece_area - area > trim * perimeter {
        return false;
    }
    let dist_edges = |p: DVec2| {
        near.iter()
            .map(|&e| {
                let (a, b) = local.index.edges[e as usize];
                crate::geom::dist_to_segment(p, a, b)
            })
            .fold(f64::INFINITY, f64::min)
    };
    // A corner of the unit outside the region must be within the trim of an edge.
    let outside_far = buf.iter().any(|p| {
        let inside = shapes.iter().any(|s| {
            let outer: Vec<DVec2> = s[0].iter().map(|q| DVec2::new(q[0], q[1])).collect();
            crate::geom::point_in_polygon(*p, &outer)
        });
        !inside && dist_edges(*p) > trim
    });
    if outside_far {
        return false;
    }
    // A corner of the region inside the unit must be within the trim of the unit outline.
    let n = buf.len();
    !near.iter().any(|&e| {
        let (a, _) = local.index.edges[e as usize];
        crate::geom::point_in_polygon(a, buf)
            && (0..n).all(|i| crate::geom::dist_to_segment(a, buf[i], buf[(i + 1) % n]) > trim)
    })
}

/// All units of a layout, in a list. Same limits as [`for_each_unit`].
pub fn lay_units(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    limits: &Limits,
) -> Result<(Vec<Unit>, Stats), LayoutError> {
    let mut out = Vec::new();
    let stats = for_each_unit(region, pattern, place, limits, &mut |u| out.push(u.clone()))?;
    Ok((out, stats))
}
