//! Agent API for hardscapes and landscape areas: styles, objects and reports.
//!
//! Methods: `hardscape_style.list|get|create|update|duplicate|delete`,
//! `landscape_area_style.list|get|create|update|duplicate|delete`,
//! `hardscape.add|set`, `landscape_area.add|set`,
//! `report.hardscape`, `report.landscape_areas`, `report.plant_list`.

use kadakas_core::uid::AsUid;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use kadakas_core::DVec2;
use kadakas_core::command::Command;
use kadakas_core::csv::write_rows as write_csv;
use kadakas_core::curve::{self, Vertex};
use kadakas_core::hardscape::offset::Align;
use kadakas_core::hardscape::{self, HardscapePatch, HardscapeStyle, Join, Mode, Slope};
use kadakas_core::landscape::{self, LandscapeAreaStyle, LandscapePatch};
use kadakas_core::model::*;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_site::SiteTerrain;

use crate::errors::bad;
use crate::{RpcError, Session};

pub const HARDSCAPE_STYLE_LIST: (&str, &str) = (
    "hardscape_style.list",
    "Hardscape styles with the number of hardscapes that use each. Returns {styles: [{id, name, used_by, total_thickness, ...fields}]}.",
);
pub const HARDSCAPE_STYLE_GET: (&str, &str) = (
    "hardscape_style.get",
    "One hardscape style. params: {id} (the ID or the style name).",
);
pub const HARDSCAPE_STYLE_CREATE: (&str, &str) = (
    "hardscape_style.create",
    "Create a hardscape style. params: {name, material? (surface material), fill?, pen? ([r,g,b,a]), thickness? (surface layer, mm, default 60), components? ([{name, material, thickness, color?}], layers below the surface from top to bottom), border? ({width, material, color?, sides? (both|left|right; a path only)} or null), joints? ({kind (running_bond|stack|herringbone|basket_weave), unit_length, unit_width, spacing (joint width), angle (degrees), color?} or null), custom? ({key: text})}. Returns {id}.",
);
pub const HARDSCAPE_STYLE_UPDATE: (&str, &str) = (
    "hardscape_style.update",
    "Change fields of a hardscape style. params: {id, name? (renames), ...fields as in create}. Hardscapes that use the style follow. A list or object you give replaces the old one; null clears border or joints.",
);
pub const HARDSCAPE_STYLE_DUPLICATE: (&str, &str) = (
    "hardscape_style.duplicate",
    "Copy a hardscape style. params: {id, name?}. Returns {id}.",
);
pub const HARDSCAPE_STYLE_DELETE: (&str, &str) = (
    "hardscape_style.delete",
    "Delete a hardscape style. params: {id, reassign?}. Fails if hardscapes use it, unless reassign names another hardscape style.",
);
pub const LANDSCAPE_AREA_STYLE_LIST: (&str, &str) = (
    "landscape_area_style.list",
    "Landscape area styles with the number of areas that use each. Returns {styles: [{id, name, used_by, ...fields}]}.",
);
pub const LANDSCAPE_AREA_STYLE_GET: (&str, &str) = (
    "landscape_area_style.get",
    "One landscape area style. params: {id} (the ID or the style name).",
);
pub const LANDSCAPE_AREA_STYLE_CREATE: (&str, &str) = (
    "landscape_area_style.create",
    "Create a landscape area style. params: {name, mix? ([{plant (plant style ID or name), percent? (share of the area, default 100, all together at most 100), spacing? (mm between plants) | density? (plants per m2), min_count?}]), fill?, pen? ([r,g,b,a]), line_weight? (mm), show_plants? (draw the plants in 2D), fields? ({key: text}, data fields such as hydrozone)}. Without spacing and density the canopy of the plant gives the spacing. Returns {id}.",
);
pub const LANDSCAPE_AREA_STYLE_UPDATE: (&str, &str) = (
    "landscape_area_style.update",
    "Change fields of a landscape area style. params: {id, name? (renames), ...fields as in create}. A new mix replaces the old one.",
);
pub const LANDSCAPE_AREA_STYLE_DUPLICATE: (&str, &str) = (
    "landscape_area_style.duplicate",
    "Copy a landscape area style. params: {id, name?}. Returns {id}.",
);
pub const LANDSCAPE_AREA_STYLE_DELETE: (&str, &str) = (
    "landscape_area_style.delete",
    "Delete a landscape area style. params: {id, reassign?}. Fails if areas use it, unless reassign names another landscape area style.",
);
pub const HARDSCAPE_ADD: (&str, &str) = (
    "hardscape.add",
    "Add a hardscape in one undo step. A slab: {boundary: [vertices]}. A path: {center: [vertices] (curves are allowed), width? (mm, default 1200), join? (miter|round|bevel), closed? (a ring without ends), align? (center|left|right: which edge of the path the line is), offset? (mm, moves the path to the left of the line; negative to the right)}. Also: style? (ID or name), slope? ({type: none} | {type: edges, elevations: [..]} (a slab: one per corner; a path: start and end) | {type: cross, percent, angle_deg?} | {type: angle, degrees, angle_deg?} | {type: rise_run, rise, run, angle_deg?}; angle_deg is the direction of the fall, a path falls to its right), elevation? (top surface above the layer; above the ground when draped), drape? (follow the site model on the layer), site_modifier? (act as a pad on the site model; the formation level is the top minus the build-up), material? and thickness? (when there is no style), layer?, class?. Returns {id}.",
);
pub const HARDSCAPE_SET: (&str, &str) = (
    "hardscape.set",
    "Change hardscapes. params: {id | ids, ...settings as in hardscape.add (center, width, join, style (null removes it), slope, elevation, drape, site_modifier, material, thickness, mode)}. Giving a center line makes a slab into a path; mode slab keeps the outline and makes a path into a slab.",
);
pub const LANDSCAPE_AREA_ADD: (&str, &str) = (
    "landscape_area.add",
    "Add a landscape area in one undo step. params: {boundary: [vertices], name?, style? (ID or name), holes? ([[vertices]], parts that stay empty and lie inside the boundary), exclude? ([object IDs] whose outline becomes a hole: rect, circle, closed polyline, hardscape, landscape area), show_plants?, drape?, layer?, class?}. Returns {id}.",
);
pub const LANDSCAPE_AREA_SET: (&str, &str) = (
    "landscape_area.set",
    "Change landscape areas. params: {id | ids, style? (null removes it), name?, holes?, exclude?, show_plants?, drape?}. Holes and exclude replace the old holes.",
);
pub const REPORT_HARDSCAPE: (&str, &str) = (
    "report.hardscape",
    "Hardscape quantities. params: {format?: csv}. Returns {rows: [{id, name, style, style_name, material, mode, plan_area_m2, surface_area_m2, perimeter_m, thickness_mm, volume_m3, draped, partially_draped (part of it lies outside the site model and follows the nearest edge height), layers: [{kind, name, material, thickness_mm, area_m2, volume_m3}]}], materials: [{kind, material, area_m2, volume_m3}], totals}. The surface area follows a slope or the site model; the volume of a layer is its thickness times the surface area.",
);
pub const REPORT_LANDSCAPE_AREAS: (&str, &str) = (
    "report.landscape_areas",
    "Landscape area quantities. params: {format?: csv}. Returns {rows: [{id, name, style, style_name, fields, holes, gross_area_m2, holes_area_m2, net_area_m2, surface_area_m2?, rows: [{plant, name, id_code, percent, density_per_m2, spacing_mm, count, covered_area_m2, total_price}]}], totals}. The count of a plant is the net area times its share times its density, at least min_count.",
);
pub const REPORT_PLANT_LIST: (&str, &str) = (
    "report.plant_list",
    "The plant list: single plants and the plants of landscape areas together, one row per plant style. params: {format?: csv}. Returns {rows: [{style, name, id_code, plants, in_areas, count, quantity, area_m2, unit_price, total_price, areas: [{area, name, count}]}], total_count, total_plants, total_in_areas, total_price}.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "hardscape": "A hardscape is a slab (closed boundary) or a path (center line with a width). Its style is a document resource with a surface material and thickness, build-up components, an optional border and a joint pattern. A path keeps its center line; the boundary of the object is the outline that follows from it.",
        "landscape_area": "A landscape area is a boundary with a name. Its style is a plant mix: each plant covers a share of the area at a density. Holes are excluded from the area. The plants show in reports and, with show_plants, as scattered symbols in 2D.",
        "examples": [
            {"method": "hardscape_style.create", "params": {"name": "Granite setts", "material": "Granite", "thickness": 60, "components": [{"name": "Bedding", "material": "Sand", "thickness": 40}, {"name": "Base", "material": "Crushed stone", "thickness": 150}], "border": {"width": 150, "material": "Kerb"}, "joints": {"kind": "herringbone", "unit_length": 200, "unit_width": 100, "spacing": 5}}},
            {"method": "hardscape.add", "params": {"center": [[0, 0], {"p": [6000, 0], "kind": "arc", "radius": 1500}, [6000, 5000]], "width": 1500, "join": "round", "style": "Granite setts", "drape": true}},
            {"method": "hardscape.set", "params": {"id": "00000000-0000-0000-0000-00000000000c", "slope": {"type": "cross", "percent": 2}}},
            {"method": "landscape_area_style.create", "params": {"name": "Meadow", "mix": [{"plant": "Lavender", "percent": 60, "density": 5}, {"plant": "Sedge", "percent": 40, "spacing": 400, "min_count": 10}], "show_plants": true, "fields": {"hydrozone": "low"}}},
            {"method": "landscape_area.add", "params": {"boundary": [[0, 0], [20000, 0], [20000, 10000], [0, 10000]], "name": "Bed A", "style": "Meadow", "holes": [[[2000, 2000], [4000, 2000], [4000, 4000]]]}},
            {"method": "report.hardscape", "params": {}},
            {"method": "report.landscape_areas", "params": {}},
            {"method": "report.plant_list", "params": {"format": "csv"}}
        ],
    })
}

fn required<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Value, RpcError> {
    m.get(key).ok_or_else(|| bad(format!("missing '{key}'")))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Hardscape,
    Landscape,
    Plant,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Hardscape => "hardscape style",
            Kind::Landscape => "landscape area style",
            Kind::Plant => "plant style",
        }
    }
}

/// A style reference: an ID, or a style name.
fn style_ref(doc: &Document, v: &Value, kind: Kind) -> Result<ResourceId, RpcError> {
    let id = match v {
        Value::String(s) => kadakas_core::uid::Uid::parse(s.trim())
            .map(ResourceId)
            .filter(|id| match kind {
                Kind::Hardscape => doc.resources.hardscape_style(*id).is_some(),
                Kind::Landscape => doc.resources.landscape_area_style(*id).is_some(),
                Kind::Plant => doc.resources.plant_style(*id).is_some(),
            })
            .or_else(|| match kind {
                Kind::Hardscape => doc.resources.find_hardscape_style(s),
                Kind::Landscape => doc.resources.find_landscape_area_style(s),
                Kind::Plant => doc.resources.find_plant_style(s),
            }),
        _ => None,
    };
    let ok = |id: ResourceId| match kind {
        Kind::Hardscape => doc.resources.hardscape_style(id).is_some(),
        Kind::Landscape => doc.resources.landscape_area_style(id).is_some(),
        Kind::Plant => doc.resources.plant_style(id).is_some(),
    };
    id.filter(|id| ok(*id))
        .ok_or_else(|| bad(format!("no {} {v}", kind.label())))
}

fn field_names<T: Serialize + Default>() -> Vec<String> {
    serde_json::to_value(T::default())
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

/// The style after applying the fields of `patch`. Fields you give replace the old ones.
fn patched<T: Serialize + DeserializeOwned + Default>(
    base: &T,
    patch: &Map<String, Value>,
    label: &str,
) -> Result<T, RpcError> {
    let keys = field_names::<T>();
    let mut v = serde_json::to_value(base).unwrap();
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

/// Replaces plant names in a mix with plant style IDs.
fn resolve_mix(doc: &Document, patch: &mut Map<String, Value>) -> Result<(), RpcError> {
    let Some(mix) = patch.get_mut("mix") else {
        return Ok(());
    };
    let list = mix
        .as_array_mut()
        .ok_or_else(|| bad("'mix' must be a list"))?;
    for e in list {
        let obj = e
            .as_object_mut()
            .ok_or_else(|| bad("each mix entry must be an object"))?;
        let plant = obj
            .get("plant")
            .ok_or_else(|| bad("a mix entry needs a 'plant'"))?;
        let id = style_ref(doc, plant, Kind::Plant)?;
        obj.insert("plant".into(), json!(id));
    }
    Ok(())
}

fn used_by_hardscape(doc: &Document, id: ResourceId) -> usize {
    doc.objects
        .values()
        .filter(|o| hardscape::style_id(o) == Some(id))
        .count()
}

fn used_by_landscape(doc: &Document, id: ResourceId) -> usize {
    doc.objects
        .values()
        .filter(|o| landscape::style_id(o) == Some(id))
        .count()
}

fn style_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("exists");
    let mut v = match &r.body {
        ResourceBody::HardscapeStyle(s) => {
            let mut v = serde_json::to_value(s).unwrap();
            v["total_thickness"] = json!(s.total_thickness());
            v["used_by"] = json!(used_by_hardscape(doc, id));
            v
        }
        ResourceBody::LandscapeAreaStyle(s) => {
            let mut v = serde_json::to_value(s).unwrap();
            v["used_by"] = json!(used_by_landscape(doc, id));
            let names: Vec<Value> = s
                .mix
                .iter()
                .map(|e| json!(doc.resources.get(e.plant).map(|r| r.name.clone())))
                .collect();
            v["mix_names"] = json!(names);
            v["total_percent"] = json!(s.total_percent());
            v
        }
        _ => json!({}),
    };
    v["id"] = json!(id);
    v["name"] = json!(r.name);
    v
}

fn unique_copy_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let first = format!("{base} copy");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{base} copy {n}"))
        .find(|n| !taken(n))
        .unwrap()
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    let known = matches!(
        method,
        "hardscape_style.list"
            | "hardscape_style.get"
            | "hardscape_style.create"
            | "hardscape_style.update"
            | "hardscape_style.duplicate"
            | "hardscape_style.delete"
            | "landscape_area_style.list"
            | "landscape_area_style.get"
            | "landscape_area_style.create"
            | "landscape_area_style.update"
            | "landscape_area_style.duplicate"
            | "landscape_area_style.delete"
            | "hardscape.add"
            | "hardscape.set"
            | "landscape_area.add"
            | "landscape_area.set"
            | "report.hardscape"
            | "report.landscape_areas"
            | "report.plant_list"
    );
    if !known {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(match p.as_object() {
        Some(m) => run(s, method, m),
        None => Err(bad("params must be an object")),
    })
}

fn style_method(
    s: &mut Session,
    kind: Kind,
    op: &str,
    m: &Map<String, Value>,
) -> Result<Value, RpcError> {
    let list = |doc: &Document| -> Vec<ResourceId> {
        match kind {
            Kind::Hardscape => doc
                .resources
                .hardscape_styles()
                .map(|(i, _, _)| i)
                .collect(),
            _ => doc
                .resources
                .landscape_area_styles()
                .map(|(i, _, _)| i)
                .collect(),
        }
    };
    let body = |kind: Kind,
                patch: &Map<String, Value>,
                base: Option<&ResourceBody>,
                doc: &Document|
     -> Result<ResourceBody, RpcError> {
        match kind {
            Kind::Hardscape => {
                let base = match base {
                    Some(ResourceBody::HardscapeStyle(s)) => (**s).clone(),
                    _ => HardscapeStyle::default(),
                };
                let st = patched(&base, patch, "hardscape style")?;
                st.validate().map_err(bad)?;
                Ok(ResourceBody::HardscapeStyle(Box::new(st)))
            }
            _ => {
                let base = match base {
                    Some(ResourceBody::LandscapeAreaStyle(s)) => (**s).clone(),
                    _ => LandscapeAreaStyle::default(),
                };
                let mut patch = patch.clone();
                resolve_mix(doc, &mut patch)?;
                let st = patched(&base, &patch, "landscape area style")?;
                st.validate().map_err(bad)?;
                Ok(ResourceBody::LandscapeAreaStyle(Box::new(st)))
            }
        }
    };
    match op {
        "list" => {
            let doc = s.editor.doc();
            Ok(
                json!({ "styles": list(doc).into_iter().map(|id| style_json(doc, id)).collect::<Vec<_>>() }),
            )
        }
        "get" => {
            let doc = s.editor.doc();
            Ok(style_json(doc, style_ref(doc, required(m, "id")?, kind)?))
        }
        "create" => {
            let name = required(m, "name")?
                .as_str()
                .ok_or_else(|| bad("'name' must be text"))?
                .to_string();
            let mut patch = m.clone();
            patch.remove("name");
            let b = body(kind, &patch, None, s.editor.doc())?;
            let out = s
                .editor
                .execute(&Command::AddResource { name, body: b })
                .map_err(RpcError::from)?;
            Ok(json!({ "id": out.created[0] }))
        }
        "update" => {
            let doc = s.editor.doc();
            let id = style_ref(doc, required(m, "id")?, kind)?;
            let mut patch = m.clone();
            patch.remove("id");
            let name = match patch.remove("name") {
                None => None,
                Some(Value::String(n)) => Some(n),
                Some(_) => return Err(bad("'name' must be text")),
            };
            let b = body(kind, &patch, doc.resources.get(id).map(|r| &r.body), doc)?;
            s.editor
                .execute(&Command::ModifyResource {
                    id,
                    name,
                    body: Some(b),
                })
                .map_err(RpcError::from)?;
            Ok(style_json(s.editor.doc(), id))
        }
        "duplicate" => {
            let doc = s.editor.doc();
            let id = style_ref(doc, required(m, "id")?, kind)?;
            let r = doc.resources.get(id).expect("checked");
            let name = match m.get("name") {
                None => unique_copy_name(&r.name, |n| match kind {
                    Kind::Hardscape => doc.resources.find_hardscape_style(n).is_some(),
                    _ => doc.resources.find_landscape_area_style(n).is_some(),
                }),
                Some(Value::String(n)) => n.clone(),
                Some(_) => return Err(bad("'name' must be text")),
            };
            let out = s
                .editor
                .execute(&Command::AddResource {
                    name,
                    body: r.body.clone(),
                })
                .map_err(RpcError::from)?;
            Ok(json!({ "id": out.created[0] }))
        }
        "delete" => {
            let doc = s.editor.doc();
            let id = style_ref(doc, required(m, "id")?, kind)?;
            let reassign = m
                .get("reassign")
                .filter(|v| !v.is_null())
                .map(|v| style_ref(doc, v, kind))
                .transpose()?;
            s.editor
                .execute(&Command::DeleteResource { id, reassign })
                .map_err(RpcError::from)?;
            s.prune_selection();
            Ok(json!({ "ok": true }))
        }
        _ => unreachable!("handle checks the method"),
    }
}

/// Takes the keys of a hardscape patch from the parameters. Unknown keys are an error.
fn hardscape_patch(
    doc: &Document,
    m: &Map<String, Value>,
    extra: &[&str],
) -> Result<HardscapePatch, RpcError> {
    let mut patch = HardscapePatch::default();
    for (k, v) in m {
        let null = v.is_null();
        match k.as_str() {
            "style" if null => patch.unstyle = true,
            "style" => patch.style = Some(style_ref(doc, v, Kind::Hardscape)?),
            "mode" => {
                patch.mode = Some(
                    Mode::from_name(v.as_str().unwrap_or(""))
                        .ok_or_else(|| bad("'mode' must be slab or path"))?,
                )
            }
            "center" => {
                patch.center = Some(serde_json::from_value::<Vec<Vertex>>(v.clone()).map_err(bad)?)
            }
            "width" => {
                patch.width = Some(v.as_f64().ok_or_else(|| bad("'width' must be a number"))?)
            }
            "closed" => {
                patch.closed = Some(
                    v.as_bool()
                        .ok_or_else(|| bad("'closed' must be true or false"))?,
                )
            }
            "align" => {
                patch.align = Some(
                    Align::from_name(v.as_str().unwrap_or(""))
                        .ok_or_else(|| bad("'align' must be center, left or right"))?,
                )
            }
            "offset" => {
                patch.offset = Some(v.as_f64().ok_or_else(|| bad("'offset' must be a number"))?)
            }
            "join" => {
                patch.join = Some(
                    Join::from_name(v.as_str().unwrap_or(""))
                        .ok_or_else(|| bad("'join' must be miter, round or bevel"))?,
                )
            }
            "slope" => {
                patch.slope = Some(if null {
                    Slope::None
                } else {
                    serde_json::from_value(v.clone()).map_err(bad)?
                })
            }
            "drape" => {
                patch.drape = Some(
                    v.as_bool()
                        .ok_or_else(|| bad("'drape' must be true or false"))?,
                )
            }
            "site_modifier" => {
                patch.site_modifier = Some(
                    v.as_bool()
                        .ok_or_else(|| bad("'site_modifier' must be true or false"))?,
                )
            }
            "material" => {
                patch.material = Some(
                    v.as_str()
                        .ok_or_else(|| bad("'material' must be text"))?
                        .to_string(),
                )
            }
            "thickness" => {
                patch.thickness = Some(
                    v.as_f64()
                        .ok_or_else(|| bad("'thickness' must be a number"))?,
                )
            }
            "elevation" => {
                patch.elevation = Some(
                    v.as_f64()
                        .ok_or_else(|| bad("'elevation' must be a number"))?,
                )
            }
            other if extra.contains(&other) => {}
            other => return Err(bad(format!("unknown field '{other}'"))),
        }
    }
    Ok(patch)
}

fn ids_param(m: &Map<String, Value>) -> Result<Vec<ObjectId>, RpcError> {
    match (m.get("ids"), m.get("id")) {
        (Some(v), _) => serde_json::from_value(v.clone()).map_err(bad),
        (None, Some(v)) => Ok(vec![serde_json::from_value(v.clone()).map_err(bad)?]),
        _ => Err(bad("give 'id' or 'ids'")),
    }
}

fn opt_id(m: &Map<String, Value>, key: &str) -> Result<Option<kadakas_core::uid::Uid>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_uid()
            .map(Some)
            .ok_or_else(|| bad(format!("'{key}' must be an ID"))),
    }
}

/// The outline of an object as a hole.
fn outline_of(doc: &Document, id: ObjectId) -> Result<Vec<Vertex>, RpcError> {
    let o = doc
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    Ok(match &o.shape {
        Shape::Polyline {
            points,
            closed: true,
        } => points.clone(),
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            boundary.clone()
        }
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            curve::corners(&[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)])
        }
        Shape::Circle { center, radius } => curve::corners(
            &(0..64)
                .map(|k| {
                    *center + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 64.0) * *radius
                })
                .collect::<Vec<_>>(),
        ),
        other => return Err(bad(format!("a {} cannot be a hole", other.kind()))),
    })
}

fn landscape_patch(
    doc: &Document,
    m: &Map<String, Value>,
    extra: &[&str],
) -> Result<LandscapePatch, RpcError> {
    let mut patch = LandscapePatch::default();
    let mut holes: Option<Vec<Vec<Vertex>>> = None;
    for (k, v) in m {
        match k.as_str() {
            "style" if v.is_null() => patch.unstyle = true,
            "style" => patch.style = Some(style_ref(doc, v, Kind::Landscape)?),
            "name" => {
                patch.name = Some(
                    v.as_str()
                        .ok_or_else(|| bad("'name' must be text"))?
                        .to_string(),
                )
            }
            "show_plants" => {
                patch.show_plants = Some(
                    v.as_bool()
                        .ok_or_else(|| bad("'show_plants' must be true or false"))?,
                )
            }
            "drape" => {
                patch.drape = Some(
                    v.as_bool()
                        .ok_or_else(|| bad("'drape' must be true or false"))?,
                )
            }
            "holes" => holes
                .get_or_insert_with(Vec::new)
                .extend(serde_json::from_value::<Vec<Vec<Vertex>>>(v.clone()).map_err(bad)?),
            "exclude" => {
                let ids: Vec<ObjectId> = serde_json::from_value(v.clone()).map_err(bad)?;
                for id in ids {
                    holes
                        .get_or_insert_with(Vec::new)
                        .push(outline_of(doc, id)?);
                }
            }
            other if extra.contains(&other) => {}
            other => return Err(bad(format!("unknown field '{other}'"))),
        }
    }
    patch.holes = holes;
    Ok(patch)
}

fn csv_of(rows: Vec<Vec<String>>) -> String {
    write_csv(&rows)
}

fn run(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if let Some(op) = method.strip_prefix("hardscape_style.") {
        return style_method(s, Kind::Hardscape, op, m);
    }
    if let Some(op) = method.strip_prefix("landscape_area_style.") {
        return style_method(s, Kind::Landscape, op, m);
    }
    let csv = m.get("format").and_then(Value::as_str) == Some("csv");
    match method {
        "hardscape.add" => {
            let doc = s.editor.doc();
            let patch = hardscape_patch(doc, m, &["boundary", "layer", "class"])?;
            let boundary = match m.get("boundary") {
                None | Some(Value::Null) => None,
                Some(v) => Some(serde_json::from_value::<Vec<Vertex>>(v.clone()).map_err(bad)?),
            };
            if patch.mode == Some(Mode::Path) && patch.center.is_none() {
                return Err(bad("a path needs a 'center' line"));
            }
            if patch.mode == Some(Mode::Slab) && boundary.is_none() {
                return Err(bad("a slab needs a 'boundary'"));
            }
            let (layer, class) = (
                opt_id(m, "layer")?.map(LayerId),
                opt_id(m, "class")?.map(ClassId),
            );
            let out = s
                .editor
                .execute(&Command::AddHardscape {
                    boundary,
                    patch: Box::new(patch),
                    layer,
                    class,
                    attrs: Attributes::default(),
                })
                .map_err(RpcError::from)?;
            Ok(json!({ "id": out.created[0] }))
        }
        "hardscape.set" => {
            let ids = ids_param(m)?;
            let patch = hardscape_patch(s.editor.doc(), m, &["id", "ids"])?;
            s.editor
                .execute(&Command::SetHardscape {
                    ids: ids.clone(),
                    patch,
                })
                .map_err(RpcError::from)?;
            Ok(json!({ "ids": ids }))
        }
        "landscape_area.add" => {
            let doc = s.editor.doc();
            let patch = landscape_patch(doc, m, &["boundary", "layer", "class"])?;
            let boundary: Vec<Vertex> =
                serde_json::from_value(required(m, "boundary")?.clone()).map_err(bad)?;
            let name = patch.name.clone().unwrap_or_default();
            let patch = LandscapePatch {
                name: None,
                ..patch
            };
            let (layer, class) = (
                opt_id(m, "layer")?.map(LayerId),
                opt_id(m, "class")?.map(ClassId),
            );
            let out = s
                .editor
                .execute(&Command::AddLandscapeArea {
                    boundary,
                    name,
                    patch: Box::new(patch),
                    layer,
                    class,
                    attrs: Attributes::default(),
                })
                .map_err(RpcError::from)?;
            Ok(json!({ "id": out.created[0] }))
        }
        "landscape_area.set" => {
            let ids = ids_param(m)?;
            let patch = landscape_patch(s.editor.doc(), m, &["id", "ids"])?;
            s.editor
                .execute(&Command::SetLandscapeArea {
                    ids: ids.clone(),
                    patch,
                })
                .map_err(RpcError::from)?;
            Ok(json!({ "ids": ids }))
        }
        "report.hardscape" => {
            let terrain = SiteTerrain::new(s.site_cache(), s.editor.revision(), s.editor.doc());
            let r = hardscape::report::report(s.editor.doc(), &terrain);
            let mut v = serde_json::to_value(&r).unwrap();
            v["totals"] = json!({ "plan_area_m2": r.total_plan_area_m2, "surface_area_m2": r.total_surface_area_m2, "volume_m3": r.total_volume_m3 });
            if csv {
                let mut rows = vec![
                    [
                        "Id",
                        "Name",
                        "Style",
                        "Mode",
                        "Plan area (m2)",
                        "Surface area (m2)",
                        "Perimeter (m)",
                        "Thickness (mm)",
                        "Volume (m3)",
                    ]
                    .map(String::from)
                    .to_vec(),
                ];
                for x in &r.rows {
                    let q = &x.quantities;
                    rows.push(vec![
                        x.id.to_string(),
                        x.name.clone(),
                        x.style_name.clone().unwrap_or_default(),
                        q.mode.into(),
                        format!("{:.3}", q.plan_area_m2),
                        format!("{:.3}", q.surface_area_m2),
                        format!("{:.3}", q.perimeter_m),
                        format!("{}", q.thickness_mm),
                        format!("{:.3}", q.volume_m3),
                    ]);
                }
                v["csv"] = json!(csv_of(rows));
            }
            Ok(v)
        }
        "report.landscape_areas" => {
            let terrain = SiteTerrain::new(s.site_cache(), s.editor.revision(), s.editor.doc());
            let r = landscape::report(s.editor.doc(), &terrain);
            let mut v = serde_json::to_value(&r).unwrap();
            v["totals"] = json!({ "gross_area_m2": r.total_gross_area_m2, "net_area_m2": r.total_net_area_m2, "count": r.total_count, "price": r.total_price });
            if csv {
                let mut rows = vec![
                    [
                        "Area",
                        "Name",
                        "Style",
                        "Plant",
                        "ID",
                        "Percent",
                        "Density (per m2)",
                        "Count",
                        "Covered (m2)",
                        "Total price",
                    ]
                    .map(String::from)
                    .to_vec(),
                ];
                for a in &r.rows {
                    for p in &a.quantities.rows {
                        rows.push(vec![
                            a.id.to_string(),
                            a.name.clone(),
                            a.style_name.clone().unwrap_or_default(),
                            p.name.clone(),
                            p.id_code.clone(),
                            format!("{}", p.percent),
                            format!("{:.2}", p.density_per_m2),
                            p.count.to_string(),
                            format!("{:.2}", p.covered_area_m2),
                            format!("{:.2}", p.total_price),
                        ]);
                    }
                }
                v["csv"] = json!(csv_of(rows));
            }
            Ok(v)
        }
        "report.plant_list" => {
            let list = kadakas_plants::plant_list::plant_list(s.editor.doc());
            let mut v = serde_json::to_value(&list).unwrap();
            if csv {
                v["csv"] = json!(kadakas_plants::plant_list::plant_list_csv(&list));
            }
            Ok(v)
        }
        _ => unreachable!("handle checks the method"),
    }
}

#[cfg(test)]
mod tests;
