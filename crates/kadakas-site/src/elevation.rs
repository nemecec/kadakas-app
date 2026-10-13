//! Colors by elevation for the 3D terrain mesh (3D-05): the elevation range is cut into bands and
//! each band takes its color from a gradient ramp. The triangles are cut at the band planes, so
//! the band edges are straight lines at one height (not the jagged edge of whole triangles). The
//! mesh of a band has one color, so the CPU and GPU renderers and the glTF and OBJ exports (one
//! color per primitive) all show the same bands.
//!
//! The work is linear in the triangles plus the cuts. A triangle that crosses `k` planes gives at
//! most `k + 1` pieces; the whole output is capped at [`MAX_CUT_TRIANGLES`]. A triangle that would
//! go over the cap is not cut: it goes whole to the band of its mean height.

use std::collections::HashMap;

use glam::DVec3;

use kadakas_core::model::Color;
use kadakas_core::site::{MAX_ELEVATION_BANDS, SiteSettings};

/// The most triangles that the cutting may output for one mesh. Beyond it, triangles are not cut.
pub const MAX_CUT_TRIANGLES: usize = 2_000_000;

/// The color of the ramp at `t` (0 is the first color, 1 the last), blended in sRGB.
/// A `t` outside 0 to 1, or not a number, is cut to the ends. An empty ramp gives grey.
pub fn ramp_color(ramp: &[Color], t: f64) -> [u8; 4] {
    match ramp {
        [] => [128, 128, 128, 255],
        [one] => one.0,
        _ => {
            let t = if t.is_finite() {
                t.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let at = t * (ramp.len() - 1) as f64;
            let i = (at.floor() as usize).min(ramp.len() - 2);
            let f = at - i as f64;
            let (a, b) = (ramp[i].0, ramp[i + 1].0);
            let mix = |k: usize| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * f).round() as u8;
            [mix(0), mix(1), mix(2), mix(3)]
        }
    }
}

/// Lowest and highest elevation of the ramp: the range of the settings, or else the range of
/// the vertices. `None` if there is neither.
pub fn range_of(settings: &SiteSettings, vertices: &[DVec3]) -> Option<(f64, f64)> {
    if let Some([lo, hi]) = settings.elevation_range {
        return Some((lo, hi));
    }
    min_max(vertices.iter().map(|v| v.z))
}

/// Lowest and highest of some heights.
pub fn min_max(zs: impl IntoIterator<Item = f64>) -> Option<(f64, f64)> {
    let mut it = zs.into_iter();
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), z| (lo.min(z), hi.max(z))))
}

/// The planes that cut the bands, and the color of each band.
#[derive(Clone, Debug, PartialEq)]
pub struct BandPlan {
    /// Heights of the cut planes, ascending. `levels.len() + 1` bands.
    pub levels: Vec<f64>,
    /// One color per band, from the lowest.
    pub colors: Vec<[u8; 4]>,
}

impl BandPlan {
    /// The plan for a height range. By default the range is cut into `elevation_bands` equal
    /// bands. With `elevation_bands_at_contours` the planes are at the datum plus whole major
    /// contour intervals; if that would give more than 64 bands, equal bands are used. A band
    /// takes the ramp color at the middle of its part of the range.
    pub fn new(settings: &SiteSettings, lo: f64, hi: f64) -> BandPlan {
        let ramp = &settings.elevation_ramp;
        let mut levels = Vec::new();
        if hi > lo && hi.is_finite() && lo.is_finite() {
            if settings.elevation_bands_at_contours {
                let (d, m) = (settings.datum, settings.major_interval);
                if m > 0.0 && m.is_finite() && (hi - lo) / m < MAX_ELEVATION_BANDS as f64 - 1.0 {
                    let mut k = ((lo - d) / m).floor() + 1.0;
                    while d + k * m < hi {
                        levels.push(d + k * m);
                        k += 1.0;
                    }
                }
            }
            if levels.is_empty() {
                let n = settings.elevation_bands.clamp(1, MAX_ELEVATION_BANDS);
                levels = (1..n)
                    .map(|k| lo + (hi - lo) * k as f64 / n as f64)
                    .collect();
            }
        }
        let t_of = |z: f64| {
            if hi > lo { (z - lo) / (hi - lo) } else { 0.5 }
        };
        let colors = (0..=levels.len())
            .map(|i| {
                let a = if i == 0 { lo } else { levels[i - 1] };
                let b = levels.get(i).copied().unwrap_or(hi);
                ramp_color(ramp, t_of((a + b) / 2.0))
            })
            .collect();
        BandPlan { levels, colors }
    }

    /// The band (0 to `levels.len()`) of a height. A height on a plane belongs to the band
    /// above. Not a number goes to band 0.
    pub fn band_of(&self, z: f64) -> usize {
        if z.is_nan() {
            return 0;
        }
        self.levels.partition_point(|l| *l <= z)
    }
}

/// The triangles of one band: their own vertices, and indices into them.
#[derive(Clone, Debug, PartialEq)]
pub struct BandMesh {
    pub band: usize,
    pub vertices: Vec<DVec3>,
    pub indices: Vec<u32>,
}

/// Where a vertex of a band mesh comes from. Two triangles that share a vertex or a cut edge
/// get the same key, so they share the vertex in the band mesh (smooth shading, no cracks).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    Source(u32),
    /// A cut of the edge between two source vertices (ordered) at a plane.
    Cut(u32, u32, u32),
}

type Pt = (Key, DVec3);

#[derive(Default)]
struct Builder {
    map: HashMap<Key, u32>,
    mesh: Option<BandMesh>,
}

impl Builder {
    fn point(&mut self, band: usize, p: Pt) -> u32 {
        let mesh = self.mesh.get_or_insert_with(|| BandMesh {
            band,
            vertices: Vec::new(),
            indices: Vec::new(),
        });
        *self.map.entry(p.0).or_insert_with(|| {
            mesh.vertices.push(p.1);
            (mesh.vertices.len() - 1) as u32
        })
    }

    /// Adds a convex polygon as a fan of triangles.
    fn polygon(&mut self, band: usize, poly: &[Pt]) -> usize {
        if poly.len() < 3 {
            return 0;
        }
        let ids: Vec<u32> = poly.iter().map(|p| self.point(band, *p)).collect();
        let mesh = self.mesh.as_mut().expect("a point was added");
        for i in 1..ids.len() - 1 {
            mesh.indices.extend([ids[0], ids[i], ids[i + 1]]);
        }
        ids.len() - 2
    }
}

/// The source edge that a polygon edge lies on. An edge between two cut vertices of the same
/// plane never crosses another plane.
fn source_edge(a: Key, b: Key) -> (u32, u32) {
    match (a, b) {
        (Key::Source(x), Key::Source(y)) => (x.min(y), x.max(y)),
        (Key::Cut(lo, hi, _), _) | (_, Key::Cut(lo, hi, _)) => (lo, hi),
    }
}

/// Splits a convex polygon at the plane `z = level` into the part below and the part above.
/// `index` is the number of the plane, for the keys of the new vertices. The new vertex is
/// computed on the source edge from its end with the lower index, so the two triangles of an edge
/// get exactly the same point.
fn split(vertices: &[DVec3], poly: &[Pt], level: f64, index: u32) -> (Vec<Pt>, Vec<Pt>) {
    let (mut below, mut above) = (Vec::new(), Vec::new());
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sa, sb) = (a.1.z < level, b.1.z < level);
        if sa {
            below.push(a);
        } else {
            above.push(a);
        }
        if sa != sb {
            let (lo, hi) = source_edge(a.0, b.0);
            let (p, q) = (vertices[lo as usize], vertices[hi as usize]);
            let t = (level - p.z) / (q.z - p.z);
            let mut at = p + (q - p) * t;
            at.z = level;
            let cut = (Key::Cut(lo, hi, index), at);
            below.push(cut);
            above.push(cut);
        }
    }
    (below, above)
}

/// Cuts a triangle mesh into one mesh per band that has triangles, in band order. The triangles
/// are cut at the planes of the plan. Indices that point outside `vertices` are skipped.
pub fn split_bands(vertices: &[DVec3], indices: &[u32], plan: &BandPlan) -> Vec<BandMesh> {
    split_bands_capped(vertices, indices, plan, MAX_CUT_TRIANGLES)
}

/// Like [`split_bands`], with the cap on the output triangles given.
fn split_bands_capped(
    vertices: &[DVec3],
    indices: &[u32],
    plan: &BandPlan,
    cap: usize,
) -> Vec<BandMesh> {
    let n = plan.levels.len() + 1;
    let mut out: Vec<Builder> = (0..n).map(|_| Builder::default()).collect();
    let mut produced = 0usize;
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            continue;
        }
        let corners: Vec<Pt> = tri
            .iter()
            .map(|&i| (Key::Source(i), vertices[i as usize]))
            .collect();
        let (zmin, zmax) = corners
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.1.z), hi.max(p.1.z))
            });
        let first = plan.band_of(zmin);
        let last = plan.band_of(zmax);
        let crossed = last.saturating_sub(first);
        if crossed == 0 || produced + 3 * (crossed + 1) > cap {
            // Inside one band, or too many pieces: the whole triangle, in the band of its mean height.
            let band = if crossed == 0 {
                first
            } else {
                plan.band_of((zmin + zmax) / 2.0)
            };
            produced += out[band].polygon(band, &corners);
            continue;
        }
        let mut rest = corners;
        for (band, builder) in out.iter_mut().enumerate().take(last).skip(first) {
            let (below, above) = split(vertices, &rest, plan.levels[band], band as u32);
            produced += builder.polygon(band, &below);
            rest = above;
        }
        produced += out[last].polygon(last, &rest);
    }
    out.into_iter()
        .enumerate()
        .filter_map(|(band, b)| {
            b.mesh.map(|mut m| {
                m.band = band;
                m
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ramp() -> Vec<Color> {
        vec![Color([0, 0, 0, 255]), Color([200, 100, 50, 255])]
    }

    fn settings(bands: u32) -> SiteSettings {
        SiteSettings {
            elevation_bands: bands,
            elevation_ramp: ramp(),
            ..Default::default()
        }
    }

    #[test]
    fn ramp_blends_between_colors_and_clamps() {
        let r = ramp();
        assert_eq!(ramp_color(&r, 0.0), [0, 0, 0, 255]);
        assert_eq!(ramp_color(&r, 1.0), [200, 100, 50, 255]);
        assert_eq!(ramp_color(&r, 0.5), [100, 50, 25, 255]);
        assert_eq!(ramp_color(&r, -3.0), [0, 0, 0, 255]);
        assert_eq!(ramp_color(&r, 7.0), [200, 100, 50, 255]);
        assert_eq!(ramp_color(&r, f64::NAN), [0, 0, 0, 255]);
        assert_eq!(ramp_color(&[], 0.3), [128, 128, 128, 255]);
        assert_eq!(ramp_color(&r[..1], 0.9), [0, 0, 0, 255]);
        let three = vec![r[0], r[1], Color([255, 255, 255, 255])];
        assert_eq!(ramp_color(&three, 0.5), [200, 100, 50, 255]);
        assert_eq!(ramp_color(&three, 1.0), [255, 255, 255, 255]);
    }

    #[test]
    fn equal_bands_use_the_middle_of_each_band() {
        let plan = BandPlan::new(&settings(2), 0.0, 10.0);
        assert_eq!(plan.levels, vec![5.0]);
        assert_eq!(
            plan.colors,
            vec![ramp_color(&ramp(), 0.25), ramp_color(&ramp(), 0.75)]
        );
        // A flat range is one band with the middle color.
        let flat = BandPlan::new(&settings(5), 3.0, 3.0);
        assert!(flat.levels.is_empty());
        assert_eq!(flat.colors, vec![ramp_color(&ramp(), 0.5)]);
    }

    #[test]
    fn bands_can_follow_the_contour_levels() {
        let mut s = settings(4);
        s.elevation_bands_at_contours = true;
        s.datum = 100.0;
        s.major_interval = 500.0;
        // 120 .. 1480: planes at 600, 1100 (datum + k * 500).
        let plan = BandPlan::new(&s, 120.0, 1480.0);
        assert_eq!(plan.levels, vec![600.0, 1100.0]);
        assert_eq!(plan.colors.len(), 3);
        // A plane that lies on the lowest height is not a cut.
        let on_low = BandPlan::new(&s, 100.0, 700.0);
        assert_eq!(on_low.levels, vec![600.0]);
        // Too many planes: equal bands instead.
        s.major_interval = 1.0;
        let many = BandPlan::new(&s, 0.0, 1000.0);
        assert_eq!(many.levels.len(), 3);
        // A bad interval falls back too.
        s.major_interval = f64::NAN;
        assert_eq!(BandPlan::new(&s, 0.0, 1000.0).levels.len(), 3);
        // A range inside one interval has one band.
        s.major_interval = 5000.0;
        assert_eq!(BandPlan::new(&s, 10.0, 20.0).levels.len(), 3);
    }

    #[test]
    fn heights_fall_into_bands() {
        let plan = BandPlan::new(&settings(5), 0.0, 10.0);
        assert_eq!(plan.band_of(0.0), 0);
        assert_eq!(plan.band_of(10.0), 4);
        assert_eq!(plan.band_of(4.9), 2);
        assert_eq!(plan.band_of(4.0), 2);
        assert_eq!(plan.band_of(-5.0), 0);
        assert_eq!(plan.band_of(99.0), 4);
        assert_eq!(plan.band_of(f64::NAN), 0);
    }

    #[test]
    fn range_comes_from_settings_or_vertices() {
        let mut s = SiteSettings::default();
        let v = [DVec3::new(0.0, 0.0, 3.0), DVec3::new(1.0, 0.0, -2.0)];
        assert_eq!(range_of(&s, &v), Some((-2.0, 3.0)));
        assert_eq!(range_of(&s, &[]), None);
        s.elevation_range = Some([10.0, 20.0]);
        assert_eq!(range_of(&s, &v), Some((10.0, 20.0)));
        assert_eq!(min_max([]), None);
    }

    fn area(m: &BandMesh) -> f64 {
        m.indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.vertices[i as usize]);
                (b - a).cross(c - a).length() / 2.0
            })
            .sum()
    }

    #[test]
    fn triangles_are_cut_at_the_planes() {
        // One tall triangle from z 0 to 10 crosses the planes at 2.5, 5 and 7.5.
        let v = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(0.0, 4.0, 10.0),
        ];
        let plan = BandPlan::new(&settings(4), 0.0, 10.0);
        let parts = split_bands(&v, &[0, 1, 2], &plan);
        assert_eq!(parts.len(), 4);
        // Every piece stays inside its band, and the pieces add up to the triangle.
        let whole = (v[1] - v[0]).cross(v[2] - v[0]).length() / 2.0;
        let sum: f64 = parts.iter().map(area).sum();
        assert!((sum - whole).abs() < 1e-9, "{sum} {whole}");
        for p in &parts {
            let (lo, hi) = (
                if p.band == 0 {
                    0.0
                } else {
                    plan.levels[p.band - 1]
                },
                plan.levels.get(p.band).copied().unwrap_or(10.0),
            );
            assert!(
                p.vertices
                    .iter()
                    .all(|q| q.z >= lo - 1e-9 && q.z <= hi + 1e-9)
            );
        }
        // Without a crossing the triangle stays whole; a bad index is skipped.
        let flat = split_bands(
            &v,
            &[0, 1, 2, 0, 1, 99],
            &BandPlan::new(&settings(1), 0.0, 10.0),
        );
        assert_eq!((flat.len(), flat[0].indices.len()), (1, 3));
        assert!(split_bands(&v, &[], &plan).is_empty());
    }

    #[test]
    fn neighbors_share_the_cut_vertices_and_the_cuts_have_no_cracks() {
        // Two triangles share the edge 1-2; its cut at 5 is one vertex in the band mesh.
        let v = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 10.0),
            DVec3::new(2.0, 2.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
        ];
        let plan = BandPlan::new(&settings(2), 0.0, 10.0);
        let parts = split_bands(&v, &[0, 1, 2, 1, 3, 2], &plan);
        let low = &parts[0];
        let at5: Vec<&DVec3> = low.vertices.iter().filter(|q| q.z == 5.0).collect();
        // Cut points: edges 0-1, 1-2, 1-3 at height 5; the shared edge 1-2 is counted once.
        assert_eq!(at5.len(), 3, "{at5:?}");
    }

    #[test]
    fn the_cap_stops_the_cutting_but_keeps_every_triangle() {
        let v = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(0.0, 1.0, 1000.0),
        ];
        let plan = BandPlan::new(&settings(64), 0.0, 1000.0);
        let idx = [0, 1, 2, 0, 1, 2];
        // With room, a triangle that crosses 63 planes gives 64 pieces (at least).
        let cut = split_bands_capped(&v, &idx[..3], &plan, MAX_CUT_TRIANGLES);
        assert_eq!(cut.len(), 64);
        // With a small cap the triangles are not cut: each goes whole to one band.
        let whole = split_bands_capped(&v, &idx, &plan, 5);
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].indices.len(), 6);
        assert_eq!(whole[0].band, plan.band_of(500.0));
    }

    proptest! {
        #[test]
        fn cutting_keeps_the_area_and_the_bands(
            zs in proptest::collection::vec(-1.0e4f64..1.0e4, 3..30),
            bands in 1u32..64,
        ) {
            let v: Vec<DVec3> = zs.iter().enumerate()
                .map(|(i, z)| DVec3::new(i as f64, (i * 7 % 5) as f64, *z)).collect();
            let idx: Vec<u32> = (0..(v.len() / 3 * 3) as u32).collect();
            let (lo, hi) = range_of(&SiteSettings::default(), &v).unwrap();
            let plan = BandPlan::new(&settings(bands), lo, hi);
            let parts = split_bands(&v, &idx, &plan);
            let before: f64 = idx.chunks(3).map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| v[i as usize]);
                (b - a).cross(c - a).length() / 2.0
            }).sum();
            let after: f64 = parts.iter().map(area).sum();
            prop_assert!((before - after).abs() <= 1e-6 * (1.0 + before));
            let mut seen = std::collections::BTreeSet::new();
            for p in &parts {
                prop_assert!(p.band < plan.colors.len());
                prop_assert!(seen.insert(p.band));
                prop_assert!(p.indices.iter().all(|i| (*i as usize) < p.vertices.len()));
                // Every piece is inside the band, up to rounding.
                let lo_b = if p.band == 0 { f64::MIN } else { plan.levels[p.band - 1] };
                let hi_b = plan.levels.get(p.band).copied().unwrap_or(f64::MAX);
                prop_assert!(p.vertices.iter().all(|q| q.z >= lo_b - 1e-9 && q.z <= hi_b + 1e-9));
            }
            // Deterministic.
            prop_assert_eq!(parts, split_bands(&v, &idx, &plan));
        }

        #[test]
        fn ramp_colors_are_always_defined(t in proptest::num::f64::ANY, n in 0usize..6) {
            let ramp: Vec<Color> = (0..n).map(|i| Color([i as u8 * 40, 10, 20, 255])).collect();
            let c = ramp_color(&ramp, t);
            prop_assert_eq!(c, ramp_color(&ramp, t));
        }
    }
}
