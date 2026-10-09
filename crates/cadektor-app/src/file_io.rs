//! Document files: JSON of `Document`, extension `.cdk.json`.

use std::path::{Path, PathBuf};

use cadektor_core::api::Session;
use cadektor_core::fsio;
use cadektor_core::model::Document;
use serde_json::{Value, json};

pub fn parse_document(text: &str) -> Result<Document, String> {
    Document::from_json(text).map_err(|e| e.to_string())
}

pub fn read_document(path: &Path) -> Result<Document, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_document(&text)
}

/// Saves atomically (temporary file, then rename) and keeps the old file as `.bak`.
pub fn write_document(path: &Path, doc: &Document) -> Result<(), String> {
    let text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    fsio::write_atomic(path, text.as_bytes()).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn write_svg(path: &Path, doc: &Document) -> Result<(), String> {
    fsio::write_atomic(path, cadektor_core::svg::render(doc, 1600).as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Adds `.cdk.json` unless the name already has it.
pub fn with_extension(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s.ends_with(".cdk.json") { path.to_path_buf() } else { format!("{s}.cdk.json").into() }
}

/// What an agent request changed in the app besides the document.
#[derive(Debug, Default, PartialEq)]
pub struct AgentEffects {
    /// The document was replaced: the app must cancel the active tool and refit the view.
    pub replaced: bool,
    /// The file path changed: the app must update the window title.
    pub path_changed: bool,
}

/// Answers one request line from an agent. Like `Session::handle_line`, and
/// also `file.save {path}` and `file.open {path}`, which need the app's file path.
pub fn agent_line(session: &mut Session, path: &mut Option<PathBuf>, line: &str, fx: &mut AgentEffects) -> Option<String> {
    let req: Option<Value> = serde_json::from_str(line).ok();
    let method = req.as_ref().and_then(|r| r["method"].as_str()).filter(|m| matches!(*m, "file.save" | "file.open"));
    let (Some(req), Some(method)) = (&req, method) else {
        let resp = session.handle_line(line);
        if session.take_replaced() {
            // `document.load`: the new document does not come from the file that was open.
            *path = None;
            fx.replaced = true;
            fx.path_changed = true;
        }
        return resp;
    };
    let result = match req["params"]["path"].as_str().map(PathBuf::from) {
        None => Err((-32602, "params.path is required".to_string())),
        Some(p) if method == "file.save" => write_document(&p, &session.editor.doc).map_err(|e| (1, e)).map(|()| {
            *path = Some(p);
            fx.path_changed = true;
        }),
        Some(p) => read_document(&p).map_err(|e| (1, e)).map(|doc| {
            session.replace_document(doc);
            session.take_replaced();
            *path = Some(p);
            fx.replaced = true;
            fx.path_changed = true;
        }),
    };
    let id = req.get("id")?.clone();
    Some(match result {
        Ok(()) => json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    }
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::api::Session;
    use cadektor_core::command::Command;
    use cadektor_core::model::{Attributes, FORMAT_VERSION, Shape};
    use glam::DVec2;

    #[test]
    fn save_and_load_roundtrip() {
        let mut s = Session::default();
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Circle { center: DVec2::new(1.0, 2.0), radius: 3.0 },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let path = std::env::temp_dir().join(format!("cadektor-test-{}.cdk.json", std::process::id()));
        write_document(&path, &s.editor.doc).unwrap();
        assert_eq!(read_document(&path).unwrap(), s.editor.doc);
        let svg = std::env::temp_dir().join(format!("cadektor-test-{}.svg", std::process::id()));
        write_svg(&svg, &s.editor.doc).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains("<circle"));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(svg);
    }

    #[test]
    fn rejects_garbage_and_newer_formats() {
        assert!(parse_document("{nope").is_err());
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        assert!(parse_document(&v.to_string()).unwrap_err().contains("newer"));
    }

    #[test]
    fn extension_helper() {
        assert_eq!(with_extension(Path::new("a")).to_str(), Some("a.cdk.json"));
        assert_eq!(with_extension(Path::new("a.cdk.json")).to_str(), Some("a.cdk.json"));
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadektor-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn call(s: &mut Session, path: &mut Option<PathBuf>, fx: &mut AgentEffects, req: Value) -> Value {
        serde_json::from_str(&agent_line(s, path, &req.to_string(), fx).unwrap()).unwrap()
    }

    #[test]
    fn saving_keeps_a_backup_and_leaves_no_temp_files() {
        let dir = temp("save");
        let p = dir.join("a.cdk.json");
        let mut doc = Document::default();
        write_document(&p, &doc).unwrap();
        doc.layers[0].name = "Second".into();
        write_document(&p, &doc).unwrap();
        assert_eq!(read_document(&p).unwrap().layers[0].name, "Second");
        assert_eq!(read_document(&dir.join("a.cdk.json.bak")).unwrap().layers[0].name, "Layer 1");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_file_save_and_open_use_the_app_path() {
        let dir = temp("agent-files");
        let p = dir.join("plan.cdk.json");
        let mut s = Session::default();
        let mut path = None;
        let mut fx = AgentEffects::default();
        let circle = json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}});
        call(&mut s, &mut path, &mut fx, json!({"id": 1, "method": "execute", "params": {"command": circle}}));
        assert_eq!(fx, AgentEffects::default(), "normal requests change nothing in the app");

        let r = call(&mut s, &mut path, &mut fx, json!({"id": 2, "method": "file.save", "params": {"path": p}}));
        assert_eq!(r["result"], json!({"ok": true}));
        assert_eq!(path.as_deref(), Some(p.as_path()));
        assert_eq!(fx, AgentEffects { replaced: false, path_changed: true });
        assert_eq!(read_document(&p).unwrap().objects.len(), 1);

        // Open a different document: the session is replaced and the app is told.
        let other = dir.join("other.cdk.json");
        write_document(&other, &Document::default()).unwrap();
        let mut fx = AgentEffects::default();
        let r = call(&mut s, &mut path, &mut fx, json!({"id": 3, "method": "file.open", "params": {"path": other}}));
        assert_eq!(r["result"], json!({"ok": true}));
        assert_eq!(fx, AgentEffects { replaced: true, path_changed: true });
        assert_eq!(path.as_deref(), Some(other.as_path()));
        assert!(s.editor.doc.objects.is_empty());
        assert!(!s.take_replaced(), "the flag was consumed by the app handler");

        // Errors: missing path, missing file, newer format. The old state stays.
        let r = call(&mut s, &mut path, &mut fx, json!({"id": 4, "method": "file.open", "params": {}}));
        assert_eq!(r["error"]["code"], -32602);
        let r = call(&mut s, &mut path, &mut fx, json!({"id": 5, "method": "file.open", "params": {"path": dir.join("nope")}}));
        assert_eq!(r["error"]["code"], 1);
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["format_version"] = json!(FORMAT_VERSION + 1);
        std::fs::write(dir.join("new.json"), v.to_string()).unwrap();
        let r = call(&mut s, &mut path, &mut fx, json!({"id": 6, "method": "file.open", "params": {"path": dir.join("new.json")}}));
        assert!(r["error"]["message"].as_str().unwrap().contains("newer"));
        assert_eq!(path.as_deref(), Some(other.as_path()));
        let r = call(&mut s, &mut path, &mut fx, json!({"id": 7, "method": "file.save", "params": {"path": dir.join("no").join("such").join("f")}}));
        assert_eq!(r["error"]["code"], 1);

        // A notification gets no reply.
        let mut fx = AgentEffects::default();
        assert!(agent_line(&mut s, &mut path, &json!({"method": "file.save", "params": {"path": p}}).to_string(), &mut fx).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_document_load_clears_the_file_path_and_tells_the_app() {
        let mut s = Session::default();
        let mut path = Some(PathBuf::from("/tmp/old.cdk.json"));
        let mut fx = AgentEffects::default();
        let doc = serde_json::to_value(Document::default()).unwrap();
        let r = call(&mut s, &mut path, &mut fx, json!({"id": 1, "method": "document.load", "params": {"document": doc}}));
        assert_eq!(r["result"], json!({"ok": true}));
        assert_eq!(path, None, "the new document is not the file that was open");
        assert_eq!(fx, AgentEffects { replaced: true, path_changed: true });

        // A refused load changes nothing.
        let mut path = Some(PathBuf::from("/tmp/keep.cdk.json"));
        let mut fx = AgentEffects::default();
        let r = call(&mut s, &mut path, &mut fx, json!({"id": 2, "method": "document.load", "params": {"document": {"format_version": 99}}}));
        assert!(r.get("error").is_some());
        assert_eq!(path, Some(PathBuf::from("/tmp/keep.cdk.json")));
        assert_eq!(fx, AgentEffects::default());

        // Lines that are not JSON still get the normal parse error.
        let line = agent_line(&mut s, &mut path, "{nope", &mut fx).unwrap();
        assert!(line.contains("-32700"));
    }
}
