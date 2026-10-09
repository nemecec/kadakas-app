//! IFC4 export for Cadektor (IFC4 ADD2 TC1, STEP physical file, ISO 10303-21).
//!
//! This crate only writes. The entity model ([`step::Entity`], [`step::Value`]) and the parser
//! ([`step::parse`]) are shared, so an IFC reader can be added later without a new model.
//!
//! # What the file contains
//!
//! - `IfcProject` with units (millimeter, square meter, cubic meter, radian) and one model context.
//! - `IfcSite` with the georeference: `IfcMapConversion` and `IfcProjectedCRS` from the document
//!   georeference, and `RefLatitude`/`RefLongitude` when the CRS is known.
//! - No `IfcBuilding` by default (IFC4 lets an `IfcSite` contain elements). Landscape work has no
//!   building. With `layers_as_storeys`, a default `IfcBuilding` holds one `IfcBuildingStorey` per design layer.
//! - Site model: `IfcGeographicElement` (TERRAIN) with an `IfcTriangulatedFaceSet`.
//! - Hardscape: `IfcSlab` (BASESLAB), like the Vectorworks default. An extruded solid when flat, a
//!   triangulated solid when sloped or draped. The style components are an `IfcMaterialLayerSet`.
//! - Landscape area: `IfcGeographicElement` with a thin extruded solid (holes cut out).
//! - Plants and existing trees: `IfcGeographicElement`: trunk and canopy cylinders, plus an `IfcBoundingBox`.
//!   IFC4 has no VEGETATION type (IFC4x3 has), so vegetation is `PredefinedType` USERDEFINED with `ObjectType` 'VEGETATION'.
//! - 2D objects are skipped and listed in the report. With `include_2d` they become `IfcAnnotation`.
//! - Classes become `IfcPresentationLayerAssignment`. Fill colors become `IfcStyledItem` and `IfcSurfaceStyle`.
//! - Data: the Data Manager map gives IFC classes and `IfcPropertySet`s. Records become `ePset_<record>`.
//!   Standard sets (`Pset_SlabCommon`) and quantities (`Qto_SlabBaseQuantities`) come with hardscapes.
//!
//! GlobalIds come from the project GUID in the document (`drawing.project.guid`) and the object IDs,
//! so they stay the same in every export. The output is deterministic.

pub mod geo;
pub mod geom;
pub mod guid;
pub mod options;
pub mod step;

mod export;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::Path;

use cadektor_core::command::Command;
use cadektor_core::model::Document;
use cadektor_io::Report;
use cadektor_site::SiteCache;

pub use options::IfcOptions;

/// Why an export failed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IfcError {
    /// An option has a bad value.
    #[error("{0}")]
    Option(String),
    /// The output could not be written.
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for IfcError {
    fn from(e: std::io::Error) -> Self {
        IfcError::Io(e.to_string())
    }
}

/// The project GUID that an export uses: the one in the document, else a stand-in that comes from the
/// project name (so the same document still gives the same file).
pub fn project_guid(doc: &Document) -> String {
    let g = doc.drawing.project.guid.trim();
    if guid::is_valid(g) { g.to_string() } else { guid::derive("", &doc.drawing.project.name, 0) }
}

/// The command that stores a new project GUID, if the document has none. Run it before the first
/// export, so the GlobalIds stay the same when the document is saved and exported again.
pub fn project_guid_command(doc: &Document) -> Option<Command> {
    (!guid::is_valid(doc.drawing.project.guid.trim())).then(|| Command::SetProjectGuid { guid: guid::random() })
}

/// Writes the document as IFC4 to `out`. Returns the output and the report.
pub fn export_to<W: Write>(doc: &Document, opts: &IfcOptions, out: W) -> Result<(W, Report), IfcError> {
    opts.check()?;
    let cache = SiteCache::default();
    let exporter = export::Exporter::new(doc, opts, project_guid(doc), &cache, out)?;
    let (out, mut report) = exporter.run()?;
    if !guid::is_valid(doc.drawing.project.guid.trim()) {
        report.notes.push("the document has no project GUID yet: GlobalIds follow the project name until one is stored".into());
    }
    Ok((out, report))
}

/// Writes the document as IFC4 into memory.
pub fn export_bytes(doc: &Document, opts: &IfcOptions) -> Result<(Vec<u8>, Report), IfcError> {
    export_to(doc, opts, Vec::new())
}

/// Writes the document to a file. The data goes to a temporary file in the same folder first, one
/// entity at a time, and is renamed over `path` at the end, so a crash never leaves half a file.
pub fn export_file(path: &Path, doc: &Document, opts: &IfcOptions) -> Result<Report, IfcError> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut opts = opts.clone();
    if opts.file_name.is_none() {
        opts.file_name = Some(name);
    }
    let result = (|| {
        let file = std::fs::File::create(&tmp).map_err(|e| IfcError::Io(format!("cannot write {}: {e}", path.display())))?;
        let (out, report) = export_to(doc, &opts, std::io::BufWriter::new(file))?;
        let file = out.into_inner().map_err(|e| IfcError::Io(e.to_string()))?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, &target).map_err(|e| IfcError::Io(format!("cannot write {}: {e}", path.display())))?;
        Ok(report)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
