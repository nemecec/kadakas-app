//! Coordinate reference systems: reading names and WKT, and converting points.
//!
//! The math comes from `proj4rs` (pure Rust). This module reads a CRS from an
//! EPSG code, a PROJ string or WKT (the `.prj` of a shapefile), and moves points
//! from one CRS to another.

use proj4rs::Proj;

/// A CRS that is ready to convert points.
pub struct Crs {
    /// `EPSG:<code>` when the code is known, else the text that the user gave.
    pub spec: String,
    proj: Proj,
    geographic: bool,
    to_meter: f64,
}

impl std::fmt::Debug for Crs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Crs({})", self.spec)
    }
}

impl Crs {
    /// Reads `EPSG:n`, a bare code, a URN, a PROJ string (`+proj=...`) or WKT.
    pub fn parse(text: &str) -> Result<Crs, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("empty CRS".into());
        }
        if t.starts_with('+') {
            return Crs::from_proj(t, t);
        }
        if let Some(code) = epsg_code(t) {
            return Crs::from_epsg(code);
        }
        if t.eq_ignore_ascii_case("wgs84") {
            return Crs::from_epsg(4326);
        }
        if t.contains('[') {
            if let Some(Ok(c)) = wkt_authority(t).map(Crs::from_epsg) {
                return Ok(c);
            }
            return Crs::from_proj(&wkt_to_proj(t)?, t);
        }
        Err(format!("unknown CRS: {t}"))
    }

    pub fn from_epsg(code: u32) -> Result<Crs, String> {
        let code16 = u16::try_from(code).map_err(|_| format!("unknown EPSG code {code}"))?;
        let proj = Proj::from_epsg_code(code16).map_err(|e| format!("EPSG:{code}: {e}"))?;
        Ok(Crs::wrap(format!("EPSG:{code}"), proj))
    }

    fn from_proj(def: &str, spec: &str) -> Result<Crs, String> {
        let proj = Proj::from_proj_string(def).map_err(|e| format!("cannot use CRS: {e}"))?;
        Ok(Crs::wrap(spec.to_string(), proj))
    }

    fn wrap(spec: String, proj: Proj) -> Crs {
        let geographic = proj.is_latlong();
        let to_meter = if geographic { 1.0 } else { proj.to_meter() };
        Crs {
            spec,
            proj,
            geographic,
            to_meter,
        }
    }

    /// True for longitude and latitude in degrees.
    pub fn is_geographic(&self) -> bool {
        self.geographic
    }

    /// Meters per unit of a projected CRS (1 for geographic CRSs).
    pub fn to_meter(&self) -> f64 {
        self.to_meter
    }

    /// The EPSG code, if `spec` is one.
    pub fn epsg(&self) -> Option<u32> {
        epsg_code(&self.spec)
    }
}

/// The UTM zone (WGS 84) for a longitude and latitude in degrees, as an EPSG code.
pub fn utm_epsg(lon: f64, lat: f64) -> u32 {
    let lon = if lon.is_finite() && lon.abs() > 180.0 {
        (lon + 180.0).rem_euclid(360.0) - 180.0
    } else {
        lon
    };
    let zone = ((lon + 180.0) / 6.0).floor().clamp(0.0, 59.0) as u32 + 1;
    if lat >= 0.0 {
        32600 + zone
    } else {
        32700 + zone
    }
}

fn epsg_code(t: &str) -> Option<u32> {
    let lower = t.to_ascii_lowercase();
    if lower == "crs84"
        || lower == "ogc:crs84"
        || lower.ends_with("/crs84")
        || lower.ends_with(":crs84")
    {
        return Some(4326);
    }
    let digits = if let Some(rest) = lower.strip_prefix("epsg:") {
        rest
    } else if lower.contains("def/crs/epsg") || lower.starts_with("urn:ogc:def:crs:epsg") {
        lower.rsplit([':', '/']).next()?
    } else if lower.bytes().all(|b| b.is_ascii_digit()) {
        &lower
    } else {
        return None;
    };
    digits.trim().parse().ok()
}

/// The EPSG code of the whole CRS: the `AUTHORITY["EPSG","n"]` (WKT 1) or `ID["EPSG",n]` (WKT 2)
/// that is a direct child of the root node. Codes of the base geographic CRS or the datum are not used.
fn wkt_authority(wkt: &str) -> Option<u32> {
    let root = parse_wkt(wkt).ok()?;
    ["AUTHORITY", "ID"].iter().find_map(|key| {
        let n = root.child(key)?;
        if !n.string(0)?.eq_ignore_ascii_case("EPSG") {
            return None;
        }
        match n.args.get(1)? {
            Arg::Num(v) if *v >= 0.0 && v.fract() == 0.0 => Some(*v as u32),
            Arg::Str(t) => t.trim().parse().ok(),
            _ => None,
        }
    })
}

#[derive(Debug)]
enum Arg {
    Num(f64),
    Str(String),
    Node(Node),
}

#[derive(Debug)]
struct Node {
    name: String,
    args: Vec<Arg>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.args.iter().find_map(|a| match a {
            Arg::Node(n) if n.name.eq_ignore_ascii_case(name) => Some(n),
            _ => None,
        })
    }

    fn num(&self, i: usize) -> Option<f64> {
        match self.args.get(i)? {
            Arg::Num(v) => Some(*v),
            _ => None,
        }
    }

    fn string(&self, i: usize) -> Option<&str> {
        match self.args.get(i)? {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Value of `PARAMETER["name", value]`, matching any of the names.
    fn param(&self, names: &[&str]) -> Option<f64> {
        self.args.iter().find_map(|a| match a {
            Arg::Node(n) if n.name.eq_ignore_ascii_case("PARAMETER") => {
                let pname = n.string(0)?.to_ascii_lowercase();
                names.contains(&pname.as_str()).then(|| n.num(1)).flatten()
            }
            _ => None,
        })
    }
}

/// Deepest nesting of WKT that is read.
const WKT_DEPTH: usize = 16;

fn parse_wkt(s: &str) -> Result<Node, String> {
    let b = s.as_bytes();
    let mut i = 0;
    let node = parse_node(b, &mut i, 0)?;
    Ok(node)
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && (b[*i] as char).is_whitespace() {
        *i += 1;
    }
}

fn parse_node(b: &[u8], i: &mut usize, depth: usize) -> Result<Node, String> {
    if depth > WKT_DEPTH {
        return Err("WKT is nested too deep".into());
    }
    skip_ws(b, i);
    let start = *i;
    while *i < b.len() && (b[*i].is_ascii_alphanumeric() || b[*i] == b'_') {
        *i += 1;
    }
    let name = String::from_utf8_lossy(&b[start..*i]).into_owned();
    if name.is_empty() {
        return Err("bad WKT".into());
    }
    skip_ws(b, i);
    let mut args = Vec::new();
    if *i < b.len() && (b[*i] == b'[' || b[*i] == b'(') {
        *i += 1;
        loop {
            skip_ws(b, i);
            match b.get(*i) {
                None => return Err("WKT ends early".into()),
                Some(b']' | b')') => {
                    *i += 1;
                    break;
                }
                Some(b',') => *i += 1,
                Some(b'"') => {
                    *i += 1;
                    let s = *i;
                    while *i < b.len() && b[*i] != b'"' {
                        *i += 1;
                    }
                    if *i >= b.len() {
                        return Err("WKT ends early".into());
                    }
                    args.push(Arg::Str(String::from_utf8_lossy(&b[s..*i]).into_owned()));
                    *i += 1;
                }
                Some(c) if c.is_ascii_digit() || *c == b'-' || *c == b'+' || *c == b'.' => {
                    let s = *i;
                    while *i < b.len() && (b[*i].is_ascii_digit() || b"+-.eE".contains(&b[*i])) {
                        *i += 1;
                    }
                    let text = String::from_utf8_lossy(&b[s..*i]).into_owned();
                    args.push(Arg::Num(
                        text.parse()
                            .map_err(|_| format!("bad number {text} in WKT"))?,
                    ));
                }
                Some(_) => args.push(Arg::Node(parse_node(b, i, depth + 1)?)),
            }
        }
    }
    Ok(Node { name, args })
}

/// Datum and ellipsoid part of a PROJ string, from a GEOGCS node.
fn geog_to_proj(g: &Node) -> Result<String, String> {
    let datum = g.child("DATUM");
    let sph = datum
        .and_then(|d| d.child("SPHEROID"))
        .ok_or("WKT has no SPHEROID")?;
    let (a, rf) = (sph.num(1).ok_or("bad SPHEROID")?, sph.num(2).unwrap_or(0.0));
    let mut out = if rf > 0.0 {
        format!("+a={a} +rf={rf}")
    } else {
        format!("+a={a} +b={a}")
    };
    let dname = datum
        .and_then(|d| d.string(0))
        .unwrap_or("")
        .to_ascii_lowercase();
    if let Some(t) = datum.and_then(|d| d.child("TOWGS84")) {
        let v: Vec<String> = t
            .args
            .iter()
            .filter_map(|a| {
                if let Arg::Num(n) = a {
                    Some(n.to_string())
                } else {
                    None
                }
            })
            .collect();
        if v.len() == 3 || v.len() == 7 {
            out += &format!(" +towgs84={}", v.join(","));
        }
    } else if dname.contains("wgs_1984")
        || dname.contains("wgs 84")
        || dname.contains("etrs")
        || dname.contains("etrf")
        || dname.contains("nad83")
    {
        // These datums agree with WGS 84 to about a meter.
        out += " +towgs84=0,0,0";
    }
    Ok(out)
}

fn wkt_to_proj(wkt: &str) -> Result<String, String> {
    let root = parse_wkt(wkt)?;
    let name = root.name.to_ascii_uppercase();
    match name.as_str() {
        "GEOGCS" => Ok(format!("+proj=longlat {} +no_defs", geog_to_proj(&root)?)),
        "PROJCS" => {
            let geog = root.child("GEOGCS").ok_or("PROJCS has no GEOGCS")?;
            let projection = root
                .child("PROJECTION")
                .and_then(|p| p.string(0))
                .ok_or("PROJCS has no PROJECTION")?
                .to_ascii_lowercase();
            let code = match projection.as_str() {
                "transverse_mercator" | "gauss_kruger" | "transverse_mercator_south_orientated" => {
                    "tmerc"
                }
                "mercator_1sp" | "mercator_2sp" | "mercator" | "mercator_auxiliary_sphere" => {
                    "merc"
                }
                "lambert_conformal_conic_2sp"
                | "lambert_conformal_conic_1sp"
                | "lambert_conformal_conic" => "lcc",
                "albers" | "albers_conic_equal_area" => "aea",
                "stereographic" | "polar_stereographic" => "stere",
                "oblique_stereographic" | "double_stereographic" => "sterea",
                "lambert_azimuthal_equal_area" => "laea",
                _ => return Err(format!("unsupported projection: {projection}")),
            };
            let p = |names: &[&str], default: f64| root.param(names).unwrap_or(default);
            let lat0 = p(
                &[
                    "latitude_of_origin",
                    "latitude_of_center",
                    "latitude_of_natural_origin",
                ],
                0.0,
            );
            let lon0 = p(
                &[
                    "central_meridian",
                    "longitude_of_center",
                    "longitude_of_natural_origin",
                    "longitude_of_origin",
                ],
                0.0,
            );
            let mut out = format!("+proj={code} +lat_0={lat0} +lon_0={lon0}");
            if code == "lcc" || code == "aea" {
                let l1 = p(&["standard_parallel_1"], lat0);
                let l2 = p(&["standard_parallel_2"], l1);
                out += &format!(" +lat_1={l1} +lat_2={l2}");
            }
            if code == "merc"
                && let Some(l1) = root.param(&["standard_parallel_1"])
            {
                out += &format!(" +lat_ts={l1}");
            }
            let unit = root
                .args
                .iter()
                .rev()
                .find_map(|a| match a {
                    Arg::Node(n) if n.name.eq_ignore_ascii_case("UNIT") => n.num(1),
                    _ => None,
                })
                .filter(|u| *u > 0.0)
                .unwrap_or(1.0);
            // False easting and northing are in the unit of the CRS in WKT, and in meters in PROJ.
            out += &format!(
                " +k={} +x_0={} +y_0={}",
                p(&["scale_factor", "scaling_factor"], 1.0),
                p(&["false_easting"], 0.0) * unit,
                p(&["false_northing"], 0.0) * unit
            );
            out += &format!(" {}", geog_to_proj(geog)?);
            out += &format!(" +to_meter={unit} +no_defs");
            Ok(out)
        }
        _ => Err(format!("unsupported WKT: {}", root.name)),
    }
}

/// Converts points between two CRSs.
pub struct Transformer<'a> {
    from: &'a Crs,
    to: &'a Crs,
}

impl<'a> Transformer<'a> {
    pub fn new(from: &'a Crs, to: &'a Crs) -> Self {
        Transformer { from, to }
    }

    /// Converts a point. The coordinates are in the unit of each CRS, as they are in a file:
    /// degrees for geographic CRSs, and for projected CRSs the unit of the CRS (meters, feet, ...).
    /// `proj4rs` applies the unit of the CRS itself. Use [`Crs::to_meter`] to get meters.
    pub fn apply(&self, x: f64, y: f64, z: f64) -> Result<(f64, f64, f64), String> {
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return Err("coordinate is not finite".into());
        }
        let f = self.from;
        if f.geographic && (x.abs() > 180.0 || y.abs() > 90.0) {
            return Err("longitude or latitude is out of range".into());
        }
        let (sx, sy) = if f.geographic {
            (x.to_radians(), y.to_radians())
        } else {
            (x, y)
        };
        let mut p = (sx, sy, z);
        proj4rs::transform::transform(&f.proj, &self.to.proj, &mut p)
            .map_err(|e| format!("cannot project coordinate: {e}"))?;
        let (ox, oy) = if self.to.geographic {
            (p.0.to_degrees(), p.1.to_degrees())
        } else {
            (p.0, p.1)
        };
        if ox.is_finite() && oy.is_finite() && p.2.is_finite() {
            Ok((ox, oy, p.2))
        } else {
            Err("cannot project coordinate".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TM35FIN_PRJ: &str = "PROJCS[\"ETRS_1989_TM35FIN\",GEOGCS[\"GCS_ETRS_1989\",DATUM[\"D_ETRS_1989\",SPHEROID[\"GRS_1980\",6378137.0,298.257222101]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],PARAMETER[\"False_Easting\",500000.0],PARAMETER[\"False_Northing\",0.0],PARAMETER[\"Central_Meridian\",27.0],PARAMETER[\"Scale_Factor\",0.9996],PARAMETER[\"Latitude_Of_Origin\",0.0],UNIT[\"Meter\",1.0]]";

    #[test]
    fn parses_names() {
        assert_eq!(Crs::parse("EPSG:3067").unwrap().epsg(), Some(3067));
        assert_eq!(Crs::parse("epsg:4326").unwrap().epsg(), Some(4326));
        assert_eq!(Crs::parse("32635").unwrap().epsg(), Some(32635));
        assert_eq!(
            Crs::parse("urn:ogc:def:crs:EPSG::25832").unwrap().epsg(),
            Some(25832)
        );
        assert_eq!(
            Crs::parse("http://www.opengis.net/def/crs/EPSG/0/3857")
                .unwrap()
                .epsg(),
            Some(3857)
        );
        assert_eq!(
            Crs::parse("urn:ogc:def:crs:OGC:1.3:CRS84").unwrap().epsg(),
            Some(4326)
        );
        assert_eq!(Crs::parse("WGS84").unwrap().epsg(), Some(4326));
        assert!(
            Crs::parse("+proj=utm +zone=35 +datum=WGS84")
                .unwrap()
                .epsg()
                .is_none()
        );
        assert!(Crs::parse("").is_err());
        assert!(Crs::parse("banana").is_err());
        assert!(Crs::parse("EPSG:99999999").is_err());
        assert!(Crs::parse("EPSG:1").is_err());
        assert!(Crs::parse("+proj=nonsense").is_err());
        assert!(Crs::parse("EPSG:4326").unwrap().is_geographic());
        assert!(!Crs::parse("EPSG:3067").unwrap().is_geographic());
        assert_eq!(
            format!("{:?}", Crs::parse("EPSG:3067").unwrap()),
            "Crs(EPSG:3067)"
        );
    }

    #[test]
    fn utm_zones() {
        assert_eq!(utm_epsg(24.9, 60.2), 32635);
        assert_eq!(utm_epsg(-122.4, 37.8), 32610);
        assert_eq!(utm_epsg(151.2, -33.9), 32756);
        assert_eq!(utm_epsg(180.0, 0.0), 32660);
        assert_eq!(utm_epsg(-180.0, 0.0), 32601);
        assert_eq!(utm_epsg(181.0, 0.0), 32601);
        assert_eq!(utm_epsg(-181.0, 0.0), 32660);
    }

    #[test]
    fn helsinki_to_tm35fin() {
        let wgs = Crs::parse("EPSG:4326").unwrap();
        let tm = Crs::parse("EPSG:3067").unwrap();
        let (x, y, z) = Transformer::new(&wgs, &tm)
            .apply(24.9384, 60.1699, 12.0)
            .unwrap();
        assert!(
            (x - 385_700.0).abs() < 1500.0 && (y - 6_672_000.0).abs() < 1500.0,
            "{x} {y}"
        );
        assert_eq!(z, 12.0);
        let (lon, lat, _) = Transformer::new(&tm, &wgs).apply(x, y, 0.0).unwrap();
        assert!((lon - 24.9384).abs() < 1e-7 && (lat - 60.1699).abs() < 1e-7);
        assert!(
            Transformer::new(&wgs, &tm)
                .apply(f64::NAN, 0.0, 0.0)
                .is_err()
        );
        assert!(Transformer::new(&wgs, &tm).apply(200.0, 0.0, 0.0).is_err());
        assert_eq!(utm_epsg(f64::NAN, 1e300), 32601);
    }

    #[test]
    fn wkt_with_and_without_authority() {
        let from_wkt = Crs::parse(TM35FIN_PRJ).unwrap();
        let from_epsg = Crs::parse("EPSG:3067").unwrap();
        let wgs = Crs::parse("EPSG:4326").unwrap();
        let a = Transformer::new(&wgs, &from_wkt)
            .apply(24.9384, 60.1699, 0.0)
            .unwrap();
        let b = Transformer::new(&wgs, &from_epsg)
            .apply(24.9384, 60.1699, 0.0)
            .unwrap();
        assert!(
            (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.0,
            "{a:?} {b:?}"
        );
        let with_auth = TM35FIN_PRJ.replace(
            "UNIT[\"Meter\",1.0]]",
            "UNIT[\"Meter\",1.0],AUTHORITY[\"EPSG\",\"3067\"]]",
        );
        assert_eq!(Crs::parse(&with_auth).unwrap().epsg(), Some(3067));
        let wkt2 = "PROJCRS[\"x\",BASEGEOGCRS[\"y\",ID[\"EPSG\",4258]],ID[\"EPSG\",25832]]";
        assert_eq!(Crs::parse(wkt2).unwrap().epsg(), Some(25832));
    }

    #[test]
    fn wkt_variants() {
        let geog = "GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]]";
        assert!(Crs::parse(geog).unwrap().is_geographic());
        let lcc = "PROJCS[\"lcc\",GEOGCS[\"g\",DATUM[\"d\",SPHEROID[\"s\",6378137,298.257222101],TOWGS84[0,0,0,0,0,0,0]]],PROJECTION[\"Lambert_Conformal_Conic_2SP\"],PARAMETER[\"standard_parallel_1\",49],PARAMETER[\"standard_parallel_2\",44],PARAMETER[\"latitude_of_origin\",46.5],PARAMETER[\"central_meridian\",3],PARAMETER[\"false_easting\",700000],PARAMETER[\"false_northing\",6600000],UNIT[\"foot\",0.3048]]";
        let c = Crs::parse(lcc).unwrap();
        let wgs = Crs::parse("EPSG:4326").unwrap();
        let (x, _, _) = Transformer::new(&wgs, &c).apply(3.0, 46.5, 0.0).unwrap();
        // The coordinates of a feet CRS are in feet: the false easting is 700000.
        assert!((x - 700_000.0).abs() < 1.0, "{x}");
        let (lon, lat, _) = Transformer::new(&c, &wgs)
            .apply(x, 6_600_000.0, 0.0)
            .unwrap();
        assert!((lon - 3.0).abs() < 1e-6 && (lat - 46.5).abs() < 1e-6);
        for proj in [
            "Mercator_1SP",
            "Albers",
            "Stereographic",
            "Oblique_Stereographic",
            "Lambert_Azimuthal_Equal_Area",
        ] {
            let w = format!(
                "PROJCS[\"p\",GEOGCS[\"g\",DATUM[\"d\",SPHEROID[\"s\",6378137,0]]],PROJECTION[\"{proj}\"],PARAMETER[\"standard_parallel_1\",30],UNIT[\"m\",1]]"
            );
            assert!(Crs::parse(&w).is_ok(), "{proj}");
        }
        assert!(Crs::parse("PROJCS[\"p\",GEOGCS[\"g\",DATUM[\"d\",SPHEROID[\"s\",6378137,298]]],PROJECTION[\"Nonsense\"]]").is_err());
        assert!(Crs::parse("PROJCS[\"p\"]").is_err());
        assert!(Crs::parse("PROJCS[\"p\",GEOGCS[\"g\"],PROJECTION[\"Mercator\"]]").is_err());
        assert!(Crs::parse("VERTCS[\"v\"]").is_err());
        assert!(Crs::parse("GEOGCS[\"unclosed").is_err());
        assert!(Crs::parse("GEOGCS[\"g\",DATUM[").is_err());
        assert!(Crs::parse("GEOGCS[1.2.3]").is_err());
        assert!(Crs::parse("[").is_err());
        assert!(Crs::parse(&format!("{}{}", "A[".repeat(40), "]".repeat(40))).is_err());
    }

    #[test]
    fn feet_crs_keeps_its_units() {
        // New York Long Island, NAD83, US survey feet (EPSG:2263).
        let ny = Crs::parse("EPSG:2263").unwrap();
        let wgs = Crs::parse("EPSG:4326").unwrap();
        assert!((ny.to_meter() - 0.3048006).abs() < 1e-6);
        let (x, y, _) = Transformer::new(&wgs, &ny)
            .apply(-74.0, 40.743, 0.0)
            .unwrap();
        // Feet, not meters: pyproj gives about (986000, 212000).
        assert!(
            (x - 985_900.0).abs() < 3000.0 && (y - 212_800.0).abs() < 3000.0,
            "{x} {y}"
        );
        let (lon, lat, _) = Transformer::new(&ny, &wgs).apply(x, y, 0.0).unwrap();
        assert!(
            (lon + 74.0).abs() < 1e-6 && (lat - 40.743).abs() < 1e-6,
            "{lon} {lat}"
        );
    }

    #[test]
    fn only_the_root_authority_counts() {
        // A custom TM CRS on a WGS 84 base: the GEOGCS authority 4326 must not win.
        let custom = "PROJCS[\"custom\",GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,298.257223563]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433],AUTHORITY[\"EPSG\",\"4326\"]],PROJECTION[\"Transverse_Mercator\"],PARAMETER[\"False_Easting\",500000.0],PARAMETER[\"False_Northing\",0.0],PARAMETER[\"Central_Meridian\",25.0],PARAMETER[\"Scale_Factor\",1.0],PARAMETER[\"Latitude_Of_Origin\",0.0],UNIT[\"Meter\",1.0]]";
        let c = Crs::parse(custom).unwrap();
        assert!(!c.is_geographic());
        assert_eq!(c.epsg(), None);
        let wgs = Crs::parse("EPSG:4326").unwrap();
        let (x, _, _) = Transformer::new(&wgs, &c).apply(25.0, 60.0, 0.0).unwrap();
        assert!((x - 500_000.0).abs() < 1.0, "{x}");
        assert_eq!(
            wkt_authority("GEOGCS[\"g\",AUTHORITY[\"EPSG\",\"4326\"]]"),
            Some(4326)
        );
        assert_eq!(
            wkt_authority("GEOGCS[\"g\",AUTHORITY[\"EPSG\",\"4326\"]"),
            None
        );
        assert_eq!(
            wkt_authority("PROJCS[\"p\",AUTHORITY[\"EPSG\",\"3067\"]]"),
            Some(3067)
        );
        assert_eq!(wkt_authority("PROJCS[\"p\",ID[\"EPSG\",3067]]"), Some(3067));
        assert_eq!(
            wkt_authority("PROJCS[\"p\",AUTHORITY[\"ESRI\",\"1\"]]"),
            None
        );
    }
}
