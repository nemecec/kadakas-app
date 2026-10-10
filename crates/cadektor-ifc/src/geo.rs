//! Georeference to IFC: the map conversion, the projected CRS and the site latitude and longitude.

use cadektor_core::georef::Georeference;
use cadektor_crs::Crs;

/// What the IFC file says about the position on the earth.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoInfo {
    /// Name of the projected CRS, for example `EPSG:3067`.
    pub crs_name: String,
    /// The CRS text when it is not an EPSG code (WKT or a PROJ string).
    pub description: Option<String>,
    pub eastings: f64,
    pub northings: f64,
    pub x_abscissa: f64,
    pub x_ordinate: f64,
    /// Map units (meters) per model unit (mm).
    pub scale: f64,
    /// Latitude and longitude of the model origin in degrees, if the CRS is known.
    pub lat_lon: Option<(f64, f64)>,
    /// True north as a direction in the model frame (x, y). It includes the rotation of the model and the
    /// meridian convergence of the CRS at the origin. Without a known CRS it is grid north.
    pub true_north: [f64; 2],
}

impl GeoInfo {
    /// Reads a georeference. `None` if the document names no CRS (then only a local shift is known).
    pub fn from_georef(g: &Georeference) -> Option<GeoInfo> {
        let text = g.crs.trim();
        if text.is_empty() || !g.is_finite() {
            return None;
        }
        let parsed = Crs::parse(text).ok();
        let epsg = parsed.as_ref().and_then(Crs::epsg);
        let (crs_name, description) = match epsg {
            Some(code) => (format!("EPSG:{code}"), None),
            None => ("Custom".to_string(), Some(text.to_string())),
        };
        let place = cadektor_crs::place_of(g)?;
        let (lat_lon, true_north) = (place.lat_lon, place.true_north);
        Some(GeoInfo {
            true_north: [true_north.x, true_north.y],
            crs_name,
            description,
            eastings: g.origin.x,
            northings: g.origin.y,
            x_abscissa: g.rotation.cos(),
            x_ordinate: g.rotation.sin(),
            scale: 0.001,
            lat_lon,
        })
    }
}

/// An angle in degrees as `IfcCompoundPlaneAngleMeasure`: degrees, minutes, seconds and millionths of
/// a second. The sign applies to every part.
pub fn compound_angle(deg: f64) -> [i64; 4] {
    let sign = if deg < 0.0 { -1 } else { 1 };
    let total = deg.abs();
    let mut d = total.floor();
    let mut m = ((total - d) * 60.0).floor();
    let mut s = (((total - d) * 60.0 - m) * 60.0).floor();
    let mut micro = (((((total - d) * 60.0 - m) * 60.0) - s) * 1.0e6).round();
    if micro >= 1.0e6 {
        micro -= 1.0e6;
        s += 1.0;
    }
    if s >= 60.0 {
        s -= 60.0;
        m += 1.0;
    }
    if m >= 60.0 {
        m -= 60.0;
        d += 1.0;
    }
    [
        sign * d as i64,
        sign * m as i64,
        sign * s as i64,
        sign * micro as i64,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    fn geo(crs: &str) -> Georeference {
        Georeference {
            crs: crs.into(),
            origin: DVec2::new(385000.0, 6672000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        }
    }

    #[test]
    fn epsg_georeference() {
        let g = GeoInfo::from_georef(&geo("EPSG:3067")).unwrap();
        assert_eq!(g.crs_name, "EPSG:3067");
        assert!(g.description.is_none());
        assert_eq!(
            (g.eastings, g.northings, g.scale),
            (385000.0, 6672000.0, 0.001)
        );
        assert_eq!((g.x_abscissa, g.x_ordinate), (1.0, 0.0));
        // Helsinki area.
        let (lat, lon) = g.lat_lon.unwrap();
        assert!((lat - 60.2).abs() < 0.3, "{lat}");
        assert!((lon - 24.9).abs() < 0.5, "{lon}");
    }

    #[test]
    fn true_north_includes_the_meridian_convergence() {
        // EPSG:3067 has its central meridian at 27 E. Helsinki is at 24.9 E, so true north leans
        // about 1.8 degrees away from grid north.
        let g = GeoInfo::from_georef(&geo("EPSG:3067")).unwrap();
        let angle = g.true_north[0].atan2(g.true_north[1]).to_degrees();
        assert!((angle.abs() - 1.8).abs() < 0.3, "{angle}");
        assert!((g.true_north[0].hypot(g.true_north[1]) - 1.0).abs() < 1e-9);
        // The rotation of the model turns it the other way.
        let mut r = geo("EPSG:3067");
        r.rotation = 0.2;
        let t = GeoInfo::from_georef(&r).unwrap().true_north;
        let turned = t[0].atan2(t[1]).to_degrees();
        assert!(
            (turned - (angle + 0.2f64.to_degrees())).abs() < 0.01,
            "{turned}"
        );
        // A CRS that is not known gives grid north.
        let unknown = GeoInfo::from_georef(&Georeference {
            rotation: 0.1,
            ..geo("MYSTERY")
        })
        .unwrap()
        .true_north;
        assert!(
            (unknown[0] - 0.1f64.sin()).abs() < 1e-12 && (unknown[1] - 0.1f64.cos()).abs() < 1e-12
        );
        // Longitude and latitude as CRS: grid north.
        let geographic = GeoInfo::from_georef(&Georeference {
            origin: DVec2::new(24.9, 60.2),
            ..geo("EPSG:4326")
        })
        .unwrap();
        assert_eq!(geographic.true_north, [0.0, 1.0]);
    }

    #[test]
    fn rotation_and_custom_crs() {
        let mut g = geo("EPSG:3067");
        g.rotation = std::f64::consts::FRAC_PI_2;
        let i = GeoInfo::from_georef(&g).unwrap();
        assert!(i.x_abscissa.abs() < 1e-12 && (i.x_ordinate - 1.0).abs() < 1e-12);
        let custom =
            GeoInfo::from_georef(&geo("+proj=utm +zone=35 +ellps=GRS80 +units=m")).unwrap();
        assert_eq!(custom.crs_name, "Custom");
        assert!(custom.description.as_deref().unwrap().starts_with("+proj"));
        assert!(custom.lat_lon.is_some());
        let unknown = GeoInfo::from_georef(&geo("MYSTERY")).unwrap();
        assert_eq!(unknown.crs_name, "Custom");
        assert!(unknown.lat_lon.is_none());
    }

    #[test]
    fn no_crs_gives_no_info() {
        assert!(GeoInfo::from_georef(&geo("")).is_none());
        assert!(GeoInfo::from_georef(&geo("  ")).is_none());
        let mut g = geo("EPSG:3067");
        g.rotation = f64::NAN;
        assert!(GeoInfo::from_georef(&g).is_none());
    }

    #[test]
    fn compound_angles() {
        assert_eq!(compound_angle(60.5), [60, 30, 0, 0]);
        assert_eq!(compound_angle(-24.25), [-24, -15, 0, 0]);
        assert_eq!(compound_angle(0.0), [0, 0, 0, 0]);
        let a = compound_angle(60.123456789);
        assert_eq!(&a[..2], &[60, 7]);
        let back = a[0] as f64 + a[1] as f64 / 60.0 + a[2] as f64 / 3600.0 + a[3] as f64 / 3.6e9;
        assert!((back - 60.123456789).abs() < 1e-8);
        // Rounding carries over.
        assert_eq!(compound_angle(10.999999999999), [11, 0, 0, 0]);
        assert_eq!(compound_angle(59.99999999999999), [60, 0, 0, 0]);
    }
}
