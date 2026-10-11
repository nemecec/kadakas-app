//! Agent API for plant pictures (PLT-16): `plant_style.set_picture|clear_picture|get_picture`.
//!
//! A picture is a PNG or JPEG file. It is stored once in the document as an `image` resource
//! (content addressed by SHA-256), and a plant style refers to it. The size limits come from
//! `SessionLimits` (`picture_bytes`, `picture_pixels`). A hostile file gives an error, not a
//! crash.

use kadakas_core::command::Command;
use kadakas_core::model::Document;
use kadakas_core::picture::{ImageResource, decode_base64};
use kadakas_core::resource::ResourceId;
use serde_json::{Map, Value, json};

use crate::errors::{app, bad};
use crate::plants_api::style_ref;
use crate::{RpcError, Session};

pub const PLANT_STYLE_SET_PICTURE: (&str, &str) = (
    "plant_style.set_picture",
    "Set the picture of a plant style from a PNG or JPEG file. params: {id (the style ID or name), path | base64 (the file bytes as base64 text), name? (the file name to remember, for base64)}. The file is stored once in the document (by SHA-256) and shared by styles that use the same file. The size is limited (default 8 MiB and 16 million pixels). One undo step. Returns {id, image (the picture resource ID), created (false if the document had the file already), picture}.",
);
pub const PLANT_STYLE_CLEAR_PICTURE: (&str, &str) = (
    "plant_style.clear_picture",
    "Remove the picture of a plant style. The picture resource goes too when no other style uses it. params: {id}. Returns {id, had_picture}.",
);
pub const PLANT_STYLE_GET_PICTURE: (&str, &str) = (
    "plant_style.get_picture",
    "The picture of a plant style. params: {id, thumbnail? (true adds thumbnail_base64, a PNG of at most 160 px), bytes? (true adds base64, the original file)}. Returns {id, has_picture, picture?: {image, sha256, format, mime, width, height, size_bytes, source}, thumbnail_base64?, base64?}.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "formats": ["png", "jpeg"],
        "limits": "A picture file is at most 8 MiB and 16 million pixels (SessionLimits.picture_bytes and picture_pixels can lower this). Larger files and files that are not pictures give an error.",
        "storage": "The file is an `image` resource (type image) with its SHA-256, size and a small thumbnail. In shared documents the file travels once as a blob.",
        "uses": "The picture shows in the Resource Manager, the plant style editor and Object Info. A worksheet column named Image (or Picture) shows it in plant lists and object lists. CSV and DXF output skip pictures.",
        "csv": "plant_style.import_csv reads a picture file name from an image, picture or photo column. Give pictures_dir (a folder) to find the files; names are relative to it and cannot leave it.",
        "examples": [
            {"method": "plant_style.set_picture", "params": {"id": "Field maple", "path": "/data/pictures/maple.jpg"}},
            {"method": "plant_style.get_picture", "params": {"id": "Field maple", "thumbnail": true}},
            {"method": "plant_style.clear_picture", "params": {"id": "Field maple"}}
        ],
    })
}

/// The metadata of a picture resource (`None` if there is none).
pub fn picture_meta(doc: &Document, image: Option<ResourceId>) -> Option<Value> {
    let id = image?;
    let i = doc.resources.image(id)?;
    Some(json!({
        "image": id,
        "sha256": i.sha256,
        "format": i.format.id(),
        "mime": i.format.mime(),
        "width": i.width,
        "height": i.height,
        "size_bytes": i.byte_len(),
        "source": i.source,
    }))
}

/// Handles a picture method. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !matches!(
        method,
        "plant_style.set_picture" | "plant_style.clear_picture" | "plant_style.get_picture"
    ) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(run(s, method, &p))
}

fn run(s: &mut Session, method: &str, p: &Value) -> Result<Value, RpcError> {
    let m = p
        .as_object()
        .ok_or_else(|| bad("params must be an object"))?;
    let id = style_ref(
        s.editor.doc(),
        m.get("id").ok_or_else(|| bad("missing 'id'"))?,
    )?;
    match method {
        "plant_style.set_picture" => {
            let (bytes, name) = picture_bytes(s, m)?;
            let res =
                kadakas_render::picture::import_picture(&bytes, &name, s.limits().picture_limits())
                    .map_err(RpcError::validation)?;
            set_picture(s, id, res)
        }
        "plant_style.clear_picture" => {
            let had = s
                .editor
                .doc()
                .resources
                .plant_style(id)
                .and_then(|st| st.image);
            let had = had.is_some_and(|i| s.editor.doc().resources.image(i).is_some());
            if had {
                s.editor
                    .execute(&Command::SetPlantPicture {
                        style: id,
                        picture: None,
                    })
                    .map_err(RpcError::from)?;
            }
            Ok(json!({"id": id, "had_picture": had}))
        }
        "plant_style.get_picture" => {
            let (want_thumb, want_bytes) = (flag(m, "thumbnail")?, flag(m, "bytes")?);
            let doc = s.editor.doc();
            let image = doc.resources.plant_style(id).and_then(|st| st.image);
            let Some(meta) = picture_meta(doc, image) else {
                return Ok(json!({"id": id, "has_picture": false}));
            };
            let res = doc
                .resources
                .image(image.expect("has meta"))
                .expect("has meta");
            let mut out = json!({"id": id, "has_picture": true, "picture": meta});
            if want_thumb {
                out["thumbnail_base64"] = json!(res.thumb);
            }
            if want_bytes {
                out["base64"] = json!(res.data);
            }
            Ok(out)
        }
        _ => unreachable!("handle checks the method"),
    }
}

/// Stores a picture for a style (shared with the CSV import).
pub(crate) fn set_picture(
    s: &mut Session,
    id: ResourceId,
    res: ImageResource,
) -> Result<Value, RpcError> {
    let out = s
        .editor
        .execute(&Command::SetPlantPicture {
            style: id,
            picture: Some(Box::new(res)),
        })
        .map_err(RpcError::from)?;
    let doc = s.editor.doc();
    let image = doc.resources.plant_style(id).and_then(|st| st.image);
    Ok(json!({
        "id": id,
        "image": image,
        "created": !out.created.is_empty(),
        "picture": picture_meta(doc, image),
    }))
}

fn flag(m: &Map<String, Value>, key: &str) -> Result<bool, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(format!("'{key}' must be true or false"))),
    }
}

/// The bytes of the file that the request names, and the name to remember.
fn picture_bytes(s: &Session, m: &Map<String, Value>) -> Result<(Vec<u8>, String), RpcError> {
    let limit = s.limits().picture_limits().max_bytes;
    match (m.get("path"), m.get("base64")) {
        (Some(_), Some(_)) => Err(bad("give path or base64, not both")),
        (Some(p), None) => {
            let path = p.as_str().ok_or_else(|| bad("'path' must be text"))?;
            let bytes = read_picture_file(s, path, limit)?;
            Ok((bytes, path.to_string()))
        }
        (None, Some(b)) => {
            let text = b
                .as_str()
                .ok_or_else(|| bad("'base64' must be text"))?
                .trim();
            let text = text.strip_prefix("data:").map_or(text, |t| {
                t.split_once("base64,").map_or(text, |(_, rest)| rest)
            });
            // Check the length before any work, so a huge text costs nothing.
            if text.len() > limit / 3 * 4 + 16 {
                return Err(RpcError::limit(format!(
                    "the picture is larger than {limit} bytes"
                )));
            }
            let clean: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
            let bytes = decode_base64(&clean, limit)
                .map_err(|e| bad(format!("the picture is not valid base64: {e}")))?;
            let name = m
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok((bytes, name))
        }
        (None, None) => Err(bad("give path or base64")),
    }
}

fn read_picture_file(s: &Session, path: &str, limit: usize) -> Result<Vec<u8>, RpcError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let p = s
            .check_path(std::path::Path::new(path), false, &["png", "jpg", "jpeg"])
            .map_err(RpcError::policy)?;
        let file = std::fs::File::open(&p)
            .map_err(|e| app(format!("cannot read {}: {e}", p.display())))?;
        // Read one byte more than the limit: a file that grows or lies about its size still stops.
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| app(format!("cannot read {}: {e}", p.display())))?;
        if bytes.len() > limit {
            return Err(RpcError::limit(format!(
                "the picture file is larger than {limit} bytes"
            )));
        }
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path, limit);
        Err(app("reading files is not available here. Use base64"))
    }
}

/// Reads a picture file that the CSV import names, relative to `dir`. Native only. The name
/// cannot be absolute and cannot leave the folder.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_csv_picture(
    s: &Session,
    dir: &std::path::Path,
    name: &str,
) -> Result<ImageResource, String> {
    let rel = std::path::Path::new(name.trim());
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(format!("'{name}' must be a file name inside the folder"));
    }
    let full = dir.join(rel);
    let canon = std::fs::canonicalize(&full).map_err(|e| format!("cannot read '{name}': {e}"))?;
    let base = std::fs::canonicalize(dir).map_err(|e| format!("cannot read the folder: {e}"))?;
    if !canon.starts_with(&base) {
        return Err(format!("'{name}' is outside the folder"));
    }
    let path = canon.to_string_lossy().into_owned();
    let limit = s.limits().picture_limits().max_bytes;
    let bytes = read_picture_file(s, &path, limit).map_err(|e| e.message)?;
    kadakas_render::picture::import_picture(&bytes, name, s.limits().picture_limits())
}
