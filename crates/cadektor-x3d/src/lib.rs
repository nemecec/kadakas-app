//! Cadektor x3d: 3D output from the model, without file access (it builds for wasm).
//!
//! - glTF 2.0 (`.gltf` with an embedded buffer, and `.glb`) and OBJ with MTL, from the 3D scene
//!   of the document (`cadektor_core::mesh::Scene`). One node for each object, or for each layer
//!   or class. Y up and metres by default. Plants and symbols with the same shape share one mesh.
//! - Materials come from a `MaterialSource`. `DocumentMaterials` resolves the material resources
//!   of the document (PBR values and textures); `BaseColors` writes the base color of the scene.
//! - Sections and elevations: the cut of the scene by a vertical plane, as cut faces, profiles
//!   and visible lines beyond (see `section`).

pub mod gltf_writer;
pub mod material;
pub mod obj;
pub mod options;
pub mod parts;
pub mod section;

use cadektor_core::mesh::Scene;
use cadektor_core::model::Document;
use serde::Serialize;

pub use material::{BaseColors, DocumentMaterials, Material, MaterialSource, Texture};
pub use obj::ObjFiles;
pub use options::{ExportOptions, Grouping, PlantDetail, Units};

/// Why an export failed.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum X3dError {
    #[error("the document has no 3D objects to export")]
    Empty,
    #[error("{0}")]
    Options(String),
    #[error("{0}")]
    Write(String),
}

/// What an export wrote.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ExportStats {
    /// Objects in the file.
    pub objects: usize,
    pub nodes: usize,
    pub meshes: usize,
    /// Nodes that share the mesh of an earlier node.
    pub instances: usize,
    pub triangles: usize,
    pub lines: usize,
    pub materials: usize,
    pub textures: usize,
    pub bytes: usize,
    /// Objects that were left out, with the reason.
    pub skipped: Vec<(cadektor_core::uid::Uid, String)>,
}

/// A `.gltf` file as text.
pub fn export_gltf(
    doc: &Document,
    scene: &Scene,
    opts: &ExportOptions,
    src: &dyn MaterialSource,
) -> Result<(String, ExportStats), X3dError> {
    let model = parts::build(doc, scene, opts, src)?;
    gltf_writer::write_gltf(&model, doc, opts)
}

/// A `.glb` file.
pub fn export_glb(
    doc: &Document,
    scene: &Scene,
    opts: &ExportOptions,
    src: &dyn MaterialSource,
) -> Result<(Vec<u8>, ExportStats), X3dError> {
    let model = parts::build(doc, scene, opts, src)?;
    gltf_writer::write_glb(&model, doc, opts)
}

/// OBJ and MTL text. `mtl_name` is the file name that the OBJ refers to.
pub fn export_obj(
    doc: &Document,
    scene: &Scene,
    opts: &ExportOptions,
    src: &dyn MaterialSource,
    mtl_name: &str,
) -> Result<(ObjFiles, ExportStats), X3dError> {
    let model = parts::build(doc, scene, opts, src)?;
    obj::write_obj(&model, doc, opts, mtl_name)
}

#[cfg(test)]
mod tests;
