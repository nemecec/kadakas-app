//! `file.open` and `file.save`: one handler for `kadakas serve`, `kadakas mcp` and the app.
//! Native only: it reads and writes files. The path policy of the session applies.
//!
//! A document file has one of two forms (DOC-20, DOC-22), told by the first bytes when it is
//! opened: the container (a ZIP file; the extension is `.kad`) and the plain JSON form (`.json`).
//! `file.save {path, format?}` writes `container` or `json`; without `format` the extension
//! decides (`.json` is JSON, anything else the container). A container keeps its pictures in the
//! file (DOC-21): `file.open` leaves them there, and the session keeps the file as its source
//! (`Session::file_source`), so a save copies the unchanged parts without decoding them.
//! Saving keeps the old file as `.bak` and is atomic. Both ways stream (DOC-19): a load parses from
//! a reader and a save writes through a buffered writer into the temporary file, so the text of
//! the file is never in memory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kadakas_core::model::{Document, LoadError};
use kadakas_file::native;
use kadakas_file::{ContainerError, FileError, FileSource, Form, Opened, ReadMode, WriteOptions};
use serde_json::{Value, json};

use crate::limits::DEFAULT_DOCUMENT_FILE_BYTES;
use crate::{RpcError, Session};

/// The extensions of a document file (lower case, no dot): the container and the plain JSON form.
pub const DOC_EXTS: [&str; 2] = ["kad", "json"];

/// How a save writes: the form, and the files the document was opened from (unchanged parts are
/// copied from them).
#[derive(Clone, Debug)]
pub struct SaveSpec {
    pub form: Form,
    pub keep: Vec<Arc<FileSource>>,
}

impl SaveSpec {
    /// The form of the extension of `path` (`.json` is the plain JSON form, anything else the
    /// container), with the files to copy from.
    pub fn from_extension(path: &Path, keep: Vec<Arc<FileSource>>) -> SaveSpec {
        SaveSpec {
            form: Form::from_extension(path),
            keep,
        }
    }

    /// The container, with nothing to copy from.
    pub fn container() -> SaveSpec {
        SaveSpec {
            form: Form::Container,
            keep: Vec::new(),
        }
    }
}

/// Stores a save: the path, the document and how to write it. The function streams the document
/// into the file (see [`write_document`]).
pub type WriteFn<'a> = dyn FnMut(&Path, &Document, &SaveSpec) -> Result<(), FileError> + 'a;

/// What `file_call` did.
#[derive(Debug, PartialEq)]
pub enum FileDone {
    /// The path, what the load repaired (the load notes of the document) and the form of the file.
    Opened(PathBuf, Vec<String>, Form),
    /// The path and the form that was written.
    Saved(PathBuf, Form),
}

impl FileDone {
    pub fn path(&self) -> &Path {
        match self {
            FileDone::Opened(p, ..) | FileDone::Saved(p, _) => p,
        }
    }

    pub fn form(&self) -> Form {
        match self {
            FileDone::Opened(_, _, f) | FileDone::Saved(_, f) => *f,
        }
    }
}

/// What the error of a file over the size limit says about raising the limit.
const RAISE_HINT: &str = ". To open it anyway, raise the limit: start `kadakas serve` or `kadakas mcp` with --max-document-mib <MiB> (or set KADAKAS_MAX_DOCUMENT_MIB), or set document_file_bytes in SessionLimits";

/// The error for a file that cannot be read.
fn read_error(path: &Path, e: FileError) -> RpcError {
    let m = format!("{}: {e}", path.display());
    match e {
        FileError::NotFound(msg) => RpcError::not_found(msg),
        FileError::NotRegular(msg) => RpcError::validation(msg),
        FileError::TooLarge { len, max } => RpcError::validation(format!(
            "cannot read {}: the file is {len} bytes; the limit is {max} bytes{RAISE_HINT}",
            path.display()
        )),
        FileError::Load(LoadError::Newer(_)) => {
            RpcError::new(crate::errors::CODE_VALIDATION, "newer_format", m)
        }
        // The bytes of the entries (uncompressed) are over the limit, though the file is not.
        FileError::Container(ContainerError::Limit(_)) => {
            RpcError::validation(format!("{m}{RAISE_HINT}"))
        }
        FileError::Container(ContainerError::Changed(msg)) => RpcError::conflict(msg),
        FileError::Io(msg) => RpcError::app(msg),
        _ => RpcError::validation(m),
    }
}

/// The error for a file that cannot be written. The old file is as it was.
fn write_error(path: &Path, e: FileError) -> RpcError {
    match e {
        FileError::Container(ContainerError::Changed(msg)) => RpcError::conflict(msg),
        FileError::Container(ContainerError::Limit(msg)) | FileError::Limit(msg) => {
            RpcError::limit(format!("cannot write {}: {msg}", path.display()))
        }
        FileError::Io(msg) => RpcError::app(msg),
        other => RpcError::app(format!("cannot write {}: {other}", path.display())),
    }
}

/// Opens a document file of either form. Only a regular file is read, and a file larger than
/// `max_bytes` is refused before it is read. A container is also refused when the sum of the
/// sizes of its entries (uncompressed, from the manifest) is over `max_bytes`: a small file must
/// not hide a big document. With [`ReadMode::Lazy`] the pictures of a container stay in the file
/// and the result keeps the open file (`Opened::source`).
pub fn open_document(path: &Path, max_bytes: u64, mode: ReadMode) -> Result<Opened, RpcError> {
    native::open_path(path, max_bytes, mode).map_err(|e| read_error(path, e))
}

/// Reads and checks a document file of at most [`DEFAULT_DOCUMENT_FILE_BYTES`].
pub fn read_document(path: &Path) -> Result<Document, RpcError> {
    read_document_capped(path, DEFAULT_DOCUMENT_FILE_BYTES)
}

/// Reads and checks a document file with every picture in memory: the result does not depend on
/// the file any more. For a document that is read from and not edited (a library, an import, a
/// local command).
pub fn read_document_capped(path: &Path, max_bytes: u64) -> Result<Document, RpcError> {
    open_document(path, max_bytes, ReadMode::Eager).map(|o| o.doc)
}

/// The folder of a document file, canonical if possible.
fn dir_of(file: &Path) -> Option<PathBuf> {
    let dir = file
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    std::fs::canonicalize(dir).ok()
}

/// The form that `file.save` writes: the parameter `format`, else the extension of the path.
fn save_form(params: &Value, path: &Path) -> Result<Form, RpcError> {
    match params.get("format") {
        None | Some(Value::Null) => Ok(Form::from_extension(path)),
        Some(Value::String(f)) => Form::from_name(f).ok_or_else(|| {
            RpcError::invalid_params(format!(
                "params.format is \"{f}\"; use \"container\" or \"json\""
            ))
        }),
        Some(_) => Err(RpcError::invalid_params(
            "params.format is \"container\" or \"json\"",
        )),
    }
}

/// Runs `file.save {path, format?}` or `file.open {path}` on the session.
///
/// `write` stores a save. The headless host passes [`write_document`]. The app adds its
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
        let spec = SaveSpec {
            form: save_form(params, &p)?,
            keep: s.file_source.iter().cloned().collect(),
        };
        let doc = s.document_for_save_in(spec.form == Form::Container)?;
        write(&p, &doc, &spec).map_err(|e| write_error(&p, e))?;
        s.collab_mark_saved();
        s.doc_dir = dir_of(&p);
        Ok(FileDone::Saved(p, spec.form))
    } else {
        let opened = open_document(&p, s.limits.document_file_bytes, ReadMode::Lazy)?;
        s.check_document_fits(&opened.doc)?;
        let form = opened.form;
        let notes = s.replace_document(opened.doc);
        s.take_replaced();
        s.file_source = opened.source;
        s.doc_dir = dir_of(&p);
        Ok(FileDone::Opened(p, notes, form))
    }
}

/// Atomic save with a `.bak` of the old file. The document is streamed into a buffered writer on
/// the temporary file; the text of the document is never built. Unchanged parts are copied from
/// the files in `spec.keep` without decoding them.
pub fn write_document(path: &Path, doc: &Document, spec: &SaveSpec) -> Result<(), FileError> {
    let opts = WriteOptions {
        generator: kadakas_file::generator(),
        keep: spec.keep.clone(),
    };
    native::save_path(path, doc, spec.form, &opts).map(|_| ())
}

/// [`write_document`] as a container with nothing to copy from (a new file). The error is a
/// message.
pub fn write_document_file(path: &Path, doc: &Document) -> Result<(), String> {
    write_document(path, doc, &SaveSpec::container()).map_err(|e| match e {
        FileError::Io(m) => m,
        other => format!("cannot write {}: {other}", path.display()),
    })
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
        Ok(FileDone::Opened(_, notes, form)) if !notes.is_empty() => {
            json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true, "format": form.name(), "notes": notes}}).to_string()
        }
        Ok(done) => {
            json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true, "format": done.form().name()}}).to_string()
        }
        Err(e) => {
            let hint = crate::feedback_api::hint(method, e.kind, &e.message);
            crate::errors::error_response_with(id, &e, hint, s.is_tainted())
        }
    }
}

/// The whole of `file.open` and `file.save` for a headless host: parse the request, run it,
/// write the response.
pub fn handle_file_request(s: &mut Session, req: &Value) -> String {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req["method"].as_str().unwrap_or_default();
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let r = file_call(s, method, &params, &mut write_document);
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
    use kadakas_sys::fsio;

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
        let first = std::fs::read(&f).unwrap();
        add_circle(&mut s);
        call(&mut s, "file.save", &f);
        assert_eq!(std::fs::read(dir.join("a.kad.bak")).unwrap(), first);
        assert_ne!(std::fs::read(&f).unwrap(), first);
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
            FileDone::Opened(f.clone(), Vec::new(), Form::Container)
                .path()
                .ends_with("a.kad")
        );
        assert!(is_file_method("file.open") && !is_file_method("render.svg"));
    }

    /// A hook that streams like `write_document_file`, and fails after `limit` bytes.
    fn failing_hook(
        limit: usize,
    ) -> impl FnMut(&Path, &Document, &SaveSpec) -> Result<(), FileError> {
        move |p, doc, _spec| {
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
            .map_err(|e| FileError::Io(e.to_string()))
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
    fn the_json_form_is_the_pretty_text_and_the_container_is_a_zip() {
        let dir = tmp("bytes");
        let mut s = restricted(&dir);
        add_circle(&mut s);
        let j = dir.join("a.json");
        assert_eq!(call(&mut s, "file.save", &j)["result"]["format"], "json");
        let on_disk = std::fs::read(&j).unwrap();
        let text = serde_json::to_string_pretty(s.editor.doc()).unwrap();
        assert_eq!(on_disk, text.as_bytes(), "byte for byte the old output");
        assert_eq!(read_document(&j).unwrap(), s.editor.doc().clone());
        let k = dir.join("a.kad");
        let r = call(&mut s, "file.save", &k);
        assert_eq!(r["result"]["format"], "container");
        assert!(std::fs::read(&k).unwrap().starts_with(b"PK\x03\x04"));
        assert_eq!(read_document(&k).unwrap(), s.editor.doc().clone());
        // The format parameter wins over the extension.
        let odd = dir.join("b.kad");
        let req = json!({"jsonrpc":"2.0","id":1,"method":"file.save","params":{"path":odd,"format":"json"}});
        let r: Value = serde_json::from_str(&handle_file_request(&mut s, &req)).unwrap();
        assert_eq!(r["result"]["format"], "json");
        assert!(std::fs::read(&odd).unwrap().starts_with(b"{"));
        let req = json!({"jsonrpc":"2.0","id":1,"method":"file.save","params":{"path":k,"format":"zip2"}});
        let r: Value = serde_json::from_str(&handle_file_request(&mut s, &req)).unwrap();
        assert_eq!(r["error"]["code"], -32602, "{r}");
        let req =
            json!({"jsonrpc":"2.0","id":1,"method":"file.save","params":{"path":k,"format":5}});
        let r: Value = serde_json::from_str(&handle_file_request(&mut s, &req)).unwrap();
        assert_eq!(r["error"]["code"], -32602, "{r}");
        assert_eq!(
            names(&dir),
            ["a.json", "a.kad", "b.kad"],
            "no temporary file is left"
        );
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
        let f = dir.join("a.json");
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
        // The error says how to raise the limit.
        assert!(e.message.contains("--max-document-mib"), "{}", e.message);
        assert!(e.message.contains("document_file_bytes"), "{}", e.message);
        // The two values of the decision.
        assert_eq!(DEFAULT_DOCUMENT_FILE_BYTES, 512 * 1024 * 1024);
        assert_eq!(
            crate::SessionLimits::strict().document_file_bytes,
            32 * 1024 * 1024
        );
        assert_eq!(
            crate::SessionLimits::default().document_file_bytes,
            DEFAULT_DOCUMENT_FILE_BYTES
        );
    }

    #[test]
    fn the_library_writes_with_the_same_streaming_save() {
        let dir = tmp("lib");
        let f = dir.join("lib.kad");
        write_document_file(&f, &Document::with_sequential_ids()).unwrap();
        assert!(write_document_file(&dir.join("no").join("x.kad"), &Document::default()).is_err());
        assert_eq!(read_document(&f).unwrap(), Document::with_sequential_ids());
    }

    /// A document with an underlay picture of about `bytes` bytes.
    fn add_picture(s: &mut Session, bytes: usize) {
        use kadakas_core::underlay::{PngData, Underlay};
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        png.extend((0..bytes).map(|i| (i as u32).wrapping_mul(2_654_435_761) as u8));
        let shape = Shape::Underlay(Box::new(Underlay {
            name: "map".into(),
            min: DVec2::ZERO,
            max: DVec2::new(100.0, 100.0),
            width: 10,
            height: 10,
            png: PngData::new(png),
            opacity: 1.0,
            source: String::new(),
        }));
        s.editor
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
    }

    fn lazy_pictures(s: &Session) -> usize {
        let mut n = 0;
        s.editor.doc().for_each_blob(&mut |b| {
            if let kadakas_core::model::DocBlob::Picture(p) = b {
                n += usize::from(p.is_lazy());
            }
        });
        n
    }

    #[test]
    fn an_opened_container_keeps_its_pictures_in_the_file_and_a_save_copies_them() {
        let dir = tmp("lazy");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        add_circle(&mut a);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        assert_eq!(lazy_pictures(&b), 1);
        assert!(b.file_source.is_some());
        assert_eq!(b.editor.doc(), a.editor.doc());
        // An edit, then a save over the file that the picture is read from.
        add_circle(&mut b);
        assert_eq!(call(&mut b, "file.save", &f)["result"]["ok"], true);
        assert_eq!(lazy_pictures(&b), 1, "still in the file");
        let mut c = restricted(&dir);
        call(&mut c, "file.open", &f);
        assert_eq!(c.editor.doc(), b.editor.doc());
        // A second save to another path moves the source: the old file may go.
        let g = dir.join("g.kad");
        assert_eq!(call(&mut b, "file.save", &g)["result"]["ok"], true);
        std::fs::remove_file(&f).unwrap();
        assert_eq!(call(&mut b, "file.save", &f)["result"]["ok"], true);
        assert_eq!(
            read_document(&f).unwrap(),
            b.editor.doc().clone(),
            "the picture was read from the new file"
        );
    }

    #[test]
    fn the_json_form_opens_without_a_source_and_converts_back() {
        let dir = tmp("jsonform");
        let mut a = restricted(&dir);
        add_picture(&mut a, 10_000);
        let j = dir.join("a.json");
        let k = dir.join("a.kad");
        assert_eq!(call(&mut a, "file.save", &j)["result"]["format"], "json");
        let mut b = restricted(&dir);
        let r = call(&mut b, "file.open", &j);
        assert_eq!(r["result"]["format"], "json");
        assert!(b.file_source.is_none());
        assert_eq!(lazy_pictures(&b), 0);
        call(&mut b, "file.save", &k);
        let mut c = restricted(&dir);
        call(&mut c, "file.open", &k);
        assert_eq!(c.editor.doc(), a.editor.doc());
    }

    #[test]
    fn a_container_that_expands_beyond_the_limit_is_refused_with_the_hint() {
        let dir = tmp("expand");
        let mut s = restricted(&dir);
        add_picture(&mut s, 200_000);
        let f = dir.join("a.kad");
        call(&mut s, "file.save", &f);
        let len = std::fs::metadata(&f).unwrap().len();
        // The picture is stored as it is: the entries are about as big as the file, so a limit
        // just above the file size but below the entries fails.
        let info = kadakas_file::native::inspect(&f).unwrap();
        assert!(info.uncompressed_bytes > len);
        s.limits.document_file_bytes = len + 1;
        let r = call(&mut s, "file.open", &f);
        assert_eq!(r["error"]["code"], 3, "{r}");
        let m = r["error"]["message"].as_str().unwrap();
        assert!(
            m.contains("together") && m.contains("--max-document-mib"),
            "{m}"
        );
        s.limits.document_file_bytes = info.uncompressed_bytes;
        assert_eq!(call(&mut s, "file.open", &f)["result"]["ok"], true);
    }

    #[test]
    fn a_file_that_changed_on_disk_is_a_conflict_and_the_save_does_not_touch_it() {
        let dir = tmp("changed");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut other = restricted(&dir);
        add_circle(&mut other);
        call(&mut other, "file.save", &f);
        let other_bytes = std::fs::read(&f).unwrap();
        let r = call(&mut b, "file.save", &f);
        assert_eq!(r["error"]["code"], 5, "{r}");
        assert!(
            r["error"]["message"]
                .as_str()
                .unwrap()
                .contains("changed on disk"),
            "{r}"
        );
        assert_eq!(std::fs::read(&f).unwrap(), other_bytes);
        assert_eq!(
            names(&dir),
            ["a.kad", "a.kad.bak"]
                .map(String::from)
                .into_iter()
                .filter(|n| dir.join(n).exists())
                .collect::<Vec<_>>()
        );
        assert!(!names(&dir).iter().any(|n| n.contains(".tmp")));
    }
}
