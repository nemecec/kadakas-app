//! Data of site models and site modifiers. Only the data lives here. The terrain
//! algorithms (TIN, contours, cut and fill) are in the `kadakas-site` crate, and derived results
//! are never saved in files.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};
use crate::model::Color;
use crate::present::{MappingKind, TextureMapping};
use crate::resource::ResourceId;

/// A 3D polygon or contour that is source data of a site model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteLine {
    pub points: Vec<DVec3>,
    #[serde(default)]
    pub closed: bool,
}

/// Which surface a view shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceDisplay {
    Existing,
    #[default]
    Proposed,
    Both,
}

/// How the 3D view shows the terrain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display3d {
    #[default]
    Mesh,
    Contours,
    /// Mesh and contours together.
    Both,
}

/// How the 3D terrain mesh gets its look (3D-05).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLook {
    /// The surface color, or the material that a slot of the object or of its class gives
    /// (the mapping of that material applies).
    #[default]
    Color,
    /// A texture with a planar top projection: the material of `texture`, or, when `texture`
    /// is not set, the material of the slot of the object or its class. The tile size, the
    /// offset and the rotation of the site model set the mapping.
    Texture,
    /// Colors by height: the elevation range is cut into bands, and each band takes its color
    /// from the gradient ramp. Materials do not apply.
    Elevation,
}

impl SurfaceLook {
    pub const ALL: [SurfaceLook; 3] = [
        SurfaceLook::Color,
        SurfaceLook::Texture,
        SurfaceLook::Elevation,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SurfaceLook::Color => "color",
            SurfaceLook::Texture => "texture",
            SurfaceLook::Elevation => "elevation",
        }
    }
}

/// Smallest and largest tile size of a terrain texture, in mm.
pub const MIN_TEXTURE_TILE: f64 = 10.0;
pub const MAX_TEXTURE_TILE: f64 = 1.0e8;
/// Largest offset of a terrain texture, in mm.
pub const MAX_TEXTURE_OFFSET: f64 = 1.0e9;
/// The most elevation bands, and the most colors of a ramp.
pub const MAX_ELEVATION_BANDS: u32 = 64;
pub const MAX_RAMP_STOPS: usize = 16;
/// The most repeats of a texture across a terrain. A smaller tile is made larger to stay under
/// it, so the texture coordinates stay precise in single precision (a coordinate of 8192 repeats
/// has a step of 0.001 repeats, which is a fraction of a texel).
pub const MAX_TEXTURE_REPEATS: f64 = 8192.0;
/// Largest turn of a terrain texture, in degrees (the mapping uses the turn modulo 360).
pub const MAX_TEXTURE_ROTATION: f64 = 1.0e6;

/// The tile size that renderers use for a terrain of the given extent (its longest side, mm):
/// the tile of the settings, or a larger one when the terrain would hold more than
/// [`MAX_TEXTURE_REPEATS`] repeats.
pub fn effective_tile(tile: f64, extent: f64) -> f64 {
    tile.max(extent / MAX_TEXTURE_REPEATS).max(1e-6)
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// The ramp of a new site model: low land green, then yellow, brown and white.
pub fn default_elevation_ramp() -> Vec<Color> {
    vec![
        Color([70, 132, 76, 255]),
        Color([214, 204, 108, 255]),
        Color([150, 104, 64, 255]),
        Color([244, 244, 240, 255]),
    ]
}

fn default_bands() -> u32 {
    12
}

fn default_tile() -> f64 {
    2000.0
}

fn is_default_bands(v: &u32) -> bool {
    *v == default_bands()
}

fn is_default_tile(v: &f64) -> bool {
    *v == default_tile()
}

fn is_default_ramp(v: &[Color]) -> bool {
    v == default_elevation_ramp().as_slice()
}

/// Settings of a site model. Lengths are in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteSettings {
    /// Height between two contour lines.
    pub minor_interval: f64,
    /// Height between two major (thick) contour lines. A multiple of the minor interval.
    pub major_interval: f64,
    /// Round the corners of contour lines.
    pub smooth_contours: bool,
    /// Draw the elevation next to major contours in 2D.
    pub label_contours: bool,
    pub display_2d: SurfaceDisplay,
    pub display_3d: Display3d,
    /// Which surface the 3D view shows.
    pub surface_3d: SurfaceDisplay,
    /// The elevation that contour levels count from.
    pub datum: f64,
    /// Color of the existing surface (mesh and contours). `None` picks a default.
    pub existing_color: Option<Color>,
    /// Color of the proposed surface. `None` picks a default.
    pub proposed_color: Option<Color>,
    /// Only the part of the surface inside this ring stays. Empty: no crop. The ring may be concave.
    pub crop: Vec<DVec2>,
    /// When false, the proposed surface changes only on an explicit "Update Site Model" and shows
    /// "out of date" until then.
    pub auto_update: bool,
    /// How the 3D mesh gets its look: surface color, texture or colors by elevation.
    #[serde(skip_serializing_if = "is_default")]
    pub surface_look: SurfaceLook,
    /// The material resource (with a texture) for the texture look. `None`: the material that
    /// a slot of the object or of its class gives ("by class").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<ResourceId>,
    /// Size of one repeat of the texture on the ground (a square), in mm.
    #[serde(skip_serializing_if = "is_default_tile")]
    pub texture_tile: f64,
    /// Shift of the texture on the ground, in mm.
    #[serde(skip_serializing_if = "is_default")]
    pub texture_offset: [f64; 2],
    /// Counterclockwise turn of the texture, in degrees.
    #[serde(skip_serializing_if = "is_default")]
    pub texture_rotation: f64,
    /// The number of elevation bands of the elevation look.
    #[serde(skip_serializing_if = "is_default_bands")]
    pub elevation_bands: u32,
    /// The colors of the gradient ramp, from the lowest to the highest ground.
    #[serde(skip_serializing_if = "is_default_ramp")]
    pub elevation_ramp: Vec<Color>,
    /// Lowest and highest elevation of the ramp. `None`: the range of the shown surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_range: Option<[f64; 2]>,
    /// Cut the bands at the contour levels: at the datum plus whole major intervals, instead of
    /// at equal parts of the range. Falls back to equal parts when that would give more than
    /// `MAX_ELEVATION_BANDS` bands.
    #[serde(skip_serializing_if = "is_default")]
    pub elevation_bands_at_contours: bool,
}

/// The most contour levels a site model may ask for.
pub const MAX_CONTOUR_LEVELS: f64 = 10_000.0;

impl Default for SiteSettings {
    fn default() -> Self {
        SiteSettings {
            minor_interval: 500.0,
            major_interval: 2500.0,
            smooth_contours: false,
            label_contours: false,
            display_2d: SurfaceDisplay::default(),
            display_3d: Display3d::default(),
            surface_3d: SurfaceDisplay::default(),
            datum: 0.0,
            existing_color: None,
            proposed_color: None,
            crop: Vec::new(),
            auto_update: true,
            surface_look: SurfaceLook::default(),
            texture: None,
            texture_tile: default_tile(),
            texture_offset: [0.0, 0.0],
            texture_rotation: 0.0,
            elevation_bands: default_bands(),
            elevation_ramp: default_elevation_ramp(),
            elevation_range: None,
            elevation_bands_at_contours: false,
        }
    }
}

impl SiteSettings {
    /// True if every number is finite.
    pub fn is_finite(&self) -> bool {
        self.minor_interval.is_finite()
            && self.major_interval.is_finite()
            && self.datum.is_finite()
            && self.crop.iter().all(|p| p.is_finite())
            && self.texture_tile.is_finite()
            && self.texture_rotation.is_finite()
            && self.texture_offset.iter().all(|v| v.is_finite())
            && self
                .elevation_range
                .is_none_or(|r| r.iter().all(|v| v.is_finite()))
    }

    /// The mapping of the texture look: a planar projection from above, in world units.
    pub fn texture_mapping(&self) -> TextureMapping {
        TextureMapping {
            kind: MappingKind::Planar,
            scale: [self.texture_tile, self.texture_tile],
            rotation: self.texture_rotation.rem_euclid(360.0),
            offset: self.texture_offset,
        }
    }

    /// Puts the fields of the 3D look back to their defaults.
    pub fn reset_look(&mut self) {
        let d = SiteSettings::default();
        self.surface_look = d.surface_look;
        self.texture = d.texture;
        self.texture_tile = d.texture_tile;
        self.texture_offset = d.texture_offset;
        self.texture_rotation = d.texture_rotation;
        self.elevation_bands = d.elevation_bands;
        self.elevation_ramp = d.elevation_ramp;
        self.elevation_range = d.elevation_range;
        self.elevation_bands_at_contours = d.elevation_bands_at_contours;
    }

    /// The numbers of the 3D look, for the range check of a shape.
    pub fn look_numbers(&self) -> Vec<f64> {
        let mut v = vec![
            self.texture_tile,
            self.texture_offset[0],
            self.texture_offset[1],
        ];
        v.extend(self.elevation_range.into_iter().flatten());
        v
    }

    /// Why the settings are not usable, if they are not.
    pub fn problem(&self) -> Option<&'static str> {
        if !self.is_finite() {
            Some("site model numbers must be finite")
        } else if self.minor_interval <= 0.0 {
            Some("minor contour interval must be positive")
        } else if self.major_interval < self.minor_interval {
            Some("major contour interval must not be smaller than the minor interval")
        } else if self.major_interval / self.minor_interval > MAX_CONTOUR_LEVELS {
            Some("major contour interval is too large for the minor interval (at most 10000 times)")
        } else if self.minor_interval < 1e-3 {
            Some("minor contour interval must be at least 0.001 mm")
        } else if !self.crop.is_empty() && self.crop.len() < 3 {
            Some("crop boundary needs at least 3 points")
        } else if !(MIN_TEXTURE_TILE..=MAX_TEXTURE_TILE).contains(&self.texture_tile) {
            Some("texture tile size must be from 10 mm to 100000 m")
        } else if self
            .texture_offset
            .iter()
            .any(|v| v.abs() > MAX_TEXTURE_OFFSET)
        {
            Some("texture offset must not be larger than 1000000 m")
        } else if !(1..=MAX_ELEVATION_BANDS).contains(&self.elevation_bands) {
            Some("elevation bands must be from 1 to 64")
        } else if !(1..=MAX_RAMP_STOPS).contains(&self.elevation_ramp.len()) {
            Some("the elevation ramp needs 1 to 16 colors")
        } else if self.texture_rotation.abs() > MAX_TEXTURE_ROTATION {
            Some("texture rotation must be within a million degrees")
        } else if self
            .elevation_range
            .is_some_and(|r| r.iter().any(|v| v.abs() > crate::model::MAX_COORD))
        {
            Some("the elevation range is out of range")
        } else if self.elevation_range.is_some_and(|r| r[1] <= r[0]) {
            Some("the elevation range must have its high end above its low end")
        } else {
            None
        }
    }
}

/// A slope of a pad: it falls in `direction` (radians, counterclockwise from +X) by `percent`
/// of the horizontal distance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadSlope {
    pub direction: f64,
    pub percent: f64,
}

/// What a site modifier does to the proposed surface.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModifierKind {
    /// A level or sloped area. `elevation` is the height at the middle of the boundary points.
    Pad {
        elevation: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slope: Option<PadSlope>,
    },
    /// The edge of the modified area: outside it, the proposed surface stays the existing one.
    GradeLimit,
}

impl ModifierKind {
    pub fn is_finite(&self) -> bool {
        match self {
            ModifierKind::Pad { elevation, slope } => {
                elevation.is_finite()
                    && slope.is_none_or(|s| s.direction.is_finite() && s.percent.is_finite())
            }
            ModifierKind::GradeLimit => true,
        }
    }

    /// The numbers of the kind, for range checks.
    pub fn numbers(&self) -> Vec<f64> {
        match self {
            ModifierKind::Pad { elevation, slope } => {
                let mut v = vec![*elevation];
                v.extend(slope.iter().flat_map(|s| [s.percent]));
                v
            }
            ModifierKind::GradeLimit => Vec::new(),
        }
    }
}

/// The keys in `Object::data` of objects that an extension draws on top of their plain shape:
/// landscape walls, grade objects, spot elevations, site sections and analysis legends.
/// They are made in `kadakas-site` (module `grading`).
pub const EXTRA_KEYS: [&str; 5] = [
    "landscape_wall",
    "grade_object",
    "spot_elevation",
    "site_section",
    "analysis_legend",
];

/// True if an extension should draw more for this object than its shape. Site models, modifiers,
/// hardscapes and landscape areas have their own hooks.
pub fn carries_extra(obj: &crate::model::Object) -> bool {
    !matches!(
        obj.shape,
        crate::model::Shape::SiteModel { .. }
            | crate::model::Shape::SiteModifier { .. }
            | crate::model::Shape::Hardscape { .. }
            | crate::model::Shape::LandscapeArea { .. }
    ) && EXTRA_KEYS.iter().any(|k| obj.data.contains_key(*k))
}

/// The corners of a modifier boundary with curves flattened.
pub fn boundary_points(boundary: &[Vertex]) -> Vec<DVec2> {
    curve::flatten(boundary, true, 2.0)
}

/// The farthest distance from the middle of the boundary points along `direction` (radians).
fn reach(boundary: &[DVec2], direction: f64) -> f64 {
    let mid = boundary.iter().copied().sum::<DVec2>() / boundary.len().max(1) as f64;
    let d = DVec2::from_angle(direction);
    boundary.iter().map(|p| d.dot(*p - mid)).fold(0.0, f64::max)
}

/// A pad slope from the elevation at its end: the height where the boundary ends in the
/// falling `direction`. `None` if the boundary has no extent in that direction.
pub fn slope_from_end_elevation(
    boundary: &[DVec2],
    elevation: f64,
    direction: f64,
    end_elevation: f64,
) -> Option<PadSlope> {
    let r = reach(boundary, direction);
    (r > 0.0 && end_elevation.is_finite()).then(|| PadSlope {
        direction,
        percent: (elevation - end_elevation) / r * 100.0,
    })
}

/// The elevation at the end of a sloped pad (see `slope_from_end_elevation`).
pub fn end_elevation(boundary: &[DVec2], elevation: f64, slope: PadSlope) -> f64 {
    elevation - slope.percent / 100.0 * reach(boundary, slope.direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_valid_and_checked() {
        let mut s = SiteSettings::default();
        assert_eq!(s.problem(), None);
        s.minor_interval = 0.0;
        assert!(s.problem().unwrap().contains("positive"));
        s.minor_interval = 500.0;
        s.major_interval = 100.0;
        assert!(s.problem().unwrap().contains("major"));
        s.major_interval = f64::NAN;
        assert!(s.problem().unwrap().contains("finite"));
        s.major_interval = 500.0;
        s.minor_interval = 1e-300;
        assert!(s.problem().is_some());
        s.minor_interval = 1.0;
        s.major_interval = 1e6;
        assert!(s.problem().unwrap().contains("too large"));
        s.major_interval = 10.0;
        s.crop = vec![DVec2::ZERO, DVec2::X];
        assert!(s.problem().unwrap().contains("crop"));
    }

    #[test]
    fn terrain_look_settings_are_checked() {
        let ok = SiteSettings::default();
        let bad = |f: &dyn Fn(&mut SiteSettings), word: &str| {
            let mut s = ok.clone();
            f(&mut s);
            assert!(s.problem().expect(word).contains(word), "{word}");
        };
        bad(&|s| s.texture_tile = 1.0, "tile");
        bad(&|s| s.texture_tile = 1e12, "tile");
        bad(&|s| s.texture_tile = f64::NAN, "finite");
        bad(&|s| s.texture_offset = [2e9, 0.0], "offset");
        bad(&|s| s.texture_offset = [0.0, f64::INFINITY], "finite");
        bad(&|s| s.texture_rotation = f64::NAN, "finite");
        bad(&|s| s.elevation_bands = 0, "bands");
        bad(&|s| s.elevation_bands = 65, "bands");
        bad(&|s| s.elevation_ramp = vec![], "ramp");
        bad(&|s| s.elevation_ramp = vec![Color([0; 4]); 17], "ramp");
        bad(&|s| s.elevation_range = Some([5.0, 5.0]), "range");
        bad(&|s| s.elevation_range = Some([0.0, 2e9]), "out of range");
        bad(&|s| s.texture_rotation = 2e6, "rotation");
        bad(&|s| s.elevation_range = Some([0.0, f64::NAN]), "finite");
    }

    #[test]
    fn terrain_look_settings_round_trip_and_stay_out_of_old_files() {
        let plain = serde_json::to_value(SiteSettings::default()).unwrap();
        for k in [
            "surface_look",
            "texture",
            "texture_tile",
            "texture_offset",
            "texture_rotation",
            "elevation_bands",
            "elevation_ramp",
            "elevation_range",
            "elevation_bands_at_contours",
        ] {
            assert!(plain.get(k).is_none(), "{k}");
        }
        let s = SiteSettings {
            surface_look: SurfaceLook::Texture,
            texture: Some(ResourceId::from_num(7)),
            texture_tile: 750.0,
            texture_offset: [1.0, 2.0],
            texture_rotation: 30.0,
            elevation_bands: 5,
            elevation_ramp: vec![Color([1, 2, 3, 255])],
            elevation_range: Some([0.0, 9.0]),
            elevation_bands_at_contours: true,
            ..Default::default()
        };
        let back: SiteSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let m = s.texture_mapping();
        assert_eq!(m.kind, MappingKind::Planar);
        assert_eq!(
            (m.scale, m.rotation, m.offset),
            ([750.0; 2], 30.0, [1.0, 2.0])
        );
        // A turn is taken modulo 360 and reset puts the look back.
        let mut t = s.clone();
        t.texture_rotation = -90.0;
        assert_eq!(t.texture_mapping().rotation, 270.0);
        assert_eq!(t.look_numbers().len(), 5);
        t.reset_look();
        assert_eq!(t, SiteSettings::default());
        assert_eq!(
            SurfaceLook::ALL.map(SurfaceLook::id),
            ["color", "texture", "elevation"]
        );
    }

    #[test]
    fn settings_read_with_missing_fields() {
        let s: SiteSettings =
            serde_json::from_str(r#"{"minor_interval": 250.0, "display_2d": "both"}"#).unwrap();
        assert_eq!(s.minor_interval, 250.0);
        assert_eq!(s.display_2d, SurfaceDisplay::Both);
        assert_eq!(s.major_interval, 2500.0);
    }

    #[test]
    fn modifier_kind_numbers_and_json() {
        let k = ModifierKind::Pad {
            elevation: 5.0,
            slope: Some(PadSlope {
                direction: 0.0,
                percent: 2.0,
            }),
        };
        assert!(k.is_finite());
        assert_eq!(k.numbers(), vec![5.0, 2.0]);
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(serde_json::from_str::<ModifierKind>(&json).unwrap(), k);
        assert!(
            !ModifierKind::Pad {
                elevation: f64::NAN,
                slope: None
            }
            .is_finite()
        );
        assert!(
            !ModifierKind::Pad {
                elevation: 0.0,
                slope: Some(PadSlope {
                    direction: f64::NAN,
                    percent: 1.0
                })
            }
            .is_finite()
        );
        assert!(
            ModifierKind::GradeLimit.is_finite() && ModifierKind::GradeLimit.numbers().is_empty()
        );
    }

    #[test]
    fn slope_from_the_elevation_at_the_end() {
        let sq = [
            DVec2::new(0.0, 0.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 10.0),
            DVec2::new(0.0, 10.0),
        ];
        // Falls toward +X; the end is 5 from the middle. 10 at the middle and 9 at the end is 20 percent.
        let s = slope_from_end_elevation(&sq, 10.0, 0.0, 9.0).unwrap();
        assert!((s.percent - 20.0).abs() < 1e-9 && s.direction == 0.0);
        assert!((end_elevation(&sq, 10.0, s) - 9.0).abs() < 1e-9);
        assert!(slope_from_end_elevation(&[], 1.0, 0.0, 2.0).is_none());
        assert!(slope_from_end_elevation(&sq, 1.0, 0.0, f64::NAN).is_none());
    }

    #[test]
    fn extras_are_found_by_their_data_key() {
        use crate::model::{Object, ObjectId, Shape};
        let mut o = Object::new(
            ObjectId::from_num(1),
            crate::model::FIRST_LAYER,
            crate::model::NONE_CLASS,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        assert!(!carries_extra(&o));
        for key in EXTRA_KEYS {
            o.data.clear();
            o.data.insert(key.into(), serde_json::json!({}));
            assert!(carries_extra(&o), "{key}");
        }
        // Objects with their own hooks are left out.
        o.shape = Shape::Hardscape {
            boundary: curve::corners(&[DVec2::ZERO]),
            material: "m".into(),
            thickness: None,
            elevation: None,
        };
        assert!(!carries_extra(&o));
        o.shape = Shape::SiteModifier {
            modifier: ModifierKind::GradeLimit,
            boundary: vec![],
        };
        assert!(!carries_extra(&o));
    }

    #[test]
    fn a_textured_site_model_takes_its_texture_along_when_it_moves() {
        use crate::model::Shape;
        let mut textured = Shape::SiteModel {
            points: vec![DVec3::new(1.0, 2.0, 3.0)],
            lines: vec![],
            settings: SiteSettings {
                surface_look: SurfaceLook::Texture,
                texture_offset: [10.0, 20.0],
                ..Default::default()
            },
        };
        textured.translate(DVec2::new(100.0, -5.0));
        let Shape::SiteModel {
            settings, points, ..
        } = &textured
        else {
            panic!()
        };
        assert_eq!(settings.texture_offset, [110.0, 15.0]);
        assert_eq!(points[0].x, 101.0);
        // Another look keeps its settings as they are.
        let mut plain = Shape::SiteModel {
            points: vec![],
            lines: vec![],
            settings: SiteSettings::default(),
        };
        plain.translate(DVec2::new(100.0, 0.0));
        assert!(
            matches!(plain, Shape::SiteModel { settings, .. } if settings == SiteSettings::default())
        );
    }

    #[test]
    fn a_file_with_a_bad_look_loads_with_the_look_reset() {
        use crate::model::{Document, Shape};
        let mut doc = Document::default();
        let mut s = SiteSettings {
            surface_look: SurfaceLook::Elevation,
            minor_interval: 250.0,
            ..Default::default()
        };
        s.texture_tile = 0.0;
        s.elevation_bands = 900;
        let id = crate::model::ObjectId(doc.alloc_id());
        doc.objects.insert(
            id,
            crate::model::Object::new(
                id,
                crate::model::FIRST_LAYER,
                crate::model::NONE_CLASS,
                Shape::SiteModel {
                    points: vec![],
                    lines: vec![],
                    settings: s,
                },
            ),
        );
        let text = serde_json::to_string(&doc).unwrap();
        let loaded = Document::from_json(&text).expect("the file opens");
        let Shape::SiteModel { settings, .. } = &loaded.object(id).unwrap().shape else {
            panic!()
        };
        assert_eq!(settings.surface_look, SurfaceLook::Color);
        assert_eq!(settings.texture_tile, 2000.0);
        assert_eq!(settings.minor_interval, 250.0, "the rest is kept");
        assert_eq!(loaded.load_notes().len(), 1);
        assert!(loaded.load_notes()[0].contains("reset"));
        // A problem outside the look still refuses the file.
        let mut bad = doc.clone();
        if let Shape::SiteModel { settings, .. } = &mut bad.object_mut(id).unwrap().shape {
            settings.minor_interval = -1.0;
        }
        assert!(Document::from_json(&serde_json::to_string(&bad).unwrap()).is_err());
        // A good file has no notes.
        assert!(Document::default().load_notes().is_empty());
    }

    #[test]
    fn the_tile_grows_on_a_very_big_terrain() {
        assert_eq!(effective_tile(1000.0, 1.0e6), 1000.0);
        assert_eq!(effective_tile(10.0, 8192.0 * 100.0), 100.0);
        assert!(effective_tile(0.0, 0.0) > 0.0);
    }

    #[test]
    fn boundary_flattens_to_points() {
        let b = curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::ONE]);
        assert_eq!(boundary_points(&b).len(), 3);
    }
}
