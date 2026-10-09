//! Document model: design layers, classes and drawing objects.

use std::collections::BTreeMap;
use std::fmt;

use glam::DVec2;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<$name> for u64 {
            fn from(id: $name) -> u64 {
                id.0
            }
        }
    };
}

id_type!(
    /// Identifier of an object. Objects, layers and classes share one ID space.
    ObjectId
);
id_type!(
    /// Identifier of a layer.
    LayerId
);
id_type!(
    /// Identifier of a class.
    ClassId
);

/// RGBA color, 8 bits per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub const BLACK: Color = Color([0, 0, 0, 255]);
    pub const NONE: Color = Color([0, 0, 0, 0]);

    pub fn to_css(self) -> String {
        let [r, g, b, a] = self.0;
        if a == 0 {
            "none".into()
        } else {
            format!("rgba({r},{g},{b},{:.3})", a as f64 / 255.0)
        }
    }
}

/// Graphic attributes. `None` means "use the class attribute".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Attributes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    /// Line weight in millimeters on paper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weight: Option<f64>,
}

impl Attributes {
    pub fn is_finite(&self) -> bool {
        self.line_weight.is_none_or(f64::is_finite)
    }
}

/// Geometry and parameters of an object. Coordinates are model units (mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Line { a: DVec2, b: DVec2 },
    Rect { min: DVec2, max: DVec2 },
    Circle { center: DVec2, radius: f64 },
    /// Counterclockwise arc. Angles are radians. A sweep of 2π or more is a full circle.
    Arc { center: DVec2, radius: f64, start: f64, end: f64 },
    Polyline { points: Vec<DVec2>, closed: bool },
    Text { at: DVec2, text: String, height: f64 },
    /// Plant instance. `spread` is the canopy diameter.
    Plant { at: DVec2, species: String, spread: f64, height: f64 },
    /// Planting area with a plant mix, bounded by a closed outline.
    LandscapeArea { boundary: Vec<DVec2>, name: String },
    /// Paved area (path, patio, drive).
    Hardscape { boundary: Vec<DVec2>, material: String },
}

/// Counterclockwise sweep of an arc in radians, from 0 (empty) to 2π (full circle).
/// An end angle below the start wraps around, so 270 degrees to 0 is a quarter turn.
pub fn arc_sweep(start: f64, end: f64) -> f64 {
    let d = end - start;
    if d.abs() >= std::f64::consts::TAU { std::f64::consts::TAU } else { d.rem_euclid(std::f64::consts::TAU) }
}

impl Shape {
    pub fn kind(&self) -> &'static str {
        match self {
            Shape::Line { .. } => "line",
            Shape::Rect { .. } => "rect",
            Shape::Circle { .. } => "circle",
            Shape::Arc { .. } => "arc",
            Shape::Polyline { .. } => "polyline",
            Shape::Text { .. } => "text",
            Shape::Plant { .. } => "plant",
            Shape::LandscapeArea { .. } => "landscape_area",
            Shape::Hardscape { .. } => "hardscape",
        }
    }

    /// True if every number in the shape is finite.
    pub fn is_finite(&self) -> bool {
        let v = |p: &DVec2| p.is_finite();
        match self {
            Shape::Line { a, b } => v(a) && v(b),
            Shape::Rect { min, max } => v(min) && v(max),
            Shape::Circle { center, radius } => v(center) && radius.is_finite(),
            Shape::Arc { center, radius, start, end } => v(center) && radius.is_finite() && start.is_finite() && end.is_finite(),
            Shape::Text { at, height, .. } => v(at) && height.is_finite(),
            Shape::Plant { at, spread, height, .. } => v(at) && spread.is_finite() && height.is_finite(),
            Shape::Polyline { points, .. }
            | Shape::LandscapeArea { boundary: points, .. }
            | Shape::Hardscape { boundary: points, .. } => points.iter().all(v),
        }
    }

    pub fn translate(&mut self, d: DVec2) {
        let mv = |p: &mut DVec2| *p += d;
        match self {
            Shape::Line { a, b } => {
                mv(a);
                mv(b);
            }
            Shape::Rect { min, max } => {
                mv(min);
                mv(max);
            }
            Shape::Circle { center, .. } | Shape::Arc { center, .. } => mv(center),
            Shape::Text { at, .. } | Shape::Plant { at, .. } => mv(at),
            Shape::Polyline { points, .. }
            | Shape::LandscapeArea { boundary: points, .. }
            | Shape::Hardscape { boundary: points, .. } => points.iter_mut().for_each(mv),
        }
    }

    /// Axis-aligned bounding box as (min, max).
    pub fn bounds(&self) -> (DVec2, DVec2) {
        let from_points = |pts: &[DVec2]| {
            pts.iter().fold(
                (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            )
        };
        match self {
            Shape::Line { a, b } => from_points(&[*a, *b]),
            Shape::Rect { min, max } => from_points(&[*min, *max]),
            Shape::Circle { center, radius } => (*center - DVec2::splat(*radius), *center + DVec2::splat(*radius)),
            Shape::Arc { center, radius, start, end } => {
                // The arc touches its endpoints, and each axis extreme that lies within the sweep.
                let sweep = arc_sweep(*start, *end);
                let mut pts = vec![
                    *center + DVec2::from_angle(*start) * *radius,
                    *center + DVec2::from_angle(*start + sweep) * *radius,
                ];
                for (k, axis) in [DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y].into_iter().enumerate() {
                    let angle = k as f64 * std::f64::consts::FRAC_PI_2;
                    if (angle - start).rem_euclid(std::f64::consts::TAU) <= sweep {
                        pts.push(*center + axis * *radius);
                    }
                }
                from_points(&pts)
            }
            Shape::Text { at, text, height } => {
                let w = *height * 0.6 * text.chars().count() as f64;
                (*at, *at + DVec2::new(w, *height))
            }
            Shape::Plant { at, spread, .. } => {
                let r = DVec2::splat(spread / 2.0);
                (*at - r, *at + r)
            }
            Shape::Polyline { points, .. }
            | Shape::LandscapeArea { boundary: points, .. }
            | Shape::Hardscape { boundary: points, .. } => from_points(points),
        }
    }

    /// Enclosed area in square model units, for closed shapes.
    pub fn area(&self) -> Option<f64> {
        match self {
            Shape::Rect { min, max } => Some(((max.x - min.x) * (max.y - min.y)).abs()),
            Shape::Circle { radius, .. } => Some(std::f64::consts::PI * radius * radius),
            Shape::Polyline { points, closed: true }
            | Shape::LandscapeArea { boundary: points, .. }
            | Shape::Hardscape { boundary: points, .. } => Some(crate::geom::polygon_area(points)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub layer: LayerId,
    pub class: ClassId,
    pub shape: Shape,
    #[serde(default)]
    pub attrs: Attributes,
    /// Record data, for example property sets. Keys are free-form.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, Value>,
    /// The group that this object belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ObjectId>,
}

impl Object {
    pub fn new(id: ObjectId, layer: LayerId, class: ClassId, shape: Shape) -> Self {
        Object { id, layer, class, shape, attrs: Attributes::default(), data: BTreeMap::new(), parent: None }
    }

    /// True if every number in the object is finite.
    pub fn is_finite(&self) -> bool {
        self.shape.is_finite() && self.attrs.is_finite()
    }
}

/// What a layer is for: a model layer, or a sheet (a page layout).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    #[default]
    Design,
    Sheet,
}

/// A design layer: a sheet of the model at an elevation, with its own scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    #[serde(default)]
    pub kind: LayerKind,
    pub visible: bool,
    pub locked: bool,
    /// Elevation of the layer base in mm.
    pub elevation: f64,
    /// Drawing scale denominator, for example 100 for 1:100.
    pub scale: f64,
}

/// A class: a second grouping axis for visibility and default attributes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Class {
    pub id: ClassId,
    pub name: String,
    pub visible: bool,
    pub attrs: Attributes,
}

/// Objects in z-order (first is at the bottom), found by ID in constant time.
/// In files they are a plain JSON array.
pub type Objects = IndexMap<ObjectId, Object>;

mod objects_as_array {
    use super::{Object, Objects};
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(map: &Objects, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(map.values())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Objects, D::Error> {
        let list = Vec::<Object>::deserialize(d)?;
        let mut map = Objects::with_capacity(list.len());
        for o in list {
            let id = o.id;
            if map.insert(id, o).is_some() {
                return Err(D::Error::custom(format!("duplicate id {id}")));
            }
        }
        Ok(map)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub format_version: u32,
    pub layers: Vec<Layer>,
    pub classes: Vec<Class>,
    #[serde(with = "objects_as_array")]
    pub objects: Objects,
    pub active_layer: LayerId,
    pub active_class: ClassId,
    #[serde(default)]
    next_id: u64,
}

/// Equality includes the z-order of the objects.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.format_version == other.format_version
            && self.layers == other.layers
            && self.classes == other.classes
            && self.active_layer == other.active_layer
            && self.active_class == other.active_class
            && self.next_id == other.next_id
            && self.objects.len() == other.objects.len()
            && self.objects.iter().eq(other.objects.iter())
    }
}

pub const FORMAT_VERSION: u32 = 1;
/// The class that every new document has. It cannot be deleted.
pub const NONE_CLASS: ClassId = ClassId(0);
/// The layer that every new document has.
pub const FIRST_LAYER: LayerId = LayerId(1);

/// Why a document could not be loaded.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LoadError {
    /// Not JSON, or not the shape of a document.
    #[error("not a Cadektor file: {0}")]
    Parse(String),
    #[error("file format {0} is newer than this app")]
    Newer(u32),
    /// The document breaks a rule, for example a duplicate ID.
    #[error("{0}")]
    Invalid(String),
}

/// Upgrades the JSON of an older format to the current one, in place.
/// Add one step per format version here when the format changes.
fn migrate(_doc: &mut Value, from: u32) -> Result<(), LoadError> {
    match from {
        0 => Err(LoadError::Invalid("unsupported format version 0".into())),
        _ => Ok(()),
    }
}

impl Default for Document {
    fn default() -> Self {
        Document {
            format_version: FORMAT_VERSION,
            layers: vec![Layer {
                id: FIRST_LAYER,
                name: "Layer 1".into(),
                kind: LayerKind::Design,
                visible: true,
                locked: false,
                elevation: 0.0,
                scale: 100.0,
            }],
            classes: vec![Class {
                id: NONE_CLASS,
                name: "None".into(),
                visible: true,
                attrs: Attributes {
                    pen: Some(Color::BLACK),
                    fill: Some(Color::NONE),
                    line_weight: Some(0.25),
                },
            }],
            objects: Objects::new(),
            active_layer: FIRST_LAYER,
            active_class: NONE_CLASS,
            next_id: 2,
        }
    }
}

impl Document {
    /// The one way to load a document: parses the JSON, refuses newer
    /// formats, migrates older ones, validates and repairs the ID counter.
    pub fn from_json(text: &str) -> Result<Document, LoadError> {
        let value = serde_json::from_str(text).map_err(|e| LoadError::Parse(e.to_string()))?;
        Document::from_value(value)
    }

    /// Like `from_json`, for JSON that is already parsed.
    pub fn from_value(mut value: Value) -> Result<Document, LoadError> {
        let version = value
            .get("format_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| LoadError::Parse("missing format_version".into()))?;
        let version = u32::try_from(version).unwrap_or(u32::MAX);
        if version > FORMAT_VERSION {
            return Err(LoadError::Newer(version));
        }
        migrate(&mut value, version)?;
        let mut doc: Document = serde_json::from_value(value).map_err(|e| LoadError::Parse(e.to_string()))?;
        doc.validate().map_err(LoadError::Invalid)?;
        Ok(doc)
    }

    /// Returns a new ID that is unique across objects, layers and classes.
    #[cfg(test)]
    pub(crate) fn set_next_id(&mut self, n: u64) {
        self.next_id = n;
    }

    /// Returns `None` when the IDs are used up.
    pub fn alloc_id(&mut self) -> Option<u64> {
        let id = self.next_id;
        self.next_id = id.checked_add(1)?;
        Some(id)
    }

    /// Checks a loaded document: IDs must be unique, and every object must
    /// refer to an existing layer and class. Also repairs the ID counter, so
    /// new objects never reuse an ID.
    pub fn validate(&mut self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        let ids = self
            .objects
            .keys()
            .map(|id| id.0)
            .chain(self.layers.iter().map(|l| l.id.0))
            .chain(self.classes.iter().map(|c| c.id.0));
        for id in ids {
            if !seen.insert(id) {
                return Err(format!("duplicate id {id}"));
            }
        }
        if self.layers.is_empty() {
            return Err("document has no layers".into());
        }
        if let Some(o) = self.objects.values().find(|o| self.layer(o.layer).is_none() || self.class(o.class).is_none()) {
            return Err(format!("object {} refers to a missing layer or class", o.id));
        }
        if let Some(o) = self.objects.values().find(|o| {
            !o.is_finite() || matches!(o.shape, Shape::Arc { start, end, .. } if arc_sweep(start, end) == 0.0)
        }) {
            return Err(format!("object {} has an invalid shape", o.id));
        }
        if self.layer(self.active_layer).is_none() || self.class(self.active_class).is_none() {
            return Err("active layer or class does not exist".into());
        }
        let max_id = seen.into_iter().max().unwrap_or(0);
        if max_id >= u64::MAX - 1 || self.next_id >= u64::MAX - 1 {
            return Err("ids are too large".into());
        }
        self.next_id = self.next_id.max(max_id + 1);
        Ok(())
    }

    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.get_mut(&id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn class(&self, id: ClassId) -> Option<&Class> {
        self.classes.iter().find(|c| c.id == id)
    }

    /// Effective attributes: object overrides, then class defaults.
    pub fn resolved_attrs(&self, obj: &Object) -> Attributes {
        let class = self.class(obj.class).map(|c| &c.attrs);
        Attributes {
            pen: obj.attrs.pen.or(class.and_then(|c| c.pen)),
            fill: obj.attrs.fill.or(class.and_then(|c| c.fill)),
            line_weight: obj.attrs.line_weight.or(class.and_then(|c| c.line_weight)),
        }
    }

    pub fn is_visible(&self, obj: &Object) -> bool {
        self.layer(obj.layer).is_some_and(|l| l.visible)
            && self.class(obj.class).is_some_and(|c| c.visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(shape: Shape) -> Object {
        Object::new(ObjectId(7), FIRST_LAYER, NONE_CLASS, shape)
    }

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn assert_vec_close(a: DVec2, b: DVec2) {
        assert!(a.distance(b) < 1e-9, "{a:?} != {b:?}");
    }

    #[test]
    fn color_to_css() {
        assert_eq!(Color::BLACK.to_css(), "rgba(0,0,0,1.000)");
        assert_eq!(Color([255, 128, 0, 128]).to_css(), "rgba(255,128,0,0.502)");
        assert_eq!(Color::NONE.to_css(), "none");
        // Fully transparent colors are "none" whatever their RGB value.
        assert_eq!(Color([10, 20, 30, 0]).to_css(), "none");
    }

    #[test]
    fn shape_kinds() {
        let cases = [
            (Shape::Line { a: v(0.0, 0.0), b: v(1.0, 1.0) }, "line"),
            (Shape::Rect { min: v(0.0, 0.0), max: v(1.0, 1.0) }, "rect"),
            (Shape::Circle { center: v(0.0, 0.0), radius: 1.0 }, "circle"),
            (Shape::Arc { center: v(0.0, 0.0), radius: 1.0, start: 0.0, end: 1.0 }, "arc"),
            (Shape::Polyline { points: vec![], closed: false }, "polyline"),
            (Shape::Text { at: v(0.0, 0.0), text: "t".into(), height: 1.0 }, "text"),
            (Shape::Plant { at: v(0.0, 0.0), species: "s".into(), spread: 1.0, height: 1.0 }, "plant"),
            (Shape::LandscapeArea { boundary: vec![], name: "n".into() }, "landscape_area"),
            (Shape::Hardscape { boundary: vec![], material: "m".into() }, "hardscape"),
        ];
        for (shape, kind) in cases {
            assert_eq!(shape.kind(), kind);
        }
    }

    #[test]
    fn translate_moves_every_point_of_every_shape() {
        let d = v(1.0, -2.0);
        let mut shapes = vec![
            Shape::Line { a: v(0.0, 0.0), b: v(1.0, 1.0) },
            Shape::Rect { min: v(0.0, 0.0), max: v(2.0, 2.0) },
            Shape::Circle { center: v(5.0, 5.0), radius: 1.0 },
            Shape::Arc { center: v(5.0, 5.0), radius: 1.0, start: 0.0, end: 1.0 },
            Shape::Polyline { points: vec![v(0.0, 0.0), v(1.0, 0.0)], closed: true },
            Shape::Text { at: v(3.0, 3.0), text: "x".into(), height: 2.0 },
            Shape::Plant { at: v(3.0, 3.0), species: "x".into(), spread: 2.0, height: 2.0 },
            Shape::LandscapeArea { boundary: vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)], name: "a".into() },
            Shape::Hardscape { boundary: vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)], material: "m".into() },
        ];
        for s in &mut shapes {
            let before = s.bounds();
            s.translate(d);
            let after = s.bounds();
            assert_vec_close(after.0, before.0 + d);
            assert_vec_close(after.1, before.1 + d);
        }
        let mut c = Shape::Circle { center: v(5.0, 5.0), radius: 1.0 };
        c.translate(d);
        assert_eq!(c, Shape::Circle { center: v(6.0, 3.0), radius: 1.0 });
        let mut r = Shape::Rect { min: v(0.0, 0.0), max: v(2.0, 2.0) };
        r.translate(d);
        assert_eq!(r, Shape::Rect { min: v(1.0, -2.0), max: v(3.0, 0.0) });
        let mut l = Shape::Line { a: v(0.0, 0.0), b: v(1.0, 1.0) };
        l.translate(d);
        assert_eq!(l, Shape::Line { a: v(1.0, -2.0), b: v(2.0, -1.0) });
        let mut t = Shape::Text { at: v(3.0, 3.0), text: "x".into(), height: 2.0 };
        t.translate(d);
        assert_eq!(t, Shape::Text { at: v(4.0, 1.0), text: "x".into(), height: 2.0 });
    }

    #[test]
    fn bounds_of_every_shape() {
        let (lo, hi) = Shape::Line { a: v(3.0, -1.0), b: v(-2.0, 5.0) }.bounds();
        assert_eq!((lo, hi), (v(-2.0, -1.0), v(3.0, 5.0)));

        // A rect with min above max still gives a normalized box.
        let (lo, hi) = Shape::Rect { min: v(5.0, 5.0), max: v(1.0, 2.0) }.bounds();
        assert_eq!((lo, hi), (v(1.0, 2.0), v(5.0, 5.0)));

        let (lo, hi) = Shape::Circle { center: v(1.0, 1.0), radius: 2.0 }.bounds();
        assert_eq!((lo, hi), (v(-1.0, -1.0), v(3.0, 3.0)));

        // An arc of 1 radian (less than a quarter turn) from angle 0 touches only the +X extreme.
        let (lo, hi) = Shape::Arc { center: v(0.0, 0.0), radius: 4.0, start: 0.0, end: 1.0 }.bounds();
        assert_vec_close(lo, v(4.0 * 1f64.cos(), 0.0));
        assert_vec_close(hi, v(4.0, 4.0 * 1f64.sin()));

        // Text width is 0.6 * height per character, counted in chars not bytes.
        let (lo, hi) = Shape::Text { at: v(1.0, 2.0), text: "héé".into(), height: 10.0 }.bounds();
        assert_vec_close(lo, v(1.0, 2.0));
        assert_vec_close(hi, v(1.0 + 18.0, 12.0));

        let (lo, hi) = Shape::Plant { at: v(10.0, 10.0), species: "Oak".into(), spread: 4.0, height: 8.0 }.bounds();
        assert_eq!((lo, hi), (v(8.0, 8.0), v(12.0, 12.0)));

        let poly = vec![v(0.0, 0.0), v(4.0, -1.0), v(2.0, 6.0)];
        let expected = (v(0.0, -1.0), v(4.0, 6.0));
        assert_eq!(Shape::Polyline { points: poly.clone(), closed: false }.bounds(), expected);
        assert_eq!(Shape::LandscapeArea { boundary: poly.clone(), name: "a".into() }.bounds(), expected);
        assert_eq!(Shape::Hardscape { boundary: poly, material: "m".into() }.bounds(), expected);
    }

    #[test]
    fn area_of_closed_shapes() {
        assert_eq!(Shape::Rect { min: v(0.0, 0.0), max: v(4.0, -3.0) }.area(), Some(12.0));
        let circle = Shape::Circle { center: v(0.0, 0.0), radius: 2.0 }.area().unwrap();
        assert!((circle - 4.0 * std::f64::consts::PI).abs() < 1e-9);

        let square = vec![v(0.0, 0.0), v(3.0, 0.0), v(3.0, 3.0), v(0.0, 3.0)];
        assert_eq!(Shape::Polyline { points: square.clone(), closed: true }.area(), Some(9.0));
        assert_eq!(Shape::Polyline { points: square.clone(), closed: false }.area(), None);
        assert_eq!(Shape::Hardscape { boundary: square.clone(), material: "m".into() }.area(), Some(9.0));

        let tri = vec![v(0.0, 0.0), v(4.0, 0.0), v(0.0, 3.0)];
        assert_eq!(Shape::LandscapeArea { boundary: tri, name: "t".into() }.area(), Some(6.0));

        let open_shapes = [
            Shape::Line { a: v(0.0, 0.0), b: v(1.0, 0.0) },
            Shape::Arc { center: v(0.0, 0.0), radius: 1.0, start: 0.0, end: 1.0 },
            Shape::Text { at: v(0.0, 0.0), text: "t".into(), height: 1.0 },
            Shape::Plant { at: v(0.0, 0.0), species: "s".into(), spread: 1.0, height: 1.0 },
        ];
        for s in open_shapes {
            assert_eq!(s.area(), None, "{}", s.kind());
        }
    }

    #[test]
    fn default_document_has_one_layer_and_none_class() {
        let doc = Document::default();
        assert_eq!(doc.format_version, FORMAT_VERSION);
        assert_eq!(doc.active_layer, FIRST_LAYER);
        assert_eq!(doc.active_class, NONE_CLASS);
        assert_eq!(doc.layer(FIRST_LAYER).unwrap().name, "Layer 1");
        assert_eq!(doc.class(NONE_CLASS).unwrap().name, "None");
        assert!(doc.layer(LayerId(99)).is_none());
        assert!(doc.class(ClassId(99)).is_none());
        assert!(doc.object(ObjectId(1)).is_none());
    }

    #[test]
    fn alloc_id_is_increasing_and_object_lookup_works() {
        let mut doc = Document::default();
        assert_eq!(doc.alloc_id(), Some(2));
        assert_eq!(doc.alloc_id(), Some(3));
        let o = obj(Shape::Circle { center: v(0.0, 0.0), radius: 1.0 });
        doc.objects.insert(o.id, o);
        assert_eq!(doc.object(ObjectId(7)).unwrap().id, ObjectId(7));
        doc.object_mut(ObjectId(7)).unwrap().layer = LayerId(9);
        assert_eq!(doc.object(ObjectId(7)).unwrap().layer, LayerId(9));
        assert!(doc.object_mut(ObjectId(8)).is_none());
    }

    #[test]
    fn resolved_attrs_falls_back_to_class_then_object_overrides() {
        let mut doc = Document::default();
        doc.classes.push(Class {
            id: ClassId(5),
            name: "Trees".into(),
            visible: true,
            attrs: Attributes { pen: Some(Color([0, 128, 0, 255])), fill: Some(Color([1, 2, 3, 255])), line_weight: Some(0.5) },
        });

        // No object overrides: every value comes from the class.
        let mut o = obj(Shape::Circle { center: v(0.0, 0.0), radius: 1.0 });
        o.class = ClassId(5);
        let a = doc.resolved_attrs(&o);
        assert_eq!(a.pen, Some(Color([0, 128, 0, 255])));
        assert_eq!(a.fill, Some(Color([1, 2, 3, 255])));
        assert_eq!(a.line_weight, Some(0.5));

        // Object overrides win per field; unset fields still come from the class.
        o.attrs = Attributes { pen: Some(Color::BLACK), fill: None, line_weight: Some(2.0) };
        let a = doc.resolved_attrs(&o);
        assert_eq!(a.pen, Some(Color::BLACK));
        assert_eq!(a.fill, Some(Color([1, 2, 3, 255])));
        assert_eq!(a.line_weight, Some(2.0));

        // Unknown class: only object values survive.
        o.class = ClassId(99);
        o.attrs = Attributes::default();
        assert_eq!(doc.resolved_attrs(&o), Attributes::default());
    }

    #[test]
    fn is_visible_respects_layer_and_class_visibility() {
        let mut doc = Document::default();
        let o = obj(Shape::Circle { center: v(0.0, 0.0), radius: 1.0 });
        assert!(doc.is_visible(&o));

        doc.layers[0].visible = false;
        assert!(!doc.is_visible(&o), "hidden layer hides its objects");
        doc.layers[0].visible = true;

        doc.classes[0].visible = false;
        assert!(!doc.is_visible(&o), "hidden class hides its objects");
        doc.classes[0].visible = true;

        let mut orphan = o.clone();
        orphan.layer = LayerId(42);
        assert!(!doc.is_visible(&orphan), "object on a missing layer is hidden");
        let mut orphan = o;
        orphan.class = ClassId(42);
        assert!(!doc.is_visible(&orphan), "object with a missing class is hidden");
    }

    // ---- regression tests for the review fixes ----

    #[test]
    fn arc_bounds_cover_endpoints_and_axis_extremes_inside_the_sweep() {
        let pi = std::f64::consts::PI;
        let arc = |start, end| Shape::Arc { center: v(1.0, 2.0), radius: 10.0, start, end }.bounds();
        // Half circle over the top: touches +X, +Y and -X.
        let (lo, hi) = arc(0.0, pi);
        assert_vec_close(lo, v(-9.0, 2.0));
        assert_vec_close(hi, v(11.0, 12.0));
        // Quarter turn in the upper right: only the two endpoints and no extra extremes.
        let (lo, hi) = arc(0.2, 1.2);
        assert_vec_close(lo, v(1.0 + 10.0 * 1.2f64.cos(), 2.0 + 10.0 * 0.2f64.sin()));
        assert_vec_close(hi, v(1.0 + 10.0 * 0.2f64.cos(), 2.0 + 10.0 * 1.2f64.sin()));
        // The sweep wraps around 0: from 300 to 60 degrees includes the +X extreme.
        let (lo, hi) = arc(5.0 * pi / 3.0, pi / 3.0);
        assert_vec_close(hi, v(11.0, 2.0 + 10.0 * (pi / 3.0).sin()));
        assert_vec_close(lo, v(1.0 + 5.0, 2.0 - 10.0 * (pi / 3.0).sin()));
        // Full circle (also written with a negative sweep) is the whole box.
        for (s, e) in [(0.3, 0.3 + 2.0 * pi), (0.3, 0.3 - 2.0 * pi), (0.0, 10.0 * pi)] {
            let (lo, hi) = arc(s, e);
            assert_vec_close(lo, v(-9.0, -8.0));
            assert_vec_close(hi, v(11.0, 12.0));
        }
    }

    #[test]
    fn arc_sweep_values() {
        let pi = std::f64::consts::PI;
        assert_eq!(arc_sweep(1.0, 1.0), 0.0);
        assert!((arc_sweep(0.0, pi) - pi).abs() < 1e-12);
        assert!((arc_sweep(1.5 * pi, 0.0) - 0.5 * pi).abs() < 1e-12);
        assert_eq!(arc_sweep(0.0, 2.0 * pi), std::f64::consts::TAU);
        assert_eq!(arc_sweep(0.0, -2.0 * pi), std::f64::consts::TAU);
        assert_eq!(arc_sweep(0.0, 100.0), std::f64::consts::TAU);
    }

    #[test]
    fn shape_is_finite_checks_every_number() {
        let nan = f64::NAN;
        let bad = [
            Shape::Line { a: v(nan, 0.0), b: v(0.0, 0.0) },
            Shape::Rect { min: v(0.0, 0.0), max: v(0.0, f64::INFINITY) },
            Shape::Circle { center: v(0.0, 0.0), radius: nan },
            Shape::Circle { center: v(nan, 0.0), radius: 1.0 },
            Shape::Arc { center: v(0.0, 0.0), radius: 1.0, start: nan, end: 1.0 },
            Shape::Arc { center: v(0.0, 0.0), radius: 1.0, start: 0.0, end: nan },
            Shape::Arc { center: v(0.0, 0.0), radius: nan, start: 0.0, end: 1.0 },
            Shape::Polyline { points: vec![v(0.0, 0.0), v(nan, 0.0)], closed: false },
            Shape::Text { at: v(0.0, 0.0), text: "t".into(), height: nan },
            Shape::Plant { at: v(0.0, 0.0), species: "s".into(), spread: 1.0, height: nan },
            Shape::Plant { at: v(0.0, 0.0), species: "s".into(), spread: nan, height: 1.0 },
            Shape::LandscapeArea { boundary: vec![v(nan, 0.0)], name: "n".into() },
            Shape::Hardscape { boundary: vec![v(0.0, nan)], material: "m".into() },
        ];
        for s in bad {
            assert!(!s.is_finite(), "{s:?}");
        }
        assert!(Shape::Circle { center: v(1e308, -1e308), radius: 1e308 }.is_finite());
        let mut o = obj(Shape::Circle { center: v(0.0, 0.0), radius: 1.0 });
        assert!(o.is_finite());
        o.attrs.line_weight = Some(nan);
        assert!(!o.is_finite());
    }

    fn doc_json() -> serde_json::Value {
        serde_json::to_value(Document::default()).unwrap()
    }

    #[test]
    fn from_json_loads_a_saved_document_and_ids_stay_plain_numbers() {
        let mut doc = Document::default();
        let o = obj(Shape::Circle { center: v(0.0, 0.0), radius: 1.0 });
        doc.objects.insert(o.id, o);
        doc.validate().unwrap();
        let text = serde_json::to_string(&doc).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["objects"][0]["id"], 7);
        assert_eq!(v["objects"][0]["layer"], 1);
        assert_eq!(v["active_layer"], 1);
        assert!(v["objects"][0].get("data").is_none(), "empty data and no parent are left out");
        assert_eq!(Document::from_json(&text).unwrap(), doc);
    }

    #[test]
    fn from_json_rejects_bad_input_with_a_clear_error() {
        assert!(matches!(Document::from_json("{nope"), Err(LoadError::Parse(_))));
        assert!(matches!(Document::from_json("[]"), Err(LoadError::Parse(m)) if m.contains("format_version")));
        assert!(matches!(Document::from_json(r#"{"format_version":1}"#), Err(LoadError::Parse(_))));

        let mut v = doc_json();
        v["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        let e = Document::from_value(v.clone()).unwrap_err();
        assert_eq!(e, LoadError::Newer(FORMAT_VERSION + 1));
        assert_eq!(e.to_string(), format!("file format {} is newer than this app", FORMAT_VERSION + 1));
        // A newer file may have a different shape. It is refused before the shape is read.
        assert_eq!(Document::from_json(r#"{"format_version":99,"layers":"???"}"#), Err(LoadError::Newer(99)));
        assert_eq!(Document::from_json(r#"{"format_version":99999999999}"#), Err(LoadError::Newer(u32::MAX)));

        v["format_version"] = serde_json::json!(0);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains("version 0")));

        let mut v = doc_json();
        v["objects"] = serde_json::json!([{"id": 5, "layer": 99, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains("missing layer")));
        let mut v = doc_json();
        v["active_layer"] = serde_json::json!(77);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(_))));
        let mut v = doc_json();
        v["layers"] = serde_json::json!([]);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains("no layers")));
    }

    #[test]
    fn from_json_rejects_duplicate_ids_and_bad_shapes() {
        let circle = |id: u64| serde_json::json!({"id": id, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}});
        let mut v = doc_json();
        v["objects"] = serde_json::json!([circle(5), circle(5)]);
        assert!(matches!(Document::from_value(v), Err(LoadError::Parse(m)) if m.contains("duplicate id 5")));
        // An object may not reuse the ID of a layer.
        let mut v = doc_json();
        v["objects"] = serde_json::json!([circle(1)]);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains("duplicate id 1")));

        let mut v = doc_json();
        v["objects"] = serde_json::json!([{"id": 5, "layer": 1, "class": 0, "shape": {"kind": "arc", "center": [0, 0], "radius": 1, "start": 1, "end": 1}}]);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains("object 5 has an invalid shape")));
    }

    #[test]
    fn huge_ids_are_rejected_and_alloc_never_overflows() {
        let mut v = doc_json();
        v["next_id"] = serde_json::json!(u64::MAX);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(_))));
        let circle = |id: u64| serde_json::json!({"id": id, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}});
        let mut v = doc_json();
        v["objects"] = serde_json::json!([circle(u64::MAX)]);
        assert!(matches!(Document::from_value(v), Err(LoadError::Invalid(_))));
        let mut doc = Document { next_id: u64::MAX, ..Default::default() };
        assert_eq!(doc.alloc_id(), None);
    }

    #[test]
    fn missing_next_id_is_repaired_and_never_collides() {
        let mut v = doc_json();
        v.as_object_mut().unwrap().remove("next_id");
        v["objects"] = serde_json::json!([{"id": 40, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        let mut doc = Document::from_value(v).unwrap();
        assert_eq!(doc.alloc_id(), Some(41));
        // A stale counter is repaired the same way.
        let mut v = doc_json();
        v["next_id"] = serde_json::json!(2);
        v["objects"] = serde_json::json!([{"id": 2, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        assert_eq!(Document::from_value(v).unwrap().alloc_id(), Some(3));
    }

    #[test]
    fn old_files_without_new_fields_load_with_defaults() {
        let mut v = doc_json();
        v["layers"][0].as_object_mut().unwrap().remove("kind");
        v["objects"] = serde_json::json!([{"id": 5, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        let doc = Document::from_value(v).unwrap();
        assert_eq!(doc.layers[0].kind, LayerKind::Design);
        let o = doc.object(ObjectId(5)).unwrap();
        assert!(o.data.is_empty() && o.parent.is_none());
    }

    #[test]
    fn data_parent_and_layer_kind_round_trip() {
        let mut doc = Document::default();
        doc.layers[0].kind = LayerKind::Sheet;
        let mut o = obj(Shape::Circle { center: v(0.0, 0.0), radius: 1.0 });
        o.data.insert("Pset_Wall".into(), serde_json::json!({"FireRating": "REI60", "Load": 2.5}));
        o.parent = Some(ObjectId(99));
        doc.objects.insert(o.id, o);
        doc.validate().unwrap();
        let text = serde_json::to_string(&doc).unwrap();
        assert!(text.contains(r#""kind":"sheet""#) && text.contains("Pset_Wall") && text.contains(r#""parent":99"#));
        assert_eq!(Document::from_json(&text).unwrap(), doc);
    }

    #[test]
    fn document_equality_includes_z_order() {
        let mut a = Document::default();
        let mk = |id| Object::new(ObjectId(id), FIRST_LAYER, NONE_CLASS, Shape::Circle { center: v(0.0, 0.0), radius: 1.0 });
        a.objects.insert(ObjectId(10), mk(10));
        a.objects.insert(ObjectId(11), mk(11));
        let mut b = Document::default();
        b.objects.insert(ObjectId(11), mk(11));
        b.objects.insert(ObjectId(10), mk(10));
        assert_ne!(a, b);
        b.objects.move_index(1, 0);
        assert_eq!(a, b);
        b.objects.pop();
        assert_ne!(a, b);
    }
}
