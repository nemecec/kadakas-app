//! GIS services for Kadakas: WMS and WMTS map backgrounds, WFS import, geoimages (a picture with a
//! world file, or a GeoTIFF) and coordinate conversion of a whole document. Native only.
//!
//! # Why a crate of its own
//!
//! `kadakas-io` reads and writes files and has no network code. A map service needs an HTTP
//! client, a tile cache, image decoding and warping. Keeping those in this crate keeps `kadakas-io`
//! small and offline, and lets this crate be tested on its own with a mock server. This crate uses
//! `kadakas-io` for the CRS math (`proj4rs`) and for the GIS importer (WFS features go through the
//! same code as GeoJSON files, so the origin logic and the object data are the same).
//!
//! # Parts
//!
//! - [`http`]: a small `ureq` client with timeouts, a size limit, manual redirects and an allow-list of URLs.
//! - [`cache`]: tiles on disk with a size limit.
//! - [`tiles`]: tile and bounding box math (pure).
//! - [`caps`]: GetCapabilities parsing for WMS 1.3.0 (and 1.1.1), WMTS 1.0.0 and WFS 2.0.
//! - [`wms`], [`wmts`]: fetch a map for a view and return a georeferenced picture.
//! - [`warp`]: move a picture from one CRS to the model frame of the document.
//! - [`geoimage`]: a PNG or JPEG with a world file, or a GeoTIFF.
//! - [`wfs`], [`gml`]: WFS 2.0 GetFeature as GeoJSON or GML 3.2 (simple features).
//! - [`coords`]: the coordinate readout (easting, northing, longitude, latitude).
//! - [`reproject`]: change the CRS of a document, with a dry-run report.
//! - [`job`]: run a fetch on a worker thread, so the UI never waits.

pub mod cache;
pub mod caps;
pub mod coords;
pub mod geoimage;
pub mod gml;
pub mod http;
pub mod job;
pub mod mosaic;
pub mod raster;
pub mod reproject;
pub mod tiles;
pub mod warp;
pub mod wfs;
pub mod wms;
pub mod wmts;

#[cfg(test)]
mod testing;

/// Why a GIS call failed. The text is meant for the user.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GisError {
    /// The address is not allowed, or it is not a valid http or https URL.
    #[error("{0}")]
    Url(String),
    /// The server did not answer, or answered with an error.
    #[error("{0}")]
    Http(String),
    /// The answer is not what the service type sends.
    #[error("{0}")]
    Format(String),
    /// A CRS is unknown or a point cannot be converted.
    #[error("{0}")]
    Crs(String),
    /// A value is not accepted.
    #[error("{0}")]
    Param(String),
    /// A limit was reached (too many tiles, too large a picture).
    #[error("{0}")]
    Limit(String),
}

pub type Result<T> = std::result::Result<T, GisError>;
