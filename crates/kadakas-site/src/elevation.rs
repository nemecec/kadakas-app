//! Colors by elevation for the 3D terrain mesh (3D-05): the elevation range is cut into bands and
//! each band takes its color from a gradient ramp. A triangle belongs to the band of its mean
//! height. The mesh of a band has one color, so the CPU and GPU renderers and the glTF and OBJ
//! exports (one color per primitive) all show the same bands. The work is linear in the triangles,
//! and the number of bands is at most [`kadakas_core::site::MAX_ELEVATION_BANDS`].

use std::collections::HashMap;

use glam::DVec3;

use kadakas_core::model::Color;
use kadakas_core::site::SiteSettings;

/// The color of the ramp at `t` (0 is the first color, 1 the last), blended in sRGB.
/// A `t` outside 0 to 1, or not a number, is cut to the ends. An empty ramp gives grey.
pub fn ramp_color(ramp: &[Color], t: f64) -> [u8; 4] {
    match ramp {
        [] => [128, 128, 128, 255],
        [one] => one.0,
        _ => {
            let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
            let at = t * (ramp.len() - 1) as f64;
            let i = (at.floor() as usize).min(ramp.len() - 2);
            let f = at - i as f64;
            let (a, b) = (ramp[i].0, ramp[i + 1].0);
            let mix = |k: usize| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * f).round() as u8;
            [mix(0), mix(1), mix(2), mix(3)]
        }
    }
}

/// The color of each band: the ramp at the middle of the band.
pub fn band_colors(ramp: &[Color], bands: u32) -> Vec<[u8; 4]> {
    let n = bands.max(1);
    (0..n)
        .map(|i| ramp_color(ramp, (i as f64 + 0.5) / n as f64))
        .collect()
}

/// Lowest and highest elevation of the ramp: the range of the settings, or else the range of
/// the vertices. `None` if there is neither.
pub fn range_of(settings: &SiteSettings, vertices: &[DVec3]) -> Option<(f64, f64)> {
    if let Some([lo, hi]) = settings.elevation_range {
        return Some((lo, hi));
    }
    let mut it = vertices.iter().map(|v| v.z);
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), z| (lo.min(z), hi.max(z))))
}

/// The band (0 to `bands - 1`) of a height. Heights outside the range go to the first or the
/// last band. A flat range gives band 0.
pub fn band_of(z: f64, lo: f64, hi: f64, bands: u32) -> usize {
    let n = bands.max(1) as usize;
    if !(hi > lo) || !z.is_finite() {
        return 0;
    }
    let t = ((z - lo) / (hi - lo)).clamp(0.0, 1.0);
    ((t * n as f64) as usize).min(n - 1)
}

/// The triangles of one band: their own vertices, and indices into them.
#[derive(Clone, Debug, PartialEq)]
pub struct BandMesh {
    pub band: usize,
    pub vertices: Vec<DVec3>,
    pub indices: Vec<u32>,
}

/// Cuts a triangle mesh into one mesh per band that has triangles, in band order. Indices that
/// point outside `vertices` are skipped. A vertex that triangles of two bands share is copied.
pub fn split_bands(
    vertices: &[DVec3],
    indices: &[u32],
    lo: f64,
    hi: f64,
    bands: u32,
) -> Vec<BandMesh> {
    let n = bands.max(1) as usize;
    let mut out: Vec<Option<BandMesh>> = vec![None; n];
    // The index of each vertex in the mesh of each band.
    let mut remap: Vec<HashMap<u32, u32>> = vec![HashMap::new(); n];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            continue;
        }
        let z = (vertices[a].z + vertices[b].z + vertices[c].z) / 3.0;
        let band = band_of(z, lo, hi, bands);
        let mesh = out[band].get_or_insert_with(|| BandMesh {
            band,
            vertices: Vec::new(),
            indices: Vec::new(),
        });
        for &i in tri {
            let new = *remap[band].entry(i).or_insert_with(|| {
                mesh.vertices.push(vertices[i as usize]);
                (mesh.vertices.len() - 1) as u32
            });
            mesh.indices.push(new);
        }
    }
    out.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ramp() -> Vec<Color> {
        vec![Color([0, 0, 0, 255]), Color([200, 100, 50, 255])]
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
    fn band_colors_use_the_middle_of_each_band() {
        let c = band_colors(&ramp(), 2);
        assert_eq!(c, vec![ramp_color(&ramp(), 0.25), ramp_color(&ramp(), 0.75)]);
        assert_eq!(band_colors(&ramp(), 0).len(), 1);
    }

    #[test]
    fn heights_fall_into_bands() {
        assert_eq!(band_of(0.0, 0.0, 10.0, 5), 0);
        assert_eq!(band_of(10.0, 0.0, 10.0, 5), 4);
        assert_eq!(band_of(4.9, 0.0, 10.0, 5), 2);
        assert_eq!(band_of(-5.0, 0.0, 10.0, 5), 0);
        assert_eq!(band_of(99.0, 0.0, 10.0, 5), 4);
        assert_eq!(band_of(3.0, 5.0, 5.0, 5), 0);
        assert_eq!(band_of(f64::NAN, 0.0, 10.0, 5), 0);
    }

    #[test]
    fn range_comes_from_settings_or_vertices() {
        let mut s = SiteSettings::default();
        let v = [DVec3::new(0.0, 0.0, 3.0), DVec3::new(1.0, 0.0, -2.0)];
        assert_eq!(range_of(&s, &v), Some((-2.0, 3.0)));
        assert_eq!(range_of(&s, &[]), None);
        s.elevation_range = Some([10.0, 20.0]);
        assert_eq!(range_of(&s, &v), Some((10.0, 20.0)));
    }

    #[test]
    fn bands_split_the_mesh() {
        let v = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
            DVec3::new(1.0, 1.0, 10.0),
            DVec3::new(2.0, 1.0, 10.0),
        ];
        // A low triangle, a high triangle that shares a vertex, and one with a bad index.
        let idx = [0, 1, 2, 1, 3, 4, 0, 1, 99];
        let parts = split_bands(&v, &idx, 0.0, 10.0, 2);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].band, 0);
        assert_eq!(parts[0].indices, vec![0, 1, 2]);
        assert_eq!(parts[1].band, 1);
        assert_eq!(parts[1].vertices.len(), 3);
        assert_eq!(parts[1].indices, vec![0, 1, 2]);
        // Only the bands that have triangles come back.
        assert_eq!(split_bands(&v, &[0, 1, 2], 0.0, 10.0, 8).len(), 1);
        assert!(split_bands(&v, &[], 0.0, 10.0, 8).is_empty());
    }

    proptest! {
        #[test]
        fn every_triangle_lands_in_exactly_one_band(
            zs in proptest::collection::vec(-1.0e6f64..1.0e6, 3..60),
            bands in 1u32..64,
        ) {
            let v: Vec<DVec3> = zs.iter().enumerate()
                .map(|(i, z)| DVec3::new(i as f64, (i * 7 % 5) as f64, *z)).collect();
            let idx: Vec<u32> = (0..(v.len() / 3 * 3) as u32).collect();
            let (lo, hi) = range_of(&SiteSettings::default(), &v).unwrap();
            let parts = split_bands(&v, &idx, lo, hi, bands);
            let total: usize = parts.iter().map(|p| p.indices.len()).sum();
            prop_assert_eq!(total, idx.len());
            let mut seen = std::collections::BTreeSet::new();
            for p in &parts {
                prop_assert!(p.band < bands as usize);
                prop_assert!(seen.insert(p.band));
                prop_assert!(p.indices.iter().all(|i| (*i as usize) < p.vertices.len()));
            }
            // Deterministic.
            prop_assert_eq!(parts, split_bands(&v, &idx, lo, hi, bands));
        }

        #[test]
        fn ramp_colors_are_always_defined(t in proptest::num::f64::ANY, n in 0usize..6) {
            let ramp: Vec<Color> = (0..n).map(|i| Color([i as u8 * 40, 10, 20, 255])).collect();
            let c = ramp_color(&ramp, t);
            prop_assert_eq!(c, ramp_color(&ramp, t));
        }
    }
}
