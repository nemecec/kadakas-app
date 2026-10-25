//! Kadakas meshio (MOD-12): triangle mesh files without file access.
//!
//! - Readers for Wavefront OBJ (with MTL colors), STL (ASCII and binary) and glTF 2.0 (`.gltf`
//!   with embedded or loaded buffers, and `.glb`). Each reader gives a [`MeshSet`]: parts with
//!   indexed triangles in millimeters and Z up (the axes and units of a Kadakas model), and an
//!   optional color for each part.
//! - A writer for STL (binary and ASCII). OBJ and glTF files of a document are written by
//!   `kadakas-x3d`.
//!
//! The readers never trust the file. A file that is too large, has too many triangles, has a
//! number that is not finite, an index that does not exist or ends too soon is refused with a
//! [`MeshError`] that names the counts. No input makes a reader panic or allocate more than the
//! [`Limits`] allow.
//!
//! The crate knows nothing of documents, so it builds for wasm and has no other Kadakas crate
//! as a dependency. The formats come from their public specifications.

mod error;
pub mod gltf;
mod mesh;
pub mod obj;
pub mod stl;
mod triangulate;
mod units;

pub use error::MeshError;
pub use mesh::{Limits, MeshSet, Part, Totals};
pub use units::{Frame, Unit, Up};

/// The format of a mesh file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Obj,
    Stl,
    Gltf,
    Glb,
    /// STEP (read by `kadakas-step`).
    Step,
}

impl Format {
    /// The format of a name (`obj`, `stl`, `gltf`, `glb`, with or without a dot), any case.
    pub fn from_name(name: &str) -> Option<Format> {
        match name
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "obj" => Some(Format::Obj),
            "stl" => Some(Format::Stl),
            "gltf" => Some(Format::Gltf),
            "glb" => Some(Format::Glb),
            "step" | "stp" => Some(Format::Step),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Obj => "obj",
            Format::Stl => "stl",
            Format::Gltf => "gltf",
            Format::Glb => "glb",
            Format::Step => "step",
        }
    }

    /// The default frame of the format. glTF is always in meters with Y up. OBJ is in meters with
    /// Y up, like the OBJ files of Kadakas. STL is in millimeters with Z up.
    pub fn default_frame(self) -> Frame {
        match self {
            Format::Obj | Format::Gltf | Format::Glb => Frame {
                unit: Unit::Metre,
                up: Up::Y,
            },
            Format::Stl | Format::Step => Frame {
                unit: Unit::Millimetre,
                up: Up::Z,
            },
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_more;
