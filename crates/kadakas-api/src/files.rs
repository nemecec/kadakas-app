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

pub const FILE_SAVE: (&str, &str) = (
    "file.save",
    "Save the document atomically (the old file stays as .bak). params: {path, format?}. format is \"container\" (a ZIP file with the drawing, the pictures and the versions; the default) or \"json\" (one readable JSON file with the pictures as base64, for version control and diffs). Without format the extension decides: .json is JSON, anything else the container. Returns {ok, format}.",
);
pub const FILE_OPEN: (&str, &str) = (
    "file.open",
    "Open a document of either form (told by the first bytes). params: {path}. The pictures of a container stay in the file until they are used. A file over the size limit, or a container whose entries together are over it, is refused with the way to raise the limit. Returns {ok, format, notes?}.",
);

/// The extensions of a document file (lower case, no dot): the container and the plain JSON form.
pub const DOC_EXTS: [&str; 2] = ["kad", "json"];

/// How a save writes: the form, and the files the document was opened from (unchanged parts are
/// copied from them).
#[derive(Clone, Debug)]
pub struct SaveSpec {
    pub form: Form,
    pub keep: Vec<Arc<FileSource>>,
    /// The drawing was edited and the versions still wait in the file: write the drawing of the
    /// file as their anchor (`Session::needs_anchor`).
    pub anchor: bool,
    /// The person chose to save without what cannot be found (`allow_missing`).
    pub skip_missing: bool,
    /// The person chose to replace a file that another program changed (`allow_overwrite`).
    pub allow_overwrite: bool,
}

/// What a save asks the person (see [`ConsentFn`]).
pub enum Ask<'a> {
    /// Save without parts that cannot be found.
    Missing(&'a kadakas_file::Missing),
    /// Replace a file that another program changed since it was opened.
    Overwrite(&'a Path),
}

/// Decides whether the person agrees. The second argument is true when the caller passed the
/// token of the refusal (`allow_missing` or `allow_overwrite`). A host without a person (`kadakas
/// serve`, `kadakas mcp`) agrees exactly when the token matches; the app asks the person, whatever
/// the token says, and the default answer is no.
pub type ConsentFn<'a> = dyn FnMut(Ask<'_>, bool) -> bool + 'a;

/// The consent of a host without a person: the token of the refusal.
pub fn consent_by_token(_ask: Ask<'_>, token_matches: bool) -> bool {
    token_matches
}

impl SaveSpec {
    /// The form of the extension of `path` (`.json` is the plain JSON form, anything else the
    /// container), with the files to copy from.
    pub fn from_extension(path: &Path, keep: Vec<Arc<FileSource>>) -> SaveSpec {
        SaveSpec {
            form: Form::from_extension(path),
            keep,
            anchor: false,
            skip_missing: false,
            allow_overwrite: false,
        }
    }

    /// The container, with nothing to copy from.
    pub fn container() -> SaveSpec {
        SaveSpec {
            form: Form::Container,
            keep: Vec::new(),
            anchor: false,
            skip_missing: false,
            allow_overwrite: false,
        }
    }
}

/// Stores a save: the path, the document and how to write it. The function streams the document
/// into the file (see [`write_document`]).
/// It returns the notes for the person (a source that could not be opened again, parts that were
/// left out).
pub type WriteFn<'a> =
    dyn FnMut(&Path, &Document, &SaveSpec) -> Result<Vec<String>, FileError> + 'a;

/// What `file_call` did.
#[derive(Debug, PartialEq)]
pub enum FileDone {
    /// The path, what the load repaired (the load notes of the document) and the form of the file.
    Opened(PathBuf, Vec<String>, Form),
    /// The path, the form that was written and what the person should know (see `file.save`).
    Saved(PathBuf, Form, Vec<String>),
}

impl FileDone {
    pub fn path(&self) -> &Path {
        match self {
            FileDone::Opened(p, ..) | FileDone::Saved(p, ..) => p,
        }
    }

    pub fn form(&self) -> Form {
        match self {
            FileDone::Opened(_, _, f) | FileDone::Saved(_, f, _) => *f,
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
    if e.is_changed() {
        return RpcError::conflict(e);
    }
    match e {
        FileError::Missing(m) => crate::review_api::missing_error(&m),
        FileError::FileChanged(file) => file_changed_error(&file),
        FileError::Container(ContainerError::Changed(msg)) => RpcError::conflict(msg),
        FileError::Container(ContainerError::Limit(msg)) | FileError::Limit(msg) => {
            RpcError::limit(format!("cannot write {}: {msg}", path.display()))
        }
        FileError::Io(msg) => RpcError::app(msg),
        other => RpcError::app(format!("cannot write {}: {other}", path.display())),
    }
}

/// The error of a save that would replace a file that another program changed.
fn file_changed_error(file: &str) -> RpcError {
    RpcError::conflict(FileError::FileChanged(file.to_string()))
        .with_reason(crate::errors::REASON_FILE_CHANGED)
        .with_details(json!({
            "file": file,
            "options": [
                {"action": "open_again", "how": "Open the file again to see what the other program wrote, then redo the changes."},
                {"action": "overwrite", "how": "Ask the person. Then call file.save again with allow_overwrite set to the value of consent_token of this error (not true; the token is good for this one save of this document as it is): the file of the other program is replaced (the old file stays as the .bak)."}
            ],
            "ask_the_person": true,
        }))
}

/// The consent parameter of a save: the token that the refusal gave. `true` is not enough, so that
/// an agent cannot consent in advance to something it has not seen.
fn consent_param(params: &Value, key: &str) -> Result<Option<String>, RpcError> {
    match params.get(key) {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
        Some(Value::String(t)) if !t.is_empty() => Ok(Some(t.clone())),
        Some(_) => Err(RpcError::invalid_params(format!(
            "params.{key} is the consent_token of the refusal that the person agreed to (not true)"
        ))),
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
    consent: &mut ConsentFn<'_>,
) -> Result<FileDone, RpcError> {
    // These calls go around `Session::handle`, so the check for an open script run is here.
    if s.script_js_busy() {
        return Err(RpcError::conflict(
            "a script is running: its calls are the only calls served until it ends",
        ));
    }
    // A panic in a reader or a writer becomes an internal error, as for every other method. The
    // call and its error go to the call log of the session, so a bug report has them.
    let note = crate::feedback_api::begin(method, params);
    let r = s.guarded(method, |s| {
        file_call_inner(s, method, params, write, consent)
    });
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
    consent: &mut ConsentFn<'_>,
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
        let allow_missing = consent_param(params, "allow_missing")?;
        let allow_overwrite = consent_param(params, "allow_overwrite")?;
        let form = save_form(params, &p)?;
        let container = form == Form::Container;
        let mut spec = SaveSpec {
            form,
            keep: s.file_source.iter().cloned().collect(),
            anchor: container && s.needs_anchor(),
            skip_missing: false,
            allow_overwrite: false,
        };
        // What the person agreed to save without, once they did.
        let mut without: Option<kadakas_file::Missing> = None;
        let notes = loop {
            // The file at the path is another program's file (any form): ask first.
            let guard = !spec.allow_overwrite && s.seen.changed(&p);
            let result = match &without {
                _ if guard => Err(file_changed_error(&p.display().to_string())),
                None => match s.document_for_save_in(container) {
                    Ok(doc) => write(&p, &doc, &spec).map_err(|e| write_error(&p, e)),
                    Err(e) => Err(e),
                },
                Some(m) => {
                    let doc = s.document_without_missing(m);
                    write(&p, &doc, &spec).map_err(|e| write_error(&p, e))
                }
            };
            match result {
                Ok(notes) => break notes,
                // Another program changed the file since the open: the person decides.
                Err(e)
                    if e.reason == Some(crate::errors::REASON_FILE_CHANGED)
                        && !spec.allow_overwrite =>
                {
                    let agreed = match &allow_overwrite {
                        Some(t) => {
                            let matches = s.redeem_consent(&e, t);
                            consent(Ask::Overwrite(&p), matches)
                        }
                        None => false,
                    };
                    if !agreed {
                        return Err(s.issue_consent(e));
                    }
                    spec.allow_overwrite = true;
                }
                // The file that the pictures stay in changed. If the person agrees to save without
                // what cannot be found: placeholders for the pictures, no versions that wait in the
                // file, and the rest is saved.
                Err(e)
                    if e.reason == Some(crate::errors::REASON_PARTS_MISSING)
                        && without.is_none() =>
                {
                    let m = s.missing_for_save();
                    let agreed = match &allow_missing {
                        Some(t) => {
                            let matches = s.redeem_consent(&e, t);
                            consent(Ask::Missing(&m), matches)
                        }
                        None => false,
                    };
                    if !agreed {
                        return Err(s.issue_consent(e));
                    }
                    spec.skip_missing = true;
                    spec.anchor = container && s.needs_anchor() && !m.history;
                    without = Some(m);
                }
                Err(e) => return Err(s.issue_consent(e)),
            }
        };
        s.clear_consent();
        s.seen.note(&p);
        let mut notes = notes;
        if let Some(m) = &without {
            s.accept_missing(m);
            let mut what = Vec::new();
            if !m.pictures.is_empty() {
                what.push(format!(
                    "{} picture(s) became placeholders (the pictures are marked with their hash, and come back when a file that has them is opened; undo takes the placeholders back)",
                    m.pictures.len()
                ));
            }
            if m.history {
                what.push("the versions were left out".to_string());
            }
            notes.push(format!(
                "Saved without what could not be found in {}: {}",
                if m.file.is_empty() {
                    "the old file"
                } else {
                    &m.file
                },
                if what.is_empty() {
                    "other parts were left out".into()
                } else {
                    what.join(", ")
                }
            ));
        }
        s.collab_mark_saved();
        s.doc_dir = dir_of(&p);
        Ok(FileDone::Saved(p, form, notes))
    } else {
        s.refuse_replace(crate::Blocked::Open(p.clone()), "file.open")?;
        let mut opened = open_document(&p, s.limits.document_file_bytes, ReadMode::Lazy)?;
        s.check_document_fits(&opened.doc)?;
        let form = opened.form;
        // Placeholders of a save without the pictures: the real ones come back when the file (or
        // its .bak) has them.
        let (relinked, relink_notes) = opened.source.as_ref().map_or((0, Vec::new()), |src| {
            let blobs: Arc<dyn kadakas_core::blob::BlobSource> = src.clone();
            opened.doc.relink_placeholders_noted(&blobs)
        });
        // After a relink the drawing is not the drawing that the file stores: the history that
        // waits in the file must not take it for its anchor.
        let mut notes = s.replace_document_as(opened.doc, relinked == 0);
        notes.extend(relink_notes);
        notes.extend(kadakas_core::props::hardscape_name_notes(s.editor.doc()));
        if relinked > 0 {
            notes.push(format!(
                "{relinked} gray placeholder(s) were replaced by the real pictures, which this file has"
            ));
        }
        s.take_replaced();
        s.set_file_source(opened.source);
        s.doc_dir = dir_of(&p);
        s.seen.note(&p);
        Ok(FileDone::Opened(p, notes, form))
    }
}

/// Atomic save with a `.bak` of the old file. The document is streamed into a buffered writer on
/// the temporary file; the text of the document is never built. Unchanged parts are copied from
/// the files in `spec.keep` without decoding them.
pub fn write_document(
    path: &Path,
    doc: &Document,
    spec: &SaveSpec,
) -> Result<Vec<String>, FileError> {
    let opts = WriteOptions {
        generator: kadakas_file::generator(),
        keep: spec.keep.clone(),
        anchor: spec.anchor,
        skip_missing: spec.skip_missing,
        allow_overwrite: spec.allow_overwrite,
    };
    let saved = native::save_path(path, doc, spec.form, &opts)?;
    let mut notes = saved.notes;
    if let Some(w) = &saved.written
        && let Some(src) = spec.keep.first()
        && let Some(told) = src.tell_undo_lost(w.undo_pictures_lost)
    {
        notes.push(told);
    }
    if let Some(w) = &saved.written
        && w.skipped > 0
    {
        notes.push(format!(
            "{} part(s) of the old file that could not be found were left out",
            w.skipped
        ));
    }
    Ok(notes)
}

/// [`write_document`] as a container with nothing to copy from (a new file). The error is a
/// message.
pub fn write_document_file(path: &Path, doc: &Document) -> Result<(), String> {
    write_document(path, doc, &SaveSpec::container())
        .map(|_| ())
        .map_err(|e| match e {
            FileError::Io(m) => m,
            other => format!("cannot write {}: {other}", path.display()),
        })
}

/// The result of a file method, as JSON: `{ok, format, notes?}`.
fn done_json(done: &FileDone) -> Value {
    let notes = match done {
        FileDone::Opened(_, notes, _) | FileDone::Saved(_, _, notes) => notes,
    };
    let mut v = json!({"ok": true, "format": done.form().name()});
    if !notes.is_empty() {
        v["notes"] = json!(notes);
    }
    v
}

/// `file.save` and `file.open` as a method of the session, for a host that has no person to ask
/// (a test, a script over the API): the same work as `handle_file_request`, with the consent of
/// the token of the refusal. The app and the CLI call `file_call` themselves.
pub(crate) fn value_call(s: &mut Session, method: &str, params: &Value) -> Result<Value, RpcError> {
    file_call_inner(
        s,
        method,
        params,
        &mut write_document,
        &mut consent_by_token,
    )
    .map(|done| done_json(&done))
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
        Ok(done) => json!({"jsonrpc": "2.0", "id": id, "result": done_json(&done)}).to_string(),
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
    let r = file_call(
        s,
        method,
        &params,
        &mut write_document,
        &mut consent_by_token,
    );
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

    /// An agent must not replace a drawing with changes that the person has not saved (the app
    /// turns the guard on). Headless sessions keep replacing, as before.
    #[test]
    fn the_guard_refuses_to_replace_unsaved_changes() {
        let dir = tmp("guard");
        let f = dir.join("other.kad");
        let mut w = restricted(&dir);
        add_circle(&mut w);
        assert_eq!(call(&mut w, "file.save", &f)["result"]["ok"], true);

        let mut s = restricted(&dir);
        s.set_guard_unsaved(true);
        // A new drawing is clean: the open works.
        assert!(!s.has_unsaved_changes());
        assert_eq!(call(&mut s, "file.open", &f)["result"]["ok"], true);
        assert!(!s.has_unsaved_changes());
        // An edit makes it unsaved: file.open, document.load and collab.connect (also with
        // replace) are refused with the reason, and the drawing stays.
        add_circle(&mut s);
        assert!(s.has_unsaved_changes());
        let r = call(&mut s, "file.open", &f);
        assert_eq!(r["error"]["data"]["reason"], "unsaved_changes", "{r}");
        assert_eq!(r["error"]["data"]["kind"], "conflict");
        assert!(
            r["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Ask the person to save"),
            "{r}"
        );
        assert_eq!(s.editor.doc().objects.len(), 2);
        assert_eq!(
            s.take_blocked(),
            Some(crate::Blocked::Open(kadakas_sys::paths::strip_verbatim(&f)))
        );
        assert_eq!(s.take_blocked(), None);
        let e = s
            .handle(
                "document.load",
                json!({"document": serde_json::to_value(s.editor.doc()).unwrap()}),
            )
            .unwrap_err();
        assert_eq!(e.reason, Some(crate::REASON_UNSAVED_CHANGES));
        assert_eq!(s.take_blocked(), Some(crate::Blocked::Load));
        let e = s
            .handle(
                "collab.connect",
                json!({"url": "ws://127.0.0.1:1", "doc": "00000000-0000-0000-0000-000000000000", "replace": true}),
            )
            .unwrap_err();
        assert_eq!(e.reason, Some(crate::REASON_UNSAVED_CHANGES));
        assert_eq!(s.take_blocked(), Some(crate::Blocked::Connect));
        assert_eq!(s.editor.doc().objects.len(), 2, "nothing was replaced");

        // After a save the open works again, and the guard stays on for the new document.
        let mine = dir.join("mine.kad");
        assert_eq!(call(&mut s, "file.save", &mine)["result"]["ok"], true);
        assert!(!s.has_unsaved_changes());
        assert_eq!(call(&mut s, "file.open", &f)["result"]["ok"], true);
        add_circle(&mut s);
        assert!(call(&mut s, "file.open", &f)["error"].is_object());
        // A recovered copy was never saved.
        let mut r = restricted(&dir);
        r.set_guard_unsaved(true);
        r.mark_unsaved();
        assert!(r.has_unsaved_changes());
        assert!(call(&mut r, "file.open", &f)["error"].is_object());

        // The default (headless): no guard, the call replaces, as it always did.
        let mut h = restricted(&dir);
        add_circle(&mut h);
        add_circle(&mut h);
        assert_eq!(call(&mut h, "file.open", &f)["result"]["ok"], true);
        assert_eq!(h.editor.doc().objects.len(), 1);
        assert_eq!(h.take_blocked(), None);
    }

    #[test]
    fn the_file_methods_are_methods_of_the_session_too() {
        // They are in `describe` and a plain session runs them (a script over the API, a test).
        let listed: Vec<&str> = crate::METHODS.iter().map(|(n, _)| *n).collect();
        assert!(listed.contains(&"file.save") && listed.contains(&"file.open"));
        let dir = tmp("as-methods");
        let mut s = restricted(&dir);
        add_circle(&mut s);
        let f = dir.join("m.kad");
        let r = s.handle("file.save", json!({"path": f})).unwrap();
        assert_eq!(r, json!({"ok": true, "format": "container"}));
        let mut t = restricted(&dir);
        let r = t.handle("file.open", json!({"path": f})).unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!(t.editor.doc().objects.len(), 1);
        let e = t.handle("file.open", json!({})).unwrap_err();
        assert_eq!(e.code, -32602);
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
    ) -> impl FnMut(&Path, &Document, &SaveSpec) -> Result<Vec<String>, FileError> {
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
            .map(|()| Vec::new())
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
            let e = file_call(
                &mut s,
                "file.save",
                &params,
                &mut failing_hook(limit),
                &mut consent_by_token,
            )
            .unwrap_err();
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
        // The header of a PNG of 10 x 10 pixels, then bytes that are no image.
        let mut png = vec![
            0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0,
            10, 0, 0, 0, 10, 8, 6, 0, 0, 0,
        ];
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
    fn a_file_that_changed_on_disk_is_found_by_content_in_the_bak() {
        let dir = tmp("changed");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        std::thread::sleep(std::time::Duration::from_millis(30));
        // Another program saves over the file; its save keeps the old file as the .bak.
        let mut other = restricted(&dir);
        add_circle(&mut other);
        call(&mut other, "file.save", &f);
        assert!(dir.join("a.kad.bak").exists());
        // The picture of the document is not in the file now, but it is in the .bak: it is found
        // by its hash, and the document can be read and saved.
        assert!(b.handle("document.get", json!({})).is_ok());
        add_circle(&mut b);
        // The file at the path is another program's file now: the save asks the person.
        let r = call(&mut b, "file.save", &f);
        assert_eq!(r["error"]["data"]["reason"], "file_changed", "{r}");
        let token = r["error"]["data"]["consent_token"].clone();
        let r = save_with(&mut b, json!({"path": f, "allow_overwrite": token}));
        assert_eq!(r["result"]["ok"], true, "{r}");
        let mut c = restricted(&dir);
        call(&mut c, "file.open", &f);
        assert_eq!(c.editor.doc().objects.len(), b.editor.doc().objects.len());
    }

    #[test]
    fn a_file_with_the_parts_gone_is_saved_only_with_the_consent_of_the_person() {
        let dir = tmp("gone");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        add_circle(&mut b);
        std::thread::sleep(std::time::Duration::from_millis(30));
        // The file is replaced by another document, and there is no .bak.
        let mut other = restricted(&dir);
        add_circle(&mut other);
        let elsewhere = dir.join("other.kad");
        call(&mut other, "file.save", &elsewhere);
        std::fs::rename(&elsewhere, &f).unwrap();
        assert!(!dir.join("a.kad.bak").exists());
        let other_bytes = std::fs::read(&f).unwrap();
        let f2 = dir.join("target.kad");
        // The document cannot be given out: a picture is read from the file now.
        let e = b.handle("document.get", json!({})).unwrap_err();
        assert!(e.message.contains("changed on disk"), "{}", e.message);
        // A save says what is missing and asks the person; nothing is written.
        let r = call(&mut b, "file.save", &f2);
        let first = r.clone();
        assert_eq!(r["error"]["code"], 5, "{r}");
        assert_eq!(r["error"]["data"]["ask_the_person"], true, "{r}");
        assert_eq!(
            r["error"]["data"]["missing"]["pictures"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{r}"
        );
        assert_eq!(std::fs::read(&f).unwrap(), other_bytes);
        let g = dir.join("elsewhere.kad");
        let r = call(&mut b, "file.save", &g);
        assert_eq!(r["error"]["code"], 5, "{r}");
        assert!(!g.exists());
        // The consent is bound to the refusal: `true` and a token of something else do not count.
        let token = first["error"]["data"]["consent_token"]
            .as_str()
            .unwrap()
            .to_string();
        for bad in [json!(true), json!("not the token"), json!("0".repeat(64))] {
            let req = json!({"jsonrpc":"2.0","id":1,"method":"file.save",
                "params":{"path": f2, "allow_missing": bad}});
            let r: Value = serde_json::from_str(&handle_file_request(&mut b, &req)).unwrap();
            assert!(r.get("error").is_some(), "{bad}: {r}");
            assert_eq!(std::fs::read(&f).unwrap(), other_bytes);
        }
        // With the consent the picture becomes a placeholder, and the rest is saved.
        let req = json!({"jsonrpc":"2.0","id":1,"method":"file.save",
            "params":{"path": f2, "allow_missing": token}});
        let r: Value = serde_json::from_str(&handle_file_request(&mut b, &req)).unwrap();
        assert_eq!(r["result"]["ok"], true, "{r}");
        let notes = r["result"]["notes"].to_string();
        assert!(notes.contains("placeholder"), "{r}");
        let mut c = restricted(&dir);
        call(&mut c, "file.open", &f2);
        let placeholder = kadakas_core::underlay::placeholder_png();
        let pngs: Vec<Vec<u8>> = c
            .editor
            .doc()
            .objects
            .values()
            .filter_map(|o| match &o.shape {
                Shape::Underlay(u) => Some((*u.png.bytes().unwrap()).clone()),
                _ => None,
            })
            .collect();
        assert_eq!(pngs, std::slice::from_ref(&placeholder));
        // The session refers to the placeholder now: the next save is whole, without a question.
        assert_eq!(b.editor.doc().unreadable_pictures().len(), 0);
        assert!(b.handle("document.get", json!({})).is_ok());
        let r = call(&mut b, "file.save", &f2);
        assert_eq!(r["result"]["ok"], true, "{r}");
        assert!(r["result"].get("notes").is_none(), "{r}");
    }

    #[test]
    fn a_file_save_returns_its_notes() {
        // A source that cannot be opened again after the save is a note of the save.
        let dir = tmp("savenotes");
        let mut s = restricted(&dir);
        add_circle(&mut s);
        let f = dir.join("a.kad");
        let r = call(&mut s, "file.save", &f);
        assert_eq!(
            r["result"],
            json!({"ok": true, "format": "container"}),
            "{r}"
        );
        let done = FileDone::Saved(f.clone(), Form::Container, vec!["a note".into()]);
        let text = file_reply(&s, "file.save", json!(1), Ok(done));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["result"]["notes"], json!(["a note"]));
    }

    #[test]
    fn document_load_clears_the_file_source() {
        let dir = tmp("loadclears");
        let mut a = restricted(&dir);
        add_picture(&mut a, 10_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        assert!(b.file_source.is_some());
        let doc = b.handle("document.get", json!({})).unwrap();
        b.handle("document.load", json!({"document": doc})).unwrap();
        assert!(b.file_source.is_none());
    }
    fn save_with(s: &mut Session, params: Value) -> Value {
        let req = json!({"jsonrpc":"2.0","id":1,"method":"file.save","params":params});
        serde_json::from_str(&handle_file_request(s, &req)).unwrap()
    }

    #[test]
    fn a_save_over_a_file_that_another_program_changed_asks_the_person() {
        let dir = tmp("overwrite");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        add_circle(&mut b);
        std::thread::sleep(std::time::Duration::from_millis(30));
        // Another program saves over the file (the old file becomes the .bak).
        let mut other = restricted(&dir);
        add_circle(&mut other);
        call(&mut other, "file.save", &f);
        let theirs = std::fs::read(&f).unwrap();
        // The save is refused, with the reason and the token of this refusal.
        let r = call(&mut b, "file.save", &f);
        assert_eq!(r["error"]["code"], 5, "{r}");
        assert_eq!(r["error"]["data"]["reason"], "file_changed", "{r}");
        assert_eq!(r["error"]["data"]["ask_the_person"], true, "{r}");
        let token = r["error"]["data"]["consent_token"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(std::fs::read(&f).unwrap(), theirs);
        // True, and a token of another refusal, are not the consent of the person.
        for bad in [json!(true), json!("0".repeat(64)), json!("parts")] {
            let r = save_with(&mut b, json!({"path": f, "allow_overwrite": bad}));
            assert!(r.get("error").is_some(), "{bad}: {r}");
            assert_eq!(std::fs::read(&f).unwrap(), theirs);
        }
        // A new file is not "over the file of the other program": the pictures are in the .bak.
        // A save ends the tokens: the one from before is dead, and a new refusal gives a new one.
        let g = dir.join("copy.kad");
        assert_eq!(call(&mut b, "file.save", &g)["result"]["ok"], true);
        let r = save_with(&mut b, json!({"path": f, "allow_overwrite": token}));
        assert_eq!(r["error"]["data"]["reason"], "file_changed", "{r}");
        assert_eq!(std::fs::read(&f).unwrap(), theirs);
        let token = r["error"]["data"]["consent_token"].clone();
        // With the token the file is replaced, and the old file stays as the .bak.
        let r = save_with(&mut b, json!({"path": f, "allow_overwrite": token}));
        assert_eq!(r["result"]["ok"], true, "{r}");
        assert_ne!(std::fs::read(&f).unwrap(), theirs);
        assert_eq!(std::fs::read(dir.join("a.kad.bak")).unwrap(), theirs);
        // The next save over our own file is not refused, also after a touch (a new stamp).
        std::thread::sleep(std::time::Duration::from_millis(30));
        let same = std::fs::read(&f).unwrap();
        std::fs::write(&f, &same).unwrap();
        add_circle(&mut b);
        assert_eq!(call(&mut b, "file.save", &f)["result"]["ok"], true);
    }

    /// A session whose file was replaced by another program (the save asks the person).
    fn session_with_changed_file(name: &str) -> (Session, std::path::PathBuf, Vec<u8>) {
        let dir = tmp(name);
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
        let theirs = std::fs::read(&f).unwrap();
        (b, f, theirs)
    }

    #[test]
    fn a_consent_token_cannot_be_forged_reused_or_carried_over() {
        // The old token was a plain hash of the kind and the facts: it is no consent now.
        let (mut b, f, theirs) = session_with_changed_file("token-forged");
        let r = call(&mut b, "file.save", &f);
        let facts = json!({"file": r["error"]["data"]["file"]});
        let forged = kadakas_core::blob::text_hash(&format!("file_changed:{facts}"));
        let r2 = save_with(&mut b, json!({"path": f, "allow_overwrite": forged}));
        assert_eq!(r2["error"]["data"]["reason"], "file_changed", "{r2}");
        assert_eq!(std::fs::read(&f).unwrap(), theirs);
        // Without any refusal first, a token is nothing (a new session, no token was given).
        let (mut c, f2, theirs2) = session_with_changed_file("token-none");
        let r = save_with(
            &mut c,
            json!({"path": f2, "allow_overwrite": "ab".repeat(32)}),
        );
        assert!(r.get("error").is_some(), "{r}");
        assert_eq!(std::fs::read(&f2).unwrap(), theirs2);
        // A token that was used is used up: it does not open the next refusal.
        let (mut d, f3, _) = session_with_changed_file("token-reused");
        let r = call(&mut d, "file.save", &f3);
        let token = r["error"]["data"]["consent_token"].clone();
        let ok = save_with(&mut d, json!({"path": f3, "allow_overwrite": token}));
        assert_eq!(ok["result"]["ok"], true, "{ok}");
        // The other program writes again; the same token is refused, and a new one is given.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut other = restricted(&d.doc_dir.clone().unwrap());
        add_circle(&mut other);
        add_circle(&mut other);
        call(&mut other, "file.save", &f3);
        let theirs3 = std::fs::read(&f3).unwrap();
        let again = save_with(&mut d, json!({"path": f3, "allow_overwrite": token}));
        assert_eq!(again["error"]["data"]["reason"], "file_changed", "{again}");
        assert_ne!(again["error"]["data"]["consent_token"], token, "{again}");
        assert_eq!(std::fs::read(&f3).unwrap(), theirs3);
        // A token does not outlive an edit of the document.
        let (mut e, f4, theirs4) = session_with_changed_file("token-edit");
        let r = call(&mut e, "file.save", &f4);
        let token = r["error"]["data"]["consent_token"].clone();
        add_circle(&mut e);
        let r = save_with(&mut e, json!({"path": f4, "allow_overwrite": token}));
        assert_eq!(r["error"]["data"]["reason"], "file_changed", "{r}");
        assert_eq!(std::fs::read(&f4).unwrap(), theirs4);
        // The token of the new refusal works.
        let token = r["error"]["data"]["consent_token"].clone();
        let r = save_with(&mut e, json!({"path": f4, "allow_overwrite": token}));
        assert_eq!(r["result"]["ok"], true, "{r}");
    }

    #[test]
    fn a_consent_token_of_another_kind_is_refused() {
        // A file that changed and has lost the picture asks twice, each with its own token.
        let dir = tmp("token-kind");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        add_circle(&mut b);
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut other = restricted(&dir);
        add_circle(&mut other);
        call(&mut other, "file.save", &f);
        let _ = std::fs::remove_file(dir.join("a.kad.bak"));
        let target = dir.join("elsewhere.kad");
        let missing = call(&mut b, "file.save", &target);
        assert_eq!(
            missing["error"]["data"]["reason"], "parts_missing",
            "{missing}"
        );
        let t_missing = missing["error"]["data"]["consent_token"].clone();
        let changed = call(&mut b, "file.save", &f);
        assert_eq!(
            changed["error"]["data"]["reason"], "file_changed",
            "{changed}"
        );
        let t_changed = changed["error"]["data"]["consent_token"].clone();
        // The token of one kind passed for the other is refused (nothing is written).
        let r = save_with(
            &mut b,
            json!({"path": target, "allow_overwrite": t_missing}),
        );
        assert!(r.get("error").is_some() && !target.exists(), "{r}");
        let r = save_with(&mut b, json!({"path": target, "allow_missing": t_changed}));
        assert!(r.get("error").is_some() && !target.exists(), "{r}");
        let r = save_with(
            &mut b,
            json!({"path": target, "allow_missing": "cd".repeat(32)}),
        );
        assert!(r.get("error").is_some() && !target.exists(), "{r}");
        // The refusal tells the agent what to send.
        let text = missing["error"]["data"]["options"].to_string();
        assert!(
            text.contains("consent_token") && !text.contains("allow_missing: true"),
            "{text}"
        );
    }

    /// A session that deleted its picture (only undo holds it now) on a file that is `f`.
    fn session_with_undo_only_picture(
        name: &str,
    ) -> (Session, std::path::PathBuf, std::path::PathBuf) {
        let dir = tmp(name);
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        let id = *b.editor.doc().objects.keys().next().unwrap();
        b.editor
            .execute(&Command::DeleteObjects { ids: vec![id] })
            .unwrap();
        add_circle(&mut b);
        (b, f, dir)
    }

    fn other_saves_over(dir: &Path, f: &Path, circles: usize) {
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut other = restricted(dir);
        for _ in 0..circles {
            add_circle(&mut other);
        }
        call(&mut other, "file.save", f);
    }

    #[test]
    fn the_pictures_that_only_undo_holds_are_read_when_the_file_is_first_found_changed() {
        let (mut b, f, dir) = session_with_undo_only_picture("rescue");
        // Another program saves over the file: our file is its .bak. The first save of ours sees
        // the change, and reads the picture that undo holds into memory.
        other_saves_over(&dir, &f, 1);
        let g = dir.join("copy.kad");
        let r = call(&mut b, "file.save", &g);
        assert_eq!(r["result"]["ok"], true, "{r}");
        assert!(r["result"].get("notes").is_none(), "{r}");
        // The other program saves again and again: the .bak has no picture any more.
        other_saves_over(&dir, &f, 2);
        other_saves_over(&dir, &f, 3);
        assert!(!std::fs::read(dir.join("a.kad.bak")).unwrap().is_empty());
        // The picture is in the file we write now (from memory), and undo brings it back whole.
        let h = dir.join("later.kad");
        let r = call(&mut b, "file.save", &h);
        assert_eq!(r["result"]["ok"], true, "{r}");
        assert!(r["result"].get("notes").is_none(), "{r}");
        let mut c = restricted(&dir);
        call(&mut c, "file.open", &h);
        assert!(b.editor.undo() && b.editor.undo());
        b.editor.doc().check_blobs().unwrap();
        assert_eq!(b.editor.doc().unreadable_pictures().len(), 0);
        let r = call(&mut b, "file.save", &dir.join("after-undo.kad"));
        assert_eq!(r["result"]["ok"], true, "{r}");
    }

    #[test]
    fn pictures_that_only_undo_could_bring_back_are_told_once_for_each_count() {
        let (mut b, _f, dir) = session_with_undo_only_picture("told");
        // The file changes twice before any save of ours looked at it: the picture is nowhere.
        let f = dir.join("a.kad");
        other_saves_over(&dir, &f, 1);
        other_saves_over(&dir, &f, 2);
        let g = dir.join("one.kad");
        let r = call(&mut b, "file.save", &g);
        assert_eq!(r["result"]["ok"], true, "{r}");
        let notes = r["result"]["notes"].to_string();
        assert!(
            notes.contains("only undo could bring back")
                && notes.contains("undo will show them empty"),
            "{notes}"
        );
        // The same count is not told again.
        add_circle(&mut b);
        let r = call(&mut b, "file.save", &dir.join("two.kad"));
        assert_eq!(r["result"]["ok"], true, "{r}");
        assert!(r["result"].get("notes").is_none(), "{r}");
    }

    #[test]
    fn a_save_over_a_file_of_any_form_that_another_program_changed_asks_the_person() {
        let dir = tmp("seen-forms");
        // The plain JSON form: opened, changed by another program, saved.
        let mut a = restricted(&dir);
        add_circle(&mut a);
        let j = dir.join("a.json");
        assert_eq!(call(&mut a, "file.save", &j)["result"]["ok"], true);
        let mut b = restricted(&dir);
        assert_eq!(call(&mut b, "file.open", &j)["result"]["ok"], true);
        add_circle(&mut b);
        // A touch with the same content is no change.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let same = std::fs::read(&j).unwrap();
        std::fs::write(&j, &same).unwrap();
        let mut other = restricted(&dir);
        add_circle(&mut other);
        add_circle(&mut other);
        // Another program writes the file.
        call(&mut other, "file.save", &j);
        let theirs = std::fs::read(&j).unwrap();
        let r = call(&mut b, "file.save", &j);
        assert_eq!(r["error"]["data"]["reason"], "file_changed", "{r}");
        assert_eq!(std::fs::read(&j).unwrap(), theirs);
        let token = r["error"]["data"]["consent_token"].clone();
        let r = save_with(&mut b, json!({"path": j, "allow_overwrite": token}));
        assert_eq!(r["result"]["ok"], true, "{r}");
        assert_ne!(std::fs::read(&j).unwrap(), theirs);
        // Its own save is what the session saw: the next save is not refused.
        add_circle(&mut b);
        assert_eq!(call(&mut b, "file.save", &j)["result"]["ok"], true);
        // A container that the session saved (it was never opened from it): the same.
        let k = dir.join("k.kad");
        assert_eq!(call(&mut b, "file.save", &k)["result"]["ok"], true);
        let mut other = restricted(&dir);
        add_circle(&mut other);
        call(&mut other, "file.save", &k);
        let theirs = std::fs::read(&k).unwrap();
        add_circle(&mut b);
        let r = call(&mut b, "file.save", &k);
        assert_eq!(r["error"]["data"]["reason"], "file_changed", "{r}");
        assert_eq!(std::fs::read(&k).unwrap(), theirs);
        // A file that is gone is no change, and a path never seen has no guard.
        std::fs::remove_file(&k).unwrap();
        assert_eq!(call(&mut b, "file.save", &k)["result"]["ok"], true);
        let fresh = dir.join("fresh.json");
        assert_eq!(call(&mut b, "file.save", &fresh)["result"]["ok"], true);
    }

    #[test]
    fn a_relink_at_open_does_not_let_the_history_take_the_drawing_for_the_file_drawing() {
        let dir = tmp("relink-history");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let r = a.handle("version.create", json!({"name": "v1"}));
        assert!(r.is_ok(), "{r:?}");
        let f = dir.join("a.kad");
        assert_eq!(call(&mut a, "file.save", &f)["result"]["ok"], true);
        let original = std::fs::read(&f).unwrap();
        // A plain open: the history waits in the file and the drawing is the file's drawing.
        let mut plain = restricted(&dir);
        call(&mut plain, "file.open", &f);
        assert!(plain.review.waits_unchanged(plain.editor.revision()));
        // The picture becomes a placeholder (it is read from the file by its hash), and g is
        // saved with the versions that wait in the file.
        let marks: std::collections::HashSet<String> = plain
            .editor
            .doc()
            .objects
            .values()
            .filter_map(|o| match &o.shape {
                Shape::Underlay(u) => u.png.lazy().map(|l| l.hash().to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(plain.editor.replace_missing_pictures_with_undo(&marks), 1);
        let a = &mut plain;
        let g = dir.join("g.kad");
        assert_eq!(call(a, "file.save", &g)["result"]["ok"], true);
        // The real picture is in the .bak of g.
        std::fs::write(dir.join("g.kad.bak"), &original).unwrap();
        let mut c = restricted(&dir);
        let opened = call(&mut c, "file.open", &g);
        assert!(
            opened["result"]["notes"]
                .to_string()
                .contains("replaced by the real"),
            "{opened}"
        );
        // The drawing differs from the one in the file (placeholders there): the history is not
        // taken to wait unchanged, and it is still readable (not read only).
        assert!(!c.review.waits_unchanged(c.editor.revision()));
        let r = c.handle("version.list", json!({}));
        assert!(r.is_ok(), "{r:?}");
        let r = c.handle("version.create", json!({"name": "v2"}));
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn a_save_without_the_pictures_marks_the_placeholders_and_can_be_taken_back() {
        let dir = tmp("placeholders");
        let mut a = restricted(&dir);
        add_picture(&mut a, 30_000);
        let f = dir.join("a.kad");
        call(&mut a, "file.save", &f);
        let original = std::fs::read(&f).unwrap();
        let mut b = restricted(&dir);
        call(&mut b, "file.open", &f);
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut other = restricted(&dir);
        add_circle(&mut other);
        let elsewhere = dir.join("other.kad");
        call(&mut other, "file.save", &elsewhere);
        std::fs::rename(&elsewhere, &f).unwrap();
        let _ = std::fs::remove_file(dir.join("a.kad.bak"));
        // Replacing the other program's file needs the token of that refusal; then the pictures
        // are missing, and that needs a token too.
        let g = dir.join("g.kad");
        let r = call(&mut b, "file.save", &g);
        let token = r["error"]["data"]["consent_token"]
            .as_str()
            .unwrap()
            .to_string();
        let r = save_with(&mut b, json!({"path": g, "allow_missing": token}));
        assert_eq!(r["result"]["ok"], true, "{r}");
        // The placeholder says which picture it stands for.
        let marks = b.editor.doc().placeholder_hashes();
        assert_eq!(marks.len(), 1, "{marks:?}");
        let mut c = restricted(&dir);
        let opened = call(&mut c, "file.open", &g);
        assert!(
            opened["result"]["notes"]
                .to_string()
                .contains("gray placeholders"),
            "{opened}"
        );
        // Undo takes the placeholders back (one step); the next save asks again.
        assert!(b.editor.undo());
        assert!(b.editor.doc().placeholder_hashes().is_empty());
        let r = call(&mut b, "file.save", &dir.join("h.kad"));
        assert_eq!(r["error"]["data"]["reason"], "parts_missing", "{r}");
        assert!(b.editor.redo());
        assert_eq!(b.editor.doc().placeholder_hashes(), marks);
        // The real picture comes back when a file with it is opened: the original is the .bak of
        // g now (put it there), and the real bytes are read from it.
        std::fs::write(dir.join("g.kad.bak"), &original).unwrap();
        let mut d = restricted(&dir);
        let opened = call(&mut d, "file.open", &g);
        assert!(
            opened["result"]["notes"]
                .to_string()
                .contains("replaced by the real"),
            "{opened}"
        );
        assert!(d.editor.doc().placeholder_hashes().is_empty());
        let png = d
            .editor
            .doc()
            .objects
            .values()
            .find_map(|o| match &o.shape {
                Shape::Underlay(u) => Some(u.png.clone()),
                _ => None,
            })
            .unwrap();
        assert!(png.is_lazy() && png.len() > 1000);
    }
}
