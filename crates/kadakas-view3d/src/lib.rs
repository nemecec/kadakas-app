//! The 3D view that the UI programs share (PLAT-11).
//!
//! - [`gpu3d`]: the GPU drawing of the scene with wgpu (shading, shadows, edges, transparency,
//!   multisampling), and the egui paint callback that puts it into a canvas.
//! - [`vertex`]: the vertex types and the conversion from the scene of `kadakas-core`.
//! - [`input`]: the keys of egui turned into the keys and actions of `kadakas-ui-core`.
//! - [`view3d`]: the state of a 3D view without a GPU or a window: the camera, the caches of the
//!   materials and of the selection lines, picking.
//!
//! `kadakas-app` (the Vectorworks-like app) and `kadakas-modeler` (the SketchUp-like modeler) draw
//! the same way. The code was in `kadakas-app` and moved here without a change of behavior.

pub mod gpu3d;
pub mod import_widgets;
pub mod input;
pub mod scroll;
pub mod vertex;
pub mod view3d;
