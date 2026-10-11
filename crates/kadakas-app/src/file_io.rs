//! Document files: JSON of `Document`. The extension is `.kad`.

use kadakas_l10n::tr;
use std::path::{Path, PathBuf};

use kadakas_api::Session;
use kadakas_core::model::Document;
use kadakas_sys::fsio;
use serde_json::Value;

pub fn parse_document(text: &str) -> Result<Document, String> {
    Document::from_json(text).map_err(|e| e.to_string())
}

pub fn read_document(path: &Path) -> Result<Document, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| tr!("file-io-cannot-read", path = path.display(), e = e))?;
    parse_document(&text)
}

/// Saves atomically (temporary file, then rename) and keeps the old file as `.bak`.
/// It counts as a manual save: a late autosave of the same file is dropped (see `autosave_fs`).
#[cfg(not(target_arch = "wasm32"))]
pub fn write_document(path: &Path, doc: &Document) -> Result<(), String> {
    crate::autosave::manual_save(path, || write_document_raw(path, doc))
}

/// `write_document` without the manual save bookkeeping.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_document_raw(path: &Path, doc: &Document) -> Result<(), String> {
    let text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    fsio::write_atomic(path, text.as_bytes())
        .map_err(|e| tr!("file-io-cannot-write", path = path.display(), e = e))
}

/// Writes the SVG of the session's document (the same SVG as `render.svg`), atomically.
pub fn write_svg(path: &Path, session: &Session) -> Result<(), String> {
    fsio::write_atomic(path, session.render_svg(1600).as_bytes())
        .map_err(|e| tr!("file-io-cannot-write", path = path.display(), e = e))
}

/// Adds `.kad` unless the name already ends in `.kad`.
pub fn with_extension(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s.ends_with(".kad") {
        path.to_path_buf()
    } else {
        format!("{s}.kad").into()
    }
}

/// What an agent request changed in the app besides the document.
#[derive(Debug, Default, PartialEq)]
#[cfg(not(target_arch = "wasm32"))]
pub struct AgentEffects {
    /// The document was replaced: the app must cancel the active tool and refit the view.
    pub replaced: bool,
    /// The file path changed: the app must update the window title.
    pub path_changed: bool,
}

/// Answers one request line from the agent connection `conn`. Like `Session::handle_line_as`, and
/// also `file.save {path}` and `file.open {path}`, which need the app's file path.
#[cfg(not(target_arch = "wasm32"))]
pub fn agent_line(
    session: &mut Session,
    path: &mut Option<PathBuf>,
    conn: u64,
    line: &str,
    fx: &mut AgentEffects,
) -> Option<String> {
    let req: Option<Value> = serde_json::from_str(line).ok();
    let method = req
        .as_ref()
        .and_then(|r| r["method"].as_str())
        .filter(|m| matches!(*m, "file.save" | "file.open"));
    let (Some(req), Some(method)) = (&req, method) else {
        let resp = session.handle_line_as(conn, line);
        if session.take_replaced() {
            // `document.load`: the new document does not come from the file that was open.
            *path = None;
            fx.replaced = true;
            fx.path_changed = true;
        }
        return resp;
    };
    // An agent may only use the allowed folders (see `Session::paths`) and only document files.
    // The same handler serves `kadakas serve` and `kadakas mcp`. A save here counts as a manual save.
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let result = kadakas_api::files::file_call(session, method, &params, &mut |p, bytes| {
        crate::autosave::manual_save(p, || kadakas_api::files::write_document_file(p, bytes))
    });
    match &result {
        Ok(kadakas_api::files::FileDone::Saved(p)) => {
            *path = Some(p.clone());
            fx.path_changed = true;
        }
        Ok(kadakas_api::files::FileDone::Opened(p)) => {
            *path = Some(p.clone());
            fx.replaced = true;
            fx.path_changed = true;
        }
        Err(_) => {}
    }
    let id = req.get("id")?.clone();
    Some(kadakas_api::files::file_reply(id, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_api::Session;
    use kadakas_core::command::Command;
    use kadakas_core::model::{Attributes, FORMAT_VERSION, Shape};
    use serde_json::json;

    #[test]
    fn save_and_load_roundtrip() {
        let mut s = Session::default();
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::new(1.0, 2.0),
                    radius: 3.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let path = std::env::temp_dir().join(format!("kadakas-test-{}.kad", std::process::id()));
        write_document(&path, s.editor.doc()).unwrap();
        assert_eq!(read_document(&path).unwrap(), s.editor.doc().clone());
        let svg = std::env::temp_dir().join(format!("kadakas-test-{}.svg", std::process::id()));
        write_svg(&svg, &s).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains("<circle"));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(svg);
    }

    #[test]
    fn rejects_garbage_and_newer_formats() {
        assert!(parse_document("{nope").is_err());
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        assert!(
            parse_document(&v.to_string())
                .unwrap_err()
                .contains("newer")
        );
    }

    #[test]
    fn extension_helper() {
        assert_eq!(with_extension(Path::new("a")).to_str(), Some("a.kad"));
        assert_eq!(with_extension(Path::new("a.kad")).to_str(), Some("a.kad"));
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn call(
        s: &mut Session,
        path: &mut Option<PathBuf>,
        fx: &mut AgentEffects,
        req: Value,
    ) -> Value {
        serde_json::from_str(&agent_line(s, path, 0, &req.to_string(), fx).unwrap()).unwrap()
    }

    #[test]
    fn saving_keeps_a_backup_and_leaves_no_temp_files() {
        let dir = temp("save");
        let p = dir.join("a.kad");
        let mut doc = Document::default();
        write_document(&p, &doc).unwrap();
        doc.layers[0].name = "Second".into();
        write_document(&p, &doc).unwrap();
        assert_eq!(read_document(&p).unwrap().layers[0].name, "Second");
        assert_eq!(
            read_document(&dir.join("a.kad.bak")).unwrap().layers[0].name,
            "Layer 1"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restricted_agents_may_use_kad_files_but_not_other_extensions() {
        let dir = temp("agent-ext");
        let mut s = Session::default();
        s.paths = kadakas_api::PathPolicy::restricted(vec![dir.clone()]);
        let (mut path, mut fx) = (None, AgentEffects::default());
        for (name, ok) in [("a.kad", true), ("b.json", false), ("c.txt", false)] {
            let p = dir.join(name);
            let r = call(
                &mut s,
                &mut path,
                &mut fx,
                json!({"id": 1, "method": "file.save", "params": {"path": p}}),
            );
            assert_eq!(r.get("error").is_none(), ok, "{name}: {r}");
            assert_eq!(p.exists(), ok, "{name}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_file_save_and_open_use_the_app_path() {
        let dir = temp("agent-files");
        let p = dir.join("plan.kad");
        let mut s = Session::default();
        let mut path = None;
        let mut fx = AgentEffects::default();
        let circle =
            json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}});
        call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 1, "method": "execute", "params": {"command": circle}}),
        );
        assert_eq!(
            fx,
            AgentEffects::default(),
            "normal requests change nothing in the app"
        );

        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 2, "method": "file.save", "params": {"path": p}}),
        );
        assert_eq!(r["result"], json!({"ok": true}));
        assert_eq!(path.as_deref(), Some(p.as_path()));
        assert_eq!(
            fx,
            AgentEffects {
                replaced: false,
                path_changed: true
            }
        );
        assert_eq!(read_document(&p).unwrap().objects.len(), 1);

        // Open a different document: the session is replaced and the app is told.
        let other = dir.join("other.kad");
        write_document(&other, &Document::default()).unwrap();
        let mut fx = AgentEffects::default();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 3, "method": "file.open", "params": {"path": other}}),
        );
        assert_eq!(r["result"], json!({"ok": true}));
        assert_eq!(
            fx,
            AgentEffects {
                replaced: true,
                path_changed: true
            }
        );
        assert_eq!(path.as_deref(), Some(other.as_path()));
        assert!(s.editor.doc().objects.is_empty());
        assert!(
            !s.take_replaced(),
            "the flag was consumed by the app handler"
        );

        // Errors: missing path, missing file, newer format. The old state stays.
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 4, "method": "file.open", "params": {}}),
        );
        assert_eq!(r["error"]["code"], -32602);
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 5, "method": "file.open", "params": {"path": dir.join("nope")}}),
        );
        assert_eq!(r["error"]["code"], 2);
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["format_version"] = json!(FORMAT_VERSION + 1);
        std::fs::write(dir.join("new.json"), v.to_string()).unwrap();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 6, "method": "file.open", "params": {"path": dir.join("new.json")}}),
        );
        assert!(r["error"]["message"].as_str().unwrap().contains("newer"));
        assert_eq!(path.as_deref(), Some(other.as_path()));
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 7, "method": "file.save", "params": {"path": dir.join("no").join("such").join("f")}}),
        );
        assert_eq!(r["error"]["code"], 1);

        // A notification gets no reply.
        let mut fx = AgentEffects::default();
        assert!(
            agent_line(
                &mut s,
                &mut path,
                0,
                &json!({"method": "file.save", "params": {"path": p}}).to_string(),
                &mut fx
            )
            .is_none()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_document_load_clears_the_file_path_and_tells_the_app() {
        let mut s = Session::default();
        let mut path = Some(PathBuf::from("/tmp/old.kad"));
        let mut fx = AgentEffects::default();
        let doc = serde_json::to_value(Document::default()).unwrap();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 1, "method": "document.load", "params": {"document": doc}}),
        );
        assert_eq!(r["result"], json!({"ok": true}));
        assert_eq!(path, None, "the new document is not the file that was open");
        assert_eq!(
            fx,
            AgentEffects {
                replaced: true,
                path_changed: true
            }
        );

        // A refused load changes nothing.
        let mut path = Some(PathBuf::from("/tmp/keep.kad"));
        let mut fx = AgentEffects::default();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 2, "method": "document.load", "params": {"document": {"format_version": 99}}}),
        );
        assert!(r.get("error").is_some());
        assert_eq!(path, Some(PathBuf::from("/tmp/keep.kad")));
        assert_eq!(fx, AgentEffects::default());

        // Lines that are not JSON still get the normal parse error.
        let line = agent_line(&mut s, &mut path, 0, "{nope", &mut fx).unwrap();
        assert!(line.contains("-32700"));
    }

    #[test]
    fn agent_can_render_and_switch_the_view_in_the_live_app() {
        let mut s = Session::default();
        let mut path = None;
        let mut fx = AgentEffects::default();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 1, "method": "view.set", "params": {"view": "front", "mode": "wireframe"}}),
        );
        assert_eq!(
            r["result"],
            json!({"view": "front", "projection": "ortho", "mode": "wireframe"})
        );
        assert_eq!(
            fx,
            AgentEffects::default(),
            "the app polls the session for view changes"
        );
        assert!(s.take_view_changed());
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 2, "method": "render.png", "params": {"width": 32, "height": 32}}),
        );
        assert_eq!(r["result"]["view"], "front");
        assert!(
            r["result"]["png_base64"]
                .as_str()
                .unwrap()
                .starts_with("iVBOR")
        );
    }
}
