//! Slope and elevation analysis of a site model: every triangle falls into a band, and the bands
//! have colors. The settings are on the site model object (`Object::data["site_analysis"]`).
//! A legend object (a rectangle with `Object::data["analysis_legend"]`) shows the bands.
//!
//! The classes are computed per triangle and kept by the content of the site model, so a change
//! of anything else in the drawing does not compute them again.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use kadakas_core::model::{Object, ObjectId, Shape};

use super::draw::{Drawn, Label, Poly};
use super::{Follow, meters, read};
use crate::model::{SiteCache, SiteResult};
use crate::tin::Tin;

/// The key on a site model object that holds the analysis settings.
pub const KEY: &str = "site_analysis";
/// The key on a legend object.
pub const LEGEND_KEY: &str = "analysis_legend";
/// Most class limits.
pub const MAX_BREAKS: usize = 40;
/// Default slope class limits, in percent.
pub const SLOPE_BREAKS: [f64; 6] = [2.0, 5.0, 10.0, 15.0, 25.0, 50.0];
/// Most analyses that a session keeps.
pub(crate) const CACHE_SIZE: usize = 64;

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
        AnalysisSpec {
            mode: Mode::Off,
            surface: Follow::Proposed,
            breaks: Vec::new(),
            colors: Vec::new(),
            opacity: 0.8,
        }
    }
}

impl AnalysisSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.breaks.len() > MAX_BREAKS {
            return Err(format!("at most {MAX_BREAKS} limits"));
        }
        if self.breaks.iter().any(|b| !b.is_finite())
            || self.breaks.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("breaks must be finite numbers in ascending order".into());
        }
        if !self.colors.is_empty() && self.colors.len() != self.breaks.len() + 1 {
            return Err(format!(
                "give {} colors for {} breaks (one more than breaks), or none",
                self.breaks.len() + 1,
                self.breaks.len()
            ));
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
    const STOPS: [[f64; 3]; 5] = [
        [46.0, 139.0, 87.0],
        [170.0, 205.0, 80.0],
        [250.0, 225.0, 90.0],
        [240.0, 140.0, 50.0],
        [190.0, 50.0, 40.0],
    ];
    (0..n)
        .map(|i| {
            let t = if n > 1 {
                i as f64 / (n - 1) as f64
            } else {
                0.0
            } * (STOPS.len() - 1) as f64;
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
    /// The outline of each band: the triangles of a band merged into rings, to be filled with the
    /// even-odd rule (a ring inside a ring is a hole). Corners on a straight line are left out.
    pub rings: Vec<Vec<Vec<DVec2>>>,
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
            if hi - lo < 1e-9 {
                Vec::new()
            } else {
                (1..7).map(|i| lo + (hi - lo) * i as f64 / 7.0).collect()
            }
        }
    }
}

/// Classes every triangle of a surface. `colors` is one per band (or empty for the ramp).
pub fn analyze(tin: &Tin, mode: Mode, breaks: &[f64], colors: &[[u8; 4]]) -> AnalysisResult {
    let n = breaks.len() + 1;
    let colors: Vec<[u8; 4]> = if colors.len() == n {
        colors.to_vec()
    } else {
        ramp(n)
    };
    let mut bands: Vec<Band> = (0..n)
        .map(|i| {
            let (from, to) = (i.checked_sub(1).map(|j| breaks[j]), breaks.get(i).copied());
            Band {
                index: i,
                from,
                to,
                color: colors[i],
                label: label(mode, from, to),
                triangles: 0,
                area_xy: 0.0,
                area_surface: 0.0,
            }
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
    let rings = band_rings(tin, &tri_band, n);
    AnalysisResult {
        mode,
        surface: Follow::Proposed,
        breaks: breaks.to_vec(),
        opacity: 0.8,
        tri_band,
        bands,
        rings,
    }
}

/// Merges the triangles of each band into rings. An edge that two triangles of the band share
/// is inside the band and goes away. The edges that stay are the outline, and they chain into rings.
fn band_rings(tin: &Tin, tri_band: &[u16], n: usize) -> Vec<Vec<Vec<DVec2>>> {
    let mut edges: Vec<HashMap<(u32, u32), ()>> = vec![HashMap::new(); n];
    for (t, band) in tin.triangles.iter().zip(tri_band) {
        let set = &mut edges[*band as usize];
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if set.remove(&(b, a)).is_none() {
                set.insert((a, b), ());
            }
        }
    }
    edges
        .into_iter()
        .map(|set| {
            let mut list: Vec<(u32, u32)> = set.into_keys().collect();
            list.sort_unstable();
            let mut next: HashMap<u32, Vec<u32>> = HashMap::new();
            for (a, b) in list {
                next.entry(a).or_default().push(b);
            }
            let mut starts: Vec<u32> = next.keys().copied().collect();
            starts.sort_unstable();
            let mut rings = Vec::new();
            for start in starts {
                // A corner where two rings touch has two ways out, so it can start more than one ring.
                while next.contains_key(&start) {
                    let mut ring = vec![start];
                    let mut at = start;
                    while let Some(outs) = next.get_mut(&at) {
                        let to = outs.pop().expect("a corner in the map has a way out");
                        if outs.is_empty() {
                            next.remove(&at);
                        }
                        if to == start {
                            break;
                        }
                        ring.push(to);
                        at = to;
                    }
                    let pts: Vec<DVec2> = ring
                        .iter()
                        .map(|i| tin.vertices[*i as usize].truncate())
                        .collect();
                    let pts = straighten(pts);
                    if pts.len() >= 3 {
                        rings.push(pts);
                    }
                }
            }
            rings
        })
        .collect()
}

/// Removes the corners of a ring that lie on the line between their neighbors.
fn straighten(pts: Vec<DVec2>) -> Vec<DVec2> {
    let n = pts.len();
    if n < 4 {
        return pts;
    }
    (0..n)
        .filter(|&i| {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (u, v) = (b - a, c - b);
            u.perp_dot(v).abs() > 1e-9 * (u.length() * v.length()).max(1e-12) || u.dot(v) < 0.0
        })
        .map(|i| pts[i])
        .collect()
}

impl SiteCache {
    /// How many analyses this cache computed (did not find in the cache) so far.
    pub fn analyses_computed(&self) -> u64 {
        self.analysis_runs
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// The analysis of a site model for the settings, from the cache when the content of the site
/// model and the settings are the same. `None` when the analysis is off. The cache belongs to
/// one session and holds at most `CACHE_SIZE` analyses.
pub fn for_site(
    cache: &SiteCache,
    site: &SiteResult,
    spec: &AnalysisSpec,
) -> Option<Arc<AnalysisResult>> {
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
    if let Some(r) = cache
        .analyses
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return Some(r);
    }
    let tin = site.surface(spec.surface);
    let breaks = resolve_breaks(spec, tin);
    let mut r = analyze(tin, spec.mode, &breaks, &spec.colors);
    r.surface = spec.surface;
    r.opacity = spec.opacity;
    cache
        .analysis_runs
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let r = Arc::new(r);
    cache
        .analyses
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, r.clone());
    Some(r)
}

/// The triangles of band `i` as indices for a mesh.
pub fn band_indices(tin: &Tin, a: &AnalysisResult, i: usize) -> Vec<u32> {
    tin.triangles
        .iter()
        .zip(&a.tri_band)
        .filter(|(_, b)| **b as usize == i)
        .flat_map(|(t, _)| *t)
        .collect()
}

/// The bands in plan: one filled shape per band, made of its rings.
pub fn draw(site_id: ObjectId, a: &AnalysisResult) -> Drawn {
    let mut out = Drawn::default();
    let alpha = (a.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    for band in a.bands.iter().filter(|b| b.triangles > 0) {
        let c = band.color;
        let mut poly = Poly::new("analysis", site_id, a.rings[band.index].clone(), true)
            .filled([c[0], c[1], c[2], alpha]);
        poly.band = Some(band.index);
        out.polys.push(poly);
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

/// A legend inside the rectangle `lo` to `hi`: a title row and a row per band.
pub fn legend_draw(
    id: ObjectId,
    lo: DVec2,
    hi: DVec2,
    d: &LegendData,
    a: &AnalysisResult,
) -> Drawn {
    let mut out = Drawn::default();
    let rows = a.bands.len() + 1;
    let row = (hi.y - lo.y) / rows as f64;
    let text = row * 0.55;
    let title = if d.title.is_empty() {
        if a.mode == Mode::Slope {
            "Slope".to_string()
        } else {
            "Elevation".to_string()
        }
    } else {
        d.title.clone()
    };
    let mut t = Label::new(
        "legend-title",
        id,
        DVec2::new(lo.x, hi.y - row * 0.7),
        title,
        text,
    );
    t.bold = true;
    out.labels.push(t);
    // The highest band is at the top.
    for (k, band) in a.bands.iter().rev().enumerate() {
        let top = hi.y - row * (k + 1) as f64;
        let (x0, y0, x1, y1) = (lo.x, top + row * 0.15, lo.x + row * 1.2, top + row * 0.85);
        let ring = vec![
            DVec2::new(x0, y0),
            DVec2::new(x1, y0),
            DVec2::new(x1, y1),
            DVec2::new(x0, y1),
        ];
        let mut swatch = Poly::new("legend-swatch", id, vec![ring], true)
            .filled(band.color)
            .stroked([0, 0, 0, 255], row * 0.03);
        swatch.band = Some(band.index);
        out.polys.push(swatch);
        let mut s = band.label.clone();
        if d.show_areas {
            s.push_str(&format!("  ({:.1} m2)", band.area_xy / 1e6));
        }
        out.labels.push(Label::new(
            "legend-label",
            id,
            DVec2::new(lo.x + row * 1.5, top + row * 0.3),
            s,
            text,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grading::testutil::*;
    use crate::model::tests::grid_site;
    use kadakas_core::command::Editor;
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
        assert!(
            (r.bands[3].area_surface / r.bands[3].area_xy - (1.0f64 + 0.01).sqrt()).abs() < 1e-9
        );
    }

    #[test]
    fn elevation_bands_use_the_height_of_the_triangle() {
        let tin = tin_of(|x, _| x);
        let breaks = [1000.0, 3000.0];
        let r = analyze(
            &tin,
            Mode::Elevation,
            &breaks,
            &[[1, 1, 1, 255], [2, 2, 2, 255], [3, 3, 3, 255]],
        );
        assert_eq!(
            r.bands.iter().map(|b| b.triangles).sum::<usize>(),
            tin.triangles.len()
        );
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
        let ok = AnalysisSpec {
            mode: Mode::Slope,
            breaks: vec![1.0, 2.0],
            colors: vec![[0, 0, 0, 255]; 3],
            ..AnalysisSpec::default()
        };
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
        let off = AnalysisSpec {
            breaks: vec![1.0],
            ..AnalysisSpec::default()
        };
        assert!(off.validate().is_ok());
    }

    #[test]
    fn defaults_and_cache() {
        let cache = SiteCache::default();
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(6, 1000.0, |x, y| x / 5.0 + y / 20.0));
        let site = SiteResult::compute(ed.doc(), id).unwrap();
        assert!(for_site(&cache, &site, &AnalysisSpec::default()).is_none());
        let spec = AnalysisSpec {
            mode: Mode::Elevation,
            surface: Follow::Existing,
            ..AnalysisSpec::default()
        };
        let before = cache.analyses_computed();
        let a = for_site(&cache, &site, &spec).unwrap();
        assert_eq!(a.bands.len(), 7, "seven default hypsometric bands");
        let b = for_site(&cache, &site, &spec).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.analyses_computed(), before + 1);
        // Other settings are other entries.
        let c = for_site(
            &cache,
            &site,
            &AnalysisSpec {
                mode: Mode::Slope,
                ..spec.clone()
            },
        )
        .unwrap();
        assert_eq!(c.bands.len(), 7);
        assert_eq!(c.breaks, SLOPE_BREAKS);
        assert_eq!(c.surface, Follow::Existing);
        assert_eq!(cache.analyses_computed(), before + 2);
        // A flat site has one elevation band.
        let mut ed2 = Editor::default();
        let flat = add(&mut ed2, grid_site(4, 1000.0, |_, _| 7.0));
        let fs = SiteResult::compute(ed2.doc(), flat).unwrap();
        assert_eq!(for_site(&cache, &fs, &spec).unwrap().bands.len(), 1);
        assert!(resolve_breaks(&AnalysisSpec::default(), &fs.existing).is_empty());
        assert_eq!(
            resolve_breaks(
                &AnalysisSpec {
                    breaks: vec![3.0],
                    ..AnalysisSpec::default()
                },
                &fs.existing
            ),
            vec![3.0]
        );
        // The cache stays bounded.
        for i in 0..(CACHE_SIZE + 3) {
            let s = AnalysisSpec {
                mode: Mode::Slope,
                breaks: vec![i as f64 + 0.5],
                ..spec.clone()
            };
            assert!(for_site(&cache, &site, &s).is_some());
        }
        assert_eq!(cache.analyses.lock().unwrap().len(), CACHE_SIZE);
    }

    #[test]
    fn settings_come_from_the_site_object() {
        let mut ed = Editor::default();
        let id = add_with(
            &mut ed,
            grid_site(3, 1000.0, |x, _| x),
            KEY,
            json!({"mode": "slope", "breaks": [5.0], "surface": "existing"}),
        );
        let spec = spec_of(ed.doc().object(id).unwrap()).unwrap();
        assert_eq!(
            (spec.mode, spec.surface, spec.breaks.clone()),
            (Mode::Slope, Follow::Existing, vec![5.0])
        );
        let c = add(
            &mut ed,
            Shape::Circle {
                center: glam::DVec2::ZERO,
                radius: 1.0,
            },
        );
        assert!(spec_of(ed.doc().object(c).unwrap()).is_none());
        assert!(legend_of(ed.doc().object(c).unwrap()).is_none());
    }

    #[test]
    fn drawing_and_legend() {
        let tin = tin_of(|x, _| x / 10.0);
        let r = analyze(&tin, Mode::Slope, &SLOPE_BREAKS, &[]);
        let d = draw(ObjectId::from_num(3), &r);
        assert_eq!(d.polys.len(), 1, "only bands with triangles are drawn");
        // A plain slope is one square: the triangles merge into a ring with four corners.
        assert_eq!(d.polys[0].rings, vec![r.rings[3][0].clone()]);
        assert_eq!(d.polys[0].rings[0].len(), 4);
        let s = d.svg();
        assert!(
            s.contains("fill-opacity=\"0.8\"")
                && s.contains("data-band=\"3\"")
                && s.contains("data-kind=\"analysis\""),
            "{s}"
        );
        let legend = legend_draw(
            ObjectId::from_num(9),
            DVec2::ZERO,
            DVec2::new(3000.0, 8000.0),
            &LegendData {
                show_areas: true,
                ..LegendData::default()
            },
            &r,
        );
        assert_eq!(legend.polys.len(), 7);
        let texts: Vec<&str> = legend.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts[0], "Slope");
        assert!(
            texts.iter().any(|t| t.contains("m2)")) && texts.contains(&"50 % and above  (0.0 m2)")
        );
        assert_eq!(
            texts[1], "50 % and above  (0.0 m2)",
            "the highest band is at the top"
        );
        let titled = legend_draw(
            ObjectId::from_num(9),
            DVec2::ZERO,
            DVec2::new(3000.0, 8000.0),
            &LegendData {
                title: "Slope <10%".into(),
                ..LegendData::default()
            },
            &r,
        );
        assert!(titled.svg().contains("Slope &lt;10%") && !titled.svg().contains("m2)"));
        let elev = analyze(&tin, Mode::Elevation, &[1.0], &[]);
        let e = legend_draw(
            ObjectId::from_num(9),
            DVec2::ZERO,
            DVec2::ONE,
            &LegendData::default(),
            &elev,
        );
        assert_eq!(e.labels[0].text, "Elevation");
    }

    /// The area of rings with the even-odd rule: outer rings add and the rings inside them subtract.
    fn evenodd_area(rings: &[Vec<DVec2>]) -> f64 {
        let area = |r: &[DVec2]| kadakas_core::geom::polygon_area(r).abs();
        // A ring is a hole when an odd number of the other rings contain a point of it.
        rings
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let depth = rings
                    .iter()
                    .enumerate()
                    .filter(|(j, o)| *j != i && kadakas_core::geom::point_in_polygon(r[0], o))
                    .count();
                if depth % 2 == 0 { area(r) } else { -area(r) }
            })
            .sum()
    }

    #[test]
    fn merged_rings_cover_exactly_the_triangles_of_the_band() {
        // A bumpy site gives bands with holes and several parts.
        let tin = tin_of(|x, y| 400.0 * ((x / 800.0).sin() + (y / 900.0).cos()));
        let r = analyze(&tin, Mode::Slope, &[10.0, 30.0], &[]);
        for band in &r.bands {
            let merged = evenodd_area(&r.rings[band.index]);
            assert!(
                (merged - band.area_xy).abs() < 1e-3 * (1.0 + band.area_xy),
                "band {}: {merged} vs {}",
                band.index,
                band.area_xy
            );
        }
        assert!(r.bands.iter().filter(|b| b.triangles > 0).count() >= 2);
    }

    #[test]
    fn a_large_site_draws_small_and_is_computed_once() {
        // 50 000 points: the bands merge into a few rings, not one path per triangle.
        let mut ed = Editor::default();
        let id = add(
            &mut ed,
            grid_site(224, 100.0, |x, y| {
                300.0 * ((x / 900.0).sin() + (y / 1100.0).cos())
            }),
        );
        let cache = SiteCache::default();
        let site = SiteResult::compute(ed.doc(), id).unwrap();
        let spec = AnalysisSpec {
            mode: Mode::Slope,
            surface: Follow::Existing,
            ..AnalysisSpec::default()
        };
        let a = for_site(&cache, &site, &spec).unwrap();
        let svg = draw(id, &a).svg();
        assert_eq!(cache.analyses_computed(), 1);
        assert!(a.tri_band.len() > 90_000);
        // About a vertex per boundary corner, far fewer than three per triangle.
        let corners: usize = a.rings.iter().flatten().map(Vec::len).sum();
        assert!(corners < a.tri_band.len() / 2, "{corners} corners");
        assert!(svg.len() < 4_000_000, "{} bytes", svg.len());
        // The second call finds the result in the cache.
        let before = cache.analyses_computed();
        let again = for_site(&cache, &site, &spec).unwrap();
        assert!(Arc::ptr_eq(&a, &again) && cache.analyses_computed() == before);
    }

    /// Timing: run in release mode, `cargo test -p kadakas-site --release --lib -- --ignored perf_`
    #[test]
    #[ignore = "timing test: run in release mode"]
    fn perf_a_large_site_draws_in_under_a_second() {
        let mut ed = Editor::default();
        let id = add(
            &mut ed,
            grid_site(224, 100.0, |x, y| {
                300.0 * ((x / 900.0).sin() + (y / 1100.0).cos())
            }),
        );
        let cache = SiteCache::default();
        let site = SiteResult::compute(ed.doc(), id).unwrap();
        let spec = AnalysisSpec {
            mode: Mode::Slope,
            surface: Follow::Existing,
            ..AnalysisSpec::default()
        };
        let start = std::time::Instant::now();
        let a = for_site(&cache, &site, &spec).unwrap();
        let svg = draw(id, &a).svg();
        let took = start.elapsed();
        assert!(!svg.is_empty());
        assert!(took.as_secs_f64() < 1.0, "analysis and svg took {took:?}");
    }
}
