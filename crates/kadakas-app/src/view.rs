//! Pan and zoom in the app. The view itself is in `kadakas_ui_core::view`; this module adds
//! egui point types to it. Import `ViewExt` to call `to_screen`, `to_model` and the others.

use egui::{Pos2, Rect, Vec2, pos2};
use glam::DVec2;

pub use kadakas_ui_core::view::{MAX_ZOOM, MIN_ZOOM, View, grid_step};

fn d(p: Pos2) -> DVec2 {
    DVec2::new(p.x as f64, p.y as f64)
}

fn pos(p: DVec2) -> Pos2 {
    pos2(p.x as f32, p.y as f32)
}

/// The egui side of the view.
pub trait ViewExt {
    fn to_screen(self, p: DVec2) -> Pos2;
    fn to_model(self, s: Pos2) -> DVec2;
    fn pan(&mut self, delta: Vec2);
    fn zoom_at(&mut self, screen: Pos2, factor: f64);
    fn fit(&mut self, min: DVec2, max: DVec2, rect: Rect);
    fn visible_model(&self, rect: Rect) -> (DVec2, DVec2);
}

impl ViewExt for View {
    fn to_screen(self, p: DVec2) -> Pos2 {
        pos(self.screen_of(p))
    }

    fn to_model(self, s: Pos2) -> DVec2 {
        self.model_of(d(s))
    }

    fn pan(&mut self, delta: Vec2) {
        self.pan_by(DVec2::new(delta.x as f64, delta.y as f64));
    }

    fn zoom_at(&mut self, screen: Pos2, factor: f64) {
        self.zoom_about(d(screen), factor);
    }

    /// Shows the model box `min..max` centered in `rect` with a margin.
    fn fit(&mut self, min: DVec2, max: DVec2, rect: Rect) {
        self.fit_box(min, max, d(rect.min), d(rect.max));
    }

    /// Model box that is visible in `rect`, as (min, max).
    fn visible_model(&self, rect: Rect) -> (DVec2, DVec2) {
        self.visible_box(d(rect.min), d(rect.max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_points_convert_both_ways() {
        let v = View {
            origin: DVec2::new(100.0, 200.0),
            zoom: 0.5,
        };
        let s = v.to_screen(DVec2::new(40.0, 60.0));
        assert_eq!((s.x, s.y), (120.0, 170.0));
        assert!(v.to_model(s).distance(DVec2::new(40.0, 60.0)) < 1e-4);
    }

    #[test]
    fn the_egui_calls_change_the_view() {
        let mut v = View::default();
        v.pan(egui::vec2(10.0, 0.0));
        assert_eq!(v.origin.x, 410.0);
        let s = pos2(250.0, 130.0);
        let m = v.to_model(s);
        v.zoom_at(s, 1.7);
        assert!(v.to_model(s).distance(m) < 1e-3);
        let rect = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        v.fit(
            DVec2::new(-1000.0, -500.0),
            DVec2::new(3000.0, 1500.0),
            rect,
        );
        assert!((v.to_screen(DVec2::new(1000.0, 500.0)) - rect.center()).length() < 0.01);
        let (lo, hi) = v.visible_model(rect);
        assert!(lo.x < hi.x && lo.y < hi.y);
    }
}
