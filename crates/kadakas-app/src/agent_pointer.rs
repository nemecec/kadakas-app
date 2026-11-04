//! What the person points at (AGR-10): the canvases show the pointer to the session once per
//! frame, and the agent reads it in the `pointer` field of `view.get`.
//!
//! The plan gives the drawing point and the object under it. The 3D view gives the ray under the
//! pointer in drawing coordinates. The session makes the hit with the code of `mesh.pick`
//! (`kadakas_api::Session::pointer_json`). The pointer stays in the session: only a call of the
//! agent reads it, it is not saved and not sent to collaborators.

use egui::{Pos2, Rect};
use glam::{DVec2, DVec3, Vec2};
use kadakas_api::{Aim, PointerInput, Session};
use kadakas_core::model::ObjectId;
use kadakas_render::Camera;
use kadakas_ui_core::tools::Ctx;

/// What the object under the pointer of the plan depends on.
#[derive(PartialEq)]
struct PlanKey {
    at: [u64; 2],
    revision: u64,
    tol: u64,
    /// The group that the person has entered: a click selects its members.
    context: Option<ObjectId>,
}

/// The object under the pointer of the plan, kept while the pointer, the drawing, the zoom and
/// the entered group stay as they are, so that a still pointer costs no search.
#[derive(Default)]
pub struct PlanMemo {
    key: Option<PlanKey>,
    ids: Vec<ObjectId>,
}

impl PlanMemo {
    /// What a click at `at` selects: none or one object.
    pub fn ids(&mut self, cx: &Ctx, at: DVec2, revision: u64) -> Vec<ObjectId> {
        let key = PlanKey {
            at: [at.x.to_bits(), at.y.to_bits()],
            revision,
            tol: cx.tol.to_bits(),
            context: cx.context,
        };
        if self.key.as_ref() != Some(&key) {
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
    /// The window has the focus of the system.
    pub focused: bool,
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
/// snapped) and the object that a click at the cursor selects, for a pointer that is on the
/// area, else `None`.
pub fn show_plan(session: &mut Session, frame: &Frame, under: Option<(DVec2, Vec<ObjectId>)>) {
    let (Some((_, canvas)), Some((point, ids))) = (frame.on_area(), under) else {
        session.clear_pointer();
        return;
    };
    session.set_pointer(PointerInput {
        canvas,
        now_ms: frame.now_ms,
        navigating: frame.navigating,
        focused: frame.focused,
        aim: Aim::Plan { point, ids },
    });
}

pub use kadakas_api::ray_under;

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
            focused: frame.focused,
            aim,
        }),
        None => session.clear_pointer(),
    }
}
