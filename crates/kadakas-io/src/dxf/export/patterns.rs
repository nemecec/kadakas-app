//! The unit pattern of hardscapes in a DXF export: gap center lines or the units themselves.
//!
//! Arcs of the outline are cut into straight pieces within the export tolerance, so a cut unit has
//! short straight edges along a curved boundary. The counts here are the counts of
//! `report.unit_pattern`, because both call the same layout engine with the same tolerance.
//!
//! The kerb border of a hardscape is not paved: the units lie inside the border, as in the report.
//!
//! The type of a unit (the name of its shape in a module pattern) is written as extended data of
//! the polyline: application `KADAKAS_UNIT`, group 1000 with the name. The four bonds have one
//! type and write none.

use std::collections::BTreeSet;

use kadakas_core::DVec2;
use kadakas_core::layout::{self, Limits, gaps, object};
use kadakas_core::model::{Object, Shape};

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

/// Writes the pattern of one object (given in model coordinates) into `w`. `budget` is the number
/// of units (or lines) that the file may still take; the objects of the file share it.
#[allow(clippy::too_many_arguments)]
pub(super) fn write(
    w: &mut W,
    cx: &Ctx,
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
    let Some((pattern, place)) = object::pattern_of(cx.doc, o) else {
        return Ok(used);
    };
    let style = kadakas_core::hardscape::data::style_of(cx.doc, o)
        .and_then(|(id, _)| cx.doc.resources.get(id).map(|r| r.name.clone()))
        .unwrap_or_default();
    let name = format!("hardscape {} (style {style})", o.id);
    let region = object::region_of(cx.doc, o, cx.tol)
        .map_err(|e| IoError::Option(format!("{name}: {e}")))?;
    let [whole_layer, cut_layer, void_layer, gap_layer] = layer_names(layer);
    let shift = cx.shift;
    let ring_verts =
        |pts: &[DVec2]| -> Vec<(DVec2, f64)> { pts.iter().map(|p| (*p + shift, 0.0)).collect() };
    let is_module = pattern.module.is_some();
    match mode {
        PatternExport::Units => {
            let (mut whole, mut cut) = (0usize, 0usize);
            let mut per_type: Vec<(usize, usize)> = Vec::new();
            let limit = MAX_EXPORT_UNITS.min(*budget);
            let mut names: Vec<String> = Vec::new();
            if let Ok(c) = pattern.compile() {
                names = c.kinds.iter().map(|k| k.name.clone()).collect();
            }
            let stats =
                layout::for_each_unit(&region, &pattern, &place, &Limits::new(limit), &mut |u| {
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
                    for r in &u.rings {
                        let l = if r.hole { &void_layer } else { target };
                        used.layers.insert(l.clone());
                        lwpolyline(w, cx, o, l, hidden, &ring_verts(&r.points), true);
                        if is_module && !r.hole {
                            let tname = names.get(u.kind).map_or("", String::as_str);
                            w.pair(1001, XDATA_APP);
                            w.pair(1000, escape_unicode(tname));
                        }
                    }
                })
                .map_err(|e| {
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
            used.xdata = is_module && stats.units > 0;
            let mut text = format!("{name}: wrote {whole} whole and {cut} cut units");
            if is_module && names.len() > 1 {
                let mut parts: Vec<String> = Vec::new();
                for (n, nm) in names.iter().enumerate() {
                    let (w_, c_) = per_type.get(n).copied().unwrap_or((0, 0));
                    parts.push(format!("{nm}: {w_} whole, {c_} cut"));
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
            let limit = MAX_EXPORT_GAP_LINES.min(*budget);
            let g = gaps::gap_lines(&region, &pattern, &place, limit, false);
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
    Ok(used)
}
