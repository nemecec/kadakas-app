//! `file.open` and `file.save`: one handler for `kadakas serve`, `kadakas mcp` and the app.
//! Native only: it reads and writes files. The path policy of the session applies.
//!
//! A document file is the JSON of `Document`. The extension is `.kad`. Saving keeps the old file as `.bak` and
//! is atomic. Both ways stream (DOC-19): a load parses from a buffered reader and a save writes
//! through a buffered writer into the temporary file, so the text of the file is never in memory.

use std::path::{Path, PathBuf};

use kadakas_core::model::{Document, LoadError};
use kadakas_sys::fsio;
use serde_json::{Value, json};

use crate::limits::DEFAULT_DOCUMENT_FILE_BYTES;
use crate::{RpcError, Session};

/// The extensions of a document file (lower case, no dot).
pub const DOC_EXTS: [&str; 1] = ["kad"];

/// Stores a save: the path and the document. The function streams the document into the file
/// (see [`write_document_file`]). The error is a message.
pub type WriteFn<'a> = dyn FnMut(&Path, &Document) -> Result<(), String> + 'a;

/// What `file_call` did.
#[derive(Debug, PartialEq)]
pub enum FileDone {
    /// The path, and what the load repaired (the load notes of the document).
    Opened(PathBuf, Vec<String>),
    Saved(PathBuf),
}

impl FileDone {
    pub fn path(&self) -> &Path {
        match self {
            FileDone::Opened(p, _) | FileDone::Saved(p) => p,
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

/// Reads and checks a document file of at most [`DEFAULT_DOCUMENT_FILE_BYTES`].
pub fn read_document(path: &Path) -> Result<Document, RpcError> {
    read_document_capped(path, DEFAULT_DOCUMENT_FILE_BYTES)
}

/// Reads and checks a document file. Only a regular file is read, and a file larger than
/// `max_bytes` is refused before it is read. The file is parsed from a buffered reader: the text
/// is never held in memory as a whole.
pub fn read_document_capped(path: &Path, max_bytes: u64) -> Result<Document, RpcError> {
    use fsio::OpenError;
    let reader = fsio::open_regular(path, max_bytes).map_err(|e| {
        let m = format!("cannot read {}: {e}", path.display());
        match e {
            OpenError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => RpcError::not_found(m),
            OpenError::Io(_) => RpcError::app(m),
            OpenError::NotRegular | OpenError::TooLarge { .. } => RpcError::validation(m),
        }
    })?;
    Document::from_reader(reader).map_err(|e| load_error(path, e))
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
/// `write` stores a save. The headless host passes [`write_document_file`]. The app adds its
/// autosave bookkeeping around it.
/// After an open, the session has the new document and `take_replaced` is already cleared:
/// the host decides what else to refresh.
pub fn file_call(
    s: &mut Session,
    method: &str,
    params: &Value,
    write: &mut WriteFn<'_>,
) -> Result<FileDone, RpcError> {
    // A panic in a reader or a writer becomes an internal error, as for every other method. The
    // call and its error go to the call log of the session, so a bug report has them.
    let note = crate::feedback_api::begin(method, params);
    let r = s.guarded(method, |s| file_call_inner(s, method, params, write));
    let ring = match &r {
        Ok(_) => Ok(Value::Null),
        Err(e) => Err(RpcError::new(e.code, e.kind, &e.message)),
    };
    crate::feedback_api::finish(s, note, &ring);
    r
}

fn file_call_inner(
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
        let doc = s.document_for_save()?;
        write(&p, &doc).map_err(RpcError::app)?;
        s.collab_mark_saved();
        s.doc_dir = dir_of(&p);
        Ok(FileDone::Saved(p))
    } else {
        let doc = read_document_capped(&p, s.limits.document_file_bytes)?;
        s.check_document_fits(&doc)?;
        let notes = s.replace_document(doc);
        s.take_replaced();
        s.doc_dir = dir_of(&p);
        Ok(FileDone::Opened(p, notes))
    }
}

/// Atomic save with a `.bak` of the old file. The document is streamed, pretty-printed, into a
/// buffered writer on the temporary file; the text of the document is never built.
pub fn write_document_file(path: &Path, doc: &Document) -> Result<(), String> {
    fsio::write_atomic_with(path, |w| doc.write_pretty(w))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// The JSON-RPC response for the result of `file_call`. An error carries the feedback hint and
/// the `tainted` mark, like the errors of every other method.
pub fn file_reply(
    s: &Session,
    method: &str,
    id: Value,
    result: Result<FileDone, RpcError>,
) -> String {
    match result {
        // Like `document.load`: what the load repaired instead of refusing the file.
        Ok(FileDone::Opened(_, notes)) if !notes.is_empty() => {
            json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true, "notes": notes}}).to_string()
        }
        Ok(_) => json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}}).to_string(),
        Err(e) => {
            let hint = crate::feedback_api::hint(method, e.kind, &e.message);
            crate::errors::error_response_with(
                id,
                e.code,
                e.kind,
                &e.message,
                hint,
                s.is_tainted(),
                e.details.as_ref(),
            )
        }
    }
}

/// The whole of `file.open` and `file.save` for a headless host: parse the request, run it,
/// write the response.
pub fn handle_file_request(s: &mut Session, req: &Value) -> String {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req["method"].as_str().unwrap_or_default();
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let r = file_call(s, method, &params, &mut write_document_file);
    file_reply(s, method, id, r)
}

/// True for the methods that `file_call` handles.
pub fn is_file_method(method: &str) -> bool {
    matches!(method, "file.save" | "file.open")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathPolicy;
    use glam::DVec2;
    use kadakas_core::command::Command;
    use kadakas_core::model::Shape;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kad-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    fn restricted(dir: &Path) -> Session {
        Session {
            paths: PathPolicy::restricted([dir.to_path_buf()]),
            ..Session::with_sequential_ids()
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
    fn saves_and_opens_kad_files() {
        let dir = tmp("both");
        let name = "plan.kad";
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

    #[test]
    fn a_second_save_keeps_the_first_as_bak() {
        let dir = tmp("bak");
        let mut s = restricted(&dir);
        let f = dir.join("a.kad");
        call(&mut s, "file.save", &f);
        let first = std::fs::read_to_string(&f).unwrap();
        add_circle(&mut s);
        call(&mut s, "file.save", &f);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.kad.bak")).unwrap(),
            first
        );
        assert_ne!(std::fs::read_to_string(&f).unwrap(), first);
    }

    #[test]
    fn the_policy_and_the_extension_apply() {
        let dir = tmp("policy");
        let other = tmp("policy-other");
        let mut s = restricted(&dir);
        let r = call(&mut s, "file.save", &other.join("x.kad"));
        assert_eq!(r["error"]["code"], 4);
        assert_eq!(r["error"]["data"]["kind"], "policy");
        let r = call(&mut s, "file.save", &dir.join("x.txt"));
        assert_eq!(r["error"]["code"], 4);
        // An unrestricted session (a library user) may use any path and extension.
        let mut u = Session::with_sequential_ids();
        assert_eq!(
            call(&mut u, "file.save", &other.join("x.kad"))["result"]["ok"],
            true
        );
    }

    #[test]
    fn errors_have_codes() {
        let dir = tmp("errors");
        let mut s = restricted(&dir);
        let r = call(&mut s, "file.open", &dir.join("missing.kad"));
        assert_eq!(
            r["error"]["code"], 4,
            "a missing file cannot be resolved: {r}"
        );
        let mut u = Session::with_sequential_ids();
        let r = call(&mut u, "file.open", &dir.join("missing.kad"));
        assert_eq!(r["error"]["code"], 2);
        let bad = dir.join("bad.kad");
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
        let f = dir.join("future.kad");
        std::fs::write(&f, r#"{"format_version":99,"layers":"???"}"#).unwrap();
        let mut s = Session::with_sequential_ids();
        let r = call(&mut s, "file.open", &f);
        let m = r["error"]["message"].as_str().unwrap();
        assert!(
            m.contains("file was made by a newer Kadakas (format 99)"),
            "{m}"
        );
        assert!(!m.contains("not a Kadakas file"), "{m}");
        assert_eq!(r["error"]["data"]["kind"], "newer_format");
    }

    #[test]
    fn open_replaces_the_document_and_clears_the_flag() {
        let dir = tmp("replace");
        let mut a = restricted(&dir);
        add_circle(&mut a);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        assert!(!b.take_replaced());
        assert_eq!(b.editor.doc().objects.len(), 1);
        assert!(
            FileDone::Opened(f.clone(), Vec::new())
                .path()
                .ends_with("a.kad")
        );
        assert!(is_file_method("file.open") && !is_file_method("render.svg"));
    }

    /// A hook that streams like `write_document_file`, and fails after `limit` bytes.
    fn failing_hook(limit: usize) -> impl FnMut(&Path, &Document) -> Result<(), String> {
        move |p, doc| {
            struct Cut(usize);
            impl std::io::Write for Cut {
                fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                    if self.0 < b.len() {
                        return Err(std::io::Error::other("disk full"));
                    }
                    self.0 -= b.len();
                    Ok(b.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            fsio::write_atomic_with(p, |w| {
                // Some real bytes reach the temporary file before the failure.
                w.write_all(b"{\"half\":")?;
                doc.write_pretty(Cut(limit))
            })
            .map_err(|e| e.to_string())
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn a_save_is_the_pretty_text_and_a_round_trip() {
        let dir = tmp("bytes");
        let mut s = restricted(&dir);
        add_circle(&mut s);
        let f = dir.join("a.kad");
        assert_eq!(call(&mut s, "file.save", &f)["result"]["ok"], true);
        let on_disk = std::fs::read(&f).unwrap();
        let text = serde_json::to_string_pretty(s.editor.doc()).unwrap();
        assert_eq!(on_disk, text.as_bytes(), "byte for byte the old output");
        assert_eq!(
            on_disk,
            serde_json::to_string_pretty(s.editor.doc())
                .unwrap()
                .into_bytes()
        );
        assert_eq!(read_document(&f).unwrap(), s.editor.doc().clone());
        assert_eq!(names(&dir), ["a.kad"], "no temporary file is left");
    }

    #[cfg(unix)]
    #[test]
    fn a_save_keeps_the_permissions_of_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp("perm");
        let mut s = restricted(&dir);
        let f = dir.join("a.kad");
        call(&mut s, "file.save", &f);
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
        add_circle(&mut s);
        call(&mut s, "file.save", &f);
        let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temporary_file() {
        let dir = tmp("fail");
        let mut s = restricted(&dir);
        let f = dir.join("a.kad");
        call(&mut s, "file.save", &f);
        let old = std::fs::read(&f).unwrap();
        add_circle(&mut s);
        for limit in [0, 40, 400] {
            let params = json!({"path": f});
            let e = file_call(&mut s, "file.save", &params, &mut failing_hook(limit)).unwrap_err();
            assert!(e.message.contains("disk full"), "{}", e.message);
            assert_eq!(std::fs::read(&f).unwrap(), old, "limit {limit}");
            assert_eq!(names(&dir), ["a.kad"], "limit {limit}");
        }
        // The same save with a good writer works.
        assert_eq!(call(&mut s, "file.save", &f)["result"]["ok"], true);
        assert_ne!(std::fs::read(&f).unwrap(), old);
    }

    #[test]
    fn a_damaged_or_hostile_file_is_an_error_with_a_code() {
        let dir = tmp("hostile");
        let mut u = Session::with_sequential_ids();
        add_circle(&mut u);
        let good = dir.join("good.kad");
        call(&mut u, "file.save", &good);
        let text = std::fs::read(&good).unwrap();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("cut", text[..text.len() / 2].to_vec()),
            ("cut-last", text[..text.len() - 1].to_vec()),
            ("empty", Vec::new()),
            ("binary", vec![0xff, 0xfe, 0, 1, 2]),
            ("deep", vec![b'['; 200_000]),
            ("array", b"[1,2,3]".to_vec()),
        ];
        for (name, bytes) in cases {
            let f = dir.join(format!("{name}.kad"));
            std::fs::write(&f, bytes).unwrap();
            let before = u.editor.doc().clone();
            let r = call(&mut u, "file.open", &f);
            assert_eq!(r["error"]["code"], 3, "{name}: {r}");
            assert!(
                r["error"]["message"].as_str().unwrap().contains(name),
                "{name}: {r}"
            );
            assert_eq!(u.editor.doc(), &before, "{name}: the document stays");
        }
    }

    #[test]
    fn a_file_above_the_size_limit_is_refused_before_it_is_read() {
        let dir = tmp("cap");
        let mut s = restricted(&dir);
        let f = dir.join("a.kad");
        call(&mut s, "file.save", &f);
        let len = std::fs::metadata(&f).unwrap().len();
        s.limits.document_file_bytes = len - 1;
        let r = call(&mut s, "file.open", &f);
        assert_eq!(r["error"]["code"], 3, "{r}");
        let m = r["error"]["message"].as_str().unwrap();
        assert!(m.contains("limit") && m.contains(&len.to_string()), "{m}");
        s.limits.document_file_bytes = len;
        assert_eq!(call(&mut s, "file.open", &f)["result"]["ok"], true);
        // A file of the size of a lie: only the header of the file counts, nothing is read.
        let big = dir.join("big.kad");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(DEFAULT_DOCUMENT_FILE_BYTES + 1).unwrap();
        let e = read_document(&big).unwrap_err();
        assert!(e.message.contains("limit"), "{}", e.message);
        assert!(crate::SessionLimits::strict().document_file_bytes < DEFAULT_DOCUMENT_FILE_BYTES);
    }

    #[test]
    fn the_library_writes_with_the_same_streaming_save() {
        let dir = tmp("lib");
        let f = dir.join("lib.kad");
        write_document_file(&f, &Document::with_sequential_ids()).unwrap();
        assert!(write_document_file(&dir.join("no").join("x.kad"), &Document::default()).is_err());
        assert_eq!(read_document(&f).unwrap(), Document::with_sequential_ids());
    }
}
