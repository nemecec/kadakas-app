//! API methods of site models: `site.create`, `site.update`, `site.elevation`, `site.cut_fill`
//! and `site.contours`. The terrain calculations are in `kadakas-site`.

use std::sync::Arc;

use glam::DVec2;
use serde::Deserialize;
use serde_json::{Value, json};

use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, Document, LayerId, ObjectId, Shape, SiteLine, SiteSettings};
use kadakas_core::provenance::{self, SiteSources};
use kadakas_site::{SiteResult, Surface};

use crate::{RpcError, Session, params};

#[derive(Deserialize)]
struct Create {
    source_ids: Vec<ObjectId>,
    #[serde(default)]
    settings: Value,
    /// The design layer of the new site model. Default: the layer of the first source.
    #[serde(default)]
    layer: Option<LayerId>,
}

#[derive(Deserialize)]
struct Update {
    id: ObjectId,
    #[serde(default)]
    settings: Value,
    #[serde(default)]
    add_source_ids: Vec<ObjectId>,
    #[serde(default)]
    remove_points: Vec<usize>,
    #[serde(default)]
    remove_lines: Vec<usize>,
    /// Sources that other objects replace: the new object takes the place (the index) of the old
    /// source, with its own tag.
    #[serde(default)]
    replace: Vec<Replace>,
}

/// One source of a site model and the object that replaces it. Give `point` or `line`.
#[derive(Deserialize)]
struct Replace {
    #[serde(default)]
    point: Option<usize>,
    #[serde(default)]
    line: Option<usize>,
    with: ObjectId,
}

#[derive(Deserialize)]
struct Query {
    id: ObjectId,
    #[serde(default)]
    surface: Option<String>,
}

fn surface_of(name: Option<&str>) -> Result<Surface, RpcError> {
    match name {
        None | Some("proposed") => Ok(Surface::Proposed),
        Some("existing") => Ok(Surface::Existing),
        Some(n) => Err(RpcError::invalid_params(format!(
            "unknown surface '{n}'. Use existing or proposed"
        ))),
    }
}

/// Takes 3D loci and 3D polylines out of objects, with the import tags that they carry. Fails for
/// any other kind of object.
fn take_sources(
    doc: &Document,
    ids: &[ObjectId],
    points: &mut Vec<glam::DVec3>,
    lines: &mut Vec<SiteLine>,
    sources: &mut SiteSources,
) -> Result<(), RpcError> {
    for id in ids {
        let o = doc
            .object(*id)
            .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
        match &o.shape {
            Shape::Locus3d { p } => {
                points.push(*p);
                sources
                    .points
                    .push(provenance::source_ref(doc, o, provenance::point_state(*p)));
            }
            Shape::Polyline3d {
                points: pts,
                closed,
            } => {
                let line = SiteLine {
                    points: pts.clone(),
                    closed: *closed,
                };
                sources.lines.push(provenance::source_ref(
                    doc,
                    o,
                    provenance::line_state(&line),
                ));
                lines.push(line);
            }
            other => {
                return Err(RpcError::invalid_params(format!(
                    "object {id} is a {}: only locus3d and polyline3d can be site model sources",
                    other.kind()
                )));
            }
        }
    }
    Ok(())
}

/// The command that stores the tags of the sources in the data of the site model, or clears
/// them when no source has one.
fn sources_command(id: ObjectId, sources: &SiteSources) -> Command {
    if sources.is_empty() {
        Command::SetData {
            id,
            set: Default::default(),
            remove: vec![provenance::SITE_SOURCES_KEY.to_string()],
        }
    } else {
        Command::SetData {
            id,
            set: [(provenance::SITE_SOURCES_KEY.to_string(), sources.to_value())].into(),
            remove: Vec::new(),
        }
    }
}

/// Merges the fields of `patch` into the settings. A field set to null goes back to its default.
fn merge_settings(current: &SiteSettings, patch: &Value) -> Result<SiteSettings, RpcError> {
    if patch.is_null() {
        return Ok(current.clone());
    }
    let Some(patch) = patch.as_object() else {
        return Err(RpcError::invalid_params("settings must be an object"));
    };
    let mut v = serde_json::to_value(current).expect("settings serialize");
    let obj = v.as_object_mut().expect("settings are an object");
    for (k, x) in patch {
        obj.insert(k.clone(), x.clone());
    }
    serde_json::from_value(v).map_err(RpcError::invalid_params)
}

impl Session {
    pub(crate) fn handle_site(&mut self, method: &str, p: Value) -> Result<Value, RpcError> {
        match method {
            "site.create" => {
                let Create {
                    source_ids,
                    settings,
                    layer,
                } = params(p)?;
                if source_ids.is_empty() {
                    return Err(RpcError::invalid_params(
                        "give source_ids: at least one locus3d or polyline3d",
                    ));
                }
                let doc = self.editor.doc();
                let (mut points, mut lines) = (Vec::new(), Vec::new());
                let mut sources = SiteSources::default();
                take_sources(doc, &source_ids, &mut points, &mut lines, &mut sources)?;
                let settings = merge_settings(&SiteSettings::default(), &settings)?;
                if let Some(l) = layer
                    && doc.layer(l).is_none()
                {
                    return Err(RpcError::not_found(format!("layer {l} does not exist")));
                }
                let layer = layer.or_else(|| doc.object(source_ids[0]).map(|o| o.layer));
                let create = Command::AddObject {
                    shape: Shape::SiteModel {
                        points,
                        lines,
                        settings,
                    },
                    layer,
                    class: None,
                    attrs: Attributes::default(),
                };
                let mut commands = vec![create];
                if !sources.is_empty() {
                    // The tags travel with the source data, so a later update from a new revision of
                    // the drawing finds the sources inside the model.
                    commands.push(sources_command(ObjectId(doc.next_ids(1)[0]), &sources));
                }
                commands.push(Command::DeleteObjects { ids: source_ids });
                let out = self
                    .editor
                    .execute(&Command::Batch { commands })
                    .map_err(RpcError::from)?;
                self.prune_selection();
                Ok(json!({ "id": out.created[0] }))
            }
            "site.update" => {
                let Update {
                    id,
                    settings,
                    add_source_ids,
                    remove_points,
                    remove_lines,
                    replace,
                } = params(p)?;
                let doc = self.editor.doc();
                let obj = doc
                    .object(id)
                    .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
                let Shape::SiteModel {
                    points,
                    lines,
                    settings: current,
                } = &obj.shape
                else {
                    return Err(RpcError::validation(format!(
                        "object {id} is not a site model"
                    )));
                };
                let (mut points, mut lines) = (points.clone(), lines.clone());
                let mut sources = SiteSources::read(obj)
                    .unwrap_or_else(|| SiteSources::blank(points.len(), lines.len()));
                for (what, list, len) in [
                    ("point", &remove_points, points.len()),
                    ("line", &remove_lines, lines.len()),
                ] {
                    if let Some(i) = list.iter().find(|i| **i >= len) {
                        return Err(RpcError::invalid_params(format!(
                            "{what} index {i} is out of range (0 to {})",
                            len.saturating_sub(1)
                        )));
                    }
                }
                if add_source_ids.contains(&id) || replace.iter().any(|r| r.with == id) {
                    return Err(RpcError::invalid_params(
                        "a site model cannot be its own source",
                    ));
                }
                // A replacement takes the place of an old source: the new coordinates and the tag
                // of the new object, in the same place of the list.
                let mut consumed: Vec<ObjectId> = Vec::new();
                let mut slots: std::collections::HashSet<(&str, usize)> = Default::default();
                for r in &replace {
                    let (what, index, len) = match (r.point, r.line) {
                        (Some(i), None) => ("point", i, points.len()),
                        (None, Some(i)) => ("line", i, lines.len()),
                        _ => {
                            return Err(RpcError::invalid_params(
                                "a replacement names a point or a line, one of them",
                            ));
                        }
                    };
                    if index >= len {
                        return Err(RpcError::invalid_params(format!(
                            "{what} index {index} is out of range (0 to {})",
                            len.saturating_sub(1)
                        )));
                    }
                    if consumed.contains(&r.with) {
                        return Err(RpcError::invalid_params(format!(
                            "object {} replaces more than one source",
                            r.with
                        )));
                    }
                    if !slots.insert((what, index)) {
                        return Err(RpcError::invalid_params(format!(
                            "{what} {index} is replaced more than once"
                        )));
                    }
                    if add_source_ids.contains(&r.with) {
                        return Err(RpcError::invalid_params(format!(
                            "object {} replaces a source and is added as a source in one call",
                            r.with
                        )));
                    }
                    let removed = if what == "point" {
                        &remove_points
                    } else {
                        &remove_lines
                    };
                    if removed.contains(&index) {
                        return Err(RpcError::invalid_params(format!(
                            "{what} {index} is replaced and removed in one call"
                        )));
                    }
                    let (mut np, mut nl) = (Vec::new(), Vec::new());
                    let mut ns = SiteSources::default();
                    take_sources(doc, &[r.with], &mut np, &mut nl, &mut ns)?;
                    match (what, np.pop(), nl.pop()) {
                        ("point", Some(p), None) => {
                            points[index] = p;
                            sources.points[index] = ns.points.pop().flatten();
                        }
                        ("line", None, Some(l)) => {
                            lines[index] = l;
                            sources.lines[index] = ns.lines.pop().flatten();
                        }
                        _ => {
                            return Err(RpcError::invalid_params(format!(
                                "object {} cannot replace a {what}: a point is replaced by a locus3d, a line by a polyline3d",
                                r.with
                            )));
                        }
                    }
                    consumed.push(r.with);
                }
                let mut index = 0usize;
                points.retain(|_| {
                    let keep = !remove_points.contains(&index);
                    index += 1;
                    keep
                });
                let mut index = 0usize;
                sources.points.retain(|_| {
                    let keep = !remove_points.contains(&index);
                    index += 1;
                    keep
                });
                let mut index = 0usize;
                lines.retain(|_| {
                    let keep = !remove_lines.contains(&index);
                    index += 1;
                    keep
                });
                let mut index = 0usize;
                sources.lines.retain(|_| {
                    let keep = !remove_lines.contains(&index);
                    index += 1;
                    keep
                });
                take_sources(doc, &add_source_ids, &mut points, &mut lines, &mut sources)?;
                let settings = merge_settings(current, &settings)?;
                let (np, nl) = (points.len(), lines.len());
                let modify = Command::ModifyObject {
                    id,
                    shape: Some(Shape::SiteModel {
                        points,
                        lines,
                        settings,
                    }),
                    layer: None,
                    class: None,
                    attrs: None,
                };
                let mut commands = vec![modify];
                let tagged = !sources.is_empty();
                if tagged || obj.data.contains_key(provenance::SITE_SOURCES_KEY) {
                    commands.push(sources_command(id, &sources));
                }
                consumed.extend(add_source_ids);
                if !consumed.is_empty() {
                    commands.push(Command::DeleteObjects { ids: consumed });
                }
                self.editor
                    .execute(&Command::Batch { commands })
                    .map_err(RpcError::from)?;
                self.prune_selection();
                Ok(json!({ "id": id, "points": np, "lines": nl }))
            }
            "site.texture" => self.site_texture(p),
            "site.refresh" => {
                // "Update Site Model": rebuild the surfaces now, even when the site model does not update by itself.
                let q: Query = params(p)?;
                let site = self
                    .site_cache
                    .update(self.editor.revision(), self.editor.doc(), q.id)
                    .map_err(RpcError::app)?;
                Ok(
                    json!({ "id": q.id, "out_of_date": false, "modifiers": site.modifiers, "has_proposed": site.proposed.is_some() }),
                )
            }
            "site.drape" => {
                #[derive(Deserialize)]
                struct P {
                    id: ObjectId,
                    points: Vec<[f64; 2]>,
                    #[serde(default)]
                    closed: bool,
                }
                let P { id, points, closed } = params(p)?;
                if points.is_empty()
                    || points
                        .iter()
                        .any(|p| !(p[0].is_finite() && p[1].is_finite()))
                {
                    return Err(RpcError::invalid_params(
                        "points must be a list of finite [x, y] pairs",
                    ));
                }
                let site = self.site(id)?;
                let flat: Vec<DVec2> = points.iter().map(|p| DVec2::new(p[0], p[1])).collect();
                // Draped on the proposed surface, with extra points where the line crosses triangle edges.
                let out = kadakas_site::drape(site.surface(Surface::Proposed), &flat, closed)
                    .map_err(RpcError::app)?;
                Ok(json!({ "points": out.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>() }))
            }
            "site.elevation" => {
                #[derive(Deserialize)]
                struct P {
                    #[serde(flatten)]
                    q: Query,
                    x: f64,
                    y: f64,
                }
                let P { q, x, y } = params(p)?;
                if !(x.is_finite() && y.is_finite()) {
                    return Err(RpcError::invalid_params("x and y must be finite numbers"));
                }
                let which = surface_of(q.surface.as_deref())?;
                let site = self.site(q.id)?;
                let at = DVec2::new(x, y);
                let tin = site.surface(which);
                let sa = tin.slope_at(at);
                Ok(json!({
                    "z": tin.elevation_at(at),
                    "slope": sa.map(|s| s.slope),
                    "aspect": sa.and_then(|s| s.aspect),
                }))
            }
            "site.cut_fill" => {
                let q: Query = params(p)?;
                let site = self.site(q.id)?;
                let c = site.cut_fill();
                const M3: f64 = 1e9;
                Ok(json!({
                    "cut_m3": c.cut / M3,
                    "fill_m3": c.fill / M3,
                    "net_m3": c.net() / M3,
                    "area_m2": c.area / 1e6,
                    "modifiers": site.modifiers,
                    "out_of_date": self.site_cache.is_stale(self.editor.doc(), q.id),
                }))
            }
            "site.contours" => {
                #[derive(Deserialize)]
                struct P {
                    #[serde(flatten)]
                    q: Query,
                    #[serde(default = "yes")]
                    points: bool,
                }
                fn yes() -> bool {
                    true
                }
                let P { q, points } = params(p)?;
                let which = surface_of(q.surface.as_deref())?;
                let site = self.site(q.id)?;
                let list = site.contours(which).map_err(RpcError::app)?;
                let mut levels: Vec<f64> = list.iter().map(|c| c.level).collect();
                levels.dedup();
                let contours: Vec<Value> = list
                    .iter()
                    .map(|c| {
                        let mut o = json!({"level": c.level, "major": c.major, "closed": c.closed});
                        if points {
                            o["points"] = json!(c.points);
                        }
                        o
                    })
                    .collect();
                Ok(
                    json!({ "count": list.len(), "major_count": list.iter().filter(|c| c.major).count(), "levels": levels, "contours": contours }),
                )
            }
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    /// The derived surfaces of a site model, from the cache.
    fn site(&self, id: ObjectId) -> Result<Arc<SiteResult>, RpcError> {
        self.site_cache
            .get(self.editor.revision(), self.editor.doc(), id)
            .map_err(RpcError::app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::uid::AsUid;

    fn call(s: &mut Session, method: &str, p: Value) -> Result<Value, RpcError> {
        s.handle(method, p)
    }

    fn exec(s: &mut Session, command: Value) -> kadakas_core::uid::Uid {
        call(s, "execute", json!({"command": command})).unwrap()["created"][0]
            .as_uid()
            .unwrap()
    }

    /// A 9 by 9 grid of loci, 1 m apart, on a plane that rises 10 percent toward +X.
    fn grid(s: &mut Session) -> Vec<kadakas_core::uid::Uid> {
        let mut ids = Vec::new();
        for i in 0..9 {
            for j in 0..9 {
                ids.push(exec(s, json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}})));
            }
        }
        ids
    }

    #[test]
    fn create_moves_the_sources_into_the_site_model_and_undo_restores_them() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        let r = call(&mut s, "site.create", json!({"source_ids": ids, "settings": {"minor_interval": 200, "major_interval": 400, "display_2d": "both"}})).unwrap();
        let id = r["id"].as_uid().unwrap();
        assert_eq!(s.editor.doc().objects.len(), 1);
        let o = call(&mut s, "objects.get", json!({"id": id})).unwrap();
        assert_eq!(o["shape"]["kind"], "site_model");
        assert_eq!(o["shape"]["points"].as_array().unwrap().len(), 81);
        assert_eq!(o["shape"]["settings"]["minor_interval"], 200.0);
        assert_eq!(o["shape"]["settings"]["display_2d"], "both");
        assert!(s.editor.undo());
        assert_eq!(s.editor.doc().objects.len(), 81);
    }

    #[test]
    fn create_puts_the_site_model_on_the_layer_that_is_asked_for() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        let site_layer = exec(
            &mut s,
            json!({"op": "add_layer", "name": "Site", "elevation": 0}),
        );
        let r = call(
            &mut s,
            "site.create",
            json!({"source_ids": ids, "layer": site_layer}),
        )
        .unwrap();
        let o = call(&mut s, "objects.get", json!({"id": r["id"]})).unwrap();
        assert_eq!(o["layer"], json!(site_layer));
        // Without `layer` the layer of the first source counts.
        let ids = grid(&mut s);
        let r = call(&mut s, "site.create", json!({"source_ids": ids})).unwrap();
        let o = call(&mut s, "objects.get", json!({"id": r["id"]})).unwrap();
        assert_ne!(o["layer"], json!(site_layer));
        let ids = grid(&mut s);
        let e = call(
            &mut s,
            "site.create",
            json!({"source_ids": ids, "layer": "00000000-0000-0000-0000-0000000003e7"}),
        )
        .unwrap_err();
        assert!(e.message.contains("does not exist"), "{}", e.message);
    }

    #[test]
    fn create_checks_its_input() {
        let mut s = Session::with_sequential_ids();
        let circle = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}),
        );
        let e = call(&mut s, "site.create", json!({"source_ids": []})).unwrap_err();
        assert_eq!(e.code, -32602);
        let e = call(&mut s, "site.create", json!({"source_ids": [circle]})).unwrap_err();
        assert!(e.message.contains("only locus3d"), "{}", e.message);
        let e = call(
            &mut s,
            "site.create",
            json!({"source_ids": ["00000000-0000-0000-0000-0000000003e7"]}),
        )
        .unwrap_err();
        assert!(e.message.contains("does not exist"));
        let locus = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [0, 0, 0]}}),
        );
        let e = call(
            &mut s,
            "site.create",
            json!({"source_ids": [locus], "settings": {"minor_interval": -1}}),
        )
        .unwrap_err();
        assert!(e.message.contains("positive"), "{}", e.message);
        let e = call(
            &mut s,
            "site.create",
            json!({"source_ids": [locus], "settings": 5}),
        )
        .unwrap_err();
        assert!(e.message.contains("object"));
        let e = call(
            &mut s,
            "site.create",
            json!({"source_ids": [locus], "settings": {"display_2d": "sideways"}}),
        )
        .unwrap_err();
        assert_eq!(e.code, -32602);
        // A failed create leaves the document alone.
        assert!(s.editor.doc().object(ObjectId(locus)).is_some());
    }

    fn tag(handle: &str) -> Value {
        json!({"drawing": "d", "revision": "r1", "handle": handle, "kind": "POINT", "hash": "0123456789abcdef", "obj": "0000000000000000"})
    }

    fn tagged_locus(s: &mut Session, x: i64, handle: &str) -> kadakas_core::uid::Uid {
        let id = exec(
            s,
            json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [x, 0, x]}}),
        );
        exec_ok(
            s,
            json!({"op": "set_data", "id": id, "set": {"import": tag(handle)}}),
        );
        id
    }

    fn exec_ok(s: &mut Session, command: Value) {
        call(s, "execute", json!({"command": command})).unwrap();
    }

    #[test]
    fn objects_get_shows_how_many_sources_are_tagged_and_the_tags_on_request() {
        let mut s = Session::with_sequential_ids();
        let a = tagged_locus(&mut s, 0, "A");
        let b = tagged_locus(&mut s, 1000, "B");
        let id = call(&mut s, "site.create", json!({"source_ids": [a, b]})).unwrap()["id"].clone();
        let plain = call(&mut s, "objects.get", json!({"id": id})).unwrap();
        let hidden = &plain["data"]["import_sources"];
        assert_eq!(hidden["tagged_points"], 2, "{plain}");
        assert!(hidden.get("points").is_none());
        let full = call(&mut s, "objects.get", json!({"id": id, "sources": true})).unwrap();
        assert_eq!(
            full["data"]["import_sources"]["points"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        // The list shows the count as well.
        let list = call(
            &mut s,
            "objects.list",
            json!({"kind": "site_model", "include_data": true}),
        );
        if let Ok(l) = list {
            assert!(!l.to_string().contains("\"handle\""), "{l}");
        }
    }

    #[test]
    fn the_sources_keep_their_tags_in_the_model_and_site_update_keeps_them_in_step() {
        let mut s = Session::with_sequential_ids();
        let a = tagged_locus(&mut s, 0, "A");
        let b = tagged_locus(&mut s, 1000, "B");
        let c = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [2000, 0, 5]}}),
        );
        let d = tagged_locus(&mut s, 3000, "D");
        let id =
            call(&mut s, "site.create", json!({"source_ids": [a, b, c, d]})).unwrap()["id"].clone();
        let data = |s: &mut Session| {
            call(s, "objects.get", json!({"id": id, "sources": true})).unwrap()["data"]["import_sources"].clone()
        };
        let handles = |v: &Value| -> Vec<Value> {
            v["points"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["tag"]["handle"].clone())
                .collect()
        };
        let v = data(&mut s);
        assert_eq!(
            v["points"][2],
            Value::Null,
            "a source without a tag has none"
        );
        assert_eq!(
            handles(&v),
            vec![json!("A"), json!("B"), Value::Null, json!("D")]
        );
        // These objects do not match the state in their tag (they were edited since the import),
        // and a source that was edited stays edited when it goes into the model.
        assert_eq!(
            v["points"][0]["tag"]["obj"],
            kadakas_core::provenance::EDITED_OBJ
        );
        // Removing a point removes its tag; a replacement takes the tag of the new object.
        call(
            &mut s,
            "site.update",
            json!({"id": id, "remove_points": [0]}),
        )
        .unwrap();
        assert_eq!(
            handles(&data(&mut s)),
            vec![json!("B"), Value::Null, json!("D")]
        );
        let n = tagged_locus(&mut s, 9000, "N");
        call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"point": 1, "with": n}]}),
        )
        .unwrap();
        assert_eq!(
            handles(&data(&mut s)),
            vec![json!("B"), json!("N"), json!("D")]
        );
        let o = call(&mut s, "objects.get", json!({"id": id})).unwrap();
        assert_eq!(o["shape"]["points"][1], json!([9000.0, 0.0, 9000.0]));
        assert!(
            s.editor.doc().object(ObjectId(n)).is_none(),
            "the replacing object is consumed"
        );
        // An added source comes with its tag, and the replace checks its input.
        let m = tagged_locus(&mut s, 11_000, "M");
        call(
            &mut s,
            "site.update",
            json!({"id": id, "add_source_ids": [m]}),
        )
        .unwrap();
        assert_eq!(handles(&data(&mut s)).last(), Some(&json!("M")));
        let m = tagged_locus(&mut s, 12_000, "M2");
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"point": 0, "line": 0, "with": m}]}),
        )
        .unwrap_err();
        assert!(e.message.contains("one of them"), "{}", e.message);
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "remove_points": [0], "replace": [{"point": 0, "with": m}]}),
        )
        .unwrap_err();
        assert!(e.message.contains("replaced and removed"), "{}", e.message);
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"line": 0, "with": m}]}),
        )
        .unwrap_err();
        assert!(e.message.contains("out of range"), "{}", e.message);
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"point": 0, "with": id}]}),
        )
        .unwrap_err();
        assert!(e.message.contains("own source"), "{}", e.message);
        // One source is replaced once, and one object replaces one source.
        let m2 = tagged_locus(&mut s, 13_000, "M3");
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"point": 0, "with": m}, {"point": 0, "with": m2}]}),
        )
        .unwrap_err();
        assert!(e.message.contains("more than once"), "{}", e.message);
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"point": 0, "with": m}, {"point": 1, "with": m}]}),
        )
        .unwrap_err();
        assert!(e.message.contains("more than one source"), "{}", e.message);
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"point": 0, "with": m}], "add_source_ids": [m]}),
        )
        .unwrap_err();
        assert!(e.message.contains("added as a source"), "{}", e.message);
        // A polyline cannot replace a point, and a replaced line cannot also be removed.
        let l = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "polyline3d", "closed": false, "points": [[0, 0, 0], [1000, 0, 100]]}}),
        );
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "replace": [{"point": 0, "with": l}]}),
        )
        .unwrap_err();
        assert!(
            e.message.contains("cannot replace a point"),
            "{}",
            e.message
        );
        let e = call(
            &mut s,
            "site.update",
            json!({"id": id, "remove_lines": [0], "replace": [{"line": 0, "with": l}]}),
        )
        .unwrap_err();
        assert!(
            e.message.contains("out of range") || e.message.contains("replaced and removed"),
            "{}",
            e.message
        );
        // Removing every tagged source clears the key.
        call(
            &mut s,
            "site.update",
            json!({"id": id, "remove_points": [0, 1, 2, 3]}),
        )
        .unwrap();
        let o = call(&mut s, "objects.get", json!({"id": id})).unwrap();
        assert!(
            o.get("data")
                .is_none_or(|d| d.get("import_sources").is_none())
        );
    }

    #[test]
    fn drape_refresh_and_tiny_intervals() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        let id = call(
            &mut s,
            "site.create",
            json!({"source_ids": ids, "settings": {"auto_update": false}}),
        )
        .unwrap()["id"]
            .as_uid()
            .unwrap();
        let r = call(
            &mut s,
            "site.drape",
            json!({"id": id, "points": [[500, 500], [3500, 500]]}),
        )
        .unwrap();
        let pts = r["points"].as_array().unwrap();
        assert!(pts.len() >= 2);
        for p in pts {
            assert!((p[2].as_f64().unwrap() - p[0].as_f64().unwrap() / 10.0).abs() < 1e-9);
        }
        assert!(
            call(
                &mut s,
                "site.drape",
                json!({"id": id, "points": [[99999, 0]]})
            )
            .is_err()
        );
        assert_eq!(
            call(&mut s, "site.drape", json!({"id": id, "points": []}))
                .unwrap_err()
                .code,
            -32602
        );
        // A pad is added: with auto_update off the model is out of date until site.refresh.
        let cf = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert_eq!(cf["out_of_date"], false);
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::SiteModifier {
                    modifier: kadakas_core::model::ModifierKind::Pad {
                        elevation: 5000.0,
                        slope: None,
                    },
                    boundary: kadakas_core::curve::corners(&[
                        DVec2::new(2000.0, 2000.0),
                        DVec2::new(4000.0, 2000.0),
                        DVec2::new(4000.0, 4000.0),
                        DVec2::new(2000.0, 4000.0),
                    ]),
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let cf = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert_eq!(cf["out_of_date"], true);
        assert_eq!(cf["fill_m3"], 0.0);
        let r = call(&mut s, "site.refresh", json!({"id": id})).unwrap();
        assert_eq!(r["has_proposed"], true);
        let cf = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert_eq!(cf["out_of_date"], false);
        assert!(cf["fill_m3"].as_f64().unwrap() > 0.0);
        // A contour interval that would overflow is refused.
        let locus = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [0, 0, 0]}}),
        );
        let e = call(
            &mut s,
            "site.create",
            json!({"source_ids": [locus], "settings": {"minor_interval": 1e-300}}),
        )
        .unwrap_err();
        assert!(e.message.contains("contour"), "{}", e.message);
    }

    #[test]
    fn elevation_slope_and_surfaces() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        let id = call(&mut s, "site.create", json!({"source_ids": ids})).unwrap()["id"]
            .as_uid()
            .unwrap();
        let r = call(
            &mut s,
            "site.elevation",
            json!({"id": id, "x": 2500, "y": 3100}),
        )
        .unwrap();
        assert!((r["z"].as_f64().unwrap() - 250.0).abs() < 1e-9);
        assert!((r["slope"].as_f64().unwrap() - 0.1).abs() < 1e-9);
        // The ground falls toward -X, so it faces west: 270 degrees clockwise from north.
        assert!((r["aspect"].as_f64().unwrap() - 1.5 * std::f64::consts::PI).abs() < 1e-9);
        let r = call(
            &mut s,
            "site.elevation",
            json!({"id": id, "x": -10, "y": 0, "surface": "existing"}),
        )
        .unwrap();
        assert!(r["z"].is_null() && r["slope"].is_null());
        assert_eq!(
            call(
                &mut s,
                "site.elevation",
                json!({"id": id, "x": 0, "y": 0, "surface": "x"})
            )
            .unwrap_err()
            .code,
            -32602
        );
        assert_eq!(
            call(
                &mut s,
                "site.elevation",
                json!({"id": id, "x": "a", "y": 0})
            )
            .unwrap_err()
            .code,
            -32602
        );
        let e = call(
            &mut s,
            "site.elevation",
            json!({"id": "00000000-0000-0000-0000-00000001e240", "x": 0, "y": 0}),
        )
        .unwrap_err();
        assert!(e.message.contains("does not exist"));
    }

    #[test]
    fn non_finite_coordinates_are_refused() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        let id = call(&mut s, "site.create", json!({"source_ids": ids})).unwrap()["id"]
            .as_uid()
            .unwrap();
        // JSON cannot hold NaN, but a very large number is still finite and simply outside the model.
        let r = call(
            &mut s,
            "site.elevation",
            json!({"id": id, "x": 1e300, "y": 0}),
        )
        .unwrap();
        assert!(r["z"].is_null());
        let p = serde_json::from_str::<Value>(
            r#"{"id": "00000000-0000-0000-0000-000000000001", "x": 1e999, "y": 0}"#,
        );
        assert!(p.is_err() || call(&mut s, "site.elevation", p.unwrap()).is_err());
    }

    #[test]
    fn pad_gives_cut_and_fill_and_contours() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        let id = call(
            &mut s,
            "site.create",
            json!({"source_ids": ids, "settings": {"minor_interval": 100, "major_interval": 400}}),
        )
        .unwrap()["id"]
            .as_uid()
            .unwrap();
        let c = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert_eq!(
            (
                c["cut_m3"].as_f64().unwrap(),
                c["fill_m3"].as_f64().unwrap()
            ),
            (0.0, 0.0)
        );
        assert!((c["area_m2"].as_f64().unwrap() - 64.0).abs() < 1e-9);
        let cs = call(&mut s, "site.contours", json!({"id": id, "points": false})).unwrap();
        // Levels 100..=800 mm: one line per level (level 0 is not above any vertex).
        assert_eq!(cs["count"], 8);
        assert_eq!(cs["major_count"], 2);
        assert!(cs["contours"][0].get("points").is_none());
        // A flat pad at 400 mm over x = 2..6 m, y = 2..6 m: the ground there is 200..600, so it is part cut, part fill.
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "pad", "elevation": 400}, "boundary": [[2000,2000],[6000,2000],[6000,6000],[2000,6000]]}}),
        );
        let c = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert!(
            c["cut_m3"].as_f64().unwrap() > 0.5 && c["fill_m3"].as_f64().unwrap() > 0.5,
            "{c}"
        );
        let z = |s: &mut Session, x: f64, surface: &str| {
            call(
                s,
                "site.elevation",
                json!({"id": id, "x": x, "y": 4000, "surface": surface}),
            )
            .unwrap()["z"]
                .as_f64()
                .unwrap()
        };
        assert!((z(&mut s, 3000.0, "proposed") - 400.0).abs() < 1e-9);
        assert!((z(&mut s, 3000.0, "existing") - 300.0).abs() < 1e-9);
        let p = call(
            &mut s,
            "site.contours",
            json!({"id": id, "surface": "proposed"}),
        )
        .unwrap();
        let e = call(
            &mut s,
            "site.contours",
            json!({"id": id, "surface": "existing"}),
        )
        .unwrap();
        assert_ne!(p["contours"], e["contours"]);
        // Undo of the pad restores the old numbers (the cache follows the revision).
        assert!(s.editor.undo());
        let c = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert_eq!(c["fill_m3"].as_f64().unwrap(), 0.0);
    }

    #[test]
    fn update_changes_settings_and_sources() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        let id = call(&mut s, "site.create", json!({"source_ids": &ids[..40]})).unwrap()["id"]
            .as_uid()
            .unwrap();
        let extra = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "polyline3d", "closed": false, "points": [[0, 0, 0], [1000, 0, 100]]}}),
        );
        let r = call(&mut s, "site.update", json!({"id": id, "settings": {"smooth_contours": true, "existing_color": [1, 2, 3, 255]}, "add_source_ids": [extra], "remove_points": [0, 1]})).unwrap();
        assert_eq!(
            (r["points"].as_u64().unwrap(), r["lines"].as_u64().unwrap()),
            (38, 1)
        );
        assert!(s.editor.doc().object(ObjectId(extra)).is_none());
        let o = call(&mut s, "objects.get", json!({"id": id})).unwrap();
        assert_eq!(o["shape"]["settings"]["smooth_contours"], true);
        assert_eq!(o["shape"]["settings"]["minor_interval"], 500.0);
        let r = call(
            &mut s,
            "site.update",
            json!({"id": id, "settings": {"existing_color": null}, "remove_lines": [0]}),
        )
        .unwrap();
        assert_eq!(r["lines"], 0);
        // Errors.
        assert!(
            call(
                &mut s,
                "site.update",
                json!({"id": id, "remove_points": [500]})
            )
            .unwrap_err()
            .message
            .contains("out of range")
        );
        assert!(
            call(
                &mut s,
                "site.update",
                json!({"id": id, "remove_lines": [1]})
            )
            .unwrap_err()
            .message
            .contains("out of range")
        );
        assert!(
            call(
                &mut s,
                "site.update",
                json!({"id": id, "add_source_ids": [id]})
            )
            .unwrap_err()
            .message
            .contains("own source")
        );
        assert!(
            call(&mut s, "site.update", json!({"id": ids[45]}))
                .unwrap_err()
                .message
                .contains("not a site model")
        );
        assert!(
            call(
                &mut s,
                "site.update",
                json!({"id": "00000000-0000-0000-0000-00000001869f"})
            )
            .unwrap_err()
            .message
            .contains("does not exist")
        );
        assert!(
            call(
                &mut s,
                "site.update",
                json!({"id": id, "settings": {"major_interval": 1}})
            )
            .is_err()
        );
        assert!(call(&mut s, "site.nothing", json!({})).unwrap_err().code == -32601);
    }

    #[test]
    fn render_svg_and_png_show_the_terrain() {
        let mut s = Session::with_sequential_ids();
        let ids = grid(&mut s);
        call(&mut s, "site.create", json!({"source_ids": ids})).unwrap();
        let svg = call(&mut s, "render.svg", json!({})).unwrap()["svg"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(svg.contains(r#"data-kind="contour""#));
        assert!(s.scene().mesh.triangle_count() > 100);
        let png = call(
            &mut s,
            "render.png",
            json!({"view": "left_isometric", "width": 64, "height": 64}),
        );
        assert!(png.is_ok(), "{png:?}");
    }
}
