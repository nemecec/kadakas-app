//! What the person points at (AGR-10): the canvases show the pointer to the session once per
//! frame, and the agent reads it in the `pointer` field of `view.get`.
//!
//! The plan gives the drawing point and the object under it. The 3D view gives the ray under the
//! pointer in drawing coordinates. The session makes the hit with the code of `mesh.pick`
//! (`kadakas_api::Session::pointer_json`). The pointer stays in the session: only a call of the
//! agent reads it, it is not saved and not sent to collaborators.

use egui::{Pos2, Rect};
use glam::{DVec2, DVec3, Vec2};
use kadakas_api::{Aim, PixelSize, PointerInput, Session};
use kadakas_core::model::ObjectId;
use kadakas_render::{Camera, Projection};
use kadakas_ui_core::tools::Ctx;

/// The object under the pointer of the plan, kept while the pointer, the drawing and the zoom
/// stay as they are, so that a still pointer costs no search.
#[derive(Default)]
pub struct PlanMemo {
    key: Option<([u64; 2], u64, u64)>,
    ids: Vec<ObjectId>,
}

impl PlanMemo {
    pub fn ids(&mut self, cx: &Ctx, at: DVec2, revision: u64) -> Vec<ObjectId> {
        let key = ([at.x.to_bits(), at.y.to_bits()], revision, cx.tol.to_bits());
        if self.key != Some(key) {
            self.ids = kadakas_ui_core::hit::hit_test_in(cx.annot, cx.doc, at, cx.tol, cx.context)
                .into_iter()
                .collect();
            self.key = Some(key);
        }
        self.ids.clone()
    }
}

/// What a canvas knows about the pointer in one frame.
pub struct Frame {
    /// The drawing area on the screen.
    pub rect: Rect,
    /// The pointer on the screen, `None` when it is not on the area (or a window covers it).
    pub pointer: Option<Pos2>,
    /// The person moves the view in this frame (a drag, the wheel, a key).
    pub navigating: bool,
    /// The clock of the window in ms.
    pub now_ms: f64,
}

impl Frame {
    /// The pointer when it is on the area, with its place in points from the top left corner.
    pub fn on_area(&self) -> Option<(Pos2, [f64; 2])> {
        let p = self.pointer.filter(|p| self.rect.contains(*p))?;
        let c = [
            f64::from(p.x - self.rect.min.x),
            f64::from(p.y - self.rect.min.y),
        ];
        Some((p, c))
    }
}

/// Shows the pointer of the plan. `under` is the place in the drawing under the pointer (not
/// snapped) and the object under it, for a pointer that is on the area, else `None`.
pub fn show_plan(session: &mut Session, frame: &Frame, under: Option<(DVec2, Vec<ObjectId>)>) {
    let (Some((_, canvas)), Some((point, ids))) = (frame.on_area(), under) else {
        session.clear_pointer();
        return;
    };
    session.set_pointer(PointerInput {
        canvas,
        now_ms: frame.now_ms,
        navigating: frame.navigating,
        aim: Aim::Plan { point, ids },
    });
}

/// The ray under a point of a 3D view, in drawing coordinates. `at` is in points from the top
/// left corner of a view of `size` points. `scene_origin` is the origin of the scene: the camera
/// works in coordinates less this point. A parallel view starts its ray on the plane through the
/// target, which can be inside the models, so the ray starts back at `reach` (mm) in front of
/// them, as the picking of objects does.
pub fn ray_under(
    camera: &Camera,
    scene_origin: DVec3,
    at: Vec2,
    size: Vec2,
    reach: f32,
) -> Option<Aim> {
    if size.x < 1.0 || size.y < 1.0 || !at.is_finite() {
        return None;
    }
    let aspect = size.x / size.y;
    let ndc = Vec2::new(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
    let (mut origin, dir) = camera.ray(ndc, aspect);
    let ortho = camera.projection == Projection::Orthographic;
    if ortho {
        origin -= dir * (camera.distance + reach);
    }
    let direction = dir.as_dvec3().try_normalize()?;
    let pixel = if ortho {
        PixelSize {
            constant: f64::from(camera.half_height() * 2.0 / size.y),
            per_distance: 0.0,
        }
    } else {
        PixelSize {
            constant: 0.0,
            per_distance: f64::from(2.0 * (camera.fov_y / 2.0).tan() / size.y),
        }
    };
    Some(Aim::Ray {
        origin: scene_origin + origin.as_dvec3(),
        direction,
        pixel,
    })
}

/// Shows the pointer of the 3D view.
pub fn show_3d(
    session: &mut Session,
    frame: &Frame,
    camera: &Camera,
    scene_origin: DVec3,
    reach: f32,
) {
    let aim = frame.on_area().and_then(|(p, canvas)| {
        let at = Vec2::new(p.x - frame.rect.min.x, p.y - frame.rect.min.y);
        let size = Vec2::new(frame.rect.width(), frame.rect.height());
        ray_under(camera, scene_origin, at, size, reach).map(|aim| (canvas, aim))
    });
    match aim {
        Some((canvas, aim)) => session.set_pointer(PointerInput {
            canvas,
            now_ms: frame.now_ms,
            navigating: frame.navigating,
            aim,
        }),
        None => session.clear_pointer(),
    }
}
