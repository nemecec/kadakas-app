//! The camera of the inference: pixels to rays and points to pixels. It is small on purpose so that
//! the modeler can build it from its own view state (`eye`, `target`, field of view or height).

use glam::{DVec2, DVec3};

/// How the camera projects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// A pinhole camera; `fov_y` is the vertical field of view in radians.
    Perspective { fov_y: f64 },
    /// A parallel projection; `height` is the visible height in world units.
    Orthographic { height: f64 },
}

/// A ray: `origin + t * dir` with `dir` of length 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

/// A point in homogeneous screen form: the pixel is `(x / w, y / w)` from the center of the view.
/// `depth` is the distance along the view direction.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hc {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub depth: f64,
}

/// A segment cut at the near plane and projected: the pixels of its ends and, for each end, the
/// parameter on the original segment and the homogeneous `w`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScreenSeg {
    pub p: [DVec2; 2],
    pub t: [f64; 2],
    pub w: [f64; 2],
}

impl ScreenSeg {
    /// The parameter on the original segment of the point at `s` (0 to 1) on the screen segment,
    /// correct for perspective.
    pub fn world_param(&self, s: f64) -> f64 {
        let (w0, w1) = (self.w[0], self.w[1]);
        let den = s * w0 + (1.0 - s) * w1;
        let tc = if den.abs() < 1e-300 { s } else { s * w0 / den };
        self.t[0] + tc * (self.t[1] - self.t[0])
    }
}

/// A camera and the size of the view in pixels. Pixels run from the top left; y points down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: DVec3,
    right: DVec3,
    up: DVec3,
    forward: DVec3,
    pub projection: Projection,
    /// The size of the view in pixels.
    pub viewport: DVec2,
    /// Points nearer than this along the view direction are not visible.
    pub near: f64,
}

impl Camera {
    /// A camera at `eye` that looks at `target`. `None` when the eye and the target are the same
    /// point, the view is parallel to `up_hint`, the view has no area or a number is not finite.
    pub fn look_at(
        eye: DVec3,
        target: DVec3,
        up_hint: DVec3,
        projection: Projection,
        viewport: DVec2,
    ) -> Option<Camera> {
        let forward = (target - eye).try_normalize()?;
        let right = forward.cross(up_hint).try_normalize()?;
        let up = right.cross(forward);
        let ok = match projection {
            Projection::Perspective { fov_y } => fov_y > 1e-6 && fov_y < 3.1,
            Projection::Orthographic { height } => height > 0.0 && height.is_finite(),
        };
        if !ok || viewport.x < 1.0 || viewport.y < 1.0 || !viewport.is_finite() || !eye.is_finite()
        {
            return None;
        }
        Some(Camera {
            eye,
            right,
            up,
            forward,
            projection,
            viewport,
            near: 1e-3,
        })
    }

    /// The direction the camera looks in.
    pub fn forward(&self) -> DVec3 {
        self.forward
    }

    /// Pixels per world unit at distance 1 (perspective) or at any distance (orthographic).
    fn scale(&self) -> f64 {
        match self.projection {
            Projection::Perspective { fov_y } => self.viewport.y * 0.5 / (fov_y * 0.5).tan(),
            Projection::Orthographic { height } => self.viewport.y / height,
        }
    }

    pub fn is_perspective(&self) -> bool {
        matches!(self.projection, Projection::Perspective { .. })
    }

    /// The distance of a point along the view direction.
    pub fn depth(&self, p: DVec3) -> f64 {
        (p - self.eye).dot(self.forward)
    }

    pub(crate) fn hc(&self, p: DVec3) -> Hc {
        let v = p - self.eye;
        let (x, y, depth) = (v.dot(self.right), v.dot(self.up), v.dot(self.forward));
        let s = self.scale();
        let w = if self.is_perspective() { depth } else { 1.0 };
        Hc {
            x: x * s,
            y: -y * s,
            w,
            depth,
        }
    }

    pub(crate) fn pixel(&self, h: Hc) -> DVec2 {
        DVec2::new(
            self.viewport.x * 0.5 + h.x / h.w,
            self.viewport.y * 0.5 + h.y / h.w,
        )
    }

    /// The pixel of a point, or `None` when the point is not in front of the camera.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        let h = self.hc(p);
        (h.depth > self.near).then(|| self.pixel(h))
    }

    /// The ray through a pixel.
    pub fn ray(&self, pixel: DVec2) -> Ray {
        let s = self.scale();
        let dx = (pixel.x - self.viewport.x * 0.5) / s;
        let dy = -(pixel.y - self.viewport.y * 0.5) / s;
        match self.projection {
            Projection::Perspective { .. } => Ray {
                origin: self.eye,
                dir: (self.forward + self.right * dx + self.up * dy).normalize(),
            },
            Projection::Orthographic { .. } => Ray {
                origin: self.eye + self.right * dx + self.up * dy,
                dir: self.forward,
            },
        }
    }

    /// The size of one pixel in world units at a distance along the view direction.
    pub fn pixel_size_at(&self, depth: f64) -> f64 {
        match self.projection {
            Projection::Perspective { .. } => depth.max(self.near) / self.scale(),
            Projection::Orthographic { .. } => 1.0 / self.scale(),
        }
    }

    /// A segment cut at the near plane and projected. `None` when all of it is behind the near
    /// plane.
    pub(crate) fn screen_seg(&self, a: DVec3, b: DVec3) -> Option<ScreenSeg> {
        let (ha, hb) = (self.hc(a), self.hc(b));
        let (da, db) = (ha.depth - self.near, hb.depth - self.near);
        if da <= 0.0 && db <= 0.0 {
            return None;
        }
        let (mut t0, mut t1) = (0.0, 1.0);
        if da <= 0.0 {
            t0 = da / (da - db);
        } else if db <= 0.0 {
            t1 = da / (da - db);
        }
        let h0 = if t0 == 0.0 {
            ha
        } else {
            self.hc(a.lerp(b, t0))
        };
        let h1 = if t1 == 1.0 {
            hb
        } else {
            self.hc(a.lerp(b, t1))
        };
        Some(ScreenSeg {
            p: [self.pixel(h0), self.pixel(h1)],
            t: [t0, t1],
            w: [h0.w, h1.w],
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn persp() -> Camera {
        Camera::look_at(
            DVec3::new(0.0, -10000.0, 5000.0),
            DVec3::ZERO,
            DVec3::Z,
            Projection::Perspective { fov_y: 0.8 },
            DVec2::new(1000.0, 800.0),
        )
        .unwrap()
    }

    #[test]
    fn the_target_is_at_the_center_and_the_ray_goes_back() {
        let c = persp();
        let p = c.project(DVec3::ZERO).unwrap();
        assert!((p - DVec2::new(500.0, 400.0)).length() < 1e-6);
        let r = c.ray(p);
        assert!(r.origin.distance(c.eye) < 1e-9);
        assert!(r.dir.distance(c.forward()) < 1e-9);
        assert!(c.is_perspective());
    }

    #[test]
    fn project_and_ray_agree_for_both_projections() {
        let o = Camera::look_at(
            DVec3::new(3000.0, 4000.0, 2000.0),
            DVec3::new(10.0, 20.0, 30.0),
            DVec3::Z,
            Projection::Orthographic { height: 6000.0 },
            DVec2::new(640.0, 480.0),
        )
        .unwrap();
        assert!(!o.is_perspective());
        for cam in [persp(), o] {
            let q = DVec3::new(400.0, -300.0, 250.0);
            let px = cam.project(q).unwrap();
            let r = cam.ray(px);
            let t = (q - r.origin).dot(r.dir);
            assert!((r.origin + r.dir * t).distance(q) < 1e-6);
        }
    }

    #[test]
    fn points_behind_the_camera_do_not_project() {
        let c = persp();
        assert!(c.project(c.eye - c.forward() * 10.0).is_none());
        assert!(
            c.screen_seg(c.eye - c.forward() * 5.0, c.eye - c.forward() * 9.0)
                .is_none()
        );
    }

    #[test]
    fn a_segment_is_cut_at_the_near_plane() {
        let c = persp();
        let a = c.eye + c.forward() * 1000.0;
        let b = c.eye - c.forward() * 1000.0;
        let s = c.screen_seg(a, b).unwrap();
        assert_eq!(s.t[0], 0.0);
        assert!(s.t[1] > 0.49 && s.t[1] < 0.51);
        let s2 = c.screen_seg(b, a).unwrap();
        assert!(s2.t[0] > 0.49 && s2.t[0] < 0.51);
        assert_eq!(s2.t[1], 1.0);
    }

    #[test]
    fn the_world_parameter_is_perspective_correct() {
        let c = persp();
        let (a, b) = (
            DVec3::new(-1000.0, 0.0, 0.0),
            DVec3::new(1000.0, 8000.0, 0.0),
        );
        let s = c.screen_seg(a, b).unwrap();
        for t in [0.1, 0.5, 0.9] {
            let px = c.project(a.lerp(b, t)).unwrap();
            let d = s.p[1] - s.p[0];
            let u = (px - s.p[0]).dot(d) / d.length_squared();
            assert!((s.world_param(u) - t).abs() < 1e-9, "{t}");
        }
    }

    #[test]
    fn bad_cameras_are_refused() {
        let p = Projection::Perspective { fov_y: 0.8 };
        let v = DVec2::new(100.0, 100.0);
        let o = Projection::Orthographic { height: 0.0 };
        let z = Projection::Perspective { fov_y: 0.0 };
        assert!(Camera::look_at(DVec3::ZERO, DVec3::ZERO, DVec3::Z, p, v).is_none());
        assert!(Camera::look_at(DVec3::ZERO, DVec3::Z, DVec3::Z, p, v).is_none());
        assert!(Camera::look_at(DVec3::ZERO, DVec3::X, DVec3::Z, z, v).is_none());
        assert!(Camera::look_at(DVec3::ZERO, DVec3::X, DVec3::Z, o, v).is_none());
        assert!(Camera::look_at(DVec3::ZERO, DVec3::X, DVec3::Z, p, DVec2::ZERO).is_none());
    }

    #[test]
    fn the_pixel_size_grows_with_distance_in_perspective_only() {
        let c = persp();
        assert!(c.pixel_size_at(2000.0) > c.pixel_size_at(1000.0));
        let o = Camera::look_at(
            DVec3::new(0.0, -1.0, 0.0),
            DVec3::ZERO,
            DVec3::Z,
            Projection::Orthographic { height: 100.0 },
            DVec2::new(100.0, 100.0),
        )
        .unwrap();
        assert_eq!(o.pixel_size_at(5.0), o.pixel_size_at(500.0));
        assert!((c.depth(c.eye + c.forward() * 7.0) - 7.0).abs() < 1e-9);
    }
}
