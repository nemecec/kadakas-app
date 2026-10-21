//! The 3D view: the scene drawn by the GPU (`kadakas-view3d`), the drawing axes, and the mouse.
//! What a drag does is decided by `kadakas_ui_core::nav3d::drag_for`; this module reads the
//! pointer, calls the navigation and draws.
//!
//! - The middle button orbits, with Shift it pans, in every tool. The wheel zooms to the pointer.
//! - The Orbit, Pan and Zoom tools use the primary button (Shift and Ctrl/Cmd change what the Zoom
//!   tool does).
//! - The Select tool selects with a click.
//!
//! The SceneCallback and the shaders are the ones of `kadakas-app`.

use eframe::egui_wgpu;
use egui::{Color32, Pos2, Rect, Sense, Stroke};
use glam::{Vec2, Vec3};
use kadakas_api::Session;
use kadakas_l10n::tr;
use kadakas_render::Projection;
use kadakas_render::styled::{light_for, resolve_style};
use kadakas_ui_core::modeler::ToolState;
use kadakas_ui_core::nav3d::{self, Button, Drag, Frame, Nav3d};
use kadakas_view3d::gpu3d::{GpuStyle, SceneCallback};
use kadakas_view3d::input::modifiers;
use kadakas_view3d::view3d::View3d;

use crate::app::Cmd;
use crate::edit3d::{Edit3d, Input};

/// The colors of the drawing axes: red is the right-left axis, green runs into the screen and blue
/// is up.
pub const AXIS_RED: Color32 = Color32::from_rgb(214, 62, 62);
pub const AXIS_GREEN: Color32 = Color32::from_rgb(70, 170, 70);
pub const AXIS_BLUE: Color32 = Color32::from_rgb(64, 110, 220);

const ACTIVE_KEY: &str = "modeler-active-drag";
const WINDOW_KEY: &str = "modeler-zoom-window";

/// The drag that runs: what it does (decided when it started), the button, and the point of the
/// model under the pointer at the start.
#[derive(Clone, Copy, Debug, Default)]
struct ActiveDrag {
    middle: bool,
    kind: Drag,
    pivot: Option<Vec3>,
}

/// Forgets the drag and its window.
fn clear_drag(ui: &egui::Ui) {
    ui.memory_mut(|m| {
        m.data.remove_temp::<ActiveDrag>(ACTIVE_KEY.into());
        m.data.remove_temp::<WindowDrag>(WINDOW_KEY.into());
    });
}

/// The window that a Zoom Window drag shows now, if one is being drawn.
#[cfg(test)]
pub fn window_drag(ctx: &egui::Context) -> Option<WindowDrag> {
    ctx.memory(|m| m.data.get_temp::<WindowDrag>(WINDOW_KEY.into()))
}

/// What the canvas needs from the program.
pub struct Canvas<'a> {
    pub session: &'a mut Session,
    pub view3d: &'a mut View3d,
    pub nav: &'a mut Nav3d,
    pub tools: &'a mut ToolState,
    /// Modeling: the drawing tools, the selection of parts and the inference.
    pub edit: &'a mut Edit3d,
    /// The GPU renderer is available. Without it the canvas only explains that.
    pub gpu_ready: bool,
    /// Show the whole model at the next frame (after an open, and at the start).
    pub fit_pending: &'a mut bool,
    /// Whether Intersect Faces With Model and With Selection are enabled (the context menu).
    pub intersect: [bool; 2],
}

/// What the canvas tells the program.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// The rectangle of the canvas.
    pub rect: Option<Rect>,
    /// The person changed the view in this frame.
    pub navigated: bool,
    /// A command of the context menu.
    pub command: Option<Cmd>,
}

/// The window that a Zoom Window drag draws, in points from the top left of the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowDrag {
    pub from: Vec2,
    pub to: Vec2,
}

/// The ndc of a point in the canvas (points from the top left), Y up.
#[cfg(test)]
pub fn ndc_of(at: Vec2, size: Vec2) -> Vec2 {
    Vec2::new(
        at.x / size.x.max(1.0) * 2.0 - 1.0,
        1.0 - at.y / size.y.max(1.0) * 2.0,
    )
}

/// The screen position of a point of the scene (local coordinates), if it is in front of the camera.
fn to_screen(
    nav: &Nav3d,
    p: Vec3,
    rect: Rect,
    aspect: f32,
    bounds: Option<kadakas_core::mesh::Bounds3>,
) -> Option<Pos2> {
    let n = nav.camera.project(p, aspect, bounds)?;
    Some(Pos2::new(
        rect.min.x + (n.x + 1.0) / 2.0 * rect.width(),
        rect.min.y + (1.0 - n.y) / 2.0 * rect.height(),
    ))
}

/// The three axes through the origin of the drawing: the positive half solid, the negative half
/// faint. `origin` is the world position of the local point (0, 0, 0).
pub fn axis_segments(origin: glam::DVec3, length: f32) -> Vec<(Vec3, Vec3, Color32, bool)> {
    let o = (-origin).as_vec3();
    [
        (Vec3::X, AXIS_RED),
        (Vec3::Y, AXIS_GREEN),
        (Vec3::Z, AXIS_BLUE),
    ]
    .iter()
    .flat_map(|(axis, color)| {
        [
            (o, o + *axis * length, *color, true),
            (o, o - *axis * length, *color, false),
        ]
    })
    .filter(|(a, b, _, _)| a.is_finite() && b.is_finite())
    .collect()
}

/// Draws the canvas and handles its input for one frame.
pub fn show(ui: &mut egui::Ui, c: Canvas) -> Shown {
    let Canvas {
        session,
        view3d,
        nav,
        tools,
        edit,
        gpu_ready,
        fit_pending,
        intersect,
    } = c;
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let mut shown = Shown {
        rect: Some(rect),
        navigated: false,
        command: None,
    };
    if !gpu_ready {
        ui.painter_at(rect).text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr!("modeler-needs-wgpu"),
            egui::FontId::proportional(16.0),
            ui.visuals().text_color(),
        );
        return shown;
    }
    let size = Vec2::new(rect.width().max(1.0), rect.height().max(1.0));
    let aspect = size.x / size.y;
    let pixel_size = f64::from(nav.camera.half_height() * 2.0 / size.y);
    let scene = session.scene_for_pixel_size(pixel_size);
    edit.keep_view(scene.origin, nav);
    edit.follow(tools.active());
    let rev = session.editor.revision();
    let frame = Frame {
        bounds: scene.bounds,
        size,
    };
    if *fit_pending {
        nav.zoom_extents(&frame);
        *fit_pending = false;
        shown.navigated = true;
    }

    let (hover, mods, scroll, pinch) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            modifiers(i.modifiers),
            i.smooth_scroll_delta.y,
            i.zoom_delta(),
        )
    });
    let local = |p: Pos2| Vec2::new(p.x - rect.min.x, p.y - rect.min.y);

    // The wheel zooms to the pointer. In the perspective projection the point of the model under
    // the pointer stays under it (the zoom goes toward that point), with no stop at a fixed
    // distance.
    if let (true, Some(h)) = (resp.hovered(), hover) {
        let factor = nav3d::wheel_factor(scroll, pinch);
        if (factor - 1.0).abs() > 1e-6 {
            let focus = if nav.is_perspective() {
                nav3d::point_under(&scene, &nav.camera, local(h), size)
            } else {
                None
            };
            nav.zoom_at_point(factor, local(h), size, focus);
            shown.navigated = true;
        }
    }

    // Drags. What a drag does is decided once, when it starts, from the tool, the button and the
    // keys held at that moment. Letting go of a key later does not change the drag.
    for (button, egui_button) in [
        (Button::Primary, egui::PointerButton::Primary),
        (Button::Middle, egui::PointerButton::Middle),
    ] {
        if resp.drag_started_by(egui_button) {
            let kind = nav3d::drag_for(tools.active(), button, mods, tools.window_armed());
            clear_drag(ui);
            if kind != Drag::None {
                // Orbit turns about, and Pan follows, the point of the model under the pointer.
                let pivot = match (kind, ui.input(|i| i.pointer.press_origin())) {
                    (Drag::Orbit | Drag::Pan, Some(o)) => {
                        nav3d::point_under(&scene, &nav.camera, local(o), size)
                    }
                    _ => None,
                };
                let active = ActiveDrag {
                    middle: button == Button::Middle,
                    kind,
                    pivot,
                };
                ui.memory_mut(|m| m.data.insert_temp(ACTIVE_KEY.into(), active));
            }
        }
    }
    if let Some(active) = ui.memory(|m| m.data.get_temp::<ActiveDrag>(ACTIVE_KEY.into())) {
        let button = if active.middle {
            egui::PointerButton::Middle
        } else {
            egui::PointerButton::Primary
        };
        if resp.dragged_by(button) {
            if active.kind == Drag::ZoomWindow {
                if let Some(start) = ui.input(|i| i.pointer.press_origin()) {
                    let to = ui.input(|i| i.pointer.latest_pos()).unwrap_or(start);
                    let w = WindowDrag {
                        from: local(start),
                        to: local(to),
                    };
                    ui.memory_mut(|m| m.data.insert_temp(WINDOW_KEY.into(), w));
                }
            } else {
                let d = resp.drag_delta();
                nav.drag(active.kind, Vec2::new(d.x, d.y), size, active.pivot);
                shown.navigated = true;
            }
        }
        if resp.drag_stopped_by(button) {
            if active.kind == Drag::ZoomWindow {
                if let Some(w) = ui.memory(|m| m.data.get_temp::<WindowDrag>(WINDOW_KEY.into()))
                    && nav.zoom_window(w.from, w.to, size)
                {
                    shown.navigated = true;
                }
                tools.disarm_window();
            }
            clear_drag(ui);
        }
    }

    // The Select tool and the drawing tools: picks, inference, shapes (MOD-08).
    let before = session.editor.revision();
    edit.pointer(
        session,
        view3d,
        &nav.camera,
        &Input {
            ui,
            resp: &resp,
            rect,
            size,
            scene: &scene,
            tool: tools.active(),
            mods,
        },
    );
    if session.editor.revision() != before {
        ui.ctx().request_repaint();
    }

    // The scene.
    let cam = nav.camera;
    let bounds = scene.bounds;
    let (extra_key, extra) = edit.overlay(scene.origin);
    let (overlay, overlay_rev) =
        view3d.selection_lines_with(&scene, rev, &session.selection, extra_key, || extra);
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
                f64::from(bg[0]) / 255.0,
                f64::from(bg[1]) / 255.0,
                f64::from(bg[2]) / 255.0,
            ],
        },
    ));

    // The drawing axes, over the scene.
    let painter = ui.painter_at(rect);
    let reach = (cam.distance * 0.6).max(1000.0);
    for (a, b, color, positive) in axis_segments(scene.origin, reach) {
        if let (Some(pa), Some(pb)) = (
            to_screen(nav, a, rect, aspect, bounds),
            to_screen(nav, b, rect, aspect, bounds),
        ) {
            let c = if positive {
                color
            } else {
                color.gamma_multiply(0.35)
            };
            painter.line_segment([pa, pb], Stroke::new(if positive { 1.5 } else { 1.0 }, c));
        }
    }

    // The marks of the tools: inference, the shape being drawn, selected edges.
    edit.paint(&painter, rect, &nav.camera, scene.origin, size);

    // The window of Zoom Window.
    if let Some(w) = ui.memory(|m| m.data.get_temp::<WindowDrag>(WINDOW_KEY.into())) {
        let r = Rect::from_two_pos(
            Pos2::new(rect.min.x + w.from.x, rect.min.y + w.from.y),
            Pos2::new(rect.min.x + w.to.x, rect.min.y + w.to.y),
        );
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }

    // The context menu (the secondary button, which no drag uses).
    resp.context_menu(|ui| {
        let mut cmds = Vec::new();
        crate::menu::intersect_menu(ui, intersect, &mut cmds);
        if let Some(c) = cmds.pop() {
            shown.command = Some(c);
        }
    });
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndc_has_y_up_and_the_center_is_zero() {
        let size = Vec2::new(800.0, 600.0);
        assert_eq!(ndc_of(size / 2.0, size), Vec2::ZERO);
        assert_eq!(ndc_of(Vec2::ZERO, size), Vec2::new(-1.0, 1.0));
        assert_eq!(ndc_of(size, size), Vec2::new(1.0, -1.0));
        // A zero size does not divide by zero.
        assert!(ndc_of(Vec2::ONE, Vec2::ZERO).is_finite());
    }

    #[test]
    fn the_axes_come_in_the_colors_red_green_and_blue_with_a_faint_negative_half() {
        let segs = axis_segments(glam::DVec3::ZERO, 1000.0);
        assert_eq!(segs.len(), 6);
        let positive: Vec<_> = segs.iter().filter(|s| s.3).collect();
        assert_eq!(positive.len(), 3);
        assert_eq!(positive[0].2, AXIS_RED);
        assert_eq!(positive[1].2, AXIS_GREEN);
        assert_eq!(positive[2].2, AXIS_BLUE);
        // Red runs along X, green along Y, blue along Z (up).
        assert_eq!(positive[0].1 - positive[0].0, Vec3::X * 1000.0);
        assert_eq!(positive[1].1 - positive[1].0, Vec3::Y * 1000.0);
        assert_eq!(positive[2].1 - positive[2].0, Vec3::Z * 1000.0);
        // The origin of the scene moves the axes: the world origin is at -origin in local points.
        let moved = axis_segments(glam::DVec3::new(500.0, 0.0, 0.0), 1000.0);
        assert_eq!(moved[0].0, Vec3::new(-500.0, 0.0, 0.0));
    }
}
