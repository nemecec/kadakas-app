//! Library files: import resources from another document, export a library, the per-user library
//! folder and favorites. Native only: these methods read and write files, and the path policy of the
//! session decides which files an agent may use.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use kadakas_core::command::{Command, Editor};
use kadakas_core::library::{self, Conflict};
use kadakas_core::model::Document;
use kadakas_core::resource::ResourceId;

use crate::errors::{app, bad};
use crate::{RpcError, Session};

/// The per-user library folder: `KADAKAS_LIBRARY_DIR`, else `~/.config/kadakas/library`.
pub fn library_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("KADAKAS_LIBRARY_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    Some(
        kadakas_sys::dirs::home_dir()?
            .join(".config")
            .join("kadakas")
            .join("library"),
    )
}

/// True for the name of a document file: `.kad`.
fn is_library_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".kad")
}

/// A file in the library folder by name. The name has no folders. A name without an extension
/// means the `.kad` file.
fn library_file(name: &str) -> Result<PathBuf, RpcError> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(bad("a library name is a file name without folders"));
    }
    let dir =
        library_dir().ok_or_else(|| app("there is no library folder: set KADAKAS_LIBRARY_DIR"))?;
    if is_library_name(name) {
        return Ok(dir.join(name));
    }
    Ok(dir.join(format!("{name}.kad")))
}

/// The path of a request (`path` or `library`), checked by the path policy of the session.
fn request_path(s: &Session, m: &Map<String, Value>, write: bool) -> Result<PathBuf, RpcError> {
    let path = match (
        m.get("path").and_then(Value::as_str),
        m.get("library").and_then(Value::as_str),
    ) {
        (Some(p), _) => PathBuf::from(p),
        (None, Some(l)) => library_file(l)?,
        _ => return Err(bad("give 'path' or 'library'")),
    };
    s.check_path(&path, write, &crate::files::DOC_EXTS)
        .map_err(RpcError::policy)
}

fn load(path: &Path) -> Result<Document, RpcError> {
    crate::files::read_document(path)
}

/// Resource IDs of a source document for the `items` of a request.
fn pick(src: &Document, m: &Map<String, Value>) -> Result<Vec<ResourceId>, RpcError> {
    if m.get("all").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(src.resources.iter().map(|r| r.id).collect());
    }
    let Some(items) = m.get("items") else {
        return Ok(Vec::new());
    };
    let items = items
        .as_array()
        .ok_or_else(|| bad("'items' must be a list"))?;
    let mut out = Vec::new();
    for it in items {
        let (ty, name) = match it {
            Value::String(n) => (None, n.as_str()),
            Value::Object(o) => (
                o.get("type").and_then(Value::as_str),
                o.get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("each item needs a 'name'"))?,
            ),
            _ => return Err(bad("an item is a name or {type, name}")),
        };
        let hits: Vec<ResourceId> = src
            .resources
            .iter()
            .filter(|r| {
                r.name.eq_ignore_ascii_case(name.trim())
                    && ty.is_none_or(|t| r.body.type_name() == t)
            })
            .map(|r| r.id)
            .collect();
        match hits.as_slice() {
            [one] => out.push(*one),
            [] => {
                return Err(app(format!(
                    "the file has no {} named '{name}'",
                    ty.unwrap_or("resource")
                )));
            }
            _ => {
                return Err(bad(format!(
                    "several resources are named '{name}'. Add the type"
                )));
            }
        }
    }
    Ok(out)
}

fn names(m: &Map<String, Value>, key: &str) -> Result<Vec<String>, RpcError> {
    match m.get(key) {
        None => Ok(Vec::new()),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| bad(format!("'{key}' must be a list of names"))),
    }
}

/// The favorites file. The path is fixed: the library folder of the host (`KADAKAS_LIBRARY_DIR` or
/// the config folder) and the name `favorites.json`. A request cannot change it, so agents cannot
/// use the favorites calls to write anywhere else.
fn favorites_path() -> Result<PathBuf, RpcError> {
    Ok(library_dir()
        .ok_or_else(|| app("there is no library folder: set KADAKAS_LIBRARY_DIR"))?
        .join("favorites.json"))
}

fn read_favorites() -> Result<Vec<Value>, RpcError> {
    let p = favorites_path()?;
    match std::fs::read_to_string(&p) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| app(format!("{}: {e}", p.display()))),
        Err(_) => Ok(Vec::new()),
    }
}

fn write_favorites(list: &[Value]) -> Result<(), RpcError> {
    let p = favorites_path()?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(app)?;
    }
    // A link in the place of the file would send the write somewhere else.
    if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(RpcError::policy(format!(
            "{} is a link. Favorites are written to the library folder only",
            p.display()
        )));
    }
    kadakas_sys::fsio::write_atomic(
        &p,
        serde_json::to_string_pretty(list).map_err(app)?.as_bytes(),
    )
    .map_err(app)
}

pub(crate) fn run(
    s: &mut Session,
    method: &str,
    m: &Map<String, Value>,
) -> Result<Value, RpcError> {
    match method {
        "resource.list_file" => {
            let path = request_path(s, m, false)?;
            let src = load(&path)?;
            let rows: Vec<Value> = library::list(&src, None, None).iter().map(|i| json!({"id": i.id, "name": i.name, "type": i.type_name, "depends_on": i.depends_on})).collect();
            Ok(json!({
                "resources": rows,
                "classes": src.classes.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                "layers": src.layers.iter().filter(|l| l.kind == kadakas_core::model::LayerKind::Design).map(|l| l.name.clone()).collect::<Vec<_>>(),
            }))
        }
        "resource.import_from_file" => {
            let path = request_path(s, m, false)?;
            let src = load(&path)?;
            let conflict = match m.get("on_conflict").and_then(Value::as_str) {
                None => Conflict::Rename,
                Some(c) => Conflict::from_name(c)
                    .ok_or_else(|| bad("'on_conflict' is rename, reuse or replace"))?,
            };
            let picks = pick(&src, m)?;
            let classes = names(m, "classes")?;
            let layers = names(m, "layers")?;
            if picks.is_empty() && classes.is_empty() && layers.is_empty() {
                return Err(bad("name what to import: items, all, classes or layers"));
            }
            // Classes and layers come first. The plan for the resources is made on a copy that has them,
            // so the IDs it plans are right, and everything runs as one undo step.
            let mut scratch = Editor::new(s.editor.doc().clone());
            let mut commands = Vec::new();
            let (cc, classes_added) =
                library::import_classes_plan(scratch.doc(), &src, &classes).map_err(app)?;
            let (lc, layers_added) =
                library::import_layers_plan(scratch.doc(), &src, &layers).map_err(app)?;
            for c in [cc, lc].into_iter().flatten() {
                scratch.execute(&c).map_err(RpcError::from)?;
                commands.push(c);
            }
            let plan = library::import_plan(scratch.doc(), &src, &picks, conflict).map_err(app)?;
            commands.extend(plan.command.clone());
            if !commands.is_empty() {
                s.editor
                    .execute(&Command::Batch { commands })
                    .map_err(RpcError::from)?;
                s.prune_selection();
            }
            let imported: Vec<Value> = plan.items.iter().map(|i| json!({"name": i.name, "type": i.type_name, "id": i.dst, "status": i.status.id(), "dependency": i.dependency})).collect();
            let mut classes_added = classes_added;
            classes_added.extend(plan.classes_added);
            Ok(
                json!({ "imported": imported, "classes_added": classes_added, "layers_added": layers_added }),
            )
        }
        "resource.export_to_file" => {
            let path = request_path(s, m, true)?;
            if path.exists()
                && !m.get("overwrite").and_then(Value::as_bool).unwrap_or(false)
                && s.paths.is_restricted()
            {
                return Err(RpcError::conflict(format!(
                    "{} exists: pass \"overwrite\": true to replace it",
                    path.display()
                )));
            }
            let doc = s.editor.doc();
            let ids: Vec<ResourceId> = match m.get("ids") {
                Some(v) => serde_json::from_value(v.clone()).map_err(bad)?,
                None => doc.resources.iter().map(|r| r.id).collect(),
            };
            let mut out = Editor::default();
            let plan = library::import_plan(out.doc(), doc, &ids, Conflict::Rename).map_err(app)?;
            if let Some(c) = &plan.command {
                out.execute(c).map_err(RpcError::from)?;
            }
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(app)?;
            }
            let text = serde_json::to_string_pretty(out.doc()).map_err(app)?;
            kadakas_sys::fsio::write_atomic(&path, text.as_bytes())
                .map_err(|e| app(format!("cannot write {}: {e}", path.display())))?;
            Ok(json!({ "path": path, "count": plan.items.len() }))
        }
        "library.list" => {
            let dir = library_dir()
                .ok_or_else(|| app("there is no library folder: set KADAKAS_LIBRARY_DIR"))?;
            let mut files = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if is_library_name(&name)
                        && !name.eq_ignore_ascii_case("favorites.json")
                        && let Ok(doc) = std::fs::read_to_string(e.path())
                            .map_err(|e| e.to_string())
                            .and_then(|t| Document::from_json(&t).map_err(|e| e.to_string()))
                    {
                        files.push(json!({"file": name, "resources": doc.resources.len()}));
                    }
                }
            }
            files.sort_by_key(|f| f["file"].as_str().unwrap_or_default().to_string());
            Ok(json!({ "dir": dir, "files": files }))
        }
        "library.favorites" => Ok(json!({ "favorites": read_favorites()? })),
        // library.favorite_add and library.favorite_remove (the router sends no other method here).
        _ => {
            let entry = json!({
                "library": m.get("library").and_then(Value::as_str).ok_or_else(|| bad("missing 'library'"))?,
                "type": m.get("type").and_then(Value::as_str).ok_or_else(|| bad("missing 'type'"))?,
                "name": m.get("name").and_then(Value::as_str).ok_or_else(|| bad("missing 'name'"))?,
            });
            let mut list = read_favorites()?;
            if method == "library.favorite_add" {
                if !list.contains(&entry) {
                    list.push(entry);
                }
            } else {
                list.retain(|e| *e != entry);
            }
            write_favorites(&list)?;
            Ok(json!({ "favorites": list }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_names_are_plain_file_names() {
        assert!(library_file("../x").is_err());
        assert!(library_file("a/b").is_err());
        assert!(library_file(".hidden").is_err());
        assert!(library_file("").is_err());
        let p = library_file("Trees").unwrap();
        assert!(p.to_string_lossy().ends_with("Trees.kad"));
        assert!(library_file("Trees.kad").unwrap().ends_with("Trees.kad"));
        assert!(is_library_name("A.KAD") && is_library_name("a.kad") && !is_library_name("a.txt"));
    }
}
