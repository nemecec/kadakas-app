//! What the 3D modeler (`kadakas-modeler`, PLAT-11, MOD-01) needs besides navigation: its tool bar
//! (the Getting Started set), the state of the active tool, and the data of the Entity Info panel.
//!
//! The module is pure and has no egui. The modeler draws the tool bar from [`ModelerTool::GROUPS`]
//! and asks [`ToolState`] what a click does. Only the tools for which [`ModelerTool::works`] is
//! true run today (Select and the navigation tools); the others are in the tool bar, disabled, with
//! a tooltip. Each later row (MOD-03 to MOD-05, MOD-08) turns tools on by changing `works`.

use kadakas_core::mesh::Scene;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::resource::ResourceId;
use kadakas_l10n::{msg, tr};

use crate::keymap::Action;

/// A tool or command of the tool bar, in the order of the tool bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelerTool {
    Select,
    Eraser,
    Line,
    Rectangle,
    Circle,
    Arc,
    PushPull,
    Move,
    Rotate,
    Scale,
    TapeMeasure,
    PaintBucket,
    Orbit,
    Pan,
    Zoom,
    /// A command, not a tool: it fits the view at once and the active tool stays.
    ZoomExtents,
}

impl ModelerTool {
    pub const ALL: [ModelerTool; 16] = [
        ModelerTool::Select,
        ModelerTool::Eraser,
        ModelerTool::Line,
        ModelerTool::Rectangle,
        ModelerTool::Circle,
        ModelerTool::Arc,
        ModelerTool::PushPull,
        ModelerTool::Move,
        ModelerTool::Rotate,
        ModelerTool::Scale,
        ModelerTool::TapeMeasure,
        ModelerTool::PaintBucket,
        ModelerTool::Orbit,
        ModelerTool::Pan,
        ModelerTool::Zoom,
        ModelerTool::ZoomExtents,
    ];

    /// The groups of the tool bar, with a gap between the groups: select and edit, draw, modify
    /// and measure, navigate.
    pub const GROUPS: [&'static [ModelerTool]; 4] = [
        &[
            ModelerTool::Select,
            ModelerTool::Eraser,
            ModelerTool::PaintBucket,
        ],
        &[
            ModelerTool::Line,
            ModelerTool::Rectangle,
            ModelerTool::Circle,
            ModelerTool::Arc,
        ],
        &[
            ModelerTool::PushPull,
            ModelerTool::Move,
            ModelerTool::Rotate,
            ModelerTool::Scale,
            ModelerTool::TapeMeasure,
        ],
        &[
            ModelerTool::Orbit,
            ModelerTool::Pan,
            ModelerTool::Zoom,
            ModelerTool::ZoomExtents,
        ],
    ];

    /// A stable name, for tests and for the icons.
    pub fn id(self) -> &'static str {
        match self {
            ModelerTool::Select => "select",
            ModelerTool::Eraser => "eraser",
            ModelerTool::Line => "line",
            ModelerTool::Rectangle => "rectangle",
            ModelerTool::Circle => "circle",
            ModelerTool::Arc => "arc",
            ModelerTool::PushPull => "push-pull",
            ModelerTool::Move => "move",
            ModelerTool::Rotate => "rotate",
            ModelerTool::Scale => "scale",
            ModelerTool::TapeMeasure => "tape-measure",
            ModelerTool::PaintBucket => "paint-bucket",
            ModelerTool::Orbit => "orbit",
            ModelerTool::Pan => "pan",
            ModelerTool::Zoom => "zoom",
            ModelerTool::ZoomExtents => "zoom-extents",
        }
    }

    /// The message id of the name.
    pub fn name_id(self) -> &'static str {
        match self {
            ModelerTool::Select => msg!("uic-modeler-tool-select"),
            ModelerTool::Eraser => msg!("uic-modeler-tool-eraser"),
            ModelerTool::Line => msg!("uic-modeler-tool-line"),
            ModelerTool::Rectangle => msg!("uic-modeler-tool-rectangle"),
            ModelerTool::Circle => msg!("uic-modeler-tool-circle"),
            ModelerTool::Arc => msg!("uic-modeler-tool-arc"),
            ModelerTool::PushPull => msg!("uic-modeler-tool-push-pull"),
            ModelerTool::Move => msg!("uic-modeler-tool-move"),
            ModelerTool::Rotate => msg!("uic-modeler-tool-rotate"),
            ModelerTool::Scale => msg!("uic-modeler-tool-scale"),
            ModelerTool::TapeMeasure => msg!("uic-modeler-tool-tape-measure"),
            ModelerTool::PaintBucket => msg!("uic-modeler-tool-paint-bucket"),
            ModelerTool::Orbit => msg!("uic-modeler-tool-orbit"),
            ModelerTool::Pan => msg!("uic-modeler-tool-pan"),
            ModelerTool::Zoom => msg!("uic-modeler-tool-zoom"),
            ModelerTool::ZoomExtents => msg!("uic-modeler-tool-zoom-extents"),
        }
    }

    /// The name in the language of the UI.
    pub fn name(self) -> String {
        kadakas_l10n::translate(self.name_id(), &[])
    }

    /// The key action that chooses the tool. Its chord is in the profile.
    pub fn action(self) -> Action {
        match self {
            ModelerTool::Select => Action::ToolSelect,
            ModelerTool::Eraser => Action::ToolEraser,
            ModelerTool::Line => Action::ToolLine,
            ModelerTool::Rectangle => Action::ToolRect,
            ModelerTool::Circle => Action::ToolCircle,
            ModelerTool::Arc => Action::ToolArc,
            ModelerTool::PushPull => Action::ToolPushPull,
            ModelerTool::Move => Action::ToolMove,
            ModelerTool::Rotate => Action::ToolRotate,
            ModelerTool::Scale => Action::ToolScale,
            ModelerTool::TapeMeasure => Action::ToolTapeMeasure,
            ModelerTool::PaintBucket => Action::ToolPaintBucket,
            ModelerTool::Orbit => Action::NavOrbit,
            ModelerTool::Pan => Action::NavPan,
            ModelerTool::Zoom => Action::NavZoom,
            ModelerTool::ZoomExtents => Action::ZoomFit,
        }
    }

    /// The tool that a key action chooses.
    pub fn from_action(action: Action) -> Option<ModelerTool> {
        ModelerTool::ALL
            .iter()
            .copied()
            .find(|t| t.action() == action)
    }

    /// True for the tools that run in this version: Select, the drawing tools of MOD-08 (Eraser,
    /// Line, Rectangle, Circle, Arc, Push/Pull), Move, Rotate and Scale and the Paint Bucket (MOD-05) and the
    /// navigation tools. Tape Measure is shown, disabled.
    pub fn works(self) -> bool {
        matches!(
            self,
            ModelerTool::Select
                | ModelerTool::Eraser
                | ModelerTool::Line
                | ModelerTool::Rectangle
                | ModelerTool::Circle
                | ModelerTool::Arc
                | ModelerTool::PushPull
                | ModelerTool::Move
                | ModelerTool::Rotate
                | ModelerTool::Scale
                | ModelerTool::PaintBucket
                | ModelerTool::Orbit
                | ModelerTool::Pan
                | ModelerTool::Zoom
                | ModelerTool::ZoomExtents
        )
    }

    /// True for the navigation tools and commands.
    pub fn is_navigation(self) -> bool {
        matches!(
            self,
            ModelerTool::Orbit | ModelerTool::Pan | ModelerTool::Zoom | ModelerTool::ZoomExtents
        )
    }

    /// True for a command that runs at once and is never the active tool.
    pub fn is_command(self) -> bool {
        self == ModelerTool::ZoomExtents
    }

    /// The line for the status bar while the tool is active: what a click and a drag do. A tool
    /// that does not run yet says so.
    pub fn hint(self) -> String {
        match self {
            ModelerTool::Select => tr!("uic-modeler-hint-select"),
            ModelerTool::Eraser => tr!("uic-modeler-hint-eraser"),
            ModelerTool::Line => tr!("uic-modeler-hint-line"),
            ModelerTool::Rectangle => tr!("uic-modeler-hint-rectangle"),
            ModelerTool::Circle => tr!("uic-modeler-hint-circle"),
            ModelerTool::Arc => tr!("uic-modeler-hint-arc"),
            ModelerTool::PushPull => tr!("uic-modeler-hint-push-pull"),
            ModelerTool::Move => tr!("uic-modeler-hint-move"),
            ModelerTool::Rotate => tr!("uic-modeler-hint-rotate"),
            ModelerTool::Scale => tr!("uic-modeler-hint-scale"),
            ModelerTool::PaintBucket => tr!("uic-modeler-hint-paint-bucket"),
            ModelerTool::Orbit => tr!("uic-modeler-hint-orbit"),
            ModelerTool::Pan => tr!("uic-modeler-hint-pan"),
            ModelerTool::Zoom => tr!("uic-modeler-hint-zoom"),
            ModelerTool::ZoomExtents => tr!("uic-modeler-hint-zoom-extents"),
            _ => tr!("uic-modeler-coming-later"),
        }
    }

    /// The tooltip: the name and the key, and for a tool that does not run yet, the note.
    pub fn tooltip(self, key_hint: &str) -> String {
        let name = if key_hint.is_empty() {
            self.name()
        } else {
            tr!(
                "uic-modeler-tool-with-key",
                name = self.name(),
                key = key_hint
            )
        };
        if self.works() {
            name
        } else {
            tr!(
                "uic-modeler-tool-later",
                name = name,
                note = tr!("uic-modeler-coming-later")
            )
        }
    }
}

/// The active tool, and what is armed for the next drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolState {
    active: ModelerTool,
    /// Zoom Window was asked for: the next drag of the Zoom tool is a window.
    window_armed: bool,
}

impl Default for ToolState {
    fn default() -> Self {
        ToolState {
            active: ModelerTool::Select,
            window_armed: false,
        }
    }
}

impl ToolState {
    pub fn active(&self) -> ModelerTool {
        self.active
    }

    /// Chooses a tool. A tool that does not run yet is refused (false), and a command never
    /// becomes the active tool (the caller runs it). Choosing a tool ends an armed window.
    pub fn choose(&mut self, tool: ModelerTool) -> bool {
        if !tool.works() || tool.is_command() {
            return false;
        }
        self.active = tool;
        self.window_armed = false;
        true
    }

    /// Back to Select (Space, Escape).
    pub fn select(&mut self) {
        self.active = ModelerTool::Select;
        self.window_armed = false;
    }

    /// Zoom Window: the Zoom tool, and the next drag draws a window.
    pub fn arm_zoom_window(&mut self) {
        self.active = ModelerTool::Zoom;
        self.window_armed = true;
    }

    pub fn window_armed(&self) -> bool {
        self.window_armed
    }

    /// The window was used (or cancelled).
    pub fn disarm_window(&mut self) {
        self.window_armed = false;
    }
}

/// What the Entity Info panel shows for the selection. Sizes and positions are in mm.
#[derive(Clone, Debug, PartialEq)]
pub enum EntityInfo {
    /// Nothing is selected: the active layer and class.
    Nothing { layer: String, class: String },
    /// One object.
    One {
        id: String,
        /// The API name of the kind (`hardscape`, `plant`).
        kind: String,
        layer: String,
        class: String,
        /// The size of the box around its geometry.
        size: Option<[f64; 3]>,
        /// The lowest corner of that box.
        min: Option<[f64; 3]>,
        /// The counts of a surface model.
        surface: Option<SurfaceStats>,
        /// Where the object is, how it is turned and how large its scale is (read only).
        placement: Option<Placement>,
        /// What the object is, for the kinds that come from the Vectorworks-like app (plants,
        /// hardscapes, site models and the like).
        summary: Option<Box<Summary>>,
        /// The material the object has as a whole (Paint Bucket), by name.
        material: Option<String>,
    },
    /// One face of a surface model.
    Face {
        model: String,
        id: u32,
        /// Area in mm².
        area: f64,
        /// The front normal in the drawing.
        normal: [f64; 3],
        /// Edges of the outer loop and of the holes together.
        edges: usize,
        holes: usize,
        /// The model object, for the changes that Entity Info makes.
        model_object: ObjectId,
        /// The material of the front and of the back (Paint Bucket), if the face has one.
        front: Option<ResourceId>,
        back: Option<ResourceId>,
    },
    /// One edge of a surface model.
    Edge {
        model: String,
        id: u32,
        /// Length in mm.
        length: f64,
        start: [f64; 3],
        end: [f64; 3],
        /// The curve (circle or arc) it belongs to, and the edges of that curve.
        curve: Option<(u32, usize)>,
        /// How many faces use it.
        faces: usize,
    },
    /// Several faces and edges of surface models.
    Parts { faces: usize, edges: usize },
    /// Several objects.
    Many {
        count: usize,
        size: Option<[f64; 3]>,
    },
}

/// The position, rotation and scale of an object, for Entity Info (MOD-05). The position is the
/// origin of the object in the world (mm), the rotation the turn about the x, y and z axes in
/// degrees (the x axis first), the scale the length of each axis of the object (1 for none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub position: [f64; 3],
    pub rotation_deg: [f64; 3],
    pub scale: [f64; 3],
    /// The axes of the object are not at right angles (a non-uniform scale after a turn): the
    /// rotation then has no single meaning and Entity Info says "skewed".
    pub skewed: bool,
}

impl Placement {
    /// The placement of a transform from the local coordinates of an object to the world.
    pub fn of_transform(t: &glam::DAffine3) -> Placement {
        let m = t.matrix3;
        let scale = glam::DVec3::new(m.x_axis.length(), m.y_axis.length(), m.z_axis.length());
        // A mirror shows as a negative scale of the x axis.
        let mirror = m.determinant() < 0.0;
        let sx = if mirror { -scale.x } else { scale.x };
        let unit = |v: glam::DVec3| v.try_normalize().unwrap_or(glam::DVec3::ZERO);
        let (ux, uy, uz) = (unit(m.x_axis), unit(m.y_axis), unit(m.z_axis));
        let skewed = ux.dot(uy).abs() > 1e-6 || ux.dot(uz).abs() > 1e-6 || uy.dot(uz).abs() > 1e-6;
        // The rotation of the axes made orthonormal (the first axis is kept).
        let x = if mirror { -ux } else { ux };
        let y = (uy - x * x.dot(uy))
            .try_normalize()
            .unwrap_or(glam::DVec3::Y);
        let z = x.cross(y);
        let rot = glam::DMat3::from_cols(x, y, z);
        let (ex, ey, ez) = glam::DQuat::from_mat3(&rot).to_euler(glam::EulerRot::XYZ);
        let clean = |a: f64| {
            let d = a.to_degrees();
            // Rounding noise of a turn that is none.
            if d.abs() < 1e-9 { 0.0 } else { d }
        };
        Placement {
            position: t.translation.to_array(),
            rotation_deg: [clean(ex), clean(ey), clean(ez)],
            scale: [sx, scale.y, scale.z],
            skewed,
        }
    }
}

/// The placement of an object that has a transform of its own (a surface model, a component
/// instance) or a position and a turn (a plant).
fn placement_of(doc: &Document, obj: &kadakas_core::model::Object) -> Option<Placement> {
    use kadakas_core::model::Shape;
    let elevation = doc.layer(obj.layer).map_or(0.0, |l| l.elevation);
    let lift = glam::DAffine3::from_translation(glam::DVec3::new(0.0, 0.0, elevation));
    match &obj.shape {
        Shape::SurfaceModel(m) => Some(Placement::of_transform(&(lift * m.transform))),
        Shape::Component { transform, .. } => Some(Placement::of_transform(
            &(lift * kadakas_core::component::affine(transform)),
        )),
        Shape::Plant { at, .. } => Some(Placement {
            position: [at.x, at.y, elevation],
            rotation_deg: [
                0.0,
                0.0,
                kadakas_core::plants::link::rotation(obj).to_degrees(),
            ],
            scale: [1.0, 1.0, 1.0],
            skewed: false,
        }),
        _ => None,
    }
}

/// What Entity Info says about an object that the modeler shows, selects and moves but that the
/// Vectorworks-like app edits (MOD-05, round B). Lengths are in mm, areas in mm squared.
#[derive(Clone, Debug, PartialEq)]
pub enum Summary {
    Plant {
        species: String,
        /// The name of the plant style, if the plant has one.
        style: Option<String>,
        height: f64,
        spread: f64,
    },
    ExistingTree {
        species: String,
        height: f64,
        spread: f64,
        /// Trunk diameter.
        dbh: f64,
    },
    Hardscape {
        area: f64,
        /// The name of the hardscape style.
        style: Option<String>,
        material: String,
    },
    LandscapeArea {
        name: String,
        area: f64,
        style: Option<String>,
    },
    SiteModel {
        points: usize,
        lines: usize,
        /// Width, depth and height of the box around the survey data.
        extent: Option<[f64; 3]>,
        /// Size of one repeat of the ground texture, with the texture look.
        tile: Option<f64>,
    },
    /// A symbol instance (2D or 3D).
    Symbol {
        name: String,
    },
    Group {
        members: usize,
    },
    /// A component instance.
    Component {
        name: String,
    },
}

impl Summary {
    /// True for the kinds that only the Kadakas app edits: the modeler moves, turns and scales
    /// them and offers "Open in the Kadakas app" for the rest.
    pub fn needs_app(&self) -> bool {
        !matches!(self, Summary::Group { .. } | Summary::Component { .. })
    }
}

fn resource_name(doc: &Document, id: Option<ResourceId>) -> Option<String> {
    id.and_then(|i| doc.resources.get(i))
        .map(|r| r.name.clone())
}

/// The summary of an object, for the kinds that have one.
pub fn summary_of(doc: &Document, obj: &kadakas_core::model::Object) -> Option<Summary> {
    use kadakas_core::model::Shape;
    Some(match &obj.shape {
        Shape::Plant {
            species,
            spread,
            height,
            ..
        } => Summary::Plant {
            species: species.clone(),
            style: resource_name(doc, kadakas_core::plants::link::style_of(obj)),
            height: *height,
            spread: *spread,
        },
        Shape::ExistingTree {
            species,
            dbh,
            spread,
            height,
            ..
        } => Summary::ExistingTree {
            species: species.clone(),
            height: *height,
            spread: *spread,
            dbh: *dbh,
        },
        Shape::Hardscape {
            boundary, material, ..
        } => Summary::Hardscape {
            area: kadakas_core::curve::area(boundary).abs(),
            style: resource_name(doc, kadakas_core::hardscape::style_id(obj)),
            material: material.clone(),
        },
        Shape::LandscapeArea { boundary, name } => Summary::LandscapeArea {
            name: name.clone(),
            area: kadakas_core::curve::area(boundary).abs(),
            style: resource_name(
                doc,
                kadakas_core::landscape::data(obj).and_then(|d| d.style),
            ),
        },
        Shape::SiteModel {
            points,
            lines,
            settings,
        } => {
            let all = points
                .iter()
                .copied()
                .chain(lines.iter().flat_map(|l| l.points.iter().copied()));
            let (lo, hi) = all.fold(None, |acc: Option<(glam::DVec3, glam::DVec3)>, p| {
                Some(match acc {
                    None => (p, p),
                    Some((a, b)) => (a.min(p), b.max(p)),
                })
            })?;
            let d = hi - lo;
            Summary::SiteModel {
                points: points.len(),
                lines: lines.len(),
                extent: Some([d.x, d.y, d.z]),
                tile: (settings.surface_look == kadakas_core::site::SurfaceLook::Texture)
                    .then_some(settings.texture_tile),
            }
        }
        Shape::Symbol { symbol, .. } => Summary::Symbol {
            name: resource_name(doc, Some(*symbol)).unwrap_or_default(),
        },
        Shape::Group { .. } => Summary::Group {
            members: doc.children(obj.id).len(),
        },
        Shape::Component { symbol, .. } => Summary::Component {
            name: resource_name(doc, Some(*symbol)).unwrap_or_default(),
        },
        _ => return None,
    })
}

/// The counts of a surface model, for Entity Info.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceStats {
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
    /// The faces close a volume (a solid).
    pub closed: bool,
    /// The places where an edge passes through a face without an edge there: Intersect Faces
    /// makes the edges (the `NotIntersected` warnings of the check).
    pub not_intersected: usize,
}

/// The size of a box and its lowest corner. The box is relative to the origin of the scene; the
/// corner is made a world position with `origin`.
fn dims(b: (glam::Vec3, glam::Vec3), origin: glam::DVec3) -> ([f64; 3], [f64; 3]) {
    let (lo, hi) = b;
    let d = hi - lo;
    let corner = lo.as_dvec3() + origin;
    (
        [f64::from(d.x), f64::from(d.y), f64::from(d.z)],
        [corner.x, corner.y, corner.z],
    )
}

/// The union of boxes.
fn union(
    boxes: impl Iterator<Item = (glam::Vec3, glam::Vec3)>,
) -> Option<(glam::Vec3, glam::Vec3)> {
    boxes.fold(None, |acc, (lo, hi)| match acc {
        None => Some((lo, hi)),
        Some((a, b)) => Some((a.min(lo), b.max(hi))),
    })
}

/// The data of the Entity Info panel for a selection. Ids that are not in the document are left
/// out (the object was deleted by an undo).
pub fn entity_info(doc: &Document, scene: &Scene, selection: &[ObjectId]) -> EntityInfo {
    let present: Vec<ObjectId> = selection
        .iter()
        .copied()
        .filter(|id| doc.object(*id).is_some())
        .collect();
    match present.as_slice() {
        [] => EntityInfo::Nothing {
            layer: doc
                .layer(doc.active_layer)
                .map_or_else(|| "?".to_string(), |l| l.name.clone()),
            class: doc
                .class(doc.active_class)
                .map_or_else(|| "?".to_string(), |c| c.name.clone()),
        },
        [id] => {
            let obj = doc.object(*id).expect("filtered above");
            let bounds = scene.object_bounds(*id).map(|b| dims(b, scene.origin));
            let surface = match &obj.shape {
                kadakas_core::model::Shape::SurfaceModel(m) => {
                    let report = m.check();
                    Some(SurfaceStats {
                        vertices: m.vertex_count(),
                        edges: m.edge_count(),
                        faces: m.face_count(),
                        closed: m.face_count() > 0 && report.is_closed(),
                        not_intersected: report.not_intersected(),
                    })
                }
                _ => None,
            };
            EntityInfo::One {
                id: id.to_string(),
                kind: obj.shape.kind().to_string(),
                layer: doc
                    .layer(obj.layer)
                    .map_or_else(|| "?".to_string(), |l| l.name.clone()),
                class: doc
                    .class(obj.class)
                    .map_or_else(|| "?".to_string(), |c| c.name.clone()),
                size: bounds.map(|b| b.0),
                min: bounds.map(|b| b.1),
                surface,
                placement: placement_of(doc, obj),
                summary: summary_of(doc, obj).map(Box::new),
                material: kadakas_core::present::object_material(doc, *id)
                    .and_then(|m| resource_name(doc, Some(m))),
            }
        }
        many => EntityInfo::Many {
            count: many.len(),
            size: union(many.iter().filter_map(|id| scene.object_bounds(*id)))
                .map(|b| dims(b, scene.origin).0),
        },
    }
}

/// The Entity Info of a selection that may hold faces and edges of surface models. With none, it
/// is [`entity_info`] of the selected objects.
pub fn entity_info_with_parts(
    doc: &Document,
    scene: &Scene,
    selection: &[ObjectId],
    parts: &crate::modeling::SubSelection,
) -> EntityInfo {
    use crate::modeling::{Item, frame_of, model_of};
    let items: Vec<Item> = parts
        .items()
        .filter(|i| match i {
            Item::Face(m, f) => model_of(doc, *m).is_some_and(|s| s.face(*f).is_some()),
            Item::Edge(m, e) => model_of(doc, *m).is_some_and(|s| s.edge(*e).is_some()),
        })
        .collect();
    match items.as_slice() {
        [] => entity_info(doc, scene, selection),
        [Item::Face(mid, fid)] => {
            let m = model_of(doc, *mid).expect("filtered above");
            let f = m.face(*fid).expect("filtered above");
            let fr = frame_of(doc, *mid);
            let n = fr.matrix3.inverse().transpose() * f.plane.normal;
            EntityInfo::Face {
                model: mid.to_string(),
                id: fid.0,
                // The scale of the transform changes areas; the models are placed rigidly.
                area: m.face_area(*fid).unwrap_or(0.0)
                    * fr.matrix3.determinant().abs().cbrt().powi(2),
                normal: n.normalize_or_zero().to_array(),
                edges: f.loop_len(),
                holes: f.holes.len(),
                model_object: *mid,
                front: kadakas_core::present::face_materials(doc, &f.attrs).0,
                back: kadakas_core::present::face_materials(doc, &f.attrs).1,
            }
        }
        [Item::Edge(mid, eid)] => {
            let m = model_of(doc, *mid).expect("filtered above");
            let e = m.edge(*eid).expect("filtered above");
            let fr = frame_of(doc, *mid);
            let p = |v| {
                m.vertex(v)
                    .map(|q| fr.transform_point3(q))
                    .unwrap_or_default()
            };
            let (a, b) = (p(e.v[0]), p(e.v[1]));
            EntityInfo::Edge {
                model: mid.to_string(),
                id: eid.0,
                length: a.distance(b),
                start: a.to_array(),
                end: b.to_array(),
                curve: m.curve_of(*eid).map(|c| {
                    (
                        c.0,
                        m.curves()
                            .find(|(k, _)| *k == c)
                            .map_or(0, |(_, es)| es.len()),
                    )
                }),
                faces: m.faces_of_edge(*eid).len(),
            }
        }
        many => EntityInfo::Parts {
            faces: many.iter().filter(|i| matches!(i, Item::Face(..))).count(),
            edges: many.iter().filter(|i| matches!(i, Item::Edge(..))).count(),
        },
    }
}

/// What the Measurements box shows. The box shows a value and takes typed values (MOD-04).
#[derive(Clone, Debug, PartialEq)]
pub enum Measurement {
    /// Nothing to show (the box is empty).
    Empty,
    /// The size of the selection: "length x width x height" in the units of the person.
    Dimensions(String),
    /// The vertical field of view while the Zoom tool is active.
    FieldOfView(String),
    /// The value of a drawing tool that runs (the length of a line, the sides of a rectangle).
    Live(crate::modeling::LiveLabel, String),
}

impl Measurement {
    /// The message id of the label in front of the box.
    pub fn label_id(&self) -> &'static str {
        match self {
            Measurement::Empty | Measurement::Dimensions(_) => {
                msg!("uic-modeler-measure-dimensions")
            }
            Measurement::FieldOfView(_) => msg!("uic-modeler-measure-fov"),
            Measurement::Live(l, _) => match l {
                crate::modeling::LiveLabel::Length => msg!("uic-modeler-measure-length"),
                crate::modeling::LiveLabel::Dimensions => msg!("uic-modeler-measure-dimensions"),
                crate::modeling::LiveLabel::Radius => msg!("uic-modeler-measure-radius"),
                crate::modeling::LiveLabel::Bulge => msg!("uic-modeler-measure-bulge"),
                crate::modeling::LiveLabel::Distance => msg!("uic-modeler-measure-distance"),
                crate::modeling::LiveLabel::Angle => msg!("uic-modeler-measure-angle"),
                crate::modeling::LiveLabel::Factor => msg!("uic-modeler-measure-scale"),
            },
        }
    }

    /// The text in the box.
    pub fn text(&self) -> &str {
        match self {
            Measurement::Empty => "",
            Measurement::Dimensions(t) | Measurement::FieldOfView(t) | Measurement::Live(_, t) => t,
        }
    }
}

/// The value of the Measurements box. The Zoom tool shows the field of view (it changes it with
/// Shift and a drag); otherwise the box shows the size of the selection.
pub fn measurement(info: &EntityInfo, tool: ModelerTool, fov_deg: f32) -> Measurement {
    let u = kadakas_units::current();
    if tool == ModelerTool::Zoom {
        return Measurement::FieldOfView(u.angle_text_d(f64::from(fov_deg), 1));
    }
    match info {
        EntityInfo::One { size: Some(s), .. } | EntityInfo::Many { size: Some(s), .. } => {
            Measurement::Dimensions(tr!(
                "uic-modeler-dimensions-value",
                x = u.length_text(s[0]),
                y = u.length_text(s[1]),
                z = u.length_text(s[2])
            ))
        }
        _ => Measurement::Empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::mesh::build;
    use kadakas_core::model::{Attributes, Shape};

    fn square(ed: &mut Editor, at: f64) -> ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::Hardscape {
                    boundary: kadakas_core::curve::corners(&[
                        DVec2::new(at, at),
                        DVec2::new(at + 1000.0, at),
                        DVec2::new(at + 1000.0, at + 2000.0),
                        DVec2::new(at, at + 2000.0),
                    ]),
                    material: "m".into(),
                    thickness: Some(100.0),
                    elevation: None,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        ObjectId(out.created[0])
    }

    #[test]
    fn the_tool_bar_has_the_sixteen_getting_started_tools_once() {
        let mut seen: Vec<ModelerTool> = ModelerTool::GROUPS
            .iter()
            .flat_map(|g| g.iter().copied())
            .collect();
        assert_eq!(seen.len(), 16);
        seen.sort_by_key(|t| t.id());
        seen.dedup();
        assert_eq!(seen.len(), 16, "each tool is in one group");
        for t in ModelerTool::ALL {
            assert!(seen.contains(&t));
        }
        let names: Vec<_> = ModelerTool::ALL.iter().map(|t| t.name_id()).collect();
        assert!(names.iter().all(|n| n.starts_with("uic-modeler-tool-")));
    }

    #[test]
    fn select_the_drawing_tools_move_rotate_scale_and_navigation_work_now() {
        let working: Vec<_> = ModelerTool::ALL
            .iter()
            .filter(|t| t.works())
            .map(|t| t.id())
            .collect();
        assert_eq!(
            working,
            [
                "select",
                "eraser",
                "line",
                "rectangle",
                "circle",
                "arc",
                "push-pull",
                "move",
                "rotate",
                "scale",
                "paint-bucket",
                "orbit",
                "pan",
                "zoom",
                "zoom-extents"
            ]
        );
        let mut s = ToolState::default();
        assert_eq!(s.active(), ModelerTool::Select);
        for t in ModelerTool::ALL.iter().filter(|t| !t.works()) {
            assert!(!s.choose(*t), "{t:?} is refused");
            assert_eq!(s.active(), ModelerTool::Select);
        }
        assert!(s.choose(ModelerTool::Orbit));
        assert_eq!(s.active(), ModelerTool::Orbit);
        assert!(
            !s.choose(ModelerTool::ZoomExtents),
            "a command is not a tool"
        );
        assert_eq!(s.active(), ModelerTool::Orbit);
        s.select();
        assert_eq!(s.active(), ModelerTool::Select);
    }

    #[test]
    fn zoom_window_is_armed_once() {
        let mut s = ToolState::default();
        s.arm_zoom_window();
        assert_eq!(s.active(), ModelerTool::Zoom);
        assert!(s.window_armed());
        s.disarm_window();
        assert!(!s.window_armed());
        s.arm_zoom_window();
        assert!(s.choose(ModelerTool::Pan));
        assert!(!s.window_armed(), "another tool ends the window");
    }

    #[test]
    fn every_tool_has_an_action_and_the_action_finds_it_again() {
        for t in ModelerTool::ALL {
            assert_eq!(ModelerTool::from_action(t.action()), Some(t), "{t:?}");
        }
        assert_eq!(ModelerTool::from_action(Action::Undo), None);
    }

    #[test]
    fn a_tool_that_does_not_run_says_so_in_its_tooltip() {
        assert!(!ModelerTool::Select.tooltip("Space").contains("later"));
        let tip = ModelerTool::TapeMeasure.tooltip("T");
        assert!(tip.contains("Tape Measure") && tip.contains('T'));
        assert!(tip.contains(&tr!("uic-modeler-coming-later")));
        // Without a key there is no empty bracket.
        assert_eq!(ModelerTool::Select.tooltip(""), ModelerTool::Select.name());
    }

    #[test]
    fn the_measurements_box_shows_the_size_of_the_selection_and_the_field_of_view() {
        let mut ed = Editor::default();
        let a = square(&mut ed, 0.0);
        let scene = build(ed.doc());
        let none = entity_info(ed.doc(), &scene, &[]);
        assert_eq!(
            measurement(&none, ModelerTool::Select, 35.0),
            Measurement::Empty
        );
        assert_eq!(Measurement::Empty.text(), "");
        let one = entity_info(ed.doc(), &scene, &[a]);
        let m = measurement(&one, ModelerTool::Select, 35.0);
        assert!(matches!(m, Measurement::Dimensions(_)), "{m:?}");
        assert!(m.text().len() > 5);
        // The Zoom tool shows the field of view, with or without a selection.
        for info in [&none, &one] {
            let z = measurement(info, ModelerTool::Zoom, 35.0);
            assert!(
                matches!(&z, Measurement::FieldOfView(t) if t.contains("35")),
                "{z:?}"
            );
        }
        assert_ne!(
            measurement(&none, ModelerTool::Zoom, 35.0).label_id(),
            m.label_id()
        );
    }

    #[test]
    fn every_tool_has_a_hint() {
        for t in ModelerTool::ALL {
            assert!(!t.hint().is_empty());
        }
        assert_eq!(
            ModelerTool::TapeMeasure.hint(),
            tr!("uic-modeler-coming-later")
        );
        assert_ne!(ModelerTool::Move.hint(), tr!("uic-modeler-coming-later"));
        assert_ne!(ModelerTool::Line.hint(), ModelerTool::Rectangle.hint());
        assert_ne!(ModelerTool::Orbit.hint(), ModelerTool::Pan.hint());
    }

    #[test]
    fn entity_info_for_nothing_one_and_many() {
        let mut ed = Editor::default();
        let a = square(&mut ed, 0.0);
        let b = square(&mut ed, 5000.0);
        let scene = build(ed.doc());
        match entity_info(ed.doc(), &scene, &[]) {
            EntityInfo::Nothing { layer, class } => {
                assert!(!layer.is_empty() && !class.is_empty());
            }
            other => panic!("{other:?}"),
        }
        match entity_info(ed.doc(), &scene, &[a]) {
            EntityInfo::One {
                kind, size, min, ..
            } => {
                assert_eq!(kind, "hardscape");
                let size = size.unwrap();
                assert!((size[0] - 1000.0).abs() < 1e-3 && (size[1] - 2000.0).abs() < 1e-3);
                assert!((min.unwrap()[0]).abs() < 1e-3);
            }
            other => panic!("{other:?}"),
        }
        match entity_info(ed.doc(), &scene, &[a, b]) {
            EntityInfo::Many { count, size } => {
                assert_eq!(count, 2);
                let size = size.unwrap();
                assert!(
                    (size[0] - 6000.0).abs() < 1e-3,
                    "the box around both: {size:?}"
                );
            }
            other => panic!("{other:?}"),
        }
        // An id that is not in the document is left out.
        let gone = ObjectId::from_num(999_999);
        assert!(matches!(
            entity_info(ed.doc(), &scene, &[gone]),
            EntityInfo::Nothing { .. }
        ));
        assert!(matches!(
            entity_info(ed.doc(), &scene, &[gone, a]),
            EntityInfo::One { .. }
        ));
    }
}
