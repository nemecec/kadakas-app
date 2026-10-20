//! Navigation in the 3D view of the modeler (MOD-02): orbit, pan, zoom, zoom to the cursor, zoom
//! extents, zoom window, standard views, perspective and parallel projection, the field of view,
//! and Previous and Next view.
//!
//! The module has no window and no GPU. A UI program turns the pointer and the keys into the calls
//! here (`drag_for` says what a drag does, `nav_action` what a key does) and draws the camera. The
//! camera is the orbit camera of `kadakas-render`; this module adds the rules of the modeler: a
//! drag of the middle button orbits, Shift with it pans, the wheel zooms to the cursor, a view
//! change is one entry of the view history.
//!
//! Screen points are in points from the top left of the viewport, with Y down. The viewport size
//! is a `Vec2`.

use glam::{Vec2, Vec3};
use kadakas_core::mesh::Bounds3;
use kadakas_render::{Camera, Projection, StandardView};

use crate::keymap::Action;
use crate::keys::Modifiers;
use crate::modeler::ModelerTool;
use crate::view_history::ViewHistory;

/// Radians of orbit for a drag of one point (the same as the Vectorworks-like app).
pub const ORBIT_PER_POINT: f32 = 0.008;
/// The zoom factor of the Zoom tool is `exp(-dy * ZOOM_PER_POINT)`: a drag up zooms in.
pub const ZOOM_PER_POINT: f32 = 0.01;
/// The zoom factor of the wheel is `exp(scroll * WHEEL_PER_POINT)`.
pub const WHEEL_PER_POINT: f32 = 0.0025;
/// Degrees of field of view for a drag of one point (Shift with the Zoom tool).
pub const FOV_DEG_PER_POINT: f32 = 0.1;
/// The narrowest and the widest vertical field of view, in degrees.
pub const MIN_FOV_DEG: f32 = 1.0;
pub const MAX_FOV_DEG: f32 = 120.0;
/// The vertical field of view that a new view has, in degrees.
pub const DEFAULT_FOV_DEG: f32 = 35.0;
/// The zoom factor of the keys for zoom in and zoom out.
pub const KEY_ZOOM: f32 = 2.0;
/// The pan of one key press, in points.
pub const KEY_PAN_POINTS: f32 = 40.0;
/// The smallest window of Zoom Window, in points. A smaller drag is a click and does nothing.
pub const MIN_WINDOW_POINTS: f32 = 4.0;

/// A mouse button, as the navigation sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Middle,
    Secondary,
}

/// What a drag does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drag {
    /// The drag is not navigation (the tool or the selection uses it).
    None,
    Orbit,
    Pan,
    /// Zoom about the center of the view; a drag up zooms in.
    Zoom,
    /// Change the field of view.
    Fov,
    /// Draw a window to zoom to.
    ZoomWindow,
}

/// What a drag with `button` does when `tool` is active and `mods` are held. `window_armed` is the
/// state of Zoom Window (`ToolState::window_armed`).
///
/// - The middle button always navigates, whatever the tool: it orbits, and with Shift it pans.
/// - The Orbit tool orbits with the primary button, and with Shift it pans.
/// - The Pan tool pans.
/// - The Zoom tool zooms; with Shift it changes the field of view; with Ctrl/Cmd, or after Zoom
///   Window, the drag is a window.
/// - Other tools and the secondary button do not navigate.
pub fn drag_for(tool: ModelerTool, button: Button, mods: Modifiers, window_armed: bool) -> Drag {
    match (button, tool) {
        (Button::Middle, _) if mods.shift => Drag::Pan,
        (Button::Middle, _) => Drag::Orbit,
        (Button::Primary, ModelerTool::Orbit) if mods.shift => Drag::Pan,
        (Button::Primary, ModelerTool::Orbit) => Drag::Orbit,
        (Button::Primary, ModelerTool::Pan) => Drag::Pan,
        (Button::Primary, ModelerTool::Zoom) if mods.shift => Drag::Fov,
        (Button::Primary, ModelerTool::Zoom) if mods.command || window_armed => Drag::ZoomWindow,
        (Button::Primary, ModelerTool::Zoom) => Drag::Zoom,
        _ => Drag::None,
    }
}

/// The zoom factor of the wheel and of the pinch gesture. `scroll` is the scroll in points (up is
/// positive), `pinch` the factor of the gesture (1 is none).
pub fn wheel_factor(scroll: f32, pinch: f32) -> f32 {
    (scroll * WHEEL_PER_POINT).exp() * pinch
}

/// A key that navigates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NavAction {
    ZoomExtents,
    /// A standard view. The Iso view is the right isometric one.
    View(StandardView),
    /// Switch between perspective and parallel projection.
    ToggleProjection,
    ZoomIn,
    ZoomOut,
    /// Pan by a step: the scene follows the key, like the scroll bars would.
    PanStep(Vec2),
    Previous,
    Next,
}

/// The navigation that a key action does, if it is one.
pub fn nav_action(action: Action) -> Option<NavAction> {
    Some(match action {
        Action::ZoomFit => NavAction::ZoomExtents,
        // The modeler has no 2D plan: Top/Plan is the Top view.
        Action::ViewPlan | Action::ViewTop => NavAction::View(StandardView::Top),
        Action::ViewBottom => NavAction::View(StandardView::Bottom),
        Action::ViewFront => NavAction::View(StandardView::Front),
        Action::ViewBack => NavAction::View(StandardView::Back),
        Action::ViewLeft => NavAction::View(StandardView::Left),
        Action::ViewRight => NavAction::View(StandardView::Right),
        Action::ViewRightIso => NavAction::View(StandardView::RightIsometric),
        Action::ViewLeftIso => NavAction::View(StandardView::LeftIsometric),
        Action::ViewLeftRearIso => NavAction::View(StandardView::LeftRearIsometric),
        Action::ViewRightRearIso => NavAction::View(StandardView::RightRearIsometric),
        Action::ViewProjection => NavAction::ToggleProjection,
        Action::ZoomIn => NavAction::ZoomIn,
        Action::ZoomOut => NavAction::ZoomOut,
        Action::PanLeft => NavAction::PanStep(Vec2::new(KEY_PAN_POINTS, 0.0)),
        Action::PanRight => NavAction::PanStep(Vec2::new(-KEY_PAN_POINTS, 0.0)),
        Action::PanUp => NavAction::PanStep(Vec2::new(0.0, KEY_PAN_POINTS)),
        Action::PanDown => NavAction::PanStep(Vec2::new(0.0, -KEY_PAN_POINTS)),
        Action::ViewHistoryPrevious => NavAction::Previous,
        Action::ViewHistoryNext => NavAction::Next,
        _ => return None,
    })
}

/// The viewport that a navigation runs in.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The box around the scene, for Zoom Extents and the standard views.
    pub bounds: Option<Bounds3>,
    /// The size of the viewport in points.
    pub size: Vec2,
}

impl Frame {
    pub fn aspect(&self) -> f32 {
        let a = self.size.x / self.size.y.max(1.0);
        if a.is_finite() && a > 1e-3 { a } else { 1.0 }
    }
}

/// The camera of the modeler with its view history.
#[derive(Debug)]
pub struct Nav3d {
    pub camera: Camera,
    history: ViewHistory<Camera>,
}

impl Default for Nav3d {
    fn default() -> Self {
        Nav3d::new()
    }
}

impl Nav3d {
    /// A perspective camera in the Iso view with the default field of view, like a new model of
    /// a modeler.
    pub fn new() -> Nav3d {
        let mut camera = Camera::standard(StandardView::RightIsometric, Projection::Perspective);
        camera.fov_y = DEFAULT_FOV_DEG.to_radians();
        Nav3d {
            camera,
            history: ViewHistory::default(),
        }
    }

    pub fn is_perspective(&self) -> bool {
        self.camera.projection == Projection::Perspective
    }

    /// The vertical field of view in degrees.
    pub fn fov_deg(&self) -> f32 {
        self.camera.fov_y.to_degrees()
    }

    /// Turns the view about the target (the center of the view). `delta` is the drag in points:
    /// right turns the scene to the right, down looks from higher up.
    pub fn orbit(&mut self, delta: Vec2) {
        self.orbit_about(delta, None);
    }

    /// Turns the view about `pivot`, a point of the scene. The pivot stays where it is on the
    /// screen, at the same depth. `None` turns about the target.
    pub fn orbit_about(&mut self, delta: Vec2, pivot: Option<Vec3>) {
        if !delta.is_finite() {
            return;
        }
        let Some(p) = pivot.filter(|p| p.is_finite()) else {
            self.camera
                .orbit(-delta.x * ORBIT_PER_POINT, delta.y * ORBIT_PER_POINT);
            return;
        };
        // The pivot in the frame of the camera: it is kept while the camera turns.
        let rel = p - self.camera.target;
        let (r, u, f) = (
            rel.dot(self.camera.right()),
            rel.dot(self.camera.up()),
            rel.dot(self.camera.forward()),
        );
        self.camera
            .orbit(-delta.x * ORBIT_PER_POINT, delta.y * ORBIT_PER_POINT);
        let c = &self.camera;
        self.camera.target = p - (c.right() * r + c.up() * u + c.forward() * f);
    }

    /// Drags the scene by `delta` points in a viewport `height` points tall.
    pub fn pan(&mut self, delta: Vec2, height: f32) {
        self.camera.pan(delta.x, delta.y, height);
    }

    /// Zooms in by `factor` (more than 1 zooms in) toward the screen point `at`: the point of the
    /// scene under the pointer stays under the pointer. `at` is in points from the top left.
    pub fn zoom_at(&mut self, factor: f32, at: Vec2, size: Vec2) {
        let ndc = Vec2::new(
            at.x / size.x.max(1.0) * 2.0 - 1.0,
            1.0 - at.y / size.y.max(1.0) * 2.0,
        );
        self.camera
            .zoom_at(factor, ndc, size.x.max(1.0) / size.y.max(1.0));
    }

    /// Zoom about the center of the view.
    pub fn zoom_center(&mut self, factor: f32) {
        self.camera.zoom_at(factor, Vec2::ZERO, 1.0);
    }

    /// Shows the whole scene. With no scene it shows a default box.
    pub fn zoom_extents(&mut self, frame: &Frame) {
        self.camera.fit(frame.bounds, frame.aspect());
    }

    /// Zooms to the window `a..b` (two corners, in points from the top left): the point at its
    /// center moves to the center of the view, and the window fills the view as far as its shape
    /// allows. A window smaller than [`MIN_WINDOW_POINTS`] does nothing. Returns true when the
    /// view changed.
    pub fn zoom_window(&mut self, a: Vec2, b: Vec2, size: Vec2) -> bool {
        let (lo, hi) = (a.min(b), a.max(b));
        let (w, h) = (hi.x - lo.x, hi.y - lo.y);
        if !(w.is_finite() && h.is_finite()) || w < MIN_WINDOW_POINTS || h < MIN_WINDOW_POINTS {
            return false;
        }
        let size = size.max(Vec2::ONE);
        let center = (lo + hi) / 2.0;
        let ndc = Vec2::new(center.x / size.x * 2.0 - 1.0, 1.0 - center.y / size.y * 2.0);
        // The share of the view that the window takes, in each direction.
        let share = (w / size.x).max(h / size.y).clamp(1e-4, 1.0);
        let c = &mut self.camera;
        let hh = c.half_height();
        let aspect = size.x / size.y;
        c.target += c.right() * ndc.x * hh * aspect + c.up() * ndc.y * hh;
        c.zoom_at(1.0 / share, Vec2::ZERO, 1.0);
        true
    }

    /// Looks from a standard view and shows the whole scene.
    pub fn standard_view(&mut self, view: StandardView, frame: &Frame) {
        self.camera.set_view(view);
        self.camera.fit(frame.bounds, frame.aspect());
    }

    /// Switches the projection. The view keeps its size at the target, so the picture does not
    /// jump: the point that the camera looks at stays where it is, and so does everything else
    /// on the plane through the target.
    pub fn set_projection(&mut self, projection: Projection) {
        self.camera.projection = projection;
    }

    pub fn toggle_projection(&mut self) {
        self.set_projection(self.camera.projection.toggled());
    }

    /// Sets the vertical field of view in degrees, between [`MIN_FOV_DEG`] and [`MAX_FOV_DEG`]. The
    /// view keeps its size at the target: a wider angle moves the eye closer, a narrower one
    /// further away. A value that is not a number is ignored.
    pub fn set_fov_deg(&mut self, deg: f32) {
        if !deg.is_finite() {
            return;
        }
        let half_height = self.camera.half_height();
        let fov = deg.clamp(MIN_FOV_DEG, MAX_FOV_DEG).to_radians();
        self.camera.fov_y = fov;
        self.camera.distance = (half_height / (fov / 2.0).tan()).clamp(1.0, 1.0e9);
    }

    /// Does a drag step. `delta` is the movement in points since the last step; `at` is the pointer
    /// and `size` the viewport. `pivot` is the point that Orbit turns about (`None`: the center of
    /// the view). A window drag does nothing here (the caller shows the window and calls
    /// [`Nav3d::zoom_window`] when the button is released).
    pub fn drag(&mut self, drag: Drag, delta: Vec2, size: Vec2, pivot: Option<Vec3>) {
        match drag {
            Drag::Orbit => self.orbit_about(delta, pivot),
            Drag::Pan => self.pan(delta, size.y),
            Drag::Zoom => self.zoom_center((-delta.y * ZOOM_PER_POINT).exp()),
            Drag::Fov => self.set_fov_deg(self.fov_deg() + delta.y * FOV_DEG_PER_POINT),
            Drag::ZoomWindow | Drag::None => {}
        }
    }

    /// Does what a key does. Previous and Next restore a view of the history; see
    /// [`Nav3d::previous`].
    pub fn apply(&mut self, action: NavAction, frame: &Frame) {
        match action {
            NavAction::ZoomExtents => self.zoom_extents(frame),
            NavAction::View(v) => self.standard_view(v, frame),
            NavAction::ToggleProjection => self.toggle_projection(),
            NavAction::ZoomIn => self.zoom_center(KEY_ZOOM),
            NavAction::ZoomOut => self.zoom_center(1.0 / KEY_ZOOM),
            NavAction::PanStep(d) => self.pan(d, frame.size.y),
            NavAction::Previous => {
                self.previous();
            }
            NavAction::Next => {
                self.next();
            }
        }
    }

    /// Call once per frame with the time in seconds. A view that has rested for
    /// [`crate::view_history::SETTLE`] becomes an entry of the history. Returns true while a
    /// change waits to settle, so the caller can ask for a repaint.
    pub fn observe(&mut self, now: f64) -> bool {
        self.history.observe(now, self.camera)
    }

    /// Goes back to the view before this one. False when there is none.
    pub fn previous(&mut self) -> bool {
        // The view that you leave can be reached again with Next.
        self.history.flush(self.camera);
        match self.history.previous() {
            Some(c) => {
                self.camera = c;
                true
            }
            None => false,
        }
    }

    /// Goes forward to the view after this one. False when there is none.
    pub fn next(&mut self) -> bool {
        match self.history.next() {
            Some(c) => {
                self.camera = c;
                true
            }
            None => false,
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.history.can_go_back()
    }

    pub fn can_go_forward(&self) -> bool {
        self.history.can_go_forward()
    }

    /// Forgets the history (a new document).
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_history::SETTLE;

    const SIZE: Vec2 = Vec2::new(800.0, 600.0);

    fn frame() -> Frame {
        Frame {
            bounds: Some((
                Vec3::new(-3000.0, -1000.0, 0.0),
                Vec3::new(5000.0, 2000.0, 1500.0),
            )),
            size: SIZE,
        }
    }

    fn ndc_of(at: Vec2) -> Vec2 {
        Vec2::new(at.x / SIZE.x * 2.0 - 1.0, 1.0 - at.y / SIZE.y * 2.0)
    }

    /// The point of the plane through the target that is under the screen point `at`.
    fn target_plane_point(c: &Camera, at: Vec2) -> Vec3 {
        let (o, d) = c.ray(ndc_of(at), SIZE.x / SIZE.y);
        let t = (c.target - o).dot(c.forward()) / d.dot(c.forward());
        o + d * t
    }

    fn screen_of(c: &Camera, p: Vec3) -> Vec2 {
        let n = c.project(p, SIZE.x / SIZE.y, None).unwrap();
        Vec2::new(n.x, n.y)
    }

    #[test]
    fn a_new_view_is_perspective_in_iso_with_the_default_field_of_view() {
        let n = Nav3d::new();
        assert!(n.is_perspective());
        assert!((n.fov_deg() - DEFAULT_FOV_DEG).abs() < 1e-3);
        let d = n.camera.eye_dir();
        assert!(
            d.x > 0.0 && d.y < 0.0 && d.z > 0.0,
            "front right, from above"
        );
    }

    #[test]
    fn orbit_turns_about_the_target_and_stops_at_the_poles() {
        let mut n = Nav3d::new();
        n.camera.set_view(StandardView::Front);
        let right = n.camera.right();
        let target = n.camera.target;
        let dist = n.camera.distance;
        n.orbit(Vec2::new(30.0, 0.0));
        // Dragging right turns the scene to the right: the eye moves to the left.
        assert!(n.camera.eye_dir().dot(right) < 0.0);
        assert_eq!(n.camera.target, target);
        assert_eq!(n.camera.distance, dist);
        n.orbit(Vec2::new(0.0, 1.0e6));
        assert!((n.camera.pitch - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        n.orbit(Vec2::new(f32::NAN, 1.0));
        assert!(n.camera.yaw.is_finite() && n.camera.pitch.is_finite());
    }

    #[test]
    fn orbit_about_a_pivot_keeps_the_pivot_on_the_screen() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut n = Nav3d::new();
            n.set_projection(projection);
            n.camera.target = Vec3::new(100.0, -200.0, 50.0);
            let pivot = Vec3::new(1500.0, 900.0, 300.0);
            let before = n.camera.project(pivot, 1.5, None).unwrap();
            n.orbit_about(Vec2::new(55.0, -23.0), Some(pivot));
            let after = n.camera.project(pivot, 1.5, None).unwrap();
            assert!(
                (after - before).length() < 1e-3,
                "{projection:?}: the pivot moved from {before:?} to {after:?}"
            );
            // The view did turn.
            let mut m = Nav3d::new();
            m.set_projection(projection);
            m.camera.target = Vec3::new(100.0, -200.0, 50.0);
            assert!((m.camera.eye_dir() - n.camera.eye_dir()).length() > 0.05);
            // Orbit about the target is the orbit about the pivot at the target.
            let mut a = Nav3d::new();
            let mut b = Nav3d::new();
            a.orbit(Vec2::new(10.0, 4.0));
            b.orbit_about(Vec2::new(10.0, 4.0), Some(b.camera.target));
            assert!((a.camera.eye() - b.camera.eye()).length() < 1e-2);
        }
        // A pivot that is not a number is ignored.
        let mut n = Nav3d::new();
        n.orbit_about(Vec2::new(5.0, 5.0), Some(Vec3::NAN));
        assert!(n.camera.target.is_finite());
    }

    #[test]
    fn zoom_to_the_cursor_keeps_the_point_under_the_cursor() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for view in [
                StandardView::RightIsometric,
                StandardView::Top,
                StandardView::Front,
            ] {
                let mut n = Nav3d::new();
                n.set_projection(projection);
                n.camera.set_view(view);
                let at = Vec2::new(620.0, 140.0);
                let p = target_plane_point(&n.camera, at);
                for factor in [1.5, 0.4, 3.0] {
                    n.zoom_at(factor, at, SIZE);
                    let s = screen_of(&n.camera, p);
                    let want = ndc_of(at);
                    assert!(
                        (s - want).length() < 2e-3,
                        "{projection:?} {view:?} x{factor}: {s:?} want {want:?}"
                    );
                }
            }
        }
        // The distance follows the factor and stays in range.
        let mut n = Nav3d::new();
        let d = n.camera.distance;
        n.zoom_at(2.0, SIZE / 2.0, SIZE);
        assert!((n.camera.distance - d / 2.0).abs() < 1e-2);
        n.zoom_at(1e-30, SIZE / 2.0, SIZE);
        assert!(n.camera.distance.is_finite() && n.camera.distance <= 1.0e9);
        n.zoom_at(f32::NAN, SIZE / 2.0, SIZE);
        n.zoom_at(1.1, Vec2::ZERO, Vec2::ZERO);
        assert!(n.camera.distance.is_finite());
    }

    #[test]
    fn the_wheel_zooms_in_when_scrolled_up() {
        assert!(wheel_factor(40.0, 1.0) > 1.0);
        assert!(wheel_factor(-40.0, 1.0) < 1.0);
        assert!((wheel_factor(0.0, 1.0) - 1.0).abs() < 1e-6);
        assert!(
            (wheel_factor(0.0, 1.3) - 1.3).abs() < 1e-6,
            "the pinch gesture too"
        );
    }

    #[test]
    fn zoom_extents_shows_the_whole_scene_in_every_view_and_projection() {
        let f = frame();
        let (lo, hi) = f.bounds.unwrap();
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for view in StandardView::ALL {
                let mut n = Nav3d::new();
                n.set_projection(projection);
                n.camera.set_view(view);
                n.zoom_extents(&f);
                for i in 0..8 {
                    let corner = Vec3::new(
                        if i & 1 == 0 { lo.x } else { hi.x },
                        if i & 2 == 0 { lo.y } else { hi.y },
                        if i & 4 == 0 { lo.z } else { hi.z },
                    );
                    let p = n.camera.project(corner, f.aspect(), f.bounds).unwrap();
                    assert!(
                        p.x.abs() <= 1.0 + 1e-3 && p.y.abs() <= 1.0 + 1e-3,
                        "{view:?} {projection:?}: {p:?}"
                    );
                }
                assert!((n.camera.target - (lo + hi) / 2.0).length() < 1e-2);
            }
        }
        // No scene, a zero-sized viewport and a box with a bad number still give a valid camera.
        let mut n = Nav3d::new();
        n.zoom_extents(&Frame {
            bounds: None,
            size: Vec2::ZERO,
        });
        assert!(n.camera.distance.is_finite() && n.camera.target.is_finite());
        n.zoom_extents(&Frame {
            bounds: Some((Vec3::NAN, Vec3::ONE)),
            size: SIZE,
        });
        assert!(n.camera.distance.is_finite() && n.camera.target.is_finite());
    }

    #[test]
    fn a_standard_view_looks_from_the_right_side_and_frames_the_scene() {
        let f = frame();
        let want = [
            (StandardView::Top, Vec3::Z),
            (StandardView::Bottom, Vec3::NEG_Z),
            (StandardView::Front, Vec3::NEG_Y),
            (StandardView::Back, Vec3::Y),
            (StandardView::Left, Vec3::NEG_X),
            (StandardView::Right, Vec3::X),
        ];
        for (view, eye) in want {
            let mut n = Nav3d::new();
            n.apply(NavAction::View(view), &f);
            assert!((n.camera.eye_dir() - eye).length() < 1e-5, "{view:?}");
            assert!(
                (n.camera.target - (f.bounds.unwrap().0 + f.bounds.unwrap().1) / 2.0).length()
                    < 1e-2
            );
        }
        let mut n = Nav3d::new();
        n.apply(NavAction::View(StandardView::RightIsometric), &f);
        let d = n.camera.eye_dir();
        assert!(d.x > 0.0 && d.y < 0.0 && d.z > 0.0);
    }

    #[test]
    fn switching_the_projection_keeps_the_view() {
        let mut n = Nav3d::new();
        n.camera.target = Vec3::new(400.0, 300.0, 100.0);
        n.orbit(Vec2::new(20.0, 10.0));
        n.zoom_at(1.7, Vec2::new(300.0, 200.0), SIZE);
        let eye = n.camera.eye();
        // The plane through the target: its points land on the same screen points in both.
        let probes: Vec<Vec3> = [(-0.6f32, 0.4f32), (0.5, -0.3), (0.0, 0.0), (0.9, 0.9)]
            .iter()
            .map(|(x, y)| {
                let hh = n.camera.half_height();
                n.camera.target + n.camera.right() * (x * hh * 1.5) + n.camera.up() * (y * hh)
            })
            .collect();
        let persp: Vec<Vec2> = probes.iter().map(|p| screen_of(&n.camera, *p)).collect();
        n.toggle_projection();
        assert!(!n.is_perspective());
        assert_eq!(n.camera.eye(), eye, "the eye does not move");
        for (p, s) in probes.iter().zip(&persp) {
            assert!((screen_of(&n.camera, *p) - *s).length() < 1e-3, "{p:?}");
        }
        n.toggle_projection();
        assert!(n.is_perspective());
        for (p, s) in probes.iter().zip(&persp) {
            assert!((screen_of(&n.camera, *p) - *s).length() < 1e-3);
        }
    }

    #[test]
    fn the_field_of_view_keeps_the_view_size_at_the_target_and_is_clamped() {
        let mut n = Nav3d::new();
        let hh = n.camera.half_height();
        for deg in [20.0, 60.0, 110.0] {
            n.set_fov_deg(deg);
            assert!((n.fov_deg() - deg).abs() < 1e-3);
            assert!((n.camera.half_height() - hh).abs() / hh < 1e-4, "{deg}");
        }
        // A wider angle moves the eye closer.
        n.set_fov_deg(20.0);
        let far = n.camera.distance;
        n.set_fov_deg(90.0);
        assert!(n.camera.distance < far);
        n.set_fov_deg(500.0);
        assert!((n.fov_deg() - MAX_FOV_DEG).abs() < 1e-3);
        n.set_fov_deg(-5.0);
        assert!((n.fov_deg() - MIN_FOV_DEG).abs() < 1e-3);
        let kept = n.camera;
        n.set_fov_deg(f32::NAN);
        assert_eq!(n.camera, kept);
        // The camera stays valid: rays and projection work.
        let p = n.camera.project(n.camera.target, 1.0, None).unwrap();
        assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4);
    }

    #[test]
    fn pan_drags_the_scene_with_the_pointer() {
        let mut n = Nav3d::new();
        n.set_projection(Projection::Orthographic);
        n.camera.set_view(StandardView::Top);
        n.camera.distance = 2000.0 / (n.camera.fov_y / 2.0).tan(); // half height 2000
        let p = Vec3::new(100.0, 100.0, 0.0);
        let before = screen_of(&n.camera, p);
        n.pan(Vec2::new(30.0, -20.0), 400.0); // 400 points tall: 10 mm per point
        let after = screen_of(&n.camera, p);
        // 30 points are 0.15 of the half height, and the width of the view is 4/3 of the height.
        assert!(
            (after.x - before.x - 30.0 / 200.0 / (SIZE.x / SIZE.y)).abs() < 1e-4,
            "right by 30 points"
        );
        assert!(
            (after.y - before.y - 20.0 / 200.0).abs() < 1e-4,
            "down by 20 points: Y is up in NDC"
        );
    }

    #[test]
    fn zoom_window_centers_and_fills_the_view() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut n = Nav3d::new();
            n.set_projection(projection);
            n.camera.set_view(StandardView::Front);
            let (a, b) = (Vec2::new(500.0, 100.0), Vec2::new(700.0, 250.0));
            // The scene points under the corners and the center of the window.
            let center = target_plane_point(&n.camera, (a + b) / 2.0);
            let ca = target_plane_point(&n.camera, a);
            let cb = target_plane_point(&n.camera, b);
            assert!(
                n.zoom_window(b, a, SIZE),
                "the corners may come in any order"
            );
            let s = screen_of(&n.camera, center);
            assert!(
                s.length() < 2e-3,
                "{projection:?}: the center of the window is the center of the view"
            );
            for c in [ca, cb] {
                let p = screen_of(&n.camera, c);
                assert!(
                    p.x.abs() <= 1.001 && p.y.abs() <= 1.001,
                    "{p:?} inside the view"
                );
            }
            // The wider side of the window fills the view.
            let pa = screen_of(&n.camera, ca);
            let pb = screen_of(&n.camera, cb);
            assert!(((pb.x - pa.x).abs() - 2.0).abs() < 2e-2, "{pa:?} {pb:?}");
        }
        // A click is not a window.
        let mut n = Nav3d::new();
        let before = n.camera;
        assert!(!n.zoom_window(Vec2::new(10.0, 10.0), Vec2::new(11.0, 11.0), SIZE));
        assert!(
            !n.zoom_window(Vec2::new(10.0, 10.0), Vec2::new(300.0, 10.0), SIZE),
            "a line is not a window"
        );
        assert_eq!(n.camera, before);
    }

    #[test]
    fn a_drag_does_what_the_tool_and_the_button_say() {
        use ModelerTool::*;
        let none = Modifiers::NONE;
        let shift = Modifiers::SHIFT;
        let cmd = Modifiers::COMMAND;
        // The middle button navigates in every tool.
        for t in ModelerTool::ALL {
            assert_eq!(
                drag_for(t, Button::Middle, none, false),
                Drag::Orbit,
                "{t:?}"
            );
            assert_eq!(
                drag_for(t, Button::Middle, shift, false),
                Drag::Pan,
                "{t:?}"
            );
        }
        assert_eq!(drag_for(Select, Button::Primary, none, false), Drag::None);
        assert_eq!(drag_for(Select, Button::Primary, shift, false), Drag::None);
        assert_eq!(drag_for(Select, Button::Secondary, none, false), Drag::None);
        assert_eq!(drag_for(Orbit, Button::Primary, none, false), Drag::Orbit);
        assert_eq!(drag_for(Orbit, Button::Primary, shift, false), Drag::Pan);
        assert_eq!(drag_for(Pan, Button::Primary, none, false), Drag::Pan);
        assert_eq!(drag_for(Zoom, Button::Primary, none, false), Drag::Zoom);
        assert_eq!(drag_for(Zoom, Button::Primary, shift, false), Drag::Fov);
        assert_eq!(
            drag_for(Zoom, Button::Primary, cmd, false),
            Drag::ZoomWindow
        );
        assert_eq!(
            drag_for(Zoom, Button::Primary, none, true),
            Drag::ZoomWindow
        );
        assert_eq!(drag_for(Orbit, Button::Secondary, none, false), Drag::None);
        assert_eq!(drag_for(Line, Button::Primary, none, false), Drag::None);
    }

    #[test]
    fn dragging_with_the_zoom_tool_zooms_in_going_up_and_changes_the_field_of_view() {
        let mut n = Nav3d::new();
        let d = n.camera.distance;
        n.drag(Drag::Zoom, Vec2::new(0.0, -50.0), SIZE, None);
        let zoomed = n.camera.distance;
        assert!(zoomed < d, "up zooms in");
        n.drag(Drag::Zoom, Vec2::new(0.0, 100.0), SIZE, None);
        assert!(n.camera.distance > zoomed, "down zooms out");
        let fov = n.fov_deg();
        n.drag(Drag::Fov, Vec2::new(0.0, -100.0), SIZE, None);
        assert!(n.fov_deg() < fov, "up narrows the angle");
        // Window and none drags change nothing.
        let kept = n.camera;
        n.drag(Drag::ZoomWindow, Vec2::new(5.0, 5.0), SIZE, None);
        n.drag(Drag::None, Vec2::new(5.0, 5.0), SIZE, None);
        assert_eq!(n.camera, kept);
    }

    #[test]
    fn keys_map_to_navigation() {
        assert_eq!(nav_action(Action::ZoomFit), Some(NavAction::ZoomExtents));
        assert_eq!(
            nav_action(Action::ViewRightIso),
            Some(NavAction::View(StandardView::RightIsometric))
        );
        assert_eq!(
            nav_action(Action::ViewBottom),
            Some(NavAction::View(StandardView::Bottom))
        );
        assert_eq!(
            nav_action(Action::ViewPlan),
            Some(NavAction::View(StandardView::Top))
        );
        assert_eq!(
            nav_action(Action::ViewProjection),
            Some(NavAction::ToggleProjection)
        );
        assert_eq!(
            nav_action(Action::ViewHistoryPrevious),
            Some(NavAction::Previous)
        );
        assert_eq!(nav_action(Action::Undo), None);
        assert_eq!(nav_action(Action::ToolLine), None);
        let mut n = Nav3d::new();
        let f = frame();
        let d = n.camera.distance;
        n.apply(NavAction::ZoomIn, &f);
        assert!((n.camera.distance - d / KEY_ZOOM).abs() < 1e-2);
        n.apply(NavAction::ZoomOut, &f);
        assert!((n.camera.distance - d).abs() < 1e-1);
        let t = n.camera.target;
        n.apply(nav_action(Action::PanLeft).unwrap(), &f);
        assert!(
            (n.camera.target - t).dot(n.camera.right()) < 0.0,
            "the view moves left, the scene follows the key to the right"
        );
        n.apply(nav_action(Action::PanRight).unwrap(), &f);
        assert!((n.camera.target - t).length() < 1e-2);
    }

    #[test]
    fn a_view_change_is_one_entry_and_previous_and_next_walk_the_entries() {
        let mut n = Nav3d::new();
        let f = frame();
        let mut t = 0.0;
        let rest = |n: &mut Nav3d, t: &mut f64| {
            n.observe(*t);
            *t += SETTLE + 0.1;
            n.observe(*t);
        };
        assert!(!n.can_go_back());
        rest(&mut n, &mut t);
        let first = n.camera;
        // A wheel turn is many small steps and one entry.
        for _ in 0..20 {
            n.zoom_at(1.05, Vec2::new(300.0, 300.0), SIZE);
            t += 0.016;
            n.observe(t);
        }
        rest(&mut n, &mut t);
        let second = n.camera;
        n.apply(NavAction::View(StandardView::Top), &f);
        rest(&mut n, &mut t);
        let third = n.camera;
        assert!(n.can_go_back() && !n.can_go_forward());
        assert!(n.previous());
        assert_eq!(n.camera, second);
        assert!(n.previous());
        assert_eq!(n.camera, first);
        assert!(!n.previous(), "no view before the first");
        assert!(n.next());
        assert_eq!(n.camera, second);
        assert!(n.next());
        assert_eq!(n.camera, third);
        assert!(!n.next());
        // A change that has not rested is not lost when you go back: Next returns to it.
        n.orbit(Vec2::new(40.0, 0.0));
        let fresh = n.camera;
        assert!(n.previous());
        assert_eq!(n.camera, third);
        assert!(n.next());
        assert_eq!(n.camera, fresh);
        // The history can be cleared (a new document).
        n.clear_history();
        assert!(!n.can_go_back() && !n.can_go_forward());
    }

    #[test]
    fn the_history_covers_the_projection_and_the_field_of_view() {
        let mut n = Nav3d::new();
        let mut t = 0.0;
        n.observe(t);
        t += SETTLE + 0.1;
        n.observe(t);
        n.toggle_projection();
        n.set_fov_deg(70.0);
        assert!(n.previous());
        assert!(n.is_perspective());
        assert!((n.fov_deg() - DEFAULT_FOV_DEG).abs() < 1e-3);
        assert!(n.next());
        assert!(!n.is_perspective() && (n.fov_deg() - 70.0).abs() < 1e-3);
    }
}
