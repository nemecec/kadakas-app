//! The 3D canvas: the scene drawn by the GPU (see `gpu3d`), with orbit, pan, zoom to the
//! pointer and click to select. What it shows is chosen in `Session::view`.

use eframe::egui_wgpu;
use egui::Sense;
use kadakas_api::Session;
use kadakas_render::Projection;
use kadakas_render::styled::{light_for, resolve_style};

use crate::gpu3d::{GpuStyle, SceneCallback};
use crate::keymap::{Action, Keymap};
use crate::view3d::{self, View3d};

/// What the canvas needs from the app.
pub struct Canvas3d<'a> {
    pub session: &'a mut Session,
    pub view3d: &'a mut View3d,
    pub keys: &'a Keymap,
    /// The GPU renderer is available. Without it the canvas only explains that.
    pub gpu_ready: bool,
    /// Zoom factor from keys that is not applied yet (1 is none).
    pub pending_zoom: &'a mut f64,
    /// Pan from keys in points, not applied yet.
    pub pending_pan: &'a mut egui::Vec2,
}

/// Draws the canvas and handles its input for one frame.
pub fn show(ui: &mut egui::Ui, c: Canvas3d) {
    let Canvas3d {
        session,
        view3d,
        keys,
        gpu_ready,
        pending_zoom,
        pending_pan,
    } = c;
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    if !gpu_ready {
        let text = format!(
            "The 3D view needs the wgpu renderer. Press {} for Top/Plan.",
            keys.hint(Action::ViewPlan)
        );
        ui.painter_at(rect).text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(16.0),
            ui.visuals().text_color(),
        );
        return;
    }
    let size = glam::Vec2::new(rect.width().max(1.0), rect.height().max(1.0));
    let aspect = size.x / size.y;
    // Plants far from the camera turn into coarse meshes and billboards (`lod_for`).
    let pixel_size = f64::from(view3d.camera.half_height() * 2.0 / size.y);
    let scene = session.scene_for_pixel_size(pixel_size);
    let rev = session.editor.revision();
    if view3d.fit_pending() {
        view3d.fit(scene.bounds, aspect);
    }

    let (hover, mods, space, scroll, pinch) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.modifiers,
            crate::keymap::held(keys, Action::PanHold, i),
            i.smooth_scroll_delta.y,
            i.zoom_delta(),
        )
    });
    let local = |p: egui::Pos2| glam::Vec2::new(p.x - rect.min.x, p.y - rect.min.y);

    // Keys: zoom and pan.
    if *pending_zoom != 1.0 {
        let factor = std::mem::replace(pending_zoom, 1.0) as f32;
        view3d.zoom_at(factor, size / 2.0, size);
    }
    if *pending_pan != egui::Vec2::ZERO {
        let d = std::mem::take(pending_pan);
        view3d.pan(glam::Vec2::new(d.x, d.y), size.y);
    }

    // Mouse: drag orbits, Shift-drag, Space-drag and middle-drag pan, the wheel zooms to the pointer.
    let delta = glam::Vec2::new(resp.drag_delta().x, resp.drag_delta().y);
    if resp.dragged_by(egui::PointerButton::Middle)
        || (resp.dragged_by(egui::PointerButton::Primary) && (mods.shift || space))
    {
        view3d.pan(delta, size.y);
    } else if resp.dragged_by(egui::PointerButton::Primary) {
        view3d.orbit(delta);
    }
    if let (true, Some(h)) = (resp.hovered(), hover) {
        let factor = ((scroll as f64 * 0.0025).exp() * pinch as f64) as f32;
        if (factor - 1.0).abs() > 1e-6 {
            view3d.zoom_at(factor, local(h), size);
        }
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let hit = view3d.pick(&scene, local(p), size);
        session.selection = view3d::click_selection(&session.selection, hit, mods.shift);
    }

    let cam = view3d.camera;
    let bounds = scene.bounds;
    let (overlay, overlay_rev) = view3d.selection_lines(&scene, rev, &session.selection);
    // The look: the active render style of the document, with the mode of the view bar. Materials
    // are resolved again only when the document changes.
    let doc = session.editor.doc();
    let mut style = resolve_style(doc, None);
    style.mode = session.view.mode;
    let look = view3d.look(doc, &scene, rev, style.textures);
    let geometry = view3d.geometry_key(&scene, rev);
    let light = light_for(doc, &style);
    let gpu_style = GpuStyle {
        mode: style.mode,
        edges: style.edges,
        shadows: style.shadows && matches!(light, kadakas_render::shade::Light::Sun(_)),
        light,
        look: Some(look),
    };
    let bg = style.background.0;
    let ppp = ui.ctx().pixels_per_point();
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        SceneCallback {
            scene,
            scene_rev: geometry,
            overlay,
            overlay_rev,
            mvp: cam.view_proj(aspect, bounds).to_cols_array_2d(),
            eye: cam.eye().to_array(),
            eye_dir: cam.eye_dir().to_array(),
            ortho: cam.projection == Projection::Orthographic,
            style: gpu_style,
            size: [
                (rect.width() * ppp).round().max(1.0) as u32,
                (rect.height() * ppp).round().max(1.0) as u32,
            ],
            background: [
                bg[0] as f64 / 255.0,
                bg[1] as f64 / 255.0,
                bg[2] as f64 / 255.0,
            ],
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::command::Command;
    use kadakas_core::model::{ObjectId, Shape};
    use kadakas_render::StandardView;

    /// The parts that the canvas needs, without the rest of the app.
    struct Harness {
        session: Session,
        view3d: View3d,
        keys: Keymap,
        gpu_ready: bool,
        pending_zoom: f64,
        pending_pan: egui::Vec2,
        ctx: egui::Context,
    }

    impl Harness {
        fn new() -> Harness {
            Harness {
                session: Session::default(),
                view3d: View3d::default(),
                keys: Keymap::defaults(),
                gpu_ready: true, // the paint callback is only recorded in these tests
                pending_zoom: 1.0,
                pending_pan: egui::Vec2::ZERO,
                ctx: egui::Context::default(),
            }
        }

        fn add_slab(&mut self) -> ObjectId {
            let out = self
                .session
                .editor
                .execute(&Command::AddObject {
                    shape: Shape::Hardscape {
                        boundary: kadakas_core::curve::corners(&[
                            DVec2::splat(-1000.0),
                            DVec2::new(1000.0, -1000.0),
                            DVec2::splat(1000.0),
                            DVec2::new(-1000.0, 1000.0),
                        ]),
                        material: "m".into(),
                        thickness: None,
                        elevation: None,
                    },
                    layer: None,
                    class: None,
                    attrs: Default::default(),
                })
                .unwrap();
            ObjectId(out.created[0])
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.frame_with(events, egui::Modifiers::NONE);
        }

        /// One frame of the canvas in an 800 x 600 window, like the app runs it.
        fn frame_with(&mut self, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
            let input = egui::RawInput {
                events,
                modifiers,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            };
            let ctx = self.ctx.clone();
            let _ = ctx.run_ui(input, |ui| {
                let forced = self.session.take_view_changed();
                self.view3d.sync(self.session.view, forced);
                egui::CentralPanel::no_frame().show(ui, |ui| {
                    show(
                        ui,
                        Canvas3d {
                            session: &mut self.session,
                            view3d: &mut self.view3d,
                            keys: &self.keys,
                            gpu_ready: self.gpu_ready,
                            pending_zoom: &mut self.pending_zoom,
                            pending_pan: &mut self.pending_pan,
                        },
                    );
                });
            });
        }

        /// A drag with the left button over several frames, from `from` by `by`.
        fn drag(&mut self, from: egui::Pos2, by: egui::Vec2, mods: egui::Modifiers) {
            let button = |pressed, pos| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: mods,
            };
            self.frame_with(vec![egui::Event::PointerMoved(from)], mods);
            self.frame_with(vec![button(true, from)], mods);
            for k in 1..=4 {
                self.frame_with(
                    vec![egui::Event::PointerMoved(from + by * (k as f32 / 4.0))],
                    mods,
                );
            }
            self.frame_with(vec![button(false, from + by)], mods);
        }

        /// A click in three frames: move, press, release.
        fn click(&mut self, at: egui::Pos2, mods: egui::Modifiers) {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: mods,
            };
            self.frame_with(vec![egui::Event::PointerMoved(at)], mods);
            self.frame_with(vec![button(true)], mods);
            self.frame_with(vec![button(false)], mods);
        }
    }

    #[test]
    fn without_a_gpu_the_canvas_explains_itself() {
        let mut h = Harness::new();
        h.gpu_ready = false;
        h.session.view.view = StandardView::Front;
        h.frame(vec![]);
        assert!(
            h.view3d.fit_pending(),
            "nothing else happens without a renderer"
        );
    }

    #[test]
    fn clicking_selects_through_the_core_mesh() {
        let mut h = Harness::new();
        let id = h.add_slab();
        h.session.view.view = StandardView::Front;
        h.frame(vec![]);
        let none = egui::Modifiers::NONE;
        // The panel is the whole window, so the middle of the window shows the middle of the slab.
        h.click(egui::pos2(400.0, 300.0), none);
        assert_eq!(h.session.selection, vec![id]);
        // A Shift-click on empty space keeps the selection; a plain click clears it.
        h.click(egui::pos2(5.0, 5.0), egui::Modifiers::SHIFT);
        assert_eq!(h.session.selection, vec![id]);
        h.click(egui::pos2(5.0, 5.0), none);
        assert!(h.session.selection.is_empty());
        // Shift-click on the slab toggles it.
        h.click(egui::pos2(400.0, 300.0), egui::Modifiers::SHIFT);
        assert_eq!(h.session.selection, vec![id]);
        h.click(egui::pos2(400.0, 300.0), egui::Modifiers::SHIFT);
        assert!(h.session.selection.is_empty());
    }

    #[test]
    fn mouse_and_keys_navigate() {
        let mut h = Harness::new();
        h.add_slab();
        h.session.view.view = StandardView::Front;
        h.frame(vec![]);
        let none = egui::Modifiers::NONE;
        let center = egui::pos2(400.0, 300.0);
        // Drag orbits.
        let before = h.view3d.camera;
        h.drag(center, egui::vec2(80.0, 40.0), none);
        assert_ne!(h.view3d.camera.yaw, before.yaw);
        assert_ne!(h.view3d.camera.pitch, before.pitch);
        // Shift-drag pans: the angles stay, the target moves.
        let (angles, target) = (
            (h.view3d.camera.yaw, h.view3d.camera.pitch),
            h.view3d.camera.target,
        );
        h.drag(center, egui::vec2(80.0, 40.0), egui::Modifiers::SHIFT);
        assert_eq!((h.view3d.camera.yaw, h.view3d.camera.pitch), angles);
        assert!((h.view3d.camera.target - target).length() > 1.0);
        // The wheel zooms in toward the pointer.
        let d = h.view3d.camera.distance;
        h.frame(vec![
            egui::Event::PointerMoved(center),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 120.0),
                phase: egui::TouchPhase::Move,
                modifiers: none,
            },
        ]);
        h.frame(vec![]);
        assert!(h.view3d.camera.distance < d, "wheel up zooms in");
        // Keys: zoom and pan reach the camera too.
        h.pending_zoom = 2.0;
        h.pending_pan = egui::vec2(60.0, 0.0);
        let (t, d) = (h.view3d.camera.target, h.view3d.camera.distance);
        h.frame(vec![]);
        assert!(
            h.view3d.camera.distance <= d / 2.0 + 1e-2,
            "the key halves the distance"
        );
        assert!((h.view3d.camera.target - t).length() > 1.0);
        assert_eq!((h.pending_zoom, h.pending_pan), (1.0, egui::Vec2::ZERO));
    }

    #[test]
    fn the_scene_is_built_again_only_when_the_document_changes() {
        let mut h = Harness::new();
        h.add_slab();
        h.session.view.view = StandardView::RightIsometric;
        for _ in 0..5 {
            h.frame(vec![]);
        }
        let rev = h.session.editor.revision();
        let a = h.session.scene();
        h.frame(vec![]);
        assert!(
            std::sync::Arc::ptr_eq(&a, &h.session.scene()),
            "unchanged document: same mesh"
        );
        assert_eq!(h.session.editor.revision(), rev);
        h.add_slab();
        assert!(!std::sync::Arc::ptr_eq(&a, &h.session.scene()));
    }

    #[test]
    fn a_window_of_zero_width_does_not_break_the_camera() {
        let mut h = Harness::new();
        h.add_slab();
        h.session.view.view = StandardView::Front;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(0.0, 300.0),
            )),
            ..Default::default()
        };
        let ctx = h.ctx.clone();
        let _ = ctx.run_ui(input, |ui| {
            show(
                ui,
                Canvas3d {
                    session: &mut h.session,
                    view3d: &mut h.view3d,
                    keys: &h.keys,
                    gpu_ready: true,
                    pending_zoom: &mut h.pending_zoom,
                    pending_pan: &mut h.pending_pan,
                },
            );
        });
        let c = h.view3d.camera;
        assert!(
            c.target.is_finite() && c.distance.is_finite() && c.distance > 0.0,
            "{c:?}"
        );
    }
}
