//! Hardscapes: paved areas (slabs) and paths, with styles, build-up components, joint patterns,
//! slopes, quantities and draping.
//!
//! The hardscape style is a resource (see `crate::resource`). A hardscape object keeps its
//! settings in `Object::data["hardscape"]` (see `data`), so older files and objects without a
//! style stay valid. Everything here is pure geometry or reads the document. The ground of a
//! site model comes in through [`TerrainSource`], so core does not depend on the site crate.

pub mod data;
pub mod joints;
pub mod offset;
pub mod report;
pub mod style;
pub mod surface;

use std::fmt::Write;

use glam::DVec2;

pub use data::{HardscapeData, HardscapePatch, Mode, Slope, data, style_id};
pub use offset::Join;
pub use style::{Border, Component, HardscapeStyle, JointKind, JointPattern, Sides};
pub use surface::Terrain;

use crate::curve;
use crate::mesh::SceneSink;
use crate::model::{Attributes, Color, Document, Object, Shape};
use surface::LayerSpec;

/// Default color of a hardscape in 3D.
pub const DEFAULT_COLOR: [u8; 4] = [160, 156, 150, 255];
const EDGE_COLOR: [u8; 4] = [40, 40, 40, 255];
/// Flattening tolerance for 3D, in mm.
const FLATTEN_TOLERANCE: f64 = 2.0;

/// The ground under objects. A reports and the 3D view ask it for heights. The site crate
/// implements it with the site model of the layer.
pub trait TerrainSource {
    /// Height of the ground at `p` for this object, or `None` if there is no ground there.
    fn elevation(&self, obj: &Object, p: DVec2) -> Option<f64>;

    /// True if the point lies on the ground itself. Outside, `elevation` may still give the height
    /// of the nearest edge.
    fn covered(&self, obj: &Object, p: DVec2) -> bool {
        self.elevation(obj, p).is_some()
    }
}

/// No ground: nothing drapes.
pub struct NoTerrain;

impl TerrainSource for NoTerrain {
    fn elevation(&self, _obj: &Object, _p: DVec2) -> Option<f64> {
        None
    }
}

/// True if the object asks to be draped on the site model (a hardscape or a landscape area).
pub fn wants_drape(obj: &Object) -> bool {
    match &obj.shape {
        Shape::Hardscape { .. } => data(obj).is_some_and(|d| d.drape),
        Shape::LandscapeArea { .. } => crate::landscape::data(obj).is_some_and(|d| d.drape),
        _ => false,
    }
}

/// Moves the data that holds positions (a path center line, holes of a landscape area).
pub fn translate_object_data(obj: &mut Object, delta: DVec2) {
    data::translate_data(obj, delta);
    crate::landscape::translate_data(obj, delta);
}

/// The attributes of an object with the fields of its style where the object has none of its own.
pub fn styled_attrs(doc: &Document, obj: &Object, mut a: Attributes) -> Attributes {
    let (fill, pen, weight) = match &obj.shape {
        Shape::Hardscape { .. } => data::style_of(doc, obj).map_or((None, None, None), |(_, s)| (s.fill, s.pen, None)),
        Shape::LandscapeArea { .. } => crate::landscape::style_of(doc, obj).map_or((None, None, None), |(_, s)| (s.fill, s.pen, s.line_weight)),
        _ => return a,
    };
    if obj.attrs.fill.is_none() && fill.is_some() {
        a.fill = fill;
    }
    if obj.attrs.pen.is_none() && pen.is_some() {
        a.pen = pen;
    }
    if obj.attrs.line_weight.is_none() && weight.is_some() {
        a.line_weight = weight;
    }
    a
}

fn rgba(c: Color) -> [u8; 4] {
    c.0
}

/// The layers of the solid of a hardscape, top first, and its base color.
pub fn layer_specs(doc: &Document, obj: &Object, base: [u8; 4]) -> Vec<LayerSpec> {
    let Shape::Hardscape { thickness, .. } = &obj.shape else { return Vec::new() };
    match data::style_of(doc, obj) {
        Some((_, s)) => {
            let mut out = vec![LayerSpec { top: 0.0, bottom: s.thickness, color: base }];
            let mut depth = s.thickness;
            for (i, c) in s.components.iter().enumerate() {
                let color = c.color.map_or_else(|| surface::shade(base, 0.8f32.powi(i as i32 + 1)), rgba);
                out.push(LayerSpec { top: depth, bottom: depth + c.thickness, color });
                depth += c.thickness;
            }
            out
        }
        None => {
            let t = thickness.filter(|t| t.is_finite() && *t > 0.0).unwrap_or(crate::mesh::DEFAULT_SLAB_THICKNESS);
            vec![LayerSpec { top: 0.0, bottom: t, color: base }]
        }
    }
}

/// Adds the 3D solid of a hardscape to a sink. `terrain` is the ground, used when the hardscape drapes.
pub fn emit_object(doc: &Document, obj: &Object, terrain: Option<Terrain<'_>>, sink: &mut SceneSink<'_>) {
    let Shape::Hardscape { boundary, .. } = &obj.shape else { return };
    let d = data(obj);
    let attrs = styled_attrs(doc, obj, doc.resolved_attrs(obj));
    let base = attrs.fill.filter(|c| c.0[3] > 0).map_or(DEFAULT_COLOR, rgba);
    let edge = attrs.pen.filter(|c| c.0[3] > 0).map_or(EDGE_COLOR, rgba);
    let ring = curve::flatten(boundary, true, FLATTEN_TOLERANCE);
    let surf = surface::surface_of(obj, d.as_ref(), terrain);
    surface::emit_solid(sink, &ring, &surf, &layer_specs(doc, obj, base), edge);
}

/// Extra SVG elements of a hardscape or a landscape area, drawn after its outline.
pub fn object_svg_extras(doc: &Document, obj: &Object, scale: f64, stroke: &str) -> String {
    match &obj.shape {
        Shape::Hardscape { .. } => svg_extras(doc, obj, scale, stroke),
        Shape::LandscapeArea { .. } => crate::landscape::svg_extras(doc, obj, scale, stroke),
        _ => String::new(),
    }
}

fn pts_attr(pts: &[DVec2]) -> String {
    pts.iter().map(|p| format!("{},{}", p.x, -p.y)).collect::<Vec<_>>().join(" ")
}

fn path_d(segments: &[joints::Segment]) -> String {
    let mut d = String::new();
    for (a, b) in segments {
        let _ = write!(d, "M {} {} L {} {} ", a.x, -a.y, b.x, -b.y);
    }
    d.trim_end().to_string()
}

/// Extra SVG elements of a hardscape drawn after its outline: the joint pattern and the border.
/// `scale` is the drawing scale of the layer, `stroke` the pen color of the object.
pub fn svg_extras(doc: &Document, obj: &Object, scale: f64, stroke: &str) -> String {
    let mut out = String::new();
    let Shape::Hardscape { boundary, .. } = &obj.shape else { return out };
    let Some((_, s)) = data::style_of(doc, obj) else { return out };
    let d = data(obj).unwrap_or_default();
    let ring = curve::flatten(boundary, true, FLATTEN_TOLERANCE);
    if let Some(j) = &s.joints {
        let lines = joints::joint_lines(&ring, j);
        if !lines.segments.is_empty() {
            let color = j.color.map_or_else(|| stroke.to_string(), Color::to_css);
            let width = j.spacing.max(0.1 * scale);
            let _ = writeln!(
                out,
                r#"<path d="{}" fill="none" stroke="{color}" stroke-width="{width}" data-part="joints" data-of="{}" data-pattern="{}"/>"#,
                path_d(&lines.segments),
                obj.id,
                j.kind.id()
            );
        }
    }
    if let Some(b) = &s.border {
        let color = b.color.map_or_else(|| stroke.to_string(), Color::to_css);
        let clip = format!("hs{}", obj.id);
        let lines: Vec<Vec<DVec2>> = if d.mode == Mode::Path {
            let (l, r) = offset::sides(&d.axis(), d.width);
            match b.sides {
                Sides::Both => vec![l, r],
                Sides::Left => vec![l],
                Sides::Right => vec![r],
            }
        } else {
            let mut closed = ring.clone();
            closed.extend(ring.first().copied());
            vec![closed]
        };
        let _ = write!(out, r#"<clipPath id="{clip}"><polygon points="{}"/></clipPath>"#, pts_attr(&ring));
        for pts in lines.iter().filter(|p| p.len() >= 2) {
            let _ = writeln!(
                out,
                r#"<polyline points="{}" fill="none" stroke="{color}" stroke-width="{}" clip-path="url(#{clip})" data-part="border" data-of="{}"/>"#,
                pts_attr(pts),
                2.0 * b.width,
                obj.id
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::mesh;
    use crate::model::ObjectId;
    use crate::resource::{ResourceBody, ResourceId};

    fn setup() -> (Editor, ObjectId, ResourceId) {
        let mut ed = Editor::default();
        let style = HardscapeStyle {
            material: "Setts".into(),
            fill: Some(Color([200, 100, 50, 255])),
            components: vec![Component { thickness: 100.0, material: "Sand".into(), ..Default::default() }],
            border: Some(Border { width: 150.0, material: "Kerb".into(), color: Some(Color([1, 2, 3, 255])), sides: Sides::Both }),
            joints: Some(JointPattern { kind: JointKind::RunningBond, ..Default::default() }),
            ..Default::default()
        };
        let sid = ResourceId(ed.execute(&Command::AddResource { name: "Setts".into(), body: ResourceBody::HardscapeStyle(Box::new(style)) }).unwrap().created[0]);
        let b = curve::corners(&[DVec2::ZERO, DVec2::new(2000.0, 0.0), DVec2::new(2000.0, 1000.0), DVec2::new(0.0, 1000.0)]);
        let shape = Shape::Hardscape { boundary: b, material: "x".into(), thickness: None, elevation: Some(50.0) };
        let id = ObjectId(ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0]);
        ed.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { style: Some(sid), ..Default::default() } }).unwrap();
        (ed, id, sid)
    }

    #[test]
    fn styled_solid_has_layers_and_colors() {
        let (ed, id, _) = setup();
        let scene = mesh::build(ed.doc());
        let (lo, hi) = scene.object_bounds(id).unwrap();
        let o = scene.origin;
        // Top at 50, surface 60 mm and a component of 100 mm below.
        assert!((hi.z as f64 + o.z - 50.0).abs() < 1e-3);
        assert!((lo.z as f64 + o.z - (50.0 - 160.0)).abs() < 1e-3);
        assert!(scene.mesh.colors.contains(&[200, 100, 50, 255]), "surface color from the style");
        // Walls of the surface layer are a shade of the surface color.
        assert!(scene.mesh.colors.iter().any(|c| *c == surface::shade([200, 100, 50, 255], 0.82)));
    }

    #[test]
    fn layer_specs_without_style_use_the_shape() {
        let (mut ed, id, _) = setup();
        let o = ed.doc().object(id).unwrap().clone();
        let specs = layer_specs(ed.doc(), &o, DEFAULT_COLOR);
        assert_eq!(specs.len(), 2);
        assert_eq!((specs[1].top, specs[1].bottom), (60.0, 160.0));
        ed.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { unstyle: true, ..Default::default() } }).unwrap();
        let o = ed.doc().object(id).unwrap().clone();
        let specs = layer_specs(ed.doc(), &o, DEFAULT_COLOR);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].bottom, 160.0, "the copied thickness stays");
        let line = Object::new(ObjectId(77), crate::model::LayerId(1), crate::model::ClassId(0), Shape::Line { a: DVec2::ZERO, b: DVec2::X });
        assert!(layer_specs(ed.doc(), &line, DEFAULT_COLOR).is_empty());
    }

    #[test]
    fn svg_extras_draw_joints_and_border() {
        let (ed, id, _) = setup();
        let o = ed.doc().object(id).unwrap();
        let svg = svg_extras(ed.doc(), o, 100.0, "rgba(0,0,0,1)");
        assert!(svg.contains(r#"data-part="joints""#) && svg.contains("running_bond"));
        assert!(svg.contains(r#"data-part="border""#) && svg.contains("clip-path"));
        assert!(svg.contains("rgba(1,2,3,1.000)"));
        // A path with a border on the left only has one border line.
        let mut ed = ed;
        let center = curve::corners(&[DVec2::ZERO, DVec2::new(5000.0, 0.0)]);
        ed.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { center: Some(center), width: Some(1000.0), ..Default::default() } }).unwrap();
        let svg = svg_extras(ed.doc(), ed.doc().object(id).unwrap(), 100.0, "black");
        assert_eq!(svg.matches(r#"data-part="border""#).count(), 2);
        // No style, or not a hardscape: nothing.
        ed.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { unstyle: true, ..Default::default() } }).unwrap();
        assert!(svg_extras(ed.doc(), ed.doc().object(id).unwrap(), 100.0, "black").is_empty());
        let line = Object::new(ObjectId(77), crate::model::LayerId(1), crate::model::ClassId(0), Shape::Line { a: DVec2::ZERO, b: DVec2::X });
        assert!(svg_extras(ed.doc(), &line, 100.0, "black").is_empty());
    }

    #[test]
    fn styled_attrs_and_drape_flags() {
        let (ed, id, _) = setup();
        let o = ed.doc().object(id).unwrap();
        let a = styled_attrs(ed.doc(), o, ed.doc().resolved_attrs(o));
        assert_eq!(a.fill, Some(Color([200, 100, 50, 255])));
        // An own fill wins.
        let mut own = o.clone();
        own.attrs.fill = Some(Color([9, 9, 9, 255]));
        assert_eq!(styled_attrs(ed.doc(), &own, own.attrs.clone()).fill, Some(Color([9, 9, 9, 255])));
        let line = Object::new(ObjectId(77), crate::model::LayerId(1), crate::model::ClassId(0), Shape::Line { a: DVec2::ZERO, b: DVec2::X });
        assert_eq!(styled_attrs(ed.doc(), &line, Attributes::default()), Attributes::default());
        assert!(!wants_drape(o));
        assert!(!wants_drape(&line));
        let mut d = ed.doc().clone();
        let mut ed2 = Editor::new(std::mem::take(&mut d));
        ed2.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { drape: Some(true), ..Default::default() } }).unwrap();
        assert!(wants_drape(ed2.doc().object(id).unwrap()));
        assert_eq!(NoTerrain.elevation(&line, DVec2::ZERO), None);
    }

    #[test]
    fn moving_moves_the_center_line() {
        let (mut ed, id, _) = setup();
        let center = curve::corners(&[DVec2::ZERO, DVec2::new(5000.0, 0.0)]);
        ed.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { center: Some(center), ..Default::default() } }).unwrap();
        ed.execute(&Command::MoveObjects { ids: vec![id], delta: DVec2::new(100.0, 200.0) }).unwrap();
        let d = data(ed.doc().object(id).unwrap()).unwrap();
        assert_eq!(d.center[0].p, DVec2::new(100.0, 200.0));
        // The outline moved with it: it equals a new outline from the moved center line.
        let again = d.outline();
        assert_eq!(ed.doc().object(id).unwrap().shape.vertices().unwrap(), &again);
    }
}

#[cfg(test)]
mod review_tests;
