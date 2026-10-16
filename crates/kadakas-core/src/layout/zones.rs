//! Zones (PAT-07): a region cut by lines into parts that share their edges exactly.
//!
//! The region edges and the split lines go into one planar arrangement
//! ([`crate::arrangement`]). The nodes are made once, so two zones that meet along a line use the
//! very same points: there are no slivers and no overlaps, and the areas of the zones add up to the
//! area of the region. The boundary of the region never moves: where the end of a split line is
//! within the tolerance of a region edge, the end moves to the edge.
//!
//! Nothing is dropped silently. A split line that stops short of the region edge (or of another
//! line) leaves a loose end, which is reported with its position and the gap. The part of a line
//! that lies outside the region is harmless and only counted.

use glam::DVec2;

use super::Region;
use crate::arrangement::{self, ClosedGap, Curve};

/// Most split lines of one region.
pub const MAX_LINES: usize = 10_000;
/// Most vertices of all split lines of one region together.
pub const MAX_LINE_POINTS: usize = 200_000;
/// Default tolerance: an end of a line within this many mm of an edge joins it.
pub const DEFAULT_TOLERANCE: f64 = 1.0;

/// A part of a region.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// The outline first, then the holes (flat rings).
    pub shape: Vec<Vec<DVec2>>,
    /// The area, in mm2.
    pub area: f64,
    /// A point inside the zone, not in a hole.
    pub inside: DVec2,
    /// The box of the outline.
    pub lo: DVec2,
    pub hi: DVec2,
}

/// An end of a split line that joins nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct LooseEnd {
    /// The index of the split line.
    pub line: usize,
    pub at: DVec2,
    /// True if the end lies inside the region: the line does not reach the edge or another line.
    pub inside: bool,
    /// The distance to the nearest edge or other line, in mm.
    pub gap: f64,
}

/// The result of a split.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Split {
    pub zones: Vec<Zone>,
    /// Ends that stop inside the region (the line does not cut). They are a message to the person.
    pub loose_ends: Vec<LooseEnd>,
    /// How many line ends lie outside the region (a line drawn longer than the region).
    pub overshoot: usize,
    pub closed_gaps: Vec<ClosedGap>,
    pub closed_gap_count: usize,
    /// Lines that cut nothing: all of their pieces were dead ends or lie outside.
    pub unused_lines: Vec<usize>,
}

/// True if `p` is inside the region (even-odd over the shapes: in an outline and not in a hole).
pub fn region_contains(region: &Region, p: DVec2) -> bool {
    region.shapes.iter().any(|s| {
        s.first()
            .is_some_and(|o| crate::geom::point_in_polygon(p, o))
            && !s
                .iter()
                .skip(1)
                .any(|h| crate::geom::point_in_polygon(p, h))
    })
}

/// Cuts `region` by `lines` (flat polylines; open or closed ones both work). `tolerance` is the
/// biggest gap that is closed (mm).
pub fn split(region: &Region, lines: &[Vec<DVec2>], tolerance: f64) -> Result<Split, String> {
    if lines.len() > MAX_LINES {
        return Err(format!("a region has at most {MAX_LINES} split lines"));
    }
    if lines.iter().map(Vec::len).sum::<usize>() > MAX_LINE_POINTS {
        return Err(format!(
            "the split lines have more than {MAX_LINE_POINTS} points"
        ));
    }
    if lines.iter().flatten().any(|p| !p.is_finite()) {
        return Err("numbers must be finite".into());
    }
    let mut curves: Vec<Curve> = Vec::new();
    let mut ranks: Vec<u8> = Vec::new();
    for ring in region.rings() {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            if a != b {
                curves.push(Curve::line(a, b));
                ranks.push(1);
            }
        }
    }
    let ring_curves = curves.len();
    // The line of each curve of the lines.
    let mut line_of: Vec<usize> = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        for w in l.windows(2) {
            if w[0] != w[1] {
                curves.push(Curve::line(w[0], w[1]));
                ranks.push(0);
                line_of.push(li);
            }
        }
    }
    let a = arrangement::build_ranked(&curves, &ranks, tolerance).map_err(|e| e.to_string())?;
    let mut out = Split {
        closed_gaps: a.closed_gaps.clone(),
        closed_gap_count: a.closed_gap_count,
        ..Default::default()
    };
    for f in &a.faces {
        if !region_contains(region, f.inside) {
            continue;
        }
        let mut shape = vec![arrangement::cycle_ring(&a, &f.outer)];
        for h in &f.holes {
            shape.push(arrangement::cycle_ring(&a, h));
        }
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in &shape[0] {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        out.zones.push(Zone {
            shape,
            area: f.area,
            inside: f.inside,
            lo,
            hi,
        });
    }
    out.zones.sort_by(|x, y| {
        x.lo.x
            .total_cmp(&y.lo.x)
            .then(x.lo.y.total_cmp(&y.lo.y))
            .then(x.area.total_cmp(&y.area))
    });
    // Lines that still have an edge in a face boundary cut something.
    let mut used = vec![false; lines.len()];
    for e in &a.edges {
        if e.source >= ring_curves {
            used[line_of[e.source - ring_curves]] = true;
        }
    }
    // Loose ends: the dead ends that are the first or last point of a line.
    let near_any = |p: DVec2, skip_line: usize| -> f64 {
        let mut best = f64::INFINITY;
        for (i, c) in curves.iter().enumerate() {
            if i >= ring_curves && line_of[i - ring_curves] == skip_line {
                continue;
            }
            best = best.min(c.closest(p).2);
        }
        best
    };
    for d in &a.dead_ends {
        let mut owner: Option<usize> = None;
        for (li, l) in lines.iter().enumerate() {
            if let (Some(f), Some(t)) = (l.first(), l.last())
                && (f.distance(d.at) < 1e-6 || t.distance(d.at) < 1e-6)
            {
                owner = Some(li);
                break;
            }
        }
        let Some(li) = owner else { continue };
        if region_contains(region, d.at) {
            out.loose_ends.push(LooseEnd {
                line: li,
                at: d.at,
                inside: true,
                gap: near_any(d.at, li),
            });
        } else {
            out.overshoot += 1;
        }
    }
    out.unused_lines = (0..lines.len()).filter(|&i| !used[i]).collect();
    Ok(out)
}

#[cfg(test)]
mod tests;
