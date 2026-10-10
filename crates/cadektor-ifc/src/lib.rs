//! IFC for Cadektor: export of IFC4 (IFC4 ADD2 TC1) and import of IFC2x3, IFC4 and IFC4x3 (STEP physical file, ISO 10303-21).
//!
//! The entity model ([`step::Entity`], [`step::Value`]) is shared. The import ([`import`]) reads files with
//! a streaming reader ([`reader::StepIndex`]) and plans one `Command::Batch`. See [`import`] for the mapping.
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
//! - Massing model: `IfcBuildingElementProxy` (NOTDEFINED), or `IfcBuilding` in the site with the option
//!   `massing_as_building`. An extruded solid when the roof is flat, a closed `IfcTriangulatedFaceSet` for a gable
//!   or hip roof. The base sits on the site model, like in the 3D view. Property sets `Pset_BuildingElementProxyCommon`
//!   (or `Pset_BuildingCommon`) and `Cadektor_Massing`; quantities `Cadektor_MassingQuantities` (or `Qto_BuildingBaseQuantities`).
//!   Map underlays are skipped.
//! - 2D objects are skipped and listed in the report. With `include_2d` they become `IfcAnnotation`.
//! - Classes become `IfcPresentationLayerAssignment`. Fill colors become `IfcStyledItem` and `IfcSurfaceStyle`.
//! - Data: the Data Manager map gives IFC classes and `IfcPropertySet`s. Records become `ePset_<record>`.
//!   Standard sets (`Pset_SlabCommon`) and quantities (`Qto_SlabBaseQuantities`) come with hardscapes.
//!
//! `TrueNorth` of the context includes the rotation of the model and the meridian convergence of the CRS at the
//! site origin (computed with proj4rs). Without a known CRS it is grid north.
//!
//! GlobalIds come from the project GUID in the document (`drawing.project.guid`) and the object IDs,
//! so they stay the same in every export. The output is deterministic.

pub mod geo;
pub mod geom;
pub use cadektor_core::guid;
pub mod import;
pub mod options;
pub mod reader;
pub mod step;

mod export;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_massing;

use std::io::Write;
use std::path::Path;

use cadektor_core::model::Document;
use cadektor_io::Report;
use cadektor_site::SiteCache;

pub use import::{IfcImportError, IfcImportOptions, import_bytes, import_file};
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

/// The project GUID that an export uses, and whether it comes from the document. Documents made or
/// loaded by the app and the API always have one. For a document that has none, the GUID comes from
/// the content, so the same document still gives the same file. An export never changes the document.
pub fn project_guid(doc: &Document) -> (String, bool) {
    let g = doc.drawing.project.guid.trim();
    if guid::is_valid(g) {
        return (g.to_string(), true);
    }
    (doc.content_guid(), false)
}

/// Writes the document as IFC4 to `out`. Returns the output and the report.
pub fn export_to<W: Write>(
    doc: &Document,
    opts: &IfcOptions,
    out: W,
) -> Result<(W, Report), IfcError> {
    opts.check()?;
    let cache = SiteCache::default();
    let (project, stored) = project_guid(doc);
    let exporter = export::Exporter::new(doc, opts, project, &cache, out)?;
    let (out, mut report) = exporter.run()?;
    if !stored {
        report.notes.push("the document has no project GUID: the GlobalIds come from its content and change when it changes. Save the document in the app, or call project.new_guid".into());
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
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut opts = opts.clone();
    if opts.file_name.is_none() {
        opts.file_name = Some(name);
    }
    let result = (|| {
        let file = std::fs::File::create(&tmp)
            .map_err(|e| IfcError::Io(format!("cannot write {}: {e}", path.display())))?;
        let (out, report) = export_to(doc, &opts, std::io::BufWriter::new(file))?;
        let file = out.into_inner().map_err(|e| IfcError::Io(e.to_string()))?;
        file.sync_all()?;
        drop(file);
        cadektor_core::fsio::replace_with_backup(&tmp, &target)
            .map_err(|e| IfcError::Io(format!("cannot write {}: {e}", path.display())))?;
        Ok(report)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
