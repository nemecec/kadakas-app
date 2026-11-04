//! Markup on the view (REV-11, stage 1): ink on the live view.
//!
//! A [`Markup`] is review data, like an issue (REV-04). It is not a drawing object: it has no layer,
//! class or shape, it is not in the object list, and it is in no quantity, IFC or DXF export. The
//! document keeps markups in `Document::markups`.
//!
//! A markup holds the viewpoint it was drawn in ([`MarkupViewpoint`]: a plan view or a 3D camera,
//! the visible layers and classes, the size of the canvas in points) and a list of [`Mark`]s: pen
//! and highlighter strokes, arrows, rectangles, ellipses and text notes. The points of a mark are
//! relative to the view (0 to 1 of the width and of the height of the canvas, y down), so a mark
//! scales with the window. A markup can belong to an issue (`issue`).
//!
//! Stored data is read tolerantly. A field, a kind of mark or a whole markup that this version
//! does not know stays as it is and is written again unchanged (`extra` and
//! [`MarkKind::Other`], [`Markups`] keeps unreadable entries). Nothing is dropped silently.
//!
//! All numbers are finite, and all lists and texts have a bound (the `MAX_*` constants, chosen
//! from the measurements in `docs/modeler.md`). A command that would break a bound is refused with
//! a message.
//!
//! The commands are in `crate::command` (`AddMarkup`, `AddMark`, ...).

use std::collections::BTreeMap;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::change::Origin;
use crate::issue::{IssueId, ViewSpec};
use crate::model::{ClassId, LayerId};
use crate::uid::{Uid, id_type};

id_type!(
    /// Identifier of a markup.
    MarkupId
);

/// The most markups in a document.
pub const MAX_MARKUPS: usize = 2_000;
/// The most marks in one markup.
pub const MAX_MARKS: usize = 200;
/// The most points in one mark (a stroke). A longer stroke is cut into marks by the app.
pub const MAX_POINTS: usize = 1_000;
/// The most points in all marks of one markup.
pub const MAX_POINTS_PER_MARKUP: usize = 20_000;
/// The most points in all markups of a document.
pub const MAX_POINTS_TOTAL: usize = 400_000;
/// The longest text of a note mark, in characters.
pub const MAX_MARK_TEXT_CHARS: usize = 500;
/// The longest text note of a markup, in characters.
pub const MAX_NOTE_CHARS: usize = 20_000;
/// The longest name or author, in characters.
pub const MAX_NAME_CHARS: usize = 120;
/// The least width of a mark, in points of the canvas.
pub const MIN_WIDTH: f64 = 0.5;
/// The largest width of a mark, in points of the canvas.
pub const MAX_WIDTH: f64 = 64.0;
/// The default width of a pen stroke, in points.
pub const DEFAULT_WIDTH: f64 = 3.0;
/// The least distance between two points of a stroke, in points of the canvas (coalescing).
pub const MIN_POINT_DISTANCE: f64 = 1.5;
/// The smallest and the largest canvas side, in points.
pub const CANVAS_RANGE: (f64, f64) = (16.0, 20_000.0);
/// The digits kept for a relative coordinate: 1e-5 of the canvas is 0.01 point at 1000 points.
const DECIMALS: f64 = 100_000.0;
/// The opacity of a highlighter stroke, 0 to 1.
pub const HIGHLIGHT_ALPHA: f64 = 0.35;
/// The width of a highlighter stroke is this many times the width of a pen stroke of the same
/// setting.
pub const HIGHLIGHT_FACTOR: f64 = 4.0;

/// The palette of eight colours of the app. A mark can have any other colour too.
pub const PALETTE: [Rgb; 8] = [
    Rgb([0xd6, 0x28, 0x28]),
    Rgb([0xf7, 0x7f, 0x00]),
    Rgb([0xfc, 0xbf, 0x49]),
    Rgb([0x2a, 0x9d, 0x8f]),
    Rgb([0x1d, 0x4e, 0xd8]),
    Rgb([0x7b, 0x2c, 0xbf]),
    Rgb([0x11, 0x11, 0x11]),
    Rgb([0xff, 0xff, 0xff]),
];

/// A colour, written as `#rrggbb`. A list of three numbers is read too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }

    /// Reads `#rrggbb` or `rrggbb`.
    pub fn parse(text: &str) -> Option<Rgb> {
        let t = text.strip_prefix('#').unwrap_or(text);
        if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let n = u32::from_str_radix(t, 16).ok()?;
        Some(Rgb([(n >> 16) as u8, (n >> 8) as u8, n as u8]))
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Rgb, D::Error> {
        let bad = || {
            serde::de::Error::custom("a colour is #rrggbb or a list of three numbers from 0 to 255")
        };
        match Value::deserialize(d)? {
            Value::String(t) => Rgb::parse(&t).ok_or_else(bad),
            Value::Array(a) if a.len() == 3 => {
                let mut out = [0u8; 3];
                for (o, v) in out.iter_mut().zip(&a) {
                    *o = v
                        .as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or_else(bad)?;
                }
                Ok(Rgb(out))
            }
            _ => Err(bad()),
        }
    }
}

impl JsonSchema for Rgb {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Rgb".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "string", "pattern": "^#[0-9a-fA-F]{6}$"})
    }
}

/// What a mark is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkKind {
    /// A freehand stroke, 1 or more points.
    Pen,
    /// A wide, half transparent freehand stroke.
    Highlighter,
    /// An arrow from the tail (point 0) to the tip (point 1).
    Arrow,
    /// A rectangle, by two opposite corners.
    Rectangle,
    /// An ellipse, by the two opposite corners of its box.
    Ellipse,
    /// A text note at one point.
    Note,
    /// A kind that this version does not know (from a newer file). It stays as it is.
    Other(String),
}

impl MarkKind {
    pub const KNOWN: [MarkKind; 6] = [
        MarkKind::Pen,
        MarkKind::Highlighter,
        MarkKind::Arrow,
        MarkKind::Rectangle,
        MarkKind::Ellipse,
        MarkKind::Note,
    ];

    pub fn name(&self) -> &str {
        match self {
            MarkKind::Pen => "pen",
            MarkKind::Highlighter => "highlighter",
            MarkKind::Arrow => "arrow",
            MarkKind::Rectangle => "rectangle",
            MarkKind::Ellipse => "ellipse",
            MarkKind::Note => "note",
            MarkKind::Other(s) => s,
        }
    }

    pub fn from_name(text: &str) -> MarkKind {
        MarkKind::KNOWN
            .iter()
            .find(|k| k.name() == text)
            .cloned()
            .unwrap_or_else(|| MarkKind::Other(text.to_string()))
    }

    /// True for a kind this version draws.
    pub fn is_known(&self) -> bool {
        !matches!(self, MarkKind::Other(_))
    }

    /// True for a freehand stroke.
    pub fn is_stroke(&self) -> bool {
        matches!(self, MarkKind::Pen | MarkKind::Highlighter)
    }

    /// True for a shape that encloses an area when it is drawn (a rectangle or an ellipse).
    pub fn is_shape(&self) -> bool {
        matches!(self, MarkKind::Rectangle | MarkKind::Ellipse)
    }
}

impl Serialize for MarkKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for MarkKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<MarkKind, D::Error> {
        Ok(MarkKind::from_name(&String::deserialize(d)?))
    }
}

impl JsonSchema for MarkKind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "MarkKind".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "pen, highlighter, arrow, rectangle, ellipse or note. Another text is a kind of a newer version and stays as it is.",
        })
    }
}

/// One mark of a markup.
///
/// A mark that this version does not understand (an unknown kind, a colour or a point that does
/// not read) is kept as JSON ([`Mark::is_raw`]) and written again with the same content. It is
/// not drawn and not picked. A mark that reads but has a value out of range (a width of -5, a point
/// of 1e300, a text of 100000 characters) is kept as it is and is not drawn either
/// ([`Mark::drawable`]). The load notes say how many.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// The number of the mark in its markup. Numbers go up, so the order of creation shows. A
    /// number of 0 in a command means "make the next one".
    pub id: u32,
    pub kind: MarkKind,
    pub color: Rgb,
    /// The width of the line in points of the canvas the markup was drawn on.
    pub width: f64,
    /// Points relative to the view.
    pub points: Vec<[f64; 2]>,
    /// The text of a note.
    pub text: String,
    /// Fields that this version does not know. They are written again unchanged.
    pub extra: Map<String, Value>,
    /// The JSON of a mark that this version cannot read. When it is set, the other fields are
    /// empty and the mark is written as this value.
    raw: Option<Value>,
}

/// The form of a mark in a file and in a command.
#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "Mark")]
struct MarkRepr {
    /// The number of the mark in its markup. Numbers go up, so the order of creation shows. A
    /// number of 0 in a command means "make the next one".
    #[serde(default)]
    id: u32,
    kind: MarkKind,
    #[serde(default = "default_color")]
    color: Rgb,
    /// The width of the line in points of the canvas the markup was drawn on. A highlighter is
    /// drawn [`HIGHLIGHT_FACTOR`] times wider.
    #[serde(default = "default_width")]
    width: f64,
    /// Points relative to the view: x and y from 0 to 1, y down. A pen or highlighter stroke has
    /// any number of points. An arrow has two (tail, tip). A rectangle and an ellipse have two
    /// opposite corners. A note has one.
    points: Vec<[f64; 2]>,
    /// The text of a note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    text: String,
    /// Fields that this version does not know. They are written again unchanged.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    #[schemars(skip)]
    extra: Map<String, Value>,
}

/// A borrowed mark for writing: the same form as [`MarkRepr`].
#[derive(Serialize)]
struct MarkOut<'a> {
    id: u32,
    kind: &'a MarkKind,
    color: Rgb,
    width: f64,
    points: &'a [[f64; 2]],
    #[serde(skip_serializing_if = "str::is_empty")]
    text: &'a str,
    #[serde(flatten, skip_serializing_if = "Map::is_empty")]
    extra: &'a Map<String, Value>,
}

impl Serialize for Mark {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if let Some(raw) = &self.raw {
            return raw.serialize(s);
        }
        MarkOut {
            id: self.id,
            kind: &self.kind,
            color: self.color,
            width: self.width,
            points: &self.points,
            text: &self.text,
            extra: &self.extra,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Mark {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Mark, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(Mark::from_json(v))
    }
}

impl JsonSchema for Mark {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Mark".into()
    }

    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        MarkRepr::json_schema(g)
    }
}

fn default_color() -> Rgb {
    PALETTE[0]
}

fn default_width() -> f64 {
    DEFAULT_WIDTH
}

/// Which view the markup was drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MarkupMode {
    Plan,
    #[serde(rename = "3d")]
    View3d,
}

impl MarkupMode {
    pub fn name(self) -> &'static str {
        match self {
            MarkupMode::Plan => "plan",
            MarkupMode::View3d => "3d",
        }
    }
}

/// The viewpoint a markup was drawn in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarkupViewpoint {
    /// The plan view (`kind: plan`, center in mm, zoom in points per mm) or the 3D camera
    /// (`kind: camera`, target in drawing mm, yaw and pitch in radians, distance, perspective).
    pub view: ViewSpec,
    /// The vertical field of view of the 3D camera in degrees. Without it the default of the
    /// app (45) is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fov: Option<f64>,
    /// The size of the canvas the markup was drawn on, in points (width, height).
    pub canvas: [f64; 2],
    /// Visibility of layers when the markup was made (`true`: visible).
    #[serde(
        default,
        with = "crate::sheets::id_map",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    #[schemars(with = "Vec<(LayerId, bool)>")]
    pub layers: BTreeMap<LayerId, bool>,
    /// Visibility of classes when the markup was made.
    #[serde(
        default,
        with = "crate::sheets::id_map",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    #[schemars(with = "Vec<(ClassId, bool)>")]
    pub classes: BTreeMap<ClassId, bool>,
}

/// The default vertical field of view of the 3D camera, in degrees.
pub const DEFAULT_FOV_DEGREES: f64 = 45.0;

impl MarkupViewpoint {
    pub fn mode(&self) -> MarkupMode {
        match self.view {
            ViewSpec::Plan { .. } => MarkupMode::Plan,
            ViewSpec::Camera { .. } => MarkupMode::View3d,
        }
    }

    /// The field of view in degrees.
    pub fn fov_degrees(&self) -> f64 {
        self.fov.unwrap_or(DEFAULT_FOV_DEGREES)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.view.check()?;
        let (lo, hi) = CANVAS_RANGE;
        if !self
            .canvas
            .iter()
            .all(|c| c.is_finite() && (lo..=hi).contains(c))
        {
            return Err(format!("the canvas is {lo} to {hi} points on each side"));
        }
        if let Some(f) = self.fov
            && !(f.is_finite() && (1.0..=170.0).contains(&f))
        {
            return Err("the field of view is 1 to 170 degrees".into());
        }
        if self.layers.len() > crate::issue::MAX_VISIBILITY
            || self.classes.len() > crate::issue::MAX_VISIBILITY
        {
            return Err(format!(
                "a viewpoint lists at most {} layers and {} classes",
                crate::issue::MAX_VISIBILITY,
                crate::issue::MAX_VISIBILITY
            ));
        }
        Ok(())
    }
}

/// A markup: marks drawn on a view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Markup {
    /// The ID. A nil ID in a command means "make a new one".
    #[serde(default)]
    pub id: MarkupId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub author: String,
    /// Who made it: a person in the app or an agent.
    #[serde(default)]
    pub origin: Origin,
    /// Milliseconds since 1970 (UTC).
    #[serde(default)]
    pub time: i64,
    pub viewpoint: MarkupViewpoint,
    #[serde(default)]
    pub marks: Vec<Mark>,
    /// The issue (REV-04) that the markup belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<IssueId>,
    /// A text note for the whole markup.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// A short digest of the model when the markup was made ([`model_digest`]). A markup is not
    /// part of a version, so this tells when the model has changed since: the picture and what
    /// is under the marks then belong to an older model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Fields that this version does not know. They are written again unchanged.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// Rounds a relative coordinate to the stored precision.
pub fn round_rel(v: f64) -> f64 {
    (v * DECIMALS).round() / DECIMALS
}

/// A short digest of the model: the objects with their kind, layer and box, and the edit count
/// of each surface model. It is cheap (no object is written out) and it changes with an edit
/// of the model. It does not change with a markup, an issue or the visibility.
pub fn model_digest(doc: &crate::model::Document) -> String {
    use crate::model::Shape;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut put = |x: u64| {
        for b in x.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for o in doc.objects.values() {
        put(o.id.0.low_u64());
        put(o.layer.0.low_u64());
        put(o.shape.kind().len() as u64);
        let (lo, hi) = o.shape.bounds();
        for v in [lo.x, lo.y, hi.x, hi.y] {
            put(v.to_bits());
        }
        if let Shape::SurfaceModel(m) = &o.shape {
            put(m.rev());
        }
    }
    format!("{h:016x}")
}

/// A relative point rounded to the stored precision.
pub fn round_rel_point(p: [f64; 2]) -> [f64; 2] {
    [round_rel(p[0]), round_rel(p[1])]
}

/// A point of the canvas (in points from the top left corner) as a relative point (0 to 1, y
/// down). A point outside the canvas goes to the edge.
pub fn rel_of(canvas_point: [f64; 2], canvas: [f64; 2]) -> [f64; 2] {
    [
        round_rel((canvas_point[0] / canvas[0]).clamp(0.0, 1.0)),
        round_rel((canvas_point[1] / canvas[1]).clamp(0.0, 1.0)),
    ]
}

/// A relative point as a point of a canvas of this size, in points.
pub fn canvas_of(rel: [f64; 2], canvas: [f64; 2]) -> [f64; 2] {
    [rel[0] * canvas[0], rel[1] * canvas[1]]
}

/// Drops the points of a stroke that are less than `min_points` (points of the canvas) from the
/// point before them, and keeps the first and the last point. The points are relative.
pub fn coalesce(points: &[[f64; 2]], canvas: [f64; 2], min_points: f64) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len().min(256));
    let dist =
        |a: [f64; 2], b: [f64; 2]| ((a[0] - b[0]) * canvas[0]).hypot((a[1] - b[1]) * canvas[1]);
    for (i, p) in points.iter().enumerate() {
        match out.last() {
            None => out.push(*p),
            Some(last) => {
                if dist(*last, *p) >= min_points {
                    out.push(*p);
                } else if i + 1 == points.len() && *last != *p {
                    // The last point of a stroke stays: the stroke ends where the pen was lifted.
                    if out.len() > 1 {
                        out.pop();
                    }
                    out.push(*p);
                }
            }
        }
    }
    out
}

impl Mark {
    /// A new mark with the default colour and width.
    pub fn new(kind: MarkKind, points: Vec<[f64; 2]>) -> Mark {
        Mark {
            id: 0,
            kind,
            color: default_color(),
            width: DEFAULT_WIDTH,
            points,
            text: String::new(),
            extra: Map::new(),
            raw: None,
        }
    }

    /// Reads a mark from JSON. What does not read is kept as it is ([`Mark::is_raw`]).
    pub fn from_json(v: Value) -> Mark {
        match MarkRepr::deserialize(&v) {
            Ok(r) if r.kind.is_known() => Mark {
                id: r.id,
                kind: r.kind,
                color: r.color,
                width: r.width,
                points: r.points,
                text: r.text,
                extra: r.extra,
                raw: None,
            },
            _ => {
                let name = v
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let id = v
                    .get("id")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(0);
                Mark {
                    id,
                    kind: MarkKind::from_name(&name),
                    color: default_color(),
                    width: DEFAULT_WIDTH,
                    points: Vec::new(),
                    text: String::new(),
                    extra: Map::new(),
                    raw: Some(v),
                }
            }
        }
    }

    /// True if the mark is kept as JSON because this version cannot read it.
    pub fn is_raw(&self) -> bool {
        self.raw.is_some()
    }

    /// True if the mark can be drawn and picked: it reads, and its values are in range.
    pub fn drawable(&self) -> bool {
        self.raw.is_none() && self.kind.is_known() && self.validate().is_ok()
    }

    /// The box around the points, relative: `(lo, hi)`. `None` for a mark without points.
    pub fn bounds(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut it = self.points.iter();
        let first = *it.next()?;
        let (mut lo, mut hi) = (first, first);
        for p in it {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        Some((lo, hi))
    }

    /// True if a stroke ends near where it began (within `tol` of the canvas diagonal), so it
    /// encloses an area. The ends are compared in points of the canvas.
    pub fn is_closed(&self, canvas: [f64; 2]) -> bool {
        if self.kind.is_shape() {
            return true;
        }
        if !self.kind.is_stroke() || self.points.len() < 4 {
            return false;
        }
        let (a, b) = (self.points[0], self.points[self.points.len() - 1]);
        let gap = ((a[0] - b[0]) * canvas[0]).hypot((a[1] - b[1]) * canvas[1]);
        let (lo, hi) = self.bounds().unwrap_or((a, a));
        let size = ((hi[0] - lo[0]) * canvas[0]).hypot((hi[1] - lo[1]) * canvas[1]);
        gap <= (size * 0.2).max(8.0)
    }

    /// Checks the mark: the number of points for its kind, finite coordinates from 0 to 1, the
    /// width and the text. A mark that is kept as JSON is not checked.
    pub fn validate(&self) -> Result<(), String> {
        if self.raw.is_some() {
            return Ok(());
        }
        let name = self.kind.name();
        if chars(name) > MAX_NAME_CHARS {
            return Err("the kind of a mark is too long".into());
        }
        if self.points.len() > MAX_POINTS {
            return Err(format!(
                "a mark has at most {MAX_POINTS} points ({name} has {})",
                self.points.len()
            ));
        }
        for p in &self.points {
            if !p.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)) {
                return Err("the points of a mark are numbers from 0 to 1".into());
            }
        }
        let want = match self.kind {
            MarkKind::Arrow | MarkKind::Rectangle | MarkKind::Ellipse => Some(2),
            MarkKind::Note => Some(1),
            MarkKind::Pen | MarkKind::Highlighter | MarkKind::Other(_) => None,
        };
        if let Some(n) = want
            && self.points.len() != n
        {
            return Err(format!("{name} has exactly {n} point(s)"));
        }
        if self.kind.is_stroke() && self.points.is_empty() {
            return Err(format!("{name} needs at least one point"));
        }
        if !(self.width.is_finite() && (MIN_WIDTH..=MAX_WIDTH).contains(&self.width)) {
            return Err(format!(
                "the width of a mark is {MIN_WIDTH} to {MAX_WIDTH} points"
            ));
        }
        if chars(&self.text) > MAX_MARK_TEXT_CHARS {
            return Err(format!(
                "the text of a note has at most {MAX_MARK_TEXT_CHARS} characters"
            ));
        }
        if self.kind == MarkKind::Note && self.text.trim().is_empty() {
            return Err("a note needs a text".into());
        }
        Ok(())
    }
}

impl Markup {
    /// A new markup without marks.
    pub fn new(viewpoint: MarkupViewpoint) -> Markup {
        Markup {
            id: MarkupId::default(),
            name: None,
            author: String::new(),
            origin: Origin::User,
            time: 0,
            viewpoint,
            marks: Vec::new(),
            issue: None,
            note: String::new(),
            model: None,
            extra: Map::new(),
        }
    }

    /// The number of points in all marks.
    pub fn point_count(&self) -> usize {
        self.marks.iter().map(|m| m.points.len()).sum()
    }

    /// The mark with this number.
    pub fn mark(&self, id: u32) -> Option<&Mark> {
        self.marks.iter().find(|m| m.id == id)
    }

    /// The number a new mark gets: one above the highest.
    pub fn next_mark_id(&self) -> u32 {
        self.marks.iter().map(|m| m.id).max().unwrap_or(0) + 1
    }

    /// Checks the name, the author and the note only: what a rename or a new note changes.
    pub fn validate_meta(&self) -> Result<(), String> {
        if self
            .name
            .as_deref()
            .is_some_and(|n| chars(n) > MAX_NAME_CHARS)
        {
            return Err(format!("a name has at most {MAX_NAME_CHARS} characters"));
        }
        if chars(&self.author) > MAX_NAME_CHARS {
            return Err(format!("an author has at most {MAX_NAME_CHARS} characters"));
        }
        if chars(&self.note) > MAX_NOTE_CHARS {
            return Err(format!("a note has at most {MAX_NOTE_CHARS} characters"));
        }
        Ok(())
    }

    /// Checks the bounds and the values of a markup and its marks.
    pub fn validate(&self) -> Result<(), String> {
        if self
            .name
            .as_deref()
            .is_some_and(|n| chars(n) > MAX_NAME_CHARS)
        {
            return Err(format!("a name has at most {MAX_NAME_CHARS} characters"));
        }
        if chars(&self.author) > MAX_NAME_CHARS {
            return Err(format!("an author has at most {MAX_NAME_CHARS} characters"));
        }
        if chars(&self.note) > MAX_NOTE_CHARS {
            return Err(format!("a note has at most {MAX_NOTE_CHARS} characters"));
        }
        self.viewpoint.validate()?;
        if self.marks.len() > MAX_MARKS {
            return Err(format!("a markup has at most {MAX_MARKS} marks"));
        }
        if self.point_count() > MAX_POINTS_PER_MARKUP {
            return Err(format!(
                "a markup has at most {MAX_POINTS_PER_MARKUP} points in all"
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for m in &self.marks {
            m.validate()?;
            if m.id != 0 && !seen.insert(m.id) {
                return Err(format!("mark {} is there twice", m.id));
            }
        }
        Ok(())
    }

    /// Gives every mark without a number the next one, and makes the points of every mark the
    /// stored precision.
    pub fn prepare(&mut self) {
        let mut next = self.next_mark_id();
        for m in &mut self.marks {
            if m.id == 0 {
                m.id = next;
                next += 1;
            }
            for p in &mut m.points {
                *p = [round_rel(p[0]), round_rel(p[1])];
            }
        }
    }

    /// The box around all marks, relative.
    pub fn bounds(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut out: Option<([f64; 2], [f64; 2])> = None;
        for (lo, hi) in self.marks.iter().filter_map(Mark::bounds) {
            out = Some(match out {
                None => (lo, hi),
                Some((a, b)) => (
                    [a[0].min(lo[0]), a[1].min(lo[1])],
                    [b[0].max(hi[0]), b[1].max(hi[1])],
                ),
            });
        }
        out
    }
}

/// The changes of a markup that `ModifyMarkup` makes. A field that is left out stays.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarkupPatch {
    /// `Some("")` clears the name.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    /// `Some(None)` takes the markup off its issue.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<IssueId>")]
    pub issue: Option<Option<IssueId>>,
}

fn double_option<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

impl MarkupPatch {
    pub fn apply(&self, old: &Markup) -> Markup {
        let mut new = old.clone();
        if let Some(n) = &self.name {
            new.name = (!n.trim().is_empty()).then(|| n.trim().to_string());
        }
        if let Some(n) = &self.note {
            new.note = n.clone();
        }
        if let Some(i) = &self.issue {
            new.issue = *i;
        }
        new
    }
}

/// The markups of a document. Cloning is cheap (the data is shared), so an undo step keeps the
/// state before and after without copying the marks. A markup that this version cannot read is
/// kept as JSON and written again as it was.
#[derive(Clone, Debug, Default)]
pub struct Markups {
    list: Arc<Vec<Arc<Markup>>>,
    /// Entries that this version cannot read, with their place in the list of the file.
    unreadable: Arc<Vec<(usize, Value)>>,
}

impl PartialEq for Markups {
    fn eq(&self, other: &Self) -> bool {
        (Arc::ptr_eq(&self.list, &other.list) || self.list == other.list)
            && (Arc::ptr_eq(&self.unreadable, &other.unreadable)
                || self.unreadable == other.unreadable)
    }
}

impl Markups {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty() && self.unreadable.is_empty()
    }

    /// The number of readable markups.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Markup> {
        self.list.iter().map(|m| &**m)
    }

    pub fn get(&self, id: MarkupId) -> Option<&Markup> {
        self.list.iter().find(|m| m.id == id).map(|m| &**m)
    }

    pub fn position(&self, id: MarkupId) -> Option<usize> {
        self.list.iter().position(|m| m.id == id)
    }

    /// The entries that this version cannot read.
    pub fn unreadable(&self) -> impl Iterator<Item = &Value> {
        self.unreadable.iter().map(|(_, v)| v)
    }

    /// How many entries this version cannot read.
    pub fn unreadable_count(&self) -> usize {
        self.unreadable.len()
    }

    /// The marks, in all markups, that this version keeps as JSON or does not draw.
    pub fn undrawable_marks(&self) -> usize {
        self.list
            .iter()
            .flat_map(|m| m.marks.iter())
            .filter(|k| !k.drawable())
            .count()
    }

    /// The bytes that a step of the history holds when the markups change from `before` to
    /// `after`: the markups that are not shared by the two states, and the lists.
    pub fn step_bytes(before: &Markups, after: &Markups) -> u64 {
        let own = |m: &Markup| (m.point_count() * 16 + m.marks.len() * 160 + 512) as u64;
        let mut n = ((before.list.len() + after.list.len()) * 8) as u64;
        for m in after.list.iter() {
            let same = before.list.iter().any(|b| Arc::ptr_eq(b, m));
            if !same {
                n += own(m);
            }
        }
        for m in before.list.iter() {
            let same = after.list.iter().any(|a| Arc::ptr_eq(a, m));
            if !same {
                n += own(m);
            }
        }
        n
    }

    /// The points in all markups.
    pub fn point_count(&self) -> usize {
        self.list.iter().map(|m| m.point_count()).sum()
    }

    /// A rough size in bytes.
    pub fn approx_bytes(&self) -> u64 {
        (self.point_count() * 16 + self.list.len() * 512) as u64
    }

    /// Adds a markup at the end.
    pub(crate) fn push(&mut self, m: Markup) {
        Arc::make_mut(&mut self.list).push(Arc::new(m));
    }

    /// Replaces the markup at an index.
    pub(crate) fn set(&mut self, at: usize, m: Markup) {
        Arc::make_mut(&mut self.list)[at] = Arc::new(m);
    }

    pub(crate) fn remove(&mut self, at: usize) {
        Arc::make_mut(&mut self.list).remove(at);
    }

    /// Makes the IDs of the markups and of their marks unique and tells what it did. A document
    /// over a bound is not cut: the notes say so.
    pub fn repair(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        if !self.unreadable.is_empty() {
            notes.push(format!(
                "{} markup(s) in the file could not be read by this version. They are kept as they are",
                self.unreadable.len()
            ));
        }
        let undrawn = self.undrawable_marks();
        if undrawn > 0 {
            notes.push(format!(
                "{undrawn} mark(s) of markups cannot be drawn: this version does not know them or a value is out of range. They are kept as they are"
            ));
        }
        let mut seen = std::collections::HashSet::new();
        let mut fixed = false;
        for m in self.list.iter() {
            if m.id.0 == Uid::NIL || !seen.insert(m.id) {
                fixed = true;
            }
            let mut ids = std::collections::HashSet::new();
            if m.marks.iter().any(|k| k.id == 0 || !ids.insert(k.id)) {
                fixed = true;
            }
        }
        if fixed {
            let mut used = std::collections::HashSet::new();
            let list = Arc::make_mut(&mut self.list);
            for m in list.iter_mut() {
                let m = Arc::make_mut(m);
                if m.id.0 == Uid::NIL || !used.insert(m.id) {
                    let old = m.id.0;
                    m.id = (1..)
                        .map(|n| MarkupId(Uid::derive(old, 21, n)))
                        .find(|id| !used.contains(id))
                        .expect("a free ID");
                    used.insert(m.id);
                }
                let mut ids = std::collections::HashSet::new();
                let mut next = m.next_mark_id();
                for k in &mut m.marks {
                    if k.id == 0 || !ids.insert(k.id) {
                        k.id = next;
                        next += 1;
                        ids.insert(k.id);
                    }
                }
            }
            notes.push("markups with repeated or missing IDs got new IDs".into());
        }
        let points = self.point_count();
        if self.list.len() > MAX_MARKUPS || points > MAX_POINTS_TOTAL {
            notes.push(format!(
                "the document has {} markups with {points} points, over the bounds ({MAX_MARKUPS} markups, {MAX_POINTS_TOTAL} points). Nothing is cut",
                self.list.len()
            ));
        }
        notes
    }
}

impl Serialize for Markups {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let total = self.list.len() + self.unreadable.len();
        let mut seq = s.serialize_seq(Some(total))?;
        let mut readable = self.list.iter();
        let mut raw = self.unreadable.iter().peekable();
        for i in 0..total {
            match raw.peek() {
                Some((at, v)) if *at <= i => {
                    seq.serialize_element(v)?;
                    raw.next();
                }
                _ => match readable.next() {
                    Some(m) => seq.serialize_element(&**m)?,
                    None => {
                        if let Some((_, v)) = raw.next() {
                            seq.serialize_element(v)?;
                        }
                    }
                },
            }
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Markups {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Markups, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Markups;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a list of markups")
            }

            // One entry at a time: the text of an entry is held while it is read, and a
            // readable entry is not copied.
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Markups, A::Error> {
                let mut list = Vec::new();
                let mut unreadable = Vec::new();
                let mut at = 0usize;
                while let Some(v) = seq.next_element::<Value>()? {
                    match Markup::deserialize(&v) {
                        Ok(m) => list.push(Arc::new(m)),
                        Err(_) => unreadable.push((at, v)),
                    }
                    at += 1;
                }
                Ok(Markups {
                    list: Arc::new(list),
                    unreadable: Arc::new(unreadable),
                })
            }
        }
        d.deserialize_seq(V)
    }
}

impl JsonSchema for Markups {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Markups".into()
    }

    fn json_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <Vec<Markup>>::json_schema(g)
    }
}

pub mod draw;

#[cfg(test)]
mod tests;
