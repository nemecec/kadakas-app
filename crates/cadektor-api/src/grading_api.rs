//! Agent API for grading and site analysis: landscape walls, grade objects, the slope and
//! elevation analysis of site models, spot elevations and stakes, and site sections.
//!
//! Methods: `wall_style.create|list|update|delete`, `wall.add|set|report`, `grade.add|set|get`,
//! `site.analysis`, `spot.add|set|get`, `section.add|set|profile|place`.
//! The objects are plain shapes with settings in `Object::data` (see `cadektor_site::grading`).

use cadektor_core::uid::AsUid;
use std::collections::BTreeMap;

use glam::DVec2;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use cadektor_core::command::Command;
use cadektor_core::curve::Vertex;
use cadektor_core::model::{
    Attributes, ClassId, Color, Document, LayerId, LayerKind, Object, ObjectId, Shape,
};
use cadektor_core::resource::ResourceId;
use cadektor_site::SiteResult;
use cadektor_site::grading::{self, analysis, grade, section, spot, wall};

use crate::errors::{app, bad};
use crate::{RpcError, Session};

pub const WALL_STYLE_CREATE: (&str, &str) = (
    "wall_style.create",
    "Create a wall style (a resource). params: {name, thickness? (mm, default 300; with components it is their total), material?, components? ([{name, material, thickness}], across the thickness), coping? ({height, overhang? (past each face), material?} or null), color? ([r,g,b,a])}. Returns {id}.",
);
pub const WALL_STYLE_LIST: (&str, &str) = (
    "wall_style.list",
    "Wall styles with the number of walls that use each. Returns {styles: [{id, name, used_by, ...fields}]}.",
);
pub const WALL_STYLE_UPDATE: (&str, &str) = (
    "wall_style.update",
    "Change a wall style. params: {id (ID or name), name? (renames), ...fields as in create}. Walls that use the style follow.",
);
pub const WALL_STYLE_DELETE: (&str, &str) = (
    "wall_style.delete",
    "Delete a wall style. params: {id (ID or name), reassign?}. Fails if walls use it, unless reassign names another wall style.",
);
pub const WALL_ADD: (&str, &str) = (
    "wall.add",
    "Add a landscape wall: a path (curves allowed) with a thickness and a top that follows a rule. params: {path: [vertices], closed?, thickness? (mm, default 300), top? ({mode: fixed, elevation} | {mode: terrain, offset} (the ground plus an offset; default offset 900) | {mode: slope, start, end} (heights at both ends)), bottom? (existing | proposed: the ground that the bottom and a terrain top follow), embed? (mm below the ground), style? (ID or name; its thickness and coping win), site? (site model ID; default the first on the layer), site_modifier? (the wall is a retaining edge: the ground behind it is set to the top of the wall, the other side keeps its height, and the wall adds no earth; needs a fixed or slope top and follows the existing ground), retains? (left | right: the side of the path, seen from its start, where the ground is at the top; default left), backfill? (mm the retained ground reaches behind the wall, default 1500), layer?, class?}. The 3D solid stands between the ground and the top. Returns {id, quantities}.",
);
pub const WALL_SET: (&str, &str) = (
    "wall.set",
    "Change walls. params: {id | ids, path? (replaces the center line; one wall only), ...settings as in wall.add}. Returns {ids}.",
);
pub const WALL_REPORT: (&str, &str) = (
    "wall.report",
    "Wall quantities. params: {ids?} (default all walls). Returns {rows: [{id, style, length_m, thickness_mm, face_area_m2 (one face), both_faces_m2, volume_m3, coping_volume_m3, min_height_mm, max_height_mm, avg_height_mm, top_start_mm, top_end_mm, bottom_start_mm, bottom_end_mm, outside_model (stations off the site model), partly_off_model}], totals}. Off the model a wall takes the height of the nearest edge of the model. Stations lie at every corner and at most 500 mm apart; the height between two stations is linear.",
);
mod retaining;
#[cfg(test)]
mod tests;

pub use retaining::{PAD_ADD, PAD_RETAINING_EDGE};

pub const GRADE_ADD: (&str, &str) = (
    "grade.add",
    "Add a grade object: a line from a to b, or a curve (path), with elevations. Give elevation_a and elevation_b, or elevation_a and a slope, or snap. On a curve the slope is along the curve. params: {a: [x, y], b: [x, y] | path: [vertices] (a curve with arcs; instead of a and b), site_modifier? (the grade changes the proposed surface: a strip of width along the path follows its elevations; no snap), width? (mm, default 1500), elevation_a?, elevation_b?, slope? ({percent: 2} | {ratio: 50} (1:50) | {angle: 1.2} (degrees); positive rises from a to b), snap? (existing | proposed: both elevations are read from the site model, live), site?, label?, end_tags?, layer?, class?}. Returns {id, grade}.",
);
pub const GRADE_SET: (&str, &str) = (
    "grade.set",
    "Change a grade object. params: {id, a?, b?, path? (replaces the shape with a curve), ...settings as in grade.add; null removes elevation_b, slope or snap}. The slope, the end elevation and the label follow the line. Returns {id, grade}.",
);
pub const GRADE_GET: (&str, &str) = (
    "grade.get",
    "The solved grade. params: {id}. Returns {id, length_mm, elevation_a, elevation_b, rise_mm, percent, ratio (run per unit of rise or null on level ground), angle_deg, falls, label}.",
);
pub const SITE_ANALYSIS: (&str, &str) = (
    "site.analysis",
    "Slope or elevation analysis of a site model. params: {id, mode? (off | slope | elevation), surface? (existing | proposed), breaks? (class limits, ascending: slope in percent, elevation in mm; N limits make N+1 bands; [] for the defaults), colors? ([[r,g,b,a]], one per band), opacity? (0 to 1, the 2D tint), legend? ({rect: [[x0, y0], [x1, y1]], title?, show_areas?}: adds a legend object on the layer of the site model)}. With no setting it only reads. The 2D plan and the 3D view show the colors. Returns {id, mode, surface, breaks, opacity, bands: [{index, from, to, label, color, triangles, area_m2, surface_area_m2, percent}], legend?}. Default slope limits: 2, 5, 10, 15, 25, 50 percent. Default elevation limits: 7 equal steps over the height range.",
);
pub const SPOT_ADD: (&str, &str) = (
    "spot.add",
    "Add a spot elevation or a stake: a point that reads the ground height from the site model whenever it is drawn. params: {at: [x, y], site?, show? (existing | proposed | both), kind? (spot | stake), prefix?, name? (stake name), decimals? (0 to 4, default 2), label_height? (mm on paper, default 2.5), layer?, class?}. A stake also shows northing and easting from the georeference. Returns {id, reading}.",
);
pub const SPOT_SET: (&str, &str) = (
    "spot.set",
    "Change a spot elevation. params: {id, at?, ...settings as in spot.add}. Returns {id, reading}.",
);
pub const SPOT_GET: (&str, &str) = (
    "spot.get",
    "Read a spot elevation, or any point. params: {id} or {at: [x, y], site?}. Returns {existing, proposed, modified, label, northing_m, easting_m, georeferenced}. Heights are in mm and null outside the site model.",
);
pub const SECTION_ADD: (&str, &str) = (
    "section.add",
    "Add a section line. params: {a: [x, y], b: [x, y], name? (default A), exaggeration? (vertical, default 2), step? (mm between samples when the line leaves the model), band? (mm: plants this near the line show), walls?, hardscapes?, plants? (true/false), site?, layer?, class?}. Returns {id}.",
);
pub const SECTION_SET: (&str, &str) = (
    "section.set",
    "Change a section line. params: {id, a?, b?, ...settings as in section.add}. Returns {id}.",
);
pub const SECTION_PROFILE: (&str, &str) = (
    "section.profile",
    "The profile of a section: the existing and proposed ground and the objects cut. params: {id}. Returns {length_mm, modified, existing: [[[s, z]...]] (one run of distance and elevation in mm; a point at every triangle edge, so it equals the site model there; off the model the nearest edge height), proposed, off_model: [[s0, s1]] (where the line is off the site model), walls: [{id, s, width, top, bottom}], hardscapes: [{id, s0, s1, top0, top1, thickness}], plants: [{id, s, offset, spread, height, base}], z_min, z_max}.",
);
pub const SECTION_PLACE: (&str, &str) = (
    "section.place",
    "Draw the profile as shapes on a layer, with the vertical exaggeration. Shapes placed before for the same section are replaced. params: {id, at: [x, y] (where the start of the section goes), layer? (default the layer of the section; may be a sheet layer), scale? (drawing scale denominator, for example 100; the drawing is divided by it. Default: 1 on a design layer, the scale of the section layer on a sheet layer)}. One undo step. Returns {ids, bounds: [[x0, y0], [x1, y1]]}. The shapes have data site_section_profile = {section, role}.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "walls": "A landscape wall is a polyline with settings (thickness, top rule, bottom ground, style). Its 3D solid stands between the ground and the top. With site_modifier it also cuts a vertical break into the proposed surface.",
        "analysis": "Slope and elevation analysis are display modes of a site model. Every triangle falls into one band. Use a legend object to show the bands on the plan.",
        "examples": [
            {"method": "wall_style.create", "params": {"name": "Block wall", "components": [{"name": "Block", "material": "Concrete", "thickness": 200}, {"name": "Render", "material": "Lime", "thickness": 20}], "coping": {"height": 50, "overhang": 20, "material": "Stone"}}},
            {"method": "wall.add", "params": {"path": [[0, 0], {"p": [6000, 0], "kind": "arc", "radius": 1500}, [6000, 5000]], "top": {"mode": "terrain", "offset": 900}, "style": "Block wall", "embed": 200}},
            {"method": "wall.add", "params": {"path": [[0, 8000], [9000, 8000]], "top": {"mode": "slope", "start": 1200, "end": 2000}, "site_modifier": true}},
            {"method": "wall.report", "params": {}},
            {"method": "grade.add", "params": {"a": [0, 0], "b": [12000, 0], "elevation_a": 10000, "slope": {"ratio": -50}}},
            {"method": "grade.add", "params": {"a": [0, 2000], "b": [12000, 2500], "snap": "existing"}},
            {"method": "site.analysis", "params": {"id": 3, "mode": "slope", "breaks": [2, 5, 10], "legend": {"rect": [[12000, 0], [16000, 6000]], "show_areas": true}}},
            {"method": "spot.add", "params": {"at": [3000, 4000], "show": "both", "prefix": "TC"}},
            {"method": "spot.add", "params": {"at": [3000, 5000], "kind": "stake", "name": "S1"}},
            {"method": "section.add", "params": {"a": [0, 5000], "b": [20000, 5000], "name": "A", "exaggeration": 3}},
            {"method": "section.place", "params": {"id": 9, "at": [0, -40000]}}
        ],
    })
}

/// True if this module handles the method.
pub fn knows(method: &str) -> bool {
    matches!(
        method,
        "wall_style.create"
            | "wall_style.list"
            | "wall_style.update"
            | "wall_style.delete"
            | "wall.add"
            | "wall.set"
            | "wall.report"
            | "grade.add"
            | "grade.set"
            | "grade.get"
            | "site.analysis"
            | "spot.add"
            | "spot.set"
            | "spot.get"
            | "section.add"
            | "section.set"
            | "section.profile"
            | "section.place"
            | "pad.add"
            | "pad.retaining_edge"
    )
}

fn map_of(p: Value) -> Result<Map<String, Value>, RpcError> {
    match p {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m),
        _ => Err(bad("params must be an object")),
    }
}

/// Removes `key` from the parameters and reads it as a point.
fn take_point(m: &mut Map<String, Value>, key: &str) -> Result<Option<DVec2>, RpcError> {
    let Some(v) = m.remove(key) else {
        return Ok(None);
    };
    let p: DVec2 = serde_json::from_value(v).map_err(|_| bad(format!("'{key}' must be [x, y]")))?;
    if !p.is_finite() || p.abs().max_element() > cadektor_core::model::MAX_COORD {
        return Err(bad(format!("'{key}' must be finite numbers")));
    }
    Ok(Some(p))
}

/// Removes `path` from the parameters and reads it as vertices (at least two).
fn take_path(m: &mut Map<String, Value>) -> Result<Option<Vec<Vertex>>, RpcError> {
    let Some(v) = m.remove("path") else {
        return Ok(None);
    };
    let path: Vec<Vertex> = serde_json::from_value(v).map_err(|e| bad(format!("bad path: {e}")))?;
    if path.len() < 2 {
        return Err(bad("a grade path needs 2 or more points"));
    }
    if !path.iter().all(Vertex::is_finite) {
        return Err(bad("the path numbers must be finite"));
    }
    Ok(Some(path))
}

fn need_point(m: &mut Map<String, Value>, key: &str) -> Result<DVec2, RpcError> {
    take_point(m, key)?.ok_or_else(|| bad(format!("missing '{key}': [x, y]")))
}

fn take_id(m: &mut Map<String, Value>, key: &str) -> Result<ObjectId, RpcError> {
    let v = m
        .remove(key)
        .ok_or_else(|| bad(format!("missing '{key}'")))?;
    serde_json::from_value(v).map_err(|_| bad(format!("'{key}' must be an object ID")))
}

fn take_where(m: &mut Map<String, Value>) -> Result<(Option<LayerId>, Option<ClassId>), RpcError> {
    let layer = m
        .remove("layer")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| bad("'layer' must be a layer ID"))?;
    let class = m
        .remove("class")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| bad("'class' must be a class ID"))?;
    Ok((layer, class))
}

fn names<T: Serialize + Default>() -> Vec<String> {
    serde_json::to_value(T::default())
        .expect("settings are JSON")
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

/// The settings after applying the fields of `patch`. Unknown fields are refused.
fn patched<T: Serialize + DeserializeOwned + Default>(
    base: &T,
    patch: &Map<String, Value>,
    label: &str,
) -> Result<T, RpcError> {
    let keys = names::<T>();
    let mut v = serde_json::to_value(base).expect("settings are JSON");
    for (k, x) in patch {
        if !keys.contains(k) {
            return Err(bad(format!(
                "unknown {label} field '{k}'. Fields: {}",
                keys.join(", ")
            )));
        }
        v[k] = x.clone();
    }
    serde_json::from_value(v).map_err(bad)
}

fn wall_style_id(doc: &Document, v: &Value) -> Result<ResourceId, RpcError> {
    let found = match v {
        Value::String(s) => cadektor_core::uid::Uid::parse(s.trim())
            .map(ResourceId)
            .filter(|id| wall::style_of(doc, *id).is_some())
            .or_else(|| {
                doc.resources
                    .iter()
                    .find(|r| r.name == *s && wall::style_of(doc, r.id).is_some())
                    .map(|r| r.id)
            }),
        _ => None,
    };
    found
        .filter(|id| wall::style_of(doc, *id).is_some())
        .ok_or_else(|| bad(format!("no wall style {v}")))
}

/// Replaces a style name in the patch with its ID.
fn resolve_style(doc: &Document, m: &mut Map<String, Value>) -> Result<(), RpcError> {
    if let Some(v) = m.get("style").filter(|v| !v.is_null()).cloned() {
        m.insert("style".into(), json!(wall_style_id(doc, &v)?));
    }
    Ok(())
}

fn used_by(doc: &Document, id: ResourceId) -> usize {
    doc.objects
        .values()
        .filter(|o| wall::data(o).is_some_and(|d| d.style == Some(id)))
        .count()
}

fn style_row(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("checked");
    let mut v =
        serde_json::to_value(wall::style_of(doc, id).expect("checked")).expect("style is JSON");
    v["id"] = json!(id);
    v["name"] = json!(r.name);
    v["used_by"] = json!(used_by(doc, id));
    v["total_thickness"] = json!(wall::style_of(doc, id).expect("checked").total_thickness());
    v
}

/// The meters of a quantity in mm, rounded for output.
fn r(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

impl Session {
    pub(crate) fn handle_grading(&mut self, method: &str, p: Value) -> Result<Value, RpcError> {
        let m = map_of(p)?;
        match method {
            "wall_style.create" => self.wall_style_create(m),
            "wall_style.list" => {
                let doc = self.editor.doc();
                let rows: Vec<Value> = doc
                    .resources
                    .iter()
                    .filter(|r| wall::style_of(doc, r.id).is_some())
                    .map(|r| style_row(doc, r.id))
                    .collect();
                Ok(json!({ "styles": rows }))
            }
            "wall_style.update" => self.wall_style_update(m),
            "wall_style.delete" => self.wall_style_delete(m),
            "wall.add" => self.wall_add(m),
            "wall.set" => self.wall_set(m),
            "wall.report" => self.wall_report(m),
            "grade.add" => self.grade_add(m),
            "grade.set" => self.grade_set(m),
            "grade.get" => self.grade_get(m),
            "site.analysis" => self.site_analysis(m),
            "spot.add" => self.spot_add(m),
            "spot.set" => self.spot_set(m),
            "spot.get" => self.spot_get(m),
            "section.add" => self.section_add(m),
            "section.set" => self.section_set(m),
            "section.profile" => self.section_profile(m),
            "section.place" => self.section_place(m),
            "pad.add" => self.pad_add(Value::Object(m)),
            "pad.retaining_edge" => self.pad_retaining_edge(Value::Object(m)),
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    /// Adds a shape and its settings as one undo step.
    fn add_object_with(
        &mut self,
        shape: Shape,
        layer: Option<LayerId>,
        class: Option<ClassId>,
        key: &str,
        data: Value,
    ) -> Result<ObjectId, RpcError> {
        let id = ObjectId(self.editor.doc().next_ids(1)[0]);
        let add = Command::AddObject {
            shape,
            layer,
            class,
            attrs: Attributes::default(),
        };
        let set = Command::SetData {
            id,
            set: BTreeMap::from([(key.to_string(), data)]),
            remove: vec![],
        };
        let out = self
            .editor
            .execute(&Command::Batch {
                commands: vec![add, set],
            })
            .map_err(RpcError::from)?;
        debug_assert_eq!(out.created.first(), Some(&id.0));
        self.prune_selection();
        Ok(id)
    }

    /// Changes the shape and the settings of an object as one undo step.
    fn change_object(
        &mut self,
        id: ObjectId,
        shape: Option<Shape>,
        key: &str,
        data: Value,
    ) -> Result<(), RpcError> {
        let mut commands = Vec::new();
        if let Some(shape) = shape {
            commands.push(Command::ModifyObject {
                id,
                shape: Some(shape),
                layer: None,
                class: None,
                attrs: None,
            });
        }
        commands.push(Command::SetData {
            id,
            set: BTreeMap::from([(key.to_string(), data)]),
            remove: vec![],
        });
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        Ok(())
    }

    fn object(&self, id: ObjectId) -> Result<&Object, RpcError> {
        self.editor
            .doc()
            .object(id)
            .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))
    }

    /// The site model that serves an object.
    fn site_for(
        &self,
        obj: &Object,
        named: Option<ObjectId>,
    ) -> Option<std::sync::Arc<SiteResult>> {
        grading::host(
            self.editor.doc(),
            &self.site_cache,
            self.editor.revision(),
            obj.layer,
            named,
        )
    }

    // ---- wall styles ----

    fn wall_style_create(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let name = m
            .remove("name")
            .and_then(|v| v.as_str().map(str::to_string))
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| bad("give a name"))?;
        let style: wall::WallStyle = patched(&wall::WallStyle::default(), &m, "wall style")?;
        style.validate().map_err(bad)?;
        let out = self
            .editor
            .execute(&Command::AddResource {
                name,
                body: style.to_body(),
            })
            .map_err(RpcError::from)?;
        Ok(json!({ "id": out.created[0] }))
    }

    fn wall_style_update(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let doc = self.editor.doc();
        let id = wall_style_id(doc, m.get("id").ok_or_else(|| bad("missing 'id'"))?)?;
        m.remove("id");
        let name = m
            .remove("name")
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad("'name' must be text"))
            })
            .transpose()?;
        let style = patched(&wall::style_of(doc, id).expect("checked"), &m, "wall style")?;
        style.validate().map_err(bad)?;
        self.editor
            .execute(&Command::ModifyResource {
                id,
                name,
                body: Some(style.to_body()),
            })
            .map_err(RpcError::from)?;
        Ok(json!({ "id": id }))
    }

    fn wall_style_delete(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let doc = self.editor.doc();
        let id = wall_style_id(doc, m.get("id").ok_or_else(|| bad("missing 'id'"))?)?;
        let reassign = m
            .remove("reassign")
            .filter(|v| !v.is_null())
            .map(|v| wall_style_id(doc, &v))
            .transpose()?;
        let users: Vec<ObjectId> = doc
            .objects
            .values()
            .filter(|o| wall::data(o).is_some_and(|d| d.style == Some(id)))
            .map(|o| o.id)
            .collect();
        if !users.is_empty() && reassign.is_none() {
            return Err(app(format!(
                "{} wall(s) use this style. Give 'reassign' with another wall style",
                users.len()
            )));
        }
        let mut commands = Vec::new();
        for u in users {
            let mut d = wall::data(doc.object(u).expect("listed")).expect("listed");
            d.style = reassign;
            commands.push(Command::SetData {
                id: u,
                set: BTreeMap::from([(
                    wall::KEY.to_string(),
                    serde_json::to_value(d).expect("JSON"),
                )]),
                remove: vec![],
            });
        }
        commands.push(Command::DeleteResource { id, reassign: None });
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        Ok(json!({ "deleted": id }))
    }

    // ---- walls ----

    fn wall_row(&self, id: ObjectId) -> Option<Value> {
        let doc = self.editor.doc();
        let obj = doc.object(id)?;
        let d = wall::data(obj)?;
        let site = self.site_for(obj, d.site);
        let ground = site.as_ref().map(|s| grading::SiteGround::new(s));
        let w = wall::evaluate(doc, obj, ground.as_ref().map(|g| g as grading::Ground<'_>))?;
        let q = w.quantities;
        let (first, last) = (w.stations.first()?, w.stations.last()?);
        Some(json!({
            "id": id,
            "style": w.data.style,
            "style_name": w.style_name,
            "length_m": r(q.length / 1e3),
            "thickness_mm": w.thickness,
            "face_area_m2": r(q.face_area / 1e6),
            "both_faces_m2": r(2.0 * q.face_area / 1e6),
            "volume_m3": r(q.volume / 1e9),
            "coping_volume_m3": r(q.coping_volume / 1e9),
            "min_height_mm": q.min_height,
            "max_height_mm": q.max_height,
            "avg_height_mm": q.avg_height,
            "top_start_mm": first.top,
            "top_end_mm": last.top,
            "bottom_start_mm": first.bottom,
            "bottom_end_mm": last.bottom,
            "outside_model": w.outside(),
            "partly_off_model": w.partly_off_model(),
            "site_modifier": w.data.site_modifier,
        }))
    }

    fn wall_add(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let (layer, class) = take_where(&mut m)?;
        let path: Vec<Vertex> = serde_json::from_value(
            m.remove("path")
                .ok_or_else(|| bad("missing 'path': [vertices]"))?,
        )
        .map_err(|e| bad(format!("bad path: {e}")))?;
        let closed = m
            .remove("closed")
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| bad("'closed' must be true or false"))
            })
            .transpose()?
            .unwrap_or(false);
        resolve_style(self.editor.doc(), &mut m)?;
        let data: wall::WallData = patched(&wall::WallData::default(), &m, "wall")?;
        data.validate().map_err(bad)?;
        let id = self.add_object_with(
            Shape::Polyline {
                points: path,
                closed,
            },
            layer,
            class,
            wall::KEY,
            serde_json::to_value(&data).expect("JSON"),
        )?;
        Ok(json!({ "id": id, "quantities": self.wall_row(id) }))
    }

    fn wall_set(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let ids = ids_of(&mut m)?;
        let path = m
            .remove("path")
            .map(|v| {
                serde_json::from_value::<Vec<Vertex>>(v).map_err(|e| bad(format!("bad path: {e}")))
            })
            .transpose()?;
        if path.is_some() && ids.len() != 1 {
            return Err(bad("path changes one wall at a time"));
        }
        resolve_style(self.editor.doc(), &mut m)?;
        let mut commands = Vec::new();
        for id in &ids {
            let obj = self.object(*id)?;
            let current = wall::data(obj)
                .ok_or_else(|| RpcError::validation(format!("object {id} is not a wall")))?;
            let data: wall::WallData = patched(&current, &m, "wall")?;
            data.validate().map_err(bad)?;
            if let Some(path) = &path {
                let Shape::Polyline { closed, .. } = obj.shape else {
                    unreachable!("walls are polylines")
                };
                commands.push(Command::ModifyObject {
                    id: *id,
                    shape: Some(Shape::Polyline {
                        points: path.clone(),
                        closed,
                    }),
                    layer: None,
                    class: None,
                    attrs: None,
                });
            }
            commands.push(Command::SetData {
                id: *id,
                set: BTreeMap::from([(
                    wall::KEY.to_string(),
                    serde_json::to_value(&data).expect("JSON"),
                )]),
                remove: vec![],
            });
        }
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        Ok(json!({ "ids": ids }))
    }

    fn wall_report(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let ids: Vec<ObjectId> = match m.remove("ids") {
            Some(v) => {
                serde_json::from_value(v).map_err(|_| bad("'ids' must be a list of object IDs"))?
            }
            None => self
                .editor
                .doc()
                .objects
                .values()
                .filter(|o| wall::data(o).is_some())
                .map(|o| o.id)
                .collect(),
        };
        let mut rows = Vec::new();
        for id in ids {
            self.object(id).and_then(|o| {
                wall::data(o)
                    .ok_or_else(|| RpcError::validation(format!("object {id} is not a wall")))
            })?;
            rows.extend(self.wall_row(id));
        }
        let sum = |k: &str| r(rows.iter().filter_map(|x| x[k].as_f64()).sum());
        let totals = json!({ "walls": rows.len(), "length_m": sum("length_m"), "face_area_m2": sum("face_area_m2"), "both_faces_m2": sum("both_faces_m2"), "volume_m3": sum("volume_m3"), "coping_volume_m3": sum("coping_volume_m3") });
        Ok(json!({ "rows": rows, "totals": totals }))
    }

    // ---- grade objects ----

    /// Solves a grade before anything is changed, so a bad grade changes nothing.
    fn solve_grade_path(
        &self,
        layer: LayerId,
        path: &[DVec2],
        d: &grade::GradeData,
    ) -> Result<grade::Grade, RpcError> {
        let doc = self.editor.doc();
        let site = d.snap.and_then(|_| {
            grading::host(doc, &self.site_cache, self.editor.revision(), layer, d.site)
        });
        let ground = site.as_ref().map(|s| grading::SiteGround::new(s));
        grade::solve_path(path, d, ground.as_ref().map(|g| g as grading::Ground<'_>)).map_err(app)
    }

    fn grade_value(id: ObjectId, g: &grade::Grade) -> Value {
        json!({
            "id": id,
            "length_mm": g.length,
            "elevation_a": g.elevation_a,
            "elevation_b": g.elevation_b,
            "rise_mm": g.rise,
            "percent": g.percent,
            "ratio": g.ratio,
            "angle_deg": g.angle_deg,
            "falls": g.falls(),
            "label": g.label(),
        })
    }

    fn grade_json(&self, id: ObjectId) -> Result<Value, RpcError> {
        let obj = self.object(id)?;
        let d = grade::data(obj)
            .ok_or_else(|| RpcError::validation(format!("object {id} is not a grade object")))?;
        let path = grade::path_of(obj, 2.0)
            .ok_or_else(|| RpcError::validation(format!("object {id} is not a grade object")))?;
        Ok(Self::grade_value(
            id,
            &self.solve_grade_path(obj.layer, &path, &d)?,
        ))
    }

    fn grade_add(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let (layer, class) = take_where(&mut m)?;
        // A curve: `path` (vertices, arcs allowed) instead of `a` and `b`.
        let curve = take_path(&mut m)?;
        let shape = match &curve {
            Some(points) => Shape::Polyline {
                points: points.clone(),
                closed: false,
            },
            None => Shape::Line {
                a: need_point(&mut m, "a")?,
                b: need_point(&mut m, "b")?,
            },
        };
        let data: grade::GradeData = patched(&grade::GradeData::default(), &m, "grade")?;
        data.validate().map_err(bad)?;
        // Solve first: a grade that cannot be solved (both ends in one place, or a snap off the model) is refused.
        let on = layer.unwrap_or(self.editor.doc().active_layer);
        let path = match &shape {
            Shape::Line { a, b } => vec![*a, *b],
            Shape::Polyline { points, .. } => cadektor_core::curve::flatten(points, false, 2.0),
            _ => unreachable!(),
        };
        let solved = self.solve_grade_path(on, &path, &data)?;
        let id = self.add_object_with(
            shape,
            layer,
            class,
            grade::KEY,
            serde_json::to_value(&data).expect("JSON"),
        )?;
        Ok(json!({ "id": id, "grade": Self::grade_value(id, &solved) }))
    }

    fn grade_set(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_id(&mut m, "id")?;
        let (a, b) = (take_point(&mut m, "a")?, take_point(&mut m, "b")?);
        let curve = take_path(&mut m)?;
        let obj = self.object(id)?;
        let current = grade::data(obj)
            .ok_or_else(|| RpcError::validation(format!("object {id} is not a grade object")))?;
        let data: grade::GradeData = patched(&current, &m, "grade")?;
        data.validate().map_err(bad)?;
        let (shape, path) = match (&curve, &obj.shape) {
            (Some(points), _) => {
                if a.is_some() || b.is_some() {
                    return Err(bad("give a path, or a and b, not both"));
                }
                (
                    Some(Shape::Polyline {
                        points: points.clone(),
                        closed: false,
                    }),
                    cadektor_core::curve::flatten(points, false, 2.0),
                )
            }
            (None, Shape::Line { a: old_a, b: old_b }) => {
                let (na, nb) = (a.unwrap_or(*old_a), b.unwrap_or(*old_b));
                (
                    (a.is_some() || b.is_some()).then_some(Shape::Line { a: na, b: nb }),
                    vec![na, nb],
                )
            }
            (None, _) => {
                if a.is_some() || b.is_some() {
                    return Err(bad(
                        "this grade is a curve: change its 'path', or give a path to replace it",
                    ));
                }
                (
                    None,
                    grade::path_of(obj, 2.0).ok_or_else(|| {
                        RpcError::validation(format!("object {id} is not a grade object"))
                    })?,
                )
            }
        };
        let solved = self.solve_grade_path(obj.layer, &path, &data)?;
        self.change_object(
            id,
            shape,
            grade::KEY,
            serde_json::to_value(&data).expect("JSON"),
        )?;
        Ok(json!({ "id": id, "grade": Self::grade_value(id, &solved) }))
    }

    fn grade_get(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_id(&mut m, "id")?;
        self.grade_json(id)
    }

    // ---- analysis ----

    fn site_analysis(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_id(&mut m, "id")?;
        let legend = m.remove("legend");
        let obj = self.object(id)?;
        if !matches!(obj.shape, Shape::SiteModel { .. }) {
            return Err(RpcError::validation(format!(
                "object {id} is not a site model"
            )));
        }
        let current = analysis::spec_of(obj).unwrap_or_default();
        if let Some(c) = m.get_mut("colors").and_then(Value::as_array_mut) {
            for col in c {
                if let Some(a) = col.as_array_mut().filter(|a| a.len() == 3) {
                    a.push(json!(255));
                }
            }
        }
        let spec: analysis::AnalysisSpec = patched(&current, &m, "analysis")?;
        spec.validate().map_err(bad)?;
        let layer = obj.layer;
        if !m.is_empty() {
            self.change_object(
                id,
                None,
                analysis::KEY,
                serde_json::to_value(&spec).expect("JSON"),
            )?;
        }
        let mut legend_id = None;
        if let Some(l) = legend {
            let mut l = map_of(l)?;
            let rect: [DVec2; 2] = serde_json::from_value(
                l.remove("rect")
                    .ok_or_else(|| bad("legend needs rect: [[x0, y0], [x1, y1]]"))?,
            )
            .map_err(|_| bad("legend rect must be [[x0, y0], [x1, y1]]"))?;
            if rect.iter().any(|p| !p.is_finite()) || rect[0] == rect[1] {
                return Err(bad("legend rect needs two different points"));
            }
            let data: analysis::LegendData = patched(
                &analysis::LegendData {
                    site: Some(id),
                    ..Default::default()
                },
                &l,
                "legend",
            )?;
            legend_id = Some(self.add_object_with(
                Shape::Rect {
                    min: rect[0].min(rect[1]),
                    max: rect[0].max(rect[1]),
                },
                Some(layer),
                None,
                analysis::LEGEND_KEY,
                serde_json::to_value(&data).expect("JSON"),
            )?);
        }
        let doc = self.editor.doc();
        let obj = doc.object(id).expect("checked");
        let mut out = json!({ "id": id, "mode": spec.mode, "surface": spec.surface, "breaks": spec.breaks, "opacity": spec.opacity });
        if let Some(l) = legend_id {
            out["legend"] = json!(l);
        }
        if spec.mode != analysis::Mode::Off {
            let site = grading::host(
                doc,
                &self.site_cache,
                self.editor.revision(),
                obj.layer,
                Some(id),
            )
            .ok_or_else(|| app("the site model has no surface yet"))?;
            if let Some(a) = analysis::for_site(&self.site_cache, &site, &spec) {
                let total: f64 = a.bands.iter().map(|b| b.area_xy).sum();
                out["breaks"] = json!(a.breaks);
                out["bands"] = json!(a
                    .bands
                    .iter()
                    .map(|b| json!({
                        "index": b.index, "from": b.from, "to": b.to, "label": b.label, "color": b.color, "triangles": b.triangles,
                        "area_m2": r(b.area_xy / 1e6), "surface_area_m2": r(b.area_surface / 1e6),
                        "percent": if total > 0.0 { r(b.area_xy / total * 100.0) } else { 0.0 },
                    }))
                    .collect::<Vec<_>>());
            }
        }
        Ok(out)
    }

    // ---- spot elevations ----

    fn reading_json(&self, obj_layer_of: &Object, d: &spot::SpotData, at: DVec2) -> Value {
        let doc = self.editor.doc();
        let georef = doc.georef.as_ref();
        let (n, e, geo) = spot::coordinates(at, georef);
        let Some(site) = self.site_for(obj_layer_of, d.site) else {
            return json!({ "existing": null, "proposed": null, "modified": false, "label": "no site model", "northing_m": n, "easting_m": e, "georeferenced": geo });
        };
        let rd = spot::read_at(&site, at);
        json!({ "existing": rd.existing, "proposed": rd.proposed, "modified": rd.modified, "label": spot::label(d, &rd, at, georef), "northing_m": n, "easting_m": e, "georeferenced": geo })
    }

    fn spot_add(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let (layer, class) = take_where(&mut m)?;
        let at = need_point(&mut m, "at")?;
        let d: spot::SpotData = patched(&spot::SpotData::default(), &m, "spot")?;
        d.validate().map_err(bad)?;
        // The marker is 1 mm on paper.
        let doc = self.editor.doc();
        let scale = layer
            .or(Some(doc.active_layer))
            .and_then(|l| doc.layer(l))
            .map_or(100.0, |l| l.scale);
        let id = self.add_object_with(
            Shape::Circle {
                center: at,
                radius: spot::MARKER_PAPER_RADIUS * scale,
            },
            layer,
            class,
            spot::KEY,
            serde_json::to_value(&d).expect("JSON"),
        )?;
        let obj = self.object(id)?;
        Ok(json!({ "id": id, "reading": self.reading_json(obj, &d, at) }))
    }

    fn spot_set(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_id(&mut m, "id")?;
        let at = take_point(&mut m, "at")?;
        let obj = self.object(id)?;
        let Shape::Circle { center, radius } = obj.shape else {
            return Err(RpcError::validation(format!(
                "object {id} is not a spot elevation"
            )));
        };
        let current = spot::data(obj)
            .ok_or_else(|| RpcError::validation(format!("object {id} is not a spot elevation")))?;
        let d: spot::SpotData = patched(&current, &m, "spot")?;
        d.validate().map_err(bad)?;
        let shape = at.map(|c| Shape::Circle { center: c, radius });
        self.change_object(
            id,
            shape,
            spot::KEY,
            serde_json::to_value(&d).expect("JSON"),
        )?;
        let obj = self.object(id)?;
        Ok(json!({ "id": id, "reading": self.reading_json(obj, &d, at.unwrap_or(center)) }))
    }

    fn spot_get(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        if m.contains_key("id") {
            let id = take_id(&mut m, "id")?;
            let obj = self.object(id)?;
            let Shape::Circle { center, .. } = obj.shape else {
                return Err(RpcError::validation(format!(
                    "object {id} is not a spot elevation"
                )));
            };
            let d = spot::data(obj).ok_or_else(|| {
                RpcError::validation(format!("object {id} is not a spot elevation"))
            })?;
            return Ok(self.reading_json(obj, &d, center));
        }
        let at = need_point(&mut m, "at")?;
        let site = m
            .remove("site")
            .map(serde_json::from_value::<ObjectId>)
            .transpose()
            .map_err(|_| bad("'site' must be an object ID"))?;
        let doc = self.editor.doc();
        let host = site
            .or_else(|| {
                doc.objects
                    .values()
                    .find(|o| matches!(o.shape, Shape::SiteModel { .. }))
                    .map(|o| o.id)
            })
            .ok_or_else(|| app("the document has no site model"))?;
        let host_obj = self.object(host)?;
        self.site_for(host_obj, Some(host)).ok_or_else(|| {
            RpcError::validation(format!("object {host} is not a site model with a surface"))
        })?;
        let d = spot::SpotData {
            site: Some(host),
            ..Default::default()
        };
        Ok(self.reading_json(host_obj, &d, at))
    }

    // ---- sections ----

    fn section_add(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let (layer, class) = take_where(&mut m)?;
        let (a, b) = (need_point(&mut m, "a")?, need_point(&mut m, "b")?);
        if a == b {
            return Err(bad("the section line needs two different points"));
        }
        let d: section::SectionData = patched(&section::SectionData::default(), &m, "section")?;
        d.validate().map_err(bad)?;
        let id = self.add_object_with(
            Shape::Line { a, b },
            layer,
            class,
            section::KEY,
            serde_json::to_value(&d).expect("JSON"),
        )?;
        Ok(json!({ "id": id }))
    }

    fn section_set(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_id(&mut m, "id")?;
        let (a, b) = (take_point(&mut m, "a")?, take_point(&mut m, "b")?);
        let obj = self.object(id)?;
        let Shape::Line { a: old_a, b: old_b } = obj.shape else {
            return Err(RpcError::validation(format!(
                "object {id} is not a section line"
            )));
        };
        let current = section::data(obj)
            .ok_or_else(|| RpcError::validation(format!("object {id} is not a section line")))?;
        let d: section::SectionData = patched(&current, &m, "section")?;
        d.validate().map_err(bad)?;
        let (na, nb) = (a.unwrap_or(old_a), b.unwrap_or(old_b));
        if na == nb {
            return Err(bad("the section line needs two different points"));
        }
        let shape = (a.is_some() || b.is_some()).then_some(Shape::Line { a: na, b: nb });
        self.change_object(
            id,
            shape,
            section::KEY,
            serde_json::to_value(&d).expect("JSON"),
        )?;
        Ok(json!({ "id": id }))
    }

    fn section_profile_of(
        &self,
        id: ObjectId,
    ) -> Result<(section::SectionData, section::Profile), RpcError> {
        let obj = self.object(id)?;
        let d = section::data(obj)
            .ok_or_else(|| RpcError::validation(format!("object {id} is not a section line")))?;
        let site = self
            .site_for(obj, d.site)
            .ok_or_else(|| app("there is no site model for the section on its layer"))?;
        let p = section::profile(self.editor.doc(), obj, &site)
            .ok_or_else(|| app("the section line has no length"))?;
        Ok((d, p))
    }

    fn section_profile(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_id(&mut m, "id")?;
        let (_, p) = self.section_profile_of(id)?;
        let runs = |runs: &[Vec<(f64, f64)>]| {
            runs.iter()
                .map(|r| r.iter().map(|(s, z)| [*s, *z]).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        };
        Ok(json!({
            "id": id,
            "length_mm": p.length,
            "modified": p.modified,
            "existing": runs(&p.existing),
            "proposed": runs(&p.proposed),
            "off_model": p.off_model.iter().map(|(a, b)| [*a, *b]).collect::<Vec<_>>(),
            "walls": p.walls.iter().map(|w| json!({"id": w.id, "s": w.s, "width": w.width, "top": w.top, "bottom": w.bottom})).collect::<Vec<_>>(),
            "hardscapes": p.slabs.iter().map(|s| json!({"id": s.id, "s0": s.s0, "s1": s.s1, "top0": s.top0, "top1": s.top1, "thickness": s.thickness})).collect::<Vec<_>>(),
            "plants": p.plants.iter().map(|x| json!({"id": x.id, "s": x.s, "offset": x.offset, "spread": x.spread, "height": x.height, "base": x.base})).collect::<Vec<_>>(),
            "z_min": p.z_min,
            "z_max": p.z_max,
        }))
    }

    fn section_place(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_id(&mut m, "id")?;
        let at = need_point(&mut m, "at")?;
        let layer_param: Option<LayerId> = m
            .remove("layer")
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| bad("'layer' must be a layer ID"))?;
        let scale_param = m
            .remove("scale")
            .map(|v| {
                v.as_f64()
                    .filter(|s| s.is_finite() && *s > 0.0)
                    .ok_or_else(|| bad("'scale' must be a positive number"))
            })
            .transpose()?;
        let (d, p) = self.section_profile_of(id)?;
        let doc = self.editor.doc();
        let section_layer = self.object(id)?.layer;
        let target = layer_param.unwrap_or(section_layer);
        let layer = doc
            .layer(target)
            .ok_or_else(|| RpcError::not_found(format!("layer {target} does not exist")))?;
        let section_scale = doc.layer(section_layer).map_or(100.0, |l| l.scale);
        let sheet = layer.kind == LayerKind::Sheet;
        let scale = scale_param.unwrap_or(if sheet { section_scale } else { 1.0 });
        // Text is 2.5 mm on paper. A sheet layer counts in paper mm. A design layer counts in model mm.
        let text = 2.5 * if sheet { 1.0 } else { layer.scale } * scale;
        let items: Vec<section::Item> = section::drawing(&p, &d, at * scale, text)
            .into_iter()
            .map(|i| i.scaled(1.0 / scale))
            .collect();
        let stale: Vec<ObjectId> = doc
            .objects
            .values()
            .filter(|o| {
                o.data
                    .get(section::PROFILE_KEY)
                    .and_then(|v| v.get("section"))
                    .and_then(AsUid::as_uid)
                    == Some(id.0)
            })
            .map(|o| o.id)
            .collect();
        let mut commands = Vec::new();
        if !stale.is_empty() {
            commands.push(Command::DeleteObjects { ids: stale });
        }
        let planned = doc.next_ids(items.len());
        let mut ids = Vec::new();
        let mut bounds = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for (k, item) in items.iter().enumerate() {
            let attrs = Attributes {
                pen: Some(Color(item.pen)),
                fill: item.fill.map(Color),
                line_weight: Some(item.weight),
                ..Default::default()
            };
            let (lo, hi) = item.shape.bounds();
            bounds = (bounds.0.min(lo), bounds.1.max(hi));
            commands.push(Command::AddObject {
                shape: item.shape.clone(),
                layer: Some(target),
                class: None,
                attrs,
            });
            ids.push(ObjectId(planned[k]));
        }
        for (obj_id, item) in ids.iter().zip(&items) {
            let tag = json!({ "section": id, "role": item.role });
            commands.push(Command::SetData {
                id: *obj_id,
                set: BTreeMap::from([(section::PROFILE_KEY.to_string(), tag)]),
                remove: vec![],
            });
        }
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        self.prune_selection();
        Ok(json!({ "ids": ids, "bounds": [[bounds.0.x, bounds.0.y], [bounds.1.x, bounds.1.y]] }))
    }
}

fn ids_of(m: &mut Map<String, Value>) -> Result<Vec<ObjectId>, RpcError> {
    let ids = match (m.remove("id"), m.remove("ids")) {
        (Some(v), None) => {
            vec![serde_json::from_value(v).map_err(|_| bad("'id' must be an object ID"))?]
        }
        (None, Some(v)) => {
            serde_json::from_value(v).map_err(|_| bad("'ids' must be a list of object IDs"))?
        }
        _ => return Err(bad("give 'id' or 'ids'")),
    };
    if ids.is_empty() {
        return Err(bad("'ids' is empty"));
    }
    Ok(ids)
}
