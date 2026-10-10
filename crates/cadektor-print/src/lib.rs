//! Cadektor print: composes sheet layers into display lists and writes them as PDF, SVG and PNG.
//! Native only (no wasm): it writes files and renders 3D viewports on the CPU.
//!
//! PDF writer: `pdf-writer` (maintained, pure Rust, no unsafe). Pages hold vector paths with
//! line weights, fills, arcs as Bezier curves, Helvetica text, and 3D viewports as images.

pub mod annot;
pub mod areas;
pub mod compose;
pub mod display;
pub mod pdf;
pub mod plan;
pub mod png;
pub mod svg;
pub mod table;

use std::path::{Path, PathBuf};

use cadektor_core::model::{Document, LayerId};

pub use compose::{SheetCache, compose_page, compose_page_on};
pub use display::{Page, Prim};

/// Why an output could not be made.
#[derive(Debug, PartialEq)]
pub enum PrintError {
    NoSheet(LayerId),
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for PrintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrintError::NoSheet(l) => write!(f, "layer {l} is not a sheet layer"),
            PrintError::Invalid(m) => write!(f, "{m}"),
            PrintError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for PrintError {}

/// The sheet layers to print: the given IDs, or all sheet layers in document order.
pub fn select_sheets(doc: &Document, ids: Option<&[LayerId]>) -> Result<Vec<LayerId>, PrintError> {
    let all: Vec<LayerId> = doc.sheet_layers().map(|l| l.id).collect();
    let list = match ids {
        None => all,
        Some(ids) => {
            if let Some(bad) = ids.iter().find(|i| !all.contains(i)) {
                return Err(PrintError::NoSheet(*bad));
            }
            ids.to_vec()
        }
    };
    if list.is_empty() {
        return Err(PrintError::Invalid(
            "the document has no sheet layers to print".into(),
        ));
    }
    Ok(list)
}

/// A PDF file with one page per sheet.
pub fn export_pdf(
    doc: &Document,
    revision: u64,
    cache: &mut SheetCache,
    sheets: &[LayerId],
) -> Result<Vec<u8>, PrintError> {
    export_pdf_on(
        doc,
        revision,
        cache,
        sheets,
        &cadektor_core::hardscape::NoTerrain,
    )
}

/// Like `export_pdf`, with the ground of the site models for the worksheet tables.
pub fn export_pdf_on(
    doc: &Document,
    revision: u64,
    cache: &mut SheetCache,
    sheets: &[LayerId],
    ground: &dyn cadektor_core::hardscape::TerrainSource,
) -> Result<Vec<u8>, PrintError> {
    let pages = sheets
        .iter()
        .map(|l| cache.page_on(doc, revision, *l, ground))
        .collect::<Result<Vec<_>, _>>()?;
    let pixels: u64 = pages
        .iter()
        .flat_map(|p| &p.prims)
        .map(|p| {
            if let Prim::Image(i) = p {
                i.width as u64 * i.height as u64
            } else {
                0
            }
        })
        .sum();
    if pixels > compose::MAX_RASTER_PIXELS {
        return Err(PrintError::Invalid(format!(
            "the 3D viewports of these sheets make {} megapixels of images. The limit is {}. Lower the dpi",
            pixels / 1_000_000,
            compose::MAX_RASTER_PIXELS / 1_000_000
        )));
    }
    let refs: Vec<&Page> = pages.iter().map(|p| &**p).collect();
    let title = if doc.drawing.project.name.is_empty() {
        "Cadektor drawing"
    } else {
        doc.drawing.project.name.as_str()
    };
    Ok(pdf::write_pdf(&refs, title))
}

/// Writes bytes to a file. The folder must exist. Existing files are replaced.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<PathBuf, PrintError> {
    std::fs::write(path, bytes).map_err(|e| PrintError::Io(format!("{}: {e}", path.display())))?;
    Ok(path.to_path_buf())
}
