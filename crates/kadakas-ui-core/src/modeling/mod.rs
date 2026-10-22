//! Modeling in the 3D modeler (MOD-08, first part): the drawing tools as headless state machines,
//! the selection of faces and edges, and the commands that a finished tool runs.
//!
//! The modeler (`kadakas-modeler`) only draws and feeds input:
//!
//! 1. [`SceneCache::get`] gives the inference scene of the document revision. It is built once per
//!    revision, not every frame.
//! 2. On a pointer move the window calls the inference (`infer3d`) with `Modeling::reference` and
//!    passes the result with the pointer ray to [`Modeling::moved`].
//! 3. Clicks go to [`Modeling::click`] (the Eraser: `press` and `release`). The [`Out`] holds the
//!    [`Action`]s; [`exec::run`] runs them as one undo step, and `Modeling::committed` tells the
//!    tool which model was used.
//! 4. [`Modeling::preview`] and [`Modeling::live`] give what to draw and what the Measurements
//!    box shows; [`Modeling::type_char`] and [`Modeling::enter`] take what the person types.
//!
//! Select (Space) uses [`select`]: [`select::pick`], [`select::rect_select`] and
//! [`select::SubSelection`]. See `docs/modeler.md`.

pub mod exec;
pub mod messages;
pub mod paint;
mod plane;
mod resize;
mod scenes;
pub mod select;
pub mod soften;
mod tape;
mod tools;
mod xform;

pub use exec::{
    Action, GuideSpec, Ran, Target, run as run_actions, run_full as run_actions_full,
    run_noted as run_actions_noted,
};
pub use plane::Plane3;
pub use resize::{ResizePlan, resize_command, resize_plan};
pub use scenes::{SceneCache, Scenes, ShowOptions, World, elevation, frame_of, model_of};
pub use select::{Item, SubSelection};
pub use soften::{EdgeStyle, SoftenPanel, edge_style_actions};
pub use tape::{PreviewGuide, RESIZE_RANGE, Resize};
pub use tools::{
    DEFAULT_ARC_SIDES, DEFAULT_POLYGON_SIDES, DEFAULT_SIDES, Excluded, KeyToggle, LiveLabel,
    Modeling, Out, Pointer, Preview, arc_points, bulge_of, circle_points, delete_actions,
    intersect_actions, is_drawing_tool, is_more_tool, polygon_points, radius_to_bulge,
    takes_values,
};
pub use xform::{
    Context as XContext, Grip, Subject, is_transform_tool, usable_grips, xform_action,
};

/// The camera of the inference for the view of the modeler. `origin` is the world position of the
/// local point (0, 0, 0) of the scene (`Scene::origin`), `size` the size of the view in points.
/// `None` when the view cannot be projected (no area).
pub fn infer_camera(
    cam: &kadakas_render::Camera,
    origin: glam::DVec3,
    size: glam::Vec2,
) -> Option<crate::infer3d::Camera> {
    use crate::infer3d::{Camera, Projection};
    let eye = cam.eye().as_dvec3() + origin;
    let target = cam.target.as_dvec3() + origin;
    let projection = match cam.projection {
        kadakas_render::Projection::Perspective => Projection::Perspective {
            fov_y: f64::from(cam.fov_y),
        },
        kadakas_render::Projection::Orthographic => Projection::Orthographic {
            height: f64::from(cam.half_height() * 2.0),
        },
    };
    // The up vector of the view: in the top and bottom views Z is parallel to the view.
    Camera::look_at(
        eye,
        target,
        cam.up().as_dvec3(),
        projection,
        glam::DVec2::new(f64::from(size.x), f64::from(size.y)),
    )
}

#[cfg(test)]
mod tests;
