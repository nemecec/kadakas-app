//! The orbit camera: view and projection matrices, fit, orbit, pan, zoom, rays.
//!
//! World axes: X east, Y north, Z up. Screen: X right, Y up.
//! The camera orbits a target. `yaw` is the angle of the eye around Z, measured from
//! +X toward +Y. `pitch` is the angle of the eye above the XY plane.

use glam::{Mat4, Vec2, Vec3, Vec4};
use kadakas_core::mesh::Bounds3;

use crate::views::{Projection, StandardView};

/// Vertical field of view of the perspective camera. The orthographic camera shows
/// the same height at the target distance, so switching projection keeps the view size.
pub const FOV_Y: f32 = std::f32::consts::FRAC_PI_4;

/// The box, unless it has a number that is not finite.
fn finite_bounds(bounds: Option<Bounds3>) -> Option<Bounds3> {
    bounds.filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
}

/// View matrix for a right-handed camera. `up` need not be exactly perpendicular.
fn look_to_rh(eye: Vec3, forward: Vec3, up: Vec3) -> Mat4 {
    let f = forward.normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols(
        Vec4::new(s.x, u.x, -f.x, 0.0),
        Vec4::new(s.y, u.y, -f.y, 0.0),
        Vec4::new(s.z, u.z, -f.z, 0.0),
        Vec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0),
    )
}

/// Perspective matrix with depth 0..1 (near to far).
fn perspective_rh(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y / 2.0).tan();
    Mat4::from_cols(
        Vec4::new(f / aspect, 0.0, 0.0, 0.0),
        Vec4::new(0.0, f, 0.0, 0.0),
        Vec4::new(0.0, 0.0, far / (near - far), -1.0),
        Vec4::new(0.0, 0.0, near * far / (near - far), 0.0),
    )
}

/// Orthographic matrix for a box `-half_w..half_w` by `-half_h..half_h`, depth 0..1.
fn orthographic_rh(half_w: f32, half_h: f32, near: f32, far: f32) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(1.0 / half_w, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0 / half_h, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0 / (near - far), 0.0),
        Vec4::new(0.0, 0.0, near / (near - far), 1.0),
    )
}

const MIN_DISTANCE: f32 = 1.0;
const MAX_DISTANCE: f32 = 1.0e9;

/// An orbit camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Distance from the eye to the target. For the orthographic camera it sets the zoom.
    pub distance: f32,
    pub projection: Projection,
}

impl Default for Camera {
    fn default() -> Self {
        Camera::standard(StandardView::RightIsometric, Projection::Orthographic)
    }
}

impl Camera {
    pub fn standard(view: StandardView, projection: Projection) -> Camera {
        let (yaw, pitch) = view.yaw_pitch();
        Camera {
            target: Vec3::ZERO,
            yaw,
            pitch,
            distance: 10_000.0,
            projection,
        }
    }

    /// Looks from a standard view, keeping target and distance.
    pub fn set_view(&mut self, view: StandardView) {
        (self.yaw, self.pitch) = view.yaw_pitch();
    }

    /// Unit vector from the target to the eye.
    pub fn eye_dir(&self) -> Vec3 {
        Vec3::new(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        )
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.eye_dir() * self.distance
    }

    /// Direction that the camera looks in.
    pub fn forward(&self) -> Vec3 {
        -self.eye_dir()
    }

    /// Screen right. It does not depend on the pitch, so the top and bottom views work.
    pub fn right(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), self.yaw.cos(), 0.0)
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward())
    }

    /// Half the visible height at the target, in mm.
    pub fn half_height(&self) -> f32 {
        self.distance * (FOV_Y / 2.0).tan()
    }

    /// Near and far plane distances along the view direction that include the box.
    pub fn clip_range(&self, bounds: Option<Bounds3>) -> (f32, f32) {
        let bounds = finite_bounds(bounds);
        let (center, radius) = match bounds {
            Some((lo, hi)) => ((lo + hi) / 2.0, (hi - lo).length() / 2.0),
            None => (self.target, self.distance),
        };
        let depth = (center - self.eye()).dot(self.forward());
        let radius = radius * 1.02 + 1.0;
        let far = (depth + radius).max(2.0);
        let near = match self.projection {
            Projection::Orthographic => depth - radius,
            Projection::Perspective => (depth - radius).max(far * 1e-3),
        };
        (near, far)
    }

    pub fn view_matrix(&self) -> Mat4 {
        look_to_rh(self.eye(), self.forward(), self.up())
    }

    /// Projection with depth in 0..1. `bounds` sets the depth range.
    pub fn proj_matrix(&self, aspect: f32, bounds: Option<Bounds3>) -> Mat4 {
        let (near, far) = self.clip_range(bounds);
        match self.projection {
            Projection::Perspective => perspective_rh(FOV_Y, aspect, near, far),
            Projection::Orthographic => {
                let hh = self.half_height();
                orthographic_rh(hh * aspect, hh, near, far)
            }
        }
    }

    pub fn view_proj(&self, aspect: f32, bounds: Option<Bounds3>) -> Mat4 {
        self.proj_matrix(aspect, bounds) * self.view_matrix()
    }

    /// Moves the target and sets the distance so that the box fills the view,
    /// with a margin of about 8 percent.
    /// A box with a number that is not finite counts as no box, and so does a bad `aspect`
    /// (a viewport of zero width), so the camera always stays valid.
    pub fn fit(&mut self, bounds: Option<Bounds3>, aspect: f32) {
        let aspect = if aspect.is_finite() && aspect > 1e-3 {
            aspect
        } else {
            1.0
        };
        let (lo, hi) = finite_bounds(bounds).unwrap_or((Vec3::splat(-1000.0), Vec3::splat(1000.0)));
        let center = (lo + hi) / 2.0;
        self.target = center;
        let (right, up, fwd) = (self.right(), self.up(), self.forward());
        let t = (FOV_Y / 2.0).tan();
        let mut need = 1.0f32;
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let o = corner - center;
            let (x, y, z) = (o.dot(right).abs(), o.dot(up).abs(), o.dot(fwd));
            let (tx, ty) = (t * aspect, t);
            need = match self.projection {
                // The corner is `z` further than the target, so the eye must be `x / tx - z` away.
                Projection::Perspective => need.max(x / tx - z).max(y / ty - z),
                Projection::Orthographic => need.max(x / tx).max(y / ty),
            };
        }
        self.distance = if need.is_finite() {
            (need * 1.08).clamp(MIN_DISTANCE, MAX_DISTANCE)
        } else {
            10_000.0
        };
    }

    /// Turns the camera around the target. Angles in radians. The pitch stops at the poles.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        if !(d_yaw.is_finite() && d_pitch.is_finite()) {
            return;
        }
        self.yaw = (self.yaw + d_yaw).rem_euclid(std::f32::consts::TAU);
        self.pitch =
            (self.pitch + d_pitch).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
    }

    /// Drags the scene by `(dx, dy)` px (screen Y down) in a viewport `height_px` tall.
    pub fn pan(&mut self, dx: f32, dy: f32, height_px: f32) {
        if !(dx.is_finite() && dy.is_finite() && height_px.is_finite()) {
            return;
        }
        let per_px = 2.0 * self.half_height() / height_px.max(1.0);
        self.target += (-self.right() * dx + self.up() * dy) * per_px;
    }

    /// Zooms in by `factor` (more than 1 zooms in), keeping the point under `ndc` fixed.
    /// `ndc` is the pointer in -1..1 with Y up.
    pub fn zoom_at(&mut self, factor: f32, ndc: Vec2, aspect: f32) {
        if !(factor.is_finite() && factor > 0.0 && ndc.is_finite() && aspect.is_finite()) {
            return;
        }
        let offset = |c: &Camera| {
            let hh = c.half_height();
            c.right() * ndc.x * hh * aspect + c.up() * ndc.y * hh
        };
        let before = offset(self);
        self.distance = (self.distance / factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        self.target += before - offset(self);
    }

    /// The ray under `ndc` (-1..1, Y up) as (origin, unit direction). For the orthographic
    /// camera the origin is on the plane through the target, so it may be inside the scene.
    pub fn ray(&self, ndc: Vec2, aspect: f32) -> (Vec3, Vec3) {
        let hh = self.half_height();
        let (right, up, fwd) = (self.right(), self.up(), self.forward());
        match self.projection {
            Projection::Orthographic => (
                self.eye() + right * ndc.x * hh * aspect + up * ndc.y * hh,
                fwd,
            ),
            Projection::Perspective => {
                let dir = (fwd
                    + right * ndc.x * (FOV_Y / 2.0).tan() * aspect
                    + up * ndc.y * (FOV_Y / 2.0).tan())
                .normalize();
                (self.eye(), dir)
            }
        }
    }

    /// Projects a point to (ndc x, ndc y, depth 0..1). `None` when it is behind the camera.
    pub fn project(&self, p: Vec3, aspect: f32, bounds: Option<Bounds3>) -> Option<Vec3> {
        let c = self.view_proj(aspect, bounds) * p.extend(1.0);
        (c.w > 1e-6).then(|| Vec3::new(c.x / c.w, c.y / c.w, c.z / c.w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::close;
    use glam::Vec2;

    #[test]
    fn standard_view_directions() {
        let dirs = [
            (StandardView::Top, Vec3::Z, Vec3::X, Vec3::Y),
            (StandardView::Bottom, Vec3::NEG_Z, Vec3::X, Vec3::NEG_Y),
            (StandardView::Front, Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (StandardView::Back, Vec3::Y, Vec3::NEG_X, Vec3::Z),
            (StandardView::Left, Vec3::NEG_X, Vec3::NEG_Y, Vec3::Z),
            (StandardView::Right, Vec3::X, Vec3::Y, Vec3::Z),
        ];
        for (v, eye, right, up) in dirs {
            let c = Camera::standard(v, Projection::Orthographic);
            assert!(close(c.eye_dir(), eye), "{v:?} eye {:?}", c.eye_dir());
            assert!(close(c.right(), right), "{v:?} right {:?}", c.right());
            assert!(close(c.up(), up), "{v:?} up {:?}", c.up());
        }
        // Isometric views look from above at an equal angle to two axes.
        let c = Camera::standard(StandardView::RightIsometric, Projection::Orthographic);
        let d = c.eye_dir();
        assert!(d.x > 0.0 && d.y < 0.0 && d.z > 0.0);
        assert!(
            (d.x + d.y).abs() < 1e-5 && (d.x - d.z * 2f32.sqrt() / 2.0 * 2f32.sqrt()).abs() < 1e-4
                || (d.x.abs() - d.z).abs() < 1e-4
        );
        let rear =
            Camera::standard(StandardView::LeftRearIsometric, Projection::Orthographic).eye_dir();
        assert!(rear.x < 0.0 && rear.y > 0.0 && rear.z > 0.0);
        let left =
            Camera::standard(StandardView::LeftIsometric, Projection::Orthographic).eye_dir();
        assert!(left.x < 0.0 && left.y < 0.0);
        let rr =
            Camera::standard(StandardView::RightRearIsometric, Projection::Orthographic).eye_dir();
        assert!(rr.x > 0.0 && rr.y > 0.0);
        // The camera basis is orthonormal for every view.
        for v in StandardView::ALL {
            let c = Camera::standard(v, Projection::Perspective);
            assert!(
                c.right().dot(c.up()).abs() < 1e-5 && (c.up().length() - 1.0).abs() < 1e-5,
                "{v:?}"
            );
            assert!(c.forward().dot(c.up()).abs() < 1e-5);
        }
    }

    #[test]
    fn projection_maps_the_target_to_the_center_and_ortho_scale_is_exact() {
        for proj in [Projection::Orthographic, Projection::Perspective] {
            let mut c = Camera::standard(StandardView::Front, proj);
            c.target = Vec3::new(100.0, 200.0, 300.0);
            let p = c.project(c.target, 1.5, None).unwrap();
            assert!(p.x.abs() < 1e-5 && p.y.abs() < 1e-5, "{proj:?} {p:?}");
            // A point one half-height above the target is at the top of the view.
            let up = c
                .project(c.target + c.up() * c.half_height(), 1.5, None)
                .unwrap();
            assert!((up.y - 1.0).abs() < 1e-4 && up.x.abs() < 1e-5);
            let right = c
                .project(c.target + c.right() * c.half_height() * 1.5, 1.5, None)
                .unwrap();
            assert!((right.x - 1.0).abs() < 1e-4);
        }
        // Behind the camera.
        let c = Camera::standard(StandardView::Front, Projection::Perspective);
        assert!(c.project(c.eye() - c.forward() * 5.0, 1.0, None).is_none());
    }

    #[test]
    fn rays_pass_through_the_projected_point() {
        for proj in [Projection::Orthographic, Projection::Perspective] {
            let mut c = Camera::standard(StandardView::LeftIsometric, proj);
            c.target = Vec3::new(500.0, -300.0, 20.0);
            c.distance = 7000.0;
            let p = Vec3::new(1200.0, 900.0, 400.0);
            let ndc = c.project(p, 1.7, None).unwrap();
            let (o, d) = c.ray(Vec2::new(ndc.x, ndc.y), 1.7);
            assert!((d.length() - 1.0).abs() < 1e-5);
            let t = (p - o).dot(d);
            assert!(
                (o + d * t - p).length() < 0.5,
                "{proj:?}: miss by {}",
                (o + d * t - p).length()
            );
        }
    }

    #[test]
    fn navigation_math() {
        let mut c = Camera::standard(StandardView::RightIsometric, Projection::Orthographic);
        c.orbit(1.0, 10.0);
        assert!(
            (c.pitch - std::f32::consts::FRAC_PI_2).abs() < 1e-6,
            "pitch stops at the pole"
        );
        c.orbit(0.0, -10.0);
        assert!((c.pitch + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        c.orbit(20.0, 0.0);
        assert!((0.0..std::f32::consts::TAU).contains(&c.yaw));

        // Pan: the point under the pointer follows the pointer.
        let mut c = Camera::standard(StandardView::Top, Projection::Orthographic);
        c.distance = 2000.0 / (FOV_Y / 2.0).tan(); // half height 2000
        let before = c.project(Vec3::new(100.0, 100.0, 0.0), 1.0, None).unwrap();
        c.pan(30.0, -20.0, 400.0); // 400 px tall: 10 mm per px
        let after = c.project(Vec3::new(100.0, 100.0, 0.0), 1.0, None).unwrap();
        assert!(
            (after.x - before.x - 30.0 / 200.0).abs() < 1e-4,
            "moves right by 30 px"
        );
        assert!(
            (after.y - before.y - 20.0 / 200.0).abs() < 1e-4,
            "moves down by 20 px"
        );

        // Zoom to cursor keeps the point under the cursor.
        for proj in [Projection::Orthographic, Projection::Perspective] {
            let mut c = Camera::standard(StandardView::RightIsometric, proj);
            let ndc = Vec2::new(0.4, -0.3);
            let (o, d) = c.ray(ndc, 1.3);
            // The point of the target plane under the cursor.
            let t = (c.target - o).dot(c.forward()) / d.dot(c.forward());
            let p = o + d * t;
            c.zoom_at(2.0, ndc, 1.3);
            assert!((c.distance - 5000.0).abs() < 1e-2);
            let q = c.project(p, 1.3, None).unwrap();
            assert!(
                (q.x - ndc.x).abs() < 1e-3 && (q.y - ndc.y).abs() < 1e-3,
                "{proj:?} {q:?}"
            );
        }
        let mut c = Camera::default();
        let d = c.distance;
        c.zoom_at(f32::NAN, Vec2::ZERO, 1.0);
        c.zoom_at(-1.0, Vec2::ZERO, 1.0);
        assert_eq!(c.distance, d);
        c.zoom_at(1e-30, Vec2::ZERO, 1.0);
        assert_eq!(c.distance, MAX_DISTANCE);
        c.zoom_at(1e30, Vec2::ZERO, 1.0);
        assert_eq!(c.distance, MIN_DISTANCE);
    }

    #[test]
    fn fit_shows_the_whole_box() {
        let bounds = (
            Vec3::new(-3000.0, -1000.0, 0.0),
            Vec3::new(5000.0, 2000.0, 1500.0),
        );
        for proj in [Projection::Orthographic, Projection::Perspective] {
            for view in StandardView::ALL {
                for aspect in [0.6, 1.0, 2.0] {
                    let mut c = Camera::standard(view, proj);
                    c.fit(Some(bounds), aspect);
                    for i in 0..8 {
                        let corner = Vec3::new(
                            if i & 1 == 0 { bounds.0.x } else { bounds.1.x },
                            if i & 2 == 0 { bounds.0.y } else { bounds.1.y },
                            if i & 4 == 0 { bounds.0.z } else { bounds.1.z },
                        );
                        let p = c.project(corner, aspect, Some(bounds)).unwrap();
                        assert!(
                            p.x.abs() <= 1.0 + 1e-3 && p.y.abs() <= 1.0 + 1e-3,
                            "{view:?} {proj:?} {aspect}: {p:?}"
                        );
                        assert!((0.0..=1.0).contains(&p.z), "depth {p:?}");
                    }
                }
            }
        }
        let mut c = Camera::default();
        c.fit(None, 1.0);
        assert_eq!(c.target, Vec3::ZERO);
    }
}
