//! The 3D canvas: the scene drawn by the GPU (see `gpu3d`), with orbit, pan, zoom to the
//! pointer and click to select. What it shows is chosen in `Session::view`.

use eframe::egui_wgpu;
use egui::Sense;
use kadakas_api::Session;
use kadakas_l10n::tr;
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
    /// The scene of a compare (REV-03): tinted objects, and the ghosts of removed ones. It is
    /// drawn instead of the scene of the session, with its own look, and nothing is selected.
    pub compare: Option<std::sync::Arc<crate::compare_ui::Compare3d>>,
    /// An object to zoom to (a click in the Compare window).
    pub frame: Option<kadakas_core::model::ObjectId>,
    /// The pins of the issues (REV-05): drawn over the scene, and a click on one opens its issue.
    pub issues: Option<&'a mut crate::issues_ui::IssuesUi>,
    /// Tells the app that the person navigates (the agent never wins against that).
    pub nav: &'a mut crate::agent_view::NavSignal,
    /// What an agent shows: the pointers and the agent cursor (AGR-03).
    pub agent: Option<&'a crate::agent_view::AgentView>,
    /// The changes of the agent that the person has not seen (AGR-05): boxes with a pattern and a
    /// mark for each status. `None`: off.
    pub review: Option<std::sync::Arc<kadakas_ui_core::agent_activity::Review>>,
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
        compare,
        frame,
        mut issues,
        nav,
        agent,
        review,
    } = c;
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    if !gpu_ready {
        let text = tr!(
            "canvas3d-the-3d-view-needs-the-wgpu-renderer",
            key = keys.hint(Action::ViewPlan)
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
    let (scene, rev) = match &compare {
        Some(c) => (c.scene.clone(), c.key),
        None => (
            session.scene_for_pixel_size(pixel_size),
            session.editor.revision(),
        ),
    };
    if view3d.fit_pending() {
        view3d.fit(scene.bounds, aspect);
    }
    if let Some(id) = frame
        && let Some(b) = scene.object_bounds(id)
    {
        view3d.fit(Some(b), aspect);
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
    nav.moved |= *pending_zoom != 1.0 || *pending_pan != egui::Vec2::ZERO;
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
        nav.moved = true;
        nav.dragging = true;
    } else if resp.dragged_by(egui::PointerButton::Primary) {
        view3d.orbit(delta);
        nav.moved = true;
        nav.dragging = true;
    }
    if let (true, Some(h)) = (resp.hovered(), hover) {
        let factor = ((scroll as f64 * 0.0025).exp() * pinch as f64) as f32;
        if (factor - 1.0).abs() > 1e-6 {
            view3d.zoom_at(factor, local(h), size);
            nav.moved = true;
        }
    }
    // A click on a pin opens its issue.
    let mut pin_clicked = false;
    if resp.clicked()
        && compare.is_none()
        && let (Some(ui_issues), Some(p)) = (issues.as_deref_mut(), resp.interact_pointer_pos())
        && let Some(id) = ui_issues.pin_at(p)
    {
        ui_issues.click_pin(id);
        pin_clicked = true;
    }
    // A compare is read only: a click selects nothing.
    if resp.clicked()
        && !pin_clicked
        && compare.is_none()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let hit = view3d.pick(&scene, local(p), size);
        // A ghost is not in the drawing: it cannot be selected. A member of a group selects the
        // group, as in the plan, unless you are in the group (MOD-13).
        let context = session.edit_context();
        // The same rule as in the plan: grey and locked things cannot be picked.
        let hit = hit
            .filter(|id| {
                let doc = session.editor.doc();
                doc.object(*id)
                    .is_some_and(|o| kadakas_ui_core::hit::pickable(doc, o))
            })
            .and_then(|id| session.editor.doc().selection_target(id, context));
        session.selection = view3d::click_selection(&session.selection, hit, mods.shift);
    }

    let cam = view3d.camera;
    let bounds = scene.bounds;
    let selection: &[kadakas_core::model::ObjectId] = if compare.is_some() {
        &[]
    } else {
        &session.selection
    };
    let (overlay, overlay_rev) = view3d.selection_lines(&scene, rev, selection);
    // The look: the active render style of the document, with the mode of the view bar. Materials
    // are resolved again only when the document changes.
    let doc = session.editor.doc();
    let mut style = resolve_style(doc, None);
    style.mode = session.view.mode;
    let look = match &compare {
        Some(c) => c.look.clone(),
        None => view3d.look(doc, &scene, rev, style.textures),
    };
    let geometry = view3d.geometry_key(&scene, rev);
    let not_in_3d = view3d.untextured(doc, rev);
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
    let view_proj = cam.view_proj(aspect, bounds);
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        SceneCallback {
            scene: scene.clone(),
            scene_rev: geometry,
            overlay,
            overlay_rev,
            mvp: view_proj.to_cols_array_2d(),
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
    if not_in_3d > 0 {
        ui.painter_at(rect).text(
            rect.left_bottom() + egui::vec2(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            tr!("canvas3d-pattern-not-drawn-in-3d", count = not_in_3d),
            egui::FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
    }
    if compare.is_none()
        && let Some(issues) = issues
    {
        issues.paint_3d(&ui.painter_at(rect), rect, view_proj, scene.origin, session);
    }
    // What the agent changed and the person has not seen (AGR-05; never part of the document).
    if compare.is_none()
        && let Some(review) = &review
    {
        crate::agent_activity_ui::paint_3d(
            &ui.painter_at(rect),
            rect,
            view_proj,
            &scene,
            session.editor.doc(),
            review,
        );
    }
    // What the agent points at, and where the agent is (never part of the document).
    if compare.is_none()
        && let Some(agent) = agent
    {
        let painter = ui.painter_at(rect);
        crate::agent_view_ui::paint_3d(
            &painter,
            rect,
            view_proj,
            &scene,
            session.editor.doc(),
            &agent.highlights,
        );
        let now_ms = ui.input(|i| i.time) * 1000.0;
        if let Some(peer) = agent.presence.peer(now_ms, tr!("agent-view-name")) {
            crate::agent_view_ui::paint_agent_cursor_3d(
                &painter,
                rect,
                view_proj,
                scene.origin,
                cam.target.z,
                &peer,
            );
        }
    }
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
        issues: crate::issues_ui::IssuesUi,
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
                issues: Default::default(),
            }
        }

        fn add_slab(&mut self) -> ObjectId {
            self.add_slab_at(DVec2::ZERO)
        }

        /// A slab of 2000 x 2000 mm around `c`.
        fn add_slab_at(&mut self, c: DVec2) -> ObjectId {
            let out = self
                .session
                .editor
                .execute(&Command::AddObject {
                    shape: Shape::Hardscape {
                        boundary: kadakas_core::curve::corners(&[
                            c + DVec2::splat(-1000.0),
                            c + DVec2::new(1000.0, -1000.0),
                            c + DVec2::splat(1000.0),
                            c + DVec2::new(-1000.0, 1000.0),
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
                            compare: None,
                            frame: None,
                            issues: Some(&mut self.issues),
                            nav: &mut Default::default(),
                            agent: None,
                            review: None,
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

    /// A closed box of 2000 x 2000 x 1000 mm around the origin (a square pulled up).
    fn add_surface_box(h: &mut Harness) -> ObjectId {
        use kadakas_core::surface::Edit;
        let id = ObjectId(
            h.session
                .editor
                .execute(&Command::AddObject {
                    shape: Shape::SurfaceModel(Box::default()),
                    layer: None,
                    class: None,
                    attrs: Default::default(),
                })
                .unwrap()
                .created[0],
        );
        let p = [
            [-1000.0, -1000.0, 0.0],
            [1000.0, -1000.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [-1000.0, 1000.0, 0.0],
        ];
        h.session
            .editor
            .execute(&Command::EditSurface {
                id,
                edit: Edit::AddEdges {
                    edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
                },
            })
            .unwrap();
        let face = match &h.session.editor.doc().object(id).unwrap().shape {
            Shape::SurfaceModel(m) => m.faces().next().unwrap().0,
            _ => unreachable!(),
        };
        h.session
            .editor
            .execute(&Command::EditSurface {
                id,
                edit: Edit::PushPull {
                    face,
                    distance: 1000.0,
                },
            })
            .unwrap();
        id
    }

    #[test]
    fn a_click_in_3d_selects_a_surface_model_an_instance_and_a_group() {
        let mut h = Harness::new();
        let id = add_surface_box(&mut h);
        h.session.view.view = StandardView::Front;
        h.frame(vec![]);
        let none = egui::Modifiers::NONE;
        h.click(egui::pos2(400.0, 300.0), none);
        assert_eq!(h.session.selection, vec![id]);
        h.click(egui::pos2(5.0, 5.0), none);
        assert!(h.session.selection.is_empty());
        // An instance of a component made from the model is picked in its place.
        let out = h
            .session
            .editor
            .execute(&Command::MakeComponent {
                name: "Box".into(),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![id],
                replace: true,
            })
            .unwrap();
        let inst = ObjectId(out.created[1]);
        h.frame(vec![]);
        h.click(egui::pos2(400.0, 300.0), none);
        assert_eq!(h.session.selection, vec![inst]);
        // A model in a group selects the group.
        let mut h = Harness::new();
        let a = add_surface_box(&mut h);
        let g = ObjectId(
            h.session
                .editor
                .execute(&Command::Group {
                    ids: vec![a],
                    name: None,
                })
                .unwrap()
                .created[0],
        );
        h.session.view.view = StandardView::Front;
        h.frame(vec![]);
        h.click(egui::pos2(400.0, 300.0), none);
        assert_eq!(h.session.selection, vec![g]);
    }

    #[test]
    fn a_click_in_3d_cannot_pick_a_grey_class_or_a_locked_layer() {
        use kadakas_core::visibility::VisMode;
        let mut h = Harness::new();
        let class = kadakas_core::model::ClassId(
            h.session
                .editor
                .execute(&Command::AddClass {
                    name: "Sheds".into(),
                    attrs: Default::default(),
                })
                .unwrap()
                .created[0],
        );
        let id = add_surface_box(&mut h);
        h.session
            .editor
            .execute(&Command::ModifyObject {
                id,
                shape: None,
                layer: None,
                class: Some(class),
                attrs: None,
            })
            .unwrap();
        h.session.view.view = StandardView::Front;
        h.frame(vec![]);
        let none = egui::Modifiers::NONE;
        h.click(egui::pos2(400.0, 300.0), none);
        assert_eq!(h.session.selection, vec![id]);
        h.click(egui::pos2(5.0, 5.0), none);
        // Grey: drawn, not picked.
        h.session
            .editor
            .execute(&Command::SetVisibility {
                layers: vec![],
                classes: vec![class],
                mode: VisMode::Grey,
            })
            .unwrap();
        h.frame(vec![]);
        h.click(egui::pos2(400.0, 300.0), none);
        assert!(h.session.selection.is_empty(), "a grey class is not picked");
        h.session
            .editor
            .execute(&Command::SetVisibility {
                layers: vec![],
                classes: vec![class],
                mode: VisMode::Visible,
            })
            .unwrap();
        // A locked layer is not picked either.
        let layer = h.session.editor.doc().object(id).unwrap().layer;
        h.session
            .editor
            .execute(&Command::ModifyLayer {
                id: layer,
                name: None,
                visible: None,
                locked: Some(true),
                elevation: None,
                scale: None,
            })
            .unwrap();
        h.frame(vec![]);
        h.click(egui::pos2(400.0, 300.0), none);
        assert!(
            h.session.selection.is_empty(),
            "a locked layer is not picked"
        );
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
    fn a_pin_is_drawn_in_3d_and_a_click_on_it_opens_the_issue_instead_of_selecting() {
        let mut h = Harness::new();
        let slab = h.add_slab();
        h.session.view.view = StandardView::RightIsometric;
        let made = h
            .session
            .handle(
                "issue.create",
                serde_json::json!({"title": "Here", "objects": [slab], "now_ms": 1}),
            )
            .unwrap();
        let id: kadakas_core::issue::IssueId = serde_json::from_value(made["id"].clone()).unwrap();
        for _ in 0..4 {
            h.frame(vec![]);
        }
        let at = h.issues.pin_screen(id).expect("the pin is on the screen");
        assert!(at.x > 0.0 && at.y > 0.0);
        h.click(at, egui::Modifiers::NONE);
        assert_eq!(h.issues.selected, Some(id), "the pin was clicked");
        assert!(
            h.session.selection.is_empty(),
            "the click did not select the slab"
        );
        let requests = h.issues.take_requests();
        assert_eq!(requests.len(), 1);
    }

    #[test]
    fn pins_are_projected_relative_to_the_scene_origin() {
        // The scene (and so the camera) is relative to the middle of the drawing. A pin has
        // drawing coordinates; a drawing far from zero must still put the pin on its object.
        let mut h = Harness::new();
        let slab = h.add_slab_at(DVec2::new(650_000.0, 590_000.0));
        h.session.view.view = StandardView::RightIsometric;
        let made = h
            .session
            .handle(
                "issue.create",
                serde_json::json!({"title": "Far", "objects": [slab], "now_ms": 1}),
            )
            .unwrap();
        let id: kadakas_core::issue::IssueId = serde_json::from_value(made["id"].clone()).unwrap();
        for _ in 0..4 {
            h.frame(vec![]);
        }
        let at = h.issues.pin_screen(id).expect("the pin is on the screen");
        // The only object is in the middle of the window.
        assert!(
            at.distance(egui::pos2(400.0, 300.0)) < 160.0,
            "the pin is at {at:?}, far from the object in the middle"
        );
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
                    compare: None,
                    frame: None,
                    issues: None,
                    nav: &mut Default::default(),
                    agent: None,
                    review: None,
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
