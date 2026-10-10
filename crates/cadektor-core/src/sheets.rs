//! Drawing documentation: sheet layers, viewports, annotations, title blocks and saved views.
//!
//! A sheet layer is a page. Its objects live in paper space: millimeters on paper, scale 1:1.
//! A viewport is an object on a sheet layer. It shows design layers at a scale.
//!
//! Attribute precedence inside a viewport (first match wins), see `Viewport::resolve`.
//! As in Vectorworks, a class override acts on attributes that are "by class" only:
//! 1. the attributes that the object sets itself (they are not by class),
//! 2. the viewport class override of the class of the object (a field that it sets),
//! 3. the attributes of the class of the object,
//! 4. the defaults (black pen, no fill, 0.25 mm).
//!
//! Visibility inside a viewport does not follow the document. The API stores the visibility of
//! every design layer and class in the viewport when it creates it (`snapshot_visibility`).
//! Later changes of the document visibility do not reach the viewport. A layer or class
//! that the viewport has no entry for (added later) is visible.

use std::collections::BTreeMap;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::model::{Attributes, ClassId, Document, LayerId, LayerKind, Object, Shape};

/// Pen, fill and weight used when neither the object nor its class sets them.
pub const DEFAULT_WEIGHT: f64 = 0.25;
/// Largest number of annotations in one viewport.
pub const MAX_ANNOTATIONS: usize = 10_000;
/// Most 3D (raster) viewports on one sheet layer. Each one is a rendered image.
pub const MAX_3D_VIEWPORTS: usize = 8;
/// Largest size of a sheet in mm (about 10 m).
pub const MAX_PAPER: f64 = 10_000.0;

// ---------------------------------------------------------------- sheets

/// A paper size. Sizes are given for portrait.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperSize {
    A0,
    A1,
    A2,
    A3,
    A4,
    Letter,
    Legal,
    Tabloid,
    ArchC,
    ArchD,
    ArchE,
    Custom { width_mm: f64, height_mm: f64 },
}

impl PaperSize {
    pub const NAMED: [PaperSize; 11] = [
        PaperSize::A0,
        PaperSize::A1,
        PaperSize::A2,
        PaperSize::A3,
        PaperSize::A4,
        PaperSize::Letter,
        PaperSize::Legal,
        PaperSize::Tabloid,
        PaperSize::ArchC,
        PaperSize::ArchD,
        PaperSize::ArchE,
    ];

    /// Width and height in mm for a portrait page.
    pub fn portrait_mm(self) -> DVec2 {
        let (w, h) = match self {
            PaperSize::A0 => (841.0, 1189.0),
            PaperSize::A1 => (594.0, 841.0),
            PaperSize::A2 => (420.0, 594.0),
            PaperSize::A3 => (297.0, 420.0),
            PaperSize::A4 => (210.0, 297.0),
            PaperSize::Letter => (215.9, 279.4),
            PaperSize::Legal => (215.9, 355.6),
            PaperSize::Tabloid => (279.4, 431.8),
            PaperSize::ArchC => (457.2, 609.6),
            PaperSize::ArchD => (609.6, 914.4),
            PaperSize::ArchE => (914.4, 1219.2),
            PaperSize::Custom { width_mm, height_mm } => (width_mm, height_mm),
        };
        DVec2::new(w, h)
    }

    pub fn id(self) -> &'static str {
        match self {
            PaperSize::A0 => "a0",
            PaperSize::A1 => "a1",
            PaperSize::A2 => "a2",
            PaperSize::A3 => "a3",
            PaperSize::A4 => "a4",
            PaperSize::Letter => "letter",
            PaperSize::Legal => "legal",
            PaperSize::Tabloid => "tabloid",
            PaperSize::ArchC => "arch_c",
            PaperSize::ArchD => "arch_d",
            PaperSize::ArchE => "arch_e",
            PaperSize::Custom { .. } => "custom",
        }
    }

    /// A named size from its id, ignoring case. `Custom` has no name.
    pub fn from_id(id: &str) -> Option<PaperSize> {
        let id = id.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        PaperSize::NAMED.into_iter().find(|p| p.id() == id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

impl Orientation {
    pub fn from_id(id: &str) -> Option<Orientation> {
        match id.trim().to_ascii_lowercase().as_str() {
            "landscape" => Some(Orientation::Landscape),
            "portrait" => Some(Orientation::Portrait),
            _ => None,
        }
    }
}

/// Page settings of one sheet layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub paper: PaperSize,
    #[serde(default)]
    pub orientation: Orientation,
    /// The sheet number, for example "L-101".
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub title: String,
}

impl Sheet {
    pub fn new(number: impl Into<String>, title: impl Into<String>) -> Sheet {
        Sheet { paper: PaperSize::A3, orientation: Orientation::Landscape, number: number.into(), title: title.into() }
    }

    /// Page size in mm, with the orientation applied.
    pub fn size_mm(&self) -> DVec2 {
        let p = self.paper.portrait_mm();
        let (short, long) = (p.x.min(p.y), p.x.max(p.y));
        match self.orientation {
            Orientation::Landscape => DVec2::new(long, short),
            Orientation::Portrait => DVec2::new(short, long),
        }
    }

    pub fn is_valid(&self) -> bool {
        let s = self.paper.portrait_mm();
        s.is_finite() && s.min_element() >= 10.0 && s.max_element() <= MAX_PAPER
    }
}

/// Project data for title blocks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub drawn_by: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub revision: String,
}

/// A stored view: what is visible, which layer is active, and the camera.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub name: String,
    /// Visibility of design layers and sheet layers. Layers that are not listed stay as they are.
    #[serde(default, with = "id_map")]
    pub layers: BTreeMap<LayerId, bool>,
    /// Visibility of classes. Classes that are not listed stay as they are.
    #[serde(default, with = "id_map")]
    pub classes: BTreeMap<ClassId, bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_layer: Option<LayerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_class: Option<ClassId>,
    /// Pixels per mm of the 2D view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    /// The model point at the middle of the 2D view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<DVec2>,
    /// A standard view id (`view.list`). `plan` is the 2D drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

impl SavedView {
    pub fn is_finite(&self) -> bool {
        self.zoom.is_none_or(|z| z.is_finite() && z > 0.0) && self.pan.is_none_or(|p| p.is_finite())
    }
}

/// Maps with ID keys, saved as lists of `[id, value]` pairs, so that every JSON reader accepts them.
pub mod id_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<K: Serialize + Ord, V: Serialize, S: Serializer>(m: &BTreeMap<K, V>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(m.iter())
    }

    pub fn deserialize<'de, K: Deserialize<'de> + Ord, V: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<K, V>, D::Error> {
        Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
    }
}

/// Drawing documentation data of a document. Older files have none of it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    #[serde(default, with = "id_map")]
    pub sheets: BTreeMap<LayerId, Sheet>,
    #[serde(default)]
    pub project: ProjectInfo,
    #[serde(default)]
    pub saved_views: Vec<SavedView>,
}

impl Drawing {
    pub fn is_empty(&self) -> bool {
        *self == Drawing::default()
    }

    pub fn saved_view(&self, name: &str) -> Option<&SavedView> {
        self.saved_views.iter().find(|v| v.name.eq_ignore_ascii_case(name.trim()))
    }
}

impl Document {
    /// Page settings of a sheet layer. A sheet layer without data gets the defaults.
    pub fn sheet(&self, layer: LayerId) -> Option<Sheet> {
        let l = self.layer(layer)?;
        if l.kind != LayerKind::Sheet {
            return None;
        }
        Some(self.drawing.sheets.get(&layer).cloned().unwrap_or_else(|| Sheet::new("", l.name.clone())))
    }

    /// The sheet layers in document order.
    pub fn sheet_layers(&self) -> impl Iterator<Item = &crate::model::Layer> {
        self.layers.iter().filter(|l| l.kind == LayerKind::Sheet)
    }

    /// The viewports of a sheet layer, in z-order.
    pub fn viewports_of(&self, layer: LayerId) -> impl Iterator<Item = (&Object, &Viewport)> {
        self.objects.values().filter(move |o| o.layer == layer).filter_map(|o| match &o.shape {
            Shape::Viewport(v) => Some((o, &**v)),
            _ => None,
        })
    }
}

// ---------------------------------------------------------------- viewports

/// The part of a viewport that is shown. A crop smaller than the frame hides the rest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Crop {
    /// A rectangle in paper coordinates.
    Rect { min: DVec2, max: DVec2 },
    /// A polygon in paper coordinates (3 or more points, not self-crossing).
    Polygon { points: Vec<DVec2> },
}

/// Changes of a class inside one viewport. A field that is `None` changes nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClassOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<crate::model::Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<crate::model::Color>,
    /// Line weight on paper in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weight: Option<f64>,
}

impl ClassOverride {
    pub fn is_finite(&self) -> bool {
        self.line_weight.is_none_or(|w| w.is_finite() && w >= 0.0)
    }
}

/// One item in the annotation space of a viewport. Positions are in paper mm,
/// measured from the lower left corner of the viewport frame, so the annotations move with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: u32,
    #[serde(default)]
    pub class: ClassId,
    #[serde(default)]
    pub attrs: Attributes,
    #[serde(flatten)]
    pub item: AnnotationItem,
}

/// What an annotation draws. `Shape` holds any 2D shape of the document (line, rect, circle,
/// arc, polyline, text), in paper units, so new annotation tools need no new container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationItem {
    Line { a: DVec2, b: DVec2 },
    Text { at: DVec2, text: String, height: f64 },
    /// A linear dimension between `a` and `b`. The dimension line is `offset` mm to the left of a to b.
    /// The text is the distance in model mm (paper length times the viewport scale) unless `text` is set.
    Dimension {
        a: DVec2,
        b: DVec2,
        #[serde(default)]
        offset: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Shape { shape: Box<Shape> },
}

impl Annotation {
    pub fn is_finite(&self) -> bool {
        self.attrs.is_finite()
            && match &self.item {
                AnnotationItem::Line { a, b } => a.is_finite() && b.is_finite(),
                AnnotationItem::Text { at, height, .. } => at.is_finite() && height.is_finite() && *height > 0.0,
                AnnotationItem::Dimension { a, b, offset, .. } => a.is_finite() && b.is_finite() && offset.is_finite(),
                AnnotationItem::Shape { shape } => shape.is_finite() && shape.is_in_range() && !matches!(**shape, Shape::Viewport(_) | Shape::TitleBlock(_)),
            }
    }
}

/// A view of design layers on a sheet layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    #[serde(default)]
    pub name: String,
    /// The frame on the sheet, in paper mm.
    pub min: DVec2,
    pub max: DVec2,
    /// The model point (mm) that shows in the middle of the frame.
    pub center: DVec2,
    /// Scale denominator: 100 means 1:100.
    pub scale: f64,
    /// The design layers that the viewport shows.
    #[serde(default)]
    pub layers: Vec<LayerId>,
    /// `plan` (Top/Plan, vector) or a 3D standard view id such as `right_isometric` (raster image).
    #[serde(default = "default_view")]
    pub view: String,
    /// Render mode of 3D views: `shaded` or `wireframe`.
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Resolution of 3D views, in dots per inch.
    #[serde(default = "default_dpi")]
    pub dpi: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
    /// Visibility of a layer in this viewport. It replaces the document setting.
    #[serde(default, with = "id_map")]
    pub layer_overrides: BTreeMap<LayerId, bool>,
    /// Visibility and graphics of a class in this viewport.
    #[serde(default, with = "id_map")]
    pub class_overrides: BTreeMap<ClassId, ClassOverride>,
    /// Draw a frame around the visible area.
    #[serde(default)]
    pub show_frame: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

pub const PLAN_VIEW: &str = "plan";

fn default_view() -> String {
    PLAN_VIEW.into()
}

fn default_mode() -> String {
    "shaded".into()
}

/// Default resolution of 3D views in viewports.
pub const DEFAULT_DPI: f64 = 150.0;

fn default_dpi() -> f64 {
    DEFAULT_DPI
}

impl Viewport {
    /// A plan viewport with a frame, a scale and layers.
    pub fn new(min: DVec2, max: DVec2, center: DVec2, scale: f64, layers: Vec<LayerId>) -> Viewport {
        Viewport {
            name: String::new(),
            min,
            max,
            center,
            scale,
            layers,
            view: default_view(),
            mode: default_mode(),
            dpi: DEFAULT_DPI,
            crop: None,
            layer_overrides: BTreeMap::new(),
            class_overrides: BTreeMap::new(),
            show_frame: false,
            annotations: Vec::new(),
        }
    }

    pub fn is_plan(&self) -> bool {
        self.view == PLAN_VIEW
    }

    /// Why the viewport is not valid, or `None`.
    pub fn problem(&self) -> Option<String> {
        let finite = self.min.is_finite() && self.max.is_finite() && self.center.is_finite() && self.scale.is_finite() && self.dpi.is_finite();
        if !finite || !self.annotations.iter().all(Annotation::is_finite) {
            return Some("numbers must be finite".into());
        }
        if self.min.x >= self.max.x || self.min.y >= self.max.y {
            return Some("the viewport frame needs min below max".into());
        }
        if self.max.max_element() > MAX_PAPER || self.min.min_element() < -MAX_PAPER {
            return Some("the viewport is too large".into());
        }
        if self.scale <= 0.0 || self.scale > 1.0e6 {
            return Some("viewport scale must be positive (a denominator such as 100)".into());
        }
        if self.center.abs().max_element() > crate::model::MAX_COORD {
            return Some("numbers must not be larger than 1e9 mm".into());
        }
        if !(30.0..=1200.0).contains(&self.dpi) {
            return Some("dpi must be 30 to 1200".into());
        }
        if !matches!(self.mode.as_str(), "shaded" | "wireframe") {
            return Some("mode must be shaded or wireframe".into());
        }
        if self.view.trim().is_empty() {
            return Some("view must not be empty".into());
        }
        match &self.crop {
            Some(Crop::Polygon { points }) if points.len() < 3 => return Some("a polygon crop needs 3 or more points".into()),
            Some(Crop::Polygon { points }) if !points.iter().all(|p| p.is_finite()) => return Some("numbers must be finite".into()),
            Some(Crop::Rect { min, max }) if !(min.is_finite() && max.is_finite()) => return Some("numbers must be finite".into()),
            _ => {}
        }
        if self.class_overrides.values().any(|c| !c.is_finite()) {
            return Some("line weights must be zero or more".into());
        }
        if self.annotations.len() > MAX_ANNOTATIONS {
            return Some("too many annotations".into());
        }
        let mut ids: Vec<u32> = self.annotations.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Some("annotation ids must be unique".into());
        }
        None
    }

    pub fn frame_center(&self) -> DVec2 {
        (self.min + self.max) / 2.0
    }

    /// Paper position of a model point.
    pub fn to_paper(&self, model: DVec2) -> DVec2 {
        self.frame_center() + (model - self.center) / self.scale
    }

    /// Model position of a paper point. The inverse of `to_paper`.
    pub fn to_model(&self, paper: DVec2) -> DVec2 {
        self.center + (paper - self.frame_center()) * self.scale
    }

    /// The area of the model that the frame shows, as (min, max).
    pub fn model_window(&self) -> (DVec2, DVec2) {
        let (a, b) = (self.to_model(self.min), self.to_model(self.max));
        (a.min(b), a.max(b))
    }

    /// The polygon (counterclockwise, paper mm) that limits what shows: the frame, cut by the crop.
    pub fn clip_polygon(&self) -> Vec<DVec2> {
        match &self.crop {
            None => rect_polygon(self.min, self.max),
            Some(Crop::Rect { min, max }) => {
                let (lo, hi) = (min.min(*max).max(self.min), min.max(*max).min(self.max));
                if lo.x >= hi.x || lo.y >= hi.y { Vec::new() } else { rect_polygon(lo, hi) }
            }
            Some(Crop::Polygon { points }) => clip_polygon_to_rect(&ccw(points), self.min, self.max),
        }
    }

    /// True if the paper point is inside the visible area.
    pub fn shows_paper(&self, p: DVec2) -> bool {
        point_in_polygon(p, &self.clip_polygon())
    }

    /// Stores the document visibility of all design layers and classes in the viewport.
    /// After this the viewport keeps its own visibility.
    pub fn snapshot_visibility(&mut self, doc: &Document) {
        for l in doc.layers.iter().filter(|l| l.kind == LayerKind::Design) {
            self.layer_overrides.insert(l.id, l.visible);
        }
        for c in &doc.classes {
            self.class_overrides.entry(c.id).or_default().visible = Some(c.visible);
        }
    }

    /// Whether this viewport draws the design layer `layer`. The document visibility does not count.
    pub fn layer_visible(&self, doc: &Document, layer: LayerId) -> bool {
        self.layers.contains(&layer) && doc.layer(layer).is_some_and(|l| l.kind == LayerKind::Design) && self.layer_overrides.get(&layer).copied().unwrap_or(true)
    }

    pub fn class_visible(&self, doc: &Document, class: ClassId) -> bool {
        doc.class(class).is_some() && self.class_overrides.get(&class).and_then(|o| o.visible).unwrap_or(true)
    }

    /// Whether the object shows in this viewport.
    pub fn shows(&self, doc: &Document, obj: &Object) -> bool {
        self.layer_visible(doc, obj.layer) && self.class_visible(doc, obj.class)
    }

    /// The attributes that an object gets in this viewport, in the order of precedence
    /// described at the top of this module. `line_weight` is on paper in mm.
    pub fn resolve(&self, doc: &Document, obj: &Object) -> Attributes {
        let class = doc.class(obj.class).map(|c| &c.attrs);
        let ov = self.class_overrides.get(&obj.class);
        Attributes {
            pen: obj.attrs.pen.or(ov.and_then(|o| o.pen)).or(class.and_then(|c| c.pen)).or(Some(crate::model::Color::BLACK)),
            fill: obj.attrs.fill.or(ov.and_then(|o| o.fill)).or(class.and_then(|c| c.fill)).or(Some(crate::model::Color::NONE)),
            line_weight: obj.attrs.line_weight.or(ov.and_then(|o| o.line_weight)).or(class.and_then(|c| c.line_weight)).or(Some(DEFAULT_WEIGHT)),
            fill_pattern: obj.attrs.fill_pattern.or(class.and_then(|c| c.fill_pattern)),
            line_type: obj.attrs.line_type.or(class.and_then(|c| c.line_type)),
        }
    }

    /// Attributes of an annotation: the annotation's own, then its class, then the defaults.
    pub fn resolve_annotation(&self, doc: &Document, a: &Annotation) -> Attributes {
        let class = doc.class(a.class).map(|c| &c.attrs);
        Attributes {
            pen: a.attrs.pen.or(class.and_then(|c| c.pen)).or(Some(crate::model::Color::BLACK)),
            fill: a.attrs.fill.or(class.and_then(|c| c.fill)).or(Some(crate::model::Color::NONE)),
            line_weight: a.attrs.line_weight.or(class.and_then(|c| c.line_weight)).or(Some(DEFAULT_WEIGHT)),
            fill_pattern: a.attrs.fill_pattern.or(class.and_then(|c| c.fill_pattern)),
            line_type: a.attrs.line_type.or(class.and_then(|c| c.line_type)),
        }
    }

    /// Box around what the viewport shows (all objects of its layers that it shows), in model mm.
    pub fn content_bounds(&self, doc: &Document) -> Option<(DVec2, DVec2)> {
        doc.objects
            .values()
            .filter(|o| self.shows(doc, o))
            .map(|o| o.shape.bounds())
            .filter(|(a, b)| a.is_finite() && b.is_finite())
            .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
    }

    /// Sets the center and the scale so that the content fills the frame (a standard scale).
    /// An empty viewport keeps its settings.
    pub fn fit(&mut self, doc: &Document) {
        if let Some((lo, hi)) = self.content_bounds(doc) {
            self.center = (lo + hi) / 2.0;
            self.scale = fit_scale(hi - lo, self.max - self.min);
        }
    }

    /// The next free annotation id.
    pub fn next_annotation_id(&self) -> u32 {
        self.annotations.iter().map(|a| a.id).max().map_or(1, |m| m.saturating_add(1))
    }

    /// Moves the frame, the crop and nothing else (annotations are relative to the frame).
    pub fn translate(&mut self, d: DVec2) {
        self.min += d;
        self.max += d;
        match &mut self.crop {
            Some(Crop::Rect { min, max }) => {
                *min += d;
                *max += d;
            }
            Some(Crop::Polygon { points }) => points.iter_mut().for_each(|p| *p += d),
            None => {}
        }
    }

    /// Where an annotation position lies on the sheet.
    pub fn annotation_to_paper(&self, p: DVec2) -> DVec2 {
        self.min + p
    }
}

/// What is wrong with a viewport or title block shape, or `None` (also for all other shapes).
pub fn shape_problem(shape: &Shape) -> Option<String> {
    match shape {
        Shape::Viewport(v) => v.problem(),
        Shape::TitleBlock(t) => t.problem(),
        _ => None,
    }
}

/// True for the shapes that may be drawn on a sheet layer.
pub fn allowed_on_sheet(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Line { .. } | Shape::Rect { .. } | Shape::Circle { .. } | Shape::Arc { .. } | Shape::Polyline { .. } | Shape::Text { .. } | Shape::Viewport(_) | Shape::TitleBlock(_)
            | Shape::Group { .. }
            | Shape::Symbol { .. }
    )
}

// ---------------------------------------------------------------- polygon helpers

/// Standard drawing scales (denominators), small to large.
pub const STANDARD_SCALES: [f64; 24] = [
    1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 250.0, 300.0, 400.0, 500.0, 750.0, 1000.0, 1250.0, 1500.0, 2000.0, 2500.0, 5000.0, 10000.0,
];

/// The largest standard scale (smallest denominator) at which `model` mm fit in `frame` mm of paper,
/// with 5 percent room. Falls back to the largest denominator that fits, or 1:1 for no size.
pub fn fit_scale(model: DVec2, frame: DVec2) -> f64 {
    let need = (model.x / (frame.x * 0.95)).max(model.y / (frame.y * 0.95));
    if !need.is_finite() || need <= 0.0 {
        return 100.0;
    }
    STANDARD_SCALES.into_iter().find(|s| *s >= need).unwrap_or_else(|| need.ceil())
}

pub fn rect_polygon(min: DVec2, max: DVec2) -> Vec<DVec2> {
    vec![min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)]
}

/// Signed area of a polygon, positive when counterclockwise.
pub fn signed_area(p: &[DVec2]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].perp_dot(p[(i + 1) % n])).sum::<f64>() / 2.0
}

/// The polygon in counterclockwise order.
pub fn ccw(p: &[DVec2]) -> Vec<DVec2> {
    let mut v = p.to_vec();
    if signed_area(&v) < 0.0 {
        v.reverse();
    }
    v
}

/// Cuts a polygon at a rectangle (Sutherland-Hodgman). The result is empty when nothing is left.
pub fn clip_polygon_to_rect(poly: &[DVec2], min: DVec2, max: DVec2) -> Vec<DVec2> {
    // Each edge keeps the points with `dot(p, n) <= c`.
    let edges = [(DVec2::NEG_X, -min.x), (DVec2::X, max.x), (DVec2::NEG_Y, -min.y), (DVec2::Y, max.y)];
    let mut out = poly.to_vec();
    for (n, c) in edges {
        let input = std::mem::take(&mut out);
        for i in 0..input.len() {
            let (a, b) = (input[i], input[(i + 1) % input.len()]);
            let (da, db) = (n.dot(a) - c, n.dot(b) - c);
            if da <= 0.0 {
                out.push(a);
            }
            if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
                out.push(a + (b - a) * (da / (da - db)));
            }
        }
    }
    if out.len() < 3 { Vec::new() } else { out }
}

/// True if the point is inside the polygon or on its border (ray casting, border counts as inside).
pub fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let d = b - a;
        let t = if d.length_squared() > 0.0 { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        if (a + d * t).distance(p) < 1e-9 {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

// ---------------------------------------------------------------- title block

/// One cell of a title block layout. Positions are in mm from the lower left corner of the
/// title block. `text` may use fields such as `{project.name}`, see `Fields`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Small caption in the corner of the cell.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub text: String,
    /// Text height in mm.
    #[serde(default = "default_cell_size")]
    pub size: f64,
}

fn default_cell_size() -> f64 {
    4.0
}

/// A title block layout: a box of `width` by `height` mm in the lower right corner of the border.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TitleLayout {
    pub width: f64,
    pub height: f64,
    pub cells: Vec<Cell>,
}

impl TitleLayout {
    /// The simple layout: project, sheet title, sheet number, scale, date and drawn by.
    pub fn standard() -> TitleLayout {
        let c = |x, y, w, h, label: &str, text: &str, size| Cell { x, y, w, h, label: label.into(), text: text.into(), size };
        TitleLayout {
            width: 180.0,
            height: 40.0,
            cells: vec![
                c(0.0, 20.0, 180.0, 20.0, "Project", "{project.name}", 6.0),
                c(0.0, 0.0, 90.0, 20.0, "Sheet title", "{sheet.title}", 4.5),
                c(90.0, 0.0, 30.0, 20.0, "Sheet no.", "{sheet.number}", 6.0),
                c(120.0, 10.0, 30.0, 10.0, "Scale", "{sheet.scale}", 3.5),
                c(150.0, 10.0, 30.0, 10.0, "Date", "{project.date}", 3.5),
                c(120.0, 0.0, 60.0, 10.0, "Drawn by", "{project.drawn_by}", 3.5),
            ],
        }
    }

    pub fn problem(&self) -> Option<String> {
        let nums = [self.width, self.height];
        if !nums.iter().all(|n| n.is_finite() && *n > 0.0 && *n <= MAX_PAPER) {
            return Some("title block width and height must be positive".into());
        }
        if self.cells.len() > 200 {
            return Some("a title block has at most 200 cells".into());
        }
        let bad = self.cells.iter().any(|c| ![c.x, c.y, c.w, c.h, c.size].iter().all(|n| n.is_finite()) || c.w <= 0.0 || c.h <= 0.0 || c.size <= 0.0);
        bad.then(|| "title block cells need finite numbers and a positive size".into())
    }
}

/// A border with a grid reference and a title block, on a sheet layer. Its frame follows the page
/// size of the sheet, so a change of paper size or orientation moves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TitleBlock {
    /// Distance from the page edge to the border, in mm.
    #[serde(default = "default_margin")]
    pub margin: f64,
    /// Columns (letters along the top and bottom) and rows (numbers along the sides) of the grid
    /// reference. 0 for none.
    #[serde(default)]
    pub grid_columns: u32,
    #[serde(default)]
    pub grid_rows: u32,
    /// Own layout. Without it the standard layout is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<TitleLayout>,
    /// Extra fields for the layout: `{field.name}`. They win over nothing else, they only add.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    /// Origin shift on the sheet. Zero for the page corner.
    #[serde(default)]
    pub offset: DVec2,
    /// The page size (mm) that the title block was made for. Commands keep it equal to the sheet
    /// size. It sets the bounds for selection. Drawing always uses the real sheet size.
    #[serde(default = "default_page")]
    pub page: DVec2,
}

fn default_page() -> DVec2 {
    DVec2::new(420.0, 297.0)
}

fn default_margin() -> f64 {
    10.0
}

impl Default for TitleBlock {
    fn default() -> Self {
        TitleBlock { margin: 10.0, grid_columns: 0, grid_rows: 0, layout: None, fields: BTreeMap::new(), offset: DVec2::ZERO, page: default_page() }
    }
}

impl TitleBlock {
    pub fn problem(&self) -> Option<String> {
        if !(self.page.is_finite() && self.page.min_element() > 0.0 && self.page.max_element() <= MAX_PAPER) {
            return Some("the page size of a title block must be positive".into());
        }
        if !self.margin.is_finite() || !(0.0..=100.0).contains(&self.margin) || !self.offset.is_finite() {
            return Some("margin must be 0 to 100 mm".into());
        }
        if self.grid_columns > 26 || self.grid_rows > 99 {
            return Some("a grid reference has at most 26 columns and 99 rows".into());
        }
        self.layout.as_ref().and_then(TitleLayout::problem)
    }

    pub fn translate(&mut self, d: DVec2) {
        self.offset += d;
    }

    /// The border rectangle on a page of this size.
    pub fn border(&self, page: DVec2) -> (DVec2, DVec2) {
        (DVec2::splat(self.margin) + self.offset, page - DVec2::splat(self.margin) + self.offset)
    }

    /// Bounds on the sheet for selection: the whole border.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        self.border(self.page)
    }
}

/// The text that a layout fills in, taken from the sheet and the document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields(pub BTreeMap<String, String>);

impl Fields {
    /// Fields for a sheet layer: `project.*` from the document, `sheet.*` from the sheet and
    /// `field.*` from the title block.
    pub fn of(doc: &Document, layer: LayerId, tb: &TitleBlock) -> Fields {
        let mut m = BTreeMap::new();
        let p = &doc.drawing.project;
        for (k, v) in [
            ("project.name", &p.name),
            ("project.client", &p.client),
            ("project.address", &p.address),
            ("project.drawn_by", &p.drawn_by),
            ("project.date", &p.date),
            ("project.revision", &p.revision),
        ] {
            m.insert(k.to_string(), v.clone());
        }
        if let Some(s) = doc.sheet(layer) {
            m.insert("sheet.number".into(), s.number);
            m.insert("sheet.title".into(), s.title);
            m.insert("sheet.paper".into(), format!("{} {}", s.paper.id().to_uppercase(), if s.orientation == Orientation::Landscape { "landscape" } else { "portrait" }));
        }
        let mut scales: Vec<f64> = doc.viewports_of(layer).map(|(_, v)| v.scale).collect();
        scales.dedup();
        let scale = match scales.as_slice() {
            [] => String::new(),
            [s] => format_scale(*s),
            _ => "As noted".into(),
        };
        m.insert("sheet.scale".into(), scale);
        let sheets: Vec<LayerId> = doc.sheet_layers().map(|l| l.id).collect();
        m.insert("sheet.index".into(), (sheets.iter().position(|l| *l == layer).unwrap_or(0) + 1).to_string());
        m.insert("sheet.count".into(), sheets.len().to_string());
        for (k, v) in &tb.fields {
            m.insert(format!("field.{k}"), v.clone());
        }
        Fields(m)
    }

    /// Replaces `{name}` in the text. Unknown names give an empty text.
    pub fn fill(&self, text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            match rest[i..].find('}') {
                Some(j) => {
                    out.push_str(self.0.get(rest[i + 1..i + j].trim()).map_or("", String::as_str));
                    rest = &rest[i + j + 1..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// "1:100" for a scale denominator of 100.
pub fn format_scale(s: f64) -> String {
    if (s - s.round()).abs() < 1e-9 { format!("1:{}", s.round()) } else { format!("1:{s}") }
}

/// A title block, worked out for a page: everything a renderer needs.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleBlockGeometry {
    pub border: (DVec2, DVec2),
    /// Line segments of the grid reference ticks, in paper mm.
    pub grid_lines: Vec<(DVec2, DVec2)>,
    /// Grid reference labels: position of the center, the text.
    pub grid_labels: Vec<(DVec2, String)>,
    /// Outer box of the title block.
    pub block: (DVec2, DVec2),
    /// Cells: box, label, filled text, text height.
    pub cells: Vec<(DVec2, DVec2, String, String, f64)>,
}

/// Works out where everything of a title block goes on a page.
pub fn title_block_geometry(tb: &TitleBlock, fields: &Fields, page: DVec2) -> TitleBlockGeometry {
    let (lo, hi) = tb.border(page);
    let mut grid_lines = Vec::new();
    let mut grid_labels = Vec::new();
    let band = (tb.margin * 0.6).clamp(0.0, 6.0);
    if band > 0.0 {
        for i in 0..tb.grid_columns {
            let (x0, x1) = (lo.x + (hi.x - lo.x) * i as f64 / tb.grid_columns as f64, lo.x + (hi.x - lo.x) * (i + 1) as f64 / tb.grid_columns as f64);
            let letter = ((b'A' + i as u8) as char).to_string();
            if i > 0 {
                grid_lines.push((DVec2::new(x0, lo.y), DVec2::new(x0, lo.y - band)));
                grid_lines.push((DVec2::new(x0, hi.y), DVec2::new(x0, hi.y + band)));
            }
            grid_labels.push((DVec2::new((x0 + x1) / 2.0, lo.y - band / 2.0), letter.clone()));
            grid_labels.push((DVec2::new((x0 + x1) / 2.0, hi.y + band / 2.0), letter));
        }
        for j in 0..tb.grid_rows {
            let (y0, y1) = (lo.y + (hi.y - lo.y) * j as f64 / tb.grid_rows as f64, lo.y + (hi.y - lo.y) * (j + 1) as f64 / tb.grid_rows as f64);
            let num = (tb.grid_rows - j).to_string();
            let _ = (y0, y1);
            let ymid = (y0 + y1) / 2.0;
            if j > 0 {
                grid_lines.push((DVec2::new(lo.x, y0), DVec2::new(lo.x - band, y0)));
                grid_lines.push((DVec2::new(hi.x, y0), DVec2::new(hi.x + band, y0)));
            }
            // Row 1 is at the top.
            grid_labels.push((DVec2::new(lo.x - band / 2.0, ymid), num.clone()));
            grid_labels.push((DVec2::new(hi.x + band / 2.0, ymid), num));
        }
    }
    let layout = tb.layout.clone().unwrap_or_else(TitleLayout::standard);
    let origin = DVec2::new(hi.x - layout.width, lo.y);
    let block = (origin.max(lo), DVec2::new(hi.x, lo.y + layout.height).min(hi));
    let cells = layout
        .cells
        .iter()
        .map(|c| (origin + DVec2::new(c.x, c.y), origin + DVec2::new(c.x + c.w, c.y + c.h), c.label.clone(), fields.fill(&c.text), c.size))
        .collect();
    TitleBlockGeometry { border: (lo, hi), grid_lines, grid_labels, block, cells }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Class, Color, FIRST_LAYER, NONE_CLASS};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn vp() -> Viewport {
        Viewport::new(v(20.0, 30.0), v(220.0, 180.0), v(5000.0, 7000.0), 50.0, vec![FIRST_LAYER])
    }

    #[test]
    fn paper_sizes_and_orientation() {
        let mut s = Sheet::new("A1", "Plan");
        assert_eq!(s.size_mm(), v(420.0, 297.0));
        s.orientation = Orientation::Portrait;
        assert_eq!(s.size_mm(), v(297.0, 420.0));
        s.paper = PaperSize::Custom { width_mm: 500.0, height_mm: 300.0 };
        assert_eq!(s.size_mm(), v(300.0, 500.0));
        assert!(s.is_valid());
        s.paper = PaperSize::Custom { width_mm: 5.0, height_mm: 300.0 };
        assert!(!s.is_valid());
        for p in PaperSize::NAMED {
            assert_eq!(PaperSize::from_id(p.id()), Some(p));
            assert!(p.portrait_mm().x < p.portrait_mm().y);
        }
        assert_eq!(PaperSize::from_id(" Arch-D "), Some(PaperSize::ArchD));
        assert_eq!(PaperSize::from_id("b7"), None);
        assert_eq!(PaperSize::Custom { width_mm: 1.0, height_mm: 1.0 }.id(), "custom");
        assert_eq!(Orientation::from_id("Portrait"), Some(Orientation::Portrait));
        assert_eq!(Orientation::from_id("x"), None);
    }

    #[test]
    fn transform_maps_the_center_and_scales() {
        let p = vp();
        assert_eq!(p.to_paper(p.center), p.frame_center());
        // 5 m of model is 100 mm of paper at 1:50.
        assert!((p.to_paper(v(10_000.0, 7000.0)).x - p.frame_center().x - 100.0).abs() < 1e-9);
        let (lo, hi) = p.model_window();
        assert!((hi.x - lo.x - 10_000.0).abs() < 1e-6 && (hi.y - lo.y - 7500.0).abs() < 1e-6);
    }

    #[test]
    fn crop_shapes() {
        let mut p = vp();
        assert_eq!(p.clip_polygon().len(), 4);
        assert!(p.shows_paper(v(100.0, 100.0)) && !p.shows_paper(v(10.0, 100.0)));
        p.crop = Some(Crop::Rect { min: v(0.0, 0.0), max: v(100.0, 100.0) });
        assert_eq!(p.clip_polygon(), rect_polygon(v(20.0, 30.0), v(100.0, 100.0)));
        p.crop = Some(Crop::Rect { min: v(500.0, 500.0), max: v(600.0, 600.0) });
        assert!(p.clip_polygon().is_empty() && !p.shows_paper(v(100.0, 100.0)));
        // A triangle that sticks out of the frame is cut at the frame.
        p.crop = Some(Crop::Polygon { points: vec![v(0.0, 0.0), v(120.0, 0.0), v(0.0, 120.0)] });
        let poly = p.clip_polygon();
        assert!(poly.len() >= 3);
        assert!(poly.iter().all(|q| q.x >= 20.0 - 1e-9 && q.y >= 30.0 - 1e-9));
        assert!(p.shows_paper(v(40.0, 40.0)) && !p.shows_paper(v(100.0, 100.0)));
        // A clockwise polygon works too.
        p.crop = Some(Crop::Polygon { points: vec![v(30.0, 40.0), v(30.0, 100.0), v(100.0, 100.0)] });
        assert!(p.shows_paper(v(40.0, 90.0)));
    }

    #[test]
    fn fitting() {
        // 20 m by 10 m on a 200 mm by 150 mm frame needs 1:105, so the standard scale is 1:125.
        assert_eq!(fit_scale(v(20_000.0, 10_000.0), v(200.0, 150.0)), 125.0);
        assert_eq!(fit_scale(v(10.0, 10.0), v(200.0, 150.0)), 1.0);
        assert_eq!(fit_scale(v(1.0e9, 1.0e9), v(200.0, 150.0)), (1.0e9_f64 / 142.5).ceil());
        assert_eq!(fit_scale(v(0.0, 0.0), v(200.0, 150.0)), 100.0);
        let mut doc = Document::default();
        doc.objects.insert(crate::model::ObjectId(5), Object::new(crate::model::ObjectId(5), FIRST_LAYER, NONE_CLASS, Shape::Rect { min: v(0.0, 0.0), max: v(20_000.0, 10_000.0) }));
        let mut p = vp();
        p.min = v(0.0, 0.0);
        p.max = v(200.0, 150.0);
        p.fit(&doc);
        assert_eq!((p.scale, p.center), (125.0, v(10_000.0, 5000.0)));
        assert_eq!(p.content_bounds(&doc), Some((v(0.0, 0.0), v(20_000.0, 10_000.0))));
        p.layers.clear();
        p.scale = 7.0;
        p.fit(&doc);
        assert_eq!(p.scale, 7.0, "an empty viewport keeps its scale");
    }

    #[test]
    fn polygon_helpers() {
        let sq = rect_polygon(v(0.0, 0.0), v(2.0, 2.0));
        assert_eq!(signed_area(&sq), 4.0);
        let mut cw = sq.clone();
        cw.reverse();
        assert_eq!(signed_area(&ccw(&cw)), 4.0);
        assert!(point_in_polygon(v(1.0, 1.0), &sq) && point_in_polygon(v(2.0, 1.0), &sq) && !point_in_polygon(v(3.0, 1.0), &sq));
        assert!(!point_in_polygon(v(1.0, 1.0), &sq[..2]));
        assert!(clip_polygon_to_rect(&sq, v(5.0, 5.0), v(6.0, 6.0)).is_empty());
        let degenerate = [v(0.0, 0.0), v(0.0, 0.0), v(0.0, 0.0)];
        assert!(!point_in_polygon(v(1.0, 1.0), &degenerate));
    }

    #[test]
    fn viewport_validation() {
        assert_eq!(vp().problem(), None);
        let bad = |f: &dyn Fn(&mut Viewport)| {
            let mut p = vp();
            f(&mut p);
            p.problem().is_some()
        };
        assert!(bad(&|p| p.scale = 0.0));
        assert!(bad(&|p| p.scale = f64::NAN));
        assert!(bad(&|p| p.max = p.min));
        assert!(bad(&|p| p.max = v(1.0e6, 1.0e6)));
        assert!(bad(&|p| p.center = v(1.0e12, 0.0)));
        assert!(bad(&|p| p.dpi = 5.0));
        assert!(bad(&|p| p.mode = "x".into()));
        assert!(bad(&|p| p.view = " ".into()));
        assert!(bad(&|p| p.crop = Some(Crop::Polygon { points: vec![v(0.0, 0.0)] })));
        assert!(bad(&|p| p.crop = Some(Crop::Polygon { points: vec![v(0.0, 0.0), v(1.0, 0.0), v(f64::NAN, 1.0)] })));
        assert!(bad(&|p| p.crop = Some(Crop::Rect { min: v(f64::NAN, 0.0), max: v(1.0, 1.0) })));
        assert!(bad(&|p| {
            p.class_overrides.insert(NONE_CLASS, ClassOverride { line_weight: Some(-1.0), ..Default::default() });
        }));
        let ann = |id| Annotation { id, class: NONE_CLASS, attrs: Attributes::default(), item: AnnotationItem::Line { a: v(0.0, 0.0), b: v(1.0, 1.0) } };
        assert!(bad(&|p| p.annotations = vec![ann(1), ann(1)]));
        assert!(!bad(&|p| p.annotations = vec![ann(1), ann(2)]));
        assert!(bad(&|p| p.annotations = vec![Annotation { item: AnnotationItem::Text { at: v(0.0, 0.0), text: "x".into(), height: 0.0 }, ..ann(1) }]));
        assert!(bad(&|p| p.annotations = vec![Annotation { item: AnnotationItem::Dimension { a: v(0.0, 0.0), b: v(1.0, f64::INFINITY), offset: 0.0, text: None }, ..ann(1) }]));
        assert!(bad(&|p| {
            let inner = Shape::Viewport(Box::new(vp()));
            p.annotations = vec![Annotation { item: AnnotationItem::Shape { shape: Box::new(inner) }, ..ann(1) }];
        }));
        let mut p = vp();
        p.annotations = vec![ann(4), ann(9)];
        assert_eq!(p.next_annotation_id(), 10);
        assert_eq!(vp().next_annotation_id(), 1);
    }

    fn doc_with_overrides() -> (Document, Object) {
        let mut doc = Document::default();
        doc.classes.push(Class { id: ClassId(9), name: "Trees".into(), visible: true, attrs: Attributes { pen: Some(Color([0, 128, 0, 255])), fill: Some(Color([0, 255, 0, 255])), line_weight: Some(0.5), ..Default::default() } });
        let mut o = Object::new(crate::model::ObjectId(50), FIRST_LAYER, ClassId(9), Shape::Circle { center: v(0.0, 0.0), radius: 10.0 });
        o.attrs.pen = Some(Color([1, 2, 3, 255]));
        (doc, o)
    }

    #[test]
    fn override_precedence() {
        let (doc, o) = doc_with_overrides();
        let mut p = vp();
        // No override: the object's own pen, then the class for the rest.
        let a = p.resolve(&doc, &o);
        assert_eq!(a.pen, Some(Color([1, 2, 3, 255])));
        assert_eq!(a.fill, Some(Color([0, 255, 0, 255])));
        assert_eq!(a.line_weight, Some(0.5));
        // A viewport override beats the class but not the attributes that the object sets itself.
        p.class_overrides.insert(ClassId(9), ClassOverride { pen: Some(Color([9, 9, 9, 255])), line_weight: Some(0.1), ..Default::default() });
        let a = p.resolve(&doc, &o);
        assert_eq!(a.pen, Some(Color([1, 2, 3, 255])), "the object has its own pen");
        assert_eq!(a.fill, Some(Color([0, 255, 0, 255])));
        assert_eq!(a.line_weight, Some(0.1));
        // Defaults when nothing sets a field.
        let plain = Object::new(crate::model::ObjectId(51), FIRST_LAYER, NONE_CLASS, o.shape.clone());
        let mut doc2 = doc.clone();
        doc2.classes[0].attrs = Attributes::default();
        let a = vp().resolve(&doc2, &plain);
        assert_eq!((a.pen, a.fill, a.line_weight), (Some(Color::BLACK), Some(Color::NONE), Some(DEFAULT_WEIGHT)));
    }

    #[test]
    fn visibility_overrides() {
        let (mut doc, o) = doc_with_overrides();
        let mut p = vp();
        assert!(p.shows(&doc, &o));
        // Not in the layer list: hidden.
        let mut other = o.clone();
        other.layer = LayerId(77);
        assert!(!p.shows(&doc, &other));
        // The document visibility does not reach the viewport.
        doc.classes[1].visible = false;
        doc.layers[0].visible = false;
        assert!(p.shows(&doc, &o), "no entry in the viewport: visible");
        // A snapshot stores the document visibility in the viewport. Later changes of the document do not reach it.
        p.snapshot_visibility(&doc);
        assert!(!p.shows(&doc, &o));
        doc.classes[1].visible = true;
        doc.layers[0].visible = true;
        assert!(!p.shows(&doc, &o), "the viewport keeps what it stored");
        p.layer_overrides.insert(FIRST_LAYER, true);
        assert!(!p.shows(&doc, &o), "the class is still off");
        p.class_overrides.insert(ClassId(9), ClassOverride { visible: Some(true), ..Default::default() });
        assert!(p.shows(&doc, &o));
        p.class_overrides.insert(ClassId(9), ClassOverride { visible: Some(false), ..Default::default() });
        assert!(!p.shows(&doc, &o));
        p.class_overrides.clear();
        p.layer_overrides.insert(FIRST_LAYER, false);
        assert!(!p.shows(&doc, &o));
        p.layer_overrides.clear();
        // A missing class is hidden. Sheet layers are never shown in a viewport.
        let mut gone = o.clone();
        gone.class = ClassId(1234);
        p.layer_overrides.clear();
        assert!(!p.shows(&doc, &gone));
        doc.layers[0].kind = LayerKind::Sheet;
        assert!(!p.layer_visible(&doc, FIRST_LAYER));
    }

    #[test]
    fn annotation_attributes_and_translate() {
        let (doc, _) = doc_with_overrides();
        let mut p = vp();
        let a = Annotation { id: 1, class: ClassId(9), attrs: Attributes { fill: Some(Color::NONE), ..Default::default() }, item: AnnotationItem::Line { a: v(1.0, 1.0), b: v(2.0, 2.0) } };
        let r = p.resolve_annotation(&doc, &a);
        assert_eq!((r.pen, r.fill, r.line_weight), (Some(Color([0, 128, 0, 255])), Some(Color::NONE), Some(0.5)));
        p.crop = Some(Crop::Rect { min: v(30.0, 40.0), max: v(60.0, 70.0) });
        p.translate(v(10.0, 5.0));
        assert_eq!(p.min, v(30.0, 35.0));
        assert_eq!(p.crop, Some(Crop::Rect { min: v(40.0, 45.0), max: v(70.0, 75.0) }));
        p.crop = Some(Crop::Polygon { points: vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)] });
        p.translate(v(1.0, 1.0));
        assert!(matches!(&p.crop, Some(Crop::Polygon { points }) if points[0] == v(1.0, 1.0)));
        assert_eq!(p.annotation_to_paper(v(1.0, 2.0)), p.min + v(1.0, 2.0));
    }

    #[test]
    fn title_fields_and_geometry() {
        let mut doc = Document::default();
        doc.layers.push(crate::model::Layer { id: LayerId(5), name: "Plan sheet".into(), kind: LayerKind::Sheet, visible: true, locked: false, elevation: 0.0, scale: 1.0 });
        doc.drawing.project.name = "Garden {1}".into();
        doc.drawing.sheets.insert(LayerId(5), Sheet::new("L-101", "Planting plan"));
        let tb = TitleBlock { grid_columns: 4, grid_rows: 3, fields: [("zone".to_string(), "7".to_string())].into(), ..Default::default() };
        let f = Fields::of(&doc, LayerId(5), &tb);
        assert_eq!(f.fill("{project.name} / {sheet.number} / {sheet.scale} / {field.zone} / {nothing} / {open"), "Garden {1} / L-101 /  / 7 /  / {open");
        assert_eq!(f.0["sheet.index"], "1");
        assert_eq!(f.0["sheet.paper"], "A3 landscape");
        let g = title_block_geometry(&tb, &f, v(420.0, 297.0));
        assert_eq!(g.border, (v(10.0, 10.0), v(410.0, 287.0)));
        assert_eq!(g.grid_labels.len(), 2 * 4 + 2 * 3);
        assert_eq!(g.grid_lines.len(), 2 * 3 + 2 * 2);
        assert_eq!(g.block, (v(230.0, 10.0), v(410.0, 50.0)));
        assert_eq!(g.cells.len(), 6);
        assert!(g.cells.iter().any(|c| c.3 == "L-101"));
        // Scale text: one scale, several scales.
        let mut vp1 = vp();
        vp1.scale = 100.0;
        let mut ed = crate::command::Editor::new(doc.clone());
        ed.execute(&crate::command::Command::AddObject { shape: Shape::Viewport(Box::new(vp1.clone())), layer: Some(LayerId(5)), class: None, attrs: Attributes::default() }).unwrap();
        assert_eq!(Fields::of(ed.doc(), LayerId(5), &tb).0["sheet.scale"], "1:100");
        vp1.scale = 12.5;
        ed.execute(&crate::command::Command::AddObject { shape: Shape::Viewport(Box::new(vp1)), layer: Some(LayerId(5)), class: None, attrs: Attributes::default() }).unwrap();
        assert_eq!(Fields::of(ed.doc(), LayerId(5), &tb).0["sheet.scale"], "As noted");
        assert_eq!(format_scale(12.5), "1:12.5");
        // No margin: no grid reference drawn.
        let tb0 = TitleBlock { margin: 0.0, grid_columns: 3, ..Default::default() };
        assert!(title_block_geometry(&tb0, &f, v(420.0, 297.0)).grid_labels.is_empty());
    }

    #[test]
    fn title_block_validation() {
        assert_eq!(TitleBlock::default().problem(), None);
        let mut tb = TitleBlock { margin: -1.0, ..Default::default() };
        assert!(tb.problem().is_some());
        tb = TitleBlock { grid_columns: 27, ..Default::default() };
        assert!(tb.problem().is_some());
        let mut l = TitleLayout::standard();
        assert_eq!(l.problem(), None);
        l.cells[0].w = 0.0;
        assert!(l.problem().is_some());
        l = TitleLayout::standard();
        l.width = f64::NAN;
        assert!(l.problem().is_some());
        l = TitleLayout::standard();
        l.cells = vec![l.cells[0].clone(); 201];
        assert!(l.problem().is_some());
        tb = TitleBlock { layout: Some(l), ..Default::default() };
        assert!(tb.problem().is_some());
        let mut tb = TitleBlock::default();
        tb.translate(v(1.0, 2.0));
        assert_eq!(tb.border(v(100.0, 100.0)).0, v(11.0, 12.0));
        assert_eq!(tb.bounds().0, v(11.0, 12.0));
        tb.page = v(0.0, 5.0);
        assert!(tb.problem().is_some());
    }

    #[test]
    fn saved_view_and_drawing_roundtrip() {
        let mut d = Drawing::default();
        assert!(d.is_empty());
        d.sheets.insert(LayerId(3), Sheet::new("1", "t"));
        d.saved_views.push(SavedView { name: "Plan".into(), layers: [(LayerId(1), true)].into(), classes: [(ClassId(0), false)].into(), zoom: Some(2.0), pan: Some(v(1.0, 2.0)), ..Default::default() });
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Drawing>(&json).unwrap(), d);
        assert!(d.saved_view(" plan ").is_some() && d.saved_view("x").is_none());
        assert!(d.saved_views[0].is_finite());
        let bad = SavedView { zoom: Some(0.0), ..Default::default() };
        assert!(!bad.is_finite());
        let bad = SavedView { pan: Some(v(f64::NAN, 0.0)), ..Default::default() };
        assert!(!bad.is_finite());
        assert!(!d.is_empty());
    }

    #[test]
    fn viewport_json_roundtrip_and_defaults() {
        let mut p = vp();
        p.crop = Some(Crop::Polygon { points: vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)] });
        p.layer_overrides.insert(FIRST_LAYER, false);
        p.class_overrides.insert(NONE_CLASS, ClassOverride { visible: Some(true), line_weight: Some(0.3), ..Default::default() });
        p.annotations.push(Annotation { id: 1, class: NONE_CLASS, attrs: Attributes::default(), item: AnnotationItem::Dimension { a: v(0.0, 0.0), b: v(10.0, 0.0), offset: 5.0, text: None } });
        p.annotations.push(Annotation { id: 2, class: NONE_CLASS, attrs: Attributes::default(), item: AnnotationItem::Shape { shape: Box::new(Shape::Circle { center: v(1.0, 1.0), radius: 2.0 }) } });
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Viewport>(&json).unwrap(), p);
        let minimal: Viewport = serde_json::from_str(r#"{"min":[0,0],"max":[10,10],"center":[0,0],"scale":10}"#).unwrap();
        assert!(minimal.is_plan() && minimal.mode == "shaded" && minimal.dpi == DEFAULT_DPI && minimal.layers.is_empty());
    }
}
