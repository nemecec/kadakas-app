//! Inference in 3D (MOD-03): what the pointer snaps to in a 3D view, headless.
//!
//! The modeler window calls this every time the pointer moves, and draws what comes back. The
//! behavior follows the public description of the inference of SketchUp-like modelers (see
//! `docs/modeler-inference.md`): point inferences (endpoint, midpoint, center, intersection, on
//! edge, on face), linear inferences from the last point (red, green and blue axis, parallel and
//! perpendicular to an edge that you hovered, and a dotted line along an axis from a point you
//! paused on) and locks (Shift keeps the current inference, an arrow key keeps an axis).
//!
//! # The integration API
//!
//! 1. Build a [`Scene`] with a [`SceneBuilder`] when the geometry changes (surface models with
//!    [`SceneBuilder::add_surface_model`]). The scene has a spatial index, so a frame costs about
//!    as much as the number of things near the pointer, not the size of the model.
//! 2. Make a [`Camera`] from the view each frame (`Camera::look_at`).
//! 3. Keep one [`Inferencer`] for the active tool. On a pointer move call
//!    [`Inferencer::infer`] with the pointer in pixels and the reference point of the tool (the
//!    last point you clicked, `None` before the first click). Draw the [`Inference`]: the dot or
//!    square at `point` in `color`, the `label` at the pointer, and the `lines`.
//!    With several models keep one scene per model and call [`Inferencer::infer_scenes`], which
//!    also takes a predicate that skips the geometry a tool is moving. A scene is `Send`: build it
//!    on a worker thread ([`Scene::from_surface_model`]). Instances of components (MOD-07) have
//!    their own helper: [`scene_of_instances`] builds one scene of any number of instances, and
//!    the definition data is built once per definition and shared ([`DefinitionScene`]).
//! 4. Forward keys with [`Inferencer::key`]; call [`Inferencer::pause`] when the pointer has rested
//!    for [`PAUSE_SECONDS`]; call [`Inferencer::reset`] when the tool starts or ends and
//!    [`Inferencer::release_locks`] after a click when the tool wants a fresh start.
//!
//! The module is pure (no window, no GPU, no clock, no randomness) and the same input gives the
//! same result.

mod bvh;
mod camera;
mod engine;
mod instances;
mod math;
mod scene;

pub use bvh::{Aabb, Bvh};
pub use camera::{Camera, Projection, Ray, clip_segment};
pub use engine::{Inferencer, Key};
pub use instances::{
    DefinitionCache, DefinitionScene, InstanceScenes, Placed, scene_of_instances,
    scene_of_instances_cached,
};
pub use scene::{GUIDE_REACH, PointKind, Scene, SceneBuilder, Tag, TagKind};

use glam::{DVec2, DVec3};
use kadakas_l10n::tr;

/// How long the pointer must rest before the tool calls [`Inferencer::pause`], in seconds.
pub const PAUSE_SECONDS: f64 = 0.6;

/// A color for the marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The red of the X axis.
pub const RED: Rgb = Rgb(0xd9, 0x30, 0x30);
/// The green of the Y axis.
pub const GREEN: Rgb = Rgb(0x2e, 0xa0, 0x43);
/// The blue of the Z axis.
pub const BLUE: Rgb = Rgb(0x2f, 0x6b, 0xe0);
/// Parallel and perpendicular inferences.
pub const MAGENTA: Rgb = Rgb(0xc0, 0x2f, 0xc0);
/// Guides: the lines, the points and the inferences that snap to them (MOD-05). An amber that no
/// axis, no geometry and no other inference uses.
pub const GUIDE: Rgb = Rgb(0xd0, 0x82, 0x1c);

/// One of the three axes of the drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    pub fn dir(self) -> DVec3 {
        match self {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        }
    }

    /// Red for X, green for Y and blue for Z.
    pub fn color(self) -> Rgb {
        match self {
            Axis::X => RED,
            Axis::Y => GREEN,
            Axis::Z => BLUE,
        }
    }
}

/// What the pointer snapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Endpoint,
    /// The center of a circle, an arc or a regular polygon.
    Center,
    Midpoint,
    /// Two edges cross, an edge crosses a face, or an inferred line crosses an edge.
    Intersection,
    OnEdge,
    /// A guide point (MOD-05).
    GuidePoint,
    /// On a guide line (MOD-05).
    OnGuide,
    /// On a line along an axis from the reference point.
    OnAxis(Axis),
    /// On a line along an axis from the point that you paused on.
    FromPoint(Axis),
    /// On a line through the reference point, parallel to the edge that you hovered.
    Parallel,
    /// On a line through the reference point, perpendicular to the edge that you hovered.
    Perpendicular,
    /// On the line from the reference point through a point that Shift locked (an endpoint,
    /// midpoint or center that you were on when Shift went down).
    ThroughPoint,
    OnFace,
    /// Nothing was near: the point on the ground plane or in the air along the pointer ray.
    Free,
}

impl Kind {
    /// The priority: a smaller number wins when more than one inference is in reach. The order is
    /// endpoint (and guide point), center, midpoint, intersection, on edge (and on guide), the
    /// linear inferences, on face and then nothing.
    pub fn priority(self) -> u8 {
        match self {
            Kind::Endpoint | Kind::GuidePoint => 0,
            Kind::Center => 1,
            Kind::Midpoint => 2,
            Kind::Intersection => 3,
            Kind::OnEdge | Kind::OnGuide => 4,
            Kind::OnAxis(_)
            | Kind::FromPoint(_)
            | Kind::Parallel
            | Kind::Perpendicular
            | Kind::ThroughPoint => 5,
            Kind::OnFace => 6,
            Kind::Free => 7,
        }
    }

    /// True for the kinds that are a point of the model (not a line or a plane).
    pub fn is_point(self) -> bool {
        matches!(
            self,
            Kind::Endpoint
                | Kind::GuidePoint
                | Kind::Center
                | Kind::Midpoint
                | Kind::Intersection
        )
    }

    /// The color of the mark.
    pub fn color(self) -> Rgb {
        match self {
            Kind::Endpoint => Rgb(0x2e, 0xa0, 0x43),
            Kind::Center => Rgb(0x1f, 0x8a, 0x70),
            Kind::Midpoint => Rgb(0x1e, 0xb4, 0xc8),
            Kind::Intersection => Rgb(0x20, 0x20, 0x20),
            Kind::OnEdge => Rgb(0xd9, 0x30, 0x30),
            Kind::GuidePoint | Kind::OnGuide => GUIDE,
            Kind::OnFace => Rgb(0x2f, 0x6b, 0xe0),
            Kind::OnAxis(a) | Kind::FromPoint(a) => a.color(),
            Kind::Parallel | Kind::Perpendicular | Kind::ThroughPoint => MAGENTA,
            Kind::Free => Rgb(0x80, 0x80, 0x80),
        }
    }

    /// The short label at the pointer, in the language of the UI. `Free` has no label.
    pub fn label(self) -> String {
        match self {
            Kind::Endpoint => tr!("uic-infer3d-endpoint"),
            Kind::Center => tr!("uic-infer3d-center"),
            Kind::Midpoint => tr!("uic-infer3d-midpoint"),
            Kind::Intersection => tr!("uic-infer3d-intersection"),
            Kind::OnEdge => tr!("uic-infer3d-on-edge"),
            Kind::GuidePoint => tr!("uic-infer3d-guide-point"),
            Kind::OnGuide => tr!("uic-infer3d-on-guide"),
            Kind::OnFace => tr!("uic-infer3d-on-face"),
            Kind::OnAxis(Axis::X) => tr!("uic-infer3d-on-red-axis"),
            Kind::OnAxis(Axis::Y) => tr!("uic-infer3d-on-green-axis"),
            Kind::OnAxis(Axis::Z) => tr!("uic-infer3d-on-blue-axis"),
            Kind::FromPoint(_) => tr!("uic-infer3d-from-point"),
            Kind::Parallel => tr!("uic-infer3d-parallel"),
            Kind::Perpendicular => tr!("uic-infer3d-perpendicular"),
            Kind::ThroughPoint => tr!("uic-infer3d-through-point"),
            Kind::Free => String::new(),
        }
    }
}

/// The geometry an inference keeps the point on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    Point(DVec3),
    Line { origin: DVec3, dir: DVec3 },
    Plane { origin: DVec3, normal: DVec3 },
}

/// A line to draw as a cue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstraintLine {
    pub from: DVec3,
    pub to: DVec3,
    pub color: Rgb,
    /// Dotted for the helper lines (from a point, the edge that a parallel inference refers to).
    pub dotted: bool,
}

/// The answer for one pointer position.
#[derive(Clone, Debug, PartialEq)]
pub struct Inference {
    /// The point to use, in world coordinates.
    pub point: DVec3,
    pub kind: Kind,
    /// The pixel of `point` (the mark is drawn there).
    pub screen: DVec2,
    pub color: Rgb,
    /// The label at the pointer, in the language of the UI.
    pub label: String,
    /// The cue lines: the inferred line from the reference point, helper lines.
    pub lines: Vec<ConstraintLine>,
    /// True when a lock (Shift or an arrow key) decides this result.
    pub locked: bool,
    /// What the point was found on: the edge, the face or the curve of a center. [`Tag::NONE`]
    /// for nothing.
    pub source: Tag,
    /// The second thing of an intersection (the other edge or the face), else [`Tag::NONE`].
    pub other: Tag,
    /// The geometry that holds the point, for a lock and for the tool. `None` for `Free`.
    pub constraint: Option<Constraint>,
}

/// The pick radii and tolerances. The radii are in pixels (multiplied by `ui_scale`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub endpoint_px: f64,
    pub center_px: f64,
    pub midpoint_px: f64,
    pub intersection_px: f64,
    pub edge_px: f64,
    /// A line inference holds when the pointer is within this angle (degrees, on the screen) of
    /// the line from its origin.
    pub axis_angle_deg: f64,
    /// A line inference needs the pointer at least this far from its origin on the screen.
    pub axis_min_px: f64,
    /// The scale of the UI (2 on a high-density display); it scales the pixel values.
    pub ui_scale: f64,
    /// At most this many edges and faces near the pointer are checked for intersections.
    pub max_near_edges: usize,
    pub max_near_faces: usize,
    /// The ground plane `z = ground_z` for a pointer over nothing; `None` for no ground.
    pub ground_z: Option<f64>,
    /// With no ground hit, `Free` is this far along the pointer ray from the camera.
    pub free_distance: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            endpoint_px: 10.0,
            center_px: 10.0,
            midpoint_px: 9.0,
            intersection_px: 9.0,
            edge_px: 6.0,
            axis_angle_deg: 4.0,
            axis_min_px: 6.0,
            ui_scale: 1.0,
            max_near_edges: 64,
            max_near_faces: 64,
            ground_z: Some(0.0),
            free_distance: 10_000.0,
        }
    }
}

impl Options {
    pub(crate) fn px(&self, v: f64) -> f64 {
        v * self.ui_scale
    }

    pub(crate) fn pad(&self) -> f64 {
        self.px(self
            .endpoint_px
            .max(self.center_px)
            .max(self.midpoint_px)
            .max(self.intersection_px)
            .max(self.edge_px))
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_guide;
