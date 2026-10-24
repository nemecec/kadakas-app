//! Entities in a tilted plane (IOP-15, research gap 6).
//!
//! The entities LWPOLYLINE, POLYLINE, CIRCLE, ARC and SOLID lie in their own plane, which the
//! extrusion direction (group codes 210, 220 and 230) gives. DXF finds the axes of that plane
//! with the arbitrary axis algorithm. A plan has no shape for a circle in a tilted plane, so these
//! entities become 3D polylines, cut within the tolerance. A hatch in a tilted plane keeps its
//! boundary only (a fill cannot lie in a tilted plane here). Text and blocks in a tilted plane are in
//! `tilt.rs`.

use kadakas_core::{DVec2, DVec3};

use super::{Entity, arc_steps, bulge_points, ellipse_points};

/// The plane of an entity: horizontal (with a mirror when the extrusion direction is down), or tilted.
#[derive(Clone, Copy, Debug)]
pub(super) enum Plane {
    Flat { flip: bool },
    Tilted(Ocs),
}

impl Plane {
    /// Reads the extrusion direction (210, 220, 230) of the entity.
    pub fn of(e: &Entity) -> Plane {
        let n = e.normal();
        match Ocs::of(n) {
            Some(ocs) => Plane::Tilted(ocs),
            None => Plane::Flat { flip: n[2] < 0.0 },
        }
    }
}

/// The axes of the object coordinate system (OCS) of an extrusion direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Ocs {
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
}

impl Ocs {
    /// The axes for a direction. `None` when the direction is (almost) straight up or down, which
    /// the walker handles with a mirror, or when it has no length.
    pub fn of(n: [f64; 3]) -> Option<Ocs> {
        let n = DVec3::from(n);
        if !n.is_finite() || n.length_squared() < 1e-24 {
            return None;
        }
        let z = n.normalize();
        // The arbitrary axis algorithm of the DXF reference.
        let flat = z.x.abs() < 1.0 / 64.0 && z.y.abs() < 1.0 / 64.0;
        if flat && z.x.abs() < 1e-9 && z.y.abs() < 1e-9 {
            return None;
        }
        let x = if flat {
            DVec3::Y.cross(z)
        } else {
            DVec3::Z.cross(z)
        }
        .normalize();
        let y = z.cross(x).normalize();
        Some(Ocs { x, y, z })
    }

    /// World point of an OCS point.
    pub fn world(&self, p: DVec3) -> DVec3 {
        self.x * p.x + self.y * p.y + self.z * p.z
    }
}

/// The points of a polyline given in OCS (corners with bulges) at `elevation`, as world points.
/// A bulge becomes points on the arc, within `tol` (in drawing units).
pub(super) fn polyline_points(
    ocs: &Ocs,
    pts: &[(DVec2, f64)],
    closed: bool,
    elevation: f64,
    tol: f64,
) -> Vec<DVec3> {
    let n = pts.len();
    let mut out: Vec<DVec3> = Vec::with_capacity(n);
    let lift = |p: DVec2| ocs.world(p.extend(elevation));
    for i in 0..n {
        out.push(lift(pts[i].0));
        let bulge = pts[i].1;
        if bulge.abs() <= 1e-9 || (i + 1 == n && !closed) {
            continue;
        }
        let (a, b) = (pts[i].0, pts[(i + 1) % n].0);
        if a.distance(b) <= 1e-12 {
            continue;
        }
        let seg = bulge_points(a, b, bulge, tol);
        out.extend(seg[1..seg.len() - 1].iter().map(|p| lift(*p)));
    }
    out
}

/// A circle (center in OCS) as a closed ring of world points.
pub(super) fn circle_points(ocs: &Ocs, center: DVec3, radius: f64, tol: f64) -> Vec<DVec3> {
    let mut ring = ellipse_points(
        center.truncate(),
        DVec2::new(radius, 0.0),
        1.0,
        0.0,
        std::f64::consts::TAU,
        tol,
    );
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    ring.into_iter()
        .map(|p| ocs.world(p.extend(center.z)))
        .collect()
}

/// An arc (center in OCS, angles in radians) as world points from start to end.
pub(super) fn arc_points(
    ocs: &Ocs,
    center: DVec3,
    radius: f64,
    a0: f64,
    sweep: f64,
    tol: f64,
) -> Vec<DVec3> {
    let n = arc_steps(radius, sweep, tol);
    (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            ocs.world((center.truncate() + DVec2::from_angle(a) * radius).extend(center.z))
        })
        .collect()
}

/// An ellipse (an ELLIPSE entity: center and major axis in world coordinates, the minor axis in
/// the plane of the extrusion direction) as world points. The parameters are in radians.
pub(super) fn ellipse_world(
    center: DVec3,
    major: DVec3,
    normal: DVec3,
    ratio: f64,
    t0: f64,
    sweep: f64,
    tol: f64,
) -> Vec<DVec3> {
    let n = normal.normalize_or(DVec3::Z);
    let minor = n.cross(major) * ratio;
    let radius = major.length().max(minor.length());
    let steps = arc_steps(radius, sweep, tol).max(8);
    (0..=steps)
        .map(|i| {
            let t = t0 + sweep * i as f64 / steps as f64;
            center + major * t.cos() + minor * t.sin()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn straight_up_or_down_has_no_axes() {
        assert!(Ocs::of([0.0, 0.0, 1.0]).is_none());
        assert!(Ocs::of([0.0, 0.0, -1.0]).is_none());
        assert!(Ocs::of([0.0, 0.0, 0.0]).is_none());
        assert!(Ocs::of([f64::NAN, 0.0, 1.0]).is_none());
    }

    #[test]
    fn the_arbitrary_axis_algorithm_matches_the_reference() {
        // The OCS of the normal (1, 0, 0) is: X = (0, 1, 0), Y = (0, 0, 1) (the reference
        // gives Ax = Wz x N when N is not close to Z, so X = (0,0,1) x (1,0,0) = (0,1,0)).
        let o = Ocs::of([1.0, 0.0, 0.0]).unwrap();
        assert!(close(o.x, DVec3::Y), "{:?}", o.x);
        assert!(close(o.y, DVec3::Z), "{:?}", o.y);
        assert!(close(o.z, DVec3::X));
        // A direction near Z uses Wy x N.
        let o = Ocs::of([0.001, 0.0, 1.0]).unwrap();
        let wy_n = DVec3::Y
            .cross(DVec3::new(0.001, 0.0, 1.0).normalize())
            .normalize();
        assert!(close(o.x, wy_n));
        // The axes are a right-handed unit system.
        for n in [
            [1.0, 2.0, 3.0],
            [-3.0, 0.5, 0.2],
            [0.0, 1.0, 0.0],
            [0.01, -0.01, -1.0],
        ] {
            let o = Ocs::of(n).unwrap();
            assert!((o.x.length() - 1.0).abs() < 1e-9 && (o.y.length() - 1.0).abs() < 1e-9);
            assert!(o.x.dot(o.y).abs() < 1e-9 && o.x.dot(o.z).abs() < 1e-9);
            assert!(close(o.x.cross(o.y), o.z));
        }
    }

    #[test]
    fn a_circle_in_a_vertical_plane_stands_up() {
        // Normal along -Y: the plane is the XZ plane. A circle at OCS (0,0,0) with radius 10.
        let o = Ocs::of([0.0, -1.0, 0.0]).unwrap();
        let ring = circle_points(&o, DVec3::ZERO, 10.0, 0.1);
        assert!(ring.len() >= 16);
        for p in &ring {
            assert!(p.y.abs() < 1e-9);
            assert!((p.x * p.x + p.z * p.z).sqrt() - 10.0 < 1e-9);
        }
        assert!(ring.iter().any(|p| p.z > 9.9));
    }

    #[test]
    fn a_polyline_with_a_bulge_is_cut_in_its_plane() {
        let o = Ocs::of([1.0, 0.0, 0.0]).unwrap();
        // OCS x is world Y, OCS y is world Z, OCS z is world X (the elevation).
        let pts = [(DVec2::new(0.0, 0.0), 1.0), (DVec2::new(10.0, 0.0), 0.0)];
        let out = polyline_points(&o, &pts, false, 5.0, 0.05);
        assert!(out.len() > 4);
        assert!(out.iter().all(|p| (p.x - 5.0).abs() < 1e-9));
        assert!(close(out[0], DVec3::new(5.0, 0.0, 0.0)));
        assert!(close(*out.last().unwrap(), DVec3::new(5.0, 10.0, 0.0)));
        // A half circle with the chord on the world Y axis bulges to world Z.
        assert!(out.iter().any(|p| p.z.abs() > 4.9));
    }

    #[test]
    fn an_arc_runs_from_its_start_to_its_end() {
        let o = Ocs::of([1.0, 0.0, 0.0]).unwrap();
        let out = arc_points(
            &o,
            DVec3::ZERO,
            10.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            0.05,
        );
        assert!(close(out[0], o.world(DVec3::new(10.0, 0.0, 0.0))));
        assert!(close(
            *out.last().unwrap(),
            o.world(DVec3::new(0.0, 10.0, 0.0))
        ));
    }

    #[test]
    fn an_ellipse_lies_in_the_plane_of_its_normal() {
        let pts = ellipse_world(
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(0.0, -1.0, 0.0),
            0.5,
            0.0,
            std::f64::consts::TAU,
            0.1,
        );
        assert!(pts.iter().all(|p| (p.y - 2.0).abs() < 1e-9));
        assert!(close(pts[0], DVec3::new(11.0, 2.0, 3.0)));
        let top = pts.iter().map(|p| p.z).fold(f64::MIN, f64::max);
        assert!((top - 8.0).abs() < 0.2, "{top}");
    }
}
