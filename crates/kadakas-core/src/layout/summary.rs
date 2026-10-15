//! Counts and quantities of a layout: whole units, cut units, slivers, area laid, gap length and
//! the number of units to order.

use serde::Serialize;

use super::units::{Tally, TooMany};
use super::{MAX_GAP_SEGMENTS, MAX_UNITS, Pattern, Placement, Region, gaps};

/// Default share of a full unit below which a cut unit counts as a sliver.
pub const SLIVER_SHARE: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SummaryOptions {
    /// Cut units with less than this share of a full unit are slivers.
    pub sliver_share: f64,
    /// Waste in percent for the order quantity. There is no default: without it, the order has no
    /// waste added (every cut unit already counts as a whole unit).
    pub waste_percent: Option<f64>,
    pub max_units: usize,
    pub max_gap_segments: usize,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        SummaryOptions {
            sliver_share: SLIVER_SHARE,
            waste_percent: None,
            max_units: MAX_UNITS,
            max_gap_segments: MAX_GAP_SEGMENTS,
        }
    }
}

/// The numbers of a layout.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    pub bond: &'static str,
    pub unit_length_mm: f64,
    pub unit_width_mm: f64,
    pub gap_mm: f64,
    /// Where the pattern starts, in drawing coordinates (mm).
    pub origin: [f64; 2],
    pub angle_deg: f64,
    pub whole_units: usize,
    pub cut_units: usize,
    /// Cut units under `sliver_share` of a full unit.
    pub slivers: usize,
    pub sliver_share: f64,
    /// Clipped pieces under 1 mm2: rounding dust, not counted as units.
    pub dust_pieces: usize,
    pub unit_area_m2: f64,
    pub region_area_m2: f64,
    /// The area of all units, whole and cut.
    pub area_laid_m2: f64,
    /// The region area that the units do not cover: the gaps.
    pub gap_area_m2: f64,
    /// The length of the gap center lines. `None` if there were too many lines to make.
    pub gap_length_m: Option<f64>,
    /// Units to order: whole units plus cut units, every cut unit counted as one unit.
    pub order_units: usize,
    pub waste_percent: Option<f64>,
    /// `order_units` plus the waste, rounded up. `None` without `waste_percent`.
    pub order_units_with_waste: Option<usize>,
}

/// Counts the units of a layout and measures the gaps. Fails when the layout has more units than
/// `max_units`.
pub fn summarize(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    opts: &SummaryOptions,
) -> Result<Summary, TooMany> {
    let mut tally = Tally::default();
    let stats = super::units::for_each_unit(region, pattern, place, opts.max_units, &mut |u| {
        tally.add(u, opts.sliver_share)
    })?;
    let g = gaps::gap_lines(region, pattern, place, opts.max_gap_segments, false);
    let order = tally.whole + tally.cut;
    Ok(Summary {
        bond: pattern.bond.id(),
        unit_length_mm: pattern.unit_length,
        unit_width_mm: pattern.unit_width,
        gap_mm: pattern.gap,
        origin: [stats.origin.x, stats.origin.y],
        angle_deg: stats.angle,
        whole_units: tally.whole,
        cut_units: tally.cut,
        slivers: tally.slivers,
        sliver_share: opts.sliver_share,
        dust_pieces: stats.dust_pieces,
        unit_area_m2: stats.full_unit_area / 1e6,
        region_area_m2: region.area / 1e6,
        area_laid_m2: tally.area / 1e6,
        gap_area_m2: ((region.area - tally.area) / 1e6).max(0.0),
        gap_length_m: (!g.truncated).then(|| gaps::length(&g.segments) / 1000.0),
        order_units: order,
        waste_percent: opts.waste_percent,
        order_units_with_waste: opts
            .waste_percent
            .map(|w| (order as f64 * (1.0 + w / 100.0)).ceil() as usize),
    })
}
