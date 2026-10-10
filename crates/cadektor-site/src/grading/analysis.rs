//! Slope and elevation analysis of a site model: every triangle falls into a band, and the bands
//! have colors. The settings are on the site model object (`Object::data["site_analysis"]`).
//! A legend object (a rectangle with `Object::data["analysis_legend"]`) shows the bands.
//!
//! The classes are computed per triangle and kept by the content of the site model, so a change
//! of anything else in the drawing does not compute them again.

use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::cell::Cell;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use cadektor_core::model::{Object, ObjectId, Shape};

use super::{Follow, esc, meters, read};
use crate::model::SiteResult;
use crate::tin::Tin;

/// The key on a site model object that holds the analysis settings.
pub const KEY: &str = "site_analysis";
/// The key on a legend object.
pub const LEGEND_KEY: &str = "analysis_legend";
/// Most class limits.
pub const MAX_BREAKS: usize = 40;
/// Default slope class limits, in percent.
pub const SLOPE_BREAKS: [f64; 6] = [2.0, 5.0, 10.0, 15.0, 25.0, 50.0];
const CACHE_SIZE: usize = 64;

/// What the site model shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    /// Color bands by slope in percent.
    Slope,
    /// Color bands by height (hypsometric tints).
    Elevation,
}

/// The analysis settings of a site model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisSpec {
    pub mode: Mode,
    pub surface: Follow,
    /// Class limits, ascending: slope in percent, or elevation in mm. N limits make N + 1 bands.
    /// Empty: the defaults of the mode.
    pub breaks: Vec<f64>,
    /// One color per band. Empty: a ramp from green to red.
    pub colors: Vec<[u8; 4]>,
    /// How much of the color shows in 2D, from 0 to 1.
    pub opacity: f64,
}

impl Default for AnalysisSpec {
    fn default() -> Self {
        AnalysisSpec { mode: Mode::Off, surface: Follow::Proposed, breaks: Vec::new(), colors: Vec::new(), opacity: 0.8 }
    }
}

impl AnalysisSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.breaks.len() > MAX_BREAKS {
            return Err(format!("at most {MAX_BREAKS} limits"));
        }
        if self.breaks.iter().any(|b| !b.is_finite()) || self.breaks.windows(2).any(|w| w[0] >= w[1]) {
            return Err("breaks must be finite numbers in ascending order".into());
        }
        if !self.colors.is_empty() && self.colors.len() != self.breaks.len() + 1 {
            return Err(format!("give {} colors for {} breaks (one more than breaks), or none", self.breaks.len() + 1, self.breaks.len()));
        }
        if !(self.opacity.is_finite() && (0.0..=1.0).contains(&self.opacity)) {
            return Err("opacity must be from 0 to 1".into());
        }
        Ok(())
    }
}

/// The analysis settings of a site model object.
pub fn spec_of(obj: &Object) -> Option<AnalysisSpec> {
    if !matches!(obj.shape, Shape::SiteModel { .. }) {
        return None;
    }
    read(obj, KEY)
}

/// A ramp of `n` colors from green through yellow to red (low to high).
pub fn ramp(n: usize) -> Vec<[u8; 4]> {
    const STOPS: [[f64; 3]; 5] = [[46.0, 139.0, 87.0], [170.0, 205.0, 80.0], [250.0, 225.0, 90.0], [240.0, 140.0, 50.0], [190.0, 50.0, 40.0]];
    (0..n)
        .map(|i| {
            let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 } * (STOPS.len() - 1) as f64;
            let k = (t.floor() as usize).min(STOPS.len() - 2);
            let f = t - k as f64;
            let c = |j: usize| (STOPS[k][j] + (STOPS[k + 1][j] - STOPS[k][j]) * f).round() as u8;
            [c(0), c(1), c(2), 255]
        })
        .collect()
}

/// The class a value falls into: the number of limits at or below it.
pub fn band_of(breaks: &[f64], v: f64) -> usize {
    breaks.partition_point(|b| *b <= v)
}

/// One class of an analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub index: usize,
    /// Lower limit, `None` for the first band.
    pub from: Option<f64>,
    /// Upper limit, `None` for the last band.
    pub to: Option<f64>,
    pub color: [u8; 4],
    pub label: String,
    pub triangles: usize,
    /// Area seen from above, mm2.
    pub area_xy: f64,
    /// Area of the sloped triangles, mm2.
    pub area_surface: f64,
}

/// The classes of every triangle of one surface.
#[derive(Clone, Debug)]
pub struct AnalysisResult {
    pub mode: Mode,
    pub surface: Follow,
    pub breaks: Vec<f64>,
    pub opacity: f64,
    /// The band of each triangle of the surface.
    pub tri_band: Vec<u16>,
    pub bands: Vec<Band>,
}

fn label(mode: Mode, from: Option<f64>, to: Option<f64>) -> String {
    let f = |v: f64| match mode {
        Mode::Slope => format!("{v}"),
        _ => meters(v, 2),
    };
    let unit = if mode == Mode::Slope { "%" } else { "m" };
    match (from, to) {
        (None, Some(t)) => format!("below {} {unit}", f(t)),
        (Some(a), Some(b)) => format!("{} to {} {unit}", f(a), f(b)),
        (Some(a), None) => format!("{} {unit} and above", f(a)),
        (None, None) => "all".into(),
    }
}

/// The class limits that the settings use: their own, or the defaults of the mode. The default
/// elevation limits divide the height range of the surface into 7 equal steps.
pub fn resolve_breaks(spec: &AnalysisSpec, tin: &Tin) -> Vec<f64> {
    if !spec.breaks.is_empty() {
        return spec.breaks.clone();
    }
    match spec.mode {
        Mode::Off => Vec::new(),
        Mode::Slope => SLOPE_BREAKS.to_vec(),
        Mode::Elevation => {
            let (lo, hi) = tin.z_range();
            if hi - lo < 1e-9 { Vec::new() } else { (1..7).map(|i| lo + (hi - lo) * i as f64 / 7.0).collect() }
        }
    }
}

/// Classes every triangle of a surface. `colors` is one per band (or empty for the ramp).
pub fn analyze(tin: &Tin, mode: Mode, breaks: &[f64], colors: &[[u8; 4]]) -> AnalysisResult {
    let n = breaks.len() + 1;
    let colors: Vec<[u8; 4]> = if colors.len() == n { colors.to_vec() } else { ramp(n) };
    let mut bands: Vec<Band> = (0..n)
        .map(|i| {
            let (from, to) = (i.checked_sub(1).map(|j| breaks[j]), breaks.get(i).copied());
            Band { index: i, from, to, color: colors[i], label: label(mode, from, to), triangles: 0, area_xy: 0.0, area_surface: 0.0 }
        })
        .collect();
    let mut tri_band = Vec::with_capacity(tin.triangles.len());
    for t in 0..tin.triangles.len() {
        let [a, b, c] = tin.corners(t);
        let value = match mode {
            Mode::Elevation => (a.z + b.z + c.z) / 3.0,
            _ => tin.triangle_slope(t).percent(),
        };
        let i = band_of(breaks, value);
        let cross = (b - a).cross(c - a);
        bands[i].triangles += 1;
        bands[i].area_xy += cross.z.abs() / 2.0;
        bands[i].area_surface += cross.length() / 2.0;
        tri_band.push(i as u16);
    }
    AnalysisResult { mode, surface: Follow::Proposed, breaks: breaks.to_vec(), opacity: 0.8, tri_band, bands }
}

thread_local! {
    static COMPUTED: Cell<u64> = const { Cell::new(0) };
}

/// How many analyses this thread computed (did not find in the cache) so far.
pub fn computed() -> u64 {
    COMPUTED.with(Cell::get)
}

fn cache() -> &'static Mutex<HashMap<u64, Arc<AnalysisResult>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<AnalysisResult>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The analysis of a site model for the settings, from the cache when the content of the site
/// model and the settings are the same. `None` when the analysis is off.
pub fn for_site(site: &SiteResult, spec: &AnalysisSpec) -> Option<Arc<AnalysisResult>> {
    if spec.mode == Mode::Off {
        return None;
    }
    let mut h = DefaultHasher::new();
    site.content_hash.hash(&mut h);
    (spec.mode, spec.surface).hash(&mut h);
    spec.breaks.iter().for_each(|b| b.to_bits().hash(&mut h));
    spec.colors.hash(&mut h);
    spec.opacity.to_bits().hash(&mut h);
    let key = h.finish();
    if let Some(r) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Some(r.clone());
    }
    let tin = site.surface(spec.surface.surface());
    let breaks = resolve_breaks(spec, tin);
    let mut r = analyze(tin, spec.mode, &breaks, &spec.colors);
    r.surface = spec.surface;
    r.opacity = spec.opacity;
    COMPUTED.with(|c| c.set(c.get() + 1));
    let r = Arc::new(r);
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if c.len() >= CACHE_SIZE {
        c.clear();
    }
    c.insert(key, r.clone());
    Some(r)
}

/// The triangles of band `i` as indices for a mesh.
pub fn band_indices(tin: &Tin, a: &AnalysisResult, i: usize) -> Vec<u32> {
    tin.triangles.iter().zip(&a.tri_band).filter(|(_, b)| **b as usize == i).flat_map(|(t, _)| *t).collect()
}

/// The css color of a band.
fn css(c: [u8; 4]) -> String {
    format!("rgb({},{},{})", c[0], c[1], c[2])
}

/// SVG of the bands in plan: one path per band.
pub fn svg(site_id: ObjectId, tin: &Tin, a: &AnalysisResult) -> String {
    let mut out = String::new();
    for band in a.bands.iter().filter(|b| b.triangles > 0) {
        let mut d = String::new();
        for (t, bi) in tin.triangles.iter().zip(&a.tri_band) {
            if *bi as usize == band.index {
                let p = t.map(|i| tin.vertices[i as usize]);
                let _ = write!(d, "M {} {} L {} {} L {} {} Z ", p[0].x, -p[0].y, p[1].x, -p[1].y, p[2].x, -p[2].y);
            }
        }
        let _ = writeln!(
            out,
            r#"<path d="{}" fill="{}" fill-opacity="{}" stroke="none" data-kind="analysis" data-band="{}" data-site="{site_id}"/>"#,
            d.trim_end(),
            css(band.color),
            a.opacity,
            band.index
        );
    }
    out
}

/// The settings of a legend object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LegendData {
    /// The site model. `None`: the first one on the layer.
    pub site: Option<ObjectId>,
    pub title: String,
    /// Show the area of each band.
    pub show_areas: bool,
}

pub fn legend_of(obj: &Object) -> Option<LegendData> {
    if !matches!(obj.shape, Shape::Rect { .. }) {
        return None;
    }
    read(obj, LEGEND_KEY)
}

/// SVG of a legend inside the rectangle `lo` to `hi`: a title row and a row per band.
pub fn legend_svg(id: ObjectId, lo: glam::DVec2, hi: glam::DVec2, d: &LegendData, a: &AnalysisResult) -> String {
    let mut out = String::new();
    let rows = a.bands.len() + 1;
    let row = (hi.y - lo.y) / rows as f64;
    let text = row * 0.55;
    let title = if d.title.is_empty() { if a.mode == Mode::Slope { "Slope".to_string() } else { "Elevation".to_string() } } else { d.title.clone() };
    let _ = writeln!(
        out,
        r#"<text x="{}" y="{}" font-size="{text}" font-family="sans-serif" font-weight="bold" fill="black" data-kind="legend-title" data-legend="{id}">{}</text>"#,
        lo.x,
        -(hi.y - row * 0.7),
        esc(&title)
    );
    // The highest band is at the top.
    for (k, band) in a.bands.iter().rev().enumerate() {
        let top = hi.y - row * (k + 1) as f64;
        let _ = writeln!(
            out,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}" stroke="black" stroke-width="{}" data-kind="legend-swatch" data-band="{}" data-legend="{id}"/>"#,
            lo.x,
            -(top + row * 0.85),
            row * 1.2,
            row * 0.7,
            css(band.color),
            row * 0.03,
            band.index
        );
        let mut s = band.label.clone();
        if d.show_areas {
            let _ = write!(s, "  ({:.1} m2)", band.area_xy / 1e6);
        }
        let _ = writeln!(
            out,
            r#"<text x="{}" y="{}" font-size="{text}" font-family="sans-serif" fill="black" data-kind="legend-label" data-band="{}" data-legend="{id}">{}</text>"#,
            lo.x + row * 1.5,
            -(top + row * 0.3),
            band.index,
            esc(&s)
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grading::testutil::*;
    use crate::model::tests::grid_site;
    use cadektor_core::command::Editor;
    use proptest::prelude::*;
    use serde_json::json;

    fn tin_of(f: impl Fn(f64, f64) -> f64) -> Tin {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(6, 1000.0, f));
        SiteResult::compute(ed.doc(), id).unwrap().existing
    }

    #[test]
    fn bands_for_a_known_slope() {
        // 10 percent along x is in the band [10, 15).
        let tin = tin_of(|x, _| x / 10.0);
        let r = analyze(&tin, Mode::Slope, &SLOPE_BREAKS, &[]);
        assert_eq!(r.bands.len(), 7);
        assert_eq!(r.bands[3].triangles, tin.triangles.len(), "{:?}", r.bands);
        assert!(r.tri_band.iter().all(|b| *b == 3));
        assert_eq!(r.bands[3].label, "10 to 15 %");
        assert_eq!(r.bands[0].label, "below 2 %");
        assert_eq!(r.bands[6].label, "50 % and above");
        assert!((r.bands[3].area_xy - tin.area_xy()).abs() < 1e-3);
        // The sloped area is larger by the length of the slope.
        assert!((r.bands[3].area_surface / r.bands[3].area_xy - (1.0f64 + 0.01).sqrt()).abs() < 1e-9);
    }

    #[test]
    fn elevation_bands_use_the_height_of_the_triangle() {
        let tin = tin_of(|x, _| x);
        let breaks = [1000.0, 3000.0];
        let r = analyze(&tin, Mode::Elevation, &breaks, &[[1, 1, 1, 255], [2, 2, 2, 255], [3, 3, 3, 255]]);
        assert_eq!(r.bands.iter().map(|b| b.triangles).sum::<usize>(), tin.triangles.len());
        assert!(r.bands.iter().all(|b| b.triangles > 0));
        assert_eq!(r.bands[1].color, [2, 2, 2, 255]);
        assert_eq!(r.bands[0].label, "below 1.00 m");
        assert_eq!(r.bands[1].label, "1.00 to 3.00 m");
        // Wrong number of colors falls back to the ramp.
        let r = analyze(&tin, Mode::Elevation, &breaks, &[[1, 1, 1, 255]]);
        assert_eq!(r.bands[0].color, ramp(3)[0]);
        assert_eq!(label(Mode::Off, None, None), "all");
    }

    #[test]
    fn ramp_and_band_of() {
        assert_eq!(ramp(0).len(), 0);
        assert_eq!(ramp(1).len(), 1);
        let r = ramp(7);
        assert_eq!(r[0], [46, 139, 87, 255]);
        assert_eq!(r[6], [190, 50, 40, 255]);
        assert_eq!(band_of(&[1.0, 2.0], 0.5), 0);
        assert_eq!(band_of(&[1.0, 2.0], 1.0), 1);
        assert_eq!(band_of(&[1.0, 2.0], 2.0), 2);
        assert_eq!(band_of(&[], 5.0), 0);
    }

    proptest! {
        #[test]
        fn every_triangle_is_in_exactly_one_band(
            gx in -1.0f64..1.0, gy in -1.0f64..1.0, wob in 0.0f64..300.0,
            mut breaks in proptest::collection::vec(-5.0f64..80.0, 0..8),
        ) {
            breaks.sort_by(|a, b| a.total_cmp(b));
            breaks.dedup();
            let tin = tin_of(|x, y| gx * x + gy * y + wob * ((x + 2.0 * y) / 700.0).sin());
            for mode in [Mode::Slope, Mode::Elevation] {
                let b: Vec<f64> = if mode == Mode::Slope { breaks.clone() } else { breaks.iter().map(|v| v * 20.0).collect() };
                let r = analyze(&tin, mode, &b, &[]);
                prop_assert_eq!(r.tri_band.len(), tin.triangles.len());
                prop_assert!(r.tri_band.iter().all(|i| (*i as usize) < b.len() + 1));
                prop_assert_eq!(r.bands.iter().map(|x| x.triangles).sum::<usize>(), tin.triangles.len());
                prop_assert!((r.bands.iter().map(|x| x.area_xy).sum::<f64>() - tin.area_xy()).abs() < 1e-3);
                // The bands do not overlap: limits go up.
                for w in r.bands.windows(2) {
                    prop_assert_eq!(w[0].to, w[1].from);
                }
                let idx: usize = (0..r.bands.len()).map(|i| band_indices(&tin, &r, i).len()).sum();
                prop_assert_eq!(idx, tin.triangles.len() * 3);
            }
        }
    }

    #[test]
    fn settings_are_checked() {
        assert!(AnalysisSpec::default().validate().is_ok());
        let ok = AnalysisSpec { mode: Mode::Slope, breaks: vec![1.0, 2.0], colors: vec![[0, 0, 0, 255]; 3], ..AnalysisSpec::default() };
        assert!(ok.validate().is_ok());
        let bad = |f: &dyn Fn(&mut AnalysisSpec)| {
            let mut s = ok.clone();
            f(&mut s);
            s.validate().is_err()
        };
        assert!(bad(&|s| s.breaks = vec![2.0, 1.0]));
        assert!(bad(&|s| s.breaks = vec![1.0, 1.0]));
        assert!(bad(&|s| s.breaks = vec![f64::NAN, 1.0]));
        assert!(bad(&|s| s.breaks = (0..50).map(f64::from).collect()));
        assert!(bad(&|s| s.colors = vec![[0; 4]]));
        assert!(bad(&|s| s.opacity = 2.0));
        assert!(bad(&|s| s.opacity = f64::NAN));
        let off = AnalysisSpec { breaks: vec![1.0], ..AnalysisSpec::default() };
        assert!(off.validate().is_ok());
    }

    #[test]
    fn defaults_and_cache() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(6, 1000.0, |x, y| x / 5.0 + y / 20.0));
        let site = SiteResult::compute(ed.doc(), id).unwrap();
        assert!(for_site(&site, &AnalysisSpec::default()).is_none());
        let spec = AnalysisSpec { mode: Mode::Elevation, surface: Follow::Existing, ..AnalysisSpec::default() };
        let before = computed();
        let a = for_site(&site, &spec).unwrap();
        assert_eq!(a.bands.len(), 7, "seven default hypsometric bands");
        let b = for_site(&site, &spec).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(computed(), before + 1);
        // Other settings are other entries.
        let c = for_site(&site, &AnalysisSpec { mode: Mode::Slope, ..spec.clone() }).unwrap();
        assert_eq!(c.bands.len(), 7);
        assert_eq!(c.breaks, SLOPE_BREAKS);
        assert_eq!(c.surface, Follow::Existing);
        assert_eq!(computed(), before + 2);
        // A flat site has one elevation band.
        let mut ed2 = Editor::default();
        let flat = add(&mut ed2, grid_site(4, 1000.0, |_, _| 7.0));
        let fs = SiteResult::compute(ed2.doc(), flat).unwrap();
        assert_eq!(for_site(&fs, &spec).unwrap().bands.len(), 1);
        assert!(resolve_breaks(&AnalysisSpec::default(), &fs.existing).is_empty());
        assert_eq!(resolve_breaks(&AnalysisSpec { breaks: vec![3.0], ..AnalysisSpec::default() }, &fs.existing), vec![3.0]);
        // The cache empties when it is full.
        for i in 0..(CACHE_SIZE + 3) {
            let s = AnalysisSpec { mode: Mode::Slope, breaks: vec![i as f64 + 0.5], ..spec.clone() };
            assert!(for_site(&site, &s).is_some());
        }
    }

    #[test]
    fn settings_come_from_the_site_object() {
        let mut ed = Editor::default();
        let id = add_with(&mut ed, grid_site(3, 1000.0, |x, _| x), KEY, json!({"mode": "slope", "breaks": [5.0], "surface": "existing"}));
        let spec = spec_of(ed.doc().object(id).unwrap()).unwrap();
        assert_eq!((spec.mode, spec.surface, spec.breaks.clone()), (Mode::Slope, Follow::Existing, vec![5.0]));
        let c = add(&mut ed, Shape::Circle { center: glam::DVec2::ZERO, radius: 1.0 });
        assert!(spec_of(ed.doc().object(c).unwrap()).is_none());
        assert!(legend_of(ed.doc().object(c).unwrap()).is_none());
    }

    #[test]
    fn drawing_and_legend() {
        let tin = tin_of(|x, _| x / 10.0);
        let r = analyze(&tin, Mode::Slope, &SLOPE_BREAKS, &[]);
        let s = svg(ObjectId(3), &tin, &r);
        assert_eq!(s.matches("data-kind=\"analysis\"").count(), 1, "only bands with triangles are drawn");
        assert!(s.contains("fill-opacity=\"0.8\"") && s.contains("data-band=\"3\""));
        let legend = legend_svg(ObjectId(9), glam::DVec2::ZERO, glam::DVec2::new(3000.0, 8000.0), &LegendData { show_areas: true, ..LegendData::default() }, &r);
        assert_eq!(legend.matches("legend-swatch").count(), 7);
        assert!(legend.contains(">Slope<") && legend.contains("m2)") && legend.contains("below 2 %"));
        let titled = legend_svg(ObjectId(9), glam::DVec2::ZERO, glam::DVec2::new(3000.0, 8000.0), &LegendData { title: "Slope <10%".into(), ..LegendData::default() }, &r);
        assert!(titled.contains("Slope &lt;10%") && !titled.contains("m2)"));
        let elev = analyze(&tin, Mode::Elevation, &[1.0], &[]);
        assert!(legend_svg(ObjectId(9), glam::DVec2::ZERO, glam::DVec2::ONE, &LegendData::default(), &elev).contains(">Elevation<"));
    }
}
