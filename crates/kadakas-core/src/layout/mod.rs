//! The unit layout engine: lays units (pavers, tiles, boards, bricks) in a bond or on a module
//! lattice over a region, and clips them exactly to the region.
//!
//! Input: a [`Region`] (an outer ring, curves allowed, and holes), a [`Pattern`] (one of the four
//! bonds with a unit length and width, or a [`Module`] of your own shapes; and the gap width) and a
//! [`Placement`] (the start point and the angle of the pattern, or an edge of the outer ring to
//! align to).
//!
//! Output: the units, whole or cut, each with its exact polygon, its area and its share of a
//! full unit ([`units::for_each_unit`]), and the center lines of the gaps ([`gaps::gap_lines`]).
//! Nothing is dropped without a message: a job over the limit is an error with the number of units
//! it would make, and clipped pieces that are too small to count are counted as `dust`.
//!
//! How the gap works: every pattern is a set of cells that tile the plane (or leave spaces). The
//! cell outlines run at the centers of the joints. A unit is its cell moved inward by half a gap.
//! So a whole unit of the four bonds is exactly `unit_length` by `unit_width`, for any ratio.
//! The four bonds are built as modules by [`bond::compile`]; the rest of the engine knows only
//! modules.
//!
//! The module is pure geometry on top of `curve` and `geom`, so it builds for wasm and has no
//! knowledge of hardscapes, landscape areas or any other object. It lives in `kadakas-core`
//! because the polygon clipper (`i_overlay`), the curve flattening and the document objects are
//! here already; a separate crate would only add a dependency edge.
//!
//! Limits, from a measurement (see the test `measure_bounds`): the on-screen drawing makes at
//! most [`gaps::MAX_DRAW_SEGMENTS`] gap lines. A layout for export or a report makes at most
//! [`MAX_UNITS`] units.

pub mod bond;
pub mod check;
pub mod course;
pub mod gaps;
pub mod generator;
pub mod module;
pub mod object;
pub mod plan;
pub mod summary;
pub mod units;
pub mod zones;

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};

pub use gaps::{Gaps, gap_lines};
pub use module::{Module, ModuleUnit};
pub use units::{
    KindTally, LayoutError, Limits, Stats, Tally, TooMany, Unfillable, Unit, UnitRing,
    for_each_unit, lay_units,
};

/// Most units that a layout for export or a report makes. About 3.5 times a 5,000 m2 lot of
/// 200 x 100 mm pavers (about 250,000 units). Measured in `measure_bounds`.
pub const MAX_UNITS: usize = 1_000_000;
/// Most gap center lines that a layout for export or a report makes.
pub const MAX_GAP_SEGMENTS: usize = 2_000_000;
/// Flattening tolerance of curves, in mm, when nothing else is given.
pub const TOLERANCE: f64 = 1.0;
/// A clipped piece with less area than this (mm2) is rounding dust, not a unit.
pub const DUST_AREA: f64 = 1.0;
/// Largest number of vertices of a region.
pub const MAX_VERTICES: usize = 500_000;

/// A straight piece of gap center line.
pub type Segment = (DVec2, DVec2);

/// The way units are laid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bond {
    /// Rows, each row shifted by half a unit.
    #[default]
    RunningBond,
    /// Rows and columns line up.
    Stack,
    /// Units at a right angle to each other, in diagonal stripes.
    Herringbone,
    /// Blocks of units, turned by 90 degrees from block to block.
    BasketWeave,
    /// Your own unit shapes on a lattice of two repeat vectors (see [`module`]).
    Module,
    /// A generator makes the unit shapes; the pattern stores the last result (see [`generator`]).
    Generator,
}

impl Bond {
    /// The four bonds of rectangular units. A module is not in this list.
    pub const ALL: [Bond; 4] = [
        Bond::RunningBond,
        Bond::Stack,
        Bond::Herringbone,
        Bond::BasketWeave,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Bond::RunningBond => "running_bond",
            Bond::Stack => "stack",
            Bond::Herringbone => "herringbone",
            Bond::BasketWeave => "basket_weave",
            Bond::Module => "module",
            Bond::Generator => "generator",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        let key = s.trim().replace([' ', '-'], "_");
        Self::ALL
            .into_iter()
            .chain([Bond::Module, Bond::Generator])
            .find(|k| k.id().eq_ignore_ascii_case(&key))
    }
}

/// Units with `gap` between them, in a bond. The four bonds lay units of `unit_length` by
/// `unit_width`: every whole unit has exactly this size, also in a herringbone or a basket weave.
/// A module lays its own shapes (`unit_length` and `unit_width` are not used).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub bond: Bond,
    pub unit_length: f64,
    pub unit_width: f64,
    pub gap: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Module>,
    /// Generator: the script and its parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<generator::GeneratorSpec>,
}

impl Pattern {
    /// Smallest unit side, in mm.
    pub const MIN_UNIT: f64 = 10.0;

    /// A pattern of one of the four bonds.
    pub fn new(bond: Bond, unit_length: f64, unit_width: f64, gap: f64) -> Pattern {
        Pattern {
            bond,
            unit_length,
            unit_width,
            gap,
            module: None,
            generator: None,
        }
    }

    /// A pattern whose unit shapes a script makes (see [`generator`]).
    pub fn of_generator(spec: generator::GeneratorSpec, gap: f64) -> Pattern {
        Pattern {
            bond: Bond::Generator,
            unit_length: 0.0,
            unit_width: 0.0,
            gap,
            module: None,
            generator: Some(spec),
        }
    }

    /// A module pattern.
    pub fn of_module(module: Module, gap: f64) -> Pattern {
        Pattern {
            bond: Bond::Module,
            unit_length: 0.0,
            unit_width: 0.0,
            gap,
            module: Some(module),
            generator: None,
        }
    }

    /// The same bond with the units `factor` times larger (the gap stays). A module stays as it is.
    pub fn scaled(&self, factor: f64) -> Pattern {
        Pattern {
            unit_length: self.unit_length * factor,
            unit_width: self.unit_width * factor,
            ..self.clone()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(self.gap.is_finite() && self.gap >= 0.0 && self.gap <= crate::model::MAX_COORD) {
            return Err("the gap must be a finite number of zero or more".into());
        }
        if self.bond == Bond::Generator {
            // A generator pattern may have no result yet; a result must be sound.
            return match &self.generator {
                Some(g) if g.units.is_empty() => Ok(()),
                Some(g) => g
                    .units
                    .cells(MAX_UNITS)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                None => Err("a generator pattern needs a generator (generator: {units})".into()),
            };
        }
        if self.bond != Bond::Module {
            let ok = |v: f64| v.is_finite() && v > 0.0 && v <= crate::model::MAX_COORD;
            if !ok(self.unit_length) || !ok(self.unit_width) {
                return Err("the unit length and width must be positive, finite numbers".into());
            }
        }
        self.compile().map(|_| ())
    }

    /// The lattice and the unit shapes of the pattern.
    pub fn compile(&self) -> Result<module::Compiled, String> {
        match (self.bond, &self.module) {
            (Bond::Module, Some(m)) => m.compile(self.gap),
            (Bond::Module, None) => {
                Err("a module pattern needs its units and repeat vectors".into())
            }
            (Bond::Generator, _) => {
                Err("a generator pattern has no lattice: a script makes its units".into())
            }
            _ => bond::compile(self),
        }
    }

    /// The area of a full unit, in mm2. For a module, the area of its first unit shape.
    pub fn full_unit_area(&self) -> f64 {
        self.compile()
            .ok()
            .and_then(|c| c.kinds.first().map(|k| k.area))
            .unwrap_or(0.0)
    }

    /// The widest joint of the pattern, in mm. It is the gap, except in a basket weave whose
    /// blocks do not fit exactly: the difference goes into the joints, never into the unit.
    pub fn widest_joint(&self) -> f64 {
        bond::widest_joint(self)
    }
}

/// Where the pattern starts and how it is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    /// The corner of the first cell of the pattern: the cells (unit plus half a gap on each side)
    /// start here, so the first unit sits half a gap in from this point. `None`: the middle of the
    /// box of the region.
    pub origin: Option<DVec2>,
    /// Rotation about the origin, in degrees, counterclockwise.
    pub angle: f64,
}

impl Placement {
    /// The origin and the angle for a region.
    pub fn resolve(&self, region: &Region) -> (DVec2, f64) {
        (
            self.origin.unwrap_or((region.lo + region.hi) / 2.0),
            self.angle,
        )
    }

    /// A placement that starts at vertex `index` of the outer ring and runs along the edge to the
    /// next vertex (a curved edge counts as its chord).
    pub fn along_edge(outer: &[Vertex], index: usize) -> Result<Placement, String> {
        if index >= outer.len() {
            return Err(format!(
                "edge {index} does not exist: the outline has {} vertices",
                outer.len()
            ));
        }
        let (a, b) = (outer[index].p, outer[(index + 1) % outer.len()].p);
        if a.distance(b) < 1e-9 {
            return Err(format!("edge {index} has no length"));
        }
        let d = b - a;
        Ok(Placement {
            origin: Some(a),
            angle: d.y.atan2(d.x).to_degrees(),
        })
    }
}

/// An area to fill: shapes made of an outer ring and holes. Built from an outline and holes, the
/// shapes are normalized (holes are cut out, overlapping holes are merged).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// Every shape: the outer ring first, then its holes.
    pub shapes: Vec<Vec<Vec<DVec2>>>,
    /// The area, in mm2.
    pub area: f64,
    /// The box of the outline. For a region made from vertices it is the exact box of the curves.
    pub lo: DVec2,
    pub hi: DVec2,
}

fn paths(rings: &[Vec<DVec2>]) -> Vec<Vec<[f64; 2]>> {
    rings
        .iter()
        .map(|r| r.iter().map(|p| [p.x, p.y]).collect())
        .collect()
}

fn to_shapes(out: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Vec<Vec<DVec2>>> {
    out.into_iter()
        .map(|s| {
            s.into_iter()
                .map(|r| r.into_iter().map(|p| DVec2::new(p[0], p[1])).collect())
                .collect()
        })
        .collect()
}

/// The area of `inner` that lies outside `outer`, in mm2 (both are flat rings).
pub fn area_outside(inner: &[DVec2], outer: &[DVec2]) -> f64 {
    let shapes = paths(&[inner.to_vec()]).overlay(
        &paths(&[outer.to_vec()]),
        OverlayRule::Difference,
        FillRule::NonZero,
    );
    shapes_area(&to_shapes(shapes))
}

/// The area of the part that two flat rings share, in mm2.
pub fn area_shared(a: &[DVec2], b: &[DVec2]) -> f64 {
    let shapes = paths(&[a.to_vec()]).overlay(
        &paths(&[b.to_vec()]),
        OverlayRule::Intersect,
        FillRule::NonZero,
    );
    shapes_area(&to_shapes(shapes))
}

/// The area that two shapes share, in mm2. A shape is its outline and then its holes.
pub fn shapes_overlap(a: &[Vec<DVec2>], b: &[Vec<DVec2>]) -> f64 {
    let shapes = paths(a).overlay(&paths(b), OverlayRule::Intersect, FillRule::EvenOdd);
    shapes_area(&to_shapes(shapes))
}

/// The parts of `inner` that lie inside `outer`, as flat rings (holes of the parts are dropped).
pub fn clip_ring_to(inner: &[DVec2], outer: &[DVec2]) -> Vec<Vec<DVec2>> {
    to_shapes(paths(&[inner.to_vec()]).overlay(
        &paths(&[outer.to_vec()]),
        OverlayRule::Intersect,
        FillRule::NonZero,
    ))
    .into_iter()
    .filter_map(|mut s| (!s.is_empty()).then(|| s.swap_remove(0)))
    .collect()
}

fn shapes_area(shapes: &[Vec<Vec<DVec2>>]) -> f64 {
    shapes
        .iter()
        .map(|s| {
            s.iter()
                .enumerate()
                .map(|(i, r)| {
                    let a = crate::geom::polygon_area(r);
                    if i == 0 { a } else { -a }
                })
                .sum::<f64>()
        })
        .sum()
}

/// The area that counts as noise for a ring, in mm2: a strip of 0.05 mm along its edge, and one
/// millionth of its area.
pub fn noise_area(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    let perimeter: f64 = (0..n).map(|i| ring[i].distance(ring[(i + 1) % n])).sum();
    0.05 * perimeter + 1e-6 * crate::geom::polygon_area(ring)
}

/// True if the ring `inner` lies inside `outer`, or leaves it by less than noise (see
/// [`noise_area`]). A vertex that sits 0.01 mm outside a kerb line still counts as inside; a corner
/// that pokes into a concave notch does not.
pub fn ring_within(inner: &[DVec2], outer: &[DVec2]) -> bool {
    area_outside(inner, outer) <= noise_area(inner).max(1.0)
}

impl Region {
    /// A region from vertices (curves are flattened to `tolerance`).
    pub fn new(outer: &[Vertex], holes: &[Vec<Vertex>], tolerance: f64) -> Result<Region, String> {
        if outer.len() + holes.iter().map(Vec::len).sum::<usize>() > MAX_VERTICES {
            return Err(format!("a region has at most {MAX_VERTICES} vertices"));
        }
        let ring = curve::flatten(outer, true, tolerance);
        let holes: Vec<Vec<DVec2>> = holes
            .iter()
            .map(|h| curve::flatten(h, true, tolerance))
            .collect();
        let mut r = Region::from_rings(&ring, &holes)?;
        // The pattern starts at the middle of the exact box of the curves.
        if outer.iter().all(Vertex::is_finite) {
            let (lo, hi) = curve::bounds(outer, true);
            if lo.is_finite() && hi.is_finite() {
                r.lo = lo;
                r.hi = hi;
            }
        }
        Ok(r)
    }

    /// A region from flat rings.
    pub fn from_rings(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Result<Region, String> {
        Region::from_paths(&[outer.to_vec()], holes)
    }

    /// A region from several outer rings (they must not overlap) and holes.
    pub fn from_paths(outers: &[Vec<DVec2>], holes: &[Vec<DVec2>]) -> Result<Region, String> {
        let outers: Vec<Vec<DVec2>> = outers
            .iter()
            .map(|o| crate::geom::open_ring(o, 1e-9))
            .filter(|o| o.len() >= 3)
            .collect();
        if outers.is_empty() {
            return Err("a region needs an outline of 3 or more points".into());
        }
        if outers
            .iter()
            .flatten()
            .chain(holes.iter().flatten())
            .any(|p| !p.is_finite())
        {
            return Err("numbers must be finite".into());
        }
        let holes: Vec<Vec<DVec2>> = holes
            .iter()
            .map(|h| crate::geom::open_ring(h, 1e-9))
            .filter(|h| h.len() >= 3)
            .collect();
        let shapes: Vec<Vec<Vec<DVec2>>> = if holes.is_empty() && outers.len() == 1 {
            vec![outers]
        } else {
            to_shapes(paths(&outers).overlay(
                &paths(&holes),
                OverlayRule::Difference,
                FillRule::NonZero,
            ))
        };
        let mut area = 0.0;
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for s in &shapes {
            for (i, r) in s.iter().enumerate() {
                let a = crate::geom::polygon_area(r);
                area += if i == 0 { a } else { -a };
                if i == 0 {
                    for p in r {
                        lo = lo.min(*p);
                        hi = hi.max(*p);
                    }
                }
            }
        }
        if shapes.is_empty() || area <= 0.0 {
            return Err("the holes cover the whole region".into());
        }
        Ok(Region {
            shapes,
            area,
            lo,
            hi,
        })
    }

    /// A region from shapes that are normalized already: each shape is its outer ring and then its
    /// holes. Nothing is clipped or merged, so the points stay exactly as given (zones that share
    /// an edge keep sharing it). The shapes must not overlap.
    pub fn from_shapes(shapes: Vec<Vec<Vec<DVec2>>>) -> Result<Region, String> {
        let shapes: Vec<Vec<Vec<DVec2>>> = shapes
            .into_iter()
            .filter_map(|s| {
                let rings: Vec<Vec<DVec2>> = s
                    .iter()
                    .map(|r| crate::geom::open_ring(r, 1e-9))
                    .filter(|r| r.len() >= 3)
                    .collect();
                // The outline must survive; a hole that collapsed is dropped.
                (s.first()
                    .is_some_and(|o| crate::geom::open_ring(o, 1e-9).len() >= 3))
                .then_some(rings)
            })
            .collect();
        if shapes.is_empty() {
            return Err("a region needs an outline of 3 or more points".into());
        }
        if shapes.iter().flatten().flatten().any(|p| !p.is_finite()) {
            return Err("numbers must be finite".into());
        }
        let mut area = 0.0;
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for s in &shapes {
            for (i, r) in s.iter().enumerate() {
                let a = crate::geom::polygon_area(r);
                area += if i == 0 { a } else { -a };
                if i == 0 {
                    for p in r {
                        lo = lo.min(*p);
                        hi = hi.max(*p);
                    }
                }
            }
        }
        if area <= 0.0 {
            return Err("the holes cover the whole region".into());
        }
        Ok(Region {
            shapes,
            area,
            lo,
            hi,
        })
    }

    /// All rings: outer rings and holes.
    pub fn rings(&self) -> impl Iterator<Item = &Vec<DVec2>> {
        self.shapes.iter().flatten()
    }

    /// The length of all rings, in mm: the outline and the edges of the holes.
    pub fn perimeter(&self) -> f64 {
        self.rings()
            .map(|r| {
                (0..r.len())
                    .map(|i| r[i].distance(r[(i + 1) % r.len()]))
                    .sum::<f64>()
            })
            .sum()
    }
}

#[cfg(test)]
mod tests;
