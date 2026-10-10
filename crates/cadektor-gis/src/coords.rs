//! The coordinate readout: what a model position is on the map.

use cadektor_core::DVec2;
use cadektor_core::georef::Georeference;
use cadektor_io::crs::{Crs, Transformer};
use serde::Serialize;

/// A position on the map in the document CRS and in longitude and latitude.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Readout {
    pub crs: String,
    /// Meters in the document CRS (east, north).
    pub easting: f64,
    pub northing: f64,
    pub lon: f64,
    pub lat: f64,
}

/// Prepared conversions for many readouts (the cursor moves many times per second).
pub struct Reader {
    georef: Georeference,
    crs: Crs,
    wgs: Crs,
}

impl Reader {
    /// `None` if the document has no known CRS.
    pub fn new(georef: &Georeference) -> Option<Reader> {
        if georef.crs.trim().is_empty() || !georef.is_finite() {
            return None;
        }
        Some(Reader {
            georef: georef.clone(),
            crs: Crs::parse(&georef.crs).ok()?,
            wgs: Crs::from_epsg(4326).ok()?,
        })
    }

    pub fn crs(&self) -> &str {
        &self.crs.spec
    }

    /// The map position of a model position (mm).
    pub fn at(&self, model: DVec2) -> Option<Readout> {
        let m = self.georef.to_map(model);
        let (lon, lat) = if self.crs.is_geographic() {
            (m.x, m.y)
        } else {
            let u = m / self.crs.to_meter();
            let (x, y, _) = Transformer::new(&self.crs, &self.wgs).apply(u.x, u.y, 0.0).ok()?;
            (x, y)
        };
        (m.is_finite() && lon.is_finite() && lat.is_finite()).then(|| Readout {
            crs: self.crs.spec.clone(),
            easting: m.x,
            northing: m.y,
            lon,
            lat,
        })
    }
}

impl Readout {
    /// Text for the Data bar: `N 6672010.250  E 385010.500` or, with `lonlat`, `60.2123456  24.9123456`.
    pub fn text(&self, lonlat: bool) -> String {
        if lonlat {
            format!("lat {:.7}  lon {:.7}", self.lat, self.lon)
        } else {
            format!("N {:.3}  E {:.3}", self.northing, self.easting)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(crs: &str) -> Georeference {
        Georeference {
            crs: crs.into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        }
    }

    #[test]
    fn reads_the_map_position() {
        let r = Reader::new(&g("EPSG:3067")).unwrap();
        assert_eq!(r.crs(), "EPSG:3067");
        let o = r.at(DVec2::new(10_500.0, 250.0)).unwrap();
        assert_eq!((o.easting, o.northing), (385_010.5, 6_672_000.25));
        assert!(o.lat > 60.0 && o.lat < 60.3 && o.lon > 24.0 && o.lon < 25.5, "{o:?}");
        assert_eq!(o.text(false), "N 6672000.250  E 385010.500");
        assert!(o.text(true).starts_with("lat 60."));
        // Rotation is part of the georeference.
        let mut rot = g("EPSG:3067");
        rot.rotation = std::f64::consts::FRAC_PI_2;
        let o = Reader::new(&rot).unwrap().at(DVec2::new(1000.0, 0.0)).unwrap();
        assert!((o.northing - 6_672_001.0).abs() < 1e-6 && (o.easting - 385_000.0).abs() < 1e-6);
    }

    #[test]
    fn no_crs_no_readout() {
        assert!(Reader::new(&g("")).is_none());
        assert!(Reader::new(&g("banana")).is_none());
        let mut bad = g("EPSG:3067");
        bad.origin = DVec2::NAN;
        assert!(Reader::new(&bad).is_none());
        // A geographic document CRS gives its own numbers as lon and lat.
        let geo = Georeference { crs: "EPSG:4326".into(), origin: DVec2::new(24.0, 60.0), rotation: 0.0, user_origin: DVec2::ZERO };
        let o = Reader::new(&geo).unwrap().at(DVec2::ZERO).unwrap();
        assert_eq!((o.lon, o.lat), (24.0, 60.0));
        // A position that cannot be converted gives nothing.
        assert!(Reader::new(&g("EPSG:3067")).unwrap().at(DVec2::new(f64::INFINITY, 0.0)).is_none());
    }
}
