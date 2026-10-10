//! Cadektor render: the camera, standard views, picking and a CPU renderer for the 3D scene
//! of `cadektor-core`. No GPU and no window, so it also compiles to WebAssembly.

pub mod camera;
pub mod hlr;
pub mod look;
pub mod pick;
pub mod raster;
pub mod shade;
pub mod styled;
pub mod texture;
pub mod thumbnail;
pub mod views;

pub use camera::{Camera, FOV_Y};
pub use pick::{pick, pick_ray};
pub use raster::{AMBIENT, BACKGROUND, Image, LIGHT_DIR, RenderOptions, render};
pub use views::{Projection, RenderMode, StandardView, ViewSettings};

#[cfg(test)]
mod testutil;
