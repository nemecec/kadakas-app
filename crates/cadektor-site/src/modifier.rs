//! Site modifiers and the proposed surface.
//!
//! The proposed surface starts as the existing source data. A pad replaces the ground inside its
//! boundary with a plane. A grade limit marks how far the change reaches: inside a grade limit
//! that holds a pad, the old points are removed, so the ground slopes from the pad edge to the
//! limit, where it meets the existing surface again. A pad that no grade limit holds gets a
//! vertical edge (a wall): the ground right outside the pad edge keeps its old height.
//! A TIN cannot hold a true wall, so the outside points sit `WALL_GAP` away from the pad edge.

use std::collections::HashSet;

use glam::{DVec2, DVec3};

use crate::SiteError;
use crate::tin::{SourceData, SourceLine, Tin, TinBuilder};

/// Distance between the pad edge and the ground outside a vertical pad edge, in mm.
/// The error in a volume is less than this gap times the pad edge length times the height.
pub const WALL_GAP: f64 = 1e-4;

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

/// True if `p` is inside the polygon or on its edge.
pub fn inside_or_on(poly: &[DVec2], p: DVec2) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let ab = b - a;
        let len2 = ab.length_squared();
        let t = if len2 == 0.0 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
        if p.distance_squared(a + ab * t) <= 1e-18 * (1.0 + p.length_squared()) {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// The polygon moved outward by `gap` at every corner. Sharp corners are limited to 4 times the gap.
fn offset_outward(poly: &[DVec2], gap: f64) -> Vec<DVec2> {
    let n = poly.len();
    let twice: f64 = (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum();
    // For a counterclockwise polygon the outside is on the right of each edge.
    let sign = if twice >= 0.0 { 1.0 } else { -1.0 };
    let normal = |a: DVec2, b: DVec2| (DVec2::new((b - a).y, -(b - a).x) * sign).normalize_or_zero();
    (0..n)
        .map(|i| {
            let (prev, cur, next) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            let (n1, n2) = (normal(prev, cur), normal(cur, next));
            let bis = (n1 + n2).normalize_or_zero();
            let scale = gap / bis.dot(n1).max(0.25);
            cur + bis * scale
        })
        .collect()
}

/// Splits a polyline into runs of vertices that are kept. Returns (run, whole line kept).
fn runs(points: &[DVec3], closed: bool, keep: impl Fn(DVec3) -> bool) -> Vec<SourceLine> {
    let flags: Vec<bool> = points.iter().map(|p| keep(*p)).collect();
    if flags.iter().all(|k| *k) {
        return vec![SourceLine { points: points.to_vec(), closed }];
    }
    let mut out = Vec::new();
    let mut cur: Vec<DVec3> = Vec::new();
    let n = points.len();
    // For a closed line, start after a dropped vertex so that no run is split in two.
    let start = if closed { flags.iter().position(|k| !*k).map_or(0, |i| i + 1) } else { 0 };
    for step in 0..n {
        let i = (start + step) % n;
        if flags[i] {
            cur.push(points[i]);
        } else if !cur.is_empty() {
            out.push(SourceLine { points: std::mem::take(&mut cur), closed: false });
        }
    }
    if !cur.is_empty() {
        out.push(SourceLine { points: cur, closed: false });
    }
    out
}

/// Builds the proposed surface from the existing source data and surface and the modifiers.
/// With no modifiers it is the existing surface again.
pub fn proposed_surface(src: &SourceData, existing: &Tin, modifiers: &[Modifier]) -> Result<Tin, SiteError> {
    let pads: Vec<(&[DVec2], f64, Option<PadSlope>)> = modifiers
        .iter()
        .filter_map(|m| match m {
            Modifier::Pad { boundary, elevation, slope } if boundary.len() >= 3 => Some((boundary.as_slice(), *elevation, *slope)),
            _ => None,
        })
        .collect();
    // A grade limit counts when it holds a whole pad.
    let limits: Vec<&[DVec2]> = modifiers
        .iter()
        .filter_map(|m| match m {
            Modifier::GradeLimit { boundary } if boundary.len() >= 3 => Some(boundary.as_slice()),
            _ => None,
        })
        .filter(|l| pads.iter().any(|(b, ..)| b.iter().all(|p| inside_or_on(l, *p))))
        .collect();
    if pads.is_empty() {
        return Tin::build(src);
    }
    // Existing vertices on the outer edge stay, so the surface does not shrink.
    let hull: HashSet<(u64, u64)> = existing
        .boundary_vertices()
        .into_iter()
        .map(|i| {
            let v = existing.vertices[i as usize];
            (v.x.to_bits(), v.y.to_bits())
        })
        .collect();
    let keep = |p: DVec3| {
        let xy = p.truncate();
        if pads.iter().any(|(b, ..)| inside_or_on(b, xy)) {
            return false;
        }
        hull.contains(&(p.x.to_bits(), p.y.to_bits())) || !limits.iter().any(|l| inside_or_on(l, xy))
    };

    let mut b = TinBuilder::default();
    for (boundary, elevation, slope) in &pads {
        let ring: Vec<DVec3> = boundary.iter().map(|p| p.extend(pad_elevation(boundary, *elevation, *slope, *p))).collect();
        b.add_line(&ring, true)?;
    }
    // A pad that no grade limit holds has a vertical edge: the old ground starts right outside it.
    for (boundary, ..) in pads.iter().filter(|(b, ..)| !limits.iter().any(|l| b.iter().all(|p| inside_or_on(l, *p)))) {
        // Next to the edge of the ground the offset point may fall outside: use the height at the corner.
        let ring: Vec<DVec3> = offset_outward(boundary, WALL_GAP)
            .into_iter()
            .zip(boundary.iter())
            .filter_map(|(p, corner)| existing.elevation_at(p).or_else(|| existing.elevation_at(*corner)).map(|z| p.extend(z)))
            .collect();
        b.add_line(&ring, true)?;
    }
    for limit in &limits {
        let ring: Vec<DVec3> = limit.iter().filter_map(|p| existing.elevation_at(*p).map(|z| p.extend(z))).collect();
        b.add_line(&ring, true)?;
    }
    for p in src.points.iter().filter(|p| keep(**p)) {
        b.add_point(*p)?;
    }
    for l in &src.lines {
        for run in runs(&l.points, l.closed, keep) {
            b.add_line(&run.points, run.closed)?;
        }
    }
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::{cut_fill, cut_fill_in};

    fn flat_grid(n: i32, step: f64, from: f64, z: f64) -> SourceData {
        let mut points = Vec::new();
        for i in 0..n {
            for j in 0..n {
                points.push(DVec3::new(from + i as f64 * step, from + j as f64 * step, z));
            }
        }
        SourceData { points, lines: vec![] }
    }

    fn square(lo: f64, hi: f64) -> Vec<DVec2> {
        vec![DVec2::new(lo, lo), DVec2::new(hi, lo), DVec2::new(hi, hi), DVec2::new(lo, hi)]
    }

    #[test]
    fn pad_elevation_flat_and_sloped() {
        let sq = square(0.0, 10.0);
        assert_eq!(pad_elevation(&sq, 5.0, None, DVec2::new(3.0, 3.0)), 5.0);
        // Falls toward +X by 10 percent: the middle is 5, one unit east is 4.9.
        let s = Some(PadSlope { direction: 0.0, percent: 10.0 });
        assert!((pad_elevation(&sq, 5.0, s, DVec2::new(5.0, 5.0)) - 5.0).abs() < 1e-12);
        assert!((pad_elevation(&sq, 5.0, s, DVec2::new(6.0, 0.0)) - 4.9).abs() < 1e-12);
        assert!((pad_elevation(&sq, 5.0, s, DVec2::new(5.0, 9.0)) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn inside_or_on_counts_the_edge() {
        let sq = square(0.0, 10.0);
        assert!(inside_or_on(&sq, DVec2::new(5.0, 5.0)));
        assert!(inside_or_on(&sq, DVec2::new(10.0, 4.0)));
        assert!(inside_or_on(&sq, DVec2::new(0.0, 0.0)));
        assert!(!inside_or_on(&sq, DVec2::new(10.1, 4.0)));
        assert!(!inside_or_on(&[], DVec2::ZERO));
    }

    #[test]
    fn no_pads_leaves_the_surface_unchanged() {
        let src = flat_grid(5, 10.0, 0.0, 3.0);
        let existing = Tin::build(&src).unwrap();
        let limit_only = Modifier::GradeLimit { boundary: square(5.0, 25.0) };
        for mods in [vec![], vec![limit_only]] {
            let p = proposed_surface(&src, &existing, &mods).unwrap();
            let r = cut_fill(&existing, &p);
            assert!(r.cut.abs() < 1e-9 && r.fill.abs() < 1e-9);
            assert_eq!(p.vertices.len(), existing.vertices.len());
        }
    }

    #[test]
    fn box_pad_on_flat_ground_is_area_times_height() {
        // Flat ground at 0 over -20..30. A 10 by 10 pad at 1000 above the ground, with a grade limit 1 around it.
        let src = flat_grid(11, 5.0, -20.0, 0.0);
        let existing = Tin::build(&src).unwrap();
        let mods = vec![
            Modifier::GradeLimit { boundary: square(-1.0, 11.0) },
            Modifier::Pad { boundary: square(0.0, 10.0), elevation: 1.0, slope: None },
        ];
        let proposed = proposed_surface(&src, &existing, &mods).unwrap();
        // Inside the pad: area times height, exactly.
        let in_pad = cut_fill_in(&existing, &proposed, Some(&square(0.0, 10.0)));
        assert!((in_pad.fill - 100.0).abs() < 1e-9, "{in_pad:?}");
        assert!(in_pad.cut.abs() < 1e-12);
        // Whole site: the pad plus the slope around it (a frustum 12 wide on the bottom).
        let all = cut_fill(&existing, &proposed);
        assert!(all.cut.abs() < 1e-9);
        assert!((all.fill - 121.0 - 1.0 / 3.0).abs() < 1e-6, "{all:?}");
        // The ground outside the limit is untouched.
        assert!(proposed.elevation_at(DVec2::new(20.0, 20.0)).unwrap().abs() < 1e-12);
        assert!((proposed.elevation_at(DVec2::new(5.0, 5.0)).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn pad_below_ground_is_cut_and_sloped_pad_averages() {
        let src = flat_grid(7, 10.0, 0.0, 0.0);
        let existing = Tin::build(&src).unwrap();
        let pad = |elevation, slope| vec![Modifier::Pad { boundary: square(20.0, 40.0), elevation, slope }];
        let p = proposed_surface(&src, &existing, &pad(-2.0, None)).unwrap();
        let r = cut_fill_in(&existing, &p, Some(&square(20.0, 40.0)));
        assert!((r.cut - 800.0).abs() < 1e-9 && r.fill.abs() < 1e-12, "{r:?}");
        // A sloped pad through the ground level at its middle: equal cut and fill.
        let slope = Some(PadSlope { direction: 0.0, percent: 5.0 });
        let p = proposed_surface(&src, &existing, &pad(0.0, slope)).unwrap();
        let r = cut_fill_in(&existing, &p, Some(&square(20.0, 40.0)));
        // Half of the pad is 0..+0.5 above ground on average 0.25 over 10 by 20: 50 each side.
        assert!((r.cut - 50.0).abs() < 1e-9 && (r.fill - 50.0).abs() < 1e-9, "{r:?}");
        assert!((p.elevation_at(DVec2::new(20.0, 25.0)).unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn contours_and_hull_survive_a_pad() {
        // A source line crosses the pad: its vertices inside are dropped, the rest stay.
        let mut src = flat_grid(5, 10.0, 0.0, 0.0);
        src.lines.push(SourceLine {
            points: vec![DVec3::new(0.0, 5.0, 0.0), DVec3::new(15.0, 5.0, 0.0), DVec3::new(25.0, 5.0, 0.0), DVec3::new(40.0, 5.0, 0.0)],
            closed: false,
        });
        src.lines.push(SourceLine { points: square(0.0, 40.0).iter().map(|p| p.extend(0.0)).collect(), closed: true });
        let existing = Tin::build(&src).unwrap();
        let mods = vec![
            Modifier::GradeLimit { boundary: square(-5.0, 100.0) },
            Modifier::Pad { boundary: square(12.0, 28.0), elevation: 0.5, slope: None },
        ];
        let p = proposed_surface(&src, &existing, &mods).unwrap();
        // The limit covers the whole site, but the hull is kept.
        assert!(existing.boundary_vertices().iter().all(|i| {
            let v = existing.vertices[*i as usize];
            p.elevation_at(v.truncate()).is_some()
        }));
        assert!((p.area_xy() - existing.area_xy()).abs() < 1e-6);
        // A closed line that has dropped vertices becomes open runs.
        let ring = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0), DVec3::new(20.0, 20.0, 0.0), DVec3::new(0.0, 10.0, 0.0)];
        let r = runs(&ring, true, |p| p.x != 20.0);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].points.len(), 3);
        assert!(!r[0].closed);
        assert_eq!(runs(&ring, true, |_| true)[0].points.len(), 4);
        assert_eq!(runs(&ring, false, |p| p.x != 10.0).len(), 2);
    }

    #[test]
    fn pad_without_grade_limits_has_a_vertical_edge() {
        let src = flat_grid(11, 10.0, 0.0, 0.0);
        let existing = Tin::build(&src).unwrap();
        let pad = Modifier::Pad { boundary: square(23.0, 61.0), elevation: 7.0, slope: None };
        let p = proposed_surface(&src, &existing, &[pad]).unwrap();
        // Ground one gap outside the edge is still at the old height.
        assert!(p.elevation_at(DVec2::new(22.99, 40.0)).unwrap().abs() < 1e-6);
        assert!((p.elevation_at(DVec2::new(23.01, 40.0)).unwrap() - 7.0).abs() < 1e-6);
        let r = cut_fill(&existing, &p);
        let expect = 38.0 * 38.0 * 7.0;
        assert!((r.fill - expect).abs() < expect * 1e-5, "{r:?} vs {expect}");
        assert!(r.cut.abs() < 1e-6);
        // A pad that touches the edge of the ground has no outside points there, which is fine.
        let edge = Modifier::Pad { boundary: square(0.0, 30.0), elevation: 2.0, slope: None };
        let p = proposed_surface(&src, &existing, &[edge]).unwrap();
        assert!((cut_fill(&existing, &p).fill - 900.0 * 2.0).abs() < 1.0);
    }

    #[test]
    fn offset_moves_corners_outward_for_both_windings() {
        let sq = square(0.0, 10.0);
        let out = offset_outward(&sq, 1.0);
        assert!(out[0].x < 0.0 && out[0].y < 0.0 && out[2].x > 10.0 && out[2].y > 10.0);
        assert!((out[0] - DVec2::new(-1.0, -1.0)).length() < 1e-9 || out[0].x.abs() < 2.0);
        let rev: Vec<DVec2> = sq.iter().rev().copied().collect();
        let out = offset_outward(&rev, 1.0);
        assert!(out.iter().zip(&rev).all(|(o, p)| (o.x - 5.0).abs() > (p.x - 5.0).abs() - 1e-12));
        assert!(out.iter().zip(&rev).all(|(o, p)| (o.y - 5.0).abs() > (p.y - 5.0).abs() - 1e-12));
    }

    #[test]
    fn pad_outside_the_ground_is_ignored_in_volumes() {
        let src = flat_grid(3, 10.0, 0.0, 0.0);
        let existing = Tin::build(&src).unwrap();
        let mods = vec![Modifier::Pad { boundary: square(100.0, 110.0), elevation: 5.0, slope: None }];
        let p = proposed_surface(&src, &existing, &mods).unwrap();
        let r = cut_fill(&existing, &p);
        assert!(r.area <= existing.area_xy() + 1e-9);
    }
}
