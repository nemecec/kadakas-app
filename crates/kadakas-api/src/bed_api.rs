//! Texture beds (3D-05): `bed.add`, `bed.set`, `bed.get` and `bed.list`.
//!
//! A texture bed is a site modifier (an object of kind `site_modifier`) that gives a region of the
//! terrain its own texture without changing the grade. The stacking order of beds is the drawing
//! order of the document: change it with the `arrange` command (`execute`). The data and the rules
//! are in `kadakas_core::texture_bed`; the cut of the terrain is in `kadakas_site::bed`.

use kadakas_core::uid::AsUid;
use serde_json::{Map, Value, json};

use kadakas_core::command::Command;
use kadakas_core::curve::Vertex;
use kadakas_core::model::{Attributes, ClassId, Document, LayerId, Object, ObjectId, Shape};
use kadakas_core::present;
use kadakas_core::site::ModifierKind;
use kadakas_core::texture_bed::BedMapping;

use crate::errors::bad;
use crate::present_api::{Kind, ref_of};
use crate::{RpcError, Session};

pub const BED_ADD: (&str, &str) = (
    "bed.add",
    "Add texture beds: site modifiers that give a region of the terrain its own texture without changing the grade (Site Modifiers tool, Texture Bed mode). params: {boundary: [vertices] (arcs allowed) | from: id or [ids] of closed shapes (a closed polyline, rectangle, circle, landscape area or hardscape; the first three are replaced by the bed unless keep_source is true; the bed takes the layer and the class of the shape), texture? (name or id of a material; omit or null: the material of the bed object's slot or of its class), tile_size? (mm, 10 to 1e8), offset? ([x, y] mm), rotation? (degrees): any of the three gives the bed its own mapping, which starts from the mapping of the site model on the layer; without them the bed uses the mapping of the site model, keep_source?, layer?, class?}. The bed acts on the site models on its layer whose box touches it. Where beds overlap, the one in front (later in the drawing order, see arrange) wins; a bed without a material has no effect. One undo step. Returns {id, ids}.",
);
pub const BED_SET: (&str, &str) = (
    "bed.set",
    "Change a texture bed. params: {id, texture? (name or id of a material, null for by class), tile_size?, offset?, rotation? (they give the bed its own mapping), own_mapping? (false: use the mapping of the site model), boundary? ([vertices])}. One undo step. Returns the row of bed.get.",
);
pub const BED_GET: (&str, &str) = (
    "bed.get",
    "Read a texture bed. params: {id}. Returns {id, layer, class, texture (material name or null), texture_id, by_class, material (the material that is used: the texture, or else the slot of the bed or of its class; null: the bed has no effect), own_mapping, tile_size, offset, rotation (of the bed, or else of the first site model it acts on, or the defaults), order (0 is the back of all beds), points, acts_on: [site model IDs], active}.",
);
pub const BED_LIST: (&str, &str) = (
    "bed.list",
    "List the texture beds from the back to the front of the stacking order. params: {site? (a site model ID: only the beds that act on it)}. Returns {beds: [row of bed.get]}.",
);

pub fn describe() -> Value {
    json!({
        "rules": "A texture bed is drawn on the proposed surface of the site models on its layer (on the existing surface when the model has no modifiers). The terrain triangles are cut along the outline, so the texture ends at a sharp line and the heights do not change. The texture comes from the material in 'texture', else from the material slot of the bed or of its class. Where beds overlap, the bed in front wins. At most 64 beds act on one site model (the frontmost ones), and one outline has at most 4096 points. Beds work with the color, texture and elevation looks of the site model (the elevation colors stay outside the beds).",
        "examples": [
            {"method": "bed.add", "params": {"boundary": [[0, 0], [8000, 0], [8000, 5000], [0, 5000]], "texture": "Grass"}},
            {"method": "bed.add", "params": {"boundary": [[1000, 1000], {"p": [4000, 1000], "kind": "arc", "radius": 500}, [4000, 3000], [1000, 3000]], "texture": "Gravel", "tile_size": 400, "rotation": 30}},
            {"method": "bed.add", "params": {"from": ["00000000-0000-0000-0000-000000000009"], "texture": "Paving"}},
            {"method": "bed.set", "params": {"id": "00000000-0000-0000-0000-00000000000a", "texture": null, "own_mapping": false}},
            {"method": "bed.list", "params": {}},
            {"method": "execute", "params": {"command": {"op": "arrange", "ids": ["00000000-0000-0000-0000-00000000000a"], "how": "front"}}}
        ],
    })
}

/// True if this module handles the method.
pub fn knows(method: &str) -> bool {
    matches!(method, "bed.add" | "bed.set" | "bed.get" | "bed.list")
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

fn is_bed(o: &Object) -> bool {
    matches!(
        o.shape,
        Shape::SiteModifier {
            modifier: ModifierKind::TextureBed { .. },
            ..
        }
    )
}

/// The mapping fields of the parameters, applied to a starting mapping.
struct MappingEdit {
    own: Option<bool>,
    tile: Option<f64>,
    offset: Option<[f64; 2]>,
    rotation: Option<f64>,
}

fn number(v: &Value, key: &str) -> Result<f64, RpcError> {
    v.as_f64()
        .filter(|x| x.is_finite())
        .ok_or_else(|| bad(format!("'{key}' must be a number")))
}

impl MappingEdit {
    fn take(m: &mut Map<String, Value>) -> Result<MappingEdit, RpcError> {
        let own = m
            .remove("own_mapping")
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| bad("'own_mapping' must be true or false"))
            })
            .transpose()?;
        let tile = m
            .remove("tile_size")
            .map(|v| number(&v, "tile_size"))
            .transpose()?;
        let rotation = m
            .remove("rotation")
            .map(|v| number(&v, "rotation"))
            .transpose()?;
        let offset = m
            .remove("offset")
            .map(|v| {
                let a = v
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or_else(|| bad("'offset' must be [x, y]"))?;
                Ok::<_, RpcError>([number(&a[0], "offset")?, number(&a[1], "offset")?])
            })
            .transpose()?;
        Ok(MappingEdit {
            own,
            tile,
            offset,
            rotation,
        })
    }

    /// The mapping of the bed after the edit: `current` is its mapping now, `site` the one that it
    /// starts from when it gets its own.
    fn apply(&self, current: Option<BedMapping>, site: BedMapping) -> Option<BedMapping> {
        let given = self.tile.is_some() || self.offset.is_some() || self.rotation.is_some();
        match self.own {
            Some(false) => return None,
            None if !given => return current,
            _ => {}
        }
        let mut m = current.unwrap_or(site);
        if let Some(t) = self.tile {
            m.tile = t;
        }
        if let Some(o) = self.offset {
            m.offset = o;
        }
        if let Some(r) = self.rotation {
            m.rotation = r;
        }
        Some(m)
    }
}

/// The mapping of the first site model on a layer, if there is one.
fn site_mapping(doc: &Document, layer: LayerId) -> Option<BedMapping> {
    doc.objects.values().find_map(|o| match &o.shape {
        Shape::SiteModel { settings, .. } if o.layer == layer => {
            Some(BedMapping::of_site(settings))
        }
        _ => None,
    })
}

fn texture_of(
    doc: &Document,
    v: &Value,
) -> Result<Option<kadakas_core::resource::ResourceId>, RpcError> {
    if v.is_null() {
        Ok(None)
    } else {
        ref_of(doc, v, Kind::Material).map(Some)
    }
}

/// The row of a bed. `order` counts from the back of all beds.
fn row(doc: &Document, obj: &Object, order: usize) -> Value {
    let Shape::SiteModifier {
        modifier: ModifierKind::TextureBed { texture, mapping },
        boundary,
    } = &obj.shape
    else {
        return Value::Null;
    };
    let name = |r: kadakas_core::resource::ResourceId| doc.resources.get(r).map(|x| x.name.clone());
    let texture_ok = texture.filter(|t| present::material(doc, *t).is_some());
    let material = texture_ok.or_else(|| present::bed_material(doc, obj));
    // The sites it acts on: those on its layer whose box touches the box of the bed.
    let (lo, hi) = obj.shape.bounds();
    let sites: Vec<&Object> = doc
        .objects
        .values()
        .filter(|s| {
            let (a, b) = s.shape.bounds();
            matches!(s.shape, Shape::SiteModel { .. })
                && s.layer == obj.layer
                && doc.is_visible(s)
                && a.x <= hi.x
                && b.x >= lo.x
                && a.y <= hi.y
                && b.y >= lo.y
        })
        .collect();
    let first = sites.first().and_then(|s| match &s.shape {
        Shape::SiteModel { settings, .. } => Some(BedMapping::of_site(settings)),
        _ => None,
    });
    let shown = mapping.or(first).unwrap_or_default();
    json!({
        "id": obj.id,
        "layer": obj.layer,
        "class": obj.class,
        "texture": texture.and_then(name),
        "texture_id": texture,
        "by_class": texture.is_none(),
        "material": material.and_then(name),
        "own_mapping": mapping.is_some(),
        "tile_size": shown.tile,
        "offset": shown.offset,
        "rotation": shown.rotation,
        "order": order,
        "points": boundary.len(),
        "acts_on": sites.iter().map(|s| s.id).collect::<Vec<_>>(),
        "active": material.is_some() && !sites.is_empty() && doc.is_visible(obj),
    })
}

fn rows(doc: &Document, site: Option<ObjectId>) -> Vec<Value> {
    doc.objects
        .values()
        .filter(|o| is_bed(o))
        .enumerate()
        .map(|(i, o)| row(doc, o, i))
        .filter(|r| {
            site.is_none_or(|s| {
                r["acts_on"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x.as_uid() == Some(s.0)))
            })
        })
        .collect()
}

impl Session {
    pub(crate) fn handle_bed(&mut self, method: &str, p: Value) -> Result<Value, RpcError> {
        let m = match p {
            Value::Null => Map::new(),
            Value::Object(m) => m,
            _ => return Err(bad("params must be an object")),
        };
        match method {
            "bed.add" => self.bed_add(m),
            "bed.set" => self.bed_set(m),
            "bed.get" => {
                let id = take_bed_id(self.editor.doc(), &m)?;
                let doc = self.editor.doc();
                let order = doc
                    .objects
                    .values()
                    .filter(|o| is_bed(o))
                    .position(|o| o.id == id)
                    .unwrap_or(0);
                Ok(row(doc, doc.object(id).expect("checked"), order))
            }
            "bed.list" => {
                let site = match m.get("site").filter(|v| !v.is_null()) {
                    None => None,
                    Some(v) => Some(ObjectId(
                        v.as_uid()
                            .ok_or_else(|| bad("'site' must be a site model ID"))?,
                    )),
                };
                if let Some(s) = site
                    && !self
                        .editor
                        .doc()
                        .object(s)
                        .is_some_and(|o| matches!(o.shape, Shape::SiteModel { .. }))
                {
                    return Err(RpcError::not_found(format!(
                        "object {s} is not a site model"
                    )));
                }
                Ok(json!({ "beds": rows(self.editor.doc(), site) }))
            }
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    fn bed_add(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let (layer_arg, class_arg) = take_where(&mut m)?;
        let doc = self.editor.doc();
        let texture = match m.remove("texture") {
            None => None,
            Some(v) => texture_of(doc, &v)?,
        };
        let edit = MappingEdit::take(&mut m)?;
        let keep = match m.remove("keep_source") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| bad("'keep_source' must be true or false"))?,
        };
        // The outlines: (boundary, layer, class, shape to remove).
        let mut outlines: Vec<(
            Vec<Vertex>,
            Option<LayerId>,
            Option<ClassId>,
            Option<ObjectId>,
        )> = Vec::new();
        match (m.remove("boundary"), m.remove("from")) {
            (Some(b), None) => {
                let b: Vec<Vertex> =
                    serde_json::from_value(b).map_err(|e| bad(format!("bad boundary: {e}")))?;
                outlines.push((b, None, None, None));
            }
            (None, Some(f)) => {
                let ids: Vec<ObjectId> = match f {
                    Value::Array(a) => a
                        .iter()
                        .map(|v| v.as_uid().map(ObjectId))
                        .collect::<Option<Vec<_>>>(),
                    v => v.as_uid().map(|u| vec![ObjectId(u)]),
                }
                .ok_or_else(|| bad("'from' must be an object ID or a list of IDs"))?;
                if ids.is_empty() {
                    return Err(bad("'from' has no IDs"));
                }
                for id in ids {
                    let o = doc.object(id).ok_or_else(|| {
                        RpcError::not_found(format!("object {id} does not exist"))
                    })?;
                    let (b, _) = kadakas_core::solid_batch::closed_profile_of(o)
                        .map_err(|why| bad(format!("object {id}: {why}")))?;
                    let consumed = matches!(
                        o.shape,
                        Shape::Polyline { .. } | Shape::Rect { .. } | Shape::Circle { .. }
                    ) && !keep;
                    outlines.push((b, Some(o.layer), Some(o.class), consumed.then_some(id)));
                }
            }
            _ => return Err(bad("give either 'boundary' or 'from'")),
        }
        if let Some(extra) = m.keys().next() {
            return Err(bad(format!("unknown parameter '{extra}'")));
        }
        let mut commands = Vec::new();
        let mut remove = Vec::new();
        for (boundary, layer, class, source) in outlines {
            let layer = layer_arg.or(layer);
            let on = layer.unwrap_or(doc.active_layer);
            let mapping = edit.apply(None, site_mapping(doc, on).unwrap_or_default());
            commands.push(Command::AddObject {
                shape: Shape::SiteModifier {
                    modifier: ModifierKind::TextureBed { texture, mapping },
                    boundary,
                },
                layer,
                class: class_arg.or(class),
                attrs: Attributes::default(),
            });
            remove.extend(source);
        }
        if !remove.is_empty() {
            commands.push(Command::DeleteObjects { ids: remove });
        }
        let out = self
            .editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        self.prune_selection();
        let ids: Vec<ObjectId> = out.created.iter().map(|u| ObjectId(*u)).collect();
        Ok(json!({ "id": ids.first(), "ids": ids }))
    }

    fn bed_set(&mut self, mut m: Map<String, Value>) -> Result<Value, RpcError> {
        let id = take_bed_id(self.editor.doc(), &m)?;
        m.remove("id");
        let doc = self.editor.doc();
        let obj = doc.object(id).expect("checked");
        let Shape::SiteModifier {
            modifier: ModifierKind::TextureBed { texture, mapping },
            boundary,
        } = &obj.shape
        else {
            unreachable!("checked");
        };
        let mut texture = *texture;
        if let Some(v) = m.remove("texture") {
            texture = texture_of(doc, &v)?;
        }
        let edit = MappingEdit::take(&mut m)?;
        let mapping = edit.apply(*mapping, site_mapping(doc, obj.layer).unwrap_or_default());
        let boundary = match m.remove("boundary") {
            None => boundary.clone(),
            Some(b) => serde_json::from_value::<Vec<Vertex>>(b)
                .map_err(|e| bad(format!("bad boundary: {e}")))?,
        };
        if let Some(extra) = m.keys().next() {
            return Err(bad(format!("unknown parameter '{extra}'")));
        }
        self.editor
            .execute(&Command::ModifyObject {
                id,
                shape: Some(Shape::SiteModifier {
                    modifier: ModifierKind::TextureBed { texture, mapping },
                    boundary,
                }),
                layer: None,
                class: None,
                attrs: None,
            })
            .map_err(RpcError::from)?;
        self.handle_bed("bed.get", json!({ "id": id }))
    }
}

fn take_bed_id(doc: &Document, m: &Map<String, Value>) -> Result<ObjectId, RpcError> {
    let id = m
        .get("id")
        .and_then(Value::as_uid)
        .map(ObjectId)
        .ok_or_else(|| bad("missing 'id': a texture bed"))?;
    match doc.object(id) {
        None => Err(RpcError::not_found(format!("object {id} does not exist"))),
        Some(o) if !is_bed(o) => Err(RpcError::validation(format!(
            "object {id} is not a texture bed"
        ))),
        Some(_) => Ok(id),
    }
}
