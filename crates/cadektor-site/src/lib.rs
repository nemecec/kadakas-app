//! Site model (terrain) for Cadektor: a triangulated surface (TIN) from 3D points and 3D lines,
//! contours, slope and aspect, site modifiers (pads and grade limits), the proposed surface,
//! cut and fill volumes and draping.
//!
//! The crate is pure Rust and compiles to WebAssembly. The low-level modules use plain geometry
//! types. `model` connects them to the document of `cadektor-core`, and `draw` draws site
//! models through the extension hooks of core.

pub mod contour;
pub mod drape;
pub mod draw;
pub mod grid;
pub mod model;
pub mod modifier;
pub mod overlay;
pub mod terrain;
pub mod tin;
pub mod volume;

pub use contour::{Contour, ContourSpec, contours};
pub use drape::drape;
pub use model::{SiteCache, SiteResult, Surface, drape_polyline, elevation_at};
pub use modifier::{Modifier, PadSlope, proposed_surface};
pub use terrain::SiteTerrain;
pub use tin::{SlopeAspect, SourceData, SourceLine, Tin, TinBuilder};
pub use volume::{CutFill, cut_fill, cut_fill_in};

use cadektor_core::model::ObjectId;

/// Why a site calculation failed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SiteError {
    #[error("site model numbers must be finite")]
    NotFinite,
    #[error(
        "the source data does not make a surface: give at least 3 points that are not on one line"
    )]
    NoSurface,
    #[error("a triangle refers to a vertex that does not exist")]
    BadTriangle,
    #[error("contour interval must be a positive number")]
    BadInterval,
    #[error("the contour interval is too small for the height of the site")]
    TooManyContours,
    #[error("point {0} is outside the site model")]
    Outside(usize),
    #[error("object {0} does not exist")]
    NoObject(ObjectId),
    #[error("object {0} is not a site model")]
    NotASiteModel(ObjectId),
}
