//! `site.texture`: the look of the 3D terrain mesh (3D-05): a texture with a planar top
//! projection, or colors by elevation bands. The values are fields of the site model settings;
//! this method checks them and gives a clear answer. `site.update` with `settings` sets the
//! same fields.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use kadakas_core::model::{ObjectId, Shape, SiteSettings};
use kadakas_core::site::SurfaceLook;

use crate::present_api::{Kind, ref_of};
use crate::{RpcError, Session, params};

pub const SITE_TEXTURE: (&str, &str) = (
    "site.texture",
    "Set or read the look of the 3D terrain mesh of a site model. params: {id, look? (color | texture | elevation), material? (name or id of a material resource whose texture is used; null: the material of the class or of the object slot), tile_size? (mm, 10 to 1e8: the size of one repeat of the texture on the ground; the mapping is a planar projection from above), offset? ([x, y] mm), rotation? (degrees, counterclockwise), bands? (1 to 64 elevation bands), ramp? ([[r, g, b, a]], 1 to 16 colors from low to high ground), range? ([low, high] mm elevation of the ramp; null: the range of the shown surface)}. Give only id to read. With look color the surface keeps its color (or the material of a slot); with texture the terrain shows the texture in the 3D view, render.png and the glTF, GLB and OBJ exports (texture coordinates and the image); with elevation each band is a mesh of one color from the ramp. Returns {id, look, material, tile_size, offset, rotation, bands, ramp, range}.",
);

#[derive(Deserialize)]
struct P {
    id: ObjectId,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

fn look_of(v: &Value) -> Result<SurfaceLook, RpcError> {
    let name = v.as_str().unwrap_or_default();
    SurfaceLook::ALL
        .into_iter()
        .find(|l| l.id() == name)
        .ok_or_else(|| RpcError::invalid_params("look must be color, texture or elevation"))
}

fn ramp_of(v: &Value) -> Result<Value, RpcError> {
    let err = || RpcError::invalid_params("ramp must be a list of [r, g, b] or [r, g, b, a]");
    let list = v.as_array().ok_or_else(err)?;
    let mut out = Vec::new();
    for c in list {
        let a = c
            .as_array()
            .filter(|a| a.len() == 3 || a.len() == 4)
            .ok_or_else(err)?;
        let mut px = [0u8, 0, 0, 255];
        for (i, x) in a.iter().enumerate() {
            px[i] = x
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(err)?;
        }
        out.push(json!(px));
    }
    Ok(Value::Array(out))
}

fn view(id: ObjectId, s: &SiteSettings, material_name: Option<String>) -> Value {
    json!({
        "id": id,
        "look": s.surface_look.id(),
        "material": material_name,
        "tile_size": s.texture_tile,
        "offset": s.texture_offset,
        "rotation": s.texture_rotation,
        "bands": s.elevation_bands,
        "ramp": s.elevation_ramp.iter().map(|c| c.0).collect::<Vec<_>>(),
        "range": s.elevation_range,
    })
}

impl Session {
    pub(crate) fn site_texture(&mut self, p: Value) -> Result<Value, RpcError> {
        let P { id, rest } = params(p)?;
        let mut patch = Map::new();
        for (key, value) in &rest {
            let (field, v) = match key.as_str() {
                "look" => ("surface_look", json!(look_of(value)?.id())),
                "material" if value.is_null() => ("texture", Value::Null),
                "material" => {
                    let rid = ref_of(self.editor.doc(), value, Kind::Material)?;
                    ("texture", json!(rid))
                }
                "tile_size" => ("texture_tile", value.clone()),
                "offset" => ("texture_offset", value.clone()),
                "rotation" => ("texture_rotation", value.clone()),
                "bands" => ("elevation_bands", value.clone()),
                "ramp" => ("elevation_ramp", ramp_of(value)?),
                "range" => ("elevation_range", value.clone()),
                other => {
                    return Err(RpcError::invalid_params(format!(
                        "unknown parameter '{other}'"
                    )));
                }
            };
            patch.insert(field.into(), v);
        }
        if !patch.is_empty() {
            self.handle_site("site.update", json!({"id": id, "settings": patch}))?;
        }
        let doc = self.editor.doc();
        let obj = doc
            .object(id)
            .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
        let Shape::SiteModel { settings, .. } = &obj.shape else {
            return Err(RpcError::validation(format!(
                "object {id} is not a site model"
            )));
        };
        let name = settings
            .texture
            .and_then(|r| doc.resources.get(r))
            .map(|r| r.name.clone());
        Ok(view(id, settings, name))
    }
}
