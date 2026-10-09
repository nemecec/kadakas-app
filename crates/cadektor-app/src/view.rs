//! Pan and zoom. The only place where model coordinates (mm, Y up) are
//! converted to screen coordinates (px, Y down).

use egui::{Pos2, Rect, Vec2, pos2};
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
        View { origin: DVec2::new(400.0, 300.0), zoom: 0.05 }
    }
}

impl View {
    pub fn to_screen(self, p: DVec2) -> Pos2 {
        pos2((self.origin.x + p.x * self.zoom) as f32, (self.origin.y - p.y * self.zoom) as f32)
    }

    pub fn to_model(self, s: Pos2) -> DVec2 {
        DVec2::new((s.x as f64 - self.origin.x) / self.zoom, (self.origin.y - s.y as f64) / self.zoom)
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.origin += DVec2::new(delta.x as f64, delta.y as f64);
    }

    /// Zooms by `factor`, keeping the model point under `screen` fixed.
    pub fn zoom_at(&mut self, screen: Pos2, factor: f64) {
        let before = self.to_model(screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_screen(before);
        self.origin += DVec2::new((screen.x - after.x) as f64, (screen.y - after.y) as f64);
    }

    /// Shows the model box `min..max` centered in `rect` with a margin.
    pub fn fit(&mut self, min: DVec2, max: DVec2, rect: Rect) {
        let size = (max - min).max(DVec2::splat(1.0));
        let avail = DVec2::new(rect.width() as f64, rect.height() as f64) * 0.9;
        self.zoom = (avail.x / size.x).min(avail.y / size.y).clamp(MIN_ZOOM, MAX_ZOOM);
        let center = (min + max) / 2.0;
        let c = rect.center();
        self.origin = DVec2::new(c.x as f64 - center.x * self.zoom, c.y as f64 + center.y * self.zoom);
    }

    /// Model box that is visible in `rect`, as (min, max).
    pub fn visible_model(&self, rect: Rect) -> (DVec2, DVec2) {
        let a = self.to_model(rect.left_bottom());
        let b = self.to_model(rect.right_top());
        (a.min(b), a.max(b))
    }
}

/// Grid spacing in mm: a 1-2-5 step that is at least `min_px` px wide.
pub fn grid_step(zoom: f64, min_px: f64) -> f64 {
    let raw = min_px / zoom;
    let mag = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_y_flip() {
        let v = View { origin: DVec2::new(100.0, 200.0), zoom: 0.5 };
        let p = DVec2::new(40.0, 60.0);
        let s = v.to_screen(p);
        assert_eq!((s.x, s.y), (120.0, 170.0)); // model Y up is screen Y down
        assert!(v.to_model(s).distance(p) < 1e-4);
    }

    #[test]
    fn zoom_keeps_cursor_point() {
        let mut v = View::default();
        let s = pos2(250.0, 130.0);
        let m = v.to_model(s);
        v.zoom_at(s, 1.7);
        assert!(v.to_model(s).distance(m) < 1e-3);
        assert!((v.zoom - 0.085).abs() < 1e-9);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut v = View::default();
        v.zoom_at(pos2(0.0, 0.0), 1e9);
        assert_eq!(v.zoom, MAX_ZOOM);
    }

    #[test]
    fn fit_contains_box() {
        let mut v = View::default();
        let rect = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        v.fit(DVec2::new(-1000.0, -500.0), DVec2::new(3000.0, 1500.0), rect);
        let a = v.to_screen(DVec2::new(-1000.0, -500.0));
        let b = v.to_screen(DVec2::new(3000.0, 1500.0));
        assert!(rect.contains(a) && rect.contains(b));
        assert!((v.to_screen(DVec2::new(1000.0, 500.0)) - rect.center()).length() < 0.01);
    }

    #[test]
    fn grid_steps() {
        assert_eq!(grid_step(1.0, 10.0), 10.0);
        assert_eq!(grid_step(0.05, 10.0), 200.0);
        assert_eq!(grid_step(0.01, 10.0), 1000.0);
        assert_eq!(grid_step(0.03, 10.0), 500.0);
    }
}
