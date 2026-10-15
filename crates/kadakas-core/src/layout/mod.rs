//! The unit layout engine: lays equal units (pavers, tiles, boards, bricks) in a bond over a
//! region, and clips them exactly to the region.
//!
//! Input: a [`Region`] (an outer ring, curves allowed, and holes), a [`Pattern`] (unit length and
//! width, gap width, bond: running bond, stack, herringbone or basket weave) and a
//! [`Placement`] (the start point and the angle of the pattern, or an edge of the outer ring to
//! align to).
//!
//! Output: the units, whole or cut, each with its exact polygon, its area and its share of a
//! full unit ([`units::for_each_unit`]), and the center lines of the gaps ([`gaps::gap_lines`]).
//! Nothing is dropped without a message: a job over the limit is an error with the number of units
//! it would make, and clipped pieces that are too small to count are counted as `dust`.
//!
//! The module is pure geometry on top of `curve` and `geom`, so it builds for wasm and has no
//! knowledge of hardscapes, landscape areas or any other object. It lives in `kadakas-core`
//! because the polygon clipper (`i_overlay`), the curve flattening and the document objects are
//! here already; a separate crate would only add a dependency edge.
//!
//! Limits, from a measurement (see `bounds` below and the test `measure_bounds`): the on-screen
//! drawing makes at most [`gaps::MAX_DRAW_SEGMENTS`] gap lines. A layout for export or a report
//! makes at most [`MAX_UNITS`] units.

pub mod bond;
pub mod gaps;
pub mod object;
pub mod summary;
pub mod units;

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};

pub use gaps::{Gaps, gap_lines};
pub use units::{Stats, Tally, TooMany, Unit, UnitRing, for_each_unit, lay_units};

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
    /// Blocks of two units, turned by 90 degrees from block to block.
    BasketWeave,
}

impl Bond {
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
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        let key = s.trim().replace([' ', '-'], "_");
        Self::ALL
            .into_iter()
            .find(|k| k.id().eq_ignore_ascii_case(&key))
    }
}

/// Units of `unit_length` by `unit_width` with `gap` between them, in a bond.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub bond: Bond,
    pub unit_length: f64,
    pub unit_width: f64,
    pub gap: f64,
}

impl Pattern {
    /// Smallest unit side, in mm.
    pub const MIN_UNIT: f64 = 10.0;

    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && v > 0.0 && v <= crate::model::MAX_COORD;
        if !ok(self.unit_length) || !ok(self.unit_width) {
            return Err("the unit length and width must be positive, finite numbers".into());
        }
        if !(self.gap.is_finite() && self.gap >= 0.0 && self.gap <= crate::model::MAX_COORD) {
            return Err("the gap must be a finite number of zero or more".into());
        }
        Ok(())
    }

    /// The area of a full unit, in mm2.
    pub fn full_unit_area(&self) -> f64 {
        bond::full_unit_area(self)
    }
}

/// Where the pattern starts and how it is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    /// The corner of a unit that sits at the start of the pattern. `None`: the middle of the
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
    pub lo: DVec2,
    pub hi: DVec2,
}

fn paths(rings: &[Vec<DVec2>]) -> Vec<Vec<[f64; 2]>> {
    rings
        .iter()
        .map(|r| r.iter().map(|p| [p.x, p.y]).collect())
        .collect()
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
        Region::from_rings(&ring, &holes)
    }

    /// A region from flat rings.
    pub fn from_rings(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Result<Region, String> {
        let outer = crate::geom::open_ring(outer, 1e-9);
        if outer.len() < 3 {
            return Err("a region needs an outline of 3 or more points".into());
        }
        if outer
            .iter()
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
        let shapes: Vec<Vec<Vec<DVec2>>> = if holes.is_empty() {
            vec![vec![outer]]
        } else {
            let out =
                paths(&[outer]).overlay(&paths(&holes), OverlayRule::Difference, FillRule::NonZero);
            out.into_iter()
                .map(|s| {
                    s.into_iter()
                        .map(|r| r.into_iter().map(|p| DVec2::new(p[0], p[1])).collect())
                        .collect()
                })
                .collect()
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

    /// All rings: outer rings and holes.
    pub fn rings(&self) -> impl Iterator<Item = &Vec<DVec2>> {
        self.shapes.iter().flatten()
    }
}

#[cfg(test)]
mod tests;
