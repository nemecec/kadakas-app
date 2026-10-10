//! 3D output and section drawings for agents. Native only: it writes files.
//!
//! - `export.file` with the formats `gltf`, `glb` and `obj` (OBJ also writes an `.mtl` file).
//! - `section_viewport.line_add|line_set|create|update|get`: section lines, and viewports on a
//!   sheet that show the cut through the 3D model (or an elevation) with cut faces and
//!   the objects beyond.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::DVec2;
use serde_json::{Map, Value, json};

use cadektor_core::command::Command;
use cadektor_core::model::{Attributes, ClassId, LayerId, LayerKind, ObjectId, Shape};
use cadektor_core::plants::form3d::Detail;
use cadektor_core::section::{self as settings, SectionLine, SectionView};
use cadektor_core::sheets::{Viewport, fit_scale};
use cadektor_x3d::section::{SectionDrawing, SectionSpec};
use cadektor_x3d::{DocumentMaterials, ExportOptions, PlantDetail, X3dError};

use crate::errors::{app, bad};
use crate::sheets_api::{class_ref, layer_list, layer_ref, sheet_ref, viewport_json};
use crate::{RpcError, Session};

pub const LINE_ADD: (&str, &str) = (
    "section_viewport.line_add",
    "Add a section line: a vertical cut plane along the line from a to b. params: {a: [x, y], b: [x, y], name? (default \"A\"), look? (left | right: the side of the line, going from a to b, that the view looks to; default left), mode? (cut | elevation; an elevation cuts nothing and shows the model seen from the line), depth? (far clip in mm; nothing farther than this beyond the line is drawn; default no limit), beyond? (draw the objects beyond the cut; default true), layer?, class?}. The line is a plain line object on a design layer with its settings in data.section_line. Returns {id}.",
);
pub const LINE_SET: (&str, &str) = (
    "section_viewport.line_set",
    "Change a section line. params: {id, a?, b?, name?, look?, mode?, depth? (null removes the limit), beyond?}. Viewports that show the line follow. Returns {id}.",
);
pub const CREATE: (&str, &str) = (
    "section_viewport.create",
    "Add a viewport on a sheet layer that shows a section line: the cut faces of the 3D objects the line crosses, filled with the fill and hatch of their class, the terrain as a profile line, plants as outlines, and the objects beyond as thin lines with hidden lines removed. params: {sheet, line (section line ID), layers? (design layers that the viewport shows; default all), min+max or at+size (frame on the sheet, mm; default the page inside the margins), scale? (denominator or \"fit\"; default fit), name?, show_frame?, cut_weight? (mm on paper, default 0.5), beyond_weight? (default 0.15)}. The viewport follows the model and the line. Its center is a point (distance along the line, height) in mm. Returns the viewport with its id.",
);
pub const UPDATE: (&str, &str) = (
    "section_viewport.update",
    "Change a section viewport. params: {id, and any of line, name, min, max, center ([distance along the line, height] in mm), scale (number or \"fit\"), layers, show_frame, cut_weight, beyond_weight, fit (true recomputes center and scale from the drawing)}. Returns the viewport.",
);
pub const GET: (&str, &str) = (
    "section_viewport.get",
    "The drawing of a section. params: {id (a section viewport or a section line), geometry? (true adds all points; default false)}. Returns {line, length_mm, mode, look, cut_faces: [{object, area_m2, rings}], profiles: [{object, length_m, points}], silhouettes, edge_count, bounds, truncated}. Points are [distance along the line, height] in mm. The drawing is cached by document revision.",
);

const METHODS: [&str; 5] = [
    "section_viewport.line_add",
    "section_viewport.line_set",
    "section_viewport.create",
    "section_viewport.update",
    "section_viewport.get",
];

/// True if this module handles the method.
pub fn knows(method: &str) -> bool {
    METHODS.contains(&method)
}

/// The kind of a 3D export that a request asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Gltf,
    Glb,
    Obj,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Gltf => "gltf",
            Kind::Glb => "glb",
            Kind::Obj => "obj",
        }
    }
}

/// The 3D format that a request asks for: the format name, or without one the file extension.
pub(crate) fn kind_of(format: Option<&str>, path: &Path) -> Option<Kind> {
    let name = match format {
        Some(f) => f.trim().trim_start_matches('.').to_ascii_lowercase(),
        None => path.extension()?.to_str()?.to_ascii_lowercase(),
    };
    match name.as_str() {
        "gltf" => Some(Kind::Gltf),
        "glb" => Some(Kind::Glb),
        "obj" => Some(Kind::Obj),
        _ => None,
    }
}

/// Formats, options and examples, for `describe`.
pub(crate) fn describe() -> Value {
    json!({
        "export": {
            "formats": {
                "gltf": "glTF 2.0 text with the data embedded (.gltf)",
                "glb": "glTF 2.0 binary (.glb)",
                "obj": "Wavefront OBJ with a .mtl file next to it (and texture files if a material has one)"
            },
            "options": {
                "grouping": "object (default: one node per object) | layer | class (one node per layer or class, meshes joined)",
                "units": "m (default, the glTF unit) | mm (the unit of the model)",
                "y_up": "true (default, the glTF axes) | false keeps the Z up axes of the model",
                "georef": "true (default): the extension CADEKTOR_georeference holds the CRS, the map position of the model origin, and the model position of the local origin. Positions in the file stay local, near 0",
                "instancing": "true (default, glTF): objects with the same shape (plants, symbols) share one mesh and have their own nodes",
                "instance_tolerance": "mm, shapes that differ less than this count as equal. Default 1",
                "data": "true (default): the data of each object goes into extras.data",
                "objects": "list of object IDs to export. Default all objects that have 3D geometry",
                "lines": "true (default): 3D lines and polylines become line primitives",
                "plant_detail": "fine (default) | coarse | billboard | auto. Fine plants of one style share a few meshes in the file. auto follows the 3D view, which draws billboards above 20 000 plants"
            },
            "nodes": "Each node has a name and extras {id, kind, name, layer, layer_id, class, class_id, data}. Materials of the Resource Manager (the slots of parts, objects, styles and classes) are written as PBR metallic roughness with base color textures: embedded images in GLB and GLTF, and PNG files next to the OBJ and MTL (they follow the path policy and need overwrite to be replaced). Triangles without a material keep their color.",
            "examples": [
                {"method": "export.file", "params": {"path": "/data/site.glb", "format": "glb", "options": {"grouping": "layer"}, "overwrite": true}},
                {"method": "export.file", "params": {"path": "/data/site.obj", "format": "obj", "options": {"units": "mm", "y_up": false}, "overwrite": true}}
            ]
        },
        "section_viewport": {
            "concepts": "A section line is a line object with settings (data.section_line). The cut plane stands on the line. A section viewport on a sheet shows what the plane cuts: solids (walls, slabs, massing, trunks) as filled faces with the hatch of their class, surfaces (terrain) as a profile line, plants as outlines, and the objects beyond as thin visible lines. mode elevation cuts nothing and draws the lines of everything in front of the far clip. Viewports update with the model and print through sheet.render and export.file (pdf, sheet_svg, sheet_png).",
            "examples": [
                {"method": "section_viewport.line_add", "params": {"a": [0, 5000], "b": [20000, 5000], "name": "A", "look": "left"}},
                {"method": "section_viewport.line_add", "params": {"a": [0, 0], "b": [20000, 0], "name": "North elevation", "mode": "elevation", "depth": 30000}},
                {"method": "section_viewport.create", "params": {"sheet": "Sections", "line": 12, "scale": "fit"}},
                {"method": "section_viewport.get", "params": {"id": 12}}
            ]
        }
    })
}

type Res = Result<Value, RpcError>;

fn map_of(p: Value) -> Result<Map<String, Value>, RpcError> {
    match p {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m),
        _ => Err(bad("params must be an object")),
    }
}

fn point(m: &mut Map<String, Value>, key: &str) -> Result<Option<DVec2>, RpcError> {
    let Some(v) = m.remove(key) else {
        return Ok(None);
    };
    let p: DVec2 = serde_json::from_value(v).map_err(|_| bad(format!("'{key}' must be [x, y]")))?;
    if !p.is_finite() || p.abs().max_element() > cadektor_core::model::MAX_COORD {
        return Err(bad(format!("'{key}' must be finite numbers")));
    }
    Ok(Some(p))
}

fn id_of(m: &mut Map<String, Value>, key: &str) -> Result<ObjectId, RpcError> {
    m.remove(key)
        .and_then(|v| v.as_u64())
        .map(ObjectId)
        .ok_or_else(|| bad(format!("'{key}' is required (an object ID)")))
}

/// The settings of a section line, with the keys that the request gives applied.
fn patched(base: &SectionLine, m: &Map<String, Value>) -> Result<SectionLine, RpcError> {
    let mut v = serde_json::to_value(base).expect("JSON");
    let o = v.as_object_mut().expect("object");
    for key in ["name", "look", "mode", "depth", "beyond"] {
        if let Some(x) = m.get(key) {
            o.insert(key.into(), x.clone());
        }
    }
    let s: SectionLine = serde_json::from_value(v).map_err(bad)?;
    s.validate().map_err(bad)?;
    Ok(s)
}

const LINE_KEYS: [&str; 5] = ["name", "look", "mode", "depth", "beyond"];

fn only_known(m: &Map<String, Value>, known: &[&str]) -> Result<(), RpcError> {
    match m.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(bad(format!("unknown parameter '{k}'"))),
        None => Ok(()),
    }
}

fn weight(m: &Map<String, Value>, key: &str) -> Result<Option<Option<f64>>, RpcError> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => match v.as_f64() {
            Some(w) if w.is_finite() && (0.0..=20.0).contains(&w) => Ok(Some(Some(w))),
            _ => Err(bad(format!(
                "'{key}' must be a line weight from 0 to 20 mm"
            ))),
        },
    }
}

impl Session {
    pub(crate) fn handle_x3d(&mut self, method: &str, p: Value) -> Res {
        let m = map_of(p)?;
        match method {
            "section_viewport.line_add" => self.section_line_add(m),
            "section_viewport.line_set" => self.section_line_set(m),
            "section_viewport.create" => self.section_viewport_create(m),
            "section_viewport.update" => self.section_viewport_update(m),
            "section_viewport.get" => self.section_viewport_get(m),
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    fn section_line_add(&mut self, mut m: Map<String, Value>) -> Res {
        let doc = self.editor.doc();
        let layer: Option<LayerId> = match m.remove("layer") {
            None | Some(Value::Null) => None,
            Some(v) => Some(layer_ref(doc, &v)?),
        };
        if let Some(l) = layer.and_then(|l| doc.layer(l))
            && l.kind != LayerKind::Design
        {
            return Err(bad("a section line goes on a design layer"));
        }
        let class: Option<ClassId> = match m.remove("class") {
            None | Some(Value::Null) => None,
            Some(v) => Some(class_ref(doc, &v)?),
        };
        let a = point(&mut m, "a")?.ok_or_else(|| bad("missing 'a': [x, y]"))?;
        let b = point(&mut m, "b")?.ok_or_else(|| bad("missing 'b': [x, y]"))?;
        if a.distance(b) < 1.0 {
            return Err(bad("the section line needs two points at least 1 mm apart"));
        }
        only_known(&m, &LINE_KEYS)?;
        let s = patched(&SectionLine::default(), &m)?;
        let id = ObjectId(doc.next_id());
        let add = Command::AddObject {
            shape: Shape::Line { a, b },
            layer,
            class,
            attrs: Attributes::default(),
        };
        let set = Command::SetData {
            id,
            set: BTreeMap::from([(
                settings::KEY.to_string(),
                serde_json::to_value(&s).expect("JSON"),
            )]),
            remove: vec![],
        };
        self.editor
            .execute(&Command::Batch {
                commands: vec![add, set],
            })
            .map_err(RpcError::from)?;
        Ok(json!({ "id": id }))
    }

    fn section_line_set(&mut self, mut m: Map<String, Value>) -> Res {
        let id = id_of(&mut m, "id")?;
        let a = point(&mut m, "a")?;
        let b = point(&mut m, "b")?;
        only_known(&m, &LINE_KEYS)?;
        let obj = self
            .editor
            .doc()
            .object(id)
            .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
        let (old_a, old_b) = settings::ends(obj)
            .ok_or_else(|| RpcError::validation(format!("object {id} is not a section line")))?;
        let current = settings::data(obj).expect("checked");
        let s = patched(&current, &m)?;
        let (na, nb) = (a.unwrap_or(old_a), b.unwrap_or(old_b));
        if na.distance(nb) < 1.0 {
            return Err(bad("the section line needs two points at least 1 mm apart"));
        }
        let mut commands = vec![Command::SetData {
            id,
            set: BTreeMap::from([(
                settings::KEY.to_string(),
                serde_json::to_value(&s).expect("JSON"),
            )]),
            remove: vec![],
        }];
        if a.is_some() || b.is_some() {
            commands.push(Command::ModifyObject {
                id,
                shape: Some(Shape::Line { a: na, b: nb }),
                layer: None,
                class: None,
                attrs: None,
            });
        }
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        Ok(json!({ "id": id }))
    }

    /// The section viewport or section line with this ID: its viewport (if any), the section line and the spec.
    fn section_target(
        &self,
        id: ObjectId,
    ) -> Result<(Option<Viewport>, ObjectId, SectionSpec), RpcError> {
        let doc = self.editor.doc();
        let obj = doc
            .object(id)
            .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
        let (vp, line) = match &obj.shape {
            Shape::Viewport(v) => {
                let sv = v.section.as_ref().ok_or_else(|| {
                    RpcError::validation(format!("viewport {id} is not a section viewport"))
                })?;
                (Some((**v).clone()), sv.line)
            }
            _ => (None, id),
        };
        let spec = doc
            .object(line)
            .and_then(SectionSpec::from_line)
            .ok_or_else(|| RpcError::validation(format!("object {line} is not a section line")))?;
        if let Some(p) = spec.problem() {
            return Err(app(p));
        }
        Ok((vp, line, spec))
    }

    /// A viewport that shows all design layers, for sections without a viewport.
    fn all_layers_viewport(&self) -> Viewport {
        let doc = self.editor.doc();
        let mut v = Viewport::new(
            DVec2::ZERO,
            DVec2::ONE,
            DVec2::ZERO,
            100.0,
            doc.layers
                .iter()
                .filter(|l| l.kind == LayerKind::Design)
                .map(|l| l.id)
                .collect(),
        );
        v.snapshot_visibility(doc);
        v
    }

    /// The drawing of a section for a viewport (or for all design layers without one), cached by
    /// document revision in the cache that the sheets use too. So a section is computed once for
    /// `section_viewport.create`, `get`, the sheet and the PDF.
    fn section_drawing(
        &mut self,
        line: ObjectId,
        vp: Option<&Viewport>,
        spec: &SectionSpec,
    ) -> Arc<SectionDrawing> {
        let revision = self.editor.revision();
        let all;
        let vp = match vp {
            Some(v) => v,
            None => {
                all = self.all_layers_viewport();
                &all
            }
        };
        let key = cadektor_print::section::drawing_key(vp, line);
        let store = self.section_cache.clone();
        let mut cache = store.lock().unwrap_or_else(|e| e.into_inner());
        cache.get(revision, key, || {
            let doc = self.editor.doc();
            let shown = cadektor_print::compose::viewport_document(doc, vp);
            let mid = (spec.a + spec.b) / 2.0;
            let ext = cadektor_site::draw::scene_ext(self.site_cache(), revision);
            let scene = cadektor_core::mesh::build_with(
                &shown,
                glam::DVec3::new(mid.x, mid.y, 0.0),
                Some(&ext),
            );
            cadektor_x3d::section::compute(&shown, &scene, spec)
        })
    }

    fn section_viewport_create(&mut self, mut m: Map<String, Value>) -> Res {
        let doc = self.editor.doc();
        let sheet = sheet_ref(doc, m.get("sheet").unwrap_or(&Value::Null))?;
        let line = id_of(&mut m, "line")?;
        let spec = doc
            .object(line)
            .and_then(SectionSpec::from_line)
            .ok_or_else(|| bad(format!("object {line} is not a section line")))?;
        let page = doc.sheet(sheet).expect("checked").size_mm();
        let layers = match m.get("layers") {
            None | Some(Value::Null) => doc
                .layers
                .iter()
                .filter(|l| l.kind == LayerKind::Design)
                .map(|l| l.id)
                .collect(),
            Some(v) => layer_list(doc, v)?,
        };
        let get_pt = |key: &str| -> Result<Option<DVec2>, RpcError> {
            match m.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => serde_json::from_value(v.clone())
                    .map(Some)
                    .map_err(|_| bad(format!("'{key}' must be [x, y]"))),
            }
        };
        let (min, max) = match (
            get_pt("min")?,
            get_pt("max")?,
            get_pt("at")?,
            get_pt("size")?,
        ) {
            (Some(a), Some(b), _, _) => (a, b),
            (_, _, Some(a), Some(sz)) => (a, a + sz),
            (None, None, None, None) => (DVec2::new(15.0, 55.0), page - DVec2::splat(15.0)),
            _ => return Err(bad("give min and max, or at and size")),
        };
        let mut vp = Viewport::new(min, max, DVec2::ZERO, 100.0, layers);
        vp.view = "section".into();
        vp.name = m
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                settings::data(doc.object(line).expect("checked"))
                    .map(|s| s.name)
                    .unwrap_or_default()
            });
        vp.show_frame = m
            .get("show_frame")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut sv = SectionView::new(line);
        sv.cut_weight = weight(&m, "cut_weight")?.flatten();
        sv.beyond_weight = weight(&m, "beyond_weight")?.flatten();
        vp.section = Some(sv);
        vp.snapshot_visibility(doc);
        if let Some(p) = vp.problem() {
            return Err(bad(p));
        }
        let d = self.section_drawing(line, Some(&vp), &spec);
        let fixed = match m.get("scale") {
            None | Some(Value::Null) => None,
            Some(Value::String(t)) if t.eq_ignore_ascii_case("fit") => None,
            Some(v) => Some(
                v.as_f64()
                    .ok_or_else(|| bad("scale is a number such as 100, or \"fit\""))?,
            ),
        };
        fit_viewport(&mut vp, &d, fixed.is_none());
        if let Some(s) = fixed {
            vp.scale = s;
        }
        if let Some(c) = get_pt("center")? {
            vp.center = c;
        }
        if let Some(p) = vp.problem() {
            return Err(bad(p));
        }
        let out = self
            .editor
            .execute(&Command::AddObject {
                shape: Shape::Viewport(Box::new(vp)),
                layer: Some(sheet),
                class: None,
                attrs: Attributes::default(),
            })
            .map_err(RpcError::from)?;
        viewport_json(self.editor.doc(), ObjectId(out.created[0]))
    }

    fn section_viewport_update(&mut self, mut m: Map<String, Value>) -> Res {
        let id = id_of(&mut m, "id")?;
        let (vp, _, _) = self.section_target(id)?;
        let mut vp = vp.ok_or_else(|| bad(format!("object {id} is not a section viewport")))?;
        let doc = self.editor.doc();
        let mut v = serde_json::to_value(&vp).expect("JSON");
        let o = v.as_object_mut().expect("object");
        for key in ["name", "min", "max", "center", "show_frame"] {
            if let Some(x) = m.get(key) {
                o.insert(key.into(), x.clone());
            }
        }
        if let Some(x) = m.get("layers") {
            o.insert("layers".into(), json!(layer_list(doc, x)?));
        }
        let fixed = match m.get("scale") {
            None | Some(Value::Null) => None,
            Some(Value::String(t)) if t.eq_ignore_ascii_case("fit") => None,
            Some(x) => Some(
                x.as_f64()
                    .ok_or_else(|| bad("scale is a number such as 100, or \"fit\""))?,
            ),
        };
        if let Some(s) = fixed {
            o.insert("scale".into(), json!(s));
        }
        let mut next: Viewport = serde_json::from_value(v).map_err(bad)?;
        let mut sv = vp.section.clone().expect("checked");
        if let Some(l) = m.get("line") {
            let l = ObjectId(l.as_u64().ok_or_else(|| bad("line is an object ID"))?);
            if doc.object(l).and_then(SectionSpec::from_line).is_none() {
                return Err(bad(format!("object {l} is not a section line")));
            }
            sv.line = l;
        }
        if let Some(w) = weight(&m, "cut_weight")? {
            sv.cut_weight = w;
        }
        if let Some(w) = weight(&m, "beyond_weight")? {
            sv.beyond_weight = w;
        }
        next.section = Some(sv.clone());
        if let Some(p) = next.problem() {
            return Err(bad(p));
        }
        let fit = m.get("fit").and_then(Value::as_bool).unwrap_or(false)
            || matches!(m.get("scale"), Some(Value::String(t)) if t.eq_ignore_ascii_case("fit"));
        if fit {
            let spec = doc
                .object(sv.line)
                .and_then(SectionSpec::from_line)
                .ok_or_else(|| RpcError::not_found("the section line is gone"))?;
            let d = self.section_drawing(sv.line, Some(&next), &spec);
            fit_viewport(&mut next, &d, true);
        }
        vp = next;
        self.editor
            .execute(&Command::ModifyObject {
                id,
                shape: Some(Shape::Viewport(Box::new(vp))),
                layer: None,
                class: None,
                attrs: None,
            })
            .map_err(RpcError::from)?;
        viewport_json(self.editor.doc(), id)
    }

    fn section_viewport_get(&mut self, mut m: Map<String, Value>) -> Res {
        let id = id_of(&mut m, "id")?;
        let geometry = m.get("geometry").and_then(Value::as_bool).unwrap_or(false);
        let (vp, line, spec) = self.section_target(id)?;
        // The cache key is the object that was asked for, so a viewport and its line can differ.
        let d = self.section_drawing(line, vp.as_ref(), &spec);
        let pts = |v: &[DVec2]| -> Value { if geometry { json!(v) } else { json!(v.len()) } };
        let faces: Vec<Value> = d
            .faces
            .iter()
            .map(|f| {
                let area: f64 = f
                    .rings
                    .iter()
                    .map(|r| cadektor_core::geom::signed_area(r))
                    .sum();
                let rings: Vec<Value> = f.rings.iter().map(|r| pts(r)).collect();
                json!({"object": f.object, "area_m2": area / 1.0e6, "rings": rings})
            })
            .collect();
        let profiles: Vec<Value> = d
            .profiles
            .iter()
            .map(|p| {
                let len: f64 = p.points.windows(2).map(|w| w[0].distance(w[1])).sum();
                json!({"object": p.object, "length_m": len / 1000.0, "points": pts(&p.points)})
            })
            .collect();
        let silhouettes: Vec<Value> = d
            .silhouettes
            .iter()
            .map(|f| json!({"object": f.object, "rings": f.rings.len()}))
            .collect();
        let mut out = json!({
            "line": line,
            "length_mm": d.length,
            "mode": spec.mode.id(),
            "look": spec.look.id(),
            "depth": spec.depth,
            "cut_faces": faces,
            "profiles": profiles,
            "silhouettes": silhouettes,
            "edge_count": d.edges.len(),
            "bounds": d.bounds.map(|(lo, hi)| json!({"min": lo, "max": hi})),
            "truncated": d.truncated,
        });
        if geometry {
            let edges: Vec<Value> = d
                .edges
                .iter()
                .map(|e| json!({"object": e.object, "a": e.a, "b": e.b, "depth": e.depth}))
                .collect();
            out["edges"] = json!(edges);
        }
        Ok(out)
    }

    /// `export.file` for gltf, glb and obj.
    pub(crate) fn export_x3d_file(
        &mut self,
        kind: Kind,
        path: PathBuf,
        options: &Value,
        overwrite: bool,
        checked: bool,
    ) -> Res {
        let opts: ExportOptions = match options {
            Value::Null => ExportOptions::default(),
            v => serde_json::from_value(v.clone()).map_err(|e| bad(format!("options: {e}")))?,
        };
        opts.validate().map_err(|e| bad(format!("options: {e}")))?;
        // The OBJ refers to the MTL by its cleaned name, so the file gets that name.
        let mtl = {
            let m = path.with_extension("mtl");
            let name = m
                .file_name()
                .and_then(|n| n.to_str())
                .map(cadektor_x3d::obj::clean)
                .unwrap_or_else(|| "model.mtl".into());
            m.with_file_name(name)
        };
        let paths: Vec<(PathBuf, &str)> = if kind == Kind::Obj {
            vec![(path.clone(), "obj"), (mtl.clone(), "mtl")]
        } else {
            vec![(path.clone(), kind.name())]
        };
        let mut checked_paths = Vec::new();
        for (p, ext) in &paths {
            let p = if checked {
                self.check_path(p, true, &[ext]).map_err(RpcError::policy)?
            } else {
                p.clone()
            };
            if checked && self.paths.is_restricted() && p.exists() && !overwrite {
                return Err(RpcError::conflict(format!(
                    "{} exists: pass \"overwrite\": true to replace it",
                    p.display()
                )));
            }
            checked_paths.push(p);
        }
        let scene = match opts.plant_detail {
            PlantDetail::Auto => self.scene(),
            PlantDetail::Fine => self.scene_with_detail(Detail::Fine),
            PlantDetail::Coarse => self.scene_with_detail(Detail::Coarse),
            PlantDetail::Billboard => self.scene_with_detail(Detail::Billboard),
        };
        let doc = self.editor.doc();
        let materials = DocumentMaterials::new(doc, &scene);
        let rpc = |e: X3dError| match e {
            X3dError::Options(_) => bad(e),
            e => app(e),
        };
        let write = |p: &Path, bytes: &[u8]| {
            cadektor_sys::fsio::write_atomic(p, bytes)
                .map_err(|e| app(format!("{}: {e}", p.display())))
        };
        let mut files = vec![checked_paths[0].clone()];
        let stats = match kind {
            Kind::Gltf => {
                let (text, stats) =
                    cadektor_x3d::export_gltf(doc, &scene, &opts, &materials).map_err(rpc)?;
                write(&checked_paths[0], text.as_bytes())?;
                stats
            }
            Kind::Glb => {
                let (bytes, stats) =
                    cadektor_x3d::export_glb(doc, &scene, &opts, &materials).map_err(rpc)?;
                write(&checked_paths[0], &bytes)?;
                stats
            }
            Kind::Obj => {
                let name = mtl
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("model.mtl");
                let (obj, stats) =
                    cadektor_x3d::export_obj(doc, &scene, &opts, &materials, name).map_err(rpc)?;
                // The texture files follow the same path policy, and nothing is written before all pass.
                let mut texture_paths = Vec::new();
                for (n, _) in &obj.textures {
                    let tp = checked_paths[0].with_file_name(n);
                    let ext = if n.ends_with(".jpg") { "jpg" } else { "png" };
                    let tp = if checked {
                        self.check_path(&tp, true, &[ext])
                            .map_err(RpcError::policy)?
                    } else {
                        tp
                    };
                    if checked && self.paths.is_restricted() && tp.exists() && !overwrite {
                        return Err(RpcError::conflict(format!(
                            "{} exists: pass \"overwrite\": true to replace it",
                            tp.display()
                        )));
                    }
                    texture_paths.push(tp);
                }
                write(&checked_paths[0], obj.obj.as_bytes())?;
                write(&checked_paths[1], obj.mtl.as_bytes())?;
                files.push(checked_paths[1].clone());
                for (tp, (_, data)) in texture_paths.into_iter().zip(&obj.textures) {
                    write(&tp, data)?;
                    files.push(tp);
                }
                stats
            }
        };
        Ok(json!({
            "path": checked_paths[0],
            "format": kind.name(),
            "files": files,
            "exported": stats.objects,
            "report": stats,
        }))
    }
}

/// Centers the drawing in the frame and, with `scale`, picks the largest standard scale that fits.
fn fit_viewport(vp: &mut Viewport, d: &SectionDrawing, scale: bool) {
    if let Some((lo, hi)) = d.bounds {
        vp.center = (lo + hi) / 2.0;
        if scale {
            vp.scale = fit_scale(hi - lo, vp.max - vp.min);
        }
    } else {
        vp.center = DVec2::new(d.length / 2.0, 0.0);
    }
}
