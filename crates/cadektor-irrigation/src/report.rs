//! Reports: zone schedule, pipe schedule, pressure analysis per zone and a materials list.
//!
//! The reports read an [`Analysis`](crate::extract::Analysis). They are plain data (serde), so the
//! API can send them as JSON and write them as CSV.
//!
//! # Zone area and precipitation rate
//!
//! The area of a zone is the area covered by the fans of its outlets, with overlaps counted once
//! (a grid sample, see [`coverage_area_m2`]). A caller can give the real area of a zone (for
//! example the planting bed) instead. The precipitation rate is `60 * flow / area` in mm/h and
//! the run time for a target depth `d` mm is `d / rate * 60` minutes.

use std::collections::{BTreeMap, HashMap};

use cadektor_core::DVec2;
use cadektor_core::csv::write_rows as write_csv;
use cadektor_core::irrigation::LineType;
use serde::Serialize;

use crate::extract::Analysis;
use crate::hydraulics::{precipitation_rate, run_time_minutes};
use crate::network::Warning;

/// A fan: position, radius in mm, arc in degrees, direction of the middle in degrees.
pub type Fan = (DVec2, f64, f64, f64);

/// Largest number of grid cells in the area sample.
const MAX_CELLS: f64 = 1.0e6;

/// Area in m2 covered by a set of fans, overlaps counted once. The area is sampled on a grid
/// of about 1/20 of the smallest radius (coarser for very large layouts, at most 1 million cells).
/// When the grid would be too coarse to see the fans, the exact areas of the fans are added up.
pub fn coverage_area_m2(fans: &[Fan]) -> f64 {
    let fans: Vec<&Fan> = fans
        .iter()
        .filter(|f| f.1 > 0.0 && f.2 > 0.0 && f.0.is_finite() && f.1.is_finite())
        .collect();
    if fans.is_empty() {
        return 0.0;
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    let mut rmin = f64::INFINITY;
    for f in &fans {
        lo = lo.min(f.0 - DVec2::splat(f.1));
        hi = hi.max(f.0 + DVec2::splat(f.1));
        rmin = rmin.min(f.1);
    }
    let size = hi - lo;
    let mut h = rmin / 20.0;
    let min_h = (size.x * size.y / MAX_CELLS).sqrt();
    if h < min_h {
        h = min_h;
    }
    // A grid this coarse cannot see the fans: add up the exact areas (overlaps count twice).
    if h > rmin / 4.0 {
        return fans
            .iter()
            .map(|f| crate::hydraulics::sector_area_m2(f.1, f.2))
            .sum();
    }
    let nx = (size.x / h).ceil() as usize + 1;
    let ny = (size.y / h).ceil() as usize + 1;
    let mut seen = vec![false; nx * ny];
    for f in &fans {
        let (at, r, arc, dir) = **f;
        let full = arc >= 360.0;
        let half = arc.to_radians() / 2.0;
        let mid = dir.to_radians();
        let x0 = (((at.x - r - lo.x) / h).floor().max(0.0)) as usize;
        let x1 = ((((at.x + r - lo.x) / h).ceil()) as usize).min(nx - 1);
        let y0 = (((at.y - r - lo.y) / h).floor().max(0.0)) as usize;
        let y1 = ((((at.y + r - lo.y) / h).ceil()) as usize).min(ny - 1);
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let c = DVec2::new(lo.x + (ix as f64 + 0.5) * h, lo.y + (iy as f64 + 0.5) * h);
                let d = c - at;
                if d.length_squared() > r * r {
                    continue;
                }
                if !full && d.length_squared() > 0.0 {
                    let mut diff = (d.to_angle() - mid).rem_euclid(std::f64::consts::TAU);
                    if diff > std::f64::consts::PI {
                        diff -= std::f64::consts::TAU;
                    }
                    if diff.abs() > half {
                        continue;
                    }
                }
                seen[iy * nx + ix] = true;
            }
        }
    }
    seen.iter().filter(|s| **s).count() as f64 * h * h / 1e6
}

/// Heads closer than this (mm) are one spot, not neighbors.
const SAME_SPOT_MM: f64 = 300.0;

/// Area in m2 that the heads and drip lines of a zone water, read from their spacing:
///
/// * A head waters the square of its distance to the nearest head (head-to-head spacing `S`),
///   times its share of a circle (`arc / 360`). A full circle waters `S * S`, a half circle at an
///   edge `S * S / 2`. With the flow in proportion to the arc, every head then gives
///   `60 * flow / (S * S)` mm/h, as for a square layout.
/// * A drip line waters its length times the distance to the nearest other drip line (the
///   emitter spacing when there is no other line).
///
/// `None` when there is nothing to read (a single head, or no neighbors apart).
pub fn layout_area_m2(
    heads: &[&crate::extract::OutletInfo],
    drips: &[&crate::extract::LineInfo],
) -> Option<f64> {
    if heads.len() > 20_000 {
        return None;
    }
    let mut total = 0.0;
    for (i, h) in heads.iter().enumerate() {
        let nn = heads
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, o)| o.at.distance(h.at))
            .filter(|d| *d >= SAME_SPOT_MM)
            .fold(f64::INFINITY, f64::min);
        if !nn.is_finite() {
            return None;
        }
        let share = if h.kind.is_some_and(|k| k.flow_scales_with_arc()) {
            h.arc.clamp(0.0, 360.0) / 360.0
        } else {
            1.0
        };
        total += nn * nn * share / 1e6;
    }
    for l in drips {
        if l.points.len() < 2 {
            continue;
        }
        let mid = path_midpoint(&l.points);
        let row = drips
            .iter()
            .filter(|o| o.id != l.id)
            .map(|o| polyline_distance(mid, &o.points))
            .filter(|d| *d > 1.0)
            .fold(f64::INFINITY, f64::min);
        let row = if row.is_finite() {
            row
        } else {
            l.emitter_spacing.unwrap_or(300.0)
        };
        total += l.length_m * row / 1000.0;
    }
    (total > 0.0).then_some(total)
}

fn path_midpoint(p: &[DVec2]) -> DVec2 {
    let half: f64 = p.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>() / 2.0;
    let mut walked = 0.0;
    for w in p.windows(2) {
        let l = w[0].distance(w[1]);
        if walked + l >= half && l > 0.0 {
            return w[0] + (w[1] - w[0]) * ((half - walked) / l);
        }
        walked += l;
    }
    p[0]
}

fn polyline_distance(q: DVec2, p: &[DVec2]) -> f64 {
    p.windows(2)
        .map(|w| cadektor_core::geom::dist_to_segment(q, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HeadCount {
    pub model: String,
    pub count: usize,
    pub flow_lpm: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ZoneRow {
    pub zone: u32,
    /// Names of the valves of the zone.
    pub valves: Vec<String>,
    pub controller: Option<String>,
    pub station: Option<u32>,
    pub heads: Vec<HeadCount>,
    pub head_count: usize,
    /// Flow of the zone in l/min, including drip lines.
    pub flow_lpm: f64,
    pub area_m2: f64,
    /// `given`, `layout` (from the spacing of the heads and drip lines) or `coverage` (the union
    /// of the fans, when there is no spacing to read).
    pub area_source: &'static str,
    pub precipitation_mm_h: f64,
    /// Minutes of running time for the target depth. `None` without a precipitation rate.
    pub run_time_min: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ZoneSchedule {
    pub target_depth_mm: f64,
    pub rows: Vec<ZoneRow>,
    pub total_flow_lpm: f64,
    pub total_heads: usize,
    /// Outlets with no zone (no water path or no valve).
    pub outlets_without_zone: usize,
}

/// The zone schedule. `areas` gives the real area in m2 of zones (by zone number).
pub fn zone_schedule(
    a: &Analysis,
    areas: &HashMap<u32, f64>,
    target_depth_mm: f64,
) -> ZoneSchedule {
    let zones = a.outlet_zones();
    let mut rows = vec![];
    for z in &a.solution.zones {
        if z.zone == 0 {
            continue;
        }
        let mine: Vec<&crate::extract::OutletInfo> = a
            .extraction
            .outlets
            .iter()
            .filter(|o| zones.get(&o.id) == Some(&z.zone))
            .collect();
        let mut by_model: BTreeMap<String, HeadCount> = BTreeMap::new();
        for o in &mine {
            let e = by_model
                .entry(o.model.to_ascii_lowercase())
                .or_insert(HeadCount {
                    model: o.model.clone(),
                    count: 0,
                    flow_lpm: 0.0,
                });
            e.count += 1;
            e.flow_lpm += o.flow_lpm;
        }
        let fans: Vec<Fan> = mine
            .iter()
            .filter_map(|o| o.radius.map(|r| (o.at, r, o.arc, o.direction)))
            .collect();
        let drips: Vec<&crate::extract::LineInfo> = a
            .extraction
            .lines
            .iter()
            .filter(|l| l.line_type == LineType::Drip && zones.get(&l.id) == Some(&z.zone))
            .collect();
        let (area, source) = match areas.get(&z.zone) {
            Some(x) if *x > 0.0 => (*x, "given"),
            _ => match layout_area_m2(&mine, &drips) {
                Some(x) => (x, "layout"),
                None => (coverage_area_m2(&fans), "coverage"),
            },
        };
        let rate = precipitation_rate(z.flow_lpm, area);
        let valves: Vec<&crate::extract::ValveInfo> = a
            .extraction
            .valves
            .iter()
            .filter(|v| v.zone == z.zone)
            .collect();
        rows.push(ZoneRow {
            zone: z.zone,
            valves: valves
                .iter()
                .map(|v| {
                    if v.name.is_empty() {
                        format!("#{}", v.id)
                    } else {
                        v.name.clone()
                    }
                })
                .collect(),
            controller: valves.iter().find_map(|v| v.controller.clone()),
            station: valves.iter().find_map(|v| v.station),
            heads: by_model.into_values().collect(),
            head_count: mine.len(),
            flow_lpm: z.flow_lpm,
            area_m2: area,
            area_source: source,
            precipitation_mm_h: rate,
            run_time_min: run_time_minutes(target_depth_mm, rate),
        });
    }
    let without = a
        .extraction
        .outlets
        .iter()
        .filter(|o| zones.get(&o.id).is_none_or(|z| *z == 0))
        .count();
    ZoneSchedule {
        target_depth_mm,
        total_flow_lpm: rows.iter().map(|r| r.flow_lpm).sum(),
        total_heads: rows.iter().map(|r| r.head_count).sum(),
        rows,
        outlets_without_zone: without,
    }
}

impl ZoneSchedule {
    pub fn to_csv(&self) -> String {
        let mut rows = vec![
            [
                "Zone",
                "Controller",
                "Station",
                "Heads",
                "Models",
                "Flow l/min",
                "Area m2",
                "Precipitation mm/h",
                "Run time min",
            ]
            .map(String::from)
            .to_vec(),
        ];
        for r in &self.rows {
            rows.push(vec![
                r.zone.to_string(),
                r.controller.clone().unwrap_or_default(),
                r.station.map(|s| s.to_string()).unwrap_or_default(),
                r.head_count.to_string(),
                r.heads
                    .iter()
                    .map(|h| format!("{} x {}", h.count, h.model))
                    .collect::<Vec<_>>()
                    .join("; "),
                format!("{:.2}", r.flow_lpm),
                format!("{:.1}", r.area_m2),
                format!("{:.1}", r.precipitation_mm_h),
                r.run_time_min
                    .map(|t| format!("{t:.1}"))
                    .unwrap_or_default(),
            ]);
        }
        write_csv(&rows)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PipeRow {
    pub line_type: &'static str,
    pub material: String,
    pub size: String,
    pub inner_diameter_mm: Option<f64>,
    pub count: usize,
    pub length_m: f64,
    pub price_per_m: f64,
    pub cost: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PipeSchedule {
    pub rows: Vec<PipeRow>,
    pub total_length_m: f64,
    pub total_cost: f64,
}

/// Pipe lengths per type, material and size.
pub fn pipe_schedule(a: &Analysis) -> PipeSchedule {
    let mut map: BTreeMap<(u8, String, String), PipeRow> = BTreeMap::new();
    for l in &a.extraction.lines {
        let rank = match l.line_type {
            LineType::Mainline => 0,
            LineType::Lateral => 1,
            LineType::Drip => 2,
        };
        let key = (
            rank,
            l.material.to_ascii_lowercase(),
            l.size.to_ascii_lowercase(),
        );
        let e = map.entry(key).or_insert(PipeRow {
            line_type: l.line_type.id(),
            material: l.material.clone(),
            size: l.size.clone(),
            inner_diameter_mm: l.inner_diameter_mm,
            count: 0,
            length_m: 0.0,
            price_per_m: l.price_per_m,
            cost: 0.0,
        });
        e.count += 1;
        e.length_m += l.length_m;
        e.cost = e.length_m * e.price_per_m;
    }
    let rows: Vec<PipeRow> = map.into_values().collect();
    PipeSchedule {
        total_length_m: rows.iter().map(|r| r.length_m).sum(),
        total_cost: rows.iter().map(|r| r.cost).sum(),
        rows,
    }
}

impl PipeSchedule {
    pub fn to_csv(&self) -> String {
        let mut rows = vec![
            [
                "Type",
                "Material",
                "Size",
                "Lines",
                "Length m",
                "Price per m",
                "Cost",
            ]
            .map(String::from)
            .to_vec(),
        ];
        for r in &self.rows {
            rows.push(vec![
                r.line_type.into(),
                r.material.clone(),
                r.size.clone(),
                r.count.to_string(),
                format!("{:.2}", r.length_m),
                format!("{:.2}", r.price_per_m),
                format!("{:.2}", r.cost),
            ]);
        }
        write_csv(&rows)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutletPressure {
    pub id: Option<u64>,
    pub label: String,
    pub flow_lpm: f64,
    pub pressure_kpa: f64,
    pub min_kpa: f64,
    pub margin_kpa: f64,
    pub ok: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PathPoint {
    pub label: String,
    pub id: Option<u64>,
    pub pressure_kpa: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ZonePressure {
    pub zone: u32,
    pub flow_lpm: f64,
    pub available_lpm: f64,
    pub static_kpa: f64,
    pub max_velocity_ms: f64,
    pub critical: Option<OutletPressure>,
    pub critical_path: Vec<PathPoint>,
    pub outlets: Vec<OutletPressure>,
    pub warnings: Vec<WarningRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WarningRow {
    pub kind: &'static str,
    pub object: Option<u64>,
    pub zone: Option<u32>,
    pub message: String,
}

impl From<&Warning> for WarningRow {
    fn from(w: &Warning) -> Self {
        WarningRow {
            kind: w.kind.id(),
            object: w.object,
            zone: w.zone,
            message: w.message.clone(),
        }
    }
}

/// Pressure analysis: per zone the outlets, the critical outlet and path, and the warnings.
pub fn pressure_analysis(a: &Analysis) -> Vec<ZonePressure> {
    a.solution
        .zones
        .iter()
        .map(|z| {
            let row = |o: &crate::network::OutletResult| OutletPressure {
                id: o.object,
                label: o.label.clone(),
                flow_lpm: o.flow_lpm,
                pressure_kpa: o.pressure_kpa,
                min_kpa: o.min_kpa,
                margin_kpa: o.margin_kpa,
                ok: o.margin_kpa >= -1e-9,
            };
            ZonePressure {
                zone: z.zone,
                flow_lpm: z.flow_lpm,
                available_lpm: z.available_lpm,
                static_kpa: z.static_kpa,
                max_velocity_ms: z.max_velocity_ms,
                critical: z.critical.map(|k| row(&z.outlets[k])),
                critical_path: z
                    .critical_path
                    .iter()
                    .map(|s| PathPoint {
                        label: s.label.clone(),
                        id: s.object,
                        pressure_kpa: s.pressure_kpa,
                    })
                    .collect(),
                outlets: z.outlets.iter().map(row).collect(),
                warnings: z.warnings.iter().map(WarningRow::from).collect(),
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MaterialRow {
    pub category: &'static str,
    pub item: String,
    pub quantity: f64,
    pub unit: &'static str,
    pub unit_price: f64,
    pub total: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MaterialsList {
    pub rows: Vec<MaterialRow>,
    pub total_cost: f64,
}

/// All parts: heads by model, pipes by material and size, valves, POCs and controllers.
/// Valves, POCs and controllers have no catalog price (zero).
pub fn materials_list(a: &Analysis) -> MaterialsList {
    let mut rows: Vec<MaterialRow> = vec![];
    let mut heads: BTreeMap<String, (String, usize, f64)> = BTreeMap::new();
    for o in &a.extraction.outlets {
        let e = heads
            .entry(o.model.to_ascii_lowercase())
            .or_insert((o.model.clone(), 0, o.price));
        e.1 += 1;
    }
    for (name, n, price) in heads.into_values() {
        rows.push(MaterialRow {
            category: "head",
            item: name,
            quantity: n as f64,
            unit: "pcs",
            unit_price: price,
            total: n as f64 * price,
        });
    }
    for p in pipe_schedule(a).rows {
        rows.push(MaterialRow {
            category: "pipe",
            item: format!("{} {} ({})", p.material, p.size, p.line_type),
            quantity: p.length_m,
            unit: "m",
            unit_price: p.price_per_m,
            total: p.cost,
        });
    }
    let mut kvs: BTreeMap<String, usize> = BTreeMap::new();
    for v in &a.extraction.valves {
        *kvs.entry(format!("Control valve kv {}", v.kv)).or_insert(0) += 1;
    }
    for (item, n) in kvs {
        rows.push(MaterialRow {
            category: "valve",
            item,
            quantity: n as f64,
            unit: "pcs",
            unit_price: 0.0,
            total: 0.0,
        });
    }
    if !a.extraction.pocs.is_empty() {
        rows.push(MaterialRow {
            category: "poc",
            item: "Point of connection".into(),
            quantity: a.extraction.pocs.len() as f64,
            unit: "pcs",
            unit_price: 0.0,
            total: 0.0,
        });
    }
    if !a.extraction.controllers.is_empty() {
        let stations: u32 = a.extraction.controllers.iter().map(|c| c.stations).sum();
        rows.push(MaterialRow {
            category: "controller",
            item: format!("Controller ({stations} stations)"),
            quantity: a.extraction.controllers.len() as f64,
            unit: "pcs",
            unit_price: 0.0,
            total: 0.0,
        });
    }
    MaterialsList {
        total_cost: rows.iter().map(|r| r.total).sum(),
        rows,
    }
}

impl MaterialsList {
    pub fn to_csv(&self) -> String {
        let mut rows = vec![
            [
                "Category",
                "Item",
                "Quantity",
                "Unit",
                "Unit price",
                "Total",
            ]
            .map(String::from)
            .to_vec(),
        ];
        for r in &self.rows {
            rows.push(vec![
                r.category.into(),
                r.item.clone(),
                format!("{}", (r.quantity * 100.0).round() / 100.0),
                r.unit.into(),
                format!("{:.2}", r.unit_price),
                format!("{:.2}", r.total),
            ]);
        }
        write_csv(&rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::fixtures::*;
    use crate::extract::{DEFAULT_TOLERANCE, analyze};
    use cadektor_core::irrigation::IrrigationShape;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1e-12)
    }

    #[test]
    fn coverage_area_of_simple_fans() {
        let r = 3000.0;
        let circle = coverage_area_m2(&[(DVec2::ZERO, r, 360.0, 0.0)]);
        assert!(close(circle, std::f64::consts::PI * 9.0, 0.02), "{circle}");
        let half = coverage_area_m2(&[(DVec2::ZERO, r, 180.0, 90.0)]);
        assert!(close(half, std::f64::consts::PI * 4.5, 0.03), "{half}");
        // 175 degrees, not 180: a fan edge on the grid diagonal would count its cells twice.
        let quarter_facing_left = coverage_area_m2(&[(DVec2::ZERO, r, 90.0, 175.0)]);
        assert!(
            close(quarter_facing_left, std::f64::consts::PI * 2.25, 0.04),
            "{quarter_facing_left}"
        );
        // Two identical fans count once. Two far fans add up.
        let twin = coverage_area_m2(&[(DVec2::ZERO, r, 360.0, 0.0), (DVec2::ZERO, r, 360.0, 0.0)]);
        assert!(close(twin, circle, 1e-9));
        let apart = coverage_area_m2(&[
            (DVec2::ZERO, r, 360.0, 0.0),
            (DVec2::new(20_000.0, 0.0), r, 360.0, 0.0),
        ]);
        assert!(close(apart, 2.0 * circle, 0.02));
        // Overlap: two circles whose centers are one radius apart cover less than twice.
        let lens = coverage_area_m2(&[
            (DVec2::ZERO, r, 360.0, 0.0),
            (DVec2::new(r, 0.0), r, 360.0, 0.0),
        ]);
        assert!(lens < 2.0 * circle && lens > circle);
        assert_eq!(coverage_area_m2(&[]), 0.0);
        assert_eq!(coverage_area_m2(&[(DVec2::ZERO, 0.0, 360.0, 0.0)]), 0.0);
        // A huge layout falls back to a coarser grid.
        let big = coverage_area_m2(&[
            (DVec2::ZERO, 100.0, 360.0, 0.0),
            (DVec2::new(1e7, 1e7), 100.0, 360.0, 0.0),
        ]);
        assert!(close(big, 2.0 * std::f64::consts::PI * 0.01, 1e-9));
    }

    fn analysis() -> Analysis {
        analyze(small_zone().doc(), DEFAULT_TOLERANCE)
    }

    /// Four Spray-4.0 heads: 32 l/min. With a given area of 160 m2: 60 * 32 / 160 = 12 mm/h.
    /// A target of 6 mm needs 6 / 12 * 60 = 30 minutes.
    #[test]
    fn zone_schedule_hand_example() {
        let a = analysis();
        let mut areas = HashMap::new();
        areas.insert(1, 160.0);
        let s = zone_schedule(&a, &areas, 6.0);
        assert_eq!(s.rows.len(), 1);
        let r = &s.rows[0];
        assert_eq!((r.zone, r.head_count), (1, 4));
        assert_eq!(r.heads[0].model, "Spray-4.0");
        assert_eq!(r.heads[0].count, 4);
        assert!(close(r.flow_lpm, 32.0, 1e-12));
        assert_eq!(r.area_source, "given");
        assert!(close(r.precipitation_mm_h, 12.0, 1e-12));
        assert!(close(r.run_time_min.unwrap(), 30.0, 1e-12));
        assert_eq!(s.total_heads, 4);
        assert_eq!(s.outlets_without_zone, 0);
        assert!(
            s.to_csv()
                .contains("1,,,4,4 x Spray-4.0,32.00,160.0,12.0,30.0")
        );
    }

    #[test]
    fn zone_schedule_reads_the_layout_without_given_area() {
        let a = analysis();
        let s = zone_schedule(&a, &HashMap::new(), 10.0);
        let r = &s.rows[0];
        assert_eq!(r.area_source, "layout");
        // Four heads, each 10 m from its nearest neighbor (two spots): about 4 * 100 m2.
        assert!(close(r.area_m2, 400.0, 0.01), "{}", r.area_m2);
        assert!(close(r.precipitation_mm_h, 60.0 * 32.0 / r.area_m2, 1e-9));
        // A bad given area is ignored.
        let mut bad = HashMap::new();
        bad.insert(1, -4.0);
        assert_eq!(zone_schedule(&a, &bad, 10.0).rows[0].area_source, "layout");
    }

    fn head(x: f64, y: f64, arc: f64) -> crate::extract::OutletInfo {
        crate::extract::OutletInfo {
            id: 1,
            at: DVec2::new(x, y),
            model: "Spray-4.0".into(),
            kind: Some(crate::catalog::HeadKind::Spray),
            arc,
            direction: 0.0,
            radius: Some(4000.0),
            flow_lpm: 8.0 * arc / 360.0,
            min_kpa: 140.0,
            price: 0.0,
            node: None,
        }
    }

    /// A 3 x 3 grid of full-circle heads at 4 m: every head waters 16 m2 and uses 8 l/min,
    /// so the zone gets 60 * 72 / 144 = 30 mm/h (the union of the fans would give 19 mm/h).
    /// Half-circle heads on an edge row use half the flow over half the area: still 30.
    #[test]
    fn precipitation_from_the_head_spacing() {
        let mut heads = vec![];
        for i in 0..3 {
            for j in 0..3 {
                heads.push(head(i as f64 * 4000.0, j as f64 * 4000.0, 360.0));
            }
        }
        let refs: Vec<&crate::extract::OutletInfo> = heads.iter().collect();
        let area = layout_area_m2(&refs, &[]).unwrap();
        assert!(close(area, 144.0, 1e-12));
        assert!(close(60.0 * 72.0 / area, 30.0, 1e-12));
        // A row of half circles along an edge.
        let row: Vec<crate::extract::OutletInfo> = (0..4)
            .map(|i| head(i as f64 * 4000.0, 0.0, 180.0))
            .collect();
        let refs: Vec<&crate::extract::OutletInfo> = row.iter().collect();
        let area = layout_area_m2(&refs, &[]).unwrap();
        assert!(close(60.0 * 4.0 * 4.0 / area, 30.0, 1e-12));
        // A single head, or heads on one spot, give no layout.
        assert_eq!(layout_area_m2(&refs[..1], &[]), None);
        let same = [head(0.0, 0.0, 360.0), head(10.0, 0.0, 360.0)];
        let refs: Vec<&crate::extract::OutletInfo> = same.iter().collect();
        assert_eq!(layout_area_m2(&refs, &[]), None);
        assert_eq!(layout_area_m2(&[], &[]), None);
    }

    fn drip(id: u64, y: f64) -> crate::extract::LineInfo {
        crate::extract::LineInfo {
            id,
            line_type: LineType::Drip,
            size: "16".into(),
            material: "PE".into(),
            length_m: 10.0,
            inner_diameter_mm: Some(12.4),
            price_per_m: 0.0,
            emitter_flow_lpm: 0.0,
            emitter_spacing: Some(400.0),
            points: vec![DVec2::new(0.0, y), DVec2::new(10_000.0, y)],
        }
    }

    /// Drip lines 0.5 m apart: each waters 10 m x 0.5 m = 5 m2. Emitters every 0.4 m with
    /// 2 l/h make 25 emitters per line, 50 l/h = 0.8333 l/min: 60 * 0.8333 / 5 = 10 mm/h, which is
    /// the emitter flow / (emitter spacing * line spacing) = 2 / (0.4 * 0.5).
    #[test]
    fn precipitation_of_drip_lines_from_emitter_and_line_spacing() {
        let lines = [drip(1, 0.0), drip(2, 500.0), drip(3, 1000.0)];
        let refs: Vec<&crate::extract::LineInfo> = lines.iter().collect();
        let area = layout_area_m2(&[], &refs).unwrap();
        assert!(close(area, 15.0, 1e-9));
        let flow = 3.0 * 25.0 * 2.0 / 60.0;
        assert!(close(60.0 * flow / area, 10.0, 1e-9));
        // One line alone uses its emitter spacing as the row width: 10 m x 0.4 m.
        assert!(close(layout_area_m2(&[], &refs[..1]).unwrap(), 4.0, 1e-9));
        // A line with one point adds nothing; a bent line has a middle on its second leg.
        let mut one = drip(4, 0.0);
        one.points.truncate(1);
        assert_eq!(layout_area_m2(&[], &[&one]), None);
        let p = path_midpoint(&[
            DVec2::ZERO,
            DVec2::new(1000.0, 0.0),
            DVec2::new(1000.0, 3000.0),
        ]);
        assert_eq!(p, DVec2::new(1000.0, 1000.0));
        assert_eq!(path_midpoint(&[DVec2::ZERO]), DVec2::ZERO);
    }

    #[test]
    fn a_zone_of_one_head_falls_back_to_the_coverage_of_the_fan() {
        let mut ed = cadektor_core::command::Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 300.0, 50.0));
        add(&mut ed, valve(v(0.0, 0.0), 1, 5.0));
        add(&mut ed, outlet(v(0.0, 0.0), "Spray-4.0", 360.0));
        let a = analyze(ed.doc(), 50.0);
        let r = &zone_schedule(&a, &HashMap::new(), 5.0).rows[0];
        assert_eq!(r.area_source, "coverage");
        assert!(close(r.area_m2, std::f64::consts::PI * 16.0, 0.02));
    }

    #[test]
    fn pipe_schedule_groups_lengths() {
        let a = analysis();
        let p = pipe_schedule(&a);
        // Mainline PVC 50: 20 m. Laterals PE 32: 30 m. Lateral PE 25: 10 m.
        assert_eq!(p.rows.len(), 3);
        assert_eq!(
            (p.rows[0].line_type, p.rows[0].size.as_str()),
            ("mainline", "50")
        );
        assert!(close(p.rows[0].length_m, 20.0, 1e-12));
        assert!(close(p.total_length_m, 60.0, 1e-12));
        // PVC 50 costs 2.6 per m, PE 32 1.4, PE 25 0.9: 52 + 42 + 9 = 103.
        assert!(close(p.total_cost, 103.0, 1e-9));
        let csv = p.to_csv();
        assert!(csv.starts_with("Type,Material,Size"));
        assert!(csv.contains("mainline,PVC,50,1,20.00,2.60,52.00"));
    }

    #[test]
    fn pressure_analysis_lists_critical_outlet() {
        let a = analysis();
        let p = pressure_analysis(&a);
        assert_eq!(p.len(), 1);
        let z = &p[0];
        assert_eq!(z.outlets.len(), 4);
        let c = z.critical.as_ref().unwrap();
        assert!(c.ok);
        assert!(
            z.outlets
                .iter()
                .all(|o| o.pressure_kpa >= c.pressure_kpa - 1e-12)
        );
        assert_eq!(z.critical_path.first().unwrap().pressure_kpa, 400.0);
        assert!(z.warnings.is_empty());
    }

    #[test]
    fn materials_list_totals() {
        let mut ed = small_zone();
        add(
            &mut ed,
            IrrigationShape::Controller {
                at: v(0.0, 5000.0),
                name: "C1".into(),
                stations: 6,
            },
        );
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        let m = materials_list(&a);
        let head = m.rows.iter().find(|r| r.category == "head").unwrap();
        assert_eq!((head.quantity, head.unit_price), (4.0, 4.2));
        assert!(close(head.total, 16.8, 1e-12));
        assert!(
            m.rows
                .iter()
                .any(|r| r.category == "valve" && r.item.contains("kv 6"))
        );
        assert!(m.rows.iter().any(|r| r.category == "poc"));
        assert!(
            m.rows
                .iter()
                .any(|r| r.category == "controller" && r.item.contains("6 stations"))
        );
        assert!(close(m.total_cost, 103.0 + 16.8, 1e-9));
        assert!(m.to_csv().contains("head,Spray-4.0,4,pcs,4.20,16.80"));
    }

    #[test]
    fn outlets_without_zone_are_counted() {
        let mut ed = small_zone();
        add(&mut ed, outlet(v(90_000.0, 90_000.0), "Spray-4.0", 360.0));
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        let s = zone_schedule(&a, &HashMap::new(), 5.0);
        assert_eq!(s.outlets_without_zone, 1);
        // An empty document gives empty reports.
        let empty = analyze(&cadektor_core::model::Document::default(), 50.0);
        assert!(zone_schedule(&empty, &HashMap::new(), 5.0).rows.is_empty());
        assert!(pipe_schedule(&empty).rows.is_empty());
        assert!(materials_list(&empty).rows.is_empty());
        assert!(pressure_analysis(&empty).is_empty());
    }

    #[test]
    fn zone_without_flow_has_no_run_time() {
        // Unknown head model: no flow, so the zone does not exist at all.
        let mut ed = cadektor_core::command::Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 300.0, 50.0));
        add(&mut ed, valve(v(0.0, 0.0), 1, 5.0));
        add(&mut ed, outlet(v(0.0, 0.0), "Ghost", 90.0));
        let a = analyze(ed.doc(), 50.0);
        assert!(zone_schedule(&a, &HashMap::new(), 5.0).rows.is_empty());
    }
}
