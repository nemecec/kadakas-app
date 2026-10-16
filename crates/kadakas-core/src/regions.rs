//! Regions from loose curves (PAT-06): lines, arcs and open polylines that were drawn without
//! closing, joined into closed areas with holes.
//!
//! The work is done by the planar arrangement ([`crate::arrangement`]): curves are split where they
//! cross, ends that lie within the tolerance of each other or of another curve are joined (and
//! reported as closed gaps), dead ends are removed (and reported with their position, length and
//! the nearest other end), and the faces are traced. Nothing is guessed: a gap wider than the
//! tolerance stays open, and the area behind it is not made.
//!
//! The boundary of a region keeps its arcs: a vertex of kind `PointOnArc` sits on each arc piece.

use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

use glam::DVec2;

use crate::arrangement::{self, ClosedGap, Curve, DeadEnd};
use crate::curve::{self, Seg, Vertex};
use crate::model::Shape;

/// Chord error when a Bezier piece of a polyline is cut into lines, in mm.
const BEZIER_ERR: f64 = 0.02;

/// The curves of a shape, or why it has none. Lines, arcs, circles, rectangles and polylines
/// (open or closed, with arcs) work.
pub fn curves_of(shape: &Shape) -> Result<Vec<Curve>, String> {
    let finite = |v: DVec2| v.is_finite();
    match shape {
        Shape::Line { a, b } if finite(*a) && finite(*b) => Ok(vec![Curve::line(*a, *b)]),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } if finite(*center) && radius.is_finite() && *radius > 0.0 => {
            let sweep = if end - start >= TAU - 1e-12 {
                TAU
            } else {
                (end - start).rem_euclid(TAU)
            };
            if sweep >= TAU - 1e-12 {
                Ok(vec![
                    Curve::arc_ccw(*center, *radius, *start, start + PI),
                    Curve::arc_ccw(*center, *radius, start + PI, start + TAU),
                ])
            } else if sweep <= 0.0 {
                Err("the arc has no length".into())
            } else {
                Ok(vec![Curve::Arc {
                    c: *center,
                    r: *radius,
                    start: *start,
                    sweep,
                }])
            }
        }
        Shape::Circle { center, radius } if finite(*center) && radius.is_finite() && *radius > 0.0 => {
            Ok(vec![
                Curve::arc_ccw(*center, *radius, 0.0, PI),
                Curve::arc_ccw(*center, *radius, PI, TAU),
            ])
        }
        Shape::Rect { min, max } if finite(*min) && finite(*max) => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            Ok((0..4).map(|i| Curve::line(c[i], c[(i + 1) % 4])).collect())
        }
        Shape::Polyline { points, closed } => {
            if points.len() < 2 || points.iter().any(|v| !v.is_finite()) {
                return Err("the polyline needs 2 or more finite points".into());
            }
            Ok(curves_of_vertices(points, *closed))
        }
        other => Err(format!(
            "a {} has no curves to join (lines, arcs, circles, rectangles and polylines work)",
            other.kind()
        )),
    }
}

/// The curves of a polyline.
pub fn curves_of_vertices(points: &[Vertex], closed: bool) -> Vec<Curve> {
    let p = curve::path(points, closed);
    let mut out = Vec::with_capacity(p.segs.len());
    for seg in &p.segs {
        match *seg {
            Seg::Line { from, to } => {
                if from != to {
                    out.push(Curve::line(from, to));
                }
            }
            Seg::Arc {
                center,
                radius,
                start,
                sweep,
                ..
            } => {
                if sweep.abs() >= TAU - 1e-12 {
                    out.push(Curve::Arc {
                        c: center,
                        r: radius,
                        start,
                        sweep: sweep / 2.0,
                    });
                    out.push(Curve::Arc {
                        c: center,
                        r: radius,
                        start: start + sweep / 2.0,
                        sweep: sweep / 2.0,
                    });
                } else if sweep != 0.0 {
                    out.push(Curve::Arc {
                        c: center,
                        r: radius,
                        start,
                        sweep,
                    });
                }
            }
            s @ (Seg::Quad { .. } | Seg::Cubic { .. }) => {
                let one = curve::Path {
                    start: s.from(),
                    segs: vec![s],
                    closed: false,
                };
                let pts = curve::flatten_path(&one, BEZIER_ERR);
                for w in pts.windows(2) {
                    if w[0] != w[1] {
                        out.push(Curve::line(w[0], w[1]));
                    }
                }
            }
        }
    }
    out
}

/// A closed area found in loose curves.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundRegion {
    /// The outline, counterclockwise. Arcs are kept (a `PointOnArc` vertex on each).
    pub outer: Vec<Vertex>,
    /// The islands inside, counterclockwise.
    pub holes: Vec<Vec<Vertex>>,
    /// The area with the holes taken off, in mm2.
    pub area: f64,
    /// 0 for an area that no other encloses; 1 for the inside of a hole of such an area; and so on.
    /// Even depths are areas to fill, odd depths are islands.
    pub depth: usize,
    /// A point inside (not in a hole).
    pub inside: DVec2,
    /// The input curves that form the outline and the holes (indexes into the list given).
    pub curves: Vec<usize>,
    /// The box of the outline.
    pub lo: DVec2,
    pub hi: DVec2,
}

/// What [`from_curves`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub regions: Vec<FoundRegion>,
    pub closed_gaps: Vec<ClosedGap>,
    pub closed_gap_count: usize,
    pub dead_ends: Vec<DeadEnd>,
    pub dead_end_count: usize,
    /// Dead ends that are part of a shape that does not close (the others are stubs).
    pub open_dead_ends: usize,
    pub ignored_short: usize,
    pub duplicates: usize,
}

/// Joins `curves` into regions. The tolerance (mm) is the biggest gap that is closed.
pub fn from_curves(curves: &[Curve], tolerance: f64) -> Result<Found, arrangement::BuildError> {
    let a = arrangement::build(curves, tolerance)?;
    let mut found = Found {
        closed_gaps: a.closed_gaps.clone(),
        closed_gap_count: a.closed_gap_count,
        dead_ends: a.dead_ends.clone(),
        dead_end_count: a.dead_end_count,
        open_dead_ends: a.open_dead_ends,
        ignored_short: a.ignored_short,
        duplicates: a.duplicates,
        ..Default::default()
    };
    for f in &a.faces {
        let ring = arrangement::cycle_ring(&a, &f.outer);
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in &ring {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let mut sources: BTreeSet<usize> = BTreeSet::new();
        for cy in std::iter::once(&f.outer).chain(f.holes.iter()) {
            for &(e, _) in &cy.edges {
                sources.insert(a.edges[e].source);
            }
        }
        found.regions.push(FoundRegion {
            outer: arrangement::cycle_vertices(&a, &f.outer),
            holes: f
                .holes
                .iter()
                .map(|h| arrangement::cycle_vertices(&a, h))
                .collect(),
            area: f.area,
            depth: f.depth,
            inside: f.inside,
            curves: sources.into_iter().collect(),
            lo,
            hi,
        });
    }
    // A stable order: by the corner of the box.
    found.regions.sort_by(|x, y| {
        x.lo.x
            .total_cmp(&y.lo.x)
            .then(x.lo.y.total_cmp(&y.lo.y))
            .then(y.area.total_cmp(&x.area))
    });
    Ok(found)
}

#[cfg(test)]
mod tests;
