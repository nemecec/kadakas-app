//! Agent API for 3D presentation: materials, render styles and the sun.
//!
//! Methods: `material.list|get|create|update|duplicate|delete|add_defaults|import_texture|assign|slots`,
//! `render_style.list|get|create|update|duplicate|delete|set_active`, `sun.set|get`,
//! `render.hidden_line`. `render.png` takes a `render_style` (see `lib.rs`).

use cadektor_core::uid::AsUid;
use serde_json::{Map, Value, json};

use cadektor_core::command::Command;
use cadektor_core::model::*;
use cadektor_core::present::{
    self, MappingKind, Material, Procedural, RenderMode, RenderResource, RenderStyle, SlotTarget,
    SunSettings, Texture, TextureMapping,
};
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::sun;

use crate::errors::{app, bad};
use crate::{RpcError, Session};

pub const MATERIAL_LIST: (&str, &str) = (
    "material.list",
    "Materials of the document with the number of slots that use each. Returns {materials: [{id, name, base_color, texture, mapping, roughness, metallic, opacity, used_by}]}. Image data is not returned.",
);
pub const MATERIAL_GET: (&str, &str) = (
    "material.get",
    "One material. params: {id} (the ID or the name).",
);
pub const MATERIAL_CREATE: (&str, &str) = (
    "material.create",
    "Create a material resource. params: {name, base_color? ([r,g,b] or [r,g,b,a]), texture? (null | \"grass\" | {procedural: grass|gravel|pavers|mulch|water|brick|timber|noise, seed?} | {png_base64} | {jpeg_base64} | {path} of a PNG or JPEG file, max_side? (default 1024)), mapping? ({kind: planar|box|cylindrical, scale: mm of one repeat as a number or [u, v], rotation: degrees, offset: [x, y] mm}), roughness? (0 to 1), metallic? (0 to 1), opacity? (0 to 1; below 1 is see-through, for water and glass)}. An image is stored in the document, scaled to at most 2048 px per side and 3 MB. Returns {id}.",
);
pub const MATERIAL_UPDATE: (&str, &str) = (
    "material.update",
    "Change a material. params: {id, name? (renames), ...fields as in material.create}. Objects that use the material follow.",
);
pub const MATERIAL_DUPLICATE: (&str, &str) = (
    "material.duplicate",
    "Copy a material. params: {id, name?}. Returns {id}.",
);
pub const MATERIAL_DELETE: (&str, &str) = (
    "material.delete",
    "Delete a material. Its slots are cleared. params: {id}.",
);
pub const MATERIAL_ADD_DEFAULTS: (&str, &str) = (
    "material.add_defaults",
    "Add the built-in materials that the document lacks: Grass, Gravel, Concrete pavers, Bark mulch, Water, Brick, Timber (generated textures, no image files). Returns {added: [{id, name}], existing: [name]}.",
);
pub const MATERIAL_IMPORT_TEXTURE: (&str, &str) = (
    "material.import_texture",
    "Put a PNG or JPEG image into a material as its texture. params: {id, path | png_base64 | jpeg_base64, max_side? (default 1024, at most 2048)}. Returns {width, height, bytes}.",
);
pub const MATERIAL_ASSIGN: (&str, &str) = (
    "material.assign",
    "Put a material on a slot, or clear it with material null. params: {material, and one target: object | objects ([ids]) with slot? (a part, like trunk or canopy for plants, surface or component:0 for hardscapes), or style (plant style or hardscape style, ID or name) with slot (trunk | canopy | surface | component:N), or class (ID)}. The material of a part comes from the part of the object, the whole object, the part of its style, then its class (in that order). Returns {assigned}.",
);
pub const MATERIAL_SLOTS: (&str, &str) = (
    "material.slots",
    "All material assignments. Returns {slots: [{target: {kind, ...}, material, material_name}]}.",
);
pub const RENDER_STYLE_LIST: (&str, &str) = (
    "render_style.list",
    "Render styles of the document, the built-in ones, and the active one. Returns {styles: [{id, name, ...fields}], builtin: [{name, ...fields}], active}.",
);
pub const RENDER_STYLE_GET: (&str, &str) = (
    "render_style.get",
    "One render style. params: {id} (the ID or the name).",
);
pub const RENDER_STYLE_CREATE: (&str, &str) = (
    "render_style.create",
    "Create a render style resource. params: {name, mode? (shaded | wireframe | hidden_line | dashed_hidden_line), edges? (draw edges on shaded surfaces), shadows? (needs sun), sun? (light with the sun of the document, see sun.set), textures?, background? ([r,g,b]), line_weight? (mm on paper, for hidden line)}. Returns {id}. Use it with render.png {render_style}, render_style.set_active, or in a viewport.",
);
pub const RENDER_STYLE_UPDATE: (&str, &str) = (
    "render_style.update",
    "Change a render style. params: {id, name? (renames), ...fields as in render_style.create}.",
);
pub const RENDER_STYLE_DUPLICATE: (&str, &str) = (
    "render_style.duplicate",
    "Copy a render style. params: {id, name?}. Returns {id}.",
);
pub const RENDER_STYLE_DELETE: (&str, &str) = (
    "render_style.delete",
    "Delete a render style. params: {id}. Viewports that used it fall back to their mode.",
);
pub const RENDER_STYLE_SET_ACTIVE: (&str, &str) = (
    "render_style.set_active",
    "Choose the render style of the 3D view and of render.png. params: {id | null} (a document style, or null for the built-in shaded style).",
);
pub const SUN_SET: (&str, &str) = (
    "sun.set",
    "Set the sun of the document (like a heliodon). Fields you give replace the old ones. params: {latitude? (degrees north), longitude? (degrees east), from_georef? (take latitude and longitude from the georeference), date? (\"YYYY-MM-DD\"), time? (\"HH:MM\" local clock time), tz_offset? (standard time, hours from UTC; default: the nominal zone of the longitude), dst? (true when summer time is in force: the clock is one hour ahead of tz_offset), north_angle? (degrees clockwise from the model +Y axis to true north; from_georef uses the rotation of the georeference and the meridian convergence of its coordinate system), off? (true removes the sun)}. Render styles with sun on use it; with shadows on, objects cast shadows. Returns the settings and the sun position.",
);
pub const SUN_GET: (&str, &str) = (
    "sun.get",
    "The sun of the document and its position. params: {times? (list of \"HH:MM\" for a sun path on the same date)}. Returns {sun, position: {azimuth, elevation, declination, equation_of_time, above_horizon}, day: {sunrise, solar_noon, sunset, always_up}, path?}. Azimuth is degrees clockwise from true north.",
);
pub const RENDER_HIDDEN_LINE: (&str, &str) = (
    "render.hidden_line",
    "Vector drawing of the visible edges of the 3D model, with the covered edges removed. params: {view?, projection?, width?, height? (px, as render.png), dashed? (also return the covered edges, dashed and gray), mesh_edges? (add silhouettes and creases of the surfaces; default true), segments? (also return the segments)}. Returns {svg, visible_edges, width, height, view, projection, segments?: [{a: [x, y], b: [x, y], hidden, color, object}]}. The sheet viewport mode hidden_line uses the same algorithm.",
);

const KNOWN: &[&str] = &[
    "material.list",
    "material.get",
    "material.create",
    "material.update",
    "material.duplicate",
    "material.delete",
    "material.add_defaults",
    "material.import_texture",
    "material.assign",
    "material.slots",
    "render_style.list",
    "render_style.get",
    "render_style.create",
    "render_style.update",
    "render_style.duplicate",
    "render_style.delete",
    "render_style.set_active",
    "sun.set",
    "sun.get",
    "render.hidden_line",
];

pub fn describe() -> Value {
    json!({
        "material": "A material combines a base color, an optional texture and physical data (roughness, metallic, opacity). Textures are generated patterns or PNG/JPEG images stored in the document. Put a material on an object, a part of an object (trunk and canopy of a plant, surface and layers of a hardscape), a style, or a class with material.assign.",
        "render_style": "A render style says how a view draws the model: mode (shaded, wireframe, hidden_line, dashed_hidden_line), edges, shadows, sun on or off, textures, background. Built-in styles: Wireframe, Shaded, Realistic, Hidden Line, Dashed Hidden Line.",
        "sun": "The sun comes from latitude, longitude, date, time and time zone (NOAA solar position). It lights the model in styles with sun on, and casts shadows with shadows on.",
        "examples": [
            {"method": "material.add_defaults", "params": {}},
            {"method": "material.create", "params": {"name": "Teak", "base_color": [150, 100, 60], "texture": {"procedural": "timber"}, "mapping": {"kind": "box", "scale": [1200, 300], "rotation": 0}, "roughness": 0.6}},
            {"method": "material.import_texture", "params": {"id": "Teak", "path": "/tmp/teak.jpg", "max_side": 1024}},
            {"method": "material.assign", "params": {"material": "Grass", "class": "00000000-0000-0000-0000-000000000000"}},
            {"method": "material.assign", "params": {"material": "Bark mulch", "style": "Oak", "slot": "trunk"}},
            {"method": "sun.set", "params": {"latitude": 60.17, "longitude": 24.94, "date": "2026-06-21", "time": "15:30", "tz_offset": 3}},
            {"method": "render_style.create", "params": {"name": "Afternoon", "mode": "shaded", "sun": true, "shadows": true}},
            {"method": "render.png", "params": {"view": "right_isometric", "render_style": "Afternoon", "width": 1024, "height": 768}},
            {"method": "render.hidden_line", "params": {"view": "right_isometric", "dashed": true}}
        ],
    })
}

/// True for the methods of this module.
pub fn knows(method: &str) -> bool {
    KNOWN.contains(&method)
}

fn required<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Value, RpcError> {
    m.get(key).ok_or_else(|| bad(format!("missing '{key}'")))
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !knows(method) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(match p.as_object() {
        Some(m) => run(s, method, m),
        None => Err(bad("params must be an object")),
    })
}

// --- Lookups ----------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Material,
    Style,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Material => "material",
            Kind::Style => "render style",
        }
    }
}

fn ref_of(doc: &Document, v: &Value, kind: Kind) -> Result<ResourceId, RpcError> {
    let ok = |id: ResourceId| match kind {
        Kind::Material => present::material(doc, id).is_some(),
        Kind::Style => present::render_style(doc, id).is_some(),
    };
    let found = match v {
        Value::String(name) => cadektor_core::uid::Uid::parse(name.trim())
            .map(ResourceId)
            .filter(|id| ok(*id))
            .or_else(|| {
                doc.resources
                    .iter()
                    .find(|r| ok(r.id) && r.name.trim().eq_ignore_ascii_case(name.trim()))
                    .map(|r| r.id)
            }),
        _ => None,
    };
    found
        .filter(|id| ok(*id))
        .ok_or_else(|| bad(format!("no {} {v}", kind.label())))
}

fn color_json(c: Color) -> Value {
    json!(c.0)
}

fn color_of(v: &Value, what: &str) -> Result<Color, RpcError> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 3 || a.len() == 4)
        .ok_or_else(|| bad(format!("'{what}' must be [r, g, b] or [r, g, b, a]")))?;
    let mut out = [0u8, 0, 0, 255];
    for (i, x) in a.iter().enumerate() {
        out[i] = x.as_u64().filter(|n| *n <= 255).ok_or_else(|| {
            bad(format!(
                "'{what}' values must be whole numbers from 0 to 255"
            ))
        })? as u8;
    }
    Ok(Color(out))
}

fn number(v: &Value, what: &str) -> Result<f64, RpcError> {
    v.as_f64()
        .filter(|x| x.is_finite())
        .ok_or_else(|| bad(format!("'{what}' must be a number")))
}

fn unit_number(v: &Value, what: &str) -> Result<f32, RpcError> {
    let x = number(v, what)?;
    if !(0.0..=1.0).contains(&x) {
        return Err(bad(format!("'{what}' must be from 0 to 1")));
    }
    Ok(x as f32)
}

fn flag(v: &Value, what: &str) -> Result<bool, RpcError> {
    v.as_bool()
        .ok_or_else(|| bad(format!("'{what}' must be true or false")))
}

// --- Materials --------------------------------------------------------------------------

fn texture_json(t: &Option<Texture>) -> Value {
    match t {
        None => Value::Null,
        Some(Texture::Procedural { kind, seed }) => {
            json!({"source": "procedural", "kind": kind.id(), "seed": seed})
        }
        Some(Texture::Image {
            png_base64,
            width,
            height,
        }) => {
            json!({"source": "image", "width": width, "height": height, "bytes": png_base64.len() / 4 * 3})
        }
    }
}

fn mapping_json(m: &TextureMapping) -> Value {
    json!({"kind": m.kind.id(), "scale": m.scale, "rotation": m.rotation, "offset": m.offset})
}

fn slot_count(doc: &Document, id: ResourceId) -> usize {
    doc.drawing
        .presentation
        .slots
        .iter()
        .filter(|e| e.material == id)
        .count()
}

fn material_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("checked");
    let m = present::material(doc, id).expect("checked");
    json!({
        "id": id,
        "name": r.name,
        "base_color": color_json(m.base_color),
        "texture": texture_json(&m.texture),
        "mapping": mapping_json(&m.mapping),
        "roughness": m.roughness,
        "metallic": m.metallic,
        "opacity": m.opacity,
        "used_by": slot_count(doc, id),
    })
}

/// A texture from the `texture` parameter.
fn texture_param(s: &Session, v: &Value, max_side: u32) -> Result<Option<Texture>, RpcError> {
    let procedural = |name: &str, seed: u32| {
        Procedural::from_name(name)
            .map(|kind| Texture::Procedural { kind, seed })
            .ok_or_else(|| {
                bad(format!(
                    "unknown pattern '{name}'. Patterns: {}",
                    Procedural::ALL.map(|p| p.id()).join(", ")
                ))
            })
    };
    match v {
        Value::Null => Ok(None),
        Value::String(name) => procedural(name, 1).map(Some),
        Value::Object(o) => {
            for k in o.keys() {
                if ![
                    "procedural",
                    "seed",
                    "png_base64",
                    "jpeg_base64",
                    "path",
                    "max_side",
                ]
                .contains(&k.as_str())
                {
                    return Err(bad(format!(
                        "unknown texture key '{k}'. Use procedural (+ seed), png_base64, jpeg_base64 or path"
                    )));
                }
            }
            let max_side = match o.get("max_side") {
                Some(x) => x
                    .as_u64()
                    .filter(|n| (1..=2048).contains(n))
                    .ok_or_else(|| bad("'max_side' must be from 1 to 2048"))?
                    as u32,
                None => max_side,
            };
            if let Some(name) = o.get("procedural") {
                let seed = match o.get("seed") {
                    Some(x) => x
                        .as_u64()
                        .filter(|n| *n <= u32::MAX as u64)
                        .ok_or_else(|| bad("'seed' must be a whole number"))?
                        as u32,
                    None => 1,
                };
                return procedural(
                    name.as_str()
                        .ok_or_else(|| bad("'procedural' must be a pattern name"))?,
                    seed,
                )
                .map(Some);
            }
            let bytes = if let Some(b) = o.get("png_base64").or_else(|| o.get("jpeg_base64")) {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(
                        b.as_str()
                            .ok_or_else(|| bad("the image must be base64 text"))?
                            .trim(),
                    )
                    .map_err(|e| bad(format!("the image is not valid base64: {e}")))?
            } else if let Some(path) = o.get("path") {
                read_image_file(s, path.as_str().ok_or_else(|| bad("'path' must be text"))?)?
            } else {
                return Err(bad("give procedural, png_base64, jpeg_base64 or path"));
            };
            image_texture(&bytes, max_side).map(Some)
        }
        _ => Err(bad("'texture' must be null, a pattern name or an object")),
    }
}

fn read_image_file(s: &Session, path: &str) -> Result<Vec<u8>, RpcError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let p = s
            .check_path(std::path::Path::new(path), false, &["png", "jpg", "jpeg"])
            .map_err(RpcError::policy)?;
        std::fs::read(&p).map_err(|e| app(format!("cannot read {}: {e}", p.display())))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path);
        Err(app(
            "reading files is not available here. Use png_base64 or jpeg_base64",
        ))
    }
}

fn image_texture(bytes: &[u8], max_side: u32) -> Result<Texture, RpcError> {
    let img = cadektor_render::texture::decode_image_bytes(bytes).map_err(app)?;
    cadektor_render::texture::to_texture(&img, max_side).map_err(app)
}

fn mapping_param(base: TextureMapping, v: &Value) -> Result<TextureMapping, RpcError> {
    let o = v
        .as_object()
        .ok_or_else(|| bad("'mapping' must be an object"))?;
    let mut m = base;
    for (k, x) in o {
        match k.as_str() {
            "kind" => {
                m.kind = MappingKind::from_name(x.as_str().unwrap_or(""))
                    .ok_or_else(|| bad("mapping kind must be planar, box or cylindrical"))?
            }
            "scale" => {
                m.scale = match x {
                    Value::Array(a) if a.len() == 2 => {
                        [number(&a[0], "scale")?, number(&a[1], "scale")?]
                    }
                    _ => {
                        let n = number(x, "scale")?;
                        [n, n]
                    }
                }
            }
            "rotation" => m.rotation = number(x, "rotation")?,
            "offset" => {
                let a = x
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or_else(|| bad("'offset' must be [x, y]"))?;
                m.offset = [number(&a[0], "offset")?, number(&a[1], "offset")?];
            }
            _ => {
                return Err(bad(format!(
                    "unknown mapping key '{k}'. Keys: kind, scale, rotation, offset"
                )));
            }
        }
    }
    Ok(m)
}

fn material_body(
    s: &Session,
    base: Material,
    patch: &Map<String, Value>,
) -> Result<ResourceBody, RpcError> {
    let mut m = base;
    for (k, v) in patch {
        match k.as_str() {
            "base_color" => m.base_color = color_of(v, "base_color")?,
            "texture" => m.texture = texture_param(s, v, 1024)?,
            "mapping" => m.mapping = mapping_param(m.mapping, v)?,
            "roughness" => m.roughness = unit_number(v, "roughness")?,
            "metallic" => m.metallic = unit_number(v, "metallic")?,
            "opacity" => m.opacity = unit_number(v, "opacity")?,
            _ => {
                return Err(bad(format!(
                    "unknown material field '{k}'. Fields: base_color, texture, mapping, roughness, metallic, opacity"
                )));
            }
        }
    }
    m.validate().map_err(bad)?;
    Ok(ResourceBody::Render(Box::new(RenderResource::Material(m))))
}

fn name_param(m: &Map<String, Value>) -> Result<Option<String>, RpcError> {
    match m.get("name") {
        None => Ok(None),
        Some(Value::String(n)) => Ok(Some(n.clone())),
        Some(_) => Err(bad("'name' must be text")),
    }
}

fn copy_name(doc: &Document, base: &str) -> String {
    let taken = |n: &str| doc.resources.iter().any(|r| r.name.eq_ignore_ascii_case(n));
    let first = format!("{base} copy");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{base} copy {n}"))
        .find(|n| !taken(n))
        .expect("endless range")
}

fn run(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    match method {
        "material.list" => {
            let doc = s.editor.doc();
            let ids: Vec<_> = doc
                .resources
                .iter()
                .filter(|r| present::material(doc, r.id).is_some())
                .map(|r| r.id)
                .collect();
            Ok(
                json!({"materials": ids.into_iter().map(|id| material_json(doc, id)).collect::<Vec<_>>()}),
            )
        }
        "material.get" => {
            let doc = s.editor.doc();
            let id = ref_of(doc, required(m, "id")?, Kind::Material)?;
            Ok(material_json(doc, id))
        }
        "material.create" => {
            let name = name_param(m)?.ok_or_else(|| bad("missing 'name'"))?;
            let mut patch = m.clone();
            patch.remove("name");
            let body = material_body(s, Material::default(), &patch)?;
            let out = s
                .editor
                .execute(&Command::AddResource { name, body })
                .map_err(RpcError::from)?;
            Ok(json!({"id": out.created[0]}))
        }
        "material.update" => {
            let doc = s.editor.doc();
            let id = ref_of(doc, required(m, "id")?, Kind::Material)?;
            let mut patch = m.clone();
            patch.remove("id");
            let name = name_param(&patch)?;
            patch.remove("name");
            let base = present::material(doc, id).expect("checked").clone();
            let body = material_body(s, base, &patch)?;
            s.editor
                .execute(&Command::ModifyResource {
                    id,
                    name,
                    body: Some(body),
                })
                .map_err(RpcError::from)?;
            Ok(material_json(s.editor.doc(), id))
        }
        "material.duplicate" | "render_style.duplicate" => {
            let kind = if method.starts_with("material") {
                Kind::Material
            } else {
                Kind::Style
            };
            let doc = s.editor.doc();
            let id = ref_of(doc, required(m, "id")?, kind)?;
            let r = doc.resources.get(id).expect("checked");
            let name = name_param(m)?.unwrap_or_else(|| copy_name(doc, &r.name));
            let out = s
                .editor
                .execute(&Command::AddResource {
                    name,
                    body: r.body.clone(),
                })
                .map_err(RpcError::from)?;
            Ok(json!({"id": out.created[0]}))
        }
        "material.delete" | "render_style.delete" => {
            let kind = if method.starts_with("material") {
                Kind::Material
            } else {
                Kind::Style
            };
            let id = ref_of(s.editor.doc(), required(m, "id")?, kind)?;
            s.editor
                .execute(&Command::DeleteResource { id, reassign: None })
                .map_err(RpcError::from)?;
            Ok(json!({"ok": true}))
        }
        "material.add_defaults" => {
            let doc = s.editor.doc();
            let mut commands = Vec::new();
            let mut existing = Vec::new();
            for (name, mat) in present::default_library() {
                if doc.resources.iter().any(|r| {
                    r.name.eq_ignore_ascii_case(name) && present::material(doc, r.id).is_some()
                }) {
                    existing.push(name);
                } else {
                    commands.push((
                        name,
                        Command::AddResource {
                            name: name.into(),
                            body: ResourceBody::Render(Box::new(RenderResource::Material(mat))),
                        },
                    ));
                }
            }
            let mut added = Vec::new();
            if !commands.is_empty() {
                let names: Vec<&str> = commands.iter().map(|c| c.0).collect();
                let out = s
                    .editor
                    .execute(&Command::Batch {
                        commands: commands.into_iter().map(|c| c.1).collect(),
                    })
                    .map_err(RpcError::from)?;
                added = names
                    .into_iter()
                    .zip(out.created)
                    .map(|(n, id)| json!({"id": id, "name": n}))
                    .collect();
            }
            Ok(json!({"added": added, "existing": existing}))
        }
        "material.import_texture" => {
            let doc = s.editor.doc();
            let id = ref_of(doc, required(m, "id")?, Kind::Material)?;
            let max_side = match m.get("max_side") {
                Some(x) => x
                    .as_u64()
                    .filter(|n| (1..=2048).contains(n))
                    .ok_or_else(|| bad("'max_side' must be from 1 to 2048"))?
                    as u32,
                None => 1024,
            };
            let mut spec = Map::new();
            for k in ["path", "png_base64", "jpeg_base64"] {
                if let Some(v) = m.get(k) {
                    spec.insert(k.into(), v.clone());
                }
            }
            if spec.is_empty() {
                return Err(bad("give path, png_base64 or jpeg_base64"));
            }
            spec.insert("max_side".into(), json!(max_side));
            let texture = texture_param(s, &Value::Object(spec), max_side)?;
            let mut mat = present::material(doc, id).expect("checked").clone();
            mat.texture = texture;
            let info = texture_json(&mat.texture);
            mat.validate().map_err(bad)?;
            s.editor
                .execute(&Command::ModifyResource {
                    id,
                    name: None,
                    body: Some(ResourceBody::Render(Box::new(RenderResource::Material(
                        mat,
                    )))),
                })
                .map_err(RpcError::from)?;
            Ok(info)
        }
        "material.assign" => assign(s, m),
        "material.slots" => {
            let doc = s.editor.doc();
            let slots: Vec<Value> = doc
                .drawing
                .presentation
                .slots
                .iter()
                .map(|e| {
                    json!({
                        "target": e.target,
                        "material": e.material,
                        "material_name": doc.resources.get(e.material).map(|r| r.name.clone()),
                    })
                })
                .collect();
            Ok(json!({"slots": slots}))
        }
        "render_style.list" => {
            let doc = s.editor.doc();
            let styles: Vec<Value> = doc
                .resources
                .iter()
                .filter(|r| present::render_style(doc, r.id).is_some())
                .map(|r| style_json(doc, r.id))
                .collect();
            let builtin: Vec<Value> = RenderStyle::presets()
                .into_iter()
                .map(|(n, st)| style_fields(Some(n), &st))
                .collect();
            Ok(json!({
                "styles": styles,
                "builtin": builtin,
                "active": doc.drawing.presentation.active_style,
                "modes": RenderMode::ALL.map(|m| m.id()),
            }))
        }
        "render_style.get" => {
            let doc = s.editor.doc();
            let id = ref_of(doc, required(m, "id")?, Kind::Style)?;
            Ok(style_json(doc, id))
        }
        "render_style.create" => {
            let name = name_param(m)?.ok_or_else(|| bad("missing 'name'"))?;
            let mut patch = m.clone();
            patch.remove("name");
            let body = style_body(RenderStyle::default(), &patch)?;
            let out = s
                .editor
                .execute(&Command::AddResource { name, body })
                .map_err(RpcError::from)?;
            Ok(json!({"id": out.created[0]}))
        }
        "render_style.update" => {
            let doc = s.editor.doc();
            let id = ref_of(doc, required(m, "id")?, Kind::Style)?;
            let mut patch = m.clone();
            patch.remove("id");
            let name = name_param(&patch)?;
            patch.remove("name");
            let base = present::render_style(doc, id).expect("checked").clone();
            let body = style_body(base, &patch)?;
            s.editor
                .execute(&Command::ModifyResource {
                    id,
                    name,
                    body: Some(body),
                })
                .map_err(RpcError::from)?;
            Ok(style_json(s.editor.doc(), id))
        }
        "render_style.set_active" => {
            let doc = s.editor.doc();
            let style = match required(m, "id")? {
                Value::Null => None,
                v => Some(ref_of(doc, v, Kind::Style)?),
            };
            s.editor
                .execute(&Command::SetRenderStyle { style })
                .map_err(RpcError::from)?;
            Ok(json!({"active": style}))
        }
        "sun.set" => sun_set(s, m),
        "sun.get" => sun_get(s, m),
        "render.hidden_line" => hidden_line(s, m),
        _ => unreachable!("handle checks the method"),
    }
}

// --- Slots ------------------------------------------------------------------------------

fn style_target(doc: &Document, v: &Value) -> Result<ResourceId, RpcError> {
    let ok = |id: ResourceId| {
        doc.resources.plant_style(id).is_some() || doc.resources.hardscape_style(id).is_some()
    };
    let id = match v {
        Value::String(name) => cadektor_core::uid::Uid::parse(name.trim())
            .map(ResourceId)
            .filter(|id| ok(*id))
            .or_else(|| doc.resources.find_plant_style(name))
            .or_else(|| doc.resources.find_hardscape_style(name)),
        _ => None,
    };
    id.filter(|id| ok(*id))
        .ok_or_else(|| bad(format!("no plant style or hardscape style {v}")))
}

fn assign(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let material = match required(m, "material")? {
        Value::Null => None,
        v => Some(ref_of(doc, v, Kind::Material)?),
    };
    let slot = match m.get("slot") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) => Some(t.clone()),
        Some(_) => return Err(bad("'slot' must be text")),
    };
    for k in m.keys() {
        if !["material", "slot", "object", "objects", "style", "class"].contains(&k.as_str()) {
            return Err(bad(format!(
                "unknown key '{k}'. Give material and one target: object, objects, style or class"
            )));
        }
    }
    let targets = [
        m.contains_key("object") || m.contains_key("objects"),
        m.contains_key("style"),
        m.contains_key("class"),
    ];
    if targets.iter().filter(|t| **t).count() != 1 {
        return Err(bad(
            "give exactly one target: object | objects, style, or class",
        ));
    }
    let mut commands = Vec::new();
    let mut push = |target: SlotTarget| {
        commands.push(Command::SetMaterialSlot { target, material });
    };
    if m.contains_key("object") || m.contains_key("objects") {
        let mut ids: Vec<ObjectId> = Vec::new();
        if let Some(v) = m.get("object") {
            ids.push(ObjectId(
                v.as_uid()
                    .ok_or_else(|| bad("'object' must be an object ID"))?,
            ));
        }
        if let Some(v) = m.get("objects") {
            for x in v
                .as_array()
                .ok_or_else(|| bad("'objects' must be a list of IDs"))?
            {
                ids.push(ObjectId(
                    x.as_uid()
                        .ok_or_else(|| bad("'objects' must be a list of IDs"))?,
                ));
            }
        }
        for object in ids {
            push(SlotTarget::Object {
                object,
                slot: slot.clone(),
            });
        }
    } else if let Some(v) = m.get("style") {
        let style = style_target(doc, v)?;
        let slot = slot
            .ok_or_else(|| bad("a style needs a 'slot' (trunk, canopy, surface or component:N)"))?;
        push(SlotTarget::Style { style, slot });
    } else if let Some(v) = m.get("class") {
        let class = ClassId(
            v.as_uid()
                .ok_or_else(|| bad("'class' must be a class ID"))?,
        );
        push(SlotTarget::Class { class });
    }
    let assigned = commands.len();
    s.editor
        .execute(&Command::Batch { commands })
        .map_err(RpcError::from)?;
    Ok(json!({"assigned": assigned}))
}

// --- Render styles -----------------------------------------------------------------------

fn style_fields(name: Option<&str>, st: &RenderStyle) -> Value {
    let mut v = json!({
        "mode": st.mode.id(),
        "edges": st.edges,
        "shadows": st.shadows,
        "sun": st.sun,
        "textures": st.textures,
        "background": st.background.0,
        "line_weight": st.line_weight,
    });
    if let Some(n) = name {
        v["name"] = json!(n);
    }
    v
}

/// The fields of a render style as JSON, for results.
pub fn style_summary(st: &RenderStyle) -> Value {
    style_fields(None, st)
}

fn style_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("checked");
    let mut v = style_fields(
        Some(&r.name),
        present::render_style(doc, id).expect("checked"),
    );
    v["id"] = json!(id);
    v["active"] = json!(doc.drawing.presentation.active_style == Some(id));
    v
}

fn style_body(base: RenderStyle, patch: &Map<String, Value>) -> Result<ResourceBody, RpcError> {
    let mut st = base;
    for (k, v) in patch {
        match k.as_str() {
            "mode" => {
                st.mode = RenderMode::from_name(v.as_str().unwrap_or("")).ok_or_else(|| {
                    bad(format!(
                        "unknown mode. Modes: {}",
                        RenderMode::ALL.map(|m| m.id()).join(", ")
                    ))
                })?
            }
            "edges" => st.edges = flag(v, "edges")?,
            "shadows" => st.shadows = flag(v, "shadows")?,
            "sun" => st.sun = flag(v, "sun")?,
            "textures" => st.textures = flag(v, "textures")?,
            "background" => st.background = color_of(v, "background")?,
            "line_weight" => st.line_weight = number(v, "line_weight")?,
            _ => {
                return Err(bad(format!(
                    "unknown render style field '{k}'. Fields: mode, edges, shadows, sun, textures, background, line_weight"
                )));
            }
        }
    }
    st.validate().map_err(bad)?;
    Ok(ResourceBody::Render(Box::new(RenderResource::RenderStyle(
        st,
    ))))
}

/// The ID of a render style resource of the document, by ID or name.
pub fn document_style(doc: &Document, v: &Value) -> Result<ResourceId, RpcError> {
    ref_of(doc, v, Kind::Style)
}

/// A render style from a parameter: a document style by ID or name, or a built-in style by name.
pub fn style_param(doc: &Document, v: &Value) -> Result<RenderStyle, RpcError> {
    if let Ok(id) = ref_of(doc, v, Kind::Style) {
        return Ok(present::render_style(doc, id).expect("checked").clone());
    }
    if let Some(name) = v.as_str() {
        let norm = |t: &str| t.trim().to_ascii_lowercase().replace(['_', '-'], " ");
        if let Some((_, st)) = RenderStyle::presets()
            .into_iter()
            .find(|(n, _)| norm(n) == norm(name))
        {
            return Ok(st);
        }
    }
    let builtin: Vec<&str> = RenderStyle::presets().iter().map(|(n, _)| *n).collect();
    Err(bad(format!(
        "no render style {v}. Built-in styles: {}",
        builtin.join(", ")
    )))
}

// --- The sun ----------------------------------------------------------------------------

/// Latitude and longitude of the georeference origin, and the angle clockwise from the model +Y axis
/// to true north (with the meridian convergence of the CRS), in degrees.
fn georef_place(doc: &Document) -> Result<(f64, f64, f64), RpcError> {
    let g = doc
        .georef
        .as_ref()
        .ok_or_else(|| bad("the document has no georeference. Give latitude and longitude"))?;
    if g.crs.trim().is_empty() {
        return Err(bad(
            "the georeference has no coordinate system. Give latitude and longitude",
        ));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let p = cadektor_crs::place_of(g).ok_or_else(|| app("cannot use the georeference"))?;
        let (lat, lon) = p
            .lat_lon
            .ok_or_else(|| app("cannot convert the georeference to latitude and longitude"))?;
        Ok((lat, lon, p.north_angle()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        if g.crs.trim().eq_ignore_ascii_case("EPSG:4326") {
            Ok((g.origin.y, g.origin.x, g.rotation.to_degrees()))
        } else {
            Err(app(
                "converting the georeference needs the desktop build. Give latitude and longitude",
            ))
        }
    }
}

fn parse_date(t: &str) -> Result<(i32, u32, u32), RpcError> {
    let p: Vec<&str> = t.trim().split('-').collect();
    let bad_date = || bad("'date' must look like 2026-06-21");
    if p.len() != 3 {
        return Err(bad_date());
    }
    Ok((
        p[0].parse().map_err(|_| bad_date())?,
        p[1].parse().map_err(|_| bad_date())?,
        p[2].parse().map_err(|_| bad_date())?,
    ))
}

fn parse_time(t: &str) -> Result<f64, RpcError> {
    let bad_time = || bad("'time' must look like 15:30");
    let p: Vec<&str> = t.trim().split(':').collect();
    if p.len() < 2 || p.len() > 3 {
        return Err(bad_time());
    }
    let h: f64 = p[0].parse().map_err(|_| bad_time())?;
    let min: f64 = p[1].parse().map_err(|_| bad_time())?;
    let sec: f64 = p
        .get(2)
        .map_or(Ok(0.0), |x| x.parse())
        .map_err(|_| bad_time())?;
    let hour_ok = (0.0..24.0).contains(&h) || (h == 24.0 && min == 0.0 && sec == 0.0);
    if !hour_ok || !(0.0..60.0).contains(&min) || !(0.0..60.0).contains(&sec) {
        return Err(bad_time());
    }
    Ok(h + min / 60.0 + sec / 3600.0)
}

fn sun_json(st: &SunSettings) -> Value {
    json!({
        "latitude": st.latitude,
        "longitude": st.longitude,
        "date": format!("{:04}-{:02}-{:02}", st.year, st.month, st.day),
        "time": sun::hhmm(st.hour),
        "tz_offset": st.tz_offset,
        "dst": st.dst,
        "north_angle": st.north_angle,
        "from_georef": st.from_georef,
    })
}

fn sun_info(st: &SunSettings) -> Value {
    let p = st.position();
    let d = sun::day(&st.sun_time());
    let h = |x: Option<f64>| x.map(sun::hhmm);
    json!({
        "sun": sun_json(st),
        "position": {
            "azimuth": p.azimuth,
            "elevation": p.elevation,
            "declination": p.declination,
            "equation_of_time": p.equation_of_time,
            "above_horizon": p.elevation > 0.0,
        },
        "day": {
            "sunrise": h(d.sunrise),
            "solar_noon": sun::hhmm(d.solar_noon),
            "sunset": h(d.sunset),
            "always_up": d.always_up,
        },
    })
}

fn sun_set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    for k in m.keys() {
        if ![
            "latitude",
            "longitude",
            "from_georef",
            "date",
            "time",
            "tz_offset",
            "dst",
            "north_angle",
            "off",
        ]
        .contains(&k.as_str())
        {
            return Err(bad(format!(
                "unknown key '{k}'. Keys: latitude, longitude, from_georef, date, time, tz_offset, dst, north_angle, off"
            )));
        }
    }
    if m.get("off")
        .map(|v| flag(v, "off"))
        .transpose()?
        .unwrap_or(false)
    {
        s.editor
            .execute(&Command::SetSun { sun: None })
            .map_err(RpcError::from)?;
        return Ok(json!({"sun": null}));
    }
    let doc = s.editor.doc();
    let mut st = doc.drawing.presentation.sun.unwrap_or_default();
    let mut tz_given = m.contains_key("tz_offset");
    if m.get("from_georef")
        .map(|v| flag(v, "from_georef"))
        .transpose()?
        .unwrap_or(false)
    {
        let (lat, lon, north) = georef_place(doc)?;
        st.latitude = lat;
        st.longitude = lon;
        st.from_georef = true;
        st.north_angle = north;
        if !tz_given && !m.contains_key("longitude") {
            st.tz_offset = (lon / 15.0).round();
            tz_given = true;
        }
    }
    if let Some(v) = m.get("latitude") {
        st.latitude = number(v, "latitude")?;
        st.from_georef = false;
    }
    if let Some(v) = m.get("longitude") {
        st.longitude = number(v, "longitude")?;
        st.from_georef = false;
        if !tz_given {
            st.tz_offset = (st.longitude / 15.0).round();
        }
    }
    if let Some(v) = m.get("date") {
        (st.year, st.month, st.day) =
            parse_date(v.as_str().ok_or_else(|| bad("'date' must be text"))?)?;
    }
    if let Some(v) = m.get("time") {
        st.hour = parse_time(v.as_str().ok_or_else(|| bad("'time' must be text"))?)?;
    }
    if let Some(v) = m.get("dst") {
        st.dst = flag(v, "dst")?;
    }
    if let Some(v) = m.get("tz_offset") {
        st.tz_offset = number(v, "tz_offset")?;
    }
    if let Some(v) = m.get("north_angle") {
        st.north_angle = number(v, "north_angle")?;
    }
    s.editor
        .execute(&Command::SetSun { sun: Some(st) })
        .map_err(RpcError::from)?;
    Ok(sun_info(&st))
}

fn sun_get(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let Some(st) = s.editor.doc().drawing.presentation.sun else {
        return Ok(json!({"sun": null, "hint": "call sun.set first"}));
    };
    let mut out = sun_info(&st);
    if let Some(times) = m.get("times") {
        let mut path = Vec::new();
        for t in times
            .as_array()
            .ok_or_else(|| bad("'times' must be a list of \"HH:MM\""))?
        {
            let hour = parse_time(t.as_str().ok_or_else(|| bad("'times' must be text"))?)?;
            let p = SunSettings { hour, ..st }.position();
            path.push(
                json!({"time": sun::hhmm(hour), "azimuth": p.azimuth, "elevation": p.elevation}),
            );
        }
        out["path"] = json!(path);
    }
    Ok(out)
}

// --- Hidden-line output --------------------------------------------------------------------

fn hidden_line(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    use cadektor_render::hlr::{HlOptions, hidden_lines};
    for k in m.keys() {
        if ![
            "view",
            "projection",
            "width",
            "height",
            "dashed",
            "mesh_edges",
            "segments",
        ]
        .contains(&k.as_str())
        {
            return Err(bad(format!("unknown key '{k}'")));
        }
    }
    let text = |k: &str| m.get(k).and_then(Value::as_str);
    let settings = s.resolve_view(text("view"), text("projection"), None)?;
    let size = |k: &str, d: u32| {
        m.get(k)
            .and_then(Value::as_u64)
            .map_or(d, |n| n.min(u32::MAX as u64) as u32)
    };
    let (width, height) = s.limits.image_size(size("width", 800), size("height", 600));
    let get = |k: &str, d: bool| m.get(k).and_then(Value::as_bool).unwrap_or(d);
    let scene = s.scene();
    let mut camera = cadektor_render::Camera::standard(settings.view, settings.projection);
    camera.fit(scene.bounds, width as f32 / height as f32);
    let lines = hidden_lines(
        &scene,
        &camera,
        width,
        height,
        &HlOptions {
            include_hidden: get("dashed", false),
            mesh_edges: get("mesh_edges", true),
        },
    );
    let mut out = json!({
        "svg": lines.to_svg(1.2),
        "visible_edges": lines.visible_edge_count(),
        "width": width,
        "height": height,
        "view": settings.view.id(),
        "projection": settings.projection.id(),
    });
    if get("segments", false) {
        out["segments"] = lines
            .segments
            .iter()
            .map(|g| json!({"a": g.a, "b": g.b, "hidden": g.hidden, "color": g.color, "object": g.object}))
            .collect::<Vec<_>>()
            .into();
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
