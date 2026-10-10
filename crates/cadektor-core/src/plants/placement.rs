//! Plant placement math. Pure functions from a placement request to points.
//!
//! Modes (the same as in the Plant tool):
//!
//! - `single`: one plant at a point.
//! - `path`: along a path. Give a `spacing` (as many plants as fit, starting at the
//!   first point), or a `count` (spread evenly over the path), or both (exactly `count`
//!   plants, `spacing` apart).
//! - `grid`: a rectangular grid inside a rectangle.
//! - `triangular`: a staggered grid inside a rectangle. `spacing` is the distance
//!   between neighbors, and rows are `spacing * sqrt(3) / 2` apart.
//! - `fill`: fill a closed boundary, by `spacing`, by `count` or by `density`
//!   (plants per square meter).
//! - `poly`: one plant at each vertex of a polyline that the user drew.
//! - `hedgerow`: several rows along a path, `row_spacing` apart, centered on the path. With
//!   `stagger`, every second row starts half a `spacing` further along, so the plants of
//!   neighbor rows do not stand side by side.
//!
//! Rules that hold for the results (and that the tests check):
//!
//! - Grid and fill: no two points are closer than the spacing (for a rectangular grid,
//!   the smaller of the two spacings).
//! - Path: neighbors are `spacing` apart along the path.
//! - Fill and grid: every point lies inside the boundary, at least `margin` from its edge.
//! - By count: the number of points equals the requested count.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};

/// The smallest spacing, in mm.
pub const MIN_SPACING: f64 = 10.0;
/// The most plants that one request may make.
pub const MAX_POINTS: usize = 20_000;
/// Chord tolerance when a boundary or path has curves, in mm.
const FLATTEN_TOLERANCE: f64 = 2.0;
const EPS: f64 = 1e-9;

#[derive(Debug, thiserror::Error, PartialEq)]
#[error("{0}")]
pub struct PlaceError(pub String);

fn bad<T>(m: impl Into<String>) -> Result<T, PlaceError> {
    Err(PlaceError(m.into()))
}

/// The arrangement of plants inside an area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Rectangular,
    #[default]
    Triangular,
}

/// A placement request. The `mode` key in JSON names the variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PlaceSpec {
    Single {
        at: DVec2,
    },
    Path {
        path: Vec<Vertex>,
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        spacing: Option<f64>,
        #[serde(default)]
        count: Option<usize>,
    },
    Grid {
        min: DVec2,
        max: DVec2,
        #[serde(default)]
        spacing: Option<f64>,
        #[serde(default)]
        spacing_x: Option<f64>,
        #[serde(default)]
        spacing_y: Option<f64>,
        #[serde(default)]
        margin: f64,
    },
    Triangular {
        min: DVec2,
        max: DVec2,
        spacing: f64,
        #[serde(default)]
        margin: f64,
    },
    Fill {
        boundary: Vec<Vertex>,
        #[serde(default)]
        spacing: Option<f64>,
        #[serde(default)]
        count: Option<usize>,
        /// Plants per square meter.
        #[serde(default)]
        density: Option<f64>,
        #[serde(default)]
        pattern: Pattern,
        #[serde(default)]
        margin: f64,
        /// Areas inside the boundary that stay empty.
        #[serde(default)]
        holes: Vec<Vec<Vertex>>,
    },
    Poly {
        points: Vec<Vertex>,
    },
    Hedgerow {
        path: Vec<Vertex>,
        #[serde(default)]
        closed: bool,
        /// Distance between plants along a row.
        spacing: f64,
        /// Number of rows, 1 to [`MAX_ROWS`]. Default 2.
        #[serde(default = "default_rows")]
        rows: usize,
        /// Distance between rows. Needed for more than one row.
        #[serde(default)]
        row_spacing: Option<f64>,
        /// Shift every second row by half a spacing. Default true.
        #[serde(default = "yes")]
        stagger: bool,
    },
}

/// The most rows of a hedgerow.
pub const MAX_ROWS: usize = 20;

fn default_rows() -> usize {
    2
}

fn yes() -> bool {
    true
}

/// Random variation of placed plants. The same seed always gives the same plants.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Variation {
    pub seed: u64,
    /// Size changes by up to this fraction up or down: 0.2 is 80% to 120%. From 0 to 0.9.
    pub size: f64,
    /// Turn each plant by a random angle.
    pub rotation: bool,
}

impl Default for Variation {
    fn default() -> Self {
        Variation {
            seed: 1,
            size: 0.0,
            rotation: false,
        }
    }
}

impl Variation {
    pub fn validate(&self) -> Result<(), String> {
        if self.size.is_finite() && (0.0..=0.9).contains(&self.size) {
            Ok(())
        } else {
            Err("size variation must be from 0 to 0.9".into())
        }
    }

    /// True if the variation changes anything.
    pub fn is_active(&self) -> bool {
        self.size > 0.0 || self.rotation
    }

    /// For each of `n` plants: the size factor and the rotation in radians. Plant `i` always gets the
    /// same values for the same seed, whatever `n` is.
    pub fn factors(&self, n: usize) -> Vec<(f64, f64)> {
        (0..n as u64)
            .map(|i| {
                let a = unit(self.seed, 2 * i);
                let b = unit(self.seed, 2 * i + 1);
                let scale = 1.0 + self.size * (2.0 * a - 1.0);
                let angle = if self.rotation {
                    b * std::f64::consts::TAU
                } else {
                    0.0
                };
                (scale, angle)
            })
            .collect()
    }
}

/// A number from 0 (included) to 1 (excluded), the same for the same seed and index (splitmix64).
fn unit(seed: u64, index: u64) -> f64 {
    let mut z = seed.wrapping_add(index.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// The plant positions for a request.
pub fn place(spec: &PlaceSpec) -> Result<Vec<DVec2>, PlaceError> {
    let pts = match spec {
        PlaceSpec::Single { at } => {
            finite(&[*at])?;
            vec![*at]
        }
        PlaceSpec::Path {
            path,
            closed,
            spacing,
            count,
        } => along_path(&flat(path, *closed)?, *closed, *spacing, *count)?,
        PlaceSpec::Grid {
            min,
            max,
            spacing,
            spacing_x,
            spacing_y,
            margin,
        } => {
            let sx = spacing_x.or(*spacing);
            let sy = spacing_y.or(*spacing);
            let (Some(sx), Some(sy)) = (sx, sy) else {
                return bad("give spacing, or spacing_x and spacing_y");
            };
            rect_grid(*min, *max, sx, sy, *margin)?
        }
        PlaceSpec::Triangular {
            min,
            max,
            spacing,
            margin,
        } => triangular_grid(*min, *max, *spacing, *margin)?,
        PlaceSpec::Fill {
            boundary,
            spacing,
            count,
            density,
            pattern,
            margin,
            holes,
        } => {
            let ring = flat(boundary, true)?;
            let holes = holes
                .iter()
                .map(|h| flat(h, true))
                .collect::<Result<Vec<_>, _>>()?;
            fill_with_holes(&ring, &holes, *spacing, *count, *density, *pattern, *margin)?
        }
        PlaceSpec::Hedgerow {
            path,
            closed,
            spacing,
            rows,
            row_spacing,
            stagger,
        } => hedgerow(
            &flat(path, *closed)?,
            *closed,
            *spacing,
            *rows,
            *row_spacing,
            *stagger,
        )?,
        PlaceSpec::Poly { points } => {
            if points.len() > MAX_POINTS {
                return bad(format!(
                    "too many plants ({}). The limit is {MAX_POINTS}",
                    points.len()
                ));
            }
            if points.is_empty() {
                return bad("poly mode needs at least one point");
            }
            let pts = curve::positions(points);
            finite(&pts)?;
            pts
        }
    };
    if pts.is_empty() {
        return bad("no plant fits");
    }
    if pts.len() > MAX_POINTS {
        return bad(format!(
            "too many plants ({}). The limit is {MAX_POINTS}",
            pts.len()
        ));
    }
    Ok(pts)
}

fn finite(pts: &[DVec2]) -> Result<(), PlaceError> {
    if pts
        .iter()
        .all(|p| p.is_finite() && p.abs().max_element() <= crate::model::MAX_COORD)
    {
        Ok(())
    } else {
        bad("numbers must be finite and not larger than 1e9 mm")
    }
}

fn flat(v: &[Vertex], closed: bool) -> Result<Vec<DVec2>, PlaceError> {
    if !v.iter().all(Vertex::is_finite) {
        return bad("numbers must be finite");
    }
    let pts = curve::flatten(v, closed, FLATTEN_TOLERANCE);
    finite(&pts)?;
    Ok(pts)
}

fn check_spacing(s: f64) -> Result<f64, PlaceError> {
    if s.is_finite() && s >= MIN_SPACING {
        Ok(s)
    } else {
        bad(format!(
            "spacing must be a number of {MIN_SPACING} mm or more"
        ))
    }
}

fn check_margin(m: f64) -> Result<f64, PlaceError> {
    if m.is_finite() && m >= 0.0 {
        Ok(m)
    } else {
        bad("margin must be zero or more")
    }
}

// ---------------------------------------------------------------------------
// Along a path

/// The point at distance `d` from the start of the polyline `pts` (`cum` holds the distance to each point).
fn point_at(pts: &[DVec2], cum: &[f64], d: f64) -> DVec2 {
    let i = cum.partition_point(|c| *c < d).clamp(1, pts.len() - 1);
    let seg = cum[i] - cum[i - 1];
    if seg <= 0.0 {
        pts[i]
    } else {
        pts[i - 1].lerp(pts[i], ((d - cum[i - 1]) / seg).clamp(0.0, 1.0))
    }
}

/// Plants along a path of points. See the module notes for `spacing` and `count`.
pub fn along_path(
    path: &[DVec2],
    closed: bool,
    spacing: Option<f64>,
    count: Option<usize>,
) -> Result<Vec<DVec2>, PlaceError> {
    if path.len() < 2 {
        return bad("a path needs 2 or more points");
    }
    // Check the size first: a huge count must not allocate anything.
    if let Some(n) = count.filter(|n| *n > MAX_POINTS) {
        return bad(format!("too many plants ({n}). The limit is {MAX_POINTS}"));
    }
    let mut pts = path.to_vec();
    if closed && pts.first() != pts.last() {
        pts.push(pts[0]);
    }
    let mut cum = vec![0.0];
    for w in pts.windows(2) {
        cum.push(cum.last().unwrap() + w[0].distance(w[1]));
    }
    let len = *cum.last().unwrap();
    if len <= EPS {
        return bad("the path has no length");
    }
    let spacing = spacing.map(check_spacing).transpose()?;
    let dist: Vec<f64> = match (spacing, count) {
        (None, None) => return bad("give spacing, count or both"),
        (Some(s), None) => {
            // For a closed path the last plant stays a full spacing from the first.
            let limit = if closed { len - s } else { len } + 1e-6;
            (0..)
                .map(|k| k as f64 * s)
                .take_while(|d| *d <= limit)
                .take(MAX_POINTS + 1)
                .collect()
        }
        (None, Some(n)) => {
            if n == 0 {
                return bad("count must be 1 or more");
            }
            match (n, closed) {
                (1, _) => vec![len / 2.0],
                (_, false) => (0..n).map(|i| len * i as f64 / (n - 1) as f64).collect(),
                (_, true) => (0..n).map(|i| len * i as f64 / n as f64).collect(),
            }
        }
        (Some(s), Some(n)) => {
            if n == 0 {
                return bad("count must be 1 or more");
            }
            let needed = if closed {
                n as f64 * s
            } else {
                (n - 1) as f64 * s
            };
            if needed > len + 1e-6 {
                return bad(format!(
                    "{n} plants {s} mm apart need {needed:.0} mm of path. The path is {len:.0} mm long"
                ));
            }
            (0..n).map(|i| i as f64 * s).collect()
        }
    };
    Ok(dist
        .into_iter()
        .map(|d| point_at(&pts, &cum, d.min(len)))
        .collect())
}

/// Rows of plants along a path. Row `k` lies `(k - (rows - 1) / 2) * row_spacing` to the left of
/// the path (negative: right). With `stagger`, odd rows start half a spacing along the path.
pub fn hedgerow(
    path: &[DVec2],
    closed: bool,
    spacing: f64,
    rows: usize,
    row_spacing: Option<f64>,
    stagger: bool,
) -> Result<Vec<DVec2>, PlaceError> {
    if path.len() < 2 {
        return bad("a path needs 2 or more points");
    }
    let spacing = check_spacing(spacing)?;
    if rows == 0 || rows > MAX_ROWS {
        return bad(format!("rows must be from 1 to {MAX_ROWS}"));
    }
    let row_spacing = match (rows, row_spacing) {
        (1, rs) => rs.unwrap_or(0.0),
        (_, Some(r)) if r.is_finite() && r >= MIN_SPACING => r,
        (_, _) => {
            return bad(format!(
                "give row_spacing: a distance of {MIN_SPACING} mm or more between rows"
            ));
        }
    };
    let mut pts = path.to_vec();
    if closed && pts.first() != pts.last() {
        pts.push(pts[0]);
    }
    let mut cum = vec![0.0];
    for w in pts.windows(2) {
        cum.push(cum.last().unwrap() + w[0].distance(w[1]));
    }
    let len = *cum.last().unwrap();
    if len <= EPS {
        return bad("the path has no length");
    }
    if rows as f64 * (len / spacing + 1.0) > 2.0 * MAX_POINTS as f64 {
        return bad("the spacing is too small for this path");
    }
    let mut out = Vec::new();
    for k in 0..rows {
        let offset = (k as f64 - (rows as f64 - 1.0) / 2.0) * row_spacing;
        let shift = if stagger && k % 2 == 1 {
            spacing / 2.0
        } else {
            0.0
        };
        // A closed row keeps a full spacing between its last plant and its first.
        let limit = if closed { len - spacing + shift } else { len } + 1e-6;
        let mut d = shift;
        while d <= limit && d < len + 1e-6 && out.len() <= MAX_POINTS {
            if closed && d >= len - 1e-6 {
                break;
            }
            let i = cum.partition_point(|c| *c < d).clamp(1, pts.len() - 1);
            let dir = (pts[i] - pts[i - 1]).normalize_or_zero();
            let normal = DVec2::new(-dir.y, dir.x);
            out.push(point_at(&pts, &cum, d.min(len)) + normal * offset);
            d += spacing;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Grids

fn normalize(a: DVec2, b: DVec2) -> Result<(DVec2, DVec2), PlaceError> {
    finite(&[a, b])?;
    Ok((a.min(b), a.max(b)))
}

fn check_cells(w: f64, h: f64, sx: f64, sy: f64) -> Result<(), PlaceError> {
    if (w / sx + 1.0) * (h / sy + 1.0) > 20.0 * MAX_POINTS as f64 {
        return bad("the spacing is too small for this area");
    }
    Ok(())
}

/// Rectangular grid inside the rectangle, `margin` away from its edges. The grid starts at the lower left corner.
pub fn rect_grid(
    min: DVec2,
    max: DVec2,
    sx: f64,
    sy: f64,
    margin: f64,
) -> Result<Vec<DVec2>, PlaceError> {
    let (lo, hi) = normalize(min, max)?;
    let (sx, sy, m) = (
        check_spacing(sx)?,
        check_spacing(sy)?,
        check_margin(margin)?,
    );
    let (lo, hi) = (lo + m, hi - m);
    if hi.x < lo.x - EPS || hi.y < lo.y - EPS {
        return bad("the margin leaves no room");
    }
    check_cells(hi.x - lo.x, hi.y - lo.y, sx, sy)?;
    let (nx, ny) = (
        ((hi.x - lo.x) / sx + 1e-9).floor() as usize,
        ((hi.y - lo.y) / sy + 1e-9).floor() as usize,
    );
    Ok((0..=ny)
        .flat_map(|j| (0..=nx).map(move |i| DVec2::new(lo.x + i as f64 * sx, lo.y + j as f64 * sy)))
        .collect())
}

/// Staggered grid inside the rectangle. Neighbors are `spacing` apart.
pub fn triangular_grid(
    min: DVec2,
    max: DVec2,
    spacing: f64,
    margin: f64,
) -> Result<Vec<DVec2>, PlaceError> {
    let (lo, hi) = normalize(min, max)?;
    let (s, m) = (check_spacing(spacing)?, check_margin(margin)?);
    let (lo, hi) = (lo + m, hi - m);
    if hi.x < lo.x - EPS || hi.y < lo.y - EPS {
        return bad("the margin leaves no room");
    }
    let row = s * 3f64.sqrt() / 2.0;
    check_cells(hi.x - lo.x, hi.y - lo.y, s, row)?;
    let ny = ((hi.y - lo.y) / row + 1e-9).floor() as usize;
    let mut out = Vec::new();
    for j in 0..=ny {
        let shift = if j % 2 == 1 { s / 2.0 } else { 0.0 };
        let y = lo.y + j as f64 * row;
        let mut x = lo.x + shift;
        while x <= hi.x + 1e-9 {
            out.push(DVec2::new(x, y));
            x += s;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Fill

fn dist_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 <= 0.0 {
        0.0
    } else {
        ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

/// A closed ring with an index for fast point tests. The edges are sorted into horizontal
/// bands, so a test looks at a few edges and not at all of them.
pub struct Ring {
    pts: Vec<DVec2>,
    lo: DVec2,
    hi: DVec2,
    band: f64,
    bands: Vec<Vec<u32>>,
}

impl Ring {
    pub fn new(pts: &[DVec2]) -> Ring {
        let (lo, hi) = ring_bounds(pts);
        let n = pts.len();
        let count = (n / 2).clamp(1, 4096);
        let band = if hi.y > lo.y {
            (hi.y - lo.y) / count as f64
        } else {
            1.0
        };
        let mut ring = Ring {
            pts: pts.to_vec(),
            lo,
            hi,
            band,
            bands: vec![Vec::new(); count],
        };
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (from, to) = (ring.band_of(a.y.min(b.y)), ring.band_of(a.y.max(b.y)));
            for k in from..=to {
                ring.bands[k].push(i as u32);
            }
        }
        ring
    }

    fn band_of(&self, y: f64) -> usize {
        (((y - self.lo.y) / self.band).floor().max(0.0) as usize).min(self.bands.len() - 1)
    }

    fn edge(&self, i: u32) -> (DVec2, DVec2) {
        let i = i as usize;
        (self.pts[i], self.pts[(i + 1) % self.pts.len()])
    }

    /// Distance to the nearest edge, if it is within `limit`. Otherwise some larger number.
    fn distance_within(&self, p: DVec2, limit: f64) -> f64 {
        if p.y + limit < self.lo.y || p.y - limit > self.hi.y {
            return f64::INFINITY;
        }
        let (from, to) = (self.band_of(p.y - limit), self.band_of(p.y + limit));
        let mut best = f64::INFINITY;
        for k in from..=to {
            for &i in &self.bands[k] {
                let (a, b) = self.edge(i);
                best = best.min(dist_to_segment(p, a, b));
            }
        }
        best
    }

    /// Even-odd test: true if `p` is inside the ring.
    fn parity(&self, p: DVec2) -> bool {
        if p.y < self.lo.y || p.y > self.hi.y {
            return false;
        }
        let mut odd = false;
        for &i in &self.bands[self.band_of(p.y)] {
            let (a, b) = self.edge(i);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
        odd
    }

    /// True if `p` is inside the ring or on its edge, and at least `margin` from every edge.
    pub fn contains(&self, p: DVec2, margin: f64) -> bool {
        let edge = self.distance_within(p, margin.max(1e-7));
        if edge < 1e-7 {
            return margin <= 1e-7;
        }
        if edge < margin {
            return false;
        }
        self.parity(p)
    }

    /// True if `p` is outside the ring (a hole) and at least `margin` from its edge.
    /// A point on the edge counts as clear when the margin is zero.
    pub fn clear_of(&self, p: DVec2, margin: f64) -> bool {
        let edge = self.distance_within(p, margin.max(1e-7));
        if edge < 1e-7 {
            return margin <= 1e-7;
        }
        edge >= margin && !self.parity(p)
    }
}

/// Distance from `p` to the edges of the ring.
pub fn distance_to_ring(ring: &[DVec2], p: DVec2) -> f64 {
    (0..ring.len())
        .map(|i| dist_to_segment(p, ring[i], ring[(i + 1) % ring.len()]))
        .fold(f64::INFINITY, f64::min)
}

/// True if `p` is inside the ring or on its edge, and at least `margin` from every edge.
pub fn inside(ring: &[DVec2], p: DVec2, margin: f64) -> bool {
    Ring::new(ring).contains(p, margin)
}

fn ring_bounds(ring: &[DVec2]) -> (DVec2, DVec2) {
    ring.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    )
}

/// The area to fill: an outer ring and holes.
struct Area {
    outer: Ring,
    holes: Vec<Ring>,
}

impl Area {
    fn accepts(&self, p: DVec2, margin: f64) -> bool {
        self.outer.contains(p, margin) && self.holes.iter().all(|h| h.clear_of(p, margin))
    }
}

/// Grid points of `pattern` at `spacing` that lie in the area.
fn fill_by_spacing(
    area: &Area,
    spacing: f64,
    pattern: Pattern,
    margin: f64,
) -> Result<Vec<DVec2>, PlaceError> {
    let (lo, hi) = (area.outer.lo, area.outer.hi);
    let grid = match pattern {
        // The grid starts `margin` inside the bounding box, so a margin does not cut the first row.
        Pattern::Rectangular => rect_grid(lo, hi, spacing, spacing, margin)?,
        Pattern::Triangular => triangular_grid(lo, hi, spacing, margin)?,
    };
    Ok(grid
        .into_iter()
        .filter(|p| area.accepts(*p, margin))
        .collect())
}

/// Spacing that gives `density` plants per square meter in a pattern.
pub fn spacing_for_density(density: f64, pattern: Pattern) -> f64 {
    let per_plant_m2 = 1.0 / density;
    let area = match pattern {
        Pattern::Rectangular => per_plant_m2,
        Pattern::Triangular => per_plant_m2 * 2.0 / 3f64.sqrt(),
    };
    area.sqrt() * 1000.0
}

/// Fills a ring (at least 3 points). Give exactly one of `spacing`, `count` or `density`.
pub fn fill(
    ring: &[DVec2],
    spacing: Option<f64>,
    count: Option<usize>,
    density: Option<f64>,
    pattern: Pattern,
    margin: f64,
) -> Result<Vec<DVec2>, PlaceError> {
    fill_with_holes(ring, &[], spacing, count, density, pattern, margin)
}

/// Like `fill`, with holes: no plant goes inside a hole, or closer to its edge than `margin`.
pub fn fill_with_holes(
    ring: &[DVec2],
    holes: &[Vec<DVec2>],
    spacing: Option<f64>,
    count: Option<usize>,
    density: Option<f64>,
    pattern: Pattern,
    margin: f64,
) -> Result<Vec<DVec2>, PlaceError> {
    if ring.len() < 3 {
        return bad("a boundary needs 3 or more points");
    }
    if holes.iter().any(|h| h.len() < 3) {
        return bad("a hole needs 3 or more points");
    }
    let margin = check_margin(margin)?;
    let area = Area {
        outer: Ring::new(ring),
        holes: holes.iter().map(|h| Ring::new(h)).collect(),
    };
    match (spacing, count, density) {
        (Some(s), None, None) => fill_by_spacing(&area, check_spacing(s)?, pattern, margin),
        (None, None, Some(d)) => {
            if !(d.is_finite() && d > 0.0) {
                return bad("density must be more than zero");
            }
            fill_by_spacing(
                &area,
                check_spacing(spacing_for_density(d, pattern))?,
                pattern,
                margin,
            )
        }
        (None, Some(n), None) => {
            let size = crate::geom::polygon_area(ring)
                - holes
                    .iter()
                    .map(|h| crate::geom::polygon_area(h))
                    .sum::<f64>();
            fill_by_count(&area, size.max(0.0), n, pattern, margin)
        }
        _ => bad("give exactly one of spacing, count or density"),
    }
}

/// Exactly `n` points: finds the widest spacing that fits at least `n`, then thins evenly.
/// The first guess comes from the area, so only a few grids are made.
fn fill_by_count(
    area: &Area,
    size: f64,
    n: usize,
    pattern: Pattern,
    margin: f64,
) -> Result<Vec<DVec2>, PlaceError> {
    if n == 0 {
        return bad("count must be 1 or more");
    }
    if n > MAX_POINTS {
        return bad(format!("too many plants ({n}). The limit is {MAX_POINTS}"));
    }
    let count_at = |s: f64| fill_by_spacing(area, s, pattern, margin).map(|v| v.len());
    let cell = match pattern {
        Pattern::Rectangular => 1.0,
        Pattern::Triangular => 3f64.sqrt() / 2.0,
    };
    let no_fit = || PlaceError(format!("{n} plants do not fit in this area"));
    let mut guess = (size / (n as f64 * cell)).sqrt().max(MIN_SPACING);
    let mut limit = 60;
    // Find a spacing that gives enough points (`good`) and one that gives too few (`high`).
    let (mut good, mut high);
    if matches!(count_at(guess), Ok(c) if c >= n) {
        good = guess;
        high = guess * 1.25;
        while matches!(count_at(high), Ok(c) if c >= n) && limit > 0 {
            good = high;
            high *= 1.25;
            limit -= 1;
        }
    } else {
        high = guess;
        loop {
            if guess <= MIN_SPACING || limit == 0 {
                return Err(no_fit());
            }
            guess = (guess / 1.25).max(MIN_SPACING);
            limit -= 1;
            if matches!(count_at(guess), Ok(c) if c >= n) {
                good = guess;
                break;
            }
            high = guess;
        }
    }
    // Bisection: the widest spacing that still gives n points or more.
    for _ in 0..8 {
        let mid = (good + high) / 2.0;
        match count_at(mid) {
            Ok(c) if c >= n => good = mid,
            _ => high = mid,
        }
    }
    let mut points = fill_by_spacing(area, good, pattern, margin)?;
    // Counts are not exactly monotonic in the spacing. Step down until enough points fit.
    while points.len() < n {
        good *= 0.99;
        if good < MIN_SPACING {
            return Err(no_fit());
        }
        points = fill_by_spacing(area, good, pattern, margin)?;
    }
    if points.len() > n {
        // Thinning keeps the points of a subset, so no pair gets closer.
        let len = points.len();
        points = (0..n).map(|i| points[i * len / n]).collect();
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::corners;
    use proptest::prelude::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn min_distance(pts: &[DVec2]) -> f64 {
        let mut m = f64::INFINITY;
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                m = m.min(pts[i].distance(pts[j]));
            }
        }
        m
    }

    fn square(side: f64) -> Vec<DVec2> {
        vec![v(0.0, 0.0), v(side, 0.0), v(side, side), v(0.0, side)]
    }

    #[test]
    fn single_and_poly() {
        assert_eq!(
            place(&PlaceSpec::Single { at: v(1.0, 2.0) }).unwrap(),
            vec![v(1.0, 2.0)]
        );
        assert!(
            place(&PlaceSpec::Single {
                at: v(f64::NAN, 2.0)
            })
            .is_err()
        );
        let pts = place(&PlaceSpec::Poly {
            points: corners(&[v(0.0, 0.0), v(5.0, 5.0)]),
        })
        .unwrap();
        assert_eq!(pts, vec![v(0.0, 0.0), v(5.0, 5.0)]);
        assert!(place(&PlaceSpec::Poly { points: vec![] }).is_err());
        assert!(
            place(&PlaceSpec::Poly {
                points: corners(&[v(1e12, 0.0)])
            })
            .is_err()
        );
    }

    #[test]
    fn path_by_spacing_starts_at_the_first_point() {
        let path = [v(0.0, 0.0), v(1000.0, 0.0)];
        let pts = along_path(&path, false, Some(250.0), None).unwrap();
        assert_eq!(pts.len(), 5);
        assert_eq!(pts[0], v(0.0, 0.0));
        assert!((pts[4].x - 1000.0).abs() < 1e-6);
        let pts = along_path(&path, false, Some(300.0), None).unwrap();
        assert_eq!(pts.len(), 4);
        assert!((pts[3].x - 900.0).abs() < 1e-6);
    }

    #[test]
    fn path_follows_corners() {
        let path = [v(0.0, 0.0), v(1000.0, 0.0), v(1000.0, 1000.0)];
        let pts = along_path(&path, false, Some(500.0), None).unwrap();
        assert_eq!(pts.len(), 5);
        assert!((pts[2] - v(1000.0, 0.0)).length() < 1e-6);
        assert!((pts[4] - v(1000.0, 1000.0)).length() < 1e-6);
    }

    #[test]
    fn path_by_count_spreads_evenly() {
        let path = [v(0.0, 0.0), v(1000.0, 0.0)];
        let pts = along_path(&path, false, None, Some(5)).unwrap();
        assert_eq!(
            pts.iter().map(|p| p.x.round()).collect::<Vec<_>>(),
            vec![0.0, 250.0, 500.0, 750.0, 1000.0]
        );
        assert_eq!(
            along_path(&path, false, None, Some(1)).unwrap(),
            vec![v(500.0, 0.0)]
        );
        // A closed path spreads over the full loop and adds no second plant at the start.
        let pts = along_path(&square(1000.0), true, None, Some(4)).unwrap();
        assert_eq!(pts.len(), 4);
        assert!(pts.contains(&v(1000.0, 0.0)));
        assert!(along_path(&path, false, None, Some(0)).is_err());
    }

    #[test]
    fn path_with_spacing_and_count() {
        let path = [v(0.0, 0.0), v(1000.0, 0.0)];
        let pts = along_path(&path, false, Some(100.0), Some(3)).unwrap();
        assert_eq!(
            pts.iter().map(|p| p.x.round()).collect::<Vec<_>>(),
            vec![0.0, 100.0, 200.0]
        );
        assert!(
            along_path(&path, false, Some(600.0), Some(3))
                .unwrap_err()
                .0
                .contains("need")
        );
        assert!(along_path(&path, true, Some(600.0), Some(4)).is_err());
        assert!(along_path(&path, false, Some(100.0), Some(0)).is_err());
    }

    #[test]
    fn closed_path_keeps_the_last_plant_one_spacing_from_the_first() {
        let pts = along_path(&square(1000.0), true, Some(1000.0), None).unwrap();
        assert_eq!(pts.len(), 4);
        let pts = along_path(&square(1000.0), true, Some(300.0), None).unwrap();
        assert!(pts.last().unwrap().distance(pts[0]) >= 300.0 - 1e-6 || pts.len() == 12);
    }

    #[test]
    fn path_errors() {
        assert!(along_path(&[v(0.0, 0.0)], false, Some(100.0), None).is_err());
        assert!(along_path(&[v(0.0, 0.0), v(0.0, 0.0)], false, Some(100.0), None).is_err());
        let p = [v(0.0, 0.0), v(100.0, 0.0)];
        assert!(along_path(&p, false, None, None).is_err());
        assert!(along_path(&p, false, Some(1.0), None).is_err());
        assert!(along_path(&p, false, Some(f64::NAN), None).is_err());
    }

    #[test]
    fn rect_grid_counts() {
        let pts = rect_grid(v(0.0, 0.0), v(1000.0, 500.0), 500.0, 250.0, 0.0).unwrap();
        assert_eq!(pts.len(), 3 * 3);
        assert!(pts.contains(&v(1000.0, 500.0)));
        // Corners in any order give the same grid.
        assert_eq!(
            rect_grid(v(1000.0, 500.0), v(0.0, 0.0), 500.0, 250.0, 0.0).unwrap(),
            pts
        );
        let pts = rect_grid(v(0.0, 0.0), v(1000.0, 1000.0), 500.0, 500.0, 100.0).unwrap();
        assert_eq!(pts.len(), 4);
        assert!(
            pts.iter()
                .all(|p| p.x >= 100.0 - 1e-9 && p.x <= 900.0 + 1e-9)
        );
        assert!(rect_grid(v(0.0, 0.0), v(100.0, 100.0), 50.0, 50.0, 60.0).is_err());
        assert!(rect_grid(v(0.0, 0.0), v(100.0, 100.0), 50.0, 50.0, -1.0).is_err());
    }

    #[test]
    fn triangular_grid_is_staggered() {
        let pts = triangular_grid(v(0.0, 0.0), v(1000.0, 1000.0), 500.0, 0.0).unwrap();
        // Row 0: x = 0, 500, 1000. Row 1 (y = 433): x = 250, 750. Row 2 (y = 866): 0, 500, 1000.
        assert_eq!(pts.len(), 3 + 2 + 3);
        assert!((min_distance(&pts) - 500.0).abs() < 1e-6);
        assert!(triangular_grid(v(0.0, 0.0), v(10.0, 10.0), 500.0, 20.0).is_err());
    }

    #[test]
    fn grid_spacing_is_checked() {
        let bad_spec = PlaceSpec::Grid {
            min: v(0.0, 0.0),
            max: v(100.0, 100.0),
            spacing: None,
            spacing_x: Some(50.0),
            spacing_y: None,
            margin: 0.0,
        };
        assert!(place(&bad_spec).is_err());
        let ok = PlaceSpec::Grid {
            min: v(0.0, 0.0),
            max: v(100.0, 100.0),
            spacing: Some(50.0),
            spacing_x: None,
            spacing_y: Some(100.0),
            margin: 0.0,
        };
        assert_eq!(place(&ok).unwrap().len(), 3 * 2);
        assert!(rect_grid(v(0.0, 0.0), v(1e8, 1e8), 10.0, 10.0, 0.0).is_err());
        assert!(rect_grid(v(0.0, 0.0), v(100.0, 100.0), 5.0, 50.0, 0.0).is_err());
        assert!(rect_grid(v(f64::NAN, 0.0), v(100.0, 100.0), 50.0, 50.0, 0.0).is_err());
    }

    #[test]
    fn inside_handles_edges_margin_and_concave_shapes() {
        let sq = square(1000.0);
        assert!(inside(&sq, v(500.0, 500.0), 0.0));
        assert!(inside(&sq, v(0.0, 500.0), 0.0));
        assert!(!inside(&sq, v(0.0, 500.0), 10.0));
        assert!(!inside(&sq, v(1001.0, 500.0), 0.0));
        assert!(!inside(&sq, v(505.0, 5.0), 10.0));
        assert!(inside(&sq, v(500.0, 500.0), 400.0));
        // An L shape: the notch is outside.
        let l = vec![
            v(0.0, 0.0),
            v(2000.0, 0.0),
            v(2000.0, 1000.0),
            v(1000.0, 1000.0),
            v(1000.0, 2000.0),
            v(0.0, 2000.0),
        ];
        assert!(inside(&l, v(500.0, 1500.0), 0.0));
        assert!(!inside(&l, v(1500.0, 1500.0), 0.0));
        assert!((distance_to_ring(&sq, v(500.0, 500.0)) - 500.0).abs() < 1e-9);
        assert_eq!(
            dist_to_segment(v(5.0, 5.0), v(1.0, 1.0), v(1.0, 1.0)),
            v(5.0, 5.0).distance(v(1.0, 1.0))
        );
    }

    #[test]
    fn fill_by_spacing_triangular_and_rectangular() {
        let ring = square(2000.0);
        let tri = fill(&ring, Some(500.0), None, None, Pattern::Triangular, 0.0).unwrap();
        let rect = fill(&ring, Some(500.0), None, None, Pattern::Rectangular, 0.0).unwrap();
        assert_eq!(rect.len(), 25);
        assert!(tri.len() > rect.len() - 5 && tri.len() < 30);
        assert!(min_distance(&tri) >= 500.0 - 1e-6);
        assert!(min_distance(&rect) >= 500.0 - 1e-6);
        let inset = fill(&ring, Some(500.0), None, None, Pattern::Rectangular, 250.0).unwrap();
        assert_eq!(inset.len(), 16);
        assert!(
            inset
                .iter()
                .all(|p| p.x >= 250.0 - 1e-6 && p.x <= 1750.0 + 1e-6)
        );
    }

    #[test]
    fn fill_by_density() {
        let ring = square(10_000.0);
        let pts = fill(&ring, None, None, Some(4.0), Pattern::Rectangular, 0.0).unwrap();
        // 4 per m2 is a 500 mm grid.
        assert!((spacing_for_density(4.0, Pattern::Rectangular) - 500.0).abs() < 1e-9);
        assert_eq!(pts.len(), 21 * 21);
        let tri = spacing_for_density(4.0, Pattern::Triangular);
        assert!(tri > 500.0 && tri < 600.0);
        assert!(fill(&ring, None, None, Some(0.0), Pattern::Rectangular, 0.0).is_err());
        assert!(fill(&ring, None, None, Some(f64::NAN), Pattern::Rectangular, 0.0).is_err());
        assert!(fill(&ring, None, None, Some(1e9), Pattern::Rectangular, 0.0).is_err());
    }

    #[test]
    fn fill_by_count_is_exact() {
        let ring = square(5000.0);
        for n in [1, 2, 7, 25, 100, 333] {
            let pts = fill(&ring, None, Some(n), None, Pattern::Triangular, 0.0).unwrap();
            assert_eq!(pts.len(), n, "n = {n}");
            assert!(pts.iter().all(|p| inside(&ring, *p, 0.0)));
        }
        let pts = fill(&ring, None, Some(50), None, Pattern::Rectangular, 200.0).unwrap();
        assert_eq!(pts.len(), 50);
        assert!(pts.iter().all(|p| inside(&ring, *p, 200.0 - 1e-6)));
    }

    #[test]
    fn fill_by_count_errors() {
        let ring = square(100.0);
        assert!(fill(&ring, None, Some(0), None, Pattern::Triangular, 0.0).is_err());
        assert!(
            fill(&ring, None, Some(10_000), None, Pattern::Triangular, 0.0)
                .unwrap_err()
                .0
                .contains("fit")
        );
        assert!(
            fill(
                &ring,
                None,
                Some(MAX_POINTS + 1),
                None,
                Pattern::Triangular,
                0.0
            )
            .is_err()
        );
    }

    #[test]
    fn fill_argument_errors() {
        let ring = square(1000.0);
        assert!(
            fill(
                &ring[..2],
                Some(100.0),
                None,
                None,
                Pattern::Triangular,
                0.0
            )
            .is_err()
        );
        assert!(fill(&ring, None, None, None, Pattern::Triangular, 0.0).is_err());
        assert!(fill(&ring, Some(100.0), Some(3), None, Pattern::Triangular, 0.0).is_err());
        assert!(fill(&ring, Some(100.0), None, None, Pattern::Triangular, -1.0).is_err());
        // A boundary with nothing inside the margin.
        assert!(
            place(&PlaceSpec::Fill {
                boundary: corners(&ring),
                spacing: Some(100.0),
                count: None,
                density: None,
                pattern: Pattern::Triangular,
                margin: 600.0,
                holes: vec![]
            })
            .is_err()
        );
    }

    #[test]
    fn place_checks_the_point_limit() {
        let r = place(&PlaceSpec::Grid {
            min: v(0.0, 0.0),
            max: v(10_000.0, 10_000.0),
            spacing: Some(50.0),
            spacing_x: None,
            spacing_y: None,
            margin: 0.0,
        });
        assert!(r.unwrap_err().0.contains("too many"));
        let r = place(&PlaceSpec::Path {
            path: corners(&[v(0.0, 0.0), v(1e7, 0.0)]),
            closed: false,
            spacing: Some(10.0),
            count: None,
        });
        assert!(r.unwrap_err().0.contains("too many"));
    }

    #[test]
    fn curved_boundaries_are_flattened() {
        let boundary = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::corner(v(4000.0, 0.0)),
            Vertex::new(
                v(4000.0, 4000.0),
                crate::curve::VertexKind::Arc { radius: 1500.0 },
            ),
            Vertex::corner(v(0.0, 4000.0)),
        ];
        let pts = place(&PlaceSpec::Fill {
            boundary,
            spacing: Some(500.0),
            count: None,
            density: None,
            pattern: Pattern::Rectangular,
            margin: 0.0,
            holes: vec![],
        })
        .unwrap();
        // The rounded corner at (4000, 4000) cuts off the grid point there.
        assert!(!pts.contains(&v(4000.0, 4000.0)));
        assert!(pts.contains(&v(0.0, 0.0)));
    }

    #[test]
    fn spec_json_uses_the_mode_key() {
        let s: PlaceSpec = serde_json::from_str(
            r#"{"mode": "fill", "boundary": [[0,0],[10,0],[10,10]], "count": 3}"#,
        )
        .unwrap();
        assert!(matches!(
            s,
            PlaceSpec::Fill {
                count: Some(3),
                pattern: Pattern::Triangular,
                ..
            }
        ));
        let j = serde_json::to_value(PlaceSpec::Single { at: v(1.0, 2.0) }).unwrap();
        assert_eq!(j["mode"], "single");
    }

    // A random simple polygon: a star around the origin, so it is always valid.
    fn star(radii: &[f64]) -> Vec<DVec2> {
        let n = radii.len();
        radii
            .iter()
            .enumerate()
            .map(|(i, r)| DVec2::from_angle(std::f64::consts::TAU * i as f64 / n as f64) * *r)
            .collect()
    }

    proptest! {
        #[test]
        fn fill_by_spacing_points_are_inside_and_apart(
            radii in prop::collection::vec(2000.0..6000.0f64, 5..12),
            spacing in 400.0..1500.0f64,
            margin in 0.0..300.0f64,
            tri in any::<bool>(),
        ) {
            let ring = star(&radii);
            let pattern = if tri { Pattern::Triangular } else { Pattern::Rectangular };
            if let Ok(pts) = fill(&ring, Some(spacing), None, None, pattern, margin) {
                for p in &pts {
                    prop_assert!(inside(&ring, *p, margin - 1e-6));
                }
                if pts.len() > 1 {
                    prop_assert!(min_distance(&pts) >= spacing - 1e-6);
                }
            }
        }

        #[test]
        fn fill_by_count_matches_the_count(
            radii in prop::collection::vec(2000.0..6000.0f64, 5..12),
            n in 1usize..120,
        ) {
            let ring = star(&radii);
            let pts = fill(&ring, None, Some(n), None, Pattern::Triangular, 0.0).unwrap();
            prop_assert_eq!(pts.len(), n);
            for p in &pts {
                prop_assert!(inside(&ring, *p, -1e-6));
            }
        }

        #[test]
        fn path_neighbors_are_one_spacing_apart_along_the_path(
            len in 1000.0..20000.0f64,
            spacing in 100.0..900.0f64,
        ) {
            let path = [v(0.0, 0.0), v(len, 0.0)];
            let pts = along_path(&path, false, Some(spacing), None).unwrap();
            prop_assert_eq!(pts.len(), (len / spacing + 1e-6).floor() as usize + 1);
            for w in pts.windows(2) {
                prop_assert!((w[0].distance(w[1]) - spacing).abs() < 1e-6);
            }
        }

        #[test]
        fn path_by_count_has_that_many_points_on_the_path(
            n in 1usize..80,
            a in -5000.0..5000.0f64,
            b in 1000.0..5000.0f64,
        ) {
            let path = [v(a, 0.0), v(a + b, 0.0), v(a + b, b)];
            let pts = along_path(&path, false, None, Some(n)).unwrap();
            prop_assert_eq!(pts.len(), n);
            for p in &pts {
                let on_first = p.y.abs() < 1e-6 && p.x >= a - 1e-6 && p.x <= a + b + 1e-6;
                let on_second = (p.x - (a + b)).abs() < 1e-6 && p.y >= -1e-6 && p.y <= b + 1e-6;
                prop_assert!(on_first || on_second);
            }
        }

        #[test]
        fn grids_keep_the_spacing_and_stay_in_the_rectangle(
            w in 500.0..8000.0f64,
            h in 500.0..8000.0f64,
            s in 100.0..900.0f64,
            tri in any::<bool>(),
        ) {
            let pts = if tri {
                triangular_grid(v(0.0, 0.0), v(w, h), s, 0.0).unwrap()
            } else {
                rect_grid(v(0.0, 0.0), v(w, h), s, s, 0.0).unwrap()
            };
            prop_assert!(!pts.is_empty());
            for p in &pts {
                prop_assert!(p.x >= -1e-9 && p.x <= w + 1e-6 && p.y >= -1e-9 && p.y <= h + 1e-6);
            }
            if pts.len() > 1 {
                prop_assert!(min_distance(&pts) >= s - 1e-6);
            }
        }
    }

    #[test]
    fn variation_is_deterministic_and_stays_in_range() {
        let v = Variation {
            seed: 42,
            size: 0.25,
            rotation: true,
        };
        let a = v.factors(500);
        assert_eq!(a, v.factors(500));
        assert_eq!(
            a[..10],
            v.factors(10)[..],
            "a plant keeps its values when more are added"
        );
        assert_ne!(a, Variation { seed: 43, ..v }.factors(500));
        assert!(a.iter().all(|(s, r)| (0.75..=1.25).contains(s) && (0.0..std::f64::consts::TAU).contains(r)));
        let mean = a.iter().map(|(s, _)| s).sum::<f64>() / 500.0;
        assert!((mean - 1.0).abs() < 0.03, "{mean}");
        assert!(
            a.iter()
                .map(|(s, _)| (s * 1000.0) as i64)
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 100,
            "values differ"
        );
        // No variation: all plants are the same, and there is no rotation.
        let none = Variation::default();
        assert!(!none.is_active());
        assert!(none.factors(5).iter().all(|f| *f == (1.0, 0.0)));
        assert!(
            v.is_active()
                && Variation {
                    rotation: false,
                    size: 0.1,
                    seed: 1
                }
                .is_active()
        );
        assert!(v.validate().is_ok());
        assert!(Variation { size: 0.95, ..v }.validate().is_err());
        assert!(Variation { size: -0.1, ..v }.validate().is_err());
        assert!(
            Variation {
                size: f64::NAN,
                ..v
            }
            .validate()
            .is_err()
        );
    }

    proptest! {
        #[test]
        fn variation_factors_are_in_range(seed in any::<u64>(), size in 0.0..0.9f64, n in 0usize..200) {
            let f = Variation { seed, size, rotation: true }.factors(n);
            prop_assert_eq!(f.len(), n);
            for (s, r) in f {
                prop_assert!(s >= 1.0 - size - 1e-12 && s <= 1.0 + size + 1e-12);
                prop_assert!((0.0..std::f64::consts::TAU).contains(&r));
            }
        }
    }

    #[test]
    fn huge_counts_are_refused_before_anything_is_made() {
        let path = [v(0.0, 0.0), v(1000.0, 0.0)];
        for spacing in [None, Some(10.0)] {
            let e = along_path(&path, false, spacing, Some(400_000_000)).unwrap_err();
            assert!(e.0.contains("too many"), "{e}");
        }
        let spec = PlaceSpec::Path {
            path: corners(&path),
            closed: true,
            spacing: None,
            count: Some(usize::MAX),
        };
        assert!(place(&spec).is_err());
        let spec = PlaceSpec::Fill {
            boundary: corners(&square(1000.0)),
            spacing: None,
            count: Some(400_000_000),
            density: None,
            pattern: Pattern::Triangular,
            margin: 0.0,
            holes: vec![],
        };
        assert!(place(&spec).unwrap_err().0.contains("too many"));
        let many = corners(&vec![v(1.0, 1.0); MAX_POINTS + 1]);
        assert!(
            place(&PlaceSpec::Poly { points: many })
                .unwrap_err()
                .0
                .contains("too many")
        );
        assert!(along_path(&path, false, None, Some(MAX_POINTS)).is_ok());
    }

    /// A wavy ring with many vertices, like a curved boundary that was flattened.
    fn wavy(n: usize, r: f64) -> Vec<DVec2> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                DVec2::from_angle(a) * (r + r * 0.1 * (7.0 * a).sin())
            })
            .collect()
    }

    #[test]
    fn filling_a_big_curved_boundary_by_count_is_fast() {
        let ring = wavy(4000, 60_000.0);
        let start = std::time::Instant::now();
        let pts = fill(
            &ring,
            None,
            Some(MAX_POINTS),
            None,
            Pattern::Triangular,
            0.0,
        )
        .unwrap();
        let took = start.elapsed();
        assert_eq!(pts.len(), MAX_POINTS);
        let index = Ring::new(&ring);
        assert!(pts.iter().all(|p| index.contains(*p, -1e-6)));
        // The goal is 200 ms in a release build. A debug build gets a loose limit.
        assert!(took.as_secs_f64() < 5.0, "took {took:?}");
    }

    #[test]
    fn the_ring_index_agrees_with_a_plain_scan() {
        let ring = wavy(300, 1000.0);
        let index = Ring::new(&ring);
        for i in 0..60 {
            for j in 0..60 {
                let p = v(-1300.0 + i as f64 * 45.0, -1300.0 + j as f64 * 45.0);
                for margin in [0.0, 30.0] {
                    let plain = {
                        let edge = distance_to_ring(&ring, p);
                        edge >= margin && crate::geom::point_in_polygon(p, &ring)
                    };
                    assert_eq!(index.contains(p, margin), plain, "{p:?} {margin}");
                }
            }
        }
        // A flat ring still has one band.
        let flat = Ring::new(&[v(0.0, 0.0), v(5.0, 0.0), v(10.0, 0.0)]);
        assert!(!flat.contains(v(5.0, 1.0), 0.0));
    }

    #[test]
    fn holes_stay_empty_and_keep_the_margin() {
        let outer = square(4000.0);
        let hole = vec![
            v(1000.0, 1000.0),
            v(3000.0, 1000.0),
            v(3000.0, 3000.0),
            v(1000.0, 3000.0),
        ];
        let pts = fill_with_holes(
            &outer,
            std::slice::from_ref(&hole),
            Some(500.0),
            None,
            None,
            Pattern::Rectangular,
            0.0,
        )
        .unwrap();
        assert!(!pts.is_empty());
        let in_hole = Ring::new(&hole);
        // The edge of a hole is not inside it.
        assert!(pts.iter().all(|p| in_hole.clear_of(*p, 0.0)));
        assert!(pts.iter().all(|p| !(p.x > 1000.0 + 1e-6
            && p.x < 3000.0 - 1e-6
            && p.y > 1000.0 + 1e-6
            && p.y < 3000.0 - 1e-6)));
        let without = fill(&outer, Some(500.0), None, None, Pattern::Rectangular, 0.0).unwrap();
        assert_eq!(
            without.len() - pts.len(),
            9,
            "the 3 x 3 points inside the hole"
        );
        // A margin keeps plants away from the hole edge.
        let pts = fill_with_holes(
            &outer,
            std::slice::from_ref(&hole),
            Some(500.0),
            None,
            None,
            Pattern::Rectangular,
            200.0,
        )
        .unwrap();
        assert!(pts.iter().all(|p| in_hole.clear_of(*p, 200.0 - 1e-6)));
        // By count, the area of the hole does not count.
        let pts = fill_with_holes(
            &outer,
            std::slice::from_ref(&hole),
            None,
            Some(40),
            None,
            Pattern::Triangular,
            0.0,
        )
        .unwrap();
        assert_eq!(pts.len(), 40);
        assert!(pts.iter().all(|p| in_hole.clear_of(*p, 0.0)));
        assert!(
            fill_with_holes(
                &outer,
                &[hole[..2].to_vec()],
                Some(500.0),
                None,
                None,
                Pattern::Rectangular,
                0.0
            )
            .is_err()
        );
        // Through the JSON spec.
        let spec: PlaceSpec = serde_json::from_value(serde_json::json!({
            "mode": "fill", "boundary": [[0,0],[4000,0],[4000,4000],[0,4000]], "spacing": 500, "pattern": "rectangular",
            "holes": [[[1000,1000],[3000,1000],[3000,3000],[1000,3000]]]
        }))
        .unwrap();
        assert_eq!(place(&spec).unwrap().len(), 81 - 9);
    }

    #[test]
    fn hedgerow_makes_staggered_rows_centered_on_the_path() {
        let path = [DVec2::new(0.0, 0.0), DVec2::new(5000.0, 0.0)];
        let pts = hedgerow(&path, false, 1000.0, 2, Some(600.0), true).unwrap();
        // Row 0 lies 300 mm right of the path (y = -300), row 1 300 mm left; row 1 starts at 500.
        let row0: Vec<_> = pts.iter().filter(|p| (p.y + 300.0).abs() < 1e-9).collect();
        let row1: Vec<_> = pts.iter().filter(|p| (p.y - 300.0).abs() < 1e-9).collect();
        assert_eq!((row0.len(), row1.len()), (6, 5));
        assert!((row0[0].x - 0.0).abs() < 1e-9 && (row1[0].x - 500.0).abs() < 1e-9);
        // Without stagger the rows stand side by side.
        let flat_rows = hedgerow(&path, false, 1000.0, 2, Some(600.0), false).unwrap();
        assert_eq!(flat_rows.len(), 12);
        // One row needs no row spacing, three rows are 600 apart, the middle one is on the path.
        assert_eq!(
            hedgerow(&path, false, 1000.0, 1, None, true).unwrap().len(),
            6
        );
        let three = hedgerow(&path, false, 1000.0, 3, Some(600.0), true).unwrap();
        assert!(three.iter().any(|p| p.y.abs() < 1e-9));
        assert!(three.iter().any(|p| (p.y - 600.0).abs() < 1e-9));
        // Errors.
        for (rows, rs, sp) in [
            (0, Some(600.0), 1000.0),
            (21, Some(600.0), 1000.0),
            (2, None, 1000.0),
            (2, Some(1.0), 1000.0),
            (2, Some(600.0), 1.0),
        ] {
            assert!(hedgerow(&path, false, sp, rows, rs, true).is_err());
        }
        assert!(hedgerow(&path[..1], false, 1000.0, 1, None, true).is_err());
        assert!(hedgerow(&[DVec2::ZERO, DVec2::ZERO], false, 1000.0, 1, None, true).is_err());
    }

    #[test]
    fn hedgerow_follows_a_closed_path_and_the_spec_parses() {
        let square = [
            DVec2::new(0.0, 0.0),
            DVec2::new(4000.0, 0.0),
            DVec2::new(4000.0, 4000.0),
            DVec2::new(0.0, 4000.0),
        ];
        let pts = hedgerow(&square, true, 1000.0, 1, None, true).unwrap();
        assert_eq!(pts.len(), 16);
        let spec: PlaceSpec = serde_json::from_value(serde_json::json!({
            "mode": "hedgerow", "path": [[0, 0], [5000, 0]], "spacing": 1000, "row_spacing": 500
        }))
        .unwrap();
        // Two rows by default, staggered.
        assert_eq!(place(&spec).unwrap().len(), 11);
    }

    proptest::proptest! {
        #[test]
        fn hedgerow_rows_keep_their_spacing(
            len in 2000.0f64..30000.0,
            spacing in 200.0f64..2000.0,
            rows in 1usize..6,
            row_spacing in 200.0f64..1500.0,
            stagger in proptest::bool::ANY,
        ) {
            let path = [DVec2::ZERO, DVec2::new(len, 0.0)];
            let pts = hedgerow(&path, false, spacing, rows, Some(row_spacing), stagger).unwrap();
            // Every plant lies within the band of the rows and no two plants of a row are closer than the spacing.
            let half = (rows as f64 - 1.0) / 2.0 * row_spacing;
            for p in &pts {
                proptest::prop_assert!(p.y.abs() <= half + 1e-6);
                proptest::prop_assert!(p.x >= -1e-6 && p.x <= len + 1e-6);
            }
            for a in &pts {
                for b in &pts {
                    if a != b && (a.y - b.y).abs() < 1e-9 {
                        proptest::prop_assert!(a.distance(*b) >= spacing - 1e-6);
                    }
                }
            }
        }
    }
}
