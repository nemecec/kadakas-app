//! Counts and quantities of a layout: whole units, cut units, slivers, area laid, gap length and
//! the number of units to order, in total and per unit shape.

use serde::Serialize;

use super::units::{LayoutError, Limits, Stats, Tally, Unfillable, for_each_unit};
use super::{MAX_GAP_SEGMENTS, MAX_UNITS, Pattern, Placement, Region, gaps};

/// Default share of a full unit below which a cut unit counts as a sliver.
pub const SLIVER_SHARE: f64 = 0.25;
/// Default share of a full unit below which a cut unit is too small to lay.
pub const UNFILLABLE_SHARE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SummaryOptions {
    /// Cut units with less than this share of a full unit are slivers.
    pub sliver_share: f64,
    /// Cut units with less than this share of a full unit cannot be laid (`unfillable_pieces`).
    pub unfillable_share: f64,
    /// Cut units narrower than this (mm) cannot be laid. 0: off.
    pub min_width: f64,
    /// Leave the unfillable pieces out of the order (`order_units`). They stay counted in
    /// `unfillable_pieces` and in `order_units_max`. Off by default.
    pub exclude_unfillable: bool,
    /// Waste in percent for the order quantity. There is no default: without it, the order has no
    /// waste added (every cut unit already counts as a whole unit).
    pub waste_percent: Option<f64>,
    /// A unit that crosses the edge by less than this (mm) counts as whole.
    pub trim: f64,
    pub max_units: usize,
    pub max_gap_segments: usize,
    /// Measure the length of the gap lines. Off by default: it is more work than the counts.
    pub gap_length: bool,
    /// A joint wider than the gap by more than this (mm) is reported in `joint_warning`. The
    /// joints of a basket weave get wider when its blocks do not fit exactly.
    pub max_extra_joint_mm: f64,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        SummaryOptions {
            sliver_share: SLIVER_SHARE,
            unfillable_share: UNFILLABLE_SHARE,
            min_width: 0.0,
            exclude_unfillable: false,
            waste_percent: None,
            trim: super::units::DEFAULT_TRIM,
            max_units: MAX_UNITS,
            max_gap_segments: MAX_GAP_SEGMENTS,
            gap_length: false,
            max_extra_joint_mm: 2.0,
        }
    }
}

/// The numbers of one unit shape.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnitCount {
    pub name: String,
    pub unit_area_m2: f64,
    pub whole_units: usize,
    pub cut_units: usize,
    pub slivers: usize,
    pub unfillable_pieces: usize,
    pub area_laid_m2: f64,
    pub order_units: usize,
    /// The lower bound of the order of this unit type: the whole units plus the rounded-up sum of
    /// the shares of its cut units.
    pub order_units_min: usize,
    /// `order_units` plus the waste of this unit type, in whole numbers and rounded up. `None`
    /// without `waste_percent`. A contractor who orders two stones gets the waste per product.
    pub order_units_with_waste: Option<usize>,
    /// The sum of the shares of the cut units (not in the JSON; it is what rows are added up with).
    #[serde(skip)]
    pub cut_share: f64,
}

impl UnitCount {
    /// Sets `order_units_min` from the whole units and the sum of the shares of the cut units.
    pub fn set_min(&mut self) {
        self.order_units_min = self.whole_units + (self.cut_share - 1e-9).max(0.0).ceil() as usize;
    }
}

/// The numbers of a layout.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    pub bond: &'static str,
    /// The unit sizes of the four bonds. `None` for a module.
    pub unit_length_mm: Option<f64>,
    pub unit_width_mm: Option<f64>,
    pub gap_mm: f64,
    /// The widest joint. It is the gap, except in a basket weave whose blocks do not fit exactly.
    pub widest_joint_mm: f64,
    /// The narrowest joint, in mm. Only for a course on a curve, where the joints taper.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrowest_joint_mm: Option<f64>,
    /// Set when the widest joint is wider than the gap by more than `max_extra_joint_mm`: the text
    /// says by how much. The pattern is laid anyway (the hard limit is in the pattern); for
    /// trafficked paving, change the unit size, the gap or the bond.
    pub joint_warning: Option<String>,
    /// Where the pattern starts, in drawing coordinates (mm).
    pub origin: [f64; 2],
    pub angle_deg: f64,
    pub whole_units: usize,
    pub cut_units: usize,
    /// Cut units under `sliver_share` of a full unit.
    pub slivers: usize,
    pub sliver_share: f64,
    /// Cut units under `unfillable_share` of a full unit, or narrower than `min_width`: pieces that
    /// are too small to lay. They are part of the cut units.
    pub unfillable_pieces: usize,
    /// Clipped pieces under 1 mm2: rounding dust, not counted as units.
    pub dust_pieces: usize,
    /// Units that crossed the edge by less than the trim and were counted as whole.
    pub trimmed_units: usize,
    /// The area of a full unit. `None` if the pattern has several unit shapes.
    pub unit_area_m2: Option<f64>,
    pub region_area_m2: f64,
    /// The area of all units, whole and cut.
    pub area_laid_m2: f64,
    /// The area of the cut units.
    pub cut_area_m2: f64,
    /// The region area that the units do not cover: the gaps.
    pub gap_area_m2: f64,
    /// The length of the gap center lines, with the lines that lie on the outline. `None` unless
    /// asked for (`gap_length`), and when there were too many lines to make.
    pub gap_length_m: Option<f64>,
    /// Units to order, an upper bound: whole units plus cut units, every cut unit counted as one
    /// unit and no offcut used twice. Without `exclude_unfillable` the unfillable pieces are in it.
    pub order_units: usize,
    /// The same as `order_units`, with the unfillable pieces always in.
    pub order_units_max: usize,
    /// A lower bound: whole units plus the sum of the shares of the cut units, rounded up. It needs
    /// perfect use of offcuts.
    pub order_units_min: usize,
    pub waste_percent: Option<f64>,
    /// `order_units` plus the waste, rounded up. `None` without `waste_percent`.
    pub order_units_with_waste: Option<usize>,
    /// The numbers per unit shape.
    pub by_unit: Vec<UnitCount>,
}

/// `n` plus `percent` percent, rounded up, in whole numbers (a hundredth of a percent is the
/// finest step).
pub fn with_waste(n: usize, percent: f64) -> usize {
    let basis = (percent * 100.0).round().max(0.0) as u128;
    ((n as u128 * (10_000 + basis)).div_ceil(10_000)) as usize
}

/// What a summary says about the pattern, besides the counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Desc {
    pub bond: &'static str,
    pub unit_length_mm: Option<f64>,
    pub unit_width_mm: Option<f64>,
    pub gap_mm: f64,
    pub widest_joint_mm: f64,
    pub narrowest_joint_mm: Option<f64>,
}

impl Desc {
    pub fn of(pattern: &Pattern) -> Desc {
        Desc {
            bond: pattern.bond.id(),
            unit_length_mm: (pattern.module.is_none()).then_some(pattern.unit_length),
            unit_width_mm: (pattern.module.is_none()).then_some(pattern.unit_width),
            gap_mm: pattern.gap,
            widest_joint_mm: pattern.widest_joint(),
            narrowest_joint_mm: None,
        }
    }
}

/// Counts the units of a layout and measures the gaps.
pub fn summarize(
    region: &Region,
    pattern: &Pattern,
    place: &Placement,
    opts: &SummaryOptions,
) -> Result<Summary, LayoutError> {
    let limits = Limits {
        max_units: opts.max_units,
        trim: opts.trim,
    };
    let gap_length_m = if opts.gap_length {
        let g = gaps::gap_lines(region, pattern, place, opts.max_gap_segments, false);
        (!g.truncated).then(|| gaps::length(&g.segments) / 1000.0)
    } else {
        None
    };
    summarize_stream(
        Desc::of(pattern),
        region.area,
        opts,
        gap_length_m,
        &mut |f| for_each_unit(region, pattern, place, &limits, f),
    )
}

/// Counts the units that `run` makes (it calls its argument for each unit and returns the stats of
/// the layout). The pattern is described by `desc`.
pub fn summarize_stream(
    desc: Desc,
    region_area: f64,
    opts: &SummaryOptions,
    gap_length_m: Option<f64>,
    run: &mut dyn FnMut(&mut dyn FnMut(&super::units::Unit)) -> Result<Stats, LayoutError>,
) -> Result<Summary, LayoutError> {
    let mut tally = Tally::default();
    let unfillable = Unfillable {
        share: opts.unfillable_share,
        width: opts.min_width,
    };
    let stats = run(&mut |u| tally.add(u, opts.sliver_share, &unfillable))?;
    tally.kinds.resize(stats.kinds.len(), Default::default());
    // The unit shapes with the same name are counted together.
    let mut by_unit: Vec<UnitCount> = Vec::new();
    for (i, (name, area)) in stats.kinds.iter().enumerate() {
        let k = tally.kinds[i];
        let order = k.whole + k.cut
            - if opts.exclude_unfillable {
                k.unfillable
            } else {
                0
            };
        match by_unit.iter_mut().find(|u| &u.name == name) {
            Some(u) => {
                u.whole_units += k.whole;
                u.cut_units += k.cut;
                u.slivers += k.slivers;
                u.unfillable_pieces += k.unfillable;
                u.area_laid_m2 += k.area / 1e6;
                u.order_units += order;
                u.cut_share += k.cut_share;
            }
            None => by_unit.push(UnitCount {
                name: name.clone(),
                unit_area_m2: area / 1e6,
                whole_units: k.whole,
                cut_units: k.cut,
                slivers: k.slivers,
                unfillable_pieces: k.unfillable,
                area_laid_m2: k.area / 1e6,
                order_units: order,
                order_units_min: 0,
                order_units_with_waste: None,
                cut_share: k.cut_share,
            }),
        }
    }
    for u in &mut by_unit {
        u.set_min();
        u.order_units_with_waste = opts.waste_percent.map(|w| with_waste(u.order_units, w));
    }
    let (whole, cut) = (tally.whole(), tally.cut());
    let unfill = tally.unfillable();
    let order = whole + cut - if opts.exclude_unfillable { unfill } else { 0 };
    let cut_share: f64 = tally.kinds.iter().map(|k| k.cut_share).sum();
    let order_min = whole + (cut_share - 1e-9).max(0.0).ceil() as usize;
    let area = tally.area();
    Ok(Summary {
        bond: desc.bond,
        unit_length_mm: desc.unit_length_mm,
        unit_width_mm: desc.unit_width_mm,
        gap_mm: desc.gap_mm,
        widest_joint_mm: desc.widest_joint_mm,
        narrowest_joint_mm: desc.narrowest_joint_mm,
        joint_warning: {
            let extra = desc.widest_joint_mm - desc.gap_mm;
            (extra > opts.max_extra_joint_mm).then(|| {
                if desc.bond == "course" {
                    format!(
                        "the joints of the course get up to {:.1} mm wide, {:.1} mm more than the gap of {} mm: the units are straight and the path bends. Use shorter units on the bend if the paving is trafficked",
                        desc.widest_joint_mm, extra, desc.gap_mm
                    )
                } else {
                    format!(
                        "the joints get up to {:.1} mm wide, {:.1} mm more than the gap of {} mm: the blocks of this basket weave do not fit the unit size exactly (the units keep their size). Change the unit size or the gap if the paving is trafficked",
                        desc.widest_joint_mm, extra, desc.gap_mm
                    )
                }
            })
        },
        origin: [stats.origin.x, stats.origin.y],
        angle_deg: stats.angle,
        whole_units: whole,
        cut_units: cut,
        slivers: tally.slivers(),
        sliver_share: opts.sliver_share,
        unfillable_pieces: unfill,
        dust_pieces: stats.dust_pieces,
        trimmed_units: stats.trimmed,
        unit_area_m2: (stats.kinds.len() == 1 || by_unit.len() == 1)
            .then_some(stats.full_unit_area / 1e6),
        region_area_m2: region_area / 1e6,
        area_laid_m2: area / 1e6,
        cut_area_m2: tally.kinds.iter().map(|k| k.cut_area).sum::<f64>() / 1e6,
        gap_area_m2: ((region_area - area) / 1e6).max(0.0),
        gap_length_m,
        order_units: order,
        order_units_max: whole + cut,
        order_units_min: order_min,
        waste_percent: opts.waste_percent,
        order_units_with_waste: opts.waste_percent.map(|w| with_waste(order, w)),
        by_unit,
    })
}
