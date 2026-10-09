//! Site modifiers and the proposed surface.
//!
//! The proposed surface is an overlay of the existing surface. Outside the work, its triangles are
//! the existing ones, so the ground there does not change. The work is made of three kinds of
//! region:
//!
//! - A pad replaces the ground inside its boundary with a plane. Where pads overlap, the pad that
//!   comes later in the list wins (modifiers nest, the last one is on top).
//! - A grade limit that holds a pad is a slope region: between the pad edges and the limit, the
//!   old points are removed and the ground slopes from the pad edge to the existing ground at the
//!   limit.
//! - A pad that no grade limit holds has a vertical edge (a wall): the ground right outside the
//!   pad edge keeps its old height. The wall has no width. The pad triangles and the old ground
//!   triangles touch along the edge at different heights.
//!
//! Rings are cut into the existing triangles, so the new points sit exactly where a ring crosses
//! an existing edge. The parts of the surface that lie outside the existing surface are dropped.

use glam::DVec2;

use crate::SiteError;
use crate::overlay::{NONE, Out, Overlay};
use crate::tin::Tin;

/// A site modifier with a plain polygon boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum Modifier {
    /// A flat or sloped area. `elevation` is the height at the average of the boundary points.
    /// A slope falls in `direction` (radians from +X) by `percent`.
    Pad { boundary: Vec<DVec2>, elevation: f64, slope: Option<PadSlope> },
    /// The reach of the modifications.
    GradeLimit { boundary: Vec<DVec2> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadSlope {
    pub direction: f64,
    pub percent: f64,
}

/// Height of a pad at `p`.
pub fn pad_elevation(boundary: &[DVec2], elevation: f64, slope: Option<PadSlope>, p: DVec2) -> f64 {
    match slope {
        None => elevation,
        Some(s) => {
            let mid = boundary.iter().copied().sum::<DVec2>() / boundary.len().max(1) as f64;
            elevation - s.percent / 100.0 * DVec2::from_angle(s.direction).dot(p - mid)
        }
    }
}

/// True if `p` is on an edge of the polygon.
pub fn on_edge(poly: &[DVec2], p: DVec2) -> bool {
    let n = poly.len();
    (0..n).any(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let ab = b - a;
        let len2 = ab.length_squared();
        let t = if len2 == 0.0 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
        p.distance_squared(a + ab * t) <= 1e-18 * (1.0 + p.length_squared())
    })
}

/// True if `p` is inside the polygon (even-odd rule). The result for a point on an edge is not defined.
fn winding_inside(poly: &[DVec2], p: DVec2) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// True if `p` is inside the polygon or on its edge.
pub fn inside_or_on(poly: &[DVec2], p: DVec2) -> bool {
    on_edge(poly, p) || winding_inside(poly, p)
}

/// True if `p` is inside the polygon and not on its edge.
fn strictly_inside(poly: &[DVec2], p: DVec2) -> bool {
    !on_edge(poly, p) && winding_inside(poly, p)
}

struct Pad<'a> {
    boundary: &'a [DVec2],
    elevation: f64,
    slope: Option<PadSlope>,
    mid: DVec2,
}

impl Pad<'_> {
    fn z(&self, p: DVec2) -> f64 {
        match self.slope {
            None => self.elevation,
            Some(s) => self.elevation - s.percent / 100.0 * DVec2::from_angle(s.direction).dot(p - self.mid),
        }
    }
}

/// The pad on top at `p`: the last one in the list that holds the point.
fn top_pad<'a, 'b>(pads: &'b [Pad<'a>], p: DVec2) -> Option<&'b Pad<'a>> {
    pads.iter().rev().find(|k| inside_or_on(k.boundary, p))
}

fn bounds_of(rings: impl Iterator<Item = impl AsRef<[DVec2]>>) -> Option<(DVec2, DVec2)> {
    let mut b: Option<(DVec2, DVec2)> = None;
    for r in rings {
        for p in r.as_ref() {
            b = Some(b.map_or((*p, *p), |(lo, hi)| (lo.min(*p), hi.max(*p))));
        }
    }
    b
}

/// Builds the proposed surface from the existing surface and the modifiers.
/// With no pads it is the existing surface again.
pub fn proposed_surface(existing: &Tin, modifiers: &[Modifier]) -> Result<Tin, SiteError> {
    let pads: Vec<Pad> = modifiers
        .iter()
        .filter_map(|m| match m {
            Modifier::Pad { boundary, elevation, slope } if boundary.len() >= 3 => Some(Pad {
                boundary,
                elevation: *elevation,
                slope: *slope,
                mid: boundary.iter().copied().sum::<DVec2>() / boundary.len() as f64,
            }),
            _ => None,
        })
        .collect();
    if pads.is_empty() {
        return Ok(existing.clone());
    }
    // A grade limit counts when it holds a whole pad. Its inside is a slope region.
    let zones: Vec<&[DVec2]> = modifiers
        .iter()
        .filter_map(|m| match m {
            Modifier::GradeLimit { boundary } if boundary.len() >= 3 => Some(boundary.as_slice()),
            _ => None,
        })
        .filter(|l| pads.iter().any(|k| k.boundary.iter().all(|p| inside_or_on(l, *p))))
        .collect();

    let (lo, hi) = bounds_of(pads.iter().map(|k| k.boundary).chain(zones.iter().copied())).expect("there is a pad");
    let mut affected = vec![false; existing.triangles.len()];
    let candidates = existing.triangles_in_box(lo, hi);
    for t in &candidates {
        affected[*t as usize] = true;
    }
    let ground_z = |p: DVec2| existing.elevation_at(p).unwrap_or(0.0);

    // The overlay of the affected triangles with every ring cut in.
    let mut a = Overlay::of_triangles(existing, candidates.iter().map(|t| *t as usize))?;
    for k in &pads {
        a.ring(k.boundary, &ground_z)?;
    }
    let mut zone_chains: Vec<Vec<(DVec2, f64)>> = Vec::new();
    for l in &zones {
        zone_chains.push(a.ring(l, &ground_z)?);
    }

    let mut out = Out::new(existing);
    for (t, tri) in existing.triangles.iter().enumerate() {
        if !affected[t] {
            out.triangles.push(*tri);
        }
    }
    let on_existing = |c: DVec2| existing.locate(c).filter(|(t, _)| affected[*t]).is_some();
    for f in a.faces() {
        let c = (f[0].xy + f[1].xy + f[2].xy) / 3.0;
        if !on_existing(c) {
            continue;
        }
        if let Some(k) = top_pad(&pads, c) {
            out.triangle(&f, |p, _| k.z(p));
        } else if !zones.iter().any(|l| inside_or_on(l, c)) {
            out.triangle(&f, |_, z| z);
        }
    }

    if !zones.is_empty() {
        slope_regions(existing, &pads, &zones, &zone_chains, (lo, hi), &mut out)?;
    }
    out.finish()
}

/// Triangulates the slope regions: the inside of each grade limit that holds a pad, without the pads.
fn slope_regions(
    existing: &Tin,
    pads: &[Pad],
    zones: &[&[DVec2]],
    chains: &[Vec<(DVec2, f64)>],
    (lo, hi): (DVec2, DVec2),
    out: &mut Out,
) -> Result<(), SiteError> {
    let ground_z = |p: DVec2| existing.elevation_at(p).unwrap_or(0.0);
    let z = |p: DVec2| top_pad(pads, p).map_or_else(|| ground_z(p), |k| k.z(p));
    let mut b = Overlay::new();
    // The limit rings, with the points where they cross the existing edges. A point that lies
    // deep inside another slope region is not on the edge of the work, so it is left out.
    for (i, chain) in chains.iter().enumerate() {
        let others = || zones.iter().enumerate().filter(move |(j, _)| *j != i).map(|(_, l)| *l);
        let free = |p: DVec2| !others().any(|l| strictly_inside(l, p));
        let mut prev: Option<(DVec2, bool)> = None;
        let mut prev_h = None;
        for (p, _) in chain {
            let ok = free(*p);
            let h = if ok { Some(b.point(*p, z(*p), NONE)?) } else { None };
            if let (Some((_, true)), Some(ph), Some(h)) = (prev, prev_h, h) {
                b.edge(ph, h);
            }
            prev = Some((*p, ok));
            prev_h = h;
        }
    }
    for k in pads {
        b.ring(k.boundary, &z)?;
    }
    // Join each corner of a pad to the nearest point of a grade limit. Without these edges the
    // choice of triangles at the corners is arbitrary and can cut a corner of the slope.
    let limit_points: Vec<DVec2> = zones.iter().flat_map(|l| l.iter().copied()).collect();
    for (i, k) in pads.iter().enumerate() {
        for v in k.boundary {
            let on_top = pads.iter().skip(i + 1).all(|o| !strictly_inside(o.boundary, *v));
            let nearest = limit_points.iter().copied().min_by(|p, q| p.distance_squared(*v).total_cmp(&q.distance_squared(*v)));
            if let (true, Some(w)) = (on_top, nearest) {
                let (hv, hw) = (b.point(*v, z(*v), NONE)?, b.point(w, z(w), NONE)?);
                b.edge(hv, hw);
            }
        }
    }
    // The edge of the existing surface inside the work stays the edge of the new surface.
    for (u, v) in existing.boundary_edges() {
        let (pu, pv) = (existing.vertices[u as usize], existing.vertices[v as usize]);
        let (blo, bhi) = (pu.truncate().min(pv.truncate()), pu.truncate().max(pv.truncate()));
        if blo.x <= hi.x && bhi.x >= lo.x && blo.y <= hi.y && bhi.y >= lo.y {
            b.segment(pu.truncate(), pv.truncate(), &ground_z)?;
        }
    }
    for f in b.faces() {
        let c = (f[0].xy + f[1].xy + f[2].xy) / 3.0;
        if existing.locate(c).is_some() && zones.iter().any(|l| inside_or_on(l, c)) && top_pad(pads, c).is_none() {
            out.triangle(&f, |_, z| z);
        }
    }
    Ok(())
}


#[cfg(test)]
#[path = "modifier_tests.rs"]
mod tests;
