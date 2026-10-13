//! Collects the commands of an import and plans their IDs.
//!
//! A `Batch` creates layers, classes and objects one after the other, and each
//! gets the next free ID. The builder counts the same way, so it knows the ID of
//! a new class before the batch runs. Tests check that the plan holds.

use std::collections::{BTreeMap, HashSet};

use kadakas_core::DVec2;
use kadakas_core::command::Command;
use kadakas_core::georef::Georeference;
use kadakas_core::model::{Attributes, ClassId, LayerId, ObjectId, Shape, VertexKind, arc_sweep};
use kadakas_core::provenance::{self, LayerSource};
use kadakas_core::resource::{ResourceBody, ResourceId};
use serde_json::Value;

use crate::{Context, ImportResult, Report};

pub struct Builder {
    ids: kadakas_core::uid::IdGen,
    cmds: Vec<Command>,
    layers: Vec<(String, LayerId)>,
    /// Names of all layers of the document, and of the layers that this import adds.
    names: HashSet<String>,
    /// Layers that an import made and that name their source (see `source_layer`), also the locked
    /// ones.
    sourced: Vec<(LayerSource, LayerId)>,
    /// The sources that the document had before this import.
    known: Vec<LayerSource>,
    /// Locked layers that the import unlocked to put objects on them. `finish` locks them again.
    relock: Vec<LayerId>,
    locked: HashSet<LayerId>,
    classes: Vec<(String, ClassId)>,
    pub report: Report,
    max_objects: usize,
    pub default_layer: LayerId,
    pub default_class: ClassId,
    full: bool,
    last_id: kadakas_core::uid::Uid,
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
    if shape.problem().is_some() {
        return Err("the shape is not valid");
    }
    if let Some(vs) = shape.vertices()
        && vs
            .iter()
            .any(|v| matches!(v.kind, VertexKind::Arc { radius } if radius < 0.0))
    {
        return Err("negative radius");
    }
    match shape {
        Shape::Circle { radius, .. } | Shape::Arc { radius, .. } if *radius <= 0.0 => {
            Err("radius is not positive")
        }
        Shape::Arc { start, end, .. } if arc_sweep(*start, *end) == 0.0 => Err("arc has no sweep"),
        Shape::Text { height, .. } if *height <= 0.0 => Err("text height is not positive"),
        Shape::Polyline { points, .. } if points.len() < 2 => {
            Err("polyline has fewer than 2 points")
        }
        Shape::Polyline3d { points, .. } if points.len() < 2 => {
            Err("polyline has fewer than 2 points")
        }
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. }
            if boundary.len() < 3 =>
        {
            Err("boundary has fewer than 3 points")
        }
        _ => Ok(()),
    }
}

impl Builder {
    pub fn new(ctx: &Context, format: &str, max_objects: usize) -> Builder {
        Builder {
            ids: ctx.ids.clone(),
            cmds: Vec::new(),
            layers: ctx
                .layers
                .iter()
                .filter(|l| !l.locked && l.kind == kadakas_core::model::LayerKind::Design)
                .map(|l| (l.name.clone(), l.id))
                .collect(),
            names: ctx.layers.iter().map(|l| l.name.clone()).collect(),
            sourced: ctx
                .layers
                .iter()
                .filter(|l| l.kind == kadakas_core::model::LayerKind::Design)
                .filter_map(|l| {
                    let src = LayerSource::of(l.extra.get(provenance::LAYER_KEY)?)?;
                    Some((src, l.id))
                })
                .collect(),
            known: ctx
                .layers
                .iter()
                .filter_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
                .collect(),
            relock: Vec::new(),
            locked: ctx
                .layers
                .iter()
                .filter(|l| l.locked)
                .map(|l| l.id)
                .collect(),
            classes: ctx.classes.iter().map(|(id, n)| (n.clone(), *id)).collect(),
            report: Report::new(format),
            max_objects,
            default_layer: ctx.active_layer,
            default_class: ctx.active_class,
            full: false,
            last_id: kadakas_core::uid::Uid::NIL,
            probe: None,
            offset: DVec2::ZERO,
        }
    }

    fn take_id(&mut self) -> kadakas_core::uid::Uid {
        self.ids.take()
    }

    /// The layer with this name. A layer is created if no unlocked layer has the name.
    pub fn layer(&mut self, name: &str, visible: bool) -> LayerId {
        if let Some((_, id)) = self.layers.iter().find(|(n, _)| n == name) {
            return *id;
        }
        let id = LayerId(self.take_id());
        self.cmds.push(Command::AddLayer {
            name: name.into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        });
        if !visible {
            self.cmds.push(Command::ModifyLayer {
                id,
                name: None,
                visible: Some(false),
                locked: None,
                elevation: None,
                scale: None,
            });
        }
        self.layers.push((name.into(), id));
        self.names.insert(name.into());
        self.report.layers_created.push(name.into());
        id
    }

    /// The layer of an import source (IOP-22). Layers are matched on the drawing and the DXF
    /// layer. It is a layer that an earlier import of the same drawing made, locked or not (a
    /// locked one is unlocked for the import and locked again at the end), else a new layer that
    /// names the source (`SetLayerSource`). When the revision or the transform differ, the source
    /// of the layer is renewed. The new layer never reuses a design layer, so imported objects stay
    /// apart from the design work: if a layer has the name already, the new one is
    /// `name (label)`, then `name (label) 2` and so on. `label` is the file name.
    pub fn source_layer(
        &mut self,
        name: &str,
        visible: bool,
        src: &LayerSource,
        label: &str,
    ) -> LayerId {
        if let Some(at) = self
            .sourced
            .iter()
            .position(|(s, _)| s.drawing == src.drawing && s.dxf_layer == src.dxf_layer)
        {
            let id = self.sourced[at].1;
            if self.sourced[at].0 != *src {
                self.cmds.push(Command::SetLayerSource {
                    id,
                    source: Some(src.clone()),
                });
                self.sourced[at].0 = src.clone();
            }
            if self.locked.remove(&id) {
                self.cmds.push(Command::ModifyLayer {
                    id,
                    name: None,
                    visible: None,
                    locked: Some(false),
                    elevation: None,
                    scale: None,
                });
                self.relock.push(id);
            }
            return id;
        }
        let mut unique = name.to_string();
        if self.names.contains(&unique) {
            unique = format!("{name} ({label})");
        }
        let mut n = 2;
        while self.names.contains(&unique) {
            unique = format!("{name} ({label}) {n}");
            n += 1;
        }
        let id = LayerId(self.take_id());
        self.cmds.push(Command::AddLayer {
            name: unique.clone(),
            elevation: 0.0,
            scale: None,
            kind: None,
        });
        self.cmds.push(Command::SetLayerSource {
            id,
            source: Some(src.clone()),
        });
        if !visible {
            self.cmds.push(Command::ModifyLayer {
                id,
                name: None,
                visible: Some(false),
                locked: None,
                elevation: None,
                scale: None,
            });
        }
        self.names.insert(unique.clone());
        self.sourced.push((src.clone(), id));
        self.report.layers_created.push(unique);
        id
    }

    /// True if the document had a layer of this drawing before the import.
    pub fn knows_drawing(&self, drawing: &str) -> bool {
        self.known.iter().any(|s| s.drawing == drawing)
    }

    /// True if the document had a layer of this drawing and revision before the import.
    pub fn knows_revision(&self, drawing: &str, revision: &str) -> bool {
        self.known
            .iter()
            .any(|s| s.drawing == drawing && s.revision == revision)
    }

    /// The shift (mm) that every added shape gets.
    pub fn offset(&self) -> DVec2 {
        self.offset
    }

    /// True in probe mode: nothing is stored, so nothing needs a tag.
    pub fn is_probe(&self) -> bool {
        self.probe.is_some()
    }

    /// The class with this name. A class is created, with `attrs`, if there is none.
    pub fn class(&mut self, name: &str, attrs: &Attributes, visible: bool) -> ClassId {
        if let Some((_, id)) = self.classes.iter().find(|(n, _)| n == name) {
            return *id;
        }
        let id = ClassId(self.take_id());
        self.cmds.push(Command::AddClass {
            name: name.into(),
            attrs: attrs.clone(),
        });
        if !visible {
            self.cmds.push(Command::ModifyClass {
                id,
                name: None,
                visible: Some(false),
                attrs: None,
            });
        }
        self.classes.push((name.into(), id));
        self.report.classes_created.push(name.into());
        id
    }

    /// Adds a resource (a symbol, a record format) and returns the ID that it will get.
    pub fn resource(&mut self, name: &str, body: ResourceBody) -> ResourceId {
        let id = ResourceId(self.take_id());
        self.cmds.push(Command::AddResource {
            name: name.into(),
            body,
        });
        id
    }

    /// Adds a command that creates nothing, for example `AttachRecord`.
    pub fn push(&mut self, cmd: Command) {
        self.cmds.push(cmd);
    }

    /// Sets the elevation of a layer that this import made.
    pub fn set_elevation(&mut self, layer: LayerId, elevation: f64) {
        self.cmds.push(Command::ModifyLayer {
            id: layer,
            name: None,
            visible: None,
            locked: None,
            elevation: Some(elevation),
            scale: None,
        });
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
        self.probe
            .filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
    }

    /// ID of the object that the last successful `add` made.
    pub fn last_id(&self) -> kadakas_core::uid::Uid {
        self.last_id
    }

    /// True when `max_objects` is reached.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Adds an object. Returns false if it was skipped (and reports why).
    pub fn add(
        &mut self,
        entity: &str,
        mut shape: Shape,
        layer: LayerId,
        class: ClassId,
        attrs: Attributes,
        mut data: BTreeMap<String, Value>,
    ) -> bool {
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
        // The tag of an import says what the object was right after the import (`provenance`).
        if let Some(Value::Object(t)) = data.get_mut(provenance::KEY)
            && t.get("obj").and_then(Value::as_str) == Some(provenance::PENDING_OBJ)
        {
            let h = provenance::state_hash(&shape, &attrs, layer, class);
            t.insert("obj".into(), Value::from(h));
        }
        self.cmds.push(Command::AddObject {
            shape,
            layer: Some(layer),
            class: Some(class),
            attrs,
        });
        if !data.is_empty() {
            self.cmds.push(Command::SetData {
                id,
                set: data,
                remove: vec![],
            });
        }
        self.report.count(kind);
        true
    }

    pub fn finish(mut self) -> ImportResult {
        for id in std::mem::take(&mut self.relock) {
            self.cmds.push(Command::ModifyLayer {
                id,
                name: None,
                visible: None,
                locked: Some(true),
                elevation: None,
                scale: None,
            });
        }
        ImportResult {
            command: Command::Batch {
                commands: self.cmds,
            },
            report: self.report,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::DVec2;
    use kadakas_core::command::Editor;
    use kadakas_core::model::{Color, Document};

    #[test]
    fn plan_matches_real_ids() {
        let doc = Document::with_sequential_ids();
        let mut b = Builder::new(&Context::of(&doc), "test", 100);
        let l = b.layer("Site", false);
        assert_eq!(b.layer("Site", true), l);
        let c = b.class(
            "Walls",
            &Attributes {
                pen: Some(Color::BLACK),
                ..Attributes::default()
            },
            false,
        );
        assert_eq!(
            b.class("None", &Attributes::default(), true),
            kadakas_core::model::NONE_CLASS
        );
        let mut data = BTreeMap::new();
        data.insert("k".to_string(), Value::from(1));
        assert!(b.add(
            "LINE",
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X
            },
            l,
            c,
            Attributes::default(),
            data
        ));
        assert!(!b.add(
            "CIRCLE",
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 0.0
            },
            l,
            c,
            Attributes::default(),
            BTreeMap::new()
        ));
        assert!(b.add(
            "LINE",
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::Y
            },
            l,
            c,
            Attributes::default(),
            BTreeMap::new()
        ));
        let r = b.finish();
        let mut ed = Editor::new(doc);
        let out = ed.execute(&r.command).unwrap();
        assert_eq!(out.created[0], l.0);
        assert_eq!(out.created[1], c.0);
        let first = ed
            .doc()
            .object(kadakas_core::model::ObjectId(out.created[2]))
            .unwrap();
        assert_eq!(first.data["k"], 1);
        assert!(!ed.doc().layer(l).unwrap().visible);
        assert!(!ed.doc().class(c).unwrap().visible);
        assert_eq!(r.report.skipped_total(), 1);
        assert!(ed.undo());
        assert_eq!(ed.doc().layers.len(), 1);
    }

    #[test]
    fn locked_layers_are_not_reused_and_limit_stops() {
        let mut doc = Document::with_sequential_ids();
        doc.layers[0].locked = true;
        let mut b = Builder::new(&Context::of(&doc), "test", 1);
        let l = b.layer("Layer 1", true);
        assert_ne!(l, doc.layers[0].id);
        let c = b.default_class;
        assert!(b.add(
            "LINE",
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X
            },
            l,
            c,
            Attributes::default(),
            BTreeMap::new()
        ));
        assert!(!b.is_full());
        assert!(!b.add(
            "LINE",
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X
            },
            l,
            c,
            Attributes::default(),
            BTreeMap::new()
        ));
        assert!(b.is_full());
        assert!(!b.add(
            "LINE",
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X
            },
            l,
            c,
            Attributes::default(),
            BTreeMap::new()
        ));
        assert_eq!(b.report.notes.len(), 1);
    }

    #[test]
    fn check_shape_rules() {
        let p = |x: f64| {
            kadakas_core::curve::corners(&[DVec2::ZERO, DVec2::new(x, 1.0), DVec2::new(1.0, x)])
        };
        assert!(
            check_shape(&Shape::Polyline {
                points: p(1.0),
                closed: true
            })
            .is_ok()
        );
        assert!(
            check_shape(&Shape::Polyline {
                points: p(f64::NAN),
                closed: true
            })
            .is_err()
        );
        assert!(
            check_shape(&Shape::Polyline {
                points: p(1e12),
                closed: true
            })
            .is_err()
        );
        assert!(
            check_shape(&Shape::Polyline {
                points: p(1.0)[..1].to_vec(),
                closed: false
            })
            .is_err()
        );
        assert!(
            check_shape(&Shape::Polyline3d {
                points: vec![],
                closed: false
            })
            .is_err()
        );
        assert!(
            check_shape(&Shape::LandscapeArea {
                boundary: p(1.0)[..2].to_vec(),
                name: String::new()
            })
            .is_err()
        );
        assert!(
            check_shape(&Shape::Arc {
                center: DVec2::ZERO,
                radius: 1.0,
                start: 1.0,
                end: 1.0
            })
            .is_err()
        );
        assert!(
            check_shape(&Shape::Text {
                at: DVec2::ZERO,
                text: "a".into(),
                height: 0.0
            })
            .is_err()
        );
        let neg = vec![
            kadakas_core::curve::Vertex::new(DVec2::ZERO, VertexKind::Arc { radius: -1.0 }),
            kadakas_core::curve::Vertex::corner(DVec2::X),
        ];
        assert!(
            check_shape(&Shape::Polyline {
                points: neg,
                closed: false
            })
            .is_err()
        );
    }
}
