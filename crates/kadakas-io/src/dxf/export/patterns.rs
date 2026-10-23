//! The unit pattern of hardscapes in a DXF export: gap center lines, the units themselves, or the
//! units as blocks.
//!
//! Arcs of the outline are cut into straight pieces within the export tolerance, so a cut unit has
//! short straight edges along a curved boundary. The counts here are the counts of
//! `report.unit_pattern`, because both call the same layout engine with the same tolerance.
//!
//! The kerb border of a hardscape is not paved: the units lie inside the border, as in the report.
//!
//! A hardscape with zones or courses (PAT-07, PAT-08) is written group by group. The field is on
//! the layers of the hardscape; a zone or a course has `<layer>-<NAME>` as its base layer, so that
//! a contractor can switch each part on and off.
//!
//! The type of a unit (the name of its shape in a module pattern, the size of the stone in a bond
//! or a course) is written as extended data of every unit polyline: application `KADAKAS_UNIT`,
//! group 1000 with the name that `report.unit_pattern` has in `by_unit`. The notes of the export
//! tell the unit types that share a name apart (a herringbone has two: lengthwise and
//! crosswise).
//!
//! **Blocks** (PAT-11): with the option `blocks`, each whole unit is an INSERT of the block of its
//! unit type, and each cut unit is a polyline as before. The block is the shape of the unit around
//! its center; the INSERT has the center and the turn of the unit. Units of one type that have the
//! same shape (a herringbone has units turned by 90 degrees; courses turn with their path) use
//! one block. The block name is `KADAKAS_<TYPE>`; a second shape of the same type gets `_2`, and
//! so on. The INSERTs are on `<layer>-UNITS-WHOLE`, so the counts of INSERTs plus the polylines
//! on `<layer>-UNITS-CUT` are the counts of the report.

use std::collections::BTreeSet;

use kadakas_core::DVec2;
use kadakas_core::annot::draw::Prim;
use kadakas_core::layout::plan::{Group, Role, groups_of};
use kadakas_core::layout::{self, Limits, gaps};
use kadakas_core::model::{Object, Shape};

use super::native::AnonBlock;
use super::{Ctx, W, common, escape_unicode, lwpolyline};
use crate::options::PatternExport;
use crate::{IoError, Report};

/// Most units that one export writes for one object. Measured (test `measure_export`): a 5,000 m2
/// lot of 200 x 100 mm pavers has 233,000 units and gives 60 MB of DXF in 0.85 s with a peak of
/// 300 MB; 514,000 units give 132 MB in 1.8 s with a peak of 525 MB (about 257 bytes and 3.5 us
/// per unit). 600,000 units is 2.4 times the real job, 155 MB of text and a peak near 610 MB.
pub const MAX_EXPORT_UNITS: usize = 600_000;
/// Most units (or gap lines) that one export file holds in all objects together. The text of the
/// file is held in memory, so the file has a bound too, not only each object.
pub const MAX_EXPORT_FILE_UNITS: usize = 1_000_000;
/// Most gap center lines that one export writes for one object (about 154 bytes each: 1,000,000
/// lines are 154 MB).
pub const MAX_EXPORT_GAP_LINES: usize = 1_000_000;
/// The application name of the extended data that holds the unit type.
pub const XDATA_APP: &str = "KADAKAS_UNIT";
/// The prefix of the names of the blocks of whole units.
pub const BLOCK_PREFIX: &str = "KADAKAS_";

/// The names of the layers that a pattern export adds to a base layer.
pub fn layer_names(base: &str) -> [String; 4] {
    [
        format!("{base}-UNITS-WHOLE"),
        format!("{base}-UNITS-CUT"),
        format!("{base}-UNITS-VOID"),
        format!("{base}-GAPS"),
    ]
}

/// What the writing of one object used.
#[derive(Default)]
pub(super) struct Used {
    pub layers: BTreeSet<String>,
    /// Extended data was written, so the application must be in the table.
    pub xdata: bool,
}

/// The shape of a whole unit around its center, turned so that it does not depend on how the unit
/// lies. Two units that are the same shape (turned and moved) give the same key.
struct Canon {
    key: String,
    /// The corners around the center, in mm, starting at the chosen corner.
    local: Vec<DVec2>,
    /// The center, in model coordinates.
    center: DVec2,
    /// The turn of the unit against the block, in degrees.
    angle_deg: f64,
}

fn area_centroid(r: &[DVec2]) -> DVec2 {
    let o = r[0];
    let (mut a2, mut c) = (0.0, DVec2::ZERO);
    for i in 0..r.len() {
        let (p, q) = (r[i] - o, r[(i + 1) % r.len()] - o);
        let cr = p.perp_dot(q);
        a2 += cr;
        c += (p + q) * cr;
    }
    if a2.abs() < 1e-12 {
        return r.iter().copied().sum::<DVec2>() / r.len() as f64;
    }
    o + c / (3.0 * a2)
}

/// The canonical form of a whole unit. The corner to start from is the one that gives the smallest
/// key, so that the same shape always gives the same key. The key rounds the corners to 0.01 mm.
fn canon(ring: &[DVec2]) -> Option<Canon> {
    let mut r: Vec<DVec2> = ring.to_vec();
    if kadakas_core::geom::signed_area(&r) < 0.0 {
        r.reverse();
    }
    let n = r.len();
    if n < 3 {
        return None;
    }
    let c = area_centroid(&r);
    let mut best: Option<(Vec<i64>, Vec<DVec2>, f64)> = None;
    for s in 0..n {
        let e = r[(s + 1) % n] - r[s];
        let phi = e.y.atan2(e.x);
        let turn = DVec2::from_angle(-phi);
        let local: Vec<DVec2> = (0..n).map(|k| turn.rotate(r[(s + k) % n] - c)).collect();
        let key: Vec<i64> = local
            .iter()
            .flat_map(|p| [(p.x * 100.0).round() as i64, (p.y * 100.0).round() as i64])
            .collect();
        if best.as_ref().is_none_or(|b| key < b.0) {
            best = Some((key, local, phi));
        }
    }
    let (k, local, phi) = best?;
    Some(Canon {
        key: k.iter().map(i64::to_string).collect::<Vec<_>>().join(","),
        local,
        center: c,
        angle_deg: phi.to_degrees(),
    })
}

/// The name of the block of a unit type.
fn block_name_base(type_name: &str) -> String {
    let s: String = type_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{BLOCK_PREFIX}{}", if s.is_empty() { "UNIT" } else { &s })
}

/// The block of this shape of this unit type: made on first use.
fn block_for(cx: &mut Ctx, type_name: &str, c: &Canon) -> String {
    let id = format!("{type_name}|{}", c.key);
    if let Some(n) = cx.pattern_blocks.get(&id) {
        return n.clone();
    }
    let base = block_name_base(type_name);
    let taken = |n: &str| cx.pattern_blocks.values().any(|v| v == n);
    let mut name = base.clone();
    let mut i = 2;
    while taken(&name) {
        name = format!("{base}_{i}");
        i += 1;
    }
    cx.pattern_blocks.insert(id, name.clone());
    cx.anon_blocks.push(AnonBlock {
        name: name.clone(),
        prims: vec![Prim::Poly {
            pts: c.local.clone(),
            closed: true,
            fill: kadakas_core::annot::draw::Fill::None,
            color: None,
        }],
        unit: cx.unit,
    });
    name
}

/// Writes the pattern of one object (given in model coordinates) into `w`. `budget` is the number
/// of units (or lines) that the file may still take; the objects of the file share it.
#[allow(clippy::too_many_arguments)]
pub(super) fn write(
    w: &mut W,
    cx: &mut Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    mode: PatternExport,
    report: &mut Report,
    budget: &mut usize,
) -> Result<Used, IoError> {
    let mut used = Used::default();
    if mode == PatternExport::None || !matches!(o.shape, Shape::Hardscape { .. }) {
        return Ok(used);
    }
    let style = kadakas_core::hardscape::data::style_of(cx.doc, o)
        .and_then(|(id, _)| cx.doc.resources.get(id).map(|r| r.name.clone()))
        .unwrap_or_default();
    let name = format!("hardscape {} (style {style})", o.id);
    let limit_units = MAX_EXPORT_UNITS.min(*budget);
    let planned = match groups_of(cx.doc, o, cx.tol, limit_units) {
        Ok(Some(p)) => p,
        Ok(None) => return Ok(used),
        Err(e) => return Err(IoError::Option(format!("{name}: {e}"))),
    };
    for note in &planned.notes {
        report.note(&format!("{name}: {note}"));
    }
    let planned_object = planned.groups.len() > 1
        || planned
            .groups
            .iter()
            .any(|g| !matches!(g.role, Role::Field));
    let (mut all_whole, mut all_cut) = (0usize, 0usize);
    for group in &planned.groups {
        let base = if group.role == Role::Field {
            layer.to_string()
        } else {
            format!("{layer}-{}", group.slug)
        };
        let label = if planned_object {
            format!("{name}, {} '{}'", role_word(group.role), group.name)
        } else {
            name.clone()
        };
        let (w_, c_) = write_group(
            w, cx, o, group, &base, &label, hidden, mode, report, budget, &mut used,
        )?;
        all_whole += w_;
        all_cut += c_;
    }
    // A hole in a course or a field must not go into the file silently. The units were written
    // within the budget of the file; the check lays them again outside it.
    {
        let gaps = kadakas_core::layout::check::coverage(
            &planned.groups,
            &kadakas_core::layout::units::Limits {
                max_units: MAX_EXPORT_UNITS,
                trim: kadakas_core::layout::units::DEFAULT_TRIM,
            },
            kadakas_core::layout::check::CheckOptions::default().coverage_tol,
        )
        .map_err(|e| IoError::Option(format!("{name}: {e}")))?;
        if let Some(w) = kadakas_core::layout::check::coverage_warning(&gaps) {
            report.warn(&format!("{name}: {w}"));
        }
        // A gap wider than the joints allow is as silent as a hole.
        let region = kadakas_core::layout::object::region_of(cx.doc, o, cx.tol)
            .map_err(|e| IoError::Option(format!("{name}: {e}")))?;
        let wide = kadakas_core::layout::check::wide_joints(
            &planned.groups,
            &region,
            &kadakas_core::layout::check::CheckOptions {
                max_units: MAX_EXPORT_UNITS,
                ..Default::default()
            },
        )
        .map_err(|e| IoError::Option(format!("{name}: {e}")))?;
        if let Some(w) = kadakas_core::layout::check::wide_joint_warning(&wide) {
            report.warn(&format!("{name}: {w}"));
        }
    }
    if planned_object && mode != PatternExport::Gaps {
        report.note(&format!(
            "{name}: wrote {all_whole} whole and {all_cut} cut units in {} parts",
            planned.groups.len()
        ));
    }
    Ok(used)
}

/// The names of the unit types for the notes of the export. Types that share a name (the two
/// positions of a herringbone, the turned units of a basket weave) get the position added, so
/// that the note says which count is which. The polylines keep the plain name, as the report has
/// it: units of one stone are one row in `by_unit`.
fn distinct_names(group: &Group, names: &[String]) -> Vec<String> {
    let repeated = |n: &String| names.iter().filter(|m| *m == n).count() > 1;
    if !names.iter().any(repeated) {
        return names.to_vec();
    }
    let basket = group
        .pattern()
        .is_some_and(|p| p.bond == kadakas_core::layout::Bond::BasketWeave);
    let half = names.len() / 2;
    let mut seen: std::collections::HashMap<&String, usize> = Default::default();
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if !repeated(n) {
                return n.clone();
            }
            let k = seen.entry(n).or_insert(0);
            *k += 1;
            if basket {
                // n units side by side, then n turned by 90 degrees.
                let (turn, nth) = if i < half {
                    ("lengthwise", i + 1)
                } else {
                    ("crosswise", i - half + 1)
                };
                format!("{n} {turn} {nth}")
            } else if names.iter().filter(|m| *m == n).count() == 2 {
                format!("{n} {}", if *k == 1 { "lengthwise" } else { "crosswise" })
            } else {
                format!("{n} #{k}")
            }
        })
        .collect()
}

fn role_word(r: Role) -> &'static str {
    match r {
        Role::Field => "field",
        Role::Zone => "zone",
        Role::Course => "course",
    }
}

#[allow(clippy::too_many_arguments)]
fn write_group(
    w: &mut W,
    cx: &mut Ctx,
    o: &Object,
    group: &Group,
    base: &str,
    name: &str,
    hidden: bool,
    mode: PatternExport,
    report: &mut Report,
    budget: &mut usize,
    used: &mut Used,
) -> Result<(usize, usize), IoError> {
    let [whole_layer, cut_layer, void_layer, gap_layer] = layer_names(base);
    let shift = cx.shift;
    let ring_verts =
        |pts: &[DVec2]| -> Vec<(DVec2, f64)> { pts.iter().map(|p| (*p + shift, 0.0)).collect() };
    let is_module = group.pattern().is_some_and(|p| p.module.is_some());
    let mut counts = (0usize, 0usize);
    match mode {
        PatternExport::Units | PatternExport::Blocks => {
            let blocks = mode == PatternExport::Blocks;
            let (mut whole, mut cut) = (0usize, 0usize);
            let mut per_type: Vec<(usize, usize)> = Vec::new();
            let limit = MAX_EXPORT_UNITS.min(*budget);
            let names = group.kind_names();
            let stats = group
                .for_each_unit(&Limits::new(limit), &mut |u| {
                    let target = if u.whole { &whole_layer } else { &cut_layer };
                    if per_type.len() <= u.kind {
                        per_type.resize(u.kind + 1, (0, 0));
                    }
                    if u.whole {
                        whole += 1;
                        per_type[u.kind].0 += 1;
                    } else {
                        cut += 1;
                        per_type[u.kind].1 += 1;
                    }
                    let tname = names.get(u.kind).map_or("", String::as_str);
                    if blocks
                        && u.whole
                        && u.rings.len() == 1
                        && let Some(c) = canon(&u.rings[0].points)
                    {
                        let bname = block_for(cx, tname, &c);
                        used.layers.insert(whole_layer.clone());
                        common(w, "INSERT", &whole_layer, o, hidden);
                        w.pair(100, "AcDbBlockReference");
                        w.pair(2, &bname);
                        cx.pt(w, 10, c.center + shift);
                        let a = c.angle_deg.rem_euclid(360.0);
                        if a > 1e-9 && a < 360.0 - 1e-9 {
                            w.num(50, a);
                        }
                        return;
                    }
                    for r in &u.rings {
                        let l = if r.hole { &void_layer } else { target };
                        used.layers.insert(l.clone());
                        lwpolyline(w, cx, o, l, hidden, &ring_verts(&r.points), true);
                        if !r.hole {
                            w.pair(1001, XDATA_APP);
                            w.pair(1000, escape_unicode(tname));
                        }
                    }
                })
                .map_err(|e| {
                    // Only a count over the limit is the limit of the export; a pattern that
                    // cannot be laid (or a region that cannot be scanned) says its own reason.
                    if !matches!(e, layout::LayoutError::TooMany(_)) {
                        return IoError::Option(format!("{name}: {e}"));
                    }
                    let why = if limit < MAX_EXPORT_UNITS {
                        format!(
                            "the file holds at most {MAX_EXPORT_FILE_UNITS} units in all objects"
                        )
                    } else {
                        format!(
                            "the limit of a DXF export is {MAX_EXPORT_UNITS} units for one object"
                        )
                    };
                    IoError::Option(format!(
                        "{name}: {e} ({why}). Export with patterns \"none\", or split the area"
                    ))
                })?;
            *budget = budget.saturating_sub(stats.units);
            used.xdata |= stats.units > 0;
            counts = (whole, cut);
            let how = if blocks {
                " (whole units as blocks)"
            } else {
                ""
            };
            let mut text = format!("{name}: wrote {whole} whole and {cut} cut units{how}");
            if names.len() > 1 {
                let mut parts: Vec<String> = Vec::new();
                let shown = distinct_names(group, &names);
                for (n, nm) in shown.iter().enumerate() {
                    let (w_, c_) = per_type.get(n).copied().unwrap_or((0, 0));
                    if is_module || w_ + c_ > 0 {
                        parts.push(format!("{nm}: {w_} whole, {c_} cut"));
                    }
                }
                text.push_str(&format!(" ({})", parts.join("; ")));
            }
            if stats.dust_pieces > 0 {
                text.push_str(&format!(
                    " ({} pieces under 1 mm2 were left out as dust)",
                    stats.dust_pieces
                ));
            }
            report.note(&text);
        }
        PatternExport::Gaps => {
            let Some((pattern, place)) = (match &group.body {
                layout::plan::Body::Pattern { pattern, place } => Some((pattern, place)),
                layout::plan::Body::Course(_) => None,
            }) else {
                report.note(&format!("{name}: a course has no gap center lines"));
                return Ok(counts);
            };
            let limit = MAX_EXPORT_GAP_LINES.min(*budget);
            let g = gaps::gap_lines(&group.region, pattern, place, limit, false);
            if g.truncated {
                return Err(IoError::Option(format!(
                    "{name}: the pattern needs more than {limit} gap lines (the limit of a DXF export). Export with patterns \"none\", or split the area"
                )));
            }
            *budget = budget.saturating_sub(g.segments.len());
            used.layers.insert(gap_layer.clone());
            for (a, b) in &g.segments {
                common(w, "LINE", &gap_layer, o, hidden);
                w.pair(100, "AcDbLine");
                cx.pt3(w, 10, a.x + shift.x, a.y + shift.y, 0.0);
                cx.pt3(w, 11, b.x + shift.x, b.y + shift.y, 0.0);
            }
            report.note(&format!("{name}: wrote {} gap lines", g.segments.len()));
        }
        PatternExport::None => {}
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64, at: DVec2, turn: f64) -> Vec<DVec2> {
        let r = DVec2::from_angle(turn);
        [
            DVec2::new(0.0, 0.0),
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]
        .iter()
        .map(|p| at + r.rotate(*p))
        .collect()
    }

    #[test]
    fn the_same_shape_turned_and_moved_has_one_key() {
        let a = canon(&rect(200.0, 100.0, DVec2::new(5.0, 7.0), 0.0)).unwrap();
        let b = canon(&rect(200.0, 100.0, DVec2::new(-900.0, 33.0), 0.7)).unwrap();
        let c = canon(&rect(100.0, 200.0, DVec2::new(0.0, 0.0), 0.0)).unwrap();
        let d = canon(&rect(200.0, 100.0, DVec2::new(0.0, 0.0), 0.0)).unwrap();
        assert_eq!(a.key, b.key);
        // A rectangle turned by 90 degrees is the same shape.
        assert_eq!(a.key, c.key);
        assert_eq!(a.key, d.key);
        assert!((a.center - DVec2::new(5.0 + 100.0, 7.0 + 50.0)).length() < 1e-9);
        let e = canon(&rect(200.0, 101.0, DVec2::new(0.0, 0.0), 0.0)).unwrap();
        assert_ne!(a.key, e.key);
        // A clockwise ring is the same as a counterclockwise one.
        let mut cw = rect(200.0, 100.0, DVec2::ZERO, 0.3);
        cw.reverse();
        assert_eq!(
            canon(&cw).unwrap().key,
            canon(&rect(200.0, 100.0, DVec2::ZERO, 0.3)).unwrap().key
        );
        assert!(canon(&[DVec2::ZERO, DVec2::X]).is_none());
    }

    #[test]
    fn the_block_rebuilds_the_unit() {
        // The block turned by the angle and put on the center gives back the corners.
        let ring = rect(200.0, 100.0, DVec2::new(40.0, -30.0), 1.1);
        let c = canon(&ring).unwrap();
        let turn = DVec2::from_angle(c.angle_deg.to_radians());
        let mut back: Vec<DVec2> = c.local.iter().map(|p| c.center + turn.rotate(*p)).collect();
        // Same corners in the same order up to the start.
        let first = ring
            .iter()
            .position(|p| p.distance(back[0]) < 1e-6)
            .expect("a corner");
        back.rotate_right(first);
        for (a, b) in ring.iter().zip(&back) {
            assert!(a.distance(*b) < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn block_names_are_dxf_safe() {
        assert_eq!(block_name_base("200x100"), "KADAKAS_200X100");
        assert_eq!(
            block_name_base("corner 200x200 at 80.5 degrees"),
            "KADAKAS_CORNER_200X200_AT_80_5_DEGREES"
        );
        assert_eq!(block_name_base(""), "KADAKAS_UNIT");
        assert_eq!(layer_names("PAVING")[0], "PAVING-UNITS-WHOLE");
    }
}
