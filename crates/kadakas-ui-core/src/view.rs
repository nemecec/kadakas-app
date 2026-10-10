//! Pan and zoom of the plan. This is the place where model coordinates (mm, Y up) are converted
//! to screen coordinates (px, Y down). Screen points are `DVec2` values, so the type does not
//! depend on a UI toolkit; a UI program converts its own point type.

use glam::DVec2;

pub const MIN_ZOOM: f64 = 1e-4;
pub const MAX_ZOOM: f64 = 1e2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Screen position of the model origin, in px.
    pub origin: DVec2,
    /// Pixels per millimeter.
    pub zoom: f64,
}

impl Default for View {
    fn default() -> Self {
        View {
            origin: DVec2::new(400.0, 300.0),
            zoom: 0.05,
        }
    }
}

impl View {
    /// The screen position (px) of a model point.
    pub fn screen_of(self, p: DVec2) -> DVec2 {
        DVec2::new(
            self.origin.x + p.x * self.zoom,
            self.origin.y - p.y * self.zoom,
        )
    }

    /// The model point (mm) at a screen position.
    pub fn model_of(self, s: DVec2) -> DVec2 {
        DVec2::new(
            (s.x - self.origin.x) / self.zoom,
            (self.origin.y - s.y) / self.zoom,
        )
    }

    /// Moves the view by `delta` px.
    pub fn pan_by(&mut self, delta: DVec2) {
        self.origin += delta;
    }

    /// Zooms by `factor`, keeping the model point under `screen` fixed.
    pub fn zoom_about(&mut self, screen: DVec2, factor: f64) {
        let before = self.model_of(screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.screen_of(before);
        self.origin += screen - after;
    }

    /// Shows the model box `min..max` centered in the screen rectangle `rect_min..rect_max`, with
    /// a margin.
    pub fn fit_box(&mut self, min: DVec2, max: DVec2, rect_min: DVec2, rect_max: DVec2) {
        let size = (max - min).max(DVec2::splat(1.0));
        let avail = (rect_max - rect_min) * 0.9;
        self.zoom = (avail.x / size.x)
            .min(avail.y / size.y)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        let center = (min + max) / 2.0;
        let c = (rect_min + rect_max) / 2.0;
        self.origin = DVec2::new(c.x - center.x * self.zoom, c.y + center.y * self.zoom);
    }

    /// The model box that is visible in the screen rectangle `rect_min..rect_max`, as (min, max).
    pub fn visible_box(&self, rect_min: DVec2, rect_max: DVec2) -> (DVec2, DVec2) {
        let a = self.model_of(DVec2::new(rect_min.x, rect_max.y));
        let b = self.model_of(DVec2::new(rect_max.x, rect_min.y));
        (a.min(b), a.max(b))
    }
}

/// Grid spacing in mm: a 1-2-5 step that is at least `min_px` px wide.
pub fn grid_step(zoom: f64, min_px: f64) -> f64 {
    let raw = min_px / zoom;
    let mag = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_y_flip() {
        let v = View {
            origin: DVec2::new(100.0, 200.0),
            zoom: 0.5,
        };
        let p = DVec2::new(40.0, 60.0);
        let s = v.screen_of(p);
        assert_eq!((s.x, s.y), (120.0, 170.0)); // model Y up is screen Y down
        assert!(v.model_of(s).distance(p) < 1e-9);
    }

    #[test]
    fn zoom_keeps_cursor_point() {
        let mut v = View::default();
        let s = DVec2::new(250.0, 130.0);
        let m = v.model_of(s);
        v.zoom_about(s, 1.7);
        assert!(v.model_of(s).distance(m) < 1e-6);
        assert!((v.zoom - 0.085).abs() < 1e-9);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut v = View::default();
        v.zoom_about(DVec2::ZERO, 1e9);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_about(DVec2::ZERO, 1e-30);
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn panning_moves_the_origin() {
        let mut v = View::default();
        v.pan_by(DVec2::new(5.0, -7.0));
        assert_eq!(v.origin, DVec2::new(405.0, 293.0));
    }

    #[test]
    fn fit_contains_box() {
        let mut v = View::default();
        let (lo, hi) = (DVec2::new(10.0, 20.0), DVec2::new(810.0, 620.0));
        v.fit_box(
            DVec2::new(-1000.0, -500.0),
            DVec2::new(3000.0, 1500.0),
            lo,
            hi,
        );
        let inside = |p: DVec2| p.cmpge(lo).all() && p.cmple(hi).all();
        assert!(inside(v.screen_of(DVec2::new(-1000.0, -500.0))));
        assert!(inside(v.screen_of(DVec2::new(3000.0, 1500.0))));
        assert!(
            v.screen_of(DVec2::new(1000.0, 500.0))
                .distance((lo + hi) / 2.0)
                < 1e-6
        );
    }

    #[test]
    fn the_visible_box_is_the_screen_rectangle_in_model_units() {
        let v = View {
            origin: DVec2::new(100.0, 200.0),
            zoom: 0.5,
        };
        let (min, max) = v.visible_box(DVec2::new(100.0, 100.0), DVec2::new(300.0, 200.0));
        assert_eq!(min, DVec2::new(0.0, 0.0));
        assert_eq!(max, DVec2::new(400.0, 200.0));
    }

    #[test]
    fn grid_steps() {
        assert_eq!(grid_step(1.0, 10.0), 10.0);
        assert_eq!(grid_step(0.05, 10.0), 200.0);
        assert_eq!(grid_step(0.01, 10.0), 1000.0);
        assert_eq!(grid_step(0.03, 10.0), 500.0);
    }
}
