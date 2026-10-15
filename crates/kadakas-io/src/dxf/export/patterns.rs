//! The unit pattern of hardscapes in a DXF export: gap center lines or the units themselves.
//!
//! Arcs of the outline are cut into straight pieces within the export tolerance, so a cut unit has
//! short straight edges along a curved boundary. The counts here are the counts of
//! `report.pattern`, because both call the same layout engine with the same tolerance.

use std::collections::BTreeSet;

use kadakas_core::DVec2;
use kadakas_core::layout::{self, gaps, object};
use kadakas_core::model::{Object, Shape};

use super::{Ctx, W, common, lwpolyline};
use crate::options::PatternExport;
use crate::{IoError, Report};

/// Most units that one export writes for one object. Measured (test `measure_export`): a 5,000 m2
/// lot of 200 x 100 mm pavers has 233,000 units and gives 60 MB of DXF in 0.85 s with a peak of
/// 300 MB; 514,000 units give 132 MB in 1.8 s with a peak of 525 MB (about 257 bytes and 3.5 us
/// per unit). 600,000 units is 2.4 times the real job, 155 MB of text and a peak near 610 MB.
pub const MAX_EXPORT_UNITS: usize = 600_000;
/// Most gap center lines that one export writes for one object (about 154 bytes each: 1,000,000
/// lines are 154 MB).
pub const MAX_EXPORT_GAP_LINES: usize = 1_000_000;

/// The names of the layers that a pattern export adds to a base layer.
pub fn layer_names(base: &str) -> [String; 4] {
    [
        format!("{base}-UNITS-WHOLE"),
        format!("{base}-UNITS-CUT"),
        format!("{base}-UNITS-VOID"),
        format!("{base}-GAPS"),
    ]
}

/// Writes the pattern of one object (given in model coordinates) into `w`. Returns the layers it
/// used.
pub(super) fn write(
    w: &mut W,
    cx: &Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    mode: PatternExport,
    report: &mut Report,
) -> Result<BTreeSet<String>, IoError> {
    let mut used = BTreeSet::new();
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
    let region =
        object::region_of(o, cx.tol).map_err(|e| IoError::Option(format!("{name}: {e}")))?;
    let [whole_layer, cut_layer, void_layer, gap_layer] = layer_names(layer);
    let shift = cx.shift;
    let ring_verts =
        |pts: &[DVec2]| -> Vec<(DVec2, f64)> { pts.iter().map(|p| (*p + shift, 0.0)).collect() };
    match mode {
        PatternExport::Units => {
            let (mut whole, mut cut) = (0usize, 0usize);
            let stats = layout::for_each_unit(&region, &pattern, &place, MAX_EXPORT_UNITS, &mut |u| {
                let target = if u.whole { &whole_layer } else { &cut_layer };
                if u.whole {
                    whole += 1;
                } else {
                    cut += 1;
                }
                for r in &u.rings {
                    let l = if r.hole { &void_layer } else { target };
                    used.insert(l.clone());
                    lwpolyline(w, cx, o, l, hidden, &ring_verts(&r.points), true);
                }
            })
            .map_err(|e| {
                IoError::Option(format!(
                    "{name}: {e} (the limit of a DXF export). Export with patterns \"none\", or split the area"
                ))
            })?;
            report.note(&format!(
                "{name}: wrote {whole} whole and {cut} cut units{}",
                if stats.dust_pieces > 0 {
                    format!(
                        " ({} pieces under 1 mm2 were left out as dust)",
                        stats.dust_pieces
                    )
                } else {
                    String::new()
                }
            ));
        }
        PatternExport::Gaps => {
            let g = gaps::gap_lines(&region, &pattern, &place, MAX_EXPORT_GAP_LINES, false);
            if g.truncated {
                return Err(IoError::Option(format!(
                    "{name}: the pattern needs more than {MAX_EXPORT_GAP_LINES} gap lines (the limit of a DXF export). Export with patterns \"none\", or split the area"
                )));
            }
            used.insert(gap_layer.clone());
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
