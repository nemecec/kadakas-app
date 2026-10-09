//! Collects the commands of an import and plans their IDs.
//!
//! A `Batch` creates layers, classes and objects one after the other, and each
//! gets the next free ID. The builder counts the same way, so it knows the ID of
//! a new class before the batch runs. Tests check that the plan holds.

use std::collections::BTreeMap;

use cadektor_core::command::Command;
use cadektor_core::georef::Georeference;
use cadektor_core::model::{arc_sweep, Attributes, ClassId, LayerId, ObjectId, Shape, VertexKind};
use cadektor_core::DVec2;
use serde_json::Value;

use crate::{Context, ImportResult, Report};

pub struct Builder {
    next_id: u64,
    cmds: Vec<Command>,
    layers: Vec<(String, LayerId)>,
    classes: Vec<(String, ClassId)>,
    pub report: Report,
    max_objects: usize,
    pub default_layer: LayerId,
    pub default_class: ClassId,
    full: bool,
    last_id: u64,
    /// Probe mode: shapes are not stored, only their bounds (mm) are collected.
    probe: Option<(DVec2, DVec2)>,
    /// Shift (mm) that every added shape gets. Imports use it to keep geometry near the origin.
    offset: DVec2,
}

/// Checks the rules that `Command::AddObject` enforces, so one bad entity
/// cannot make the whole batch fail.
pub fn check_shape(shape: &Shape) -> Result<(), &'static str> {
    if !shape.is_finite() {
        return Err("number is not finite");
    }
    if !shape.is_in_range() {
        return Err("coordinate is out of range (more than 1e9 mm)");
    }
    if let Some(vs) = shape.vertices()
        && vs.iter().any(|v| matches!(v.kind, VertexKind::Arc { radius } if radius < 0.0))
    {
        return Err("negative radius");
    }
    match shape {
        Shape::Circle { radius, .. } | Shape::Arc { radius, .. } if *radius <= 0.0 => Err("radius is not positive"),
        Shape::Arc { start, end, .. } if arc_sweep(*start, *end) == 0.0 => Err("arc has no sweep"),
        Shape::Text { height, .. } if *height <= 0.0 => Err("text height is not positive"),
        Shape::Polyline { points, .. } if points.len() < 2 => Err("polyline has fewer than 2 points"),
        Shape::Polyline3d { points, .. } if points.len() < 2 => Err("polyline has fewer than 2 points"),
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } if boundary.len() < 3 => {
            Err("boundary has fewer than 3 points")
        }
        _ => Ok(()),
    }
}

impl Builder {
    pub fn new(ctx: &Context, format: &str, max_objects: usize) -> Builder {
        Builder {
            next_id: ctx.next_id,
            cmds: Vec::new(),
            layers: ctx.layers.iter().filter(|l| !l.locked && l.kind == cadektor_core::model::LayerKind::Design).map(|l| (l.name.clone(), l.id)).collect(),
            classes: ctx.classes.iter().map(|(id, n)| (n.clone(), *id)).collect(),
            report: Report::new(format),
            max_objects,
            default_layer: ctx.active_layer,
            default_class: ctx.active_class,
            full: false,
            last_id: 0,
            probe: None,
            offset: DVec2::ZERO,
        }
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// The layer with this name. A layer is created if no unlocked layer has the name.
    pub fn layer(&mut self, name: &str, visible: bool) -> LayerId {
        if let Some((_, id)) = self.layers.iter().find(|(n, _)| n == name) {
            return *id;
        }
        let id = LayerId(self.take_id());
        self.cmds.push(Command::AddLayer { name: name.into(), elevation: 0.0, scale: None, kind: None });
        if !visible {
            self.cmds.push(Command::ModifyLayer { id, name: None, visible: Some(false), locked: None, elevation: None, scale: None });
        }
        self.layers.push((name.into(), id));
        self.report.layers_created.push(name.into());
        id
    }

    /// The class with this name. A class is created, with `attrs`, if there is none.
    pub fn class(&mut self, name: &str, attrs: &Attributes, visible: bool) -> ClassId {
        if let Some((_, id)) = self.classes.iter().find(|(n, _)| n == name) {
            return *id;
        }
        let id = ClassId(self.take_id());
        self.cmds.push(Command::AddClass { name: name.into(), attrs: attrs.clone() });
        if !visible {
            self.cmds.push(Command::ModifyClass { id, name: None, visible: Some(false), attrs: None });
        }
        self.classes.push((name.into(), id));
        self.report.classes_created.push(name.into());
        id
    }

    pub fn set_georef(&mut self, g: Georeference) {
        self.cmds.push(Command::SetGeoreference { georef: Some(g) });
        self.report.georeferenced = true;
    }

    /// Shifts every shape added from now on by `-offset` (mm).
    pub fn set_offset(&mut self, offset: DVec2) {
        self.offset = offset;
    }

    /// Switches to probe mode: nothing is stored and no range is checked. See [`Builder::bounds`].
    pub fn set_probe(&mut self) {
        self.probe = Some((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)));
    }

    /// The box (mm) around all shapes that a probe saw. `None` if it saw none.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        self.probe.filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
    }

    /// ID of the object that the last successful `add` made.
    pub fn last_id(&self) -> u64 {
        self.last_id
    }

    /// True when `max_objects` is reached.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Adds an object. Returns false if it was skipped (and reports why).
    pub fn add(&mut self, entity: &str, mut shape: Shape, layer: LayerId, class: ClassId, attrs: Attributes, data: BTreeMap<String, Value>) -> bool {
        if self.report.total() >= self.max_objects {
            if !self.full {
                self.full = true;
                self.report.note("import stopped at the object limit");
            }
            self.report.skip(entity, "object limit reached");
            return false;
        }
        if let Some((lo, hi)) = self.probe.as_mut() {
            if shape.is_finite() {
                let (a, b) = shape.bounds();
                if a.is_finite() && b.is_finite() {
                    *lo = lo.min(a);
                    *hi = hi.max(b);
                }
            }
            self.report.count(shape.kind());
            return true;
        }
        if self.offset != DVec2::ZERO && shape.is_finite() {
            shape.translate(-self.offset);
        }
        if let Err(why) = check_shape(&shape) {
            self.report.skip(entity, why);
            return false;
        }
        let id = ObjectId(self.take_id());
        self.last_id = id.0;
        let kind = shape.kind();
        self.cmds.push(Command::AddObject { shape, layer: Some(layer), class: Some(class), attrs });
        if !data.is_empty() {
            self.cmds.push(Command::SetData { id, set: data, remove: vec![] });
        }
        self.report.count(kind);
        true
    }

    pub fn finish(self) -> ImportResult {
        ImportResult { command: Command::Batch { commands: self.cmds }, report: self.report }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::command::Editor;
    use cadektor_core::model::{Document, Color};
    use cadektor_core::DVec2;

    #[test]
    fn plan_matches_real_ids() {
        let doc = Document::default();
        let mut b = Builder::new(&Context::of(&doc), "test", 100);
        let l = b.layer("Site", false);
        assert_eq!(b.layer("Site", true), l);
        let c = b.class("Walls", &Attributes { pen: Some(Color::BLACK), ..Attributes::default() }, false);
        assert_eq!(b.class("None", &Attributes::default(), true), cadektor_core::model::NONE_CLASS);
        let mut data = BTreeMap::new();
        data.insert("k".to_string(), Value::from(1));
        assert!(b.add("LINE", Shape::Line { a: DVec2::ZERO, b: DVec2::X }, l, c, Attributes::default(), data));
        assert!(!b.add("CIRCLE", Shape::Circle { center: DVec2::ZERO, radius: 0.0 }, l, c, Attributes::default(), BTreeMap::new()));
        assert!(b.add("LINE", Shape::Line { a: DVec2::ZERO, b: DVec2::Y }, l, c, Attributes::default(), BTreeMap::new()));
        let r = b.finish();
        let mut ed = Editor::new(doc);
        let out = ed.execute(&r.command).unwrap();
        assert_eq!(out.created[0], l.0);
        assert_eq!(out.created[1], c.0);
        let first = ed.doc().object(cadektor_core::model::ObjectId(out.created[2])).unwrap();
        assert_eq!(first.data["k"], 1);
        assert!(!ed.doc().layer(l).unwrap().visible);
        assert!(!ed.doc().class(c).unwrap().visible);
        assert_eq!(r.report.skipped_total(), 1);
        assert!(ed.undo());
        assert_eq!(ed.doc().layers.len(), 1);
    }

    #[test]
    fn locked_layers_are_not_reused_and_limit_stops() {
        let mut doc = Document::default();
        doc.layers[0].locked = true;
        let mut b = Builder::new(&Context::of(&doc), "test", 1);
        let l = b.layer("Layer 1", true);
        assert_ne!(l, doc.layers[0].id);
        let c = b.default_class;
        assert!(b.add("LINE", Shape::Line { a: DVec2::ZERO, b: DVec2::X }, l, c, Attributes::default(), BTreeMap::new()));
        assert!(!b.is_full());
        assert!(!b.add("LINE", Shape::Line { a: DVec2::ZERO, b: DVec2::X }, l, c, Attributes::default(), BTreeMap::new()));
        assert!(b.is_full());
        assert!(!b.add("LINE", Shape::Line { a: DVec2::ZERO, b: DVec2::X }, l, c, Attributes::default(), BTreeMap::new()));
        assert_eq!(b.report.notes.len(), 1);
    }

    #[test]
    fn check_shape_rules() {
        let p = |x: f64| cadektor_core::curve::corners(&[DVec2::ZERO, DVec2::new(x, 1.0), DVec2::new(1.0, x)]);
        assert!(check_shape(&Shape::Polyline { points: p(1.0), closed: true }).is_ok());
        assert!(check_shape(&Shape::Polyline { points: p(f64::NAN), closed: true }).is_err());
        assert!(check_shape(&Shape::Polyline { points: p(1e12), closed: true }).is_err());
        assert!(check_shape(&Shape::Polyline { points: p(1.0)[..1].to_vec(), closed: false }).is_err());
        assert!(check_shape(&Shape::Polyline3d { points: vec![], closed: false }).is_err());
        assert!(check_shape(&Shape::LandscapeArea { boundary: p(1.0)[..2].to_vec(), name: String::new() }).is_err());
        assert!(check_shape(&Shape::Arc { center: DVec2::ZERO, radius: 1.0, start: 1.0, end: 1.0 }).is_err());
        assert!(check_shape(&Shape::Text { at: DVec2::ZERO, text: "a".into(), height: 0.0 }).is_err());
        let neg = vec![
            cadektor_core::curve::Vertex::new(DVec2::ZERO, VertexKind::Arc { radius: -1.0 }),
            cadektor_core::curve::Vertex::corner(DVec2::X),
        ];
        assert!(check_shape(&Shape::Polyline { points: neg, closed: false }).is_err());
    }
}
