//! Pads with a retaining edge: `pad.add` and `pad.retaining_edge`.
//!
//! A pad is a site modifier that sets the ground inside its boundary to a level. Where the pad
//! is above or below the ground, its edge is a vertical break. A retaining edge builds that break
//! as a landscape wall: a closed wall along the boundary of the pad, with its top at the level of
//! the pad (plus an offset). The wall is an ordinary wall, so it has a style, quantities and a
//! 3D solid. The pad keeps the ID of its wall in its data, and the wall keeps the ID of its pad.

use kadakas_core::uid::AsUid;
use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use kadakas_core::command::Command;
use kadakas_core::curve::Vertex;
use kadakas_core::model::{Attributes, ObjectId, Shape};
use kadakas_core::site::ModifierKind;
use kadakas_site::grading::wall;

use super::{map_of, resolve_style, take_id, take_where};
use crate::errors::bad;
use crate::{RpcError, Session};

/// The data key of a pad that has a retaining edge: `{wall, thickness, top_offset}`.
pub const PAD_KEY: &str = "retaining_edge";
/// The data key of the wall of a retaining edge: the ID of its pad.
pub const WALL_KEY: &str = "retaining_pad";

pub const PAD_ADD: (&str, &str) = (
    "pad.add",
    "Add a pad: a site modifier that sets the ground inside its boundary to a level. params: {boundary: [vertices] (arcs allowed), elevation (mm), slope? ({direction (radians the ground falls), percent}), retaining_edge? (true or the settings of pad.retaining_edge: a wall along the edge), layer?, class?}. Where pads overlap, the later one wins. A pad without a grade limit has a vertical edge. Returns {id, wall?, quantities?}.",
);
pub const GRADE_LIMIT_ADD: (&str, &str) = (
    "grade_limit.add",
    "Add grade limits: a site modifier (Site Modifiers tool, Grade Limits mode) that closes the area in which the proposed surface may differ from the existing one. The ground outside the boundary keeps its existing height, and a pad inside it gets sloping sides down to that ground; without grade limits a pad has a vertical edge. A thin method over the add_object command with a site_modifier shape. params: {boundary: [vertices] (3 or more, arcs allowed), layer? (the modifier acts on the site models of its layer), class?}. Returns {id}.",
);
pub const PAD_RETAINING_EDGE: (&str, &str) = (
    "pad.retaining_edge",
    "Give a pad a retaining edge: a closed landscape wall along its boundary, with its top at the level of the pad plus top_offset, standing on the existing ground. The wall is centered on the edge. The pad must be level. Calling it again replaces the wall. params: {id (a pad), enabled? (false removes the wall; default true), thickness? (mm, default 300), top_offset? (mm, default 0), embed? (mm below the ground, default 0), style? (wall style ID or name), class?}. One undo step. Returns {id, wall, quantities} (the rows of wall.report).",
);

/// The settings of a retaining edge.
struct Edge {
    thickness: f64,
    top_offset: f64,
    embed: f64,
    style: Option<Value>,
    class: Option<Value>,
}

fn number(m: &Map<String, Value>, key: &str, default: f64) -> Result<f64, RpcError> {
    match m.get(key).filter(|v| !v.is_null()) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .ok_or_else(|| bad(format!("'{key}' must be a number"))),
    }
}

impl Edge {
    fn read(m: &Map<String, Value>) -> Result<Edge, RpcError> {
        let e = Edge {
            thickness: number(m, "thickness", wall::DEFAULT_THICKNESS)?,
            top_offset: number(m, "top_offset", 0.0)?,
            embed: number(m, "embed", 0.0)?,
            style: m.get("style").filter(|v| !v.is_null()).cloned(),
            class: m.get("class").filter(|v| !v.is_null()).cloned(),
        };
        if !(e.thickness > 0.0 && e.thickness <= 10_000.0) {
            return Err(bad("thickness must be more than 0 and at most 10 000 mm"));
        }
        if !(0.0..=1.0e6).contains(&e.embed) {
            return Err(bad("embed must be from 0 to 1 000 000 mm"));
        }
        if e.top_offset.abs() > 1.0e6 {
            return Err(bad("top_offset is too large"));
        }
        Ok(e)
    }
}

/// The level and the boundary of a pad object.
fn pad_parts(s: &Session, id: ObjectId) -> Result<(f64, Vec<Vertex>), RpcError> {
    let o = s
        .editor
        .doc()
        .object(id)
        .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
    match &o.shape {
        Shape::SiteModifier {
            modifier:
                ModifierKind::Pad {
                    elevation,
                    slope: None,
                },
            boundary,
        } => Ok((*elevation, boundary.clone())),
        Shape::SiteModifier {
            modifier: ModifierKind::Pad { .. },
            ..
        } => Err(RpcError::validation(format!(
            "pad {id} is sloped. A retaining edge needs a level pad"
        ))),
        other => Err(RpcError::validation(format!(
            "object {id} is a {}, not a pad",
            other.kind()
        ))),
    }
}

impl Session {
    /// The commands that build (or replace) the retaining edge of a pad. The wall gets the next ID
    /// after the ones in `ahead` (objects that the same batch creates first).
    /// `cut` if the existing ground along the edge is higher than the pad everywhere (the wall holds the
    /// ground back and its top is the ground), else `fill`. Without a site model: `fill`.
    fn edge_mode(
        &self,
        layer: Option<kadakas_core::model::LayerId>,
        level: f64,
        boundary: &[Vertex],
    ) -> (&'static str, f64) {
        use kadakas_core::ground::GroundSource;
        use kadakas_site::grading::{self, Follow};
        let doc = self.editor.doc();
        let layer = layer.unwrap_or(doc.active_layer);
        let at = boundary.first().map_or(glam::DVec2::ZERO, |v| v.p);
        let Some(site) = grading::host_near(
            doc,
            &self.site_cache,
            self.editor.revision(),
            layer,
            None,
            at,
        ) else {
            return ("fill", 0.0);
        };
        let ground = grading::SiteGround::new(&site);
        let (mut above, mut below, mut sum, mut n) = (0, 0, 0.0, 0);
        for p in kadakas_core::curve::flatten(boundary, true, 100.0) {
            if let Some(z) = ground.exact(None, p, Follow::Existing) {
                sum += z;
                n += 1;
                if z > level + 1.0 {
                    above += 1;
                } else if z < level - 1.0 {
                    below += 1;
                }
            }
        }
        if above > 0 && below == 0 {
            // The wall goes from the pad up to the ground: its depth below the ground is the mean
            // height of the ground over the pad.
            ("cut", sum / n as f64 - level)
        } else {
            ("fill", 0.0)
        }
    }

    fn edge_commands(
        &self,
        pad: ObjectId,
        pad_id_is_new: bool,
        edge: &Edge,
        level: f64,
        boundary: Vec<Vertex>,
        ahead: u64,
    ) -> Result<(Vec<Command>, ObjectId), RpcError> {
        let doc = self.editor.doc();
        let layer = if pad_id_is_new {
            None
        } else {
            doc.object(pad).map(|o| o.layer)
        };
        let mut wall_data = Map::new();
        wall_data.insert("thickness".into(), json!(edge.thickness));
        let layer_of_pad = layer.or_else(|| doc.object(pad).map(|o| o.layer));
        let (mode, depth) = self.edge_mode(layer_of_pad, level, &boundary);
        if mode == "cut" {
            // The wall holds back ground that is higher than the pad: its top is the ground, and
            // it reaches down to the pad (a constant depth: the mean height of the ground over the pad).
            wall_data.insert(
                "top".into(),
                json!({"mode": "terrain", "offset": edge.top_offset}),
            );
            wall_data.insert("embed".into(), json!(edge.embed + depth));
        } else {
            wall_data.insert(
                "top".into(),
                json!({"mode": "fixed", "elevation": level + edge.top_offset}),
            );
        }
        if mode != "cut" {
            wall_data.insert("embed".into(), json!(edge.embed));
        }
        if let Some(st) = &edge.style {
            wall_data.insert("style".into(), st.clone());
            resolve_style(doc, &mut wall_data)?;
        }
        let data: wall::WallData = super::patched(&wall::WallData::default(), &wall_data, "wall")?;
        data.validate().map_err(bad)?;
        let class = match &edge.class {
            Some(v) => Some(crate::sheets_api::class_ref(doc, v)?),
            None => None,
        };
        let wall_id = ObjectId(doc.next_ids(ahead as usize + 1)[ahead as usize]);
        let mut cmds = Vec::new();
        // The old wall of this pad goes first.
        if !pad_id_is_new
            && let Some(old) = doc
                .object(pad)
                .and_then(|o| o.data.get(PAD_KEY))
                .and_then(|v| v.get("wall"))
                .and_then(AsUid::as_uid)
                .map(ObjectId)
                .filter(|w| doc.object(*w).is_some_and(|o| wall::data(o).is_some()))
        {
            cmds.push(Command::DeleteObjects { ids: vec![old] });
        }
        cmds.push(Command::AddObject {
            shape: Shape::Polyline {
                points: boundary,
                closed: true,
            },
            layer,
            class,
            attrs: Attributes::default(),
        });
        cmds.push(Command::SetData {
            id: wall_id,
            set: BTreeMap::from([
                (
                    wall::KEY.to_string(),
                    serde_json::to_value(&data).expect("JSON"),
                ),
                (WALL_KEY.to_string(), json!(pad)),
            ]),
            remove: Vec::new(),
        });
        cmds.push(Command::SetData {
            id: pad,
            set: BTreeMap::from([(
                PAD_KEY.to_string(),
                json!({"wall": wall_id, "thickness": edge.thickness, "top_offset": edge.top_offset, "mode": mode}),
            )]),
            remove: Vec::new(),
        });
        Ok((cmds, wall_id))
    }

    pub(super) fn pad_retaining_edge(&mut self, p: Value) -> Result<Value, RpcError> {
        let mut m = map_of(p)?;
        let id = take_id(&mut m, "id")?;
        let enabled = m
            .remove("enabled")
            .map(|v| v.as_bool())
            .unwrap_or(Some(true));
        let enabled = enabled.ok_or_else(|| bad("'enabled' must be true or false"))?;
        let (level, boundary) = pad_parts(self, id)?;
        if !enabled {
            let doc = self.editor.doc();
            let old = doc
                .object(id)
                .and_then(|o| o.data.get(PAD_KEY))
                .and_then(|v| v.get("wall"))
                .and_then(AsUid::as_uid)
                .map(ObjectId)
                .filter(|w| doc.object(*w).is_some());
            let mut cmds = Vec::new();
            if let Some(w) = old {
                cmds.push(Command::DeleteObjects { ids: vec![w] });
            }
            cmds.push(Command::SetData {
                id,
                set: BTreeMap::new(),
                remove: vec![PAD_KEY.to_string()],
            });
            self.editor
                .execute(&Command::Batch { commands: cmds })
                .map_err(RpcError::from)?;
            self.prune_selection();
            return Ok(json!({"id": id, "wall": null}));
        }
        let edge = Edge::read(&m)?;
        let (cmds, wall_id) = self.edge_commands(id, false, &edge, level, boundary, 0)?;
        self.editor
            .execute(&Command::Batch { commands: cmds })
            .map_err(RpcError::from)?;
        self.prune_selection();
        Ok(json!({"id": id, "wall": wall_id, "quantities": self.wall_row(wall_id)}))
    }

    pub(super) fn grade_limit_add(&mut self, p: Value) -> Result<Value, RpcError> {
        let mut m = map_of(p)?;
        let (layer, class) = take_where(&mut m)?;
        let boundary: Vec<Vertex> = serde_json::from_value(
            m.remove("boundary")
                .ok_or_else(|| bad("missing 'boundary': [vertices]"))?,
        )
        .map_err(|e| bad(format!("bad boundary: {e}")))?;
        if boundary.len() < 3 {
            return Err(bad("a grade limit boundary needs 3 or more points"));
        }
        let out = self
            .editor
            .execute(&Command::AddObject {
                shape: Shape::SiteModifier {
                    modifier: ModifierKind::GradeLimit,
                    boundary,
                },
                layer,
                class,
                attrs: Attributes::default(),
            })
            .map_err(RpcError::from)?;
        self.prune_selection();
        Ok(json!({ "id": out.created[0] }))
    }

    pub(super) fn pad_add(&mut self, p: Value) -> Result<Value, RpcError> {
        let mut m = map_of(p)?;
        let (layer, class) = take_where(&mut m)?;
        let boundary: Vec<Vertex> = serde_json::from_value(
            m.remove("boundary")
                .ok_or_else(|| bad("missing 'boundary': [vertices]"))?,
        )
        .map_err(|e| bad(format!("bad boundary: {e}")))?;
        if boundary.len() < 3 {
            return Err(bad("a pad boundary needs 3 or more points"));
        }
        let elevation = m
            .remove("elevation")
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
            .ok_or_else(|| bad("give elevation: the level of the pad in mm"))?;
        let slope = match m.remove("slope").filter(|v| !v.is_null()) {
            None => None,
            Some(v) => Some(
                serde_json::from_value::<kadakas_core::site::PadSlope>(v)
                    .map_err(|e| bad(format!("bad slope: {e}")))?,
            ),
        };
        let edge = match m.remove("retaining_edge") {
            None | Some(Value::Null) | Some(Value::Bool(false)) => None,
            Some(Value::Bool(true)) => Some(Edge::read(&Map::new())?),
            Some(Value::Object(o)) => Some(Edge::read(&o)?),
            Some(_) => return Err(bad("'retaining_edge' must be true, false or settings")),
        };
        if edge.is_some() && slope.is_some() {
            return Err(bad("a retaining edge needs a level pad: give no slope"));
        }
        let pad_id = ObjectId(self.editor.doc().next_ids(1)[0]);
        let mut cmds = vec![Command::AddObject {
            shape: Shape::SiteModifier {
                modifier: ModifierKind::Pad { elevation, slope },
                boundary: boundary.clone(),
            },
            layer,
            class,
            attrs: Attributes::default(),
        }];
        let mut wall_id = None;
        if let Some(edge) = &edge {
            let (more, w) = self.edge_commands(pad_id, true, edge, elevation, boundary, 1)?;
            // The wall goes on the layer of the pad.
            let mut more = more;
            if let Some(Command::AddObject { layer: l, .. }) = more.first_mut() {
                *l = layer;
            }
            cmds.extend(more);
            wall_id = Some(w);
        }
        self.editor
            .execute(&Command::Batch { commands: cmds })
            .map_err(RpcError::from)?;
        self.prune_selection();
        Ok(json!({
            "id": pad_id,
            "wall": wall_id,
            "quantities": wall_id.and_then(|w| self.wall_row(w)),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &mut Session, method: &str, p: Value) -> Value {
        s.handle(method, p)
            .unwrap_or_else(|e| panic!("{method}: {}", e.message))
    }

    #[test]
    fn grade_limit_add_makes_a_site_modifier_and_checks_its_boundary() {
        let mut s = Session::with_sequential_ids();
        let site = site(&mut s);
        let ring = json!([[0, 0], [10000, 0], [10000, 10000], [0, 10000]]);
        let r = ok(&mut s, "grade_limit.add", json!({"boundary": ring}));
        let o = ok(&mut s, "objects.get", json!({"id": r["id"]}));
        assert_eq!(o["shape"]["kind"], "site_modifier");
        assert_eq!(o["shape"]["modifier"]["type"], "grade_limit");
        // The site model counts it as one of its modifiers.
        let sr = ok(&mut s, "site.cut_fill", json!({"id": site}));
        assert!(
            sr["modifiers"].as_array().unwrap().contains(&r["id"]),
            "{sr}"
        );
        let e = s
            .handle("grade_limit.add", json!({"boundary": [[0, 0], [1, 1]]}))
            .unwrap_err();
        assert!(e.message.contains("3 or more"), "{}", e.message);
        let e = s.handle("grade_limit.add", json!({})).unwrap_err();
        assert!(e.message.contains("boundary"), "{}", e.message);
        assert!(s.editor.undo());
    }

    /// A site 20 m by 20 m, flat at 0.
    fn site(s: &mut Session) -> kadakas_core::uid::Uid {
        let mut ids = Vec::new();
        for x in 0..5 {
            for y in 0..5 {
                let r = ok(
                    s,
                    "execute",
                    json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 5000, y * 5000, 0]}}}),
                );
                ids.push(r["created"][0].as_uid().unwrap());
            }
        }
        ok(s, "site.create", json!({"source_ids": ids}))["id"]
            .as_uid()
            .unwrap()
    }

    const SQUARE: [[i64; 2]; 4] = [[5000, 5000], [15000, 5000], [15000, 15000], [5000, 15000]];

    #[test]
    fn a_pad_with_a_retaining_edge_adds_a_wall_at_the_level_of_the_pad() {
        let mut s = Session::with_sequential_ids();
        let site = site(&mut s);
        let r = ok(
            &mut s,
            "pad.add",
            json!({"boundary": SQUARE, "elevation": 800, "retaining_edge": {"thickness": 250, "top_offset": 100}}),
        );
        let (pad, wall) = (r["id"].as_uid().unwrap(), r["wall"].as_uid().unwrap());
        let rows = r["quantities"].clone();
        assert_eq!(rows["length_m"], 40.0);
        assert_eq!(rows["thickness_mm"], 250.0);
        // The top is 100 mm above the pad (900 mm), the ground is at 0: the wall is 900 mm high.
        assert!(
            (rows["max_height_mm"].as_f64().unwrap() - 900.0).abs() < 1.0,
            "{rows}"
        );
        let w = ok(&mut s, "objects.get", json!({"id": wall}));
        assert_eq!(w["data"]["retaining_pad"], pad);
        assert_eq!(w["data"]["landscape_wall"]["top"]["elevation"], 900.0);
        assert_eq!(
            ok(&mut s, "objects.get", json!({"id": pad}))["data"]["retaining_edge"]["wall"],
            wall
        );
        // The pad raises the proposed ground to 800 mm.
        let z = ok(
            &mut s,
            "site.elevation",
            json!({"id": site, "x": 10000, "y": 10000, "surface": "proposed"}),
        );
        assert!((z["z"].as_f64().unwrap() - 800.0).abs() < 1e-6);
        // Calling it again replaces the wall.
        let r2 = ok(
            &mut s,
            "pad.retaining_edge",
            json!({"id": pad, "thickness": 400}),
        );
        let wall2 = r2["wall"].as_uid().unwrap();
        assert_ne!(wall, wall2);
        assert!(s.handle("objects.get", json!({"id": wall})).is_err());
        assert_eq!(r2["quantities"]["thickness_mm"], 400.0);
        // A single undo returns to the first wall.
        assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
        assert!(s.handle("objects.get", json!({"id": wall})).is_ok());
        ok(&mut s, "redo", json!({}));
        // Off: the wall and the link go.
        let r3 = ok(
            &mut s,
            "pad.retaining_edge",
            json!({"id": pad, "enabled": false}),
        );
        assert!(r3["wall"].is_null());
        assert!(s.handle("objects.get", json!({"id": wall2})).is_err());
        assert!(
            ok(&mut s, "objects.get", json!({"id": pad}))["data"]
                .get("retaining_edge")
                .is_none()
        );
        // Wall style by name.
        ok(
            &mut s,
            "wall_style.create",
            json!({"name": "Stone", "thickness": 350}),
        );
        let r4 = ok(
            &mut s,
            "pad.retaining_edge",
            json!({"id": pad, "style": "Stone"}),
        );
        assert_eq!(r4["quantities"]["thickness_mm"], 350.0);
    }

    #[test]
    fn retaining_edge_errors() {
        let mut s = Session::with_sequential_ids();
        site(&mut s);
        let sloped = ok(
            &mut s,
            "pad.add",
            json!({"boundary": SQUARE, "elevation": 800, "slope": {"direction": 0.0, "percent": 2.0}}),
        )["id"]
            .clone();
        assert!(
            s.handle("pad.retaining_edge", json!({"id": sloped}))
                .is_err()
        );
        assert!(
            s.handle(
                "pad.add",
                json!({"boundary": SQUARE, "elevation": 0, "slope": {"direction": 0.0, "percent": 1.0}, "retaining_edge": true})
            )
            .is_err()
        );
        let circle = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}))["created"][0].clone();
        for bad in [
            json!({}),
            json!({"id": "00000000-0000-0000-0000-00000000270f"}),
            json!({"id": circle}),
            json!({"id": sloped, "enabled": "yes"}),
        ] {
            assert!(s.handle("pad.retaining_edge", bad).is_err());
        }
        for bad in [
            json!({"elevation": 0}),
            json!({"boundary": SQUARE}),
            json!({"boundary": [[0, 0], [1, 1]], "elevation": 0}),
            json!({"boundary": SQUARE, "elevation": 0, "retaining_edge": 5}),
            json!({"boundary": SQUARE, "elevation": 0, "retaining_edge": {"thickness": 0}}),
            json!({"boundary": SQUARE, "elevation": 0, "retaining_edge": {"embed": -1}}),
            json!({"boundary": SQUARE, "elevation": 0, "retaining_edge": {"top_offset": 1e9}}),
            json!({"boundary": SQUARE, "elevation": 0, "retaining_edge": {"style": "none"}}),
        ] {
            assert!(s.handle("pad.add", bad).is_err());
        }
        // Without a retaining edge a pad is just a pad.
        let plain = ok(
            &mut s,
            "pad.add",
            json!({"boundary": SQUARE, "elevation": 100}),
        );
        assert!(plain["wall"].is_null());
        let with_true = ok(
            &mut s,
            "pad.add",
            json!({"boundary": SQUARE, "elevation": 100, "retaining_edge": true}),
        );
        assert!(with_true["wall"].is_string());
    }

    #[test]
    fn a_cut_pad_gets_a_wall_from_the_pad_up_to_the_ground() {
        let mut s = Session::with_sequential_ids();
        site(&mut s);
        let r = ok(
            &mut s,
            "pad.add",
            json!({"boundary": SQUARE, "elevation": -1000, "retaining_edge": true}),
        );
        let q = &r["quantities"];
        assert!(
            (q["max_height_mm"].as_f64().unwrap() - 1000.0).abs() < 1.0,
            "{q}"
        );
        assert!(
            (q["min_height_mm"].as_f64().unwrap() - 1000.0).abs() < 1.0,
            "{q}"
        );
        assert!(
            q["face_area_m2"].as_f64().unwrap() > 30.0 && q["volume_m3"].as_f64().unwrap() > 0.0
        );
        let w = ok(&mut s, "objects.get", json!({"id": r["wall"]}));
        assert_eq!(w["data"]["landscape_wall"]["top"]["mode"], "terrain");
        let pad = ok(&mut s, "objects.get", json!({"id": r["id"]}));
        assert_eq!(pad["data"]["retaining_edge"]["mode"], "cut");
    }

    #[test]
    fn the_wall_follows_its_pad_and_goes_with_it() {
        let mut s = Session::with_sequential_ids();
        site(&mut s);
        let r = ok(
            &mut s,
            "pad.add",
            json!({"boundary": SQUARE, "elevation": 800, "retaining_edge": {"top_offset": 100}}),
        );
        let (pad, wall) = (r["id"].as_uid().unwrap(), r["wall"].as_uid().unwrap());
        // Moving the pad moves the wall (also when only the pad is moved).
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "move_objects", "ids": [pad], "delta": [1000, 2000]}}),
        );
        let w = ok(&mut s, "objects.get", json!({"id": wall}));
        assert_eq!(w["shape"]["points"][0], json!([6000.0, 7000.0]));
        // Changing the level of the pad changes the top of the wall.
        let mut shape = ok(&mut s, "objects.get", json!({"id": pad}))["shape"].clone();
        shape["modifier"]["elevation"] = json!(1500);
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "modify_object", "id": pad, "shape": shape}}),
        );
        let w = ok(&mut s, "objects.get", json!({"id": wall}));
        assert_eq!(w["data"]["landscape_wall"]["top"]["elevation"], 1600.0);
        // Deleting the pad deletes the wall. One undo brings both back.
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "delete_objects", "ids": [pad]}}),
        );
        assert!(s.handle("objects.get", json!({"id": wall})).is_err());
        assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
        assert!(s.handle("objects.get", json!({"id": wall})).is_ok());
        assert!(s.handle("objects.get", json!({"id": pad})).is_ok());
        // Undoing the edit undoes the wall too.
        ok(&mut s, "undo", json!({}));
        let w = ok(&mut s, "objects.get", json!({"id": wall}));
        assert_eq!(w["data"]["landscape_wall"]["top"]["elevation"], 900.0);
    }
}
