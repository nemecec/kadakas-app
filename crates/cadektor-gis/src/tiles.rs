//! Bounding boxes, tile grids and axis order. Pure math, no network.

use cadektor_core::DVec2;

/// Half the width of the Web Mercator (EPSG:3857) square, in meters.
pub const MERCATOR_HALF: f64 = 20_037_508.342_789_244;
/// Meters per degree on the WMTS reference sphere (the OGC value 6378137 m radius).
pub const METERS_PER_DEGREE: f64 = 111_319.490_793_273_58;
/// Size of a pixel in meters that scale denominators refer to (OGC WMTS 1.0.0).
pub const STANDARD_PIXEL_M: f64 = 0.00028;

/// Longitude and latitude in degrees to Web Mercator meters. The latitude is clamped to the
/// range that Web Mercator covers.
pub fn lonlat_to_mercator(lon: f64, lat: f64) -> DVec2 {
    let lat = lat.clamp(-85.051_128_78, 85.051_128_78);
    let x = lon * MERCATOR_HALF / 180.0;
    let y = (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
        .tan()
        .ln()
        * (MERCATOR_HALF / std::f64::consts::PI);
    DVec2::new(x, y)
}

/// Web Mercator meters to longitude and latitude in degrees.
pub fn mercator_to_lonlat(x: f64, y: f64) -> DVec2 {
    let lon = x * 180.0 / MERCATOR_HALF;
    let lat = (2.0 * (y * std::f64::consts::PI / MERCATOR_HALF).exp().atan()
        - std::f64::consts::FRAC_PI_2)
        .to_degrees();
    DVec2::new(lon, lat)
}

/// EPSG codes of projected CRSs whose official axis order is northing, easting. (Geographic CRSs
/// have latitude first, except CRS:84.) `proj4rs` has no axis information, so the table comes
/// from the axis definitions of the EPSG registry for the grids that are known to be north first:
/// Estonia (3301), Lithuania (3346), Latvia (3059), Poland (2176 to 2180), Sweden (3006 to 3016,
/// 3021), the European LAEA and LCC grids (3034, 3035), and the German Gauss-Kruger zones
/// (31466 to 31469). Most other grids (UTM, Web Mercator, ETRS-TM35FIN 3067, Swiss 2056 and
/// 21781, Lambert-93, British and Dutch grids) are easting first and need no entry.
const NORTHING_FIRST: [u32; 21] = [
    3301, 3346, 3059, 2176, 2177, 2178, 2179, 2180, 3006, 3007, 3008, 3009, 3010, 3011, 3012, 3013,
    3014, 3015, 3016, 3021, 3034,
];

/// More ranges that are north first.
fn northing_first_range(code: u32) -> bool {
    matches!(code, 3035 | 31466..=31469)
}

/// True if the official axis order of the CRS is y (north or latitude) first. WMS 1.3.0 and WFS 2.0
/// use this order in bounding boxes and positions.
pub fn northing_first(epsg: Option<u32>, geographic: bool) -> bool {
    match epsg {
        Some(c) if c == 4326 || c == 3857 => c == 4326,
        Some(c) => geographic || NORTHING_FIRST.contains(&c) || northing_first_range(c),
        None => false,
    }
}

/// A box in some CRS, x then y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bbox {
    pub min: DVec2,
    pub max: DVec2,
}

impl Bbox {
    /// A box from two corners in any order.
    pub fn new(a: DVec2, b: DVec2) -> Bbox {
        Bbox {
            min: a.min(b),
            max: a.max(b),
        }
    }

    pub fn from_points<I: IntoIterator<Item = DVec2>>(pts: I) -> Option<Bbox> {
        let mut it = pts.into_iter();
        let first = it.next()?;
        Some(it.fold(Bbox::new(first, first), |b, p| Bbox {
            min: b.min.min(p),
            max: b.max.max(p),
        }))
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }

    pub fn intersects(&self, o: &Bbox) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
    }

    pub fn union(&self, o: &Bbox) -> Bbox {
        Bbox {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    /// Grows the box by `f` of its size on each side.
    pub fn expand(&self, f: f64) -> Bbox {
        let d = (self.max - self.min) * f;
        Bbox {
            min: self.min - d,
            max: self.max + d,
        }
    }
}

/// The `BBOX` parameter of a WMS GetMap request. WMS 1.3.0 uses the axis order of the CRS (so
/// latitude first for EPSG:4326). Older versions always use x, y.
pub fn wms_bbox(b: &Bbox, epsg: Option<u32>, geographic: bool, version: &str) -> String {
    let swap = version.starts_with("1.3") && northing_first(epsg, geographic);
    let (a, c) = if swap {
        (DVec2::new(b.min.y, b.min.x), DVec2::new(b.max.y, b.max.x))
    } else {
        (b.min, b.max)
    };
    format!("{},{},{},{}", num(a.x), num(a.y), num(c.x), num(c.y))
}

/// A number for a URL: no exponent, at most 9 decimals, no trailing zeros.
pub fn num(v: f64) -> String {
    let s = format!("{v:.9}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// Reads a position pair from text in the axis order of a CRS and returns (x, y).
pub fn xy_from_text(a: f64, b: f64, y_first: bool) -> DVec2 {
    if y_first {
        DVec2::new(b, a)
    } else {
        DVec2::new(a, b)
    }
}

/// A grid of square tiles that all start at (0, 0) in the CRS. The pixel size is a power of two
/// times a base, so the same view at the same zoom asks for the same tiles, and the disk
/// cache works while the user pans.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileGrid {
    /// Size of a pixel in the units of the CRS.
    pub pixel: f64,
    /// Pixels on a side of a tile.
    pub tile_px: u32,
}

const GRID_BASE: f64 = 0.001;

impl TileGrid {
    /// The grid that gives about `target_px` pixels across `extent` units.
    pub fn for_extent(extent: f64, target_px: u32, tile_px: u32) -> TileGrid {
        let wanted = (extent / f64::from(target_px.max(1))).max(1.0e-12);
        let k = (wanted / GRID_BASE).log2().round();
        TileGrid {
            pixel: GRID_BASE * k.exp2(),
            tile_px: tile_px.max(16),
        }
    }

    /// Side of a tile in CRS units.
    pub fn tile_size(&self) -> f64 {
        self.pixel * f64::from(self.tile_px)
    }

    /// The tile columns and rows that cover the box: (col0, col1, row0, row1), both ends included.
    pub fn range(&self, b: &Bbox) -> (i64, i64, i64, i64) {
        let s = self.tile_size();
        let f = |v: f64| (v / s).floor() as i64;
        (f(b.min.x), f(b.max.x), f(b.min.y), f(b.max.y))
    }

    pub fn tile_count(&self, b: &Bbox) -> u64 {
        let (c0, c1, r0, r1) = self.range(b);
        ((c1 - c0 + 1) as u64).saturating_mul((r1 - r0 + 1) as u64)
    }

    pub fn tile_bbox(&self, col: i64, row: i64) -> Bbox {
        let s = self.tile_size();
        Bbox {
            min: DVec2::new(col as f64 * s, row as f64 * s),
            max: DVec2::new((col + 1) as f64 * s, (row + 1) as f64 * s),
        }
    }
}

/// One level of a WMTS tile matrix set.
#[derive(Clone, Debug, PartialEq)]
pub struct TileMatrix {
    pub id: String,
    pub scale_denominator: f64,
    /// Top left corner, as x, y in the CRS of the set.
    pub top_left: DVec2,
    pub tile_width: u32,
    pub tile_height: u32,
    pub matrix_width: u64,
    pub matrix_height: u64,
}

impl TileMatrix {
    /// Size of a pixel in CRS units. `meters_per_unit` is 1 for meters and `METERS_PER_DEGREE` for degrees.
    pub fn pixel_size(&self, meters_per_unit: f64) -> f64 {
        self.scale_denominator * STANDARD_PIXEL_M / meters_per_unit
    }

    /// Size of a tile in CRS units: (width, height).
    pub fn tile_span(&self, meters_per_unit: f64) -> DVec2 {
        let p = self.pixel_size(meters_per_unit);
        DVec2::new(
            p * f64::from(self.tile_width),
            p * f64::from(self.tile_height),
        )
    }

    /// The tiles that cover the box, clamped to the matrix: (col0, col1, row0, row1). `None` if the
    /// box does not touch the matrix.
    pub fn range(&self, b: &Bbox, meters_per_unit: f64) -> Option<(u64, u64, u64, u64)> {
        let span = self.tile_span(meters_per_unit);
        let col = |x: f64| ((x - self.top_left.x) / span.x).floor();
        let row = |y: f64| ((self.top_left.y - y) / span.y).floor();
        let (c0, c1) = (col(b.min.x), col(b.max.x));
        let (r0, r1) = (row(b.max.y), row(b.min.y));
        let (mw, mh) = (self.matrix_width as f64, self.matrix_height as f64);
        if c1 < 0.0 || r1 < 0.0 || c0 >= mw || r0 >= mh {
            return None;
        }
        Some((
            c0.max(0.0) as u64,
            c1.min(mw - 1.0) as u64,
            r0.max(0.0) as u64,
            r1.min(mh - 1.0) as u64,
        ))
    }

    /// The box of one tile.
    pub fn tile_bbox(&self, col: u64, row: u64, meters_per_unit: f64) -> Bbox {
        let span = self.tile_span(meters_per_unit);
        let min_x = self.top_left.x + col as f64 * span.x;
        let max_y = self.top_left.y - row as f64 * span.y;
        Bbox {
            min: DVec2::new(min_x, max_y - span.y),
            max: DVec2::new(min_x + span.x, max_y),
        }
    }
}

/// The matrix to use for a wanted pixel size: the coarsest one that is at least as fine as wanted,
/// or the finest if none is.
pub fn pick_matrix(
    matrices: &[TileMatrix],
    wanted_pixel: f64,
    meters_per_unit: f64,
) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    let mut finest: Option<(usize, f64)> = None;
    for (i, m) in matrices.iter().enumerate() {
        let p = m.pixel_size(meters_per_unit);
        if finest.is_none_or(|(_, fp)| p < fp) {
            finest = Some((i, p));
        }
        if p <= wanted_pixel * 1.0000001 && best.is_none_or(|(_, bp)| p > bp) {
            best = Some((i, p));
        }
    }
    best.or(finest).map(|b| b.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn mercator_known_points() {
        let p = lonlat_to_mercator(0.0, 0.0);
        assert!(p.length() < 1e-9);
        let p = lonlat_to_mercator(180.0, 0.0);
        assert!((p.x - MERCATOR_HALF).abs() < 1e-6);
        // Helsinki.
        let p = lonlat_to_mercator(24.9384, 60.1699);
        assert!((p.x - 2_776_130.0).abs() < 50.0, "{p}");
        assert!((p.y - 8_437_662.0).abs() < 50.0, "{p}");
        // The latitude is limited.
        let top = lonlat_to_mercator(0.0, 90.0);
        assert!((top.y - MERCATOR_HALF).abs() < 1.0);
    }

    proptest! {
        #[test]
        fn mercator_round_trip(lon in -179.9..179.9f64, lat in -85.0..85.0f64) {
            let p = lonlat_to_mercator(lon, lat);
            let q = mercator_to_lonlat(p.x, p.y);
            prop_assert!((q.x - lon).abs() < 1e-9);
            prop_assert!((q.y - lat).abs() < 1e-9);
        }

        #[test]
        fn grid_tiles_cover_the_box(
            x in -1.0e6..1.0e6f64, y in -1.0e6..1.0e6f64,
            w in 1.0..5000.0f64, h in 1.0..5000.0f64,
            px in 64u32..2000,
        ) {
            let b = Bbox::new(DVec2::new(x, y), DVec2::new(x + w, y + h));
            let g = TileGrid::for_extent(w, px, 256);
            let (c0, c1, r0, r1) = g.range(&b);
            let first = g.tile_bbox(c0, r0);
            let last = g.tile_bbox(c1, r1);
            prop_assert!(first.min.x <= b.min.x + 1e-6 && first.min.y <= b.min.y + 1e-6);
            prop_assert!(last.max.x >= b.max.x - 1e-6 && last.max.y >= b.max.y - 1e-6);
            prop_assert_eq!(g.tile_count(&b), ((c1 - c0 + 1) * (r1 - r0 + 1)) as u64);
            // Neighbors share an edge.
            let a = g.tile_bbox(c0, r0);
            let n = g.tile_bbox(c0 + 1, r0);
            prop_assert!((a.max.x - n.min.x).abs() < 1e-6 * a.max.x.abs().max(1.0));
        }

        #[test]
        fn wmts_tile_boxes_tile_the_plane(col in 0u64..16, row in 0u64..16, zoom in 0u32..4) {
            let m = web_mercator_matrix(zoom);
            let b = m.tile_bbox(col % m.matrix_width, row % m.matrix_height, 1.0);
            let inner = Bbox::new(b.min + (b.max - b.min) * 0.25, b.max - (b.max - b.min) * 0.25);
            let (c0, c1, r0, r1) = m.range(&inner, 1.0).unwrap();
            prop_assert_eq!((c0, c1, r0, r1), (col % m.matrix_width, col % m.matrix_width, row % m.matrix_height, row % m.matrix_height));
        }
    }

    pub(crate) fn web_mercator_matrix(z: u32) -> TileMatrix {
        let n = 1u64 << z;
        TileMatrix {
            id: z.to_string(),
            scale_denominator: 559_082_264.028_717_5 / (n as f64),
            top_left: DVec2::new(-MERCATOR_HALF, MERCATOR_HALF),
            tile_width: 256,
            tile_height: 256,
            matrix_width: n,
            matrix_height: n,
        }
    }

    #[test]
    fn web_mercator_matrix_has_the_known_pixel_size() {
        let m = web_mercator_matrix(0);
        assert!((m.pixel_size(1.0) - 156_543.033_928).abs() < 1e-3);
        let m10 = web_mercator_matrix(10);
        assert!((m10.pixel_size(1.0) - 152.874_057).abs() < 1e-3);
        let b = m10.tile_bbox(0, 0, 1.0);
        assert!((b.min.x + MERCATOR_HALF).abs() < 1e-6 && (b.max.y - MERCATOR_HALF).abs() < 1e-6);
        assert!((b.width() - 256.0 * m10.pixel_size(1.0)).abs() < 1e-6);
    }

    #[test]
    fn matrix_range_clamps_and_rejects() {
        let m = web_mercator_matrix(2);
        let all = Bbox::new(
            DVec2::splat(-MERCATOR_HALF * 2.0),
            DVec2::splat(MERCATOR_HALF * 2.0),
        );
        assert_eq!(m.range(&all, 1.0), Some((0, 3, 0, 3)));
        let outside = Bbox::new(
            DVec2::splat(MERCATOR_HALF * 2.0),
            DVec2::splat(MERCATOR_HALF * 3.0),
        );
        assert_eq!(m.range(&outside, 1.0), None);
        let left = Bbox::new(
            DVec2::new(-MERCATOR_HALF * 3.0, 0.0),
            DVec2::new(-MERCATOR_HALF * 2.0, 1.0),
        );
        assert_eq!(m.range(&left, 1.0), None);
    }

    #[test]
    fn picks_the_matrix_by_pixel_size() {
        let ms: Vec<TileMatrix> = (0..6).map(web_mercator_matrix).collect();
        // Level 3 has pixels of 19567 m.
        assert_eq!(pick_matrix(&ms, 20_000.0, 1.0), Some(3));
        assert_eq!(pick_matrix(&ms, 156_543.04, 1.0), Some(0));
        // Wanted finer than the finest: the finest.
        assert_eq!(pick_matrix(&ms, 1.0, 1.0), Some(5));
        // Coarser than all: the coarsest that is fine enough is the coarsest one.
        assert_eq!(pick_matrix(&ms, 1.0e9, 1.0), Some(0));
        assert_eq!(pick_matrix(&[], 1.0, 1.0), None);
    }

    #[test]
    fn bbox_helpers() {
        let a = Bbox::new(DVec2::new(5.0, 6.0), DVec2::new(1.0, 2.0));
        assert_eq!(a.min, DVec2::new(1.0, 2.0));
        assert_eq!((a.width(), a.height()), (4.0, 4.0));
        let b = Bbox::new(DVec2::new(4.0, 4.0), DVec2::new(9.0, 9.0));
        assert!(a.intersects(&b));
        assert!(!a.intersects(&Bbox::new(DVec2::splat(10.0), DVec2::splat(11.0))));
        assert_eq!(a.union(&b).max, DVec2::splat(9.0));
        assert_eq!(a.expand(0.5).min, DVec2::new(-1.0, 0.0));
        assert!(a.is_finite());
        assert!(!Bbox::new(DVec2::NAN, DVec2::ZERO).is_finite() || a.is_finite());
        assert_eq!(Bbox::from_points([]), None);
        assert_eq!(
            Bbox::from_points([DVec2::new(1.0, 5.0), DVec2::new(-2.0, 3.0)]).unwrap(),
            Bbox {
                min: DVec2::new(-2.0, 3.0),
                max: DVec2::new(1.0, 5.0)
            }
        );
    }

    #[test]
    fn axis_order_in_wms_bboxes() {
        let b = Bbox::new(DVec2::new(24.0, 60.0), DVec2::new(25.0, 61.0));
        assert_eq!(wms_bbox(&b, Some(4326), true, "1.3.0"), "60,24,61,25");
        assert_eq!(wms_bbox(&b, Some(4326), true, "1.1.1"), "24,60,25,61");
        assert_eq!(wms_bbox(&b, Some(3857), false, "1.3.0"), "24,60,25,61");
        assert_eq!(wms_bbox(&b, Some(3035), false, "1.3.0"), "60,24,61,25");
        // Estonian L-EST97 is north first. The Swiss grids and the Finnish grid are east first.
        assert_eq!(wms_bbox(&b, Some(3301), false, "1.3.0"), "60,24,61,25");
        for east_first in [2056, 21781, 3067, 3857, 32635, 25832, 2154, 27700, 28992] {
            assert!(!northing_first(Some(east_first), false), "{east_first}");
        }
        for north_first in [3301, 3346, 3059, 2180, 3006, 31467, 4326] {
            assert!(
                northing_first(Some(north_first), north_first == 4326),
                "{north_first}"
            );
        }
        assert_eq!(wms_bbox(&b, None, false, "1.3.0"), "24,60,25,61");
        assert!(northing_first(Some(4258), true));
        assert!(!northing_first(Some(3067), false));
        assert_eq!(xy_from_text(60.0, 24.0, true), DVec2::new(24.0, 60.0));
        assert_eq!(xy_from_text(24.0, 60.0, false), DVec2::new(24.0, 60.0));
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(100.0), "100");
        assert_eq!(num(0.000_000_000_1), "0");
    }
}
