//! API methods of site models: `site.create`, `site.update`, `site.elevation`, `site.cut_fill`
//! and `site.contours`. The terrain calculations are in `cadektor-site`.

use std::sync::Arc;

use glam::DVec2;
use serde::Deserialize;
use serde_json::{json, Value};

use cadektor_core::command::Command;
use cadektor_core::model::{Attributes, Document, ObjectId, Shape, SiteLine, SiteSettings};
use cadektor_site::{SiteResult, Surface};

use crate::{params, RpcError, Session};

#[derive(Deserialize)]
struct Create {
    source_ids: Vec<ObjectId>,
    #[serde(default)]
    settings: Value,
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
        Some(n) => Err(RpcError::invalid_params(format!("unknown surface '{n}'. Use existing or proposed"))),
    }
}

/// Takes 3D loci and 3D polylines out of objects. Fails for any other kind of object.
fn take_sources(doc: &Document, ids: &[ObjectId], points: &mut Vec<glam::DVec3>, lines: &mut Vec<SiteLine>) -> Result<(), RpcError> {
    for id in ids {
        let o = doc.object(*id).ok_or_else(|| RpcError::app(format!("object {id} does not exist")))?;
        match &o.shape {
            Shape::Locus3d { p } => points.push(*p),
            Shape::Polyline3d { points: pts, closed } => lines.push(SiteLine { points: pts.clone(), closed: *closed }),
            other => return Err(RpcError::invalid_params(format!("object {id} is a {}: only locus3d and polyline3d can be site model sources", other.kind()))),
        }
    }
    Ok(())
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
                let Create { source_ids, settings } = params(p)?;
                if source_ids.is_empty() {
                    return Err(RpcError::invalid_params("give source_ids: at least one locus3d or polyline3d"));
                }
                let doc = self.editor.doc();
                let (mut points, mut lines) = (Vec::new(), Vec::new());
                take_sources(doc, &source_ids, &mut points, &mut lines)?;
                let settings = merge_settings(&SiteSettings::default(), &settings)?;
                let layer = doc.object(source_ids[0]).map(|o| o.layer);
                let create = Command::AddObject { shape: Shape::SiteModel { points, lines, settings }, layer, class: None, attrs: Attributes::default() };
                let out = self.editor.execute(&Command::Batch { commands: vec![create, Command::DeleteObjects { ids: source_ids }] }).map_err(RpcError::app)?;
                self.prune_selection();
                Ok(json!({ "id": out.created[0] }))
            }
            "site.update" => {
                let Update { id, settings, add_source_ids, remove_points, remove_lines } = params(p)?;
                let doc = self.editor.doc();
                let obj = doc.object(id).ok_or_else(|| RpcError::app(format!("object {id} does not exist")))?;
                let Shape::SiteModel { points, lines, settings: current } = &obj.shape else {
                    return Err(RpcError::app(format!("object {id} is not a site model")));
                };
                let (mut points, mut lines) = (points.clone(), lines.clone());
                for (what, list, len) in [("point", &remove_points, points.len()), ("line", &remove_lines, lines.len())] {
                    if let Some(i) = list.iter().find(|i| **i >= len) {
                        return Err(RpcError::invalid_params(format!("{what} index {i} is out of range (0 to {})", len.saturating_sub(1))));
                    }
                }
                let keep = |len: usize, remove: &[usize]| -> Vec<bool> { (0..len).map(|i| !remove.contains(&i)).collect() };
                let kp = keep(points.len(), &remove_points);
                let kl = keep(lines.len(), &remove_lines);
                let mut it = kp.iter();
                points.retain(|_| *it.next().unwrap());
                let mut it = kl.iter();
                lines.retain(|_| *it.next().unwrap());
                if add_source_ids.contains(&id) {
                    return Err(RpcError::invalid_params("a site model cannot be its own source"));
                }
                take_sources(doc, &add_source_ids, &mut points, &mut lines)?;
                let settings = merge_settings(current, &settings)?;
                let (np, nl) = (points.len(), lines.len());
                let modify = Command::ModifyObject { id, shape: Some(Shape::SiteModel { points, lines, settings }), layer: None, class: None, attrs: None };
                let mut commands = vec![modify];
                if !add_source_ids.is_empty() {
                    commands.push(Command::DeleteObjects { ids: add_source_ids });
                }
                self.editor.execute(&Command::Batch { commands }).map_err(RpcError::app)?;
                self.prune_selection();
                Ok(json!({ "id": id, "points": np, "lines": nl }))
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
                Ok(json!({ "count": list.len(), "major_count": list.iter().filter(|c| c.major).count(), "levels": levels, "contours": contours }))
            }
            _ => Err(RpcError { code: -32601, message: format!("unknown method: {method}") }),
        }
    }

    /// The derived surfaces of a site model, from the cache.
    fn site(&self, id: ObjectId) -> Result<Arc<SiteResult>, RpcError> {
        self.site_cache.get(self.editor.revision(), self.editor.doc(), id).map_err(RpcError::app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Session, method: &str, p: Value) -> Result<Value, RpcError> {
        s.handle(method, p)
    }

    fn exec(s: &mut Session, command: Value) -> u64 {
        call(s, "execute", json!({"command": command})).unwrap()["created"][0].as_u64().unwrap()
    }

    /// A 9 by 9 grid of loci, 1 m apart, on a plane that rises 10 percent toward +X.
    fn grid(s: &mut Session) -> Vec<u64> {
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
        let mut s = Session::default();
        let ids = grid(&mut s);
        let r = call(&mut s, "site.create", json!({"source_ids": ids, "settings": {"minor_interval": 200, "major_interval": 400, "display_2d": "both"}})).unwrap();
        let id = r["id"].as_u64().unwrap();
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
    fn create_checks_its_input() {
        let mut s = Session::default();
        let circle = exec(&mut s, json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}));
        let e = call(&mut s, "site.create", json!({"source_ids": []})).unwrap_err();
        assert_eq!(e.code, -32602);
        let e = call(&mut s, "site.create", json!({"source_ids": [circle]})).unwrap_err();
        assert!(e.message.contains("only locus3d"), "{}", e.message);
        let e = call(&mut s, "site.create", json!({"source_ids": [999]})).unwrap_err();
        assert!(e.message.contains("does not exist"));
        let locus = exec(&mut s, json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [0, 0, 0]}}));
        let e = call(&mut s, "site.create", json!({"source_ids": [locus], "settings": {"minor_interval": -1}})).unwrap_err();
        assert!(e.message.contains("positive"), "{}", e.message);
        let e = call(&mut s, "site.create", json!({"source_ids": [locus], "settings": 5})).unwrap_err();
        assert!(e.message.contains("object"));
        let e = call(&mut s, "site.create", json!({"source_ids": [locus], "settings": {"display_2d": "sideways"}})).unwrap_err();
        assert_eq!(e.code, -32602);
        // A failed create leaves the document alone.
        assert!(s.editor.doc().object(ObjectId(locus)).is_some());
    }

    #[test]
    fn elevation_slope_and_surfaces() {
        let mut s = Session::default();
        let ids = grid(&mut s);
        let id = call(&mut s, "site.create", json!({"source_ids": ids})).unwrap()["id"].as_u64().unwrap();
        let r = call(&mut s, "site.elevation", json!({"id": id, "x": 2500, "y": 3100})).unwrap();
        assert!((r["z"].as_f64().unwrap() - 250.0).abs() < 1e-9);
        assert!((r["slope"].as_f64().unwrap() - 0.1).abs() < 1e-9);
        // The ground falls toward -X, so it faces west: 270 degrees clockwise from north.
        assert!((r["aspect"].as_f64().unwrap() - 1.5 * std::f64::consts::PI).abs() < 1e-9);
        let r = call(&mut s, "site.elevation", json!({"id": id, "x": -10, "y": 0, "surface": "existing"})).unwrap();
        assert!(r["z"].is_null() && r["slope"].is_null());
        assert_eq!(call(&mut s, "site.elevation", json!({"id": id, "x": 0, "y": 0, "surface": "x"})).unwrap_err().code, -32602);
        assert_eq!(call(&mut s, "site.elevation", json!({"id": id, "x": "a", "y": 0})).unwrap_err().code, -32602);
        let e = call(&mut s, "site.elevation", json!({"id": 123456, "x": 0, "y": 0})).unwrap_err();
        assert!(e.message.contains("does not exist"));
    }

    #[test]
    fn non_finite_coordinates_are_refused() {
        let mut s = Session::default();
        let ids = grid(&mut s);
        let id = call(&mut s, "site.create", json!({"source_ids": ids})).unwrap()["id"].as_u64().unwrap();
        // JSON cannot hold NaN, but a very large number is still finite and simply outside the model.
        let r = call(&mut s, "site.elevation", json!({"id": id, "x": 1e300, "y": 0})).unwrap();
        assert!(r["z"].is_null());
        let p = serde_json::from_str::<Value>(r#"{"id": 1, "x": 1e999, "y": 0}"#);
        assert!(p.is_err() || call(&mut s, "site.elevation", p.unwrap()).is_err());
    }

    #[test]
    fn pad_gives_cut_and_fill_and_contours() {
        let mut s = Session::default();
        let ids = grid(&mut s);
        let id = call(&mut s, "site.create", json!({"source_ids": ids, "settings": {"minor_interval": 100, "major_interval": 400}})).unwrap()["id"].as_u64().unwrap();
        let c = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert_eq!((c["cut_m3"].as_f64().unwrap(), c["fill_m3"].as_f64().unwrap()), (0.0, 0.0));
        assert!((c["area_m2"].as_f64().unwrap() - 64.0).abs() < 1e-9);
        let cs = call(&mut s, "site.contours", json!({"id": id, "points": false})).unwrap();
        // Levels 100..=800 mm: one line per level (level 0 is not above any vertex).
        assert_eq!(cs["count"], 8);
        assert_eq!(cs["major_count"], 2);
        assert!(cs["contours"][0].get("points").is_none());
        // A flat pad at 400 mm over x = 2..6 m, y = 2..6 m: the ground there is 200..600, so it is part cut, part fill.
        exec(&mut s, json!({"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "pad", "elevation": 400}, "boundary": [[2000,2000],[6000,2000],[6000,6000],[2000,6000]]}}));
        let c = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert!(c["cut_m3"].as_f64().unwrap() > 0.5 && c["fill_m3"].as_f64().unwrap() > 0.5, "{c}");
        let z = |s: &mut Session, x: f64, surface: &str| {
            call(s, "site.elevation", json!({"id": id, "x": x, "y": 4000, "surface": surface})).unwrap()["z"].as_f64().unwrap()
        };
        assert!((z(&mut s, 3000.0, "proposed") - 400.0).abs() < 1e-9);
        assert!((z(&mut s, 3000.0, "existing") - 300.0).abs() < 1e-9);
        let p = call(&mut s, "site.contours", json!({"id": id, "surface": "proposed"})).unwrap();
        let e = call(&mut s, "site.contours", json!({"id": id, "surface": "existing"})).unwrap();
        assert_ne!(p["contours"], e["contours"]);
        // Undo of the pad restores the old numbers (the cache follows the revision).
        assert!(s.editor.undo());
        let c = call(&mut s, "site.cut_fill", json!({"id": id})).unwrap();
        assert_eq!(c["fill_m3"].as_f64().unwrap(), 0.0);
    }

    #[test]
    fn update_changes_settings_and_sources() {
        let mut s = Session::default();
        let ids = grid(&mut s);
        let id = call(&mut s, "site.create", json!({"source_ids": &ids[..40]})).unwrap()["id"].as_u64().unwrap();
        let extra = exec(&mut s, json!({"op": "add_object", "shape": {"kind": "polyline3d", "closed": false, "points": [[0, 0, 0], [1000, 0, 100]]}}));
        let r = call(&mut s, "site.update", json!({"id": id, "settings": {"smooth_contours": true, "existing_color": [1, 2, 3, 255]}, "add_source_ids": [extra], "remove_points": [0, 1]})).unwrap();
        assert_eq!((r["points"].as_u64().unwrap(), r["lines"].as_u64().unwrap()), (38, 1));
        assert!(s.editor.doc().object(ObjectId(extra)).is_none());
        let o = call(&mut s, "objects.get", json!({"id": id})).unwrap();
        assert_eq!(o["shape"]["settings"]["smooth_contours"], true);
        assert_eq!(o["shape"]["settings"]["minor_interval"], 500.0);
        let r = call(&mut s, "site.update", json!({"id": id, "settings": {"existing_color": null}, "remove_lines": [0]})).unwrap();
        assert_eq!(r["lines"], 0);
        // Errors.
        assert!(call(&mut s, "site.update", json!({"id": id, "remove_points": [500]})).unwrap_err().message.contains("out of range"));
        assert!(call(&mut s, "site.update", json!({"id": id, "remove_lines": [1]})).unwrap_err().message.contains("out of range"));
        assert!(call(&mut s, "site.update", json!({"id": id, "add_source_ids": [id]})).unwrap_err().message.contains("own source"));
        assert!(call(&mut s, "site.update", json!({"id": ids[45]})).unwrap_err().message.contains("not a site model"));
        assert!(call(&mut s, "site.update", json!({"id": 99999})).unwrap_err().message.contains("does not exist"));
        assert!(call(&mut s, "site.update", json!({"id": id, "settings": {"major_interval": 1}})).is_err());
        assert!(call(&mut s, "site.nothing", json!({})).unwrap_err().code == -32601);
    }

    #[test]
    fn render_svg_and_png_show_the_terrain() {
        let mut s = Session::default();
        let ids = grid(&mut s);
        call(&mut s, "site.create", json!({"source_ids": ids})).unwrap();
        let svg = call(&mut s, "render.svg", json!({})).unwrap()["svg"].as_str().unwrap().to_string();
        assert!(svg.contains(r#"data-kind="contour""#));
        assert!(s.scene().mesh.triangle_count() > 100);
        let png = call(&mut s, "render.png", json!({"view": "left_isometric", "width": 64, "height": 64}));
        assert!(png.is_ok(), "{png:?}");
    }
}
