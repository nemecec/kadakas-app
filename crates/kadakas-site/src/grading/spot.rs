//! Spot elevations and stake objects: a point that reads the ground height from the site model
//! each time it is drawn or asked. The object is a small circle (`Shape::Circle`) with settings
//! under `Object::data["spot_elevation"]`. A stake object shows the northing and easting from the
//! georeference of the document, for setting out.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use kadakas_core::georef::Georeference;
use kadakas_core::model::{Object, ObjectId, Shape};

use super::draw::{Drawn, Label, Poly};
use super::{finite, meters, read};
use crate::model::SiteResult;

/// The key in `Object::data` that holds the settings.
pub const KEY: &str = "spot_elevation";
/// Radius of the marker on paper, in mm.
pub const MARKER_PAPER_RADIUS: f64 = 1.0;

/// Which heights the label shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Show {
    Existing,
    Proposed,
    #[default]
    Both,
}

/// What the point is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A spot elevation: the height of the ground.
    #[default]
    Spot,
    /// A stake: a point to set out, with northing and easting.
    Stake,
}

/// The settings of a spot elevation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpotData {
    /// The site model. `None`: the first one on the layer.
    pub site: Option<ObjectId>,
    pub show: Show,
    pub kind: Kind,
    /// Text height on paper in mm.
    pub label_height: f64,
    /// Text before the heights, for example `TC`.
    pub prefix: String,
    /// Decimals of the heights and coordinates in meters.
    pub decimals: usize,
    /// The name of a stake, for example `S1`.
    pub name: String,
}

impl Default for SpotData {
    fn default() -> Self {
        SpotData {
            site: None,
            show: Show::Both,
            kind: Kind::Spot,
            label_height: 2.5,
            prefix: String::new(),
            decimals: 2,
            name: String::new(),
        }
    }
}

impl SpotData {
    pub fn validate(&self) -> Result<(), String> {
        finite("label_height", self.label_height)?;
        if !(self.label_height > 0.0 && self.label_height <= 50.0) {
            return Err("label_height must be more than 0 and at most 50 mm on paper".into());
        }
        if self.decimals > 4 {
            return Err("decimals is at most 4".into());
        }
        Ok(())
    }
}

/// The heights of the ground at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// `None` outside the site model.
    pub existing: Option<f64>,
    pub proposed: Option<f64>,
    /// True when the proposed surface differs from the existing one at this point.
    pub modified: bool,
}

/// The smallest difference between the two surfaces that counts as a change, in mm.
const MODIFIED_EPS: f64 = 0.01;

/// Like [`read_at`], with `offset` added to the heights: the elevation of the layer of the model
/// (`grading::site_elevation`) gives heights above the origin, as `plants.drape` stores them.
pub fn read_at_offset(site: &SiteResult, p: DVec2, offset: f64) -> Reading {
    let r = read_at(site, p);
    Reading {
        existing: r.existing.map(|z| z + offset),
        proposed: r.proposed.map(|z| z + offset),
        ..r
    }
}

/// Reads the heights of a site model at a point. `modified` is true where the proposed surface
/// differs from the existing one, so it is false outside every modifier.
pub fn read_at(site: &SiteResult, p: DVec2) -> Reading {
    let existing = site.surface(crate::Surface::Existing).elevation_at(p);
    let proposed = site.elevation_at(p);
    let modified = match (existing, proposed) {
        (Some(e), Some(q)) => (e - q).abs() > MODIFIED_EPS,
        (None, None) => false,
        _ => true,
    };
    Reading {
        existing,
        proposed,
        modified,
    }
}

/// Northing and easting in meters. Without a georeference they are the local coordinates, and the
/// last value is false.
pub fn coordinates(p: DVec2, georef: Option<&Georeference>) -> (f64, f64, bool) {
    match georef {
        Some(g) => {
            let m = g.to_map(p);
            (m.y, m.x, true)
        }
        None => (p.y / 1000.0, p.x / 1000.0, false),
    }
}

/// The label text of a spot elevation or stake.
pub fn label(d: &SpotData, r: &Reading, p: DVec2, georef: Option<&Georeference>) -> String {
    let h = |v: Option<f64>| v.map_or("n/a".to_string(), |v| meters(v, d.decimals));
    let prefix = if d.prefix.is_empty() {
        String::new()
    } else {
        format!("{} ", d.prefix)
    };
    match d.kind {
        Kind::Spot => {
            let body = match d.show {
                Show::Existing => format!("EX {}", h(r.existing)),
                Show::Proposed => format!("PR {}", h(r.proposed)),
                Show::Both if !r.modified => h(r.proposed),
                Show::Both => format!("EX {} / PR {}", h(r.existing), h(r.proposed)),
            };
            format!("{prefix}{body}")
        }
        Kind::Stake => {
            let (n, e, _) = coordinates(p, georef);
            let z = if d.show == Show::Existing {
                r.existing
            } else {
                r.proposed
            };
            let name = if d.name.is_empty() {
                String::new()
            } else {
                format!("{} ", d.name)
            };
            let p = d.decimals.max(3);
            format!("{prefix}{name}N {n:.p$} E {e:.p$} Z {}", h(z))
        }
    }
}

/// The settings of a spot elevation object. `None` for other objects.
pub fn data(obj: &Object) -> Option<SpotData> {
    if !matches!(obj.shape, Shape::Circle { .. }) {
        return None;
    }
    read(obj, KEY)
}

/// The label and a cross in the marker. `scale` is the drawing scale of the layer.
pub fn draw(
    id: ObjectId,
    center: DVec2,
    radius: f64,
    d: &SpotData,
    text: &str,
    scale: f64,
) -> Drawn {
    let mut out = Drawn::default();
    let black = [0, 0, 0, 255];
    let w = 0.18 * scale;
    out.polys.push(
        Poly::new(
            "spot-marker",
            id,
            vec![vec![center - DVec2::X * radius, center + DVec2::X * radius]],
            false,
        )
        .stroked(black, w),
    );
    out.polys.push(
        Poly::new(
            "spot-marker",
            id,
            vec![vec![center - DVec2::Y * radius, center + DVec2::Y * radius]],
            false,
        )
        .stroked(black, w),
    );
    out.labels.push(Label::new(
        "spot-label",
        id,
        center + DVec2::new(radius * 1.5, radius * 1.2),
        text.to_string(),
        d.label_height * scale,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grading::testutil::*;
    use kadakas_core::command::Editor;
    use kadakas_core::model::ModifierKind;
    use proptest::prelude::*;
    use serde_json::json;

    fn modified_site() -> SiteResult {
        let mut ed = Editor::default();
        let id = site(&mut ed, |x, _| x / 10.0);
        let b = kadakas_core::curve::corners(&[
            DVec2::new(2000.0, 2000.0),
            DVec2::new(4000.0, 2000.0),
            DVec2::new(4000.0, 4000.0),
            DVec2::new(2000.0, 4000.0),
        ]);
        add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 900.0,
                    slope: None,
                },
                boundary: b,
            },
        );
        SiteResult::compute(ed.doc(), id).unwrap()
    }

    fn plain_site() -> SiteResult {
        let mut ed = Editor::default();
        let id = site(&mut ed, |x, y| x / 10.0 + y / 20.0);
        SiteResult::compute(ed.doc(), id).unwrap()
    }

    #[test]
    fn reading_equals_the_site_model() {
        let s = modified_site();
        let inside = DVec2::new(3000.0, 3000.0);
        let r = read_at(&s, inside);
        assert_eq!(r.proposed, s.elevation_at(inside));
        assert!(
            (r.proposed.unwrap() - 900.0).abs() < 1e-9
                && (r.existing.unwrap() - 300.0).abs() < 1e-9
        );
        assert!(r.modified);
        // Outside every modifier the two surfaces are the same: the reading is not modified.
        let outside = read_at(&s, DVec2::new(7000.0, 6000.0));
        assert!(!outside.modified, "{outside:?}");
        assert_eq!(outside.existing, outside.proposed);
        let out = read_at(&s, DVec2::new(-10.0, 0.0));
        assert_eq!((out.existing, out.proposed), (None, None));
    }

    proptest! {
        #[test]
        fn spot_always_equals_elevation_at(x in 0.0f64..10_000.0, y in 0.0f64..10_000.0) {
            let s = plain_site();
            let p = DVec2::new(x, y);
            let r = read_at(&s, p);
            prop_assert_eq!(r.proposed, s.elevation_at(p));
            prop_assert_eq!(r.existing, s.surface(crate::Surface::Existing).elevation_at(p));
            prop_assert!(!r.modified);
        }
    }

    #[test]
    fn labels() {
        let both = Reading {
            existing: Some(300.0),
            proposed: Some(900.0),
            modified: true,
        };
        let same = Reading {
            modified: false,
            ..both
        };
        let d = SpotData::default();
        let p = DVec2::new(1500.0, 2500.0);
        assert_eq!(label(&d, &both, p, None), "EX 0.30 / PR 0.90");
        assert_eq!(label(&d, &same, p, None), "0.90");
        assert_eq!(
            label(
                &SpotData {
                    show: Show::Existing,
                    prefix: "TC".into(),
                    ..d.clone()
                },
                &both,
                p,
                None
            ),
            "TC EX 0.30"
        );
        assert_eq!(
            label(
                &SpotData {
                    show: Show::Proposed,
                    decimals: 3,
                    ..d.clone()
                },
                &both,
                p,
                None
            ),
            "PR 0.900"
        );
        let none = Reading {
            existing: None,
            proposed: None,
            modified: false,
        };
        assert_eq!(label(&d, &none, p, None), "n/a");
        let stake = SpotData {
            kind: Kind::Stake,
            name: "S1".into(),
            ..d.clone()
        };
        assert_eq!(label(&stake, &both, p, None), "S1 N 2.500 E 1.500 Z 0.90");
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(500_000.0, 6_700_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        assert_eq!(
            label(&stake, &both, p, Some(&g)),
            "S1 N 6700002.500 E 500001.500 Z 0.90"
        );
        assert_eq!(
            label(
                &SpotData {
                    show: Show::Existing,
                    name: String::new(),
                    ..stake.clone()
                },
                &both,
                p,
                None
            ),
            "N 2.500 E 1.500 Z 0.30"
        );
        let (n, e, geo) = coordinates(p, Some(&g));
        assert!(geo && (n - 6_700_002.5).abs() < 1e-9 && (e - 500_001.5).abs() < 1e-9);
        assert!(!coordinates(p, None).2);
    }

    #[test]
    fn settings_are_checked() {
        assert!(SpotData::default().validate().is_ok());
        assert!(
            SpotData {
                label_height: 0.0,
                ..SpotData::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            SpotData {
                label_height: f64::NAN,
                ..SpotData::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            SpotData {
                label_height: 99.0,
                ..SpotData::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            SpotData {
                decimals: 9,
                ..SpotData::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn objects_and_drawing() {
        let mut ed = Editor::default();
        let id = add_with(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(10.0, 20.0),
                radius: 100.0,
            },
            KEY,
            json!({"prefix": "<A>"}),
        );
        let d = data(ed.doc().object(id).unwrap()).unwrap();
        assert_eq!(d.prefix, "<A>");
        let out = draw(id, DVec2::new(10.0, 20.0), 100.0, &d, "<A> 1.20", 100.0);
        assert_eq!(out.polys.len(), 2);
        assert_eq!(out.labels[0].height, 250.0);
        let s = out.svg();
        assert!(
            s.contains("spot-marker")
                && s.contains("&lt;A&gt; 1.20")
                && s.contains(r#"font-size="250""#)
        );
        let line = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        assert!(data(ed.doc().object(line).unwrap()).is_none());
    }
}
