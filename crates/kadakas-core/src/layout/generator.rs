//! Generated patterns (PAT-09): a script makes the unit shapes, Kadakas lays them.
//!
//! A pattern of the kind `generator` names a script and gives it parameters. The core does not
//! run scripts. A host installs a [`UnitGenerator`] for the thread (`kadakas-api` does, with the
//! script engine), and the layout asks it for the cells of a region. A cell is a closed shape
//! with a type name, **not clipped**: it can reach over the edge of the region or over a hole.
//! Everything after that is the work of the engine, the same as for the other kinds: the cells are
//! moved inward by half a gap, clipped to the region and its holes, sorted into whole and cut
//! units, counted per type, checked and exported.
//!
//! Like the cells of a bond or a module, the shapes of a generator are the outlines at the
//! centers of the joints, so a generator is written once and works for any gap.
//!
//! A generator is a pure function of the region, the parameters, the origin and the angle. The
//! host runs it with fixed limits (fuel, time, memory, unit count) and may keep the result for
//! the same input, so a pattern is "regenerated" when anything it depends on changes.

use std::cell::RefCell;
use std::sync::Arc;

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

/// Which script a generator pattern runs, and with what parameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneratorSpec {
    /// A script resource: its ID or its name.
    pub script: String,
    /// Passed to the script as `args.params`.
    pub params: Value,
}

/// One cell: a unit type and a closed shape in drawing coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// The index into [`Generated::kinds`].
    pub kind: usize,
    pub ring: Vec<DVec2>,
}

/// What a generator made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    /// The names of the unit types.
    pub kinds: Vec<String>,
    pub cells: Vec<Cell>,
}

/// What a generator is asked for.
pub struct Request<'a> {
    pub spec: &'a GeneratorSpec,
    pub region: &'a Region,
    /// Where the pattern starts, and how it is turned (degrees). The script may use them or not.
    pub origin: DVec2,
    pub angle: f64,
    /// The width of a joint. The cells are the outlines at the centers of the joints.
    pub gap: f64,
    /// The most cells the generator may make. More is an error.
    pub max_units: usize,
}

/// Makes cells for a region. The host implements it (the script engine).
pub trait UnitGenerator {
    fn generate(&self, request: &Request<'_>) -> Result<Arc<Generated>, String>;
}

thread_local! {
    static INSTALLED: RefCell<Option<Arc<dyn UnitGenerator>>> = const { RefCell::new(None) };
}

/// Installs the generator of this thread (`None` removes it). A layout of a generator pattern
/// without one is an error that says so.
pub fn install(generator: Option<Arc<dyn UnitGenerator>>) {
    INSTALLED.with(|g| *g.borrow_mut() = generator);
}

/// The generator of this thread.
pub fn installed() -> Option<Arc<dyn UnitGenerator>> {
    INSTALLED.with(|g| g.borrow().clone())
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

/// Checks what a generator returned.
pub fn check(g: &Generated, max_units: usize) -> Result<(), LayoutError> {
    let bad = |m: String| Err(LayoutError::Invalid(m));
    if g.cells.len() > max_units {
        return Err(LayoutError::TooMany(TooMany {
            estimated: g.cells.len() as u64,
            max: max_units,
        }));
    }
    if g.kinds.len() > MAX_KINDS {
        return bad(format!(
            "the generator names {} unit types, at most {MAX_KINDS} are allowed",
            g.kinds.len()
        ));
    }
    for (i, c) in g.cells.iter().enumerate() {
        if c.kind >= g.kinds.len() {
            return bad(format!(
                "unit {i} has the type {}, but the generator names {} types",
                c.kind,
                g.kinds.len()
            ));
        }
        if c.ring.len() < 3 || c.ring.len() > MAX_CELL_VERTICES {
            return bad(format!(
                "unit {i} has {} points. A unit needs 3 to {MAX_CELL_VERTICES}",
                c.ring.len()
            ));
        }
        if c.ring.iter().any(|p| !p.is_finite()) {
            return bad(format!("unit {i} has a number that is not finite"));
        }
    }
    Ok(())
}

fn request<'a>(
    spec: &'a GeneratorSpec,
    region: &'a Region,
    pattern: &Pattern,
    place: &Placement,
    max_units: usize,
) -> Request<'a> {
    let (origin, angle) = place.resolve(region);
    Request {
        spec,
        region,
        origin,
        angle,
        gap: pattern.gap,
        max_units,
    }
}

fn spec_of(pattern: &Pattern) -> Result<&GeneratorSpec, LayoutError> {
    pattern.generator.as_ref().ok_or_else(|| {
        LayoutError::Invalid("a generator pattern needs a script (generator.script)".into())
    })
}

fn generate(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    max_units: usize,
) -> Result<Arc<Generated>, LayoutError> {
    let spec = spec_of(pattern)?;
    let host = installed().ok_or_else(|| {
        LayoutError::Invalid(
            "this pattern is made by a script, and no script engine is available here".into(),
        )
    })?;
    let g = host
        .generate(&request(spec, region, pattern, place, max_units))
        .map_err(LayoutError::Invalid)?;
    check(&g, max_units)?;
    Ok(g)
}

/// The names of the unit types of a generator pattern for a region (empty if the generator fails).
pub fn kind_names(region: &Region, pattern: &Pattern, place: &Placement) -> Vec<String> {
    generate(region, pattern, place, super::MAX_UNITS)
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
    let g = generate(region, pattern, place, limits.max_units)?;
    let (origin, angle) = place.resolve(region);
    // The pieces of every cell, and the full area of each type: the largest cell of the type.
    let mut pieces: Vec<(usize, Vec<DVec2>)> = Vec::with_capacity(g.cells.len());
    let mut full = vec![0.0f64; g.kinds.len()];
    let mut vanished = 0usize;
    let mut extent: f64 = 0.0;
    for c in &g.cells {
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

    struct Fixed(Generated);

    impl UnitGenerator for Fixed {
        fn generate(&self, _r: &Request<'_>) -> Result<Arc<Generated>, String> {
            Ok(Arc::new(self.0.clone()))
        }
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ]
    }

    fn pattern(gap: f64) -> Pattern {
        Pattern::of_generator(
            GeneratorSpec {
                script: "s".into(),
                params: Value::Null,
            },
            gap,
        )
    }

    /// A 1000 x 500 mm region with three cells: two that fit (400 x 250 and 300 x 250) and one
    /// that sticks out over the right edge by half.
    fn setup(cells: Vec<Cell>, kinds: &[&str]) -> Region {
        install(Some(Arc::new(Fixed(Generated {
            kinds: kinds.iter().map(|k| k.to_string()).collect(),
            cells,
        }))));
        Region::from_rings(&rect(0.0, 0.0, 1000.0, 500.0), &[]).unwrap()
    }

    #[test]
    fn whole_and_cut_units_are_counted_per_type() {
        let region = setup(
            vec![
                Cell {
                    kind: 0,
                    ring: rect(0.0, 0.0, 400.0, 250.0),
                },
                Cell {
                    kind: 1,
                    ring: rect(400.0, 0.0, 700.0, 250.0),
                },
                Cell {
                    kind: 0,
                    ring: rect(800.0, 0.0, 1200.0, 250.0),
                },
            ],
            &["big", "small"],
        );
        let (units, stats) = lay_units(
            &region,
            &pattern(0.0),
            &Placement::default(),
            &Limits::default(),
        )
        .unwrap();
        install(None);
        assert_eq!(units.len(), 3);
        let whole = units.iter().filter(|u| u.whole).count();
        assert_eq!(whole, 2, "two fit; the third is cut at x = 1000");
        let cut = units.iter().find(|u| !u.whole).unwrap();
        assert!((cut.area - 200.0 * 250.0).abs() < 1e-6, "{}", cut.area);
        assert!((cut.share - 0.5).abs() < 1e-9, "half of a 400 x 250 unit");
        assert_eq!(cut.kind, 0);
        assert_eq!(stats.kinds[0].0, "big");
        assert!((stats.kinds[0].1 - 100_000.0).abs() < 1e-6);
        assert!((stats.kinds[1].1 - 75_000.0).abs() < 1e-6);
        assert_eq!(stats.units, 3);
    }

    #[test]
    fn the_gap_takes_half_a_gap_off_every_side_of_a_cell() {
        let region = setup(
            vec![Cell {
                kind: 0,
                ring: rect(100.0, 100.0, 400.0, 300.0),
            }],
            &["a"],
        );
        let (units, stats) = lay_units(
            &region,
            &pattern(10.0),
            &Placement::default(),
            &Limits::default(),
        )
        .unwrap();
        install(None);
        // 300 x 200 minus 5 mm on each side: 290 x 190.
        assert!((units[0].area - 290.0 * 190.0).abs() < 1e-6);
        assert!((stats.full_unit_area - 290.0 * 190.0).abs() < 1e-6);
        assert!(units[0].whole);
    }

    #[test]
    fn a_hole_cuts_a_unit_and_a_cell_outside_the_region_is_dropped() {
        install(Some(Arc::new(Fixed(Generated {
            kinds: vec!["a".into()],
            cells: vec![
                Cell {
                    kind: 0,
                    ring: rect(0.0, 0.0, 400.0, 400.0),
                },
                Cell {
                    kind: 0,
                    ring: rect(2000.0, 0.0, 2400.0, 400.0),
                },
            ],
        }))));
        let hole = rect(100.0, 100.0, 200.0, 200.0);
        let region = Region::from_rings(&rect(0.0, 0.0, 1000.0, 500.0), &[hole]).unwrap();
        let (units, _) = lay_units(
            &region,
            &pattern(0.0),
            &Placement::default(),
            &Limits::default(),
        )
        .unwrap();
        install(None);
        assert_eq!(units.len(), 1, "the cell far outside is not a unit");
        assert!(!units[0].whole);
        assert!((units[0].area - (160_000.0 - 10_000.0)).abs() < 1e-6);
        assert_eq!(units[0].rings.len(), 2, "the hole is a ring of the unit");
    }

    #[test]
    fn too_many_cells_is_an_error_and_so_is_a_missing_generator() {
        let region = setup(
            (0..5)
                .map(|i| Cell {
                    kind: 0,
                    ring: rect(i as f64 * 100.0, 0.0, i as f64 * 100.0 + 90.0, 90.0),
                })
                .collect(),
            &["a"],
        );
        let r = lay_units(
            &region,
            &pattern(0.0),
            &Placement::default(),
            &Limits::new(4),
        );
        assert!(matches!(r, Err(LayoutError::TooMany(_))), "{r:?}");
        install(None);
        let r = lay_units(
            &region,
            &pattern(0.0),
            &Placement::default(),
            &Limits::default(),
        );
        assert!(matches!(r, Err(LayoutError::Invalid(m)) if m.contains("no script engine")));
    }

    #[test]
    fn a_bad_answer_is_refused_with_the_unit_number() {
        let region = setup(
            vec![Cell {
                kind: 3,
                ring: rect(0.0, 0.0, 10.0, 10.0),
            }],
            &["a"],
        );
        let r = lay_units(
            &region,
            &pattern(0.0),
            &Placement::default(),
            &Limits::default(),
        );
        install(None);
        assert!(matches!(r, Err(LayoutError::Invalid(m)) if m.contains("unit 0")));
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
}
