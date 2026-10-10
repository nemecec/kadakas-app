//! Editing 2D geometry: transforms, arrays, offset, trim, extend, split, fillet, chamfer and
//! surface operations (booleans).
//!
//! Everything in this module is pure geometry on shapes or on small values; none of it touches a
//! document. `plan` turns the results into commands (`Command::Batch` of `ModifyObject`,
//! `AddObject` and `DeleteObjects`), so every edit is one undo step. `Command::TransformObjects`
//! and `Command::ArrayObjects` (see `command::transform_ops`) copy and change objects directly.
//!
//! How curves are handled (the same rules everywhere):
//! - Lines, arcs, circles and rectangles are exact.
//! - A polyline is read as exact pieces: lines, and arcs for fillet vertices and arcs through
//!   three points. Bezier and spline pieces are cut into short lines at `FLATTEN_TOLERANCE`.
//! - An edited polyline is written back with corners, and with "point on arc" vertices for arcs, so
//!   arcs stay exact.
//! - Surface operations (add, subtract, clip, intersect, compose) flatten curves at
//!   `BOOL_TOLERANCE` (see `boolean`).

pub mod array;
pub mod boolean;
pub mod modify;
pub mod offset;
pub mod piece;
pub mod plan;
pub mod spec;
pub mod xform;

pub use array::{ArraySpec, Layout};
pub use piece::{Chain, Piece};
pub use spec::{ArrayParams, Transform};
pub use xform::Xf;
