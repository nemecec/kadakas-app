//! Generated patterns (PAT-09): the unit shapes come from a generator, Kadakas lays them.
//!
//! A pattern of the kind `generator` holds a **stored result**: the unit shapes that a generator
//! made, in drawing coordinates, **not clipped**. A shape is a closed outline with a type name; it
//! can reach over the edge of the region or over a hole. Drawing, reports, the check and the
//! exports use the stored result and run no code. Everything after the shapes is the work of the
//! engine, the same as for the other kinds: the shapes are moved inward by half a gap, clipped to
//! the region and its holes, sorted into whole and cut units, counted per type, checked and
//! exported. Like the cells of a bond or a module, the shapes are the outlines at the centers of
//! the joints, so a result works for any gap.
//!
//! Who makes the result is not the business of the core. Today an agent does: `script.eval` runs a
//! JavaScript program (it may call the API for the region) and writes the shapes into a pattern
//! or a hardscape style. An installed package will do it later (PLAT-15); `label` and `params`
//! record what made the result.

use std::sync::{Arc, OnceLock};

use glam::DVec2;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::units::{LayoutError, Limits, Local, Placer, Stats, TooMany, Unit, min_width};
use super::{Pattern, Placement, Region};

/// Most unit types a generator may name.
pub const MAX_KINDS: usize = 64;
/// Most vertices of one cell.
pub const MAX_CELL_VERTICES: usize = 2000;
/// Longest name of a unit type, in characters.
pub const MAX_KIND_NAME: usize = 64;
/// Most points in all polygons of a result together.
pub const MAX_TOTAL_POINTS: usize = 2_000_000;
/// Longest label, in characters.
pub const MAX_LABEL: usize = 256;
/// Most bytes of the parameters as JSON.
pub const MAX_PARAMS_BYTES: usize = 64 * 1024;

/// Rectangles, as numbers: `data` has cx, cy, length, width and the angle in degrees for each.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rects {
    /// The unit type of each rectangle (an index into `types`); empty means all type 0.
    pub kind: Vec<u32>,
    pub data: Vec<f64>,
}

/// Polygons, as numbers: `points` are x, y pairs of all polygons, and polygon `i` is the pairs
/// from `start[i]` to `start[i + 1]`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Polygons {
    pub kind: Vec<u32>,
    pub start: Vec<u32>,
    pub points: Vec<f64>,
}

/// The unit shapes of a generator, in the form they are stored in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredUnits {
    /// The names of the unit types. Empty means one type, `unit`.
    pub types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rects: Option<Rects>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polygons: Option<Polygons>,
}

/// The parsed shapes of a spec, made once. A copy of the spec starts without it; a pattern that is
/// copied often holds the spec in an `Arc`, so the copies share it.
#[derive(Default)]
struct Parsed(OnceLock<Result<Arc<Generated>, String>>);

impl Clone for Parsed {
    fn clone(&self) -> Self {
        Parsed::default()
    }
}

impl PartialEq for Parsed {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Parsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Parsed")
    }
}

/// What made the shapes, and the shapes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneratorSpec {
    /// Who made the result, for the person who reads it: the name of a program or a package.
    pub label: String,
    /// The parameters the generator had, kept so that the result can be made again.
    pub params: Value,
    /// The last result.
    pub units: StoredUnits,
    #[serde(skip)]
    parsed: Parsed,
}

impl GeneratorSpec {
    /// A spec with these fields.
    pub fn new(label: impl Into<String>, params: Value, units: StoredUnits) -> Self {
        GeneratorSpec {
            label: label.into(),
            params,
            units,
            parsed: Parsed::default(),
        }
    }

    /// Checks the label, the parameters and the numbers of the result. It builds no rings, so it
    /// is cheap: it reads each number once.
    pub fn validate(&self) -> Result<(), String> {
        if self.label.chars().count() > MAX_LABEL || self.label.chars().any(char::is_control) {
            return Err(format!(
                "the label of a generator has at most {MAX_LABEL} characters and no control characters"
            ));
        }
        if self.params.to_string().len() > MAX_PARAMS_BYTES {
            return Err(format!(
                "the parameters of a generator have at most {MAX_PARAMS_BYTES} bytes as JSON"
            ));
        }
        self.units.validate().map_err(|e| e.to_string())
    }

    /// The shapes as rings, parsed once for this spec (and for every copy through its `Arc`).
    /// The count against a limit of the session is the business of the caller.
    pub fn generated(&self) -> Result<&Arc<Generated>, LayoutError> {
        self.parsed
            .0
            .get_or_init(|| self.units.build().map(Arc::new).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| LayoutError::Invalid(e.clone()))
    }
}

/// One cell: a unit type and a closed shape in drawing coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// The index into [`Generated::kinds`].
    pub kind: usize,
    pub ring: Vec<DVec2>,
}

/// The shapes of a generator as closed rings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    /// The names of the unit types.
    pub kinds: Vec<String>,
    pub cells: Vec<Cell>,
}

fn kind_name(n: &str) -> String {
    n.chars()
        .filter(|c| !c.is_control())
        .take(MAX_KIND_NAME)
        .collect::<String>()
        .trim()
        .to_string()
}

impl StoredUnits {
    /// The number of shapes.
    pub fn len(&self) -> usize {
        self.rects.as_ref().map_or(0, |r| r.data.len() / 5)
            + self
                .polygons
                .as_ref()
                .map_or(0, |p| p.start.len().saturating_sub(1))
    }

    /// True if there is nothing in it (no numbers at all). A result with broken numbers is not
    /// empty: laying it says what is wrong.
    pub fn is_empty(&self) -> bool {
        self.rects.as_ref().is_none_or(|r| r.data.is_empty())
            && self
                .polygons
                .as_ref()
                .is_none_or(|p| p.points.is_empty() && p.start.len() <= 1)
    }

    /// Checks the numbers of the result without building anything: the sizes first, then every
    /// number once. Numbers must be finite and not larger than the limit of the drawing, a
    /// rectangle needs a positive length and width, a polygon 3 or more points and an area.
    pub fn validate(&self) -> Result<(), LayoutError> {
        let bad = |m: String| Err(LayoutError::Invalid(m));
        if self.types.len() > MAX_KINDS {
            return bad(format!(
                "the generator names {} unit types, at most {MAX_KINDS} are allowed",
                self.types.len()
            ));
        }
        if self.types.iter().any(|t| kind_name(t).is_empty()) {
            return bad("every name in 'types' must be non-empty text".into());
        }
        let kinds = self.types.len().max(1);
        let coord = |v: f64| v.is_finite() && v.abs() <= crate::model::MAX_COORD;
        let too_many = |n: usize| {
            Err(LayoutError::TooMany(TooMany {
                estimated: n as u64,
                max: super::MAX_UNITS,
            }))
        };
        if let Some(r) = &self.rects {
            if r.data.len() % 5 != 0 {
                return bad(format!(
                    "rects.data has {} numbers, but a rectangle takes 5 (cx, cy, length, width, angle in degrees)",
                    r.data.len()
                ));
            }
            let n = r.data.len() / 5;
            if n > super::MAX_UNITS {
                return too_many(n);
            }
            if !r.kind.is_empty() && r.kind.len() != n {
                return bad(format!(
                    "rects.kind has {} entries for {n} rectangles",
                    r.kind.len()
                ));
            }
            for (i, d) in r.data.as_chunks::<5>().0.iter().enumerate() {
                if !d.iter().all(|v| coord(*v)) || !(d[2] > 0.0 && d[3] > 0.0) {
                    return bad(format!(
                        "rectangle {i} needs numbers up to 1e9 and a positive length and width"
                    ));
                }
                let k = r.kind.get(i).map_or(0, |k| *k as usize);
                if k >= kinds {
                    return bad(format!(
                        "unit {i} has the type {k}, but the generator names {kinds} types"
                    ));
                }
            }
        }
        if let Some(p) = &self.polygons {
            if p.points.len() % 2 != 0 {
                return bad("polygons.points holds x and y pairs".into());
            }
            let npts = p.points.len() / 2;
            if npts > MAX_TOTAL_POINTS {
                return bad(format!(
                    "the polygons have {npts} points in all, at most {MAX_TOTAL_POINTS} are allowed"
                ));
            }
            if p.start.len() < 2
                || p.start[0] != 0
                || p.start.windows(2).any(|w| w[0] > w[1])
                || p.start.last().copied().unwrap_or(0) as usize != npts
            {
                return bad(format!(
                    "polygons.start must have n + 1 entries that rise from 0 to the number of points ({npts})"
                ));
            }
            let n = p.start.len() - 1;
            if n > super::MAX_UNITS {
                return too_many(n);
            }
            if !p.kind.is_empty() && p.kind.len() != n {
                return bad(format!(
                    "polygons.kind has {} entries for {n} polygons",
                    p.kind.len()
                ));
            }
            if !p.points.iter().all(|v| coord(*v)) {
                return bad(
                    "polygons.points has a number that is not finite or is over 1e9".into(),
                );
            }
            for i in 0..n {
                let (a, b) = (p.start[i] as usize, p.start[i + 1] as usize);
                if !(3..=MAX_CELL_VERTICES).contains(&(b - a)) {
                    return bad(format!(
                        "unit {i} has {} points. A unit needs 3 to {MAX_CELL_VERTICES}",
                        b - a
                    ));
                }
                let area: f64 = (a..b)
                    .map(|k| {
                        let next = if k + 1 == b { a } else { k + 1 };
                        p.points[2 * k] * p.points[2 * next + 1]
                            - p.points[2 * next] * p.points[2 * k + 1]
                    })
                    .sum::<f64>()
                    / 2.0;
                if area == 0.0 {
                    return bad(format!("unit {i} has no area"));
                }
                let k = p.kind.get(i).map_or(0, |k| *k as usize);
                if k >= kinds {
                    return bad(format!(
                        "unit {i} has the type {k}, but the generator names {kinds} types"
                    ));
                }
            }
        }
        Ok(())
    }

    /// The shapes as rings. Checks the numbers first (see [`StoredUnits::validate`]), so nothing is
    /// allocated for a result that is refused. More than `max_units` shapes is an error.
    pub fn cells(&self, max_units: usize) -> Result<Generated, LayoutError> {
        if self.len() > max_units {
            return Err(LayoutError::TooMany(TooMany {
                estimated: self.len() as u64,
                max: max_units,
            }));
        }
        self.build()
    }

    fn build(&self) -> Result<Generated, LayoutError> {
        self.validate()?;
        let kinds: Vec<String> = if self.types.is_empty() {
            vec!["unit".to_string()]
        } else {
            self.types.iter().map(|t| kind_name(t)).collect()
        };
        let mut cells: Vec<Cell> = Vec::with_capacity(self.len());
        if let Some(r) = &self.rects {
            for (i, d) in r.data.as_chunks::<5>().0.iter().enumerate() {
                let (c, l, w) = (DVec2::new(d[0], d[1]), d[2], d[3]);
                let u = DVec2::from_angle(d[4].to_radians()) * (l / 2.0);
                let v = DVec2::from_angle(d[4].to_radians()).perp() * (w / 2.0);
                cells.push(Cell {
                    kind: r.kind.get(i).map_or(0, |k| *k as usize),
                    ring: vec![c - u - v, c + u - v, c + u + v, c - u + v],
                });
            }
        }
        if let Some(p) = &self.polygons {
            for i in 0..p.start.len().saturating_sub(1) {
                let ring: Vec<DVec2> = (p.start[i] as usize..p.start[i + 1] as usize)
                    .map(|k| DVec2::new(p.points[2 * k], p.points[2 * k + 1]))
                    .collect();
                cells.push(Cell {
                    kind: p.kind.get(i).map_or(0, |k| *k as usize),
                    ring,
                });
            }
        }
        Ok(Generated { kinds, cells })
    }
}

fn ring_area(r: &[DVec2]) -> f64 {
    (0..r.len())
        .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
        .sum::<f64>()
        / 2.0
}

/// A cell moved inward by `d`, as pieces (usually one). A rectangle is shrunk without the offset
/// code; other shapes go through it.
fn inset(ring: &[DVec2], d: f64) -> Vec<Vec<DVec2>> {
    if d <= 0.0 {
        return vec![ring.to_vec()];
    }
    if let Some(r) = inset_rectangle(ring, d) {
        return r.map_or_else(Vec::new, |r| vec![r]);
    }
    let shape = crate::model::Shape::Polyline {
        points: crate::curve::corners(ring),
        closed: true,
    };
    crate::edit2d::offset::offset_shapes(&shape, d, crate::edit2d::offset::Side::Inside)
        .map(|shapes| {
            shapes
                .iter()
                .filter_map(crate::edit2d::offset::outline_points)
                .map(|r| {
                    let mut r = crate::geom::open_ring(&r, 1e-9);
                    if ring_area(&r) < 0.0 {
                        r.reverse();
                    }
                    r
                })
                .filter(|r| r.len() >= 3 && ring_area(r) > 1e-6)
                .collect()
        })
        .unwrap_or_default()
}

/// `Some(Some(ring))`: a rectangle shrunk by `d` on every side. `Some(None)`: a rectangle that
/// disappears. `None`: not a rectangle.
fn inset_rectangle(ring: &[DVec2], d: f64) -> Option<Option<Vec<DVec2>>> {
    if ring.len() != 4 {
        return None;
    }
    let (e0, e1, e2, e3) = (
        ring[1] - ring[0],
        ring[2] - ring[1],
        ring[3] - ring[2],
        ring[0] - ring[3],
    );
    let (l0, l1) = (e0.length(), e1.length());
    let tol = 1e-9 * l0.max(l1).max(1.0);
    if l0 < tol
        || l1 < tol
        || e0.dot(e1).abs() > tol * l0.max(l1)
        || (e0 + e2).length() > tol
        || (e1 + e3).length() > tol
    {
        return None;
    }
    if l0 <= 2.0 * d || l1 <= 2.0 * d {
        return Some(None);
    }
    let (u, v) = (e0 / l0, e1 / l1);
    let mut r = vec![
        ring[0] + u * d + v * d,
        ring[1] - u * d + v * d,
        ring[2] - u * d - v * d,
        ring[3] + u * d - v * d,
    ];
    if ring_area(&r) < 0.0 {
        r.reverse();
    }
    Some(Some(r))
}

fn stored(pattern: &Pattern, max_units: usize) -> Result<Arc<Generated>, LayoutError> {
    let spec = pattern.generator.as_ref().ok_or_else(|| {
        LayoutError::Invalid("a generator pattern needs the shapes of a generator".into())
    })?;
    if spec.units.is_empty() {
        return Err(LayoutError::Invalid(
            "this generator pattern has no stored result yet. A generator (an agent with script.eval, or a package) must make the unit shapes first".into(),
        ));
    }
    let g = spec.generated()?;
    if g.cells.len() > max_units {
        return Err(LayoutError::TooMany(TooMany {
            estimated: g.cells.len() as u64,
            max: max_units,
        }));
    }
    Ok(g.clone())
}

/// The names of the unit types of a generator pattern (empty if it has no valid result).
pub fn kind_names(pattern: &Pattern) -> Vec<String> {
    stored(pattern, super::MAX_UNITS)
        .map(|g| g.kinds.clone())
        .unwrap_or_default()
}

/// Lays the cells of a generator over a region. Same contract as `for_each_unit`.
pub(super) fn lay(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    limits: &Limits,
    f: &mut dyn FnMut(&Unit),
) -> Result<Stats, LayoutError> {
    let g = stored(pattern, limits.max_units)?;
    let (origin, angle) = place.resolve(region);
    // The pieces of every cell, and the full area of each type: the largest cell of the type.
    let mut pieces: Vec<(usize, Vec<DVec2>)> = Vec::with_capacity(g.cells.len());
    let mut full = vec![0.0f64; g.kinds.len()];
    let mut vanished = 0usize;
    let mut extent: f64 = 0.0;
    // Cells far from the region cannot matter: leave them out before any work on them.
    let reach = pattern.gap.max(0.0);
    let (lo, hi) = (
        region.lo - DVec2::splat(reach),
        region.hi + DVec2::splat(reach),
    );
    for c in &g.cells {
        let (mut clo, mut chi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in &c.ring {
            clo = clo.min(*p);
            chi = chi.max(*p);
        }
        if chi.x < lo.x || clo.x > hi.x || chi.y < lo.y || clo.y > hi.y {
            continue;
        }
        let mut ring = c.ring.clone();
        if ring_area(&ring) < 0.0 {
            ring.reverse();
        }
        let parts = inset(&ring, pattern.gap / 2.0);
        if parts.is_empty() {
            vanished += 1;
            continue;
        }
        let area: f64 = parts.iter().map(|p| ring_area(p)).sum();
        full[c.kind] = full[c.kind].max(area);
        for p in parts {
            let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            for q in &p {
                lo = lo.min(*q);
                hi = hi.max(*q);
            }
            extent = extent.max((hi - lo).max_element());
            pieces.push((c.kind, p));
        }
    }
    let kinds: Vec<(String, f64)> = g.kinds.iter().cloned().zip(full.iter().copied()).collect();
    let stats = Stats {
        origin,
        angle,
        full_unit_area: kinds.first().map_or(0.0, |k| k.1),
        kinds,
        region_area: region.area,
        units: 0,
        dust_pieces: vanished,
        trimmed: 0,
        dust_at: Vec::new(),
        trimmed_at: Vec::new(),
    };
    // The cells are in drawing coordinates: the frame of the engine is the drawing.
    let local = Local::new(region, DVec2::ZERO, 0.0, extent.max(1.0));
    let mut placer = Placer::new(
        local,
        DVec2::ZERO,
        DVec2::new(1.0, 0.0),
        limits.trim.max(0.0),
        limits.max_units,
        pieces.len() as u64,
        stats,
    );
    for (kind, ring) in &pieces {
        let area = ring_area(ring);
        placer.place(ring, *kind, area, full[*kind], min_width(ring), f)?;
    }
    Ok(placer.stats)
}

/// The outlines of the units of a generator pattern as line pieces, for the drawing. `None` if
/// there are more than `max_segments`.
pub(super) fn outline_lines(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    max_segments: usize,
) -> Option<Vec<super::Segment>> {
    let mut out: Vec<super::Segment> = Vec::new();
    let mut over = false;
    let limits = Limits::new(max_segments);
    let r = super::units::for_each_unit(region, pattern, place, &limits, &mut |u| {
        if over {
            return;
        }
        for ring in &u.rings {
            let n = ring.points.len();
            for i in 0..n {
                out.push((ring.points[i], ring.points[(i + 1) % n]));
            }
        }
        over = out.len() > max_segments;
    });
    (r.is_ok() && !over).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::units::lay_units;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ]
    }

    /// A pattern whose stored result is these rings (kind, ring) and type names.
    fn pattern(gap: f64, cells: &[(u32, Vec<DVec2>)], kinds: &[&str]) -> Pattern {
        let mut p = Polygons {
            start: vec![0],
            ..Default::default()
        };
        for (k, ring) in cells {
            p.kind.push(*k);
            p.points.extend(ring.iter().flat_map(|v| [v.x, v.y]));
            p.start.push((p.points.len() / 2) as u32);
        }
        Pattern::of_generator(
            GeneratorSpec::new(
                "test",
                Value::Null,
                StoredUnits {
                    types: kinds.iter().map(|k| k.to_string()).collect(),
                    rects: None,
                    polygons: Some(p),
                },
            ),
            gap,
        )
    }

    fn region() -> Region {
        Region::from_rings(&rect(0.0, 0.0, 1000.0, 500.0), &[]).unwrap()
    }

    #[test]
    fn whole_and_cut_units_are_counted_per_type() {
        // Two cells that fit (400 x 250 and 300 x 250) and one that sticks out over the right
        // edge by half.
        let p = pattern(
            0.0,
            &[
                (0, rect(0.0, 0.0, 400.0, 250.0)),
                (1, rect(400.0, 0.0, 700.0, 250.0)),
                (0, rect(800.0, 0.0, 1200.0, 250.0)),
            ],
            &["big", "small"],
        );
        let (units, stats) =
            lay_units(&region(), &p, &Placement::default(), &Limits::default()).unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units.iter().filter(|u| u.whole).count(), 2);
        let cut = units.iter().find(|u| !u.whole).unwrap();
        assert!((cut.area - 200.0 * 250.0).abs() < 1e-6, "{}", cut.area);
        assert!((cut.share - 0.5).abs() < 1e-9, "half of a 400 x 250 unit");
        assert_eq!(cut.kind, 0);
        assert_eq!(stats.kinds[0].0, "big");
        assert!((stats.kinds[0].1 - 100_000.0).abs() < 1e-6);
        assert!((stats.kinds[1].1 - 75_000.0).abs() < 1e-6);
        assert_eq!(stats.units, 3);
        assert_eq!(kind_names(&p), ["big", "small"]);
    }

    #[test]
    fn rectangles_in_numbers_are_laid_like_polygons() {
        // The same three cells as rectangles: cx, cy, length, width, angle.
        let p = Pattern::of_generator(
            GeneratorSpec {
                units: StoredUnits {
                    types: vec!["a".into()],
                    rects: Some(Rects {
                        kind: vec![],
                        data: vec![
                            200.0, 125.0, 400.0, 250.0, 0.0, 1000.0, 125.0, 400.0, 250.0, 0.0,
                        ],
                    }),
                    polygons: None,
                },
                ..Default::default()
            },
            0.0,
        );
        let (units, _) =
            lay_units(&region(), &p, &Placement::default(), &Limits::default()).unwrap();
        assert_eq!(units.len(), 2);
        assert!(
            units[0].whole && !units[1].whole,
            "the second is cut in half at x = 1000"
        );
    }

    #[test]
    fn the_gap_takes_half_a_gap_off_every_side_of_a_cell() {
        let p = pattern(10.0, &[(0, rect(100.0, 100.0, 400.0, 300.0))], &["a"]);
        let (units, stats) =
            lay_units(&region(), &p, &Placement::default(), &Limits::default()).unwrap();
        // 300 x 200 minus 5 mm on each side: 290 x 190.
        assert!((units[0].area - 290.0 * 190.0).abs() < 1e-6);
        assert!((stats.full_unit_area - 290.0 * 190.0).abs() < 1e-6);
        assert!(units[0].whole);
    }

    #[test]
    fn a_hole_cuts_a_unit_and_a_cell_outside_the_region_is_dropped() {
        let p = pattern(
            0.0,
            &[
                (0, rect(0.0, 0.0, 400.0, 400.0)),
                (0, rect(2000.0, 0.0, 2400.0, 400.0)),
            ],
            &["a"],
        );
        let hole = rect(100.0, 100.0, 200.0, 200.0);
        let region = Region::from_rings(&rect(0.0, 0.0, 1000.0, 500.0), &[hole]).unwrap();
        let (units, _) = lay_units(&region, &p, &Placement::default(), &Limits::default()).unwrap();
        assert_eq!(units.len(), 1, "the cell far outside is not a unit");
        assert!(!units[0].whole);
        assert!((units[0].area - (160_000.0 - 10_000.0)).abs() < 1e-6);
        assert_eq!(units[0].rings.len(), 2, "the hole is a ring of the unit");
    }

    #[test]
    fn too_many_cells_no_result_and_bad_numbers_are_errors() {
        let cells: Vec<(u32, Vec<DVec2>)> = (0..5)
            .map(|i| {
                let x = i as f64 * 100.0;
                (0, rect(x, 0.0, x + 90.0, 90.0))
            })
            .collect();
        let p = pattern(0.0, &cells, &["a"]);
        let r = lay_units(&region(), &p, &Placement::default(), &Limits::new(4));
        assert!(matches!(r, Err(LayoutError::TooMany(_))), "{r:?}");
        let empty = Pattern::of_generator(GeneratorSpec::default(), 0.0);
        let r = lay_units(&region(), &empty, &Placement::default(), &Limits::default());
        assert!(matches!(r, Err(LayoutError::Invalid(m)) if m.contains("no stored result")));
        let bad_type = pattern(0.0, &[(3, rect(0.0, 0.0, 10.0, 10.0))], &["a"]);
        let r = lay_units(
            &region(),
            &bad_type,
            &Placement::default(),
            &Limits::default(),
        );
        assert!(matches!(r, Err(LayoutError::Invalid(m)) if m.contains("unit 0")));
        let mut uneven = StoredUnits {
            rects: Some(Rects {
                kind: vec![],
                data: vec![1.0; 6],
            }),
            ..Default::default()
        };
        assert!(uneven.cells(10).is_err(), "6 numbers are not a rectangle");
        uneven.rects = Some(Rects {
            kind: vec![0, 0],
            data: vec![0.0, 0.0, 10.0, 10.0, 0.0],
        });
        assert!(uneven.cells(10).is_err(), "two kinds for one rectangle");
        uneven.rects = Some(Rects {
            kind: vec![],
            data: vec![0.0, 0.0, -1.0, 10.0, 0.0],
        });
        assert!(uneven.cells(10).is_err(), "a negative length");
        let poly = |start: Vec<u32>, points: Vec<f64>| StoredUnits {
            polygons: Some(Polygons {
                kind: vec![],
                start,
                points,
            }),
            ..Default::default()
        };
        assert!(
            poly(vec![0, 3], vec![0.0; 4]).cells(10).is_err(),
            "start past the points"
        );
        assert!(
            poly(vec![0, 2], vec![0.0; 3]).cells(10).is_err(),
            "odd number of points"
        );
        assert!(
            poly(vec![0, 1, 0], vec![0.0; 2]).cells(10).is_err(),
            "start goes down"
        );
        let named = StoredUnits {
            types: vec!["  ".into()],
            ..Default::default()
        };
        assert!(named.cells(10).is_err(), "an empty name");
    }

    #[test]
    fn a_stored_result_survives_json() {
        let p = pattern(5.0, &[(0, rect(0.0, 0.0, 10.0, 10.0))], &["a"]);
        let text = serde_json::to_string(&p).unwrap();
        let back: Pattern = serde_json::from_str(&text).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn rectangles_inset_without_the_offset_code_and_others_with_it() {
        let r = inset(&rect(0.0, 0.0, 100.0, 50.0), 5.0);
        assert_eq!(r.len(), 1);
        assert!((ring_area(&r[0]) - 90.0 * 40.0).abs() < 1e-9);
        assert!(
            inset(&rect(0.0, 0.0, 8.0, 50.0), 5.0).is_empty(),
            "too thin"
        );
        let tri = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(100.0, 0.0),
            DVec2::new(0.0, 100.0),
        ];
        let t = inset(&tri, 5.0);
        assert_eq!(t.len(), 1);
        assert!(ring_area(&t[0]) < 5000.0 && ring_area(&t[0]) > 3000.0);
    }

    #[test]
    fn hostile_results_are_refused_before_anything_is_built() {
        let rect = |data: Vec<f64>| StoredUnits {
            rects: Some(Rects { kind: vec![], data }),
            ..Default::default()
        };
        assert!(rect(vec![0.0, 0.0, 10.0, 10.0, 0.0]).validate().is_ok());
        for bad in [
            vec![f64::NAN, 0.0, 10.0, 10.0, 0.0],
            vec![f64::INFINITY, 0.0, 10.0, 10.0, 0.0],
            vec![2e9, 0.0, 10.0, 10.0, 0.0],
            vec![0.0, 0.0, 1e12, 10.0, 0.0],
            vec![0.0, 0.0, 0.0, 10.0, 0.0],
        ] {
            assert!(rect(bad.clone()).validate().is_err(), "{bad:?}");
        }
        // A polygon with no area, with a coordinate over the limit, and with too few points.
        let poly = |start: Vec<u32>, points: Vec<f64>| StoredUnits {
            polygons: Some(Polygons {
                kind: vec![],
                start,
                points,
            }),
            ..Default::default()
        };
        assert!(
            poly(vec![0, 3], vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0])
                .validate()
                .is_err(),
            "a line"
        );
        assert!(
            poly(vec![0, 3], vec![0.0, 0.0, 5e9, 0.0, 0.0, 5e9])
                .validate()
                .is_err()
        );
        assert!(
            poly(vec![0, 2], vec![0.0, 0.0, 1.0, 0.0])
                .validate()
                .is_err()
        );
        assert!(
            poly(vec![0, 3], vec![0.0, 0.0, 4.0, 0.0, 0.0, 4.0])
                .validate()
                .is_ok()
        );
        // Sizes are checked before the numbers are read: this is refused by its count alone.
        let many = StoredUnits {
            rects: Some(Rects {
                kind: vec![],
                data: vec![1.0; 5 * (crate::layout::MAX_UNITS + 1)],
            }),
            ..Default::default()
        };
        assert!(matches!(many.validate(), Err(LayoutError::TooMany(_))));
        // The label and the parameters are bounded.
        let mut spec = GeneratorSpec::default();
        assert!(spec.validate().is_ok());
        spec.label = "x".repeat(MAX_LABEL + 1);
        assert!(spec.validate().is_err());
        spec.label = "line\nbreak".into();
        assert!(spec.validate().is_err());
        spec.label = "ok".into();
        spec.params = Value::String("p".repeat(MAX_PARAMS_BYTES));
        assert!(spec.validate().is_err());
    }

    #[test]
    fn the_parsed_shapes_are_made_once_and_shared_by_the_copies_of_a_pattern() {
        let p = pattern(0.0, &[(0, rect(0.0, 0.0, 10.0, 10.0))], &["a"]);
        let q = p.clone();
        let (a, b) = (
            p.generator.as_ref().unwrap().generated().unwrap(),
            q.generator.as_ref().unwrap().generated().unwrap(),
        );
        assert!(
            Arc::ptr_eq(a, b),
            "the copy shares the spec and its parsed shapes"
        );
        // A spec that is copied by itself starts without the parsed shapes, and is equal.
        let spec = (**p.generator.as_ref().unwrap()).clone();
        assert_eq!(&spec, &**p.generator.as_ref().unwrap());
    }

    #[test]
    fn cells_far_from_the_region_cost_nothing() {
        // 10,000 cells far away and one inside: only the one inside is laid.
        let mut cells: Vec<(u32, Vec<DVec2>)> = (0..10_000)
            .map(|i| {
                let x = 1.0e6 + f64::from(i) * 20.0;
                (0, rect(x, 0.0, x + 10.0, 10.0))
            })
            .collect();
        cells.push((0, rect(0.0, 0.0, 100.0, 100.0)));
        let p = pattern(0.0, &cells, &["a"]);
        let (units, stats) =
            lay_units(&region(), &p, &Placement::default(), &Limits::default()).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(stats.dust_pieces, 0);
    }
}
