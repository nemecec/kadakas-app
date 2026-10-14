//! Document model: design layers, classes and drawing objects.

use std::collections::BTreeMap;

use glam::{DVec2, DVec3};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::curve;
pub use crate::curve::{Vertex, VertexKind};
use crate::plants::tree::{self, TreeAction, TreeCondition};
use crate::resource::{ResourceId, Resources};
pub use crate::site::{ModifierKind, PadSlope, SiteLine, SiteSettings};

use crate::uid::{IdGen, Uid, id_type};

id_type!(
    /// Identifier of an object. Objects, layers, classes and resources have IDs that are unique
    /// in the world (see `crate::uid`).
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
    /// A hatch or tile resource that fills closed shapes, drawn in the pen color over the fill color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_pattern: Option<ResourceId>,
    /// A line type resource (dashes, and symbols along the line) for the outline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_type: Option<ResourceId>,
}

impl Attributes {
    /// These attributes, with every field they leave empty taken from `base`.
    pub fn over(&self, base: &Attributes) -> Attributes {
        Attributes {
            pen: self.pen.or(base.pen),
            fill: self.fill.or(base.fill),
            line_weight: self.line_weight.or(base.line_weight),
            fill_pattern: self.fill_pattern.or(base.fill_pattern),
            line_type: self.line_type.or(base.line_type),
        }
    }

    pub fn is_finite(&self) -> bool {
        self.line_weight.is_none_or(f64::is_finite)
    }
}

/// Largest absolute value of any coordinate, size or elevation, in mm (1000 km).
/// Larger numbers are refused, because 3D views and exports cannot hold them.
pub const MAX_COORD: f64 = 1.0e9;

/// Geometry and parameters of an object. Coordinates are model units (mm).
///
/// `Unknown` holds a shape of a kind that this version does not know. It keeps the original JSON
/// and writes it back unchanged. It is not drawn and it can only be deleted. The derive skips it:
/// the `shape_serde` functions of `Object` read and write it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    /// A shape of an unknown kind, as in the file. `raw` is the whole shape object.
    #[serde(skip)]
    Unknown {
        kind: String,
        raw: Value,
    },
    Line {
        a: DVec2,
        b: DVec2,
    },
    Rect {
        min: DVec2,
        max: DVec2,
    },
    Circle {
        center: DVec2,
        radius: f64,
    },
    /// Counterclockwise arc. Angles are radians. A sweep of 2π or more is a full circle.
    Arc {
        center: DVec2,
        radius: f64,
        start: f64,
        end: f64,
    },
    Polyline {
        points: Vec<Vertex>,
        closed: bool,
    },
    Text {
        at: DVec2,
        text: String,
        height: f64,
    },
    /// Plant instance. `spread` is the canopy diameter.
    Plant {
        at: DVec2,
        species: String,
        spread: f64,
        height: f64,
    },
    /// Existing (survey) tree. `dbh` is the trunk diameter, `spread` the canopy diameter.
    /// The root protection zone has a radius of `rpz_factor` times `dbh` (default 12), at most `rpz_cap` (see `plants::tree`).
    ExistingTree {
        at: DVec2,
        #[serde(default)]
        species: String,
        dbh: f64,
        spread: f64,
        height: f64,
        #[serde(default)]
        condition: TreeCondition,
        #[serde(default)]
        action: TreeAction,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rpz_factor: Option<f64>,
        /// Largest root protection radius in mm (default 15 000, see `plants::tree`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rpz_cap: Option<f64>,
    },
    /// Plant tag: a label at `at` with a leader line to `anchor`. It shows the ID code and the
    /// number of plants of the style on its layer. `height` is the text height.
    PlantTag {
        at: DVec2,
        anchor: DVec2,
        style: ResourceId,
        #[serde(default = "default_tag_height")]
        height: f64,
    },
    /// Planting area with a plant mix, bounded by a closed outline.
    LandscapeArea {
        boundary: Vec<Vertex>,
        name: String,
    },
    /// Paved area (path, patio, drive).
    /// `thickness` (default 150 mm) is measured down from the top surface. `elevation` (default 0)
    /// is the height of the top surface above the layer elevation.
    Hardscape {
        boundary: Vec<Vertex>,
        material: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elevation: Option<f64>,
    },
    /// Survey point with a Z value.
    Locus3d {
        p: DVec3,
    },
    /// Polygon or contour with a Z value per vertex.
    Polyline3d {
        points: Vec<DVec3>,
        closed: bool,
    },
    /// Terrain. The source data (3D points and 3D polygons) lives inside the site model.
    /// The TIN, contours and volumes are derived by `kadakas-site` and never saved.
    SiteModel {
        #[serde(default)]
        points: Vec<DVec3>,
        #[serde(default)]
        lines: Vec<SiteLine>,
        #[serde(default)]
        settings: SiteSettings,
    },
    /// A pad or grade limits that change the proposed surface of the site models on the same layer.
    SiteModifier {
        modifier: ModifierKind,
        boundary: Vec<Vertex>,
    },
    /// Viewport on a sheet layer: design layers at a scale. Coordinates are paper mm (see `sheets`).
    Viewport(Box<crate::sheets::Viewport>),
    /// Building block: footprint, height or floors, and a simple roof (see `massing`).
    Massing(Box<crate::massing::Massing>),
    /// Georeferenced raster, such as a map background (see `underlay`). Drawn below the vector objects.
    Underlay(Box<crate::underlay::Underlay>),
    /// Border, grid reference and title block on a sheet layer.
    TitleBlock(crate::sheets::TitleBlock),
    /// Dimension, callout, data tag, legend, north arrow, scale bar, revision cloud or styled text.
    Annotation(Box<crate::annot::Annot>),
    /// A group. Its members have this object as their `parent`. The group has no drawing of its own.
    Group {
        #[serde(default)]
        name: String,
    },
    /// An instance of a symbol definition (a resource). `at` is where the insertion point of the
    /// definition goes. The instance is turned by `rotation` (radians) and scaled by `scale`
    /// (in x, and in y too unless `scale_y` is set) about that point. A negative scale mirrors.
    Symbol {
        symbol: ResourceId,
        at: DVec2,
        #[serde(default)]
        rotation: f64,
        #[serde(default = "default_symbol_scale")]
        scale: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale_y: Option<f64>,
    },
    /// A generic 3D object: a triangle mesh with absolute coordinates (see `mesh3d`). The IFC importer uses it
    /// for slabs, coverings and other shapes that have no native Kadakas object.
    Mesh3d(Box<crate::mesh3d::Mesh3d>),
    /// A closed 2D profile pushed up by a height (see `solid3d`). The profile stays editable.
    Extrude(Box<crate::solid3d::Extrude>),
    /// A profile turned around an axis, with a sweep angle, a segment angle and a pitch (see `solid3d`).
    Sweep(Box<crate::solid3d::Sweep>),
    /// An irrigation object: outlet, line, point of connection, valve or controller
    /// (see `crate::irrigation`). In JSON it is `{"kind": "irrigation", "type": "outlet", ...}`.
    Irrigation(Box<crate::irrigation::IrrigationShape>),
}

fn default_symbol_scale() -> f64 {
    1.0
}

/// Default text height of a plant tag, in mm.
pub const DEFAULT_TAG_HEIGHT: f64 = 400.0;

fn default_tag_height() -> f64 {
    DEFAULT_TAG_HEIGHT
}

/// Half the width of the cross that marks a `Locus3d` in drawings and 3D views, in model units.
pub const LOCUS_MARKER_HALF: f64 = 100.0;

/// Area of the XY projection of a polygon, in square model units.
fn polygon_area_xy(points: &[DVec3]) -> f64 {
    let n = points.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    twice.abs() / 2.0
}

/// Counterclockwise sweep of an arc in radians, from 0 (empty) to 2π (full circle).
/// An end angle below the start wraps around, so 270 degrees to 0 is a quarter turn.
pub fn arc_sweep(start: f64, end: f64) -> f64 {
    let d = end - start;
    if d.abs() >= std::f64::consts::TAU {
        std::f64::consts::TAU
    } else {
        d.rem_euclid(std::f64::consts::TAU)
    }
}

impl Shape {
    pub fn kind(&self) -> &'static str {
        match self {
            Shape::Unknown { .. } => "unknown",
            Shape::Line { .. } => "line",
            Shape::Rect { .. } => "rect",
            Shape::Circle { .. } => "circle",
            Shape::Arc { .. } => "arc",
            Shape::Polyline { .. } => "polyline",
            Shape::Text { .. } => "text",
            Shape::Plant { .. } => "plant",
            Shape::ExistingTree { .. } => "existing_tree",
            Shape::PlantTag { .. } => "plant_tag",
            Shape::LandscapeArea { .. } => "landscape_area",
            Shape::Hardscape { .. } => "hardscape",
            Shape::Locus3d { .. } => "locus3d",
            Shape::Polyline3d { .. } => "polyline3d",
            Shape::SiteModel { .. } => "site_model",
            Shape::SiteModifier { .. } => "site_modifier",
            Shape::Viewport(_) => "viewport",
            Shape::TitleBlock(_) => "title_block",
            Shape::Annotation(_) => "annotation",
            Shape::Group { .. } => "group",
            Shape::Symbol { .. } => "symbol",
            Shape::Massing(_) => "massing",
            Shape::Underlay(_) => "underlay",
            Shape::Mesh3d(_) => "mesh3d",
            Shape::Extrude(_) => "extrude",
            Shape::Sweep(_) => "sweep",
            Shape::Irrigation(i) => i.kind(),
        }
    }

    /// The vertices of a polyline, landscape area or hardscape.
    pub fn vertices(&self) -> Option<&Vec<Vertex>> {
        match self {
            Shape::Polyline { points, .. } => Some(points),
            Shape::LandscapeArea { boundary, .. }
            | Shape::Hardscape { boundary, .. }
            | Shape::SiteModifier { boundary, .. } => Some(boundary),
            Shape::Massing(m) => Some(&m.footprint),
            Shape::Extrude(e) => Some(&e.profile),
            _ => None,
        }
    }

    pub fn vertices_mut(&mut self) -> Option<&mut Vec<Vertex>> {
        match self {
            Shape::Polyline { points, .. } => Some(points),
            Shape::LandscapeArea { boundary, .. }
            | Shape::Hardscape { boundary, .. }
            | Shape::SiteModifier { boundary, .. } => Some(boundary),
            Shape::Massing(m) => Some(&mut m.footprint),
            Shape::Extrude(e) => Some(&mut e.profile),
            _ => None,
        }
    }

    /// True if every number in the shape is finite.
    pub fn is_finite(&self) -> bool {
        let v = |p: &DVec2| p.is_finite();
        match self {
            Shape::Viewport(vp) => vp.problem().is_none(),
            Shape::TitleBlock(tb) => tb.problem().is_none(),
            Shape::Annotation(a) => a.is_finite(),
            Shape::Group { .. } | Shape::Unknown { .. } => true,
            Shape::Massing(m) => m.is_finite(),
            Shape::Underlay(u) => u.is_finite(),
            Shape::Mesh3d(m) => m.is_finite(),
            Shape::Extrude(e) => e.is_finite(),
            Shape::Sweep(s) => s.is_finite(),
            Shape::Irrigation(i) => i.is_finite(),
            Shape::Symbol {
                at,
                rotation,
                scale,
                scale_y,
                ..
            } => {
                v(at)
                    && rotation.is_finite()
                    && scale.is_finite()
                    && scale_y.is_none_or(f64::is_finite)
            }
            Shape::Locus3d { p } => p.is_finite(),
            Shape::Polyline3d { points, .. } => points.iter().all(|p| p.is_finite()),
            Shape::SiteModel {
                points,
                lines,
                settings,
            } => {
                points.iter().all(|p| p.is_finite())
                    && lines.iter().all(|l| l.points.iter().all(|p| p.is_finite()))
                    && settings.is_finite()
            }
            Shape::SiteModifier { modifier, boundary } => {
                modifier.is_finite() && boundary.iter().all(Vertex::is_finite)
            }
            Shape::Line { a, b } => v(a) && v(b),
            Shape::Rect { min, max } => v(min) && v(max),
            Shape::Circle { center, radius } => v(center) && radius.is_finite(),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => v(center) && radius.is_finite() && start.is_finite() && end.is_finite(),
            Shape::Text { at, height, .. } => v(at) && height.is_finite(),
            Shape::Plant {
                at, spread, height, ..
            } => v(at) && spread.is_finite() && height.is_finite(),
            Shape::ExistingTree {
                at,
                dbh,
                spread,
                height,
                rpz_factor,
                rpz_cap,
                ..
            } => {
                v(at)
                    && dbh.is_finite()
                    && spread.is_finite()
                    && height.is_finite()
                    && rpz_factor.is_none_or(f64::is_finite)
                    && rpz_cap.is_none_or(f64::is_finite)
            }
            Shape::PlantTag {
                at, anchor, height, ..
            } => v(at) && v(anchor) && height.is_finite(),
            Shape::Hardscape {
                boundary,
                thickness,
                elevation,
                ..
            } => {
                boundary.iter().all(Vertex::is_finite)
                    && thickness.is_none_or(f64::is_finite)
                    && elevation.is_none_or(f64::is_finite)
            }
            Shape::Polyline { points, .. }
            | Shape::LandscapeArea {
                boundary: points, ..
            } => points.iter().all(Vertex::is_finite),
        }
    }

    /// Calls `f` with every number of the shape.
    fn for_each_number(&self, f: &mut dyn FnMut(f64)) {
        let mut v = |p: &DVec2| {
            f(p.x);
            f(p.y);
        };
        let vertex = |x: &Vertex, f: &mut dyn FnMut(f64)| {
            f(x.p.x);
            f(x.p.y);
            if let VertexKind::Arc { radius } = x.kind {
                f(radius);
            }
        };
        match self {
            // Viewports and title blocks are checked as a whole (`Viewport::problem`).
            Shape::Viewport(_)
            | Shape::TitleBlock(_)
            | Shape::Group { .. }
            | Shape::Unknown { .. } => {}
            Shape::Irrigation(i) => i.for_each_number(f),
            Shape::Annotation(a) => a.numbers(f),
            Shape::Massing(m) => {
                m.footprint.iter().for_each(|x| vertex(x, f));
                [m.height, m.floor_height, m.pitch_deg]
                    .into_iter()
                    .chain(m.ridge_angle)
                    .chain(m.base)
                    .for_each(&mut *f);
            }
            Shape::Underlay(u) => {
                v(&u.min);
                v(&u.max);
            }
            Shape::Mesh3d(m) => m
                .vertices
                .iter()
                .for_each(|p| p.to_array().into_iter().for_each(&mut *f)),
            Shape::Extrude(e) => {
                e.profile
                    .iter()
                    .chain(e.holes.iter().flatten())
                    .for_each(|x| vertex(x, f));
                f(e.height);
                f(e.base);
            }
            Shape::Sweep(s) => {
                s.profile.iter().for_each(|x| vertex(x, f));
                s.origin.to_array().into_iter().for_each(&mut *f);
                s.axis.to_array().into_iter().for_each(&mut *f);
                [
                    s.rotation_deg,
                    s.sweep_angle_deg,
                    s.segment_angle_deg,
                    s.pitch,
                ]
                .into_iter()
                .for_each(&mut *f);
            }
            Shape::Symbol {
                at,
                rotation,
                scale,
                scale_y,
                ..
            } => {
                v(at);
                [*rotation, *scale]
                    .into_iter()
                    .chain(scale_y.iter().copied())
                    .for_each(&mut *f);
            }
            Shape::Locus3d { p } => p.to_array().into_iter().for_each(&mut *f),
            Shape::Polyline3d { points, .. } => points
                .iter()
                .for_each(|p| p.to_array().into_iter().for_each(&mut *f)),
            Shape::SiteModel {
                points,
                lines,
                settings,
            } => {
                points
                    .iter()
                    .for_each(|p| p.to_array().into_iter().for_each(&mut *f));
                lines
                    .iter()
                    .flat_map(|l| &l.points)
                    .for_each(|p| p.to_array().into_iter().for_each(&mut *f));
                [
                    settings.minor_interval,
                    settings.major_interval,
                    settings.datum,
                ]
                .into_iter()
                .chain(settings.look_numbers())
                .for_each(&mut *f);
            }
            Shape::SiteModifier { modifier, boundary } => {
                modifier.numbers().into_iter().for_each(&mut *f);
                boundary.iter().for_each(|x| vertex(x, f));
            }
            Shape::Line { a, b } => {
                v(a);
                v(b);
            }
            Shape::Rect { min, max } => {
                v(min);
                v(max);
            }
            Shape::Circle { center, radius } => {
                v(center);
                f(*radius);
            }
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                v(center);
                [*radius, *start, *end].into_iter().for_each(&mut *f);
            }
            Shape::Text { at, height, .. } => {
                v(at);
                f(*height);
            }
            Shape::Plant {
                at, spread, height, ..
            } => {
                v(at);
                f(*spread);
                f(*height);
            }
            Shape::ExistingTree {
                at,
                dbh,
                spread,
                height,
                rpz_factor,
                rpz_cap,
                ..
            } => {
                v(at);
                [*dbh, *spread, *height]
                    .into_iter()
                    .chain(rpz_factor.iter().copied())
                    .chain(rpz_cap.iter().copied())
                    .for_each(&mut *f);
            }
            Shape::PlantTag {
                at, anchor, height, ..
            } => {
                v(at);
                v(anchor);
                f(*height);
            }
            Shape::Hardscape {
                boundary,
                thickness,
                elevation,
                ..
            } => {
                boundary.iter().for_each(|x| vertex(x, f));
                thickness.iter().chain(elevation.iter()).for_each(|x| f(*x));
            }
            Shape::Polyline { points, .. }
            | Shape::LandscapeArea {
                boundary: points, ..
            } => points.iter().for_each(|x| vertex(x, f)),
        }
    }

    /// True if no number is larger than `MAX_COORD` in absolute value (NaN fails too).
    pub fn is_in_range(&self) -> bool {
        let mut ok = true;
        self.for_each_number(&mut |x| ok &= x.abs() <= MAX_COORD);
        ok
    }

    /// The first problem of a shape whose values are checked as a whole: a massing model, an
    /// underlay, a mesh, a solid, a site model or a texture bed. `None` for the other shapes.
    pub fn problem(&self) -> Option<String> {
        match self {
            Shape::Massing(m) => m.problem(),
            Shape::Underlay(u) => u.problem(),
            Shape::Mesh3d(m) => m.problem(),
            Shape::Extrude(e) => e.problem(),
            Shape::Sweep(s) => s.problem(),
            Shape::SiteModel { settings, .. } => settings.problem().map(String::from),
            Shape::SiteModifier { .. } => crate::texture_bed::problem(self).map(String::from),
            _ => None,
        }
    }

    /// The mesh of an extrude or a sweep, with `z` measured from the layer. `None` for other
    /// shapes and for a solid with a problem.
    pub fn solid_mesh(&self) -> Option<crate::solid3d::SolidMesh> {
        match self {
            Shape::Extrude(e) => e.mesh(),
            Shape::Sweep(s) => s.mesh(),
            _ => None,
        }
    }

    /// The 3D box of an extrude or a sweep as (min, max), with `z` measured from the layer.
    pub fn solid_bounds3(&self) -> Option<(DVec3, DVec3)> {
        match self {
            Shape::Extrude(e) => Some(e.bounds3()),
            Shape::Sweep(s) => Some(s.bounds3()),
            _ => None,
        }
    }

    /// True unless a hardscape has a thickness of zero or less.
    pub fn has_valid_thickness(&self) -> bool {
        !matches!(self, Shape::Hardscape { thickness: Some(t), .. } if t.is_nan() || *t <= 0.0)
    }

    pub fn translate(&mut self, d: DVec2) {
        let mv = |p: &mut DVec2| *p += d;
        match self {
            Shape::Viewport(vp) => vp.translate(d),
            Shape::TitleBlock(tb) => tb.translate(d),
            Shape::Annotation(a) => a.translate(d),
            Shape::Massing(m) => m.footprint.iter_mut().for_each(|x| mv(&mut x.p)),
            Shape::Extrude(e) => e
                .profile
                .iter_mut()
                .chain(e.holes.iter_mut().flatten())
                .for_each(|x| mv(&mut x.p)),
            Shape::Sweep(s) => {
                s.origin.x += d.x;
                s.origin.y += d.y;
            }
            Shape::Underlay(u) => {
                mv(&mut u.min);
                mv(&mut u.max);
            }
            Shape::Group { .. } | Shape::Unknown { .. } => {}
            Shape::Mesh3d(m) => m.vertices.iter_mut().for_each(|p| {
                p.x += d.x;
                p.y += d.y;
            }),
            Shape::Irrigation(i) => i.translate(d),
            Shape::Symbol { at, .. } => mv(at),
            Shape::Locus3d { p } => {
                p.x += d.x;
                p.y += d.y;
            }
            Shape::Polyline3d { points, .. } => points.iter_mut().for_each(|p| {
                p.x += d.x;
                p.y += d.y;
            }),
            Shape::SiteModel {
                points,
                lines,
                settings,
            } => {
                points
                    .iter_mut()
                    .chain(lines.iter_mut().flat_map(|l| l.points.iter_mut()))
                    .for_each(|p| {
                        p.x += d.x;
                        p.y += d.y;
                    });
                // The texture is anchored to the ground: it moves with the model.
                if settings.surface_look == crate::site::SurfaceLook::Texture {
                    settings.texture_offset[0] += d.x;
                    settings.texture_offset[1] += d.y;
                }
            }
            Shape::Line { a, b } => {
                mv(a);
                mv(b);
            }
            Shape::Rect { min, max } => {
                mv(min);
                mv(max);
            }
            Shape::Circle { center, .. } | Shape::Arc { center, .. } => mv(center),
            Shape::Text { at, .. } | Shape::Plant { at, .. } | Shape::ExistingTree { at, .. } => {
                mv(at)
            }
            Shape::PlantTag { at, anchor, .. } => {
                mv(at);
                mv(anchor);
            }
            Shape::Polyline { points, .. }
            | Shape::LandscapeArea {
                boundary: points, ..
            }
            | Shape::Hardscape {
                boundary: points, ..
            } => points.iter_mut().for_each(|x| mv(&mut x.p)),
            Shape::SiteModifier { modifier, boundary } => {
                boundary.iter_mut().for_each(|x| mv(&mut x.p));
                // The texture of a bed with its own mapping is anchored to the ground.
                if let ModifierKind::TextureBed {
                    mapping: Some(m), ..
                } = modifier
                {
                    *m = m.translated(d);
                }
            }
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
            Shape::Viewport(vp) => (vp.min, vp.max),
            Shape::TitleBlock(tb) => tb.bounds(),
            Shape::Annotation(a) => a.bounds(),
            Shape::Massing(m) => curve::bounds(&m.footprint, true),
            Shape::Extrude(e) => curve::bounds(&e.profile, true),
            Shape::Sweep(s) => {
                let (lo, hi) = s.bounds3();
                (lo.truncate(), hi.truncate())
            }
            Shape::Underlay(u) => (u.min, u.max),
            Shape::Mesh3d(m) => {
                let (lo, hi) = m.bounds3();
                (lo.truncate(), hi.truncate())
            }
            // The size of these depends on the document. See `Document::object_bounds`.
            Shape::Group { .. } | Shape::Unknown { .. } => from_points(&[]),
            Shape::Symbol { at, .. } => (*at, *at),
            Shape::Irrigation(i) => i.bounds(),
            Shape::Locus3d { p } => (p.truncate(), p.truncate()),
            Shape::Polyline3d { points, .. } => {
                from_points(&points.iter().map(|p| p.truncate()).collect::<Vec<_>>())
            }
            Shape::SiteModel { points, lines, .. } => {
                let all: Vec<DVec2> = points
                    .iter()
                    .chain(lines.iter().flat_map(|l| &l.points))
                    .map(|p| p.truncate())
                    .collect();
                from_points(&all)
            }
            Shape::Line { a, b } => from_points(&[*a, *b]),
            Shape::Rect { min, max } => from_points(&[*min, *max]),
            Shape::Circle { center, radius } => (
                *center - DVec2::splat(*radius),
                *center + DVec2::splat(*radius),
            ),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                // The arc touches its endpoints, and each axis extreme that lies within the sweep.
                let sweep = arc_sweep(*start, *end);
                let mut pts = vec![
                    *center + DVec2::from_angle(*start) * *radius,
                    *center + DVec2::from_angle(*start + sweep) * *radius,
                ];
                for (k, axis) in [DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y]
                    .into_iter()
                    .enumerate()
                {
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
            Shape::ExistingTree {
                at,
                spread,
                dbh,
                rpz_factor,
                rpz_cap,
                ..
            } => {
                let r =
                    DVec2::splat((spread / 2.0).max(tree::rpz_radius(*dbh, *rpz_factor, *rpz_cap)));
                (*at - r, *at + r)
            }
            Shape::PlantTag {
                at, anchor, height, ..
            } => {
                // The label is about eight characters wide.
                from_points(&[*at, *at + DVec2::new(*height * 0.6 * 8.0, *height), *anchor])
            }
            Shape::Polyline { points, closed } => curve::bounds(points, *closed),
            Shape::LandscapeArea {
                boundary: points, ..
            }
            | Shape::Hardscape {
                boundary: points, ..
            }
            | Shape::SiteModifier {
                boundary: points, ..
            } => curve::bounds(points, true),
        }
    }

    /// Enclosed area in square model units, for closed shapes.
    pub fn area(&self) -> Option<f64> {
        match self {
            Shape::Rect { min, max } => Some(((max.x - min.x) * (max.y - min.y)).abs()),
            Shape::Massing(m) => Some(curve::area(&m.footprint)),
            Shape::Extrude(e) => Some(e.profile_area()),
            Shape::Circle { radius, .. } => Some(std::f64::consts::PI * radius * radius),
            Shape::Polyline3d {
                points,
                closed: true,
            } => Some(polygon_area_xy(points)),
            Shape::Polyline {
                points,
                closed: true,
            }
            | Shape::LandscapeArea {
                boundary: points, ..
            }
            | Shape::SiteModifier {
                boundary: points, ..
            }
            | Shape::Hardscape {
                boundary: points, ..
            } => Some(curve::area(points)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub layer: LayerId,
    pub class: ClassId,
    #[serde(with = "shape_serde")]
    pub shape: Shape,
    #[serde(default)]
    pub attrs: Attributes,
    /// Record data, for example property sets. Keys are free-form.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, Value>,
    /// The group that this object belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ObjectId>,
    /// Fields that this version does not know (from a newer file). They are saved again unchanged.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, Value>,
}

impl Object {
    pub fn new(id: ObjectId, layer: LayerId, class: ClassId, shape: Shape) -> Self {
        Object {
            id,
            layer,
            class,
            shape,
            attrs: Attributes::default(),
            data: BTreeMap::new(),
            parent: None,
            extra: Default::default(),
        }
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
    /// Fields that this version does not know (from a newer file). They are saved again unchanged.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, Value>,
}

/// A class: a second grouping axis for visibility and default attributes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Class {
    pub id: ClassId,
    pub name: String,
    pub visible: bool,
    pub attrs: Attributes,
    /// Fields that this version does not know (from a newer file). They are saved again unchanged.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, Value>,
}

/// Reads and writes `Object::shape`, with the fallback for unknown kinds.
mod shape_serde {
    use super::Shape;
    use crate::underlay::PngData;
    use serde::de::{MapAccess, SeqAccess, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    pub fn serialize<S: Serializer>(shape: &Shape, s: S) -> Result<S::Ok, S::Error> {
        match shape {
            Shape::Unknown { raw, .. } => raw.serialize(s),
            other => other.serialize(s),
        }
    }

    /// Reads the text of a shape as a `Value`, because a kind that this version does not know must
    /// stay as it is (`Shape::Unknown`). One field is the exception: the picture of an underlay can be
    /// tens of MB of base64 text. When the `kind` is already known to be `underlay`, the `png`
    /// text is decoded where the parser holds it and does not go into the `Value`.
    struct RawShape;

    impl<'de> Visitor<'de> for RawShape {
        type Value = (Value, Option<PngData>);

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a shape")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut map = serde_json::Map::new();
            let (mut underlay, mut png) = (false, None);
            while let Some(key) = access.next_key::<String>()? {
                let v = if underlay && key == "png" {
                    png = Some(access.next_value::<PngData>()?);
                    Value::String(String::new())
                } else {
                    let v: Value = access.next_value()?;
                    underlay |= key == "kind" && v.as_str() == Some("underlay");
                    v
                };
                if map.insert(key.clone(), v).is_some() {
                    return Err(serde::de::Error::custom(format!("duplicate field `{key}`")));
                }
            }
            Ok((Value::Object(map), png))
        }

        // Anything that is not an object is no shape; `Shape` says so below.
        fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
            Ok((Value::Bool(v), None))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok((Value::Null, None))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok((Value::from(v), None))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok((Value::from(v), None))
        }

        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
            Ok((Value::from(v), None))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
            Ok((Value::from(v), None))
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut list = Vec::new();
            while let Some(v) = seq.next_element::<Value>()? {
                list.push(v);
            }
            Ok((Value::Array(list), None))
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Shape, D::Error> {
        let (raw, png) = d.deserialize_any(RawShape)?;
        // The picture was taken out of `raw` only for a shape with the kind `underlay`, a kind that
        // this version knows. It never becomes `Shape::Unknown`, so no text is lost.
        match Shape::deserialize(&raw) {
            Ok(Shape::Underlay(mut u)) => {
                if let Some(png) = png {
                    u.png = png;
                }
                Ok(Shape::Underlay(u))
            }
            Err(e) if png.is_some() => Err(serde::de::Error::custom(e)),
            Ok(shape) => Ok(shape),
            Err(e) => {
                // Only a kind that we do not know becomes `Unknown`. A known kind with bad fields is an error.
                let kind = raw.get("kind").and_then(Value::as_str);
                let message = e.to_string();
                // A site modifier of a kind that we do not know (a newer version) stays as it is.
                let modifier = raw
                    .get("modifier")
                    .and_then(|m| m.get("type"))
                    .and_then(Value::as_str);
                match kind {
                    Some(k) if message.starts_with(&format!("unknown variant `{k}`")) => {
                        Ok(Shape::Unknown {
                            kind: k.to_string(),
                            raw,
                        })
                    }
                    Some("site_modifier")
                        if modifier.is_some_and(|t| {
                            message.starts_with(&format!("unknown variant `{t}`"))
                        }) =>
                    {
                        Ok(Shape::Unknown {
                            kind: "site_modifier".to_string(),
                            raw,
                        })
                    }
                    _ => Err(serde::de::Error::custom(e)),
                }
            }
        }
    }
}

/// Objects in z-order (first is at the bottom), found by ID in constant time.
/// In files they are a plain JSON array.
pub type Objects = IndexMap<ObjectId, Object>;

mod objects_as_array {
    use super::{Object, Objects};
    use serde::de::{Error, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(map: &Objects, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(map.values())
    }

    /// Reads the array object by object straight into the map. There is no `Vec` of all objects
    /// first, which would hold the objects twice at the end of a big load.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Objects, D::Error> {
        struct ObjectsVisitor;
        impl<'de> Visitor<'de> for ObjectsVisitor {
            type Value = Objects;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an array of objects")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Objects, A::Error> {
                // A hostile size hint must not reserve memory: the map grows with the objects.
                let mut map = Objects::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
                while let Some(o) = seq.next_element::<Object>()? {
                    let id = o.id;
                    if map.insert(id, o).is_some() {
                        return Err(A::Error::custom(format!("duplicate id {id}")));
                    }
                }
                Ok(map)
            }
        }
        d.deserialize_seq(ObjectsVisitor)
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
    /// Where the drawing sits on the earth, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub georef: Option<crate::georef::Georeference>,
    /// Reusable definitions, such as plant styles. Older files have none.
    #[serde(default, skip_serializing_if = "Resources::is_empty")]
    pub resources: Resources,
    /// Sheets, project data and saved views. Older files have none.
    #[serde(default, skip_serializing_if = "crate::sheets::Drawing::is_empty")]
    pub drawing: crate::sheets::Drawing,
    /// The generator of new IDs (see `crate::uid`). It is not saved.
    #[serde(skip)]
    ids: IdGen,
    /// What the last load repaired (see `validate`). It is not saved.
    #[serde(skip)]
    load_notes: Vec<String>,
    /// Fields that this version does not know (from a newer file). They are saved again unchanged.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, Value>,
}

/// Equality includes the z-order of the objects.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.format_version == other.format_version
            && self.layers == other.layers
            && self.classes == other.classes
            && self.active_layer == other.active_layer
            && self.active_class == other.active_class
            && self.georef == other.georef
            && self.resources == other.resources
            && self.drawing == other.drawing
            && self.extra == other.extra
            && self.objects.len() == other.objects.len()
            && self.objects.iter().eq(other.objects.iter())
    }
}

/// The file format version. 3: IDs are UUID texts (see `crate::uid`) and there is no `next_id`.
/// 2: unknown shape kinds and unknown fields are kept (no data changed from 1).
pub const FORMAT_VERSION: u32 = 3;
/// The oldest format that this version reads. Older files have numbers as IDs.
pub const OLDEST_FORMAT: u32 = 3;
/// The class that every new document has. It cannot be deleted. Like `FIRST_LAYER`, it has a
/// well-known ID, the same in every document.
pub const NONE_CLASS: ClassId = ClassId::from_num(0);
/// The layer that every new document has.
pub const FIRST_LAYER: LayerId = LayerId::from_num(1);

/// Why a document could not be loaded.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LoadError {
    /// Not JSON, or not the shape of a document.
    #[error("not a Kadakas file: {0}")]
    Parse(String),
    #[error("file was made by a newer Kadakas (format {0})")]
    Newer(u32),
    /// The file is older than the oldest format that this version reads.
    #[error(
        "unsupported old format {0}: this version of Kadakas reads format {OLDEST_FORMAT} and newer (IDs are UUID texts now)"
    )]
    Unsupported(u32),
    /// The document breaks a rule, for example a duplicate ID.
    #[error("{0}")]
    Invalid(String),
}

/// Refuses a `format_version` that this version cannot read.
fn check_version(version: u64) -> Result<(), LoadError> {
    let version = u32::try_from(version).unwrap_or(u32::MAX);
    if version > FORMAT_VERSION {
        Err(LoadError::Newer(version))
    } else if version < OLDEST_FORMAT {
        Err(LoadError::Unsupported(version))
    } else {
        Ok(())
    }
}

/// Reads only `format_version` of a document. Every other field is skipped without storing it.
/// It accepts a top-level object only, and refuses a second `format_version`.
struct VersionProbe {
    format_version: Option<Value>,
}

impl<'de> Deserialize<'de> for VersionProbe {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, IgnoredAny, MapAccess, Visitor};
        struct Probe;
        impl<'de> Visitor<'de> for Probe {
            type Value = VersionProbe;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a document")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<VersionProbe, A::Error> {
                let mut version = None;
                while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
                    if key == "format_version" {
                        if version.is_some() {
                            return Err(A::Error::custom("duplicate key `format_version`"));
                        }
                        version = Some(map.next_value::<Value>()?);
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(VersionProbe {
                    format_version: version,
                })
            }
        }
        d.deserialize_map(Probe)
    }
}

/// The error for a text that does not parse as a document. `first` is the error of the typed
/// parse. `probe` reads only the version, so a newer or older file says so, and a file without a
/// version says that. A reader error (a failed read) is reported as it is.
fn explain_failure(
    first: serde_json::Error,
    probe: impl FnOnce() -> Result<VersionProbe, serde_json::Error>,
) -> LoadError {
    if first.is_io() {
        return LoadError::Parse(first.to_string());
    }
    match probe() {
        Ok(VersionProbe {
            format_version: Some(Value::Number(n)),
        }) if n.is_u64() => match check_version(n.as_u64().unwrap_or_default()) {
            Err(e) => e,
            Ok(()) => LoadError::Parse(first.to_string()),
        },
        // JSON, but not a document: no version, or a version that is no number.
        Ok(_) => LoadError::Parse("missing format_version".into()),
        Err(p) if p.is_data() => {
            let m = p.to_string();
            if m.starts_with("duplicate key") {
                LoadError::Parse(m)
            } else {
                // Not an object at the top.
                LoadError::Parse("missing format_version".into())
            }
        }
        // Not JSON at all, or a failed seek: the first error says where.
        Err(_) => LoadError::Parse(first.to_string()),
    }
}

impl Default for Document {
    fn default() -> Self {
        Document {
            format_version: FORMAT_VERSION,
            layers: vec![Layer {
                extra: Default::default(),
                id: FIRST_LAYER,
                name: "Layer 1".into(),
                kind: LayerKind::Design,
                visible: true,
                locked: false,
                elevation: 0.0,
                scale: 100.0,
            }],
            classes: vec![Class {
                extra: Default::default(),
                id: NONE_CLASS,
                name: "None".into(),
                visible: true,
                attrs: Attributes {
                    pen: Some(Color::BLACK),
                    fill: Some(Color::NONE),
                    line_weight: Some(0.25),
                    ..Attributes::default()
                },
            }],
            objects: Objects::new(),
            active_layer: FIRST_LAYER,
            active_class: NONE_CLASS,
            georef: None,
            resources: Resources::default(),
            drawing: Default::default(),
            ids: IdGen::default(),
            load_notes: Vec::new(),
            extra: Default::default(),
        }
    }
}

impl Document {
    /// The one way to load a document from text: parses the JSON, refuses newer
    /// formats, migrates older ones, validates and repairs the ID counter.
    pub fn from_json(text: &str) -> Result<Document, LoadError> {
        Document::from_slice(text.as_bytes())
    }

    /// Like `from_json`, for bytes that are already in memory (the browser's file picker). Use
    /// this for bytes and `from_reader` for files: parsing a slice is faster than parsing a reader.
    /// There is no `serde_json::Value` copy of the document: the typed parser reads the bytes.
    pub fn from_slice(bytes: &[u8]) -> Result<Document, LoadError> {
        match serde_json::from_slice::<Document>(bytes) {
            Ok(doc) => doc.checked(),
            Err(e) => Err(explain_failure(e, || {
                serde_json::from_slice::<VersionProbe>(bytes)
            })),
        }
    }

    /// Like `from_json`, but reads the JSON from a reader (a buffered file). The text is never held
    /// in memory as a whole, and there is no `serde_json::Value` copy of the document: the reader
    /// feeds the typed parser directly. `Seek` is for the error path only: when the typed parse
    /// fails, the reader goes back to the start and a second pass reads just `format_version`
    /// (everything else is skipped without storing it), so a newer or an older file still gets its
    /// own clear error and not a complaint about its shape. If the reader cannot seek, the error
    /// of the first pass is the answer.
    ///
    /// A known field that appears twice in the file is an error (`duplicate field`), because
    /// two readers could take different values. The old reading through a `Value` took the last one.
    pub fn from_reader<R: std::io::Read + std::io::Seek>(
        mut reader: R,
    ) -> Result<Document, LoadError> {
        match serde_json::from_reader::<_, Document>(&mut reader) {
            Ok(doc) => doc.checked(),
            Err(e) => Err(explain_failure(e, || {
                reader.rewind().map_err(serde_json::Error::io)?;
                serde_json::from_reader::<_, VersionProbe>(&mut reader)
            })),
        }
    }

    /// The version check and the repairs after a typed parse.
    fn checked(self) -> Result<Document, LoadError> {
        check_version(u64::from(self.format_version))?;
        self.finish_load()
    }

    /// Like `from_json`, for JSON that is already parsed.
    pub fn from_value(value: Value) -> Result<Document, LoadError> {
        let version = value
            .get("format_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| LoadError::Parse("missing format_version".into()))?;
        check_version(version)?;
        let doc: Document =
            serde_json::from_value(value).map_err(|e| LoadError::Parse(e.to_string()))?;
        doc.finish_load()
    }

    /// The checks and repairs that follow the parse of every load.
    fn finish_load(mut self) -> Result<Document, LoadError> {
        self.validate().map_err(LoadError::Invalid)?;
        self.resources
            .verify_pictures()
            .map_err(LoadError::Invalid)?;
        Ok(self)
    }

    /// Writes the document as pretty-printed JSON (the text of a saved `.kad` file) straight into
    /// `w`, without building the text. The bytes are those of `serde_json::to_string_pretty`.
    /// Pass a buffered writer for a file.
    pub fn write_pretty<W: std::io::Write>(&self, w: W) -> std::io::Result<()> {
        serde_json::to_writer_pretty(w, self).map_err(std::io::Error::from)
    }

    /// Like `write_pretty`, with no white space (the autosave copies).
    pub fn write_compact<W: std::io::Write>(&self, w: W) -> std::io::Result<()> {
        serde_json::to_writer(w, self).map_err(std::io::Error::from)
    }

    /// The IDs that the next `n` new objects, layers, classes or resources get, in order. File
    /// importers and the API use them to plan the IDs of a batch of commands: the commands, run
    /// next, take exactly these IDs.
    pub fn next_ids(&self, n: usize) -> Vec<Uid> {
        self.ids.peek(n)
    }

    /// A copy of the generator of IDs, to plan the IDs of a batch of commands that adds many
    /// things: the n-th ID that the copy gives is the n-th ID that the commands take.
    pub fn id_plan(&self) -> IdGen {
        self.ids.clone()
    }

    /// Takes a new ID. It is unique in the world (UUID version 7, see `crate::uid`).
    pub fn alloc_id(&mut self) -> Uid {
        self.ids.take()
    }

    /// An empty document whose IDs count 2, 3, 4 and so on. For tests and fixtures; see
    /// [`Document::use_sequential_ids`].
    pub fn with_sequential_ids() -> Document {
        let mut doc = Document::default();
        doc.use_sequential_ids(2);
        doc
    }

    /// Makes the generator count on from the largest small ID in the document (at least from 2).
    /// For a host that runs in the predictable mode (`kadakas serve --sequential-ids`) when it
    /// opens a document.
    pub fn use_sequential_ids_after_existing(&mut self) {
        let last = self
            .objects
            .keys()
            .map(|i| i.0)
            .chain(self.layers.iter().map(|l| l.id.0))
            .chain(self.classes.iter().map(|c| c.id.0))
            .chain(self.resources.iter().map(|r| r.id.0))
            .filter(|u| u.as_u128() < (1 << 63))
            .map(|u| u.low_u64())
            .max()
            .unwrap_or(0);
        self.use_sequential_ids(last.saturating_add(1).max(2));
    }

    /// Swaps the generators of IDs of two documents.
    pub(crate) fn swap_ids(&mut self, other: &mut Document) {
        std::mem::swap(&mut self.ids, &mut other.ids);
    }

    /// Makes the generator count from `first`: 2, 3, 4 and so on, as small numbers. For tests and
    /// fixtures that need predictable IDs. These IDs are not unique in the world.
    pub fn use_sequential_ids(&mut self, first: u64) {
        self.ids = IdGen::sequential(first);
    }

    /// Restarts the generator of IDs from a time (milliseconds since 1970) and a random number.
    /// A host without a clock in `std` (the browser) calls this once, with its own clock.
    pub fn seed_ids(&mut self, unix_ms: u64, salt: u64) {
        self.ids = IdGen::seed(unix_ms, salt);
    }

    /// Checks a loaded document: IDs must be unique, and every object must
    /// refer to an existing layer and class.
    /// What `validate` repaired instead of refusing the file, in plain English.
    pub fn load_notes(&self) -> &[String] {
        &self.load_notes
    }

    pub fn validate(&mut self) -> Result<(), String> {
        // A bad value in the 3D look of a site model (from a hand-edited file or a newer
        // version) must not make the file unreadable: the look goes back to its defaults.
        self.load_notes.clear();
        let mut notes = Vec::new();
        for o in self.objects.values_mut() {
            if let Some(what) = crate::texture_bed::repair(&mut o.shape) {
                notes.push(format!("texture bed {}: {what}", o.id));
            }
            if let Shape::SiteModel { settings, .. } = &mut o.shape
                && settings.problem().is_some()
            {
                let mut fixed = settings.clone();
                fixed.reset_look();
                if fixed.problem().is_none() && fixed.look_numbers().iter().all(|v| v.is_finite()) {
                    notes.push(format!(
                        "site model {}: the 3D look settings were not valid and were reset",
                        o.id
                    ));
                    *settings = fixed;
                }
            }
        }
        notes.extend(crate::provenance::repair(self));
        self.load_notes = notes;
        let mut seen = std::collections::HashSet::new();
        let ids = self
            .objects
            .keys()
            .map(|id| id.0)
            .chain(self.layers.iter().map(|l| l.id.0))
            .chain(self.classes.iter().map(|c| c.id.0))
            .chain(self.resources.iter().map(|r| r.id.0));
        for id in ids {
            if !seen.insert(id) {
                return Err(format!("duplicate id {id}"));
            }
        }
        if self.layers.is_empty() {
            return Err("document has no layers".into());
        }
        if let Some(l) = self
            .layers
            .iter()
            .find(|l| l.elevation.is_nan() || l.elevation.abs() > MAX_COORD)
        {
            return Err(format!("layer {} has an elevation out of range", l.id));
        }
        if let Some(o) = self
            .objects
            .values()
            .find(|o| self.layer(o.layer).is_none() || self.class(o.class).is_none())
        {
            return Err(format!(
                "object {} refers to a missing layer or class",
                o.id
            ));
        }
        if let Some(o) = self.objects.values().find(|o| {
            !o.is_finite()
                || !o.shape.is_in_range()
                || !o.shape.has_valid_thickness()
                || o.shape.problem().is_some()
                || matches!(o.shape, Shape::Arc { start, end, .. } if arc_sweep(start, end) == 0.0)
                || o.shape.vertices().is_some_and(|vs| {
                    vs.iter()
                        .any(|v| matches!(v.kind, VertexKind::Arc { radius } if radius < 0.0))
                })
        }) {
            return Err(format!("object {} has an invalid shape", o.id));
        }
        if let Some(o) = self.objects.values().find(|o| {
            let sheet = self
                .layer(o.layer)
                .is_some_and(|l| l.kind == LayerKind::Sheet);
            let paper_only = matches!(o.shape, Shape::Viewport(_) | Shape::TitleBlock(_));
            (sheet && !crate::sheets::allowed_on_sheet(&o.shape)) || (paper_only && !sheet)
        }) {
            return Err(format!("object {} is on the wrong kind of layer", o.id));
        }
        self.resources.validate()?;
        crate::symbols::repair_edits(self);
        crate::group::check_tree(self)?;
        if self.layer(self.active_layer).is_none() || self.class(self.active_class).is_none() {
            return Err("active layer or class does not exist".into());
        }
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
        match class {
            Some(c) => obj.attrs.over(c),
            None => obj.attrs.clone(),
        }
    }

    /// True if the object shows in the model: its design layer and class are visible.
    /// Objects on sheet layers are paper space and never show in the model (see `is_shown`).
    pub fn is_visible(&self, obj: &Object) -> bool {
        self.layer(obj.layer)
            .is_some_and(|l| l.visible && l.kind == LayerKind::Design)
            && self.class(obj.class).is_some_and(|c| c.visible)
            && !crate::symbols::is_being_edited(obj)
    }

    /// True if the object is drawn: visible, and not hidden by the phase display rules. Only
    /// drawing and picking use this. Calculations (terrain, quantities, reports) use `is_visible`:
    /// display rules never change a result.
    pub fn is_displayed(&self, obj: &Object) -> bool {
        self.is_visible(obj) && !crate::viewrules::hidden_by_phase(self, obj)
    }

    /// True if the object can be picked or snapped to: it shows in the model, or it is on the
    /// active sheet layer.
    pub fn is_pickable(&self, obj: &Object) -> bool {
        (self.is_displayed(obj) || (self.is_shown(obj) && obj.layer == self.active_layer))
            && !self.is_grey(obj)
    }

    /// True if the layer and the class of the object are visible, on any kind of layer.
    pub fn is_shown(&self, obj: &Object) -> bool {
        self.layer(obj.layer).is_some_and(|l| l.visible)
            && self.class(obj.class).is_some_and(|c| c.visible)
            && !crate::symbols::is_being_edited(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(shape: Shape) -> Object {
        Object::new(ObjectId::from_num(7), FIRST_LAYER, NONE_CLASS, shape)
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
            (
                Shape::Line {
                    a: v(0.0, 0.0),
                    b: v(1.0, 1.0),
                },
                "line",
            ),
            (
                Shape::Rect {
                    min: v(0.0, 0.0),
                    max: v(1.0, 1.0),
                },
                "rect",
            ),
            (
                Shape::Circle {
                    center: v(0.0, 0.0),
                    radius: 1.0,
                },
                "circle",
            ),
            (
                Shape::Arc {
                    center: v(0.0, 0.0),
                    radius: 1.0,
                    start: 0.0,
                    end: 1.0,
                },
                "arc",
            ),
            (
                Shape::Polyline {
                    points: crate::curve::corners(&[]),
                    closed: false,
                },
                "polyline",
            ),
            (
                Shape::Text {
                    at: v(0.0, 0.0),
                    text: "t".into(),
                    height: 1.0,
                },
                "text",
            ),
            (
                Shape::Plant {
                    at: v(0.0, 0.0),
                    species: "s".into(),
                    spread: 1.0,
                    height: 1.0,
                },
                "plant",
            ),
            (
                Shape::LandscapeArea {
                    boundary: crate::curve::corners(&[]),
                    name: "n".into(),
                },
                "landscape_area",
            ),
            (
                Shape::Hardscape {
                    boundary: crate::curve::corners(&[]),
                    material: "m".into(),
                    thickness: None,
                    elevation: None,
                },
                "hardscape",
            ),
            (Shape::Locus3d { p: DVec3::ZERO }, "locus3d"),
            (
                Shape::Polyline3d {
                    points: vec![],
                    closed: false,
                },
                "polyline3d",
            ),
        ];
        for (shape, kind) in cases {
            assert_eq!(shape.kind(), kind);
        }
    }

    #[test]
    fn translate_moves_every_point_of_every_shape() {
        let d = v(1.0, -2.0);
        let mut shapes = vec![
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(1.0, 1.0),
            },
            Shape::Rect {
                min: v(0.0, 0.0),
                max: v(2.0, 2.0),
            },
            Shape::Circle {
                center: v(5.0, 5.0),
                radius: 1.0,
            },
            Shape::Arc {
                center: v(5.0, 5.0),
                radius: 1.0,
                start: 0.0,
                end: 1.0,
            },
            Shape::Polyline {
                points: crate::curve::corners(&[v(0.0, 0.0), v(1.0, 0.0)]),
                closed: true,
            },
            Shape::Text {
                at: v(3.0, 3.0),
                text: "x".into(),
                height: 2.0,
            },
            Shape::Plant {
                at: v(3.0, 3.0),
                species: "x".into(),
                spread: 2.0,
                height: 2.0,
            },
            Shape::LandscapeArea {
                boundary: crate::curve::corners(&[v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)]),
                name: "a".into(),
            },
            Shape::Hardscape {
                boundary: crate::curve::corners(&[v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)]),
                material: "m".into(),
                thickness: None,
                elevation: None,
            },
        ];
        for s in &mut shapes {
            let before = s.bounds();
            s.translate(d);
            let after = s.bounds();
            assert_vec_close(after.0, before.0 + d);
            assert_vec_close(after.1, before.1 + d);
        }
        let mut c = Shape::Circle {
            center: v(5.0, 5.0),
            radius: 1.0,
        };
        c.translate(d);
        assert_eq!(
            c,
            Shape::Circle {
                center: v(6.0, 3.0),
                radius: 1.0
            }
        );
        let mut r = Shape::Rect {
            min: v(0.0, 0.0),
            max: v(2.0, 2.0),
        };
        r.translate(d);
        assert_eq!(
            r,
            Shape::Rect {
                min: v(1.0, -2.0),
                max: v(3.0, 0.0)
            }
        );
        let mut l = Shape::Line {
            a: v(0.0, 0.0),
            b: v(1.0, 1.0),
        };
        l.translate(d);
        assert_eq!(
            l,
            Shape::Line {
                a: v(1.0, -2.0),
                b: v(2.0, -1.0)
            }
        );
        let mut t = Shape::Text {
            at: v(3.0, 3.0),
            text: "x".into(),
            height: 2.0,
        };
        t.translate(d);
        assert_eq!(
            t,
            Shape::Text {
                at: v(4.0, 1.0),
                text: "x".into(),
                height: 2.0
            }
        );
    }

    #[test]
    fn locus3d_and_polyline3d_translate_in_xy_only() {
        let mut l = Shape::Locus3d {
            p: DVec3::new(1.0, 2.0, 3.0),
        };
        l.translate(v(10.0, -1.0));
        assert_eq!(
            l,
            Shape::Locus3d {
                p: DVec3::new(11.0, 1.0, 3.0)
            }
        );
        let mut p = Shape::Polyline3d {
            points: vec![DVec3::new(0.0, 0.0, 5.0), DVec3::new(1.0, 1.0, 6.0)],
            closed: false,
        };
        p.translate(v(1.0, 2.0));
        assert_eq!(
            p,
            Shape::Polyline3d {
                points: vec![DVec3::new(1.0, 2.0, 5.0), DVec3::new(2.0, 3.0, 6.0)],
                closed: false
            }
        );
    }

    #[test]
    fn locus3d_and_polyline3d_bounds_ignore_z() {
        let (lo, hi) = Shape::Locus3d {
            p: DVec3::new(4.0, -2.0, 99.0),
        }
        .bounds();
        assert_eq!((lo, hi), (v(4.0, -2.0), v(4.0, -2.0)));
        let pts = vec![
            DVec3::new(0.0, 0.0, 1.0),
            DVec3::new(4.0, -1.0, 50.0),
            DVec3::new(2.0, 6.0, -3.0),
        ];
        assert_eq!(
            Shape::Polyline3d {
                points: pts,
                closed: true
            }
            .bounds(),
            (v(0.0, -1.0), v(4.0, 6.0))
        );
    }

    #[test]
    fn polyline3d_area_is_the_xy_area_when_closed() {
        let sq = vec![
            DVec3::new(0.0, 0.0, 1.0),
            DVec3::new(3.0, 0.0, 9.0),
            DVec3::new(3.0, 3.0, 2.0),
            DVec3::new(0.0, 3.0, 0.0),
        ];
        assert_eq!(
            Shape::Polyline3d {
                points: sq.clone(),
                closed: true
            }
            .area(),
            Some(9.0)
        );
        let cw: Vec<DVec3> = sq.iter().rev().copied().collect();
        assert_eq!(
            Shape::Polyline3d {
                points: cw,
                closed: true
            }
            .area(),
            Some(9.0)
        );
        assert_eq!(
            Shape::Polyline3d {
                points: sq,
                closed: false
            }
            .area(),
            None
        );
        assert_eq!(Shape::Locus3d { p: DVec3::ZERO }.area(), None);
    }

    #[test]
    fn locus3d_and_polyline3d_finite_range_and_serde() {
        let ok = Shape::Locus3d {
            p: DVec3::new(1.0, 2.0, 3.0),
        };
        assert!(ok.is_finite() && ok.is_in_range());
        assert!(
            !Shape::Locus3d {
                p: DVec3::new(0.0, 0.0, f64::NAN)
            }
            .is_finite()
        );
        assert!(
            !Shape::Locus3d {
                p: DVec3::new(0.0, 0.0, 2.0 * MAX_COORD)
            }
            .is_in_range()
        );
        let far = Shape::Polyline3d {
            points: vec![DVec3::ZERO, DVec3::new(0.0, 0.0, -2.0 * MAX_COORD)],
            closed: false,
        };
        assert!(far.is_finite() && !far.is_in_range());
        assert!(
            !Shape::Polyline3d {
                points: vec![DVec3::new(f64::INFINITY, 0.0, 0.0)],
                closed: false
            }
            .is_finite()
        );
        let j = serde_json::to_value(&ok).unwrap();
        assert_eq!(
            j,
            serde_json::json!({"kind": "locus3d", "p": [1.0, 2.0, 3.0]})
        );
        let pl = Shape::Polyline3d {
            points: vec![DVec3::new(0.0, 1.0, 2.0)],
            closed: true,
        };
        let j = serde_json::to_value(&pl).unwrap();
        assert_eq!(
            j,
            serde_json::json!({"kind": "polyline3d", "points": [[0.0, 1.0, 2.0]], "closed": true})
        );
        assert_eq!(serde_json::from_value::<Shape>(j).unwrap(), pl);
    }

    #[test]
    fn bounds_of_every_shape() {
        let (lo, hi) = Shape::Line {
            a: v(3.0, -1.0),
            b: v(-2.0, 5.0),
        }
        .bounds();
        assert_eq!((lo, hi), (v(-2.0, -1.0), v(3.0, 5.0)));

        // A rect with min above max still gives a normalized box.
        let (lo, hi) = Shape::Rect {
            min: v(5.0, 5.0),
            max: v(1.0, 2.0),
        }
        .bounds();
        assert_eq!((lo, hi), (v(1.0, 2.0), v(5.0, 5.0)));

        let (lo, hi) = Shape::Circle {
            center: v(1.0, 1.0),
            radius: 2.0,
        }
        .bounds();
        assert_eq!((lo, hi), (v(-1.0, -1.0), v(3.0, 3.0)));

        // An arc of 1 radian (less than a quarter turn) from angle 0 touches only the +X extreme.
        let (lo, hi) = Shape::Arc {
            center: v(0.0, 0.0),
            radius: 4.0,
            start: 0.0,
            end: 1.0,
        }
        .bounds();
        assert_vec_close(lo, v(4.0 * 1f64.cos(), 0.0));
        assert_vec_close(hi, v(4.0, 4.0 * 1f64.sin()));

        // Text width is 0.6 * height per character, counted in chars not bytes.
        let (lo, hi) = Shape::Text {
            at: v(1.0, 2.0),
            text: "héé".into(),
            height: 10.0,
        }
        .bounds();
        assert_vec_close(lo, v(1.0, 2.0));
        assert_vec_close(hi, v(1.0 + 18.0, 12.0));

        let (lo, hi) = Shape::Plant {
            at: v(10.0, 10.0),
            species: "Oak".into(),
            spread: 4.0,
            height: 8.0,
        }
        .bounds();
        assert_eq!((lo, hi), (v(8.0, 8.0), v(12.0, 12.0)));

        let poly = vec![v(0.0, 0.0), v(4.0, -1.0), v(2.0, 6.0)];
        let expected = (v(0.0, -1.0), v(4.0, 6.0));
        assert_eq!(
            Shape::Polyline {
                points: crate::curve::corners(&poly.clone()),
                closed: false
            }
            .bounds(),
            expected
        );
        assert_eq!(
            Shape::LandscapeArea {
                boundary: crate::curve::corners(&poly.clone()),
                name: "a".into()
            }
            .bounds(),
            expected
        );
        assert_eq!(
            Shape::Hardscape {
                boundary: crate::curve::corners(&poly),
                material: "m".into(),
                thickness: None,
                elevation: None
            }
            .bounds(),
            expected
        );
    }

    #[test]
    fn area_of_closed_shapes() {
        assert_eq!(
            Shape::Rect {
                min: v(0.0, 0.0),
                max: v(4.0, -3.0)
            }
            .area(),
            Some(12.0)
        );
        let circle = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 2.0,
        }
        .area()
        .unwrap();
        assert!((circle - 4.0 * std::f64::consts::PI).abs() < 1e-9);

        let square = vec![v(0.0, 0.0), v(3.0, 0.0), v(3.0, 3.0), v(0.0, 3.0)];
        assert_eq!(
            Shape::Polyline {
                points: crate::curve::corners(&square.clone()),
                closed: true
            }
            .area(),
            Some(9.0)
        );
        assert_eq!(
            Shape::Polyline {
                points: crate::curve::corners(&square.clone()),
                closed: false
            }
            .area(),
            None
        );
        assert_eq!(
            Shape::Hardscape {
                boundary: crate::curve::corners(&square.clone()),
                material: "m".into(),
                thickness: None,
                elevation: None
            }
            .area(),
            Some(9.0)
        );

        let tri = vec![v(0.0, 0.0), v(4.0, 0.0), v(0.0, 3.0)];
        assert_eq!(
            Shape::LandscapeArea {
                boundary: crate::curve::corners(&tri),
                name: "t".into()
            }
            .area(),
            Some(6.0)
        );

        let open_shapes = [
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(1.0, 0.0),
            },
            Shape::Arc {
                center: v(0.0, 0.0),
                radius: 1.0,
                start: 0.0,
                end: 1.0,
            },
            Shape::Text {
                at: v(0.0, 0.0),
                text: "t".into(),
                height: 1.0,
            },
            Shape::Plant {
                at: v(0.0, 0.0),
                species: "s".into(),
                spread: 1.0,
                height: 1.0,
            },
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
        assert!(doc.layer(LayerId::from_num(99)).is_none());
        assert!(doc.class(ClassId::from_num(99)).is_none());
        assert!(doc.object(ObjectId::from_num(1)).is_none());
    }

    #[test]
    fn alloc_id_is_unique_increasing_and_planned_and_object_lookup_works() {
        let mut doc = Document::default();
        let plan = doc.next_ids(3);
        let (a, b) = (doc.alloc_id(), doc.alloc_id());
        assert_eq!(
            (plan[0], plan[1]),
            (a, b),
            "the plan shows the IDs that are taken"
        );
        assert!(a < b);
        assert_eq!((a.version(), b.version()), (7, 7));
        assert_eq!(doc.id_plan().take(), plan[2]);
        // Another document makes other IDs, also when both start in the same millisecond.
        let mut other = Document::default();
        other.seed_ids(a.unix_ms(), 12345);
        doc.seed_ids(a.unix_ms(), 54321);
        assert_ne!(doc.alloc_id(), other.alloc_id());
        let o = obj(Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0,
        });
        doc.objects.insert(o.id, o);
        assert_eq!(
            doc.object(ObjectId::from_num(7)).unwrap().id,
            ObjectId::from_num(7)
        );
        doc.object_mut(ObjectId::from_num(7)).unwrap().layer = LayerId::from_num(9);
        assert_eq!(
            doc.object(ObjectId::from_num(7)).unwrap().layer,
            LayerId::from_num(9)
        );
        assert!(doc.object_mut(ObjectId::from_num(8)).is_none());
    }

    #[test]
    fn resolved_attrs_falls_back_to_class_then_object_overrides() {
        let mut doc = Document::default();
        doc.classes.push(Class {
            extra: Default::default(),
            id: ClassId::from_num(5),
            name: "Trees".into(),
            visible: true,
            attrs: Attributes {
                pen: Some(Color([0, 128, 0, 255])),
                fill: Some(Color([1, 2, 3, 255])),
                line_weight: Some(0.5),
                ..Default::default()
            },
        });

        // No object overrides: every value comes from the class.
        let mut o = obj(Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0,
        });
        o.class = ClassId::from_num(5);
        let a = doc.resolved_attrs(&o);
        assert_eq!(a.pen, Some(Color([0, 128, 0, 255])));
        assert_eq!(a.fill, Some(Color([1, 2, 3, 255])));
        assert_eq!(a.line_weight, Some(0.5));

        // Object overrides win per field; unset fields still come from the class.
        o.attrs = Attributes {
            pen: Some(Color::BLACK),
            fill: None,
            line_weight: Some(2.0),
            ..Default::default()
        };
        let a = doc.resolved_attrs(&o);
        assert_eq!(a.pen, Some(Color::BLACK));
        assert_eq!(a.fill, Some(Color([1, 2, 3, 255])));
        assert_eq!(a.line_weight, Some(2.0));

        // Unknown class: only object values survive.
        o.class = ClassId::from_num(99);
        o.attrs = Attributes::default();
        assert_eq!(doc.resolved_attrs(&o), Attributes::default());
    }

    #[test]
    fn is_visible_respects_layer_and_class_visibility() {
        let mut doc = Document::default();
        let o = obj(Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0,
        });
        assert!(doc.is_visible(&o));

        doc.layers[0].visible = false;
        assert!(!doc.is_visible(&o), "hidden layer hides its objects");
        doc.layers[0].visible = true;

        doc.classes[0].visible = false;
        assert!(!doc.is_visible(&o), "hidden class hides its objects");
        doc.classes[0].visible = true;

        let mut orphan = o.clone();
        orphan.layer = LayerId::from_num(42);
        assert!(
            !doc.is_visible(&orphan),
            "object on a missing layer is hidden"
        );
        let mut orphan = o;
        orphan.class = ClassId::from_num(42);
        assert!(
            !doc.is_visible(&orphan),
            "object with a missing class is hidden"
        );
    }

    // ---- regression tests for the review fixes ----

    #[test]
    fn arc_bounds_cover_endpoints_and_axis_extremes_inside_the_sweep() {
        let pi = std::f64::consts::PI;
        let arc = |start, end| {
            Shape::Arc {
                center: v(1.0, 2.0),
                radius: 10.0,
                start,
                end,
            }
            .bounds()
        };
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
        for (s, e) in [
            (0.3, 0.3 + 2.0 * pi),
            (0.3, 0.3 - 2.0 * pi),
            (0.0, 10.0 * pi),
        ] {
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
            Shape::Line {
                a: v(nan, 0.0),
                b: v(0.0, 0.0),
            },
            Shape::Rect {
                min: v(0.0, 0.0),
                max: v(0.0, f64::INFINITY),
            },
            Shape::Circle {
                center: v(0.0, 0.0),
                radius: nan,
            },
            Shape::Circle {
                center: v(nan, 0.0),
                radius: 1.0,
            },
            Shape::Arc {
                center: v(0.0, 0.0),
                radius: 1.0,
                start: nan,
                end: 1.0,
            },
            Shape::Arc {
                center: v(0.0, 0.0),
                radius: 1.0,
                start: 0.0,
                end: nan,
            },
            Shape::Arc {
                center: v(0.0, 0.0),
                radius: nan,
                start: 0.0,
                end: 1.0,
            },
            Shape::Polyline {
                points: crate::curve::corners(&[v(0.0, 0.0), v(nan, 0.0)]),
                closed: false,
            },
            Shape::Text {
                at: v(0.0, 0.0),
                text: "t".into(),
                height: nan,
            },
            Shape::Plant {
                at: v(0.0, 0.0),
                species: "s".into(),
                spread: 1.0,
                height: nan,
            },
            Shape::Plant {
                at: v(0.0, 0.0),
                species: "s".into(),
                spread: nan,
                height: 1.0,
            },
            Shape::LandscapeArea {
                boundary: crate::curve::corners(&[v(nan, 0.0)]),
                name: "n".into(),
            },
            Shape::Hardscape {
                boundary: crate::curve::corners(&[v(0.0, nan)]),
                material: "m".into(),
                thickness: None,
                elevation: None,
            },
        ];
        for s in bad {
            assert!(!s.is_finite(), "{s:?}");
        }
        assert!(
            Shape::Circle {
                center: v(1e308, -1e308),
                radius: 1e308
            }
            .is_finite()
        );
        let mut o = obj(Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0,
        });
        assert!(o.is_finite());
        o.attrs.line_weight = Some(nan);
        assert!(!o.is_finite());
    }

    fn doc_json() -> serde_json::Value {
        serde_json::to_value(Document::default()).unwrap()
    }

    /// The JSON text of the ID that has the small number `n`.
    fn u(n: u64) -> String {
        Uid::from_num(n).to_string()
    }

    #[test]
    fn from_json_loads_a_saved_document_and_ids_are_uuid_texts() {
        let mut doc = Document::default();
        let o = obj(Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0,
        });
        doc.objects.insert(o.id, o);
        doc.validate().unwrap();
        let text = serde_json::to_string(&doc).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["objects"][0]["id"], u(7));
        assert_eq!(v["objects"][0]["layer"], u(1));
        assert_eq!(v["active_layer"], u(1));
        assert!(
            v.get("next_id").is_none(),
            "there is no counter in the file"
        );
        assert_eq!(v["format_version"], FORMAT_VERSION);
        assert!(
            v["objects"][0].get("data").is_none(),
            "empty data and no parent are left out"
        );
        assert_eq!(Document::from_json(&text).unwrap(), doc);
    }

    #[test]
    fn loading_a_file_reads_the_pictures_once_and_refuses_a_false_hash() {
        use crate::resource::{Resource, ResourceBody, ResourceId};
        let mut doc = Document::default();
        let good = crate::picture::tests::sample(1);
        let mut bad = crate::picture::tests::sample(2);
        bad.sha256 = "0".repeat(64);
        for (n, p) in [("Good", good), ("Bad", bad)] {
            let id = ResourceId(doc.alloc_id());
            doc.resources.push(Resource {
                id,
                name: n.into(),
                body: ResourceBody::Image(Box::new(p)),
            });
            let text = serde_json::to_string(&doc).unwrap();
            let loaded = Document::from_json(&text);
            if n == "Good" {
                assert!(loaded.is_ok());
            } else {
                assert!(matches!(loaded, Err(LoadError::Invalid(m)) if m.contains("Bad")));
            }
        }
    }

    #[test]
    fn from_json_rejects_bad_input_with_a_clear_error() {
        assert!(matches!(
            Document::from_json("{nope"),
            Err(LoadError::Parse(_))
        ));
        assert!(
            matches!(Document::from_json("[]"), Err(LoadError::Parse(m)) if m.contains("format_version"))
        );
        // Files of an older format are refused with a clear message.
        let e = Document::from_json(r#"{"format_version":1}"#).unwrap_err();
        assert_eq!(e, LoadError::Unsupported(1));
        assert!(e.to_string().contains("unsupported old format 1"), "{e}");
        assert_eq!(
            Document::from_json(r#"{"format_version":2,"layers":"???"}"#),
            Err(LoadError::Unsupported(2))
        );

        let mut v = doc_json();
        v["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        let e = Document::from_value(v.clone()).unwrap_err();
        assert_eq!(e, LoadError::Newer(FORMAT_VERSION + 1));
        assert_eq!(
            e.to_string(),
            format!(
                "file was made by a newer Kadakas (format {})",
                FORMAT_VERSION + 1
            )
        );
        // A newer file may have a different shape. It is refused before the shape is read.
        assert_eq!(
            Document::from_json(r#"{"format_version":99,"layers":"???"}"#),
            Err(LoadError::Newer(99))
        );
        assert_eq!(
            Document::from_json(r#"{"format_version":99999999999}"#),
            Err(LoadError::Newer(u32::MAX))
        );

        v["format_version"] = serde_json::json!(0);
        assert_eq!(Document::from_value(v), Err(LoadError::Unsupported(0)));

        let mut v = doc_json();
        v["objects"] = serde_json::json!([{"id": u(5), "layer": u(99), "class": u(0), "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        assert!(
            matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains("missing layer"))
        );
        let mut v = doc_json();
        v["active_layer"] = serde_json::json!(u(77));
        assert!(matches!(
            Document::from_value(v),
            Err(LoadError::Invalid(_))
        ));
        let mut v = doc_json();
        v["layers"] = serde_json::json!([]);
        assert!(
            matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains("no layers"))
        );
    }

    #[test]
    fn from_json_rejects_duplicate_ids_and_bad_shapes() {
        let circle = |id: u64| serde_json::json!({"id": u(id), "layer": u(1), "class": u(0), "shape": {"kind": "circle", "center": [0, 0], "radius": 1}});
        let mut v = doc_json();
        v["objects"] = serde_json::json!([circle(5), circle(5)]);
        assert!(
            matches!(Document::from_value(v), Err(LoadError::Parse(m)) if m.contains(&format!("duplicate id {}", u(5))))
        );
        // An object may not reuse the ID of a layer.
        let mut v = doc_json();
        v["objects"] = serde_json::json!([circle(1)]);
        assert!(
            matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains(&format!("duplicate id {}", u(1))))
        );

        let mut v = doc_json();
        v["objects"] = serde_json::json!([{"id": u(5), "layer": u(1), "class": u(0), "shape": {"kind": "arc", "center": [0, 0], "radius": 1, "start": 1, "end": 1}}]);
        assert!(
            matches!(Document::from_value(v), Err(LoadError::Invalid(m)) if m.contains(&format!("object {} has an invalid shape", u(5))))
        );
    }

    #[test]
    fn a_reader_loads_shapes_of_every_json_kind_and_pictures_in_any_key_order() {
        let obj = |shape: serde_json::Value| serde_json::json!({"id": u(5), "layer": u(1), "class": u(0), "shape": shape});
        // A shape that is no object is an error, never a panic.
        for bad in [
            serde_json::json!(5),
            serde_json::json!(-5),
            serde_json::json!(1.5),
            serde_json::json!(true),
            serde_json::json!(null),
            serde_json::json!("circle"),
            serde_json::json!([1, 2]),
            serde_json::json!({"kind": "circle"}),
        ] {
            let mut v = doc_json();
            v["objects"] = serde_json::json!([obj(bad.clone())]);
            let text = v.to_string();
            let e = Document::from_json(&text).unwrap_err();
            assert!(matches!(e, LoadError::Parse(_)), "{bad}: {e:?}");
        }
        // The objects are an array.
        let mut v = doc_json();
        v["objects"] = serde_json::json!({"a": 1});
        assert!(matches!(
            Document::from_json(&v.to_string()),
            Err(LoadError::Parse(_))
        ));
        // A picture loads whether `png` comes before `kind` or after it, and a bad one is refused.
        let png = base64_for(&[0x89, b'P', b'N', b'G', 1, 2, 3, 4, 5]);
        let after = format!(
            r#"{{"kind":"underlay","name":"n","min":[0,0],"max":[1,1],"width":1,"height":1,"png":"{png}"}}"#
        );
        let before = format!(
            r#"{{"png":"{png}","kind":"underlay","name":"n","min":[0,0],"max":[1,1],"width":1,"height":1}}"#
        );
        let load = |shape: &str| {
            let mut v = doc_json();
            v["objects"] = serde_json::json!([]);
            let text = v.to_string().replace(
                r#""objects":[]"#,
                &format!(
                    r#""objects":[{{"id":"{}","layer":"{}","class":"{}","shape":{shape}}}]"#,
                    u(5),
                    u(1),
                    u(0)
                ),
            );
            Document::from_json(&text)
        };
        let a = load(&after).unwrap();
        let b = load(&before).unwrap();
        assert_eq!(a, b);
        assert!(matches!(
            &a.objects.values().next().unwrap().shape,
            Shape::Underlay(x) if x.png.0.len() == 9
        ));
        let broken = after.replace(&png, "!!!!");
        let e = load(&broken).unwrap_err();
        assert!(e.to_string().contains("not valid base64"), "{e}");
        let e = load(&broken.replace("\"!!!!\"", "7")).unwrap_err();
        assert!(matches!(e, LoadError::Parse(_)));
    }

    fn base64_for(bytes: &[u8]) -> String {
        crate::underlay::base64_encode(bytes)
    }

    #[test]
    fn ids_must_be_uuid_texts_and_a_number_is_refused() {
        let mut v = doc_json();
        v["objects"] = serde_json::json!([{"id": 5, "layer": u(1), "class": u(0), "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        let e = Document::from_value(v).unwrap_err();
        assert!(
            matches!(&e, LoadError::Parse(m) if m.contains("UUID")),
            "{e}"
        );
        let mut v = doc_json();
        v["objects"] = serde_json::json!([{"id": "nope", "layer": u(1), "class": u(0), "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        assert!(matches!(Document::from_value(v), Err(LoadError::Parse(_))));
    }

    #[test]
    fn a_loaded_document_makes_new_ids_that_never_collide() {
        let mut v = doc_json();
        v["objects"] = serde_json::json!([{"id": u(40), "layer": u(1), "class": u(0), "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        let mut doc = Document::from_value(v).unwrap();
        let id = doc.alloc_id();
        assert!(!doc.objects.contains_key(&ObjectId(id)));
        assert_eq!(id.version(), 7);
    }

    #[test]
    fn old_files_without_new_fields_load_with_defaults() {
        let mut v = doc_json();
        v["layers"][0].as_object_mut().unwrap().remove("kind");
        v["objects"] = serde_json::json!([{"id": u(5), "layer": u(1), "class": u(0), "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        let doc = Document::from_value(v).unwrap();
        assert_eq!(doc.layers[0].kind, LayerKind::Design);
        let o = doc.object(ObjectId::from_num(5)).unwrap();
        assert!(o.data.is_empty() && o.parent.is_none());
    }

    #[test]
    fn data_parent_and_layer_kind_round_trip() {
        let mut doc = Document::default();
        doc.layers[0].kind = LayerKind::Sheet;
        let mut o = obj(Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.0,
        });
        o.data.insert(
            "Pset_Wall".into(),
            serde_json::json!({"FireRating": "REI60", "Load": 2.5}),
        );
        o.parent = Some(ObjectId::from_num(99));
        doc.objects.insert(o.id, o);
        // The parent must be a group that exists.
        assert!(doc.validate().unwrap_err().contains("does not exist"));
        doc.objects.insert(
            ObjectId::from_num(99),
            Object::new(
                ObjectId::from_num(99),
                FIRST_LAYER,
                NONE_CLASS,
                Shape::Group { name: "g".into() },
            ),
        );
        doc.validate().unwrap();
        let text = serde_json::to_string(&doc).unwrap();
        assert!(
            text.contains(r#""kind":"sheet""#)
                && text.contains("Pset_Wall")
                && text.contains(&format!(r#""parent":"{}""#, u(99)))
        );
        assert_eq!(Document::from_json(&text).unwrap(), doc);
    }

    #[test]
    fn document_equality_includes_z_order() {
        let mut a = Document::default();
        let mk = |id| {
            Object::new(
                ObjectId(id),
                FIRST_LAYER,
                NONE_CLASS,
                Shape::Circle {
                    center: v(0.0, 0.0),
                    radius: 1.0,
                },
            )
        };
        a.objects
            .insert(ObjectId::from_num(10), mk(crate::uid::Uid::from_num(10)));
        a.objects
            .insert(ObjectId::from_num(11), mk(crate::uid::Uid::from_num(11)));
        let mut b = Document::default();
        b.objects
            .insert(ObjectId::from_num(11), mk(crate::uid::Uid::from_num(11)));
        b.objects
            .insert(ObjectId::from_num(10), mk(crate::uid::Uid::from_num(10)));
        assert_ne!(a, b);
        b.objects.move_index(1, 0);
        assert_eq!(a, b);
        b.objects.pop();
        assert_ne!(a, b);
    }

    #[test]
    fn plain_points_load_and_curved_vertices_round_trip() {
        let old = format!(
            r#"{{"format_version":{FORMAT_VERSION},"layers":[{{"id":"{l}","name":"L","visible":true,"locked":false,"elevation":0,"scale":100}}],
            "classes":[{{"id":"{c}","name":"None","visible":true,"attrs":{{}}}}],
            "objects":[{{"id":"{o2}","layer":"{l}","class":"{c}","shape":{{"kind":"hardscape","boundary":[[0,0],[4,0],[4,3]],"material":"m"}}}},
                       {{"id":"{o3}","layer":"{l}","class":"{c}","shape":{{"kind":"polyline","points":[[0,0],[1,1]],"closed":false}}}}],
            "active_layer":"{l}","active_class":"{c}"}}"#,
            l = u(1),
            c = u(0),
            o2 = u(2),
            o3 = u(3)
        );
        let doc = Document::from_json(&old).unwrap();
        assert_eq!(
            doc.object(ObjectId::from_num(2)).unwrap().shape.area(),
            Some(6.0)
        );
        // Saving writes corners as plain points, so old readers' format is kept for old content.
        let text = serde_json::to_string(&doc).unwrap();
        assert!(
            text.contains(r#""boundary":[[0.0,0.0],[4.0,0.0],[4.0,3.0]]"#),
            "{text}"
        );
        // Curved vertices round-trip, and a negative radius is refused on load.
        let curved = old.replace(
            "[[0,0],[1,1]]",
            r#"[[0,0],{"p":[1,1],"kind":"arc","radius":5},[3,0]]"#,
        );
        let doc = Document::from_json(&curved).unwrap();
        assert_eq!(
            Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap(),
            doc
        );
        let negative = curved.replace(r#""radius":5"#, r#""radius":-5"#);
        assert!(Document::from_json(&negative).is_err());
    }

    #[test]
    fn curved_bounds_and_area() {
        let arc = |x: f64, y: f64| Vertex::new(v(x, y), VertexKind::Arc { radius: 10.0 });
        let rounded = Shape::Hardscape {
            boundary: vec![
                arc(0.0, 0.0),
                arc(100.0, 0.0),
                arc(100.0, 100.0),
                arc(0.0, 100.0),
            ],
            material: "m".into(),
            thickness: None,
            elevation: None,
        };
        // The fillets cut the corners, so the box is still the square (the sides touch it).
        let (lo, hi) = rounded.bounds();
        assert_vec_close(lo, v(0.0, 0.0));
        assert_vec_close(hi, v(100.0, 100.0));
        let expected = 100.0 * 100.0 - (4.0 - std::f64::consts::PI) * 100.0;
        assert!((rounded.area().unwrap() - expected).abs() < 1e-6 * expected);
        // A bulging point on an arc pushes the box out.
        let bulge = Shape::Polyline {
            points: vec![
                Vertex::corner(v(-10.0, 0.0)),
                Vertex::new(v(0.0, 10.0), VertexKind::PointOnArc),
                Vertex::corner(v(10.0, 0.0)),
            ],
            closed: false,
        };
        let (lo, hi) = bulge.bounds();
        assert!((hi.y - 10.0).abs() < 1e-9 && lo.y == 0.0);
    }

    #[test]
    fn existing_tree_and_plant_tag_basics() {
        use crate::plants::tree::{TreeAction, TreeCondition};
        let tree = Shape::ExistingTree {
            at: v(10.0, 10.0),
            species: "Oak".into(),
            dbh: 500.0,
            spread: 8000.0,
            height: 12000.0,
            condition: TreeCondition::Poor,
            action: TreeAction::Relocate,
            rpz_factor: None,
            rpz_cap: None,
        };
        let tag = Shape::PlantTag {
            at: v(0.0, 100.0),
            anchor: v(-50.0, 0.0),
            style: ResourceId::from_num(3),
            height: 400.0,
        };
        assert_eq!((tree.kind(), tag.kind()), ("existing_tree", "plant_tag"));
        // The root protection zone (6000 mm) is larger than the canopy radius (4000 mm).
        assert_eq!(tree.bounds(), (v(-5990.0, -5990.0), v(6010.0, 6010.0)));
        let (lo, hi) = tag.bounds();
        assert_eq!((lo, hi), (v(-50.0, 0.0), v(1920.0, 500.0)));
        assert!(tree.is_finite() && tag.is_finite() && tree.is_in_range() && tag.is_in_range());
        let mut moved = tree.clone();
        moved.translate(v(1.0, 2.0));
        assert_eq!(moved.bounds().0, v(-5989.0, -5988.0));
        let mut moved = tag.clone();
        moved.translate(v(1.0, 2.0));
        assert!(
            matches!(moved, Shape::PlantTag { at, anchor, .. } if at == v(1.0, 102.0) && anchor == v(-49.0, 2.0))
        );
        assert_eq!(tree.area(), None);
        let odd = |dbh: f64, factor: Option<f64>| Shape::ExistingTree {
            at: v(0.0, 0.0),
            species: String::new(),
            dbh,
            spread: 1.0,
            height: 1.0,
            condition: Default::default(),
            action: Default::default(),
            rpz_factor: factor,
            rpz_cap: None,
        };
        assert!(!odd(f64::NAN, None).is_finite());
        assert!(!odd(1.0, Some(f64::INFINITY)).is_finite());
        assert!(!odd(1.0, Some(2.0 * MAX_COORD)).is_in_range());
        assert!(
            !Shape::PlantTag {
                at: v(0.0, 0.0),
                anchor: v(f64::NAN, 0.0),
                style: ResourceId::from_num(1),
                height: 1.0
            }
            .is_finite()
        );
        assert!(
            !Shape::PlantTag {
                at: v(0.0, 0.0),
                anchor: v(2.0 * MAX_COORD, 0.0),
                style: ResourceId::from_num(1),
                height: 1.0
            }
            .is_in_range()
        );
        // JSON: short names, defaults for the optional fields.
        let j = serde_json::json!({"kind": "existing_tree", "at": [1, 2], "dbh": 300, "spread": 5000, "height": 9000});
        let back: Shape = serde_json::from_value(j).unwrap();
        assert!(matches!(
            back,
            Shape::ExistingTree {
                condition: TreeCondition::Fair,
                action: TreeAction::Retain,
                rpz_factor: None,
                ..
            }
        ));
        let j = serde_json::to_value(&tree).unwrap();
        assert_eq!(j["action"], "relocate");
        assert_eq!(serde_json::from_value::<Shape>(j).unwrap(), tree);
        let j =
            serde_json::json!({"kind": "plant_tag", "at": [0, 0], "anchor": [1, 1], "style": u(4)});
        assert!(
            matches!(serde_json::from_value::<Shape>(j).unwrap(), Shape::PlantTag { height, .. } if height == DEFAULT_TAG_HEIGHT)
        );
    }

    #[test]
    fn files_without_resources_load_and_plants_keep_their_fields() {
        // A file with no `resources` key, and a plant with its own fields.
        let old = serde_json::json!({
            "format_version": FORMAT_VERSION,
            "layers": [{"id": u(1), "name": "Layer 1", "visible": true, "locked": false, "elevation": 0.0, "scale": 100.0}],
            "classes": [{"id": u(0), "name": "None", "visible": true, "attrs": {}}],
            "objects": [{"id": u(5), "layer": u(1), "class": u(0), "shape": {"kind": "plant", "at": [1.0, 2.0], "species": "Quercus robur", "spread": 6000.0, "height": 12000.0}}],
            "active_layer": u(1),
            "active_class": u(0)
        });
        let doc = Document::from_value(old).unwrap();
        assert!(doc.resources.is_empty());
        let e = crate::plants::link::effective(&doc, doc.object(ObjectId::from_num(5)).unwrap())
            .unwrap();
        assert_eq!(
            (
                e.style_id,
                e.style.botanical_name.as_str(),
                e.style.spread,
                e.style.height
            ),
            (None, "Quercus robur", 6000.0, 12000.0)
        );
        // Saving a document without resources writes no `resources` key.
        assert!(
            serde_json::to_value(&doc)
                .unwrap()
                .get("resources")
                .is_none()
        );
    }

    #[test]
    fn resources_are_saved_loaded_and_checked() {
        use crate::command::{Command, Editor};
        use crate::resource::ResourceBody;
        let mut ed = Editor::default();
        let id = ed
            .execute(&Command::AddResource {
                name: "Oak".into(),
                body: ResourceBody::plant_style(Default::default()),
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::AddPlants {
            style: Some(ResourceId(id)),
            overrides: Default::default(),
            positions: vec![v(0.0, 0.0)],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        let text = serde_json::to_string(ed.doc()).unwrap();
        let back = Document::from_json(&text).unwrap();
        assert_eq!(&back, ed.doc());
        // The ID of a resource is unique among all IDs.
        let mut j: Value = serde_json::from_str(&text).unwrap();
        j["resources"][0]["id"] = serde_json::json!(u(1));
        assert!(
            Document::from_value(j.clone())
                .unwrap_err()
                .to_string()
                .contains("duplicate id")
        );
        j["resources"][0]["id"] = serde_json::json!(u(40));
        let mut loaded = Document::from_value(j).unwrap();
        let fresh = loaded.alloc_id();
        assert!(!loaded.resources.iter().any(|r| r.id.0 == fresh));
        // A bad style stops the load.
        let mut j: Value = serde_json::from_str(&text).unwrap();
        j["resources"][0]["body"]["spread"] = serde_json::json!(-1.0);
        assert!(matches!(
            Document::from_value(j),
            Err(LoadError::Invalid(_))
        ));
    }
}
