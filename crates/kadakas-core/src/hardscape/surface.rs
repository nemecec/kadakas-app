//! The top surface of a hardscape: a plane for the slope, or the ground of a site model, and
//! the 3D solid with its components.

use std::collections::HashMap;

use glam::{DVec2, DVec3};

use super::data::{HardscapeData, Mode, Slope};
use crate::mesh::{SceneSink, clean_ring, triangulate};
use crate::model::{Object, Shape};
use crate::present::PART_LAYER;

/// Ground height at a point, or `None` outside the terrain. Same frame as the layer.
pub type Terrain<'a> = &'a dyn Fn(DVec2) -> Option<f64>;

/// How far a draped surface lies above the ground, in mm. This avoids flicker where the surface
/// and the terrain meet in 3D.
pub const LIFT: f64 = 5.0;
/// Largest edge of a triangle on a draped surface, in mm.
pub const DRAPE_STEP: f64 = 500.0;
/// Most triangles of one draped surface.
pub const MAX_TRIANGLES: f64 = 20_000.0;

/// A plane `z = a x + b y + c`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

impl Plane {
    pub fn flat(z: f64) -> Self {
        Plane {
            a: 0.0,
            b: 0.0,
            c: z,
        }
    }

    pub fn z(&self, p: DVec2) -> f64 {
        self.a * p.x + self.b * p.y + self.c
    }

    /// Area of the plane over a unit of plan area.
    pub fn area_factor(&self) -> f64 {
        (1.0 + self.a * self.a + self.b * self.b).sqrt()
    }

    /// The plane with height `z` at `at` and a fall of `percent` toward `dir` (a unit vector).
    fn falling(z: f64, at: DVec2, dir: DVec2, percent: f64) -> Self {
        let (a, b) = (-percent / 100.0 * dir.x, -percent / 100.0 * dir.y);
        Plane {
            a,
            b,
            c: z - a * at.x - b * at.y,
        }
    }

    /// The least squares plane through points with heights. A flat plane at the average height if
    /// the points are on one line.
    pub fn fit(points: &[DVec3]) -> Self {
        let n = points.len() as f64;
        let mean = points.iter().copied().sum::<DVec3>() / n.max(1.0);
        let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for p in points {
            let d = *p - mean;
            sxx += d.x * d.x;
            sxy += d.x * d.y;
            syy += d.y * d.y;
            sxz += d.x * d.z;
            syz += d.y * d.z;
        }
        let det = sxx * syy - sxy * sxy;
        if det.abs() <= 1e-12 * (sxx * syy).abs().max(1e-300) {
            return Plane::flat(mean.z);
        }
        let a = (sxz * syy - syz * sxy) / det;
        let b = (syz * sxx - sxz * sxy) / det;
        Plane {
            a,
            b,
            c: mean.z - a * mean.x - b * mean.y,
        }
    }
}

/// The height of the top surface at any point.
#[derive(Clone, Copy)]
pub struct Surface<'a> {
    pub plane: Plane,
    /// Height above the ground when draped.
    pub offset: f64,
    pub terrain: Option<Terrain<'a>>,
}

impl Surface<'_> {
    /// Height of the top surface at `p`. Where the terrain has no height, the plane is used.
    pub fn z(&self, p: DVec2) -> f64 {
        match self.terrain.and_then(|t| t(p)) {
            Some(g) => g + self.offset,
            None => self.plane.z(p),
        }
    }

    pub fn is_draped(&self) -> bool {
        self.terrain.is_some()
    }
}

/// The surface of a hardscape object. The terrain counts only if the hardscape drapes.
pub fn surface_of<'a>(
    obj: &Object,
    data: Option<&HardscapeData>,
    terrain: Option<Terrain<'a>>,
) -> Surface<'a> {
    let Shape::Hardscape {
        boundary,
        elevation,
        ..
    } = &obj.shape
    else {
        return Surface {
            plane: Plane::flat(0.0),
            offset: 0.0,
            terrain: None,
        };
    };
    let elevation = elevation.filter(|e| e.is_finite()).unwrap_or(0.0);
    let ring: Vec<DVec2> = crate::curve::positions(boundary);
    let mid = if ring.is_empty() {
        DVec2::ZERO
    } else {
        ring.iter().copied().sum::<DVec2>() / ring.len() as f64
    };
    let mut plane = Plane::flat(elevation);
    if let Some(d) = data {
        let center = d.center_points();
        let chord = (center.len() >= 2).then(|| (center[0], center[center.len() - 1]));
        match (&d.slope, d.mode) {
            (s, Mode::Path) if s.fall().is_some() && chord.is_some() && !d.closed => {
                let (percent, _) = s.fall().expect("falls");
                let (s0, e) = chord.expect("checked");
                let dir = (e - s0).normalize_or_zero();
                // To the right of the direction of travel.
                plane = Plane::falling(
                    elevation,
                    (s0 + e) / 2.0,
                    DVec2::new(dir.y, -dir.x),
                    percent,
                );
            }
            (s, _) if s.fall().is_some() => {
                let (percent, angle_deg) = s.fall().expect("falls");
                plane = Plane::falling(
                    elevation,
                    mid,
                    DVec2::from_angle(angle_deg.to_radians()),
                    percent,
                );
            }
            (Slope::Edges { elevations }, Mode::Slab) if elevations.len() == ring.len() => {
                let pts: Vec<DVec3> = ring
                    .iter()
                    .zip(elevations)
                    .map(|(p, z)| p.extend(*z))
                    .collect();
                plane = Plane::fit(&pts);
            }
            (Slope::Edges { elevations }, Mode::Path) if elevations.len() >= 2 => {
                if let Some((s, e)) = chord {
                    // Heights at the two ends: a plane that rises along the chord and is level across it.
                    let len = s.distance(e);
                    let dir = (e - s) / len;
                    let g = (elevations[1] - elevations[0]) / len;
                    plane = Plane {
                        a: g * dir.x,
                        b: g * dir.y,
                        c: elevations[0] - g * dir.dot(s),
                    };
                }
            }
            _ => {}
        }
    }
    let drape = data.is_some_and(|d| d.drape);
    Surface {
        plane,
        offset: elevation + LIFT,
        terrain: if drape { terrain } else { None },
    }
}

/// Points and triangles of the top surface over a ring. Every point has the height of the
/// surface at its position. With `step`, triangles are split until no edge is longer than it.
pub fn top_surface(
    ring: &[DVec2],
    z: &dyn Fn(DVec2) -> f64,
    step: Option<f64>,
) -> (Vec<DVec3>, Vec<u32>) {
    let ring = clean_ring(ring);
    let tris = triangulate(&ring);
    let area = crate::geom::polygon_area(&ring);
    let step = step.map(|s| s.max((area / (MAX_TRIANGLES / 2.5)).sqrt()));
    let mut points: Vec<DVec3> = Vec::new();
    let mut index: HashMap<(u64, u64), u32> = HashMap::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut add = |p: DVec2, points: &mut Vec<DVec3>| -> u32 {
        let key = ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits());
        *index.entry(key).or_insert_with(|| {
            points.push(p.extend(z(p)));
            (points.len() - 1) as u32
        })
    };
    for [i, j, k] in tris {
        let mut stack = vec![([ring[i], ring[j], ring[k]], 0u8)];
        while let Some((t, depth)) = stack.pop() {
            let edges = [
                t[0].distance(t[1]),
                t[1].distance(t[2]),
                t[2].distance(t[0]),
            ];
            let (longest, len) = edges
                .iter()
                .enumerate()
                .fold((0, 0.0), |m, (i, l)| if *l > m.1 { (i, *l) } else { m });
            match step {
                Some(s) if len > s && depth < 24 => {
                    let (a, b, c) = (t[longest], t[(longest + 1) % 3], t[(longest + 2) % 3]);
                    let m = (a + b) / 2.0;
                    stack.push(([a, m, c], depth + 1));
                    stack.push(([m, b, c], depth + 1));
                }
                _ => {
                    for p in t {
                        indices.push(add(p, &mut points));
                    }
                }
            }
        }
    }
    (points, indices)
}

/// Area of the triangles in 3D, in square model units.
pub fn mesh_area(points: &[DVec3], indices: &[u32]) -> f64 {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let (a, b, c) = (
                points[t[0] as usize],
                points[t[1] as usize],
                points[t[2] as usize],
            );
            (b - a).cross(c - a).length() / 2.0
        })
        .sum()
}

/// Area of the triangles seen from above, in square model units.
pub fn plan_mesh_area(points: &[DVec3], indices: &[u32]) -> f64 {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let (a, b, c) = (
                points[t[0] as usize].truncate(),
                points[t[1] as usize].truncate(),
                points[t[2] as usize].truncate(),
            );
            (b - a).perp_dot(c - a).abs() / 2.0
        })
        .sum()
}

/// Ratio of the surface area to the plan area, over a ring.
pub fn area_ratio(ring: &[DVec2], surface: &Surface<'_>) -> f64 {
    if !surface.is_draped() {
        return surface.plane.area_factor();
    }
    let z = |p| surface.z(p);
    let (points, indices) = top_surface(ring, &z, Some(DRAPE_STEP));
    let plan = plan_mesh_area(&points, &indices);
    if plan > 0.0 {
        mesh_area(&points, &indices) / plan
    } else {
        1.0
    }
}

/// One layer of the solid: depths below the top surface, and a color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerSpec {
    pub top: f64,
    pub bottom: f64,
    pub color: [u8; 4],
}

/// Multiplies the color channels.
pub fn shade(c: [u8; 4], k: f32) -> [u8; 4] {
    [
        (c[0] as f32 * k) as u8,
        (c[1] as f32 * k) as u8,
        (c[2] as f32 * k) as u8,
        c[3],
    ]
}

/// Adds the top surface, the walls of every layer, and the outlines of a solid to a sink. The
/// holes are open: the surface has no triangles over them, and the walls close them.
pub fn emit_solid(
    sink: &mut SceneSink<'_>,
    ring: &[DVec2],
    holes: &[Vec<DVec2>],
    surface: &Surface<'_>,
    layers: &[LayerSpec],
    edge: [u8; 4],
) {
    let ring = clean_ring(ring);
    if ring.len() < 3 || layers.is_empty() {
        return;
    }
    let holes: Vec<Vec<DVec2>> = holes
        .iter()
        .map(|h| clean_ring(h))
        .filter(|h| h.len() >= 3)
        .collect();
    let z = |p| surface.z(p);
    let step = surface.is_draped().then_some(DRAPE_STEP);
    let (mut points, mut indices) = (Vec::new(), Vec::new());
    for piece in crate::geom::split_with_holes(&ring, &holes) {
        let (p, i) = top_surface(&piece, &z, step);
        let off = points.len() as u32;
        points.extend(p);
        indices.extend(i.iter().map(|k| k + off));
    }
    sink.set_part(PART_LAYER);
    sink.mesh(&points, &indices, layers[0].color);
    // Points along the outline and the holes (the holes run clockwise, so the outside of the
    // solid is on the right of every edge). A draped outline gets extra points so it follows the ground.
    let mut loops: Vec<Vec<DVec2>> = vec![ring.clone()];
    loops.extend(holes.iter().map(|h| h.iter().rev().copied().collect()));
    let outlines: Vec<Vec<DVec2>> = loops
        .iter()
        .map(|lp| {
            let n = lp.len();
            let mut outline: Vec<DVec2> = Vec::new();
            for i in 0..n {
                let (p, q) = (lp[i], lp[(i + 1) % n]);
                let parts = step.map_or(1, |s| ((p.distance(q) / s).ceil() as usize).clamp(1, 400));
                for k in 0..parts {
                    outline.push(p.lerp(q, k as f64 / parts as f64));
                }
            }
            outline
        })
        .collect();
    for (li, layer) in layers.iter().enumerate() {
        sink.set_part(PART_LAYER + li.min(200) as u8);
        let (mut pos, mut idx) = (Vec::new(), Vec::new());
        for outline in &outlines {
            let m = outline.len();
            for i in 0..m {
                let (p, q) = (outline[i], outline[(i + 1) % m]);
                let (zp, zq) = (z(p), z(q));
                let base = pos.len() as u32;
                pos.extend([
                    p.extend(zp - layer.bottom),
                    q.extend(zq - layer.bottom),
                    q.extend(zq - layer.top),
                    p.extend(zp - layer.top),
                ]);
                idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
        sink.mesh(&pos, &idx, shade(layer.color, 0.82));
    }
    sink.set_part(0);
    let depth = layers.iter().map(|l| l.bottom).fold(0.0, f64::max);
    for outline in &outlines {
        let top: Vec<DVec3> = outline.iter().map(|p| p.extend(z(*p))).collect();
        sink.polyline(&top, true, edge);
        let bottom: Vec<DVec3> = outline.iter().map(|p| p.extend(z(*p) - depth)).collect();
        sink.polyline(&bottom, true, edge);
    }
    for lp in &loops {
        let n = lp.len();
        for i in 0..n {
            let (a, b, c) = (lp[(i + n - 1) % n], lp[i], lp[(i + 1) % n]);
            let (u, v) = ((b - a).normalize_or_zero(), (c - b).normalize_or_zero());
            if u.dot(v) < 0.94 {
                sink.polyline(&[b.extend(z(b) - depth), b.extend(z(b))], false, edge);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve;

    fn hardscape(pts: &[DVec2], elevation: Option<f64>) -> Object {
        Object::new(
            crate::model::ObjectId::from_num(1),
            crate::model::LayerId::from_num(1),
            crate::model::ClassId::from_num(0),
            Shape::Hardscape {
                boundary: curve::corners(pts),
                material: "m".into(),
                thickness: None,
                elevation,
            },
        )
    }

    fn square(s: f64) -> Vec<DVec2> {
        vec![
            DVec2::ZERO,
            DVec2::new(s, 0.0),
            DVec2::new(s, s),
            DVec2::new(0.0, s),
        ]
    }

    #[test]
    fn plane_fit_is_exact_for_a_plane() {
        let f = |x: f64, y: f64| 100.0 + 0.02 * x - 0.01 * y;
        let pts: Vec<DVec3> = [
            (0.0, 0.0),
            (1000.0, 0.0),
            (1000.0, 500.0),
            (0.0, 500.0),
            (400.0, 200.0),
        ]
        .iter()
        .map(|(x, y)| DVec3::new(*x, *y, f(*x, *y)))
        .collect();
        let p = Plane::fit(&pts);
        assert!((p.a - 0.02).abs() < 1e-12 && (p.b + 0.01).abs() < 1e-12);
        assert!((p.z(DVec2::new(123.0, 77.0)) - f(123.0, 77.0)).abs() < 1e-9);
        assert!((p.area_factor() - (1.0f64 + 0.0004 + 0.0001).sqrt()).abs() < 1e-12);
        // Points on a line give a flat plane at the mean.
        let line = [
            DVec3::new(0.0, 0.0, 1.0),
            DVec3::new(1.0, 1.0, 3.0),
            DVec3::new(2.0, 2.0, 5.0),
        ];
        assert_eq!(Plane::fit(&line), Plane::flat(3.0));
        assert_eq!(Plane::fit(&[]), Plane::flat(0.0));
    }

    #[test]
    fn slopes_make_planes() {
        let mut o = hardscape(&square(2000.0), Some(100.0));
        let flat = surface_of(&o, None, None);
        assert_eq!(flat.z(DVec2::new(5.0, 5.0)), 100.0);
        let mut d = HardscapeData {
            slope: Slope::Cross {
                percent: 2.0,
                angle_deg: 0.0,
            },
            ..Default::default()
        };
        let s = surface_of(&o, Some(&d), None);
        // Level at the middle, falling 2 percent toward +X.
        assert!((s.z(DVec2::new(1000.0, 500.0)) - 100.0).abs() < 1e-9);
        assert!((s.z(DVec2::new(2000.0, 500.0)) - 80.0).abs() < 1e-9);
        d.slope = Slope::Edges {
            elevations: vec![0.0, 40.0, 40.0, 0.0],
        };
        let s = surface_of(&o, Some(&d), None);
        assert!(
            (s.z(DVec2::new(0.0, 0.0))).abs() < 1e-9
                && (s.z(DVec2::new(2000.0, 2000.0)) - 40.0).abs() < 1e-9
        );
        // A wrong number of elevations is ignored.
        d.slope = Slope::Edges {
            elevations: vec![0.0, 40.0],
        };
        assert_eq!(surface_of(&o, Some(&d), None).z(DVec2::ZERO), 100.0);
        // Paths.
        d = HardscapeData {
            mode: Mode::Path,
            center: curve::corners(&[DVec2::ZERO, DVec2::new(1000.0, 0.0)]),
            width: 400.0,
            slope: Slope::Cross {
                percent: 10.0,
                angle_deg: 77.0,
            },
            ..Default::default()
        };
        let s = surface_of(&o, Some(&d), None);
        assert!(
            (s.z(DVec2::new(500.0, -200.0)) - 80.0).abs() < 1e-9,
            "falls to the right (negative y) of +X travel"
        );
        assert!((s.z(DVec2::new(900.0, 0.0)) - 100.0).abs() < 1e-9);
        d.slope = Slope::Edges {
            elevations: vec![0.0, 50.0],
        };
        let s = surface_of(&o, Some(&d), None);
        assert!(
            (s.z(DVec2::new(0.0, 300.0))).abs() < 1e-9
                && (s.z(DVec2::new(1000.0, -300.0)) - 50.0).abs() < 1e-9
        );
        // Not a hardscape.
        o.shape = Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        };
        assert_eq!(surface_of(&o, None, None).z(DVec2::ONE), 0.0);
    }

    #[test]
    fn terrain_counts_only_when_draped() {
        let o = hardscape(&square(1000.0), Some(20.0));
        let ground = |p: DVec2| (p.x < 900.0).then_some(p.x / 10.0);
        let mut d = HardscapeData::default();
        assert!(!surface_of(&o, Some(&d), Some(&ground)).is_draped());
        d.drape = true;
        let s = surface_of(&o, Some(&d), Some(&ground));
        assert!(s.is_draped());
        assert_eq!(
            s.z(DVec2::new(100.0, 0.0)),
            10.0 + 20.0 + LIFT,
            "ground, the height above it, and the lift"
        );
        // Outside the terrain the plane is used.
        assert_eq!(s.z(DVec2::new(950.0, 0.0)), 20.0);
        assert!(!surface_of(&o, Some(&d), None).is_draped());
    }

    #[test]
    fn top_surface_covers_the_ring_and_samples_the_height() {
        let z = |p: DVec2| p.x * 0.1 + (p.y * 0.01).sin() * 10.0;
        let (pts, idx) = top_surface(&square(3000.0), &z, Some(500.0));
        assert!(
            pts.iter().all(|p| (p.z - z(p.truncate())).abs() == 0.0),
            "every point is on the surface"
        );
        assert!((plan_mesh_area(&pts, &idx) - 9.0e6).abs() < 1.0);
        assert!(mesh_area(&pts, &idx) > 9.0e6);
        // Edges are no longer than the step.
        for t in idx.as_chunks::<3>().0 {
            for k in 0..3 {
                let (a, b) = (
                    pts[t[k] as usize].truncate(),
                    pts[t[(k + 1) % 3] as usize].truncate(),
                );
                assert!(a.distance(b) <= 500.0 + 1e-6);
            }
        }
        // Without a step: two triangles for a square.
        let (pts, idx) = top_surface(&square(3000.0), &z, None);
        assert_eq!((pts.len(), idx.len()), (4, 6));
        // A huge area is limited.
        let (_, idx) = top_surface(&square(1.0e6), &z, Some(500.0));
        assert!(idx.len() / 3 < 40_000);
        assert!(top_surface(&[], &z, None).1.is_empty());
    }

    #[test]
    fn area_ratio_of_planes_and_terrain() {
        let o = hardscape(&square(1000.0), None);
        let d = HardscapeData {
            slope: Slope::Cross {
                percent: 100.0,
                angle_deg: 0.0,
            },
            ..Default::default()
        };
        let s = surface_of(&o, Some(&d), None);
        assert!((area_ratio(&square(1000.0), &s) - 2.0f64.sqrt()).abs() < 1e-12);
        let ground = |p: DVec2| Some(p.x);
        let d = HardscapeData {
            drape: true,
            ..Default::default()
        };
        let s = surface_of(&o, Some(&d), Some(&ground));
        assert!((area_ratio(&square(1000.0), &s) - 2.0f64.sqrt()).abs() < 1e-9);
        let degenerate = [DVec2::ZERO, DVec2::X, DVec2::new(2.0, 0.0)];
        assert_eq!(area_ratio(&degenerate, &s), 1.0);
    }
}
