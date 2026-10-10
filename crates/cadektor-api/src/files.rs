//! `file.open` and `file.save`: one handler for `cadektor serve`, `cadektor mcp` and the app.
//! Native only: it reads and writes files. The path policy of the session applies.
//!
//! A document file is the JSON of `Document`. The extension is `.cdk` (the main format) or the old
//! `.cdk.json` (a path that ends in `.json`). Saving keeps the old file as `.bak` and is atomic.

use std::path::{Path, PathBuf};

use cadektor_core::fsio;
use cadektor_core::model::{Document, LoadError};
use serde_json::{Value, json};

use crate::{RpcError, Session};

/// The extensions of a document file (lower case, no dot). `.cdk.json` ends in `json`.
pub const DOC_EXTS: [&str; 2] = ["cdk", "json"];

/// Stores the bytes of a save: the path and the bytes. The error is a message.
pub type WriteFn<'a> = dyn FnMut(&Path, &[u8]) -> Result<(), String> + 'a;

/// What `file_call` did.
#[derive(Debug, PartialEq)]
pub enum FileDone {
    Opened(PathBuf),
    Saved(PathBuf),
}

impl FileDone {
    pub fn path(&self) -> &Path {
        match self {
            FileDone::Opened(p) | FileDone::Saved(p) => p,
        }
    }
}

/// The error for a document that cannot be loaded.
fn load_error(path: &Path, e: LoadError) -> RpcError {
    let m = format!("{}: {e}", path.display());
    match e {
        LoadError::Newer(_) => RpcError::new(crate::errors::CODE_VALIDATION, "newer_format", m),
        _ => RpcError::validation(m),
    }
}

/// Reads and checks a document file.
pub fn read_document(path: &Path) -> Result<Document, RpcError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        let m = format!("cannot read {}: {e}", path.display());
        if e.kind() == std::io::ErrorKind::NotFound {
            RpcError::not_found(m)
        } else {
            RpcError::app(m)
        }
    })?;
    Document::from_json(&text).map_err(|e| load_error(path, e))
}

/// The text of a document file.
pub fn document_text(doc: &Document) -> Result<String, RpcError> {
    serde_json::to_string_pretty(doc).map_err(RpcError::app)
}

/// The folder of a document file, canonical if possible.
fn dir_of(file: &Path) -> Option<PathBuf> {
    let dir = file
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    std::fs::canonicalize(dir).ok()
}

/// Runs `file.save {path}` or `file.open {path}` on the session.
///
/// `write` stores the bytes of a save. The headless host passes `fsio::write_atomic`
/// (see [`write_document_file`]). The app adds its autosave bookkeeping around it.
/// After an open, the session has the new document and `take_replaced` is already cleared:
/// the host decides what else to refresh.
pub fn file_call(
    s: &mut Session,
    method: &str,
    params: &Value,
    write: &mut WriteFn<'_>,
) -> Result<FileDone, RpcError> {
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("params.path is required"))?;
    let saving = method == "file.save";
    let p = s
        .check_path(Path::new(path), saving, &DOC_EXTS)
        .map_err(RpcError::policy)?;
    if saving {
        let text = document_text(s.editor.doc())?;
        write(&p, text.as_bytes()).map_err(RpcError::app)?;
        s.doc_dir = dir_of(&p);
        Ok(FileDone::Saved(p))
    } else {
        let doc = read_document(&p)?;
        s.replace_document(doc);
        s.take_replaced();
        s.doc_dir = dir_of(&p);
        Ok(FileDone::Opened(p))
    }
}

/// Atomic save with a `.bak` of the old file.
pub fn write_document_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fsio::write_atomic(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// The JSON-RPC response for the result of `file_call`.
pub fn file_reply(id: Value, result: Result<FileDone, RpcError>) -> String {
    match result {
        Ok(_) => json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}}).to_string(),
        Err(e) => crate::errors::error_response(id, e.code, e.kind, &e.message),
    }
}

/// The whole of `file.open` and `file.save` for a headless host: parse the request, run it,
/// write the response.
pub fn handle_file_request(s: &mut Session, req: &Value) -> String {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req["method"].as_str().unwrap_or_default();
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let r = file_call(s, method, &params, &mut write_document_file);
    file_reply(id, r)
}

/// True for the methods that `file_call` handles.
pub fn is_file_method(method: &str) -> bool {
    matches!(method, "file.save" | "file.open")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathPolicy;
    use cadektor_core::command::Command;
    use cadektor_core::model::Shape;
    use glam::DVec2;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cdk-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    fn restricted(dir: &Path) -> Session {
        Session {
            paths: PathPolicy::restricted([dir.to_path_buf()]),
            ..Session::default()
        }
    }

    fn add_circle(s: &mut Session) {
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::ZERO,
                    radius: 5.0,
                },
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
    }

    fn call(s: &mut Session, method: &str, path: &Path) -> Value {
        let req = json!({"jsonrpc":"2.0","id":1,"method":method,"params":{"path":path}});
        serde_json::from_str(&handle_file_request(s, &req)).unwrap()
    }

    #[test]
    fn saves_and_opens_cdk_and_old_cdk_json() {
        let dir = tmp("both");
        for name in ["plan.cdk", "plan.cdk.json", "plan.json"] {
            let mut s = restricted(&dir);
            add_circle(&mut s);
            let f = dir.join(name);
            assert_eq!(
                call(&mut s, "file.save", &f)["result"]["ok"],
                true,
                "{name}"
            );
            let mut t = restricted(&dir);
            assert_eq!(
                call(&mut t, "file.open", &f)["result"]["ok"],
                true,
                "{name}"
            );
            assert_eq!(t.editor.doc().objects.len(), 1);
            assert_eq!(t.doc_dir.as_deref(), Some(dir.as_path()));
        }
    }

    #[test]
    fn a_second_save_keeps_the_first_as_bak() {
        let dir = tmp("bak");
        let mut s = restricted(&dir);
        let f = dir.join("a.cdk");
        call(&mut s, "file.save", &f);
        let first = std::fs::read_to_string(&f).unwrap();
        add_circle(&mut s);
        call(&mut s, "file.save", &f);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.cdk.bak")).unwrap(),
            first
        );
        assert_ne!(std::fs::read_to_string(&f).unwrap(), first);
    }

    #[test]
    fn the_policy_and_the_extension_apply() {
        let dir = tmp("policy");
        let other = tmp("policy-other");
        let mut s = restricted(&dir);
        let r = call(&mut s, "file.save", &other.join("x.cdk"));
        assert_eq!(r["error"]["code"], 4);
        assert_eq!(r["error"]["data"]["kind"], "policy");
        let r = call(&mut s, "file.save", &dir.join("x.txt"));
        assert_eq!(r["error"]["code"], 4);
        // An unrestricted session (a library user) may use any path and extension.
        let mut u = Session::default();
        assert_eq!(
            call(&mut u, "file.save", &other.join("x.cdk"))["result"]["ok"],
            true
        );
    }

    #[test]
    fn errors_have_codes() {
        let dir = tmp("errors");
        let mut s = restricted(&dir);
        let r = call(&mut s, "file.open", &dir.join("missing.cdk"));
        assert_eq!(
            r["error"]["code"], 4,
            "a missing file cannot be resolved: {r}"
        );
        let mut u = Session::default();
        let r = call(&mut u, "file.open", &dir.join("missing.cdk"));
        assert_eq!(r["error"]["code"], 2);
        let bad = dir.join("bad.cdk");
        std::fs::write(&bad, "{nope").unwrap();
        let r = call(&mut u, "file.open", &bad);
        assert_eq!(r["error"]["code"], 3);
        let req = json!({"jsonrpc":"2.0","id":2,"method":"file.open","params":{}});
        let r: Value = serde_json::from_str(&handle_file_request(&mut u, &req)).unwrap();
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn a_newer_file_says_so() {
        let dir = tmp("newer");
        let f = dir.join("future.cdk");
        std::fs::write(&f, r#"{"format_version":99,"layers":"???"}"#).unwrap();
        let mut s = Session::default();
        let r = call(&mut s, "file.open", &f);
        let m = r["error"]["message"].as_str().unwrap();
        assert!(
            m.contains("file was made by a newer Cadektor (format 99)"),
            "{m}"
        );
        assert!(!m.contains("not a Cadektor file"), "{m}");
        assert_eq!(r["error"]["data"]["kind"], "newer_format");
    }

    #[test]
    fn open_replaces_the_document_and_clears_the_flag() {
        let dir = tmp("replace");
        let mut a = restricted(&dir);
        add_circle(&mut a);
        let f = dir.join("a.cdk");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        assert!(!b.take_replaced());
        assert_eq!(b.editor.doc().objects.len(), 1);
        assert!(FileDone::Opened(f.clone()).path().ends_with("a.cdk"));
        assert!(is_file_method("file.open") && !is_file_method("render.svg"));
    }
}
