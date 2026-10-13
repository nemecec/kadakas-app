//! 3D presentation: materials, render styles, the sun and the material slots.
//!
//! - A [`Material`] is a resource. It combines the look (a base color and an optional texture)
//!   with physical data (roughness, metallic and opacity). The texture is a generated pattern
//!   or a PNG image that lives inside the document.
//! - A [`RenderStyle`] is a resource. It says how a view or a viewport draws the model.
//! - [`Presentation`] is the part of the document that links these to the model: the sun
//!   (a heliodon), the material slots and the active render style.
//!
//! A *slot* is a place that can take a material: one object (or one part of it, like the trunk
//! of a plant), a part of a style (the canopy of a plant style, a layer of a hardscape style), or
//! a class. [`SlotIndex::resolve`] picks the material of a triangle.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{ClassId, Color, Document, Object, ObjectId, Shape};
use crate::resource::{ResourceBody, ResourceId};
use crate::site::SurfaceLook;
use crate::sun::{self, SunTime};

/// Largest texture image, as base64 text of the PNG file.
pub const MAX_TEXTURE_BASE64: usize = 4 * 1024 * 1024;
/// Largest side of a texture image in pixels.
pub const MAX_TEXTURE_SIDE: u32 = 2048;

// --- Mesh parts -------------------------------------------------------------------------

/// Triangle part: everything else.
pub const PART_MAIN: u8 = 0;
/// Triangle part: the trunk of a plant.
pub const PART_TRUNK: u8 = 1;
/// Triangle part: the canopy of a plant.
pub const PART_CANOPY: u8 = 2;
/// Triangle part: the sides of a plain slab.
pub const PART_SIDE: u8 = 3;
/// Triangle part of layer `i` of a hardscape solid: `PART_LAYER + i`. Layer 0 is the surface.
pub const PART_LAYER: u8 = 10;

// --- Render modes -----------------------------------------------------------------------

/// How a 3D view draws the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    /// Lit surfaces, with their edges if the render style says so.
    #[default]
    Shaded,
    /// Edges only, all of them.
    Wireframe,
    /// Edges that the surfaces do not cover. Surfaces are white.
    HiddenLine,
    /// Like hidden line, and the covered edges are dashed.
    DashedHiddenLine,
}

impl RenderMode {
    pub const ALL: [RenderMode; 4] = [
        RenderMode::Shaded,
        RenderMode::Wireframe,
        RenderMode::HiddenLine,
        RenderMode::DashedHiddenLine,
    ];

    pub fn id(self) -> &'static str {
        match self {
            RenderMode::Shaded => "shaded",
            RenderMode::Wireframe => "wireframe",
            RenderMode::HiddenLine => "hidden_line",
            RenderMode::DashedHiddenLine => "dashed_hidden_line",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RenderMode::Shaded => "Shaded",
            RenderMode::Wireframe => "Wireframe",
            RenderMode::HiddenLine => "Hidden Line",
            RenderMode::DashedHiddenLine => "Dashed Hidden Line",
        }
    }

    /// Finds a mode by id or label. Case, spaces and hyphens do not matter. `opengl` is shaded.
    pub fn from_name(name: &str) -> Option<RenderMode> {
        let norm: String = name
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c == ' ' || c == '-' { '_' } else { c })
            .collect();
        match norm.as_str() {
            "shaded" | "opengl" | "textured" => Some(RenderMode::Shaded),
            "wireframe" => Some(RenderMode::Wireframe),
            "hidden_line" | "hiddenline" => Some(RenderMode::HiddenLine),
            "dashed_hidden_line" | "dashed_hidden" => Some(RenderMode::DashedHiddenLine),
            _ => None,
        }
    }

    /// The mode that the shortcut for shaded and wireframe switches to.
    pub fn toggled(self) -> RenderMode {
        match self {
            RenderMode::Shaded => RenderMode::Wireframe,
            _ => RenderMode::Shaded,
        }
    }

    /// True for the two modes that output hidden-line vectors.
    pub fn is_hidden_line(self) -> bool {
        matches!(self, RenderMode::HiddenLine | RenderMode::DashedHiddenLine)
    }
}

// --- Materials --------------------------------------------------------------------------

/// Patterns that the app can make without an image file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Procedural {
    Grass,
    Gravel,
    Pavers,
    Mulch,
    Water,
    Brick,
    Timber,
    /// Plain noise, for rough ground and plaster.
    Noise,
}

impl Procedural {
    pub const ALL: [Procedural; 8] = [
        Procedural::Grass,
        Procedural::Gravel,
        Procedural::Pavers,
        Procedural::Mulch,
        Procedural::Water,
        Procedural::Brick,
        Procedural::Timber,
        Procedural::Noise,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Procedural::Grass => "grass",
            Procedural::Gravel => "gravel",
            Procedural::Pavers => "pavers",
            Procedural::Mulch => "mulch",
            Procedural::Water => "water",
            Procedural::Brick => "brick",
            Procedural::Timber => "timber",
            Procedural::Noise => "noise",
        }
    }

    pub fn from_name(name: &str) -> Option<Procedural> {
        let n = name.trim().to_ascii_lowercase();
        Procedural::ALL.into_iter().find(|p| p.id() == n)
    }
}

/// The texture of a material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Texture {
    /// A generated pattern. The seed changes the random part.
    Procedural {
        kind: Procedural,
        #[serde(default)]
        seed: u32,
    },
    /// A PNG image inside the document (base64 text of the file).
    Image {
        png_base64: String,
        width: u32,
        height: u32,
    },
}

/// How a texture is put on a surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingKind {
    /// Straight down along Z. Vertical faces get streaks.
    Planar,
    /// Along the axis that the face points to most.
    #[default]
    Box,
    /// Around a vertical axis through the middle of the object.
    Cylindrical,
}

impl MappingKind {
    pub fn id(self) -> &'static str {
        match self {
            MappingKind::Planar => "planar",
            MappingKind::Box => "box",
            MappingKind::Cylindrical => "cylindrical",
        }
    }

    pub fn from_name(name: &str) -> Option<MappingKind> {
        match name.trim().to_ascii_lowercase().as_str() {
            "planar" => Some(MappingKind::Planar),
            "box" => Some(MappingKind::Box),
            "cylindrical" | "cylinder" => Some(MappingKind::Cylindrical),
            _ => None,
        }
    }
}

/// Size, turn and shift of a texture in world units (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextureMapping {
    pub kind: MappingKind,
    /// Size of one repeat of the texture along U and V, in mm.
    pub scale: [f64; 2],
    /// Counterclockwise turn in degrees.
    pub rotation: f64,
    /// Shift of the texture in mm.
    pub offset: [f64; 2],
}

impl Default for TextureMapping {
    fn default() -> Self {
        TextureMapping {
            kind: MappingKind::Box,
            scale: [1000.0, 1000.0],
            rotation: 0.0,
            offset: [0.0, 0.0],
        }
    }
}

/// Where a surface point lands in the texture, in repeats (1.0 is one full texture).
///
/// `p` is the world position in mm, `n` the unit normal. `center` and `radius` describe the
/// object (its middle and its half size) and only matter for cylindrical mapping.
pub fn texture_uv(
    m: &TextureMapping,
    p: [f64; 3],
    n: [f32; 3],
    center: [f64; 3],
    radius: f64,
) -> [f32; 2] {
    let uv = texture_uv_f64(m, p, n, center, radius);
    [uv[0] as f32, uv[1] as f32]
}

/// Like [`texture_uv`], in double precision. A caller can take away whole repeats before it
/// goes to single precision, so a texture far from the origin stays sharp.
pub fn texture_uv_f64(
    m: &TextureMapping,
    p: [f64; 3],
    n: [f32; 3],
    center: [f64; 3],
    radius: f64,
) -> [f64; 2] {
    let (a, b) = match m.kind {
        MappingKind::Planar => (p[0], p[1]),
        MappingKind::Box => {
            let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
            if az >= ax && az >= ay {
                (p[0], p[1])
            } else if ax >= ay {
                (p[1], p[2])
            } else {
                (p[0], p[2])
            }
        }
        MappingKind::Cylindrical => {
            let angle = (p[1] - center[1]).atan2(p[0] - center[0]);
            // The arc length at the radius of the object, so a texture is not stretched.
            (angle * radius.max(1.0), p[2])
        }
    };
    let (a, b) = (a - m.offset[0], b - m.offset[1]);
    let (s, c) = m.rotation.to_radians().sin_cos();
    let (u, v) = (a * c + b * s, -a * s + b * c);
    [u / m.scale[0], v / m.scale[1]]
}

/// A material: color and texture, plus roughness, metallic and opacity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    pub base_color: Color,
    pub texture: Option<Texture>,
    pub mapping: TextureMapping,
    /// 0 is a mirror, 1 is fully rough.
    pub roughness: f32,
    /// 0 is a non-metal, 1 is a metal.
    pub metallic: f32,
    /// 1 is solid, 0 is clear (glass and water).
    pub opacity: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            base_color: Color([200, 200, 200, 255]),
            texture: None,
            mapping: TextureMapping::default(),
            roughness: 0.8,
            metallic: 0.0,
            opacity: 1.0,
        }
    }
}

impl Material {
    pub fn validate(&self) -> Result<(), String> {
        for (name, v) in [
            ("roughness", self.roughness),
            ("metallic", self.metallic),
            ("opacity", self.opacity),
        ] {
            if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
                return Err(format!("{name} must be from 0 to 1"));
            }
        }
        let m = &self.mapping;
        if !m
            .scale
            .iter()
            .all(|s| s.is_finite() && *s > 0.0 && *s <= 1e9)
        {
            return Err("texture scale must be a positive, finite size".into());
        }
        if !(m.rotation.is_finite() && m.offset.iter().all(|o| o.is_finite() && o.abs() <= 1e9)) {
            return Err("texture rotation and offset must be finite".into());
        }
        if let Some(Texture::Image {
            png_base64,
            width,
            height,
        }) = &self.texture
        {
            if png_base64.len() > MAX_TEXTURE_BASE64 {
                return Err("the texture image is larger than 3 MB".into());
            }
            if !(1..=MAX_TEXTURE_SIDE).contains(width) || !(1..=MAX_TEXTURE_SIDE).contains(height) {
                return Err(format!(
                    "a texture image side must be 1 to {MAX_TEXTURE_SIDE} pixels"
                ));
            }
        }
        Ok(())
    }

    /// True when the material lets light through.
    pub fn is_transparent(&self) -> bool {
        self.opacity < 0.999
    }
}

/// Materials of the built-in library: name and definition. `material.add_defaults` adds the
/// ones that the document lacks. All textures are generated, so the library has no image files.
pub fn default_library() -> Vec<(&'static str, Material)> {
    fn m(
        color: [u8; 3],
        kind: Procedural,
        scale: [f64; 2],
        roughness: f32,
        opacity: f32,
    ) -> Material {
        Material {
            base_color: Color([color[0], color[1], color[2], 255]),
            texture: Some(Texture::Procedural { kind, seed: 1 }),
            mapping: TextureMapping {
                kind: MappingKind::Box,
                scale,
                ..Default::default()
            },
            roughness,
            metallic: 0.0,
            opacity,
        }
    }
    vec![
        (
            "Grass",
            m([96, 142, 62], Procedural::Grass, [800.0, 800.0], 0.95, 1.0),
        ),
        (
            "Gravel",
            m(
                [168, 160, 146],
                Procedural::Gravel,
                [600.0, 600.0],
                0.9,
                1.0,
            ),
        ),
        (
            "Concrete pavers",
            m(
                [188, 188, 182],
                Procedural::Pavers,
                [1200.0, 1200.0],
                0.85,
                1.0,
            ),
        ),
        (
            "Bark mulch",
            m([104, 72, 48], Procedural::Mulch, [700.0, 700.0], 0.95, 1.0),
        ),
        (
            "Water",
            m(
                [62, 128, 170],
                Procedural::Water,
                [2000.0, 2000.0],
                0.05,
                0.55,
            ),
        ),
        (
            "Brick",
            m([158, 78, 58], Procedural::Brick, [960.0, 960.0], 0.8, 1.0),
        ),
        (
            "Timber",
            m(
                [152, 108, 68],
                Procedural::Timber,
                [1200.0, 300.0],
                0.7,
                1.0,
            ),
        ),
    ]
}

// --- Render styles ----------------------------------------------------------------------

/// A render style: how a view or a viewport draws the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderStyle {
    pub mode: RenderMode,
    /// Draw the edges over shaded surfaces.
    pub edges: bool,
    /// Cast shadows from the sun.
    pub shadows: bool,
    /// Light the model with the sun of the document. Off: a fixed light from the upper left.
    pub sun: bool,
    /// Show textures. Off: flat base colors.
    pub textures: bool,
    pub background: Color,
    /// Line weight of hidden-line output, in mm on paper.
    pub line_weight: f64,
}

impl Default for RenderStyle {
    fn default() -> Self {
        RenderStyle {
            mode: RenderMode::Shaded,
            edges: true,
            shadows: false,
            sun: false,
            textures: true,
            background: Color([246, 247, 249, 255]),
            line_weight: 0.25,
        }
    }
}

impl RenderStyle {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.line_weight.is_finite() && (0.0..=20.0).contains(&self.line_weight)) {
            return Err("line weight must be from 0 to 20 mm".into());
        }
        Ok(())
    }

    /// The styles that every new document offers in the view bar (not stored in the document).
    pub fn presets() -> Vec<(&'static str, RenderStyle)> {
        let base = RenderStyle::default();
        vec![
            (
                "Wireframe",
                RenderStyle {
                    mode: RenderMode::Wireframe,
                    ..base.clone()
                },
            ),
            ("Shaded", base.clone()),
            (
                "Realistic",
                RenderStyle {
                    shadows: true,
                    sun: true,
                    ..base.clone()
                },
            ),
            (
                "Hidden Line",
                RenderStyle {
                    mode: RenderMode::HiddenLine,
                    background: Color([255, 255, 255, 255]),
                    ..base.clone()
                },
            ),
            (
                "Dashed Hidden Line",
                RenderStyle {
                    mode: RenderMode::DashedHiddenLine,
                    background: Color([255, 255, 255, 255]),
                    ..base
                },
            ),
        ]
    }
}

// --- Resource body ----------------------------------------------------------------------

/// The content of a material or render style resource.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderResource {
    Material(Material),
    RenderStyle(RenderStyle),
}

impl RenderResource {
    pub fn type_name(&self) -> &'static str {
        match self {
            RenderResource::Material(_) => "material",
            RenderResource::RenderStyle(_) => "render_style",
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            RenderResource::Material(m) => m.validate(),
            RenderResource::RenderStyle(s) => s.validate(),
        }
    }

    /// Reads the JSON of a resource. `None` if the `type` is not one of ours.
    /// `Err` if the type is ours and the content is wrong.
    pub fn from_json(v: &serde_json::Value) -> Option<Result<RenderResource, String>> {
        let parse = |r: Result<RenderResource, serde_json::Error>| r.map_err(|e| e.to_string());
        match v.get("type")?.as_str()? {
            "material" => Some(parse(
                serde_json::from_value(v.clone()).map(RenderResource::Material),
            )),
            "render_style" => Some(parse(
                serde_json::from_value(v.clone()).map(RenderResource::RenderStyle),
            )),
            _ => None,
        }
    }
}

impl Serialize for RenderResource {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (v, name) = match self {
            RenderResource::Material(m) => (serde_json::to_value(m), "material"),
            RenderResource::RenderStyle(r) => (serde_json::to_value(r), "render_style"),
        };
        let mut v = v.map_err(serde::ser::Error::custom)?;
        v["type"] = name.into();
        v.serialize(s)
    }
}

/// The material resource with this ID.
pub fn material(doc: &Document, id: ResourceId) -> Option<&Material> {
    match &doc.resources.get(id)?.body {
        ResourceBody::Render(r) => match r.as_ref() {
            RenderResource::Material(m) => Some(m),
            _ => None,
        },
        _ => None,
    }
}

/// The render style resource with this ID.
pub fn render_style(doc: &Document, id: ResourceId) -> Option<&RenderStyle> {
    match &doc.resources.get(id)?.body {
        ResourceBody::Render(r) => match r.as_ref() {
            RenderResource::RenderStyle(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

// --- The sun ----------------------------------------------------------------------------

/// The sun of the document (a heliodon): where on earth, which day, which time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SunSettings {
    /// Degrees north.
    pub latitude: f64,
    /// Degrees east.
    pub longitude: f64,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Local clock time in decimal hours.
    pub hour: f64,
    /// Offset of the local standard time from UTC in hours.
    pub tz_offset: f64,
    /// Summer time is in force: the clock is one hour ahead of the standard time.
    pub dst: bool,
    /// Angle in degrees, clockwise from the model +Y axis to true north (a drawing with north to the right has 90).
    pub north_angle: f64,
    /// True when latitude and longitude came from the georeference of the document.
    pub from_georef: bool,
}

impl Default for SunSettings {
    fn default() -> Self {
        SunSettings {
            latitude: 51.5,
            longitude: 0.0,
            year: 2026,
            month: 6,
            day: 21,
            hour: 12.0,
            tz_offset: 0.0,
            dst: false,
            north_angle: 0.0,
            from_georef: false,
        }
    }
}

impl SunSettings {
    pub fn sun_time(&self) -> SunTime {
        SunTime {
            latitude: self.latitude,
            longitude: self.longitude,
            year: self.year,
            month: self.month,
            day: self.day,
            hour: self.hour,
            tz_offset: self.tz_offset + f64::from(u8::from(self.dst)),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.north_angle.is_finite() {
            return Err("north angle must be finite".into());
        }
        sun::check(&self.sun_time())
    }

    pub fn position(&self) -> sun::SunPosition {
        sun::position(&self.sun_time())
    }

    /// Unit vector toward the sun in model axes, or `None` when the sun is below the horizon.
    pub fn light_direction(&self) -> Option<[f64; 3]> {
        let p = self.position();
        (p.elevation > 0.0).then(|| sun::direction(&p, self.north_angle))
    }
}

// --- Slots ------------------------------------------------------------------------------

/// A place that can take a material.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotTarget {
    /// One object. With a `slot` name, one part of it (`trunk`, `canopy`, `surface`).
    Object {
        object: ObjectId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot: Option<String>,
    },
    /// A part of a style. Plant style slots: `trunk`, `canopy`. Hardscape style slots:
    /// `surface` and `component:N` (N counts from 0 below the surface layer).
    Style { style: ResourceId, slot: String },
    /// All objects of a class.
    Class { class: ClassId },
}

/// One assignment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotEntry {
    pub target: SlotTarget,
    pub material: ResourceId,
}

/// The part of the document that links materials, sun and render style to the model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Presentation {
    /// The sun. `None`: the sun was never set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sun: Option<SunSettings>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<SlotEntry>,
    /// The render style that the 3D view and `render.png` use. `None`: the built-in shaded style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_style: Option<ResourceId>,
}

impl Presentation {
    /// Sets or clears (`None`) the material of a target.
    pub fn set_slot(&mut self, target: SlotTarget, material: Option<ResourceId>) {
        self.slots.retain(|e| e.target != target);
        if let Some(material) = material {
            self.slots.push(SlotEntry { target, material });
        }
    }

    pub fn slot(&self, target: &SlotTarget) -> Option<ResourceId> {
        self.slots
            .iter()
            .find(|e| &e.target == target)
            .map(|e| e.material)
    }

    /// Removes every use of a resource (after it was deleted).
    pub fn forget(&mut self, id: ResourceId) {
        self.slots.retain(|e| {
            e.material != id
                && !matches!(&e.target, SlotTarget::Style { style, .. } if *style == id)
        });
        if self.active_style == Some(id) {
            self.active_style = None;
        }
    }

    /// Removes the slots of an object that no longer exists.
    pub fn forget_object(&mut self, id: ObjectId) {
        self.slots
            .retain(|e| !matches!(&e.target, SlotTarget::Object { object, .. } if *object == id));
    }
}

/// The name of the slot that a triangle part has on an object (`trunk`, `surface`, ...).
pub fn part_slot(obj: &Object, part: u8) -> Option<String> {
    match (&obj.shape, part) {
        (Shape::Plant { .. } | Shape::ExistingTree { .. }, PART_TRUNK) => Some("trunk".into()),
        (Shape::Plant { .. } | Shape::ExistingTree { .. }, PART_CANOPY) => Some("canopy".into()),
        (Shape::Hardscape { .. }, p) if p >= PART_LAYER => Some(match p - PART_LAYER {
            0 => "surface".into(),
            i => format!("component:{}", i - 1),
        }),
        _ => None,
    }
}

/// The style resource of an object that has slots in its style (a plant style or hardscape style).
fn style_of(obj: &Object) -> Option<ResourceId> {
    crate::plants::link::style_of(obj).or_else(|| crate::hardscape::style_id(obj))
}

/// The mapping that an object forces on its material, if it does. A site model with the texture
/// look maps its texture from above in world units, with the tile size, offset and rotation of
/// its settings (3D-05).
pub fn mapping_override(obj: &Object) -> Option<TextureMapping> {
    match &obj.shape {
        Shape::SiteModel { settings, .. } if settings.surface_look == SurfaceLook::Texture => {
            Some(settings.texture_mapping())
        }
        _ => None,
    }
}

/// The slots of a document, indexed for fast lookup.
pub struct SlotIndex {
    objects: HashMap<(ObjectId, Option<String>), ResourceId>,
    styles: HashMap<(ResourceId, String), ResourceId>,
    classes: HashMap<ClassId, ResourceId>,
}

impl SlotIndex {
    pub fn new(doc: &Document) -> SlotIndex {
        let mut ix = SlotIndex {
            objects: HashMap::new(),
            styles: HashMap::new(),
            classes: HashMap::new(),
        };
        for e in &doc.drawing.presentation.slots {
            if material(doc, e.material).is_none() {
                continue;
            }
            match &e.target {
                SlotTarget::Object { object, slot } => {
                    ix.objects.insert((*object, slot.clone()), e.material);
                }
                SlotTarget::Style { style, slot } => {
                    ix.styles.insert((*style, slot.clone()), e.material);
                }
                SlotTarget::Class { class } => {
                    ix.classes.insert(*class, e.material);
                }
            }
        }
        // The texture of a site model with the texture look is a slot of the object.
        for obj in doc.objects.values() {
            if let Shape::SiteModel { settings, .. } = &obj.shape
                && settings.surface_look == SurfaceLook::Texture
                && let Some(m) = settings.texture
                && material(doc, m).is_some()
            {
                ix.objects.insert((obj.id, None), m);
            }
        }
        ix
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.styles.is_empty() && self.classes.is_empty()
    }

    /// The material for a part of an object. The order is: the part of the object, the whole
    /// object, the part of its style, its class. `None`: the object keeps its own color.
    pub fn resolve(&self, obj: &Object, part: u8) -> Option<ResourceId> {
        // Colors by elevation are vertex colors: no material covers them.
        if matches!(&obj.shape, Shape::SiteModel { settings, .. } if settings.surface_look == SurfaceLook::Elevation)
        {
            return None;
        }
        let slot = part_slot(obj, part);
        if let Some(s) = &slot
            && let Some(m) = self.objects.get(&(obj.id, Some(s.clone())))
        {
            return Some(*m);
        }
        if let Some(m) = self.objects.get(&(obj.id, None)) {
            return Some(*m);
        }
        if let (Some(s), Some(style)) = (slot, style_of(obj))
            && let Some(m) = self.styles.get(&(style, s))
        {
            return Some(*m);
        }
        self.classes.get(&obj.class).copied()
    }
}

#[cfg(test)]
mod tests;
