//! Generated 2D plant outlines. A plan view draws these instead of a plain circle.
//! The outlines are plain point lists, so the SVG writer and the app canvas share them.

use std::f64::consts::TAU;

use glam::DVec2;

use super::style::PlantGraphic;

/// One stroke of a plant symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphicPath {
    pub points: Vec<DVec2>,
    pub closed: bool,
}

const SAMPLES: usize = 96;

/// A closed outline with radius `f(angle)`, sampled evenly.
fn polar(center: DVec2, f: impl Fn(f64) -> f64) -> GraphicPath {
    let points = (0..SAMPLES)
        .map(|k| {
            let a = TAU * k as f64 / SAMPLES as f64;
            center + DVec2::from_angle(a) * f(a)
        })
        .collect();
    GraphicPath { points, closed: true }
}

/// The strokes of a plant symbol with this outline, centered at `center`, within `radius`.
/// The first path is always the closed outline.
pub fn paths(graphic: PlantGraphic, center: DVec2, radius: f64) -> Vec<GraphicPath> {
    let r = radius.max(0.0);
    match graphic {
        PlantGraphic::Circle => vec![polar(center, |_| r)],
        PlantGraphic::Cloud => {
            // Ten bumps with sharp notches between them.
            vec![polar(center, |a| r * (0.86 + 0.14 * (5.0 * a).sin().abs()))]
        }
        PlantGraphic::Star => {
            let points = (0..16)
                .map(|k| {
                    let a = TAU * k as f64 / 16.0;
                    center + DVec2::from_angle(a) * if k % 2 == 0 { r } else { r * 0.45 }
                })
                .collect();
            vec![GraphicPath { points, closed: true }]
        }
        PlantGraphic::Spokes => {
            let mut out = vec![polar(center, |_| r)];
            for k in 0..8 {
                let d = DVec2::from_angle(TAU * k as f64 / 8.0);
                out.push(GraphicPath { points: vec![center, center + d * r], closed: false });
            }
            out
        }
        PlantGraphic::Rosette => vec![polar(center, |a| r * (0.45 + 0.55 * (4.0 * a).cos().abs()))],
        PlantGraphic::Clover => vec![polar(center, |a| r * (0.4 + 0.6 * (1.5 * a).cos().abs()))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outline_stays_within_the_radius_and_is_finite() {
        let c = DVec2::new(100.0, -50.0);
        for g in PlantGraphic::ALL {
            let p = paths(g, c, 2000.0);
            assert!(!p.is_empty());
            assert!(p[0].closed && p[0].points.len() >= 16, "{g:?}");
            for path in &p {
                for pt in &path.points {
                    assert!(pt.is_finite());
                    assert!(pt.distance(c) <= 2000.0 + 1e-6, "{g:?} {pt:?}");
                }
            }
        }
    }

    #[test]
    fn outlines_reach_the_radius() {
        for g in PlantGraphic::ALL {
            let p = paths(g, DVec2::ZERO, 1000.0);
            let far = p[0].points.iter().map(|q| q.length()).fold(0.0, f64::max);
            assert!(far > 990.0, "{g:?} reaches {far}");
        }
    }

    #[test]
    fn spokes_add_eight_strokes_and_circle_is_round() {
        assert_eq!(paths(PlantGraphic::Spokes, DVec2::ZERO, 10.0).len(), 9);
        assert_eq!(paths(PlantGraphic::Circle, DVec2::ZERO, 10.0).len(), 1);
        for q in &paths(PlantGraphic::Circle, DVec2::ZERO, 10.0)[0].points {
            assert!((q.length() - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn negative_radius_is_treated_as_zero() {
        let p = paths(PlantGraphic::Star, DVec2::ZERO, -5.0);
        assert!(p[0].points.iter().all(|q| *q == DVec2::ZERO));
    }
}
