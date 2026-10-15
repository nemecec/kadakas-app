//! Limits of one session: memory, time and sizes of results.
//!
//! A host that serves many documents in one process gives each session its own limits. The caps
//! that were constants before (image sizes, import text, tile cache, script time) are fields
//! here, with the old values as defaults. The memory figure is an estimate: it counts what a
//! session holds in a typical case (objects, resources, pictures, history, the cached scene),
//! not every byte.

use std::time::Duration;

use crate::{RpcError, Session};

/// The default for [`SessionLimits::document_file_bytes`]: 512 MiB, the document limit of the paid
/// collaboration tier. A loaded drawing needs 1.1 to 1.9 times the bytes of its text in memory
/// (`docs/doc-load-benchmark.md`).
pub const DEFAULT_DOCUMENT_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// The value of [`SessionLimits::document_file_bytes`] in `strict()`: 32 MiB, the snapshot limit of
/// the free collaboration tier.
pub const STRICT_DOCUMENT_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// The part of `describe` about document files (DOC-20 to DOC-22): the two forms, `file.open` and
/// `file.save`, and the limits. The methods are answered by the host (`kadakas serve`, `kadakas
/// mcp`, the app), so they are not in `methods`.
pub(crate) fn describe_files() -> serde_json::Value {
    serde_json::json!({
        "forms": {
            "container": "The default, extension .kad: a ZIP file with document.json (the drawing, deflated), blobs/<sha256> (each picture of 4 KiB or more, stored once, with the hash of collaboration), history/ (the versions) and manifest.json (every entry with its size and SHA-256). Any ZIP tool opens it. The pictures and the versions stay in the file until a call uses them.",
            "json": "One readable JSON file, extension .json: the text of the document with the pictures as base64 and the versions in the field history. For git, diffs and debugging.",
        },
        "file.save": "params: {path, format?}. format is \"container\" or \"json\"; without it the extension decides (.json is JSON, anything else the container). The save is atomic and the old file stays as <path>.bak. Returns {ok, format}. If the file that the open document was read from changed on disk, the call fails with code 5 (conflict) and writes nothing; open the file again.",
        "file.open": "params: {path}. The form is told by the first bytes. Returns {ok, format, notes?}. A file over the size limit, or a container whose entries together are over it, is refused (code 3) with the way to raise the limit (kadakas serve --max-document-mib). A damaged or hostile container is an error that names the entry.",
        "cli": "kadakas convert <input> <output> [--format container|json] converts between the forms and checks the input.",
        "limits": {
            "document_file_bytes": "The most bytes of a file that file.open reads, and the most bytes of all entries of a container together (uncompressed, from the manifest).",
            "entries": MAX_CONTAINER_ENTRIES,
            "inflate_ratio": "An entry above 1 MiB that inflates more than 200 times is refused as a compression bomb.",
        },
    })
}

/// The most entries a container holds (the 16 bit count of ZIP, less one).
const MAX_CONTAINER_ENTRIES: usize = 65_534;

/// The limits of one session. `Default` gives the limits that Kadakas always had, and no limit
/// for the number of objects, the memory and the time of a request.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionLimits {
    /// Most objects in the document. A call that adds more is undone and fails with a limit error.
    pub max_objects: usize,
    /// Most estimated memory of the session, in bytes (see `Session::estimated_bytes`). A call
    /// that changes the document and grows the session beyond it is undone and fails with a limit
    /// error. A session that is over the limit without a change (a big cache) drops its caches.
    pub max_memory_bytes: u64,
    /// Most bytes that the undo steps may hold. The oldest steps are dropped first.
    pub max_history_bytes: u64,
    /// Most named versions in the history of the document (REV-01). A version that would go over
    /// it is refused, and no version is deleted for it. 0 turns versions off.
    pub max_versions: usize,
    /// Most bytes of the history of versions (an estimate of its JSON). Handled like
    /// `max_versions`.
    pub max_version_bytes: u64,
    /// Most versions that a file may hold for the session to read its history. A file with more is
    /// opened with its history kept as it is (read only, and written back with the drawing it
    /// belongs to); raise the limit and open the file again to read it.
    pub history_read_versions: usize,
    /// Most bytes of changes and pictures that a file's history may hold for the session to read
    /// it (see `history_read_versions`).
    pub history_read_bytes: u64,
    /// Time budget of one call. A call that takes longer fails with a limit error; if it changed
    /// the document, the change is undone. The check runs after the call, so it does not stop a
    /// long calculation: it keeps its result from the document. `None`: no budget. Native only.
    pub request_time: Option<Duration>,
    /// Longest side of a rendered image, in px.
    pub render_side: u32,
    /// Most pixels in a rendered image.
    pub render_pixels: u64,
    /// Most pixels of 3D viewport images on one sheet page and in one export.
    pub raster_pixels: u64,
    /// The most text that an import reads, in bytes.
    pub import_text_bytes: u64,
    /// The largest document file that `file.open` reads, in bytes. The size is checked before the
    /// file is read. For a container (DOC-20) the sum of the sizes of its entries, uncompressed
    /// (from the manifest), is checked against it too, as is the size of each entry, so a small
    /// file cannot hide a big document. An agent cannot be asked, so this is a hard limit; the
    /// error says how to raise it (`kadakas serve --max-document-mib`). The desktop app does not
    /// use it for the files of the person: it asks (`kadakas_ui_core::open_limit`).
    pub document_file_bytes: u64,
    /// Most entities whose summaries a DXF or DWG import keeps for updates (`base: "auto"`). A
    /// drawing over it is refused with the choices `link` and `none`. It can be lowered, never
    /// raised above `provenance::MAX_BASE_RECS` (200,000).
    pub base_records: usize,
    /// Most bytes that the pending update from DXF may hold (`import.update`, IOP-23): the new
    /// objects of the changes and the update data of the new revision. A file over it is refused
    /// with a limit error; `layers` limits the update.
    pub update_bytes: u64,
    /// Time limit of one script reset.
    pub script_timeout: Duration,
    /// Time limit of one request to a map service.
    pub gis_timeout: Duration,
    /// Size of the tile cache on disk, in bytes. The default folder is shared by all sessions of
    /// the user, and each save trims it to the limit of the session that saves. A host with many
    /// sessions gives each one its own folder (`GisState::cache_dir`) or sets one limit for all.
    pub gis_cache_bytes: u64,
    /// Largest picture file (a plant style picture), in bytes. Never above the document limit
    /// of 8 MiB.
    pub picture_bytes: u64,
    /// Most pixels of a picture (width times height). Never above 16 million.
    pub picture_pixels: u64,
}

impl Default for SessionLimits {
    fn default() -> Self {
        SessionLimits {
            max_objects: usize::MAX,
            max_memory_bytes: u64::MAX,
            max_history_bytes: u64::MAX,
            max_versions: crate::VERSIONS_MAX,
            max_version_bytes: crate::VERSION_BYTES_MAX,
            history_read_versions: kadakas_review::history::HARD_MAX_VERSIONS,
            history_read_bytes: kadakas_review::history::HARD_MAX_BYTES as u64,
            request_time: None,
            render_side: 2048,
            render_pixels: 4_000_000,
            raster_pixels: 64_000_000,
            import_text_bytes: 256 * 1024 * 1024,
            document_file_bytes: DEFAULT_DOCUMENT_FILE_BYTES,
            base_records: kadakas_core::provenance::MAX_BASE_RECS,
            update_bytes: 256 * 1024 * 1024,
            script_timeout: Duration::from_secs(5),
            gis_timeout: Duration::from_secs(20),
            gis_cache_bytes: 256 * 1024 * 1024,
            picture_bytes: kadakas_core::picture::MAX_PICTURE_BYTES as u64,
            picture_pixels: kadakas_core::picture::MAX_PICTURE_PIXELS,
        }
    }
}

impl SessionLimits {
    /// Limits for documents of free and demo accounts, where many sessions share a process.
    pub fn strict() -> Self {
        SessionLimits {
            max_objects: 50_000,
            max_memory_bytes: 512 * 1024 * 1024,
            max_history_bytes: 64 * 1024 * 1024,
            max_versions: 20,
            max_version_bytes: 16 * 1024 * 1024,
            request_time: Some(Duration::from_secs(10)),
            render_pixels: 1_000_000,
            raster_pixels: 8_000_000,
            import_text_bytes: 16 * 1024 * 1024,
            document_file_bytes: STRICT_DOCUMENT_FILE_BYTES,
            update_bytes: 32 * 1024 * 1024,
            script_timeout: Duration::from_secs(2),
            gis_cache_bytes: 32 * 1024 * 1024,
            picture_bytes: 2 * 1024 * 1024,
            picture_pixels: 4_000_000,
            ..SessionLimits::default()
        }
    }

    /// The picture limits for the importer of `kadakas-render`.
    pub fn picture_limits(&self) -> kadakas_render::picture::PictureLimits {
        kadakas_render::picture::PictureLimits {
            max_bytes: usize::try_from(self.picture_bytes).unwrap_or(usize::MAX),
            max_pixels: self.picture_pixels,
        }
        .capped()
    }

    /// The size to render: each side 16 to `render_side`, scaled down in one piece if the area is
    /// too big.
    pub fn image_size(&self, width: u32, height: u32) -> (u32, u32) {
        let side = self.render_side.max(16);
        let (w, h) = (width.clamp(16, side), height.clamp(16, side));
        let area = w as u64 * h as u64;
        if area <= self.render_pixels {
            return (w, h);
        }
        let k = (self.render_pixels as f64 / area as f64).sqrt();
        (
            ((w as f64 * k) as u32).max(16),
            ((h as f64 * k) as u32).max(16),
        )
    }
}

/// Costs in the estimate of the memory of a session, in bytes.
const OBJECT_BYTES: u64 = 320;
const RESOURCE_BYTES: u64 = 2048;

/// A rough figure for a document: objects, resources and pictures.
pub(crate) fn document_bytes(doc: &kadakas_core::model::Document) -> u64 {
    let underlays: u64 = doc
        .objects
        .values()
        .map(|o| match &o.shape {
            kadakas_core::model::Shape::Underlay(u) => u.png.resident_len() as u64,
            _ => 0,
        })
        .sum();
    let plant_pictures: u64 = doc
        .resources
        .iter()
        .map(|r| match &r.body {
            kadakas_core::resource::ResourceBody::Image(i) => {
                (i.data.resident_len() + i.thumb.len()) as u64
            }
            _ => 0,
        })
        .sum();
    let pictures = underlays + plant_pictures;
    let versions = doc.history.as_ref().map_or(0, |h| h.bytes() as u64);
    doc.objects.len() as u64 * OBJECT_BYTES
        + doc.resources.len() as u64 * RESOURCE_BYTES
        + pictures
        + versions
}

impl Session {
    /// The limits of this session.
    pub fn limits(&self) -> &SessionLimits {
        &self.limits
    }

    /// Sets the limits and passes them on to the parts that enforce them (the history, the page
    /// cache, the map services and the scripts). The values stay when the document is replaced.
    pub fn set_limits(&mut self, limits: SessionLimits) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sheet_cache.raster_limit = Some(limits.raster_pixels);
            self.gis.timeout = limits.gis_timeout;
            self.gis.cache_limit = limits.gis_cache_bytes;
            self.scripting.limits.timeout = limits.script_timeout;
        }
        self.editor.set_history_limit(limits.max_history_bytes);
        // Every way to add a picture passes the editor, so the limits hold for `execute`, a batch,
        // `resource.create` and a script as well as for `plant_style.set_picture`.
        let pic = limits.picture_limits();
        self.editor
            .set_picture_limits(pic.max_bytes, pic.max_pixels);
        self.limits = limits;
    }

    /// A rough figure for the memory of the session in bytes: the document, its pictures, the
    /// undo and redo steps, the change log and the cached scene.
    pub fn estimated_bytes(&self) -> u64 {
        let bytes = document_bytes(self.editor.doc())
            + self.editor.history_bytes()
            + self.editor.log_bytes()
            + self.mesh_cache.approx_bytes()
            + self.compare.approx_bytes();
        // The update from DXF that waits for `import.apply` (native only).
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = bytes + self.update_bytes_held();
        bytes
    }

    /// Refuses a document that does not fit the limits. A host calls it before it replaces the
    /// document (`document.load`, `file.open`), so the open document stays when the new one is
    /// too big.
    pub fn check_document_fits(&self, doc: &kadakas_core::model::Document) -> Result<(), RpcError> {
        if doc.objects.len() > self.limits.max_objects {
            return Err(RpcError::limit(format!(
                "the document has {} objects, the limit is {}",
                doc.objects.len(),
                self.limits.max_objects
            )));
        }
        if document_bytes(doc) > self.limits.max_memory_bytes {
            return Err(RpcError::limit(format!(
                "the document needs more than {} bytes of memory",
                self.limits.max_memory_bytes
            )));
        }
        Ok(())
    }

    /// Frees the caches that are built again when needed (scene, annotation, worksheet and site
    /// results).
    pub fn drop_caches(&mut self) {
        self.mesh_cache = Default::default();
        self.lod_scene = None;
        self.compare.clear();
        #[cfg(not(target_arch = "wasm32"))]
        self.drop_update_preview();
        self.annot_cache = Default::default();
        self.worksheet_cache = Default::default();
        self.site_cache = Default::default();
    }

    /// Checks the limits after a call. `steps` is the number of undo steps the call added, and
    /// `changed` tells if the document changed.
    ///
    /// - A call that changed the document and broke the object or memory limit is undone and gives
    ///   a limit error. The caches are dropped first, so a big scene does not cost the change.
    /// - A session that is over the memory limit without a change (the caches grew) drops them
    ///   and the call succeeds. Reads and saves never fail for the size of the session.
    /// - A call that took longer than the time budget gives a limit error.
    pub(crate) fn enforce_limits(
        &mut self,
        steps: usize,
        changed: bool,
        elapsed: Option<Duration>,
        result: Result<serde_json::Value, RpcError>,
    ) -> Result<serde_json::Value, RpcError> {
        let Ok(value) = result else {
            return result;
        };
        let over_memory = |s: &Session| {
            s.limits.max_memory_bytes != u64::MAX && s.estimated_bytes() > s.limits.max_memory_bytes
        };
        if over_memory(self) {
            self.drop_caches();
        }
        let mut broken = None;
        if changed && steps > 0 {
            if self.editor.doc().objects.len() > self.limits.max_objects {
                broken = Some(format!(
                    "the document may have at most {} objects",
                    self.limits.max_objects
                ));
            } else if over_memory(self) {
                broken = Some(format!(
                    "the session may use at most {} bytes of memory",
                    self.limits.max_memory_bytes
                ));
            }
        }
        if broken.is_none()
            && let (Some(budget), Some(took)) = (self.limits.request_time, elapsed)
            && took > budget
        {
            broken = Some(format!(
                "the call took {:.1} s, the budget is {:.1} s",
                took.as_secs_f64(),
                budget.as_secs_f64()
            ));
        }
        match broken {
            None => Ok(value),
            Some(message) => {
                self.editor.rollback(steps);
                Err(RpcError::limit(message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn add_circles(s: &mut Session, n: usize) -> Result<serde_json::Value, RpcError> {
        let ops: Vec<_> = (0..n)
            .map(|i| {
                json!({"op": "add_object", "shape": {"kind": "circle", "center": [i as f64 * 10.0, 0.0], "radius": 2.0}})
            })
            .collect();
        s.handle(
            "execute",
            json!({"command": {"op": "batch", "commands": ops}}),
        )
    }

    #[test]
    fn defaults_are_the_old_caps() {
        let l = SessionLimits::default();
        assert_eq!(l.image_size(99999, 99999), (2000, 2000));
        assert_eq!(l.image_size(2048, 2048), (2000, 2000));
        assert_eq!(l.image_size(2000, 2000), (2000, 2000));
        assert_eq!(l.image_size(2048, 1024), (2048, 1024));
        assert_eq!(l.image_size(1, 1), (16, 16));
        let strict = SessionLimits::strict();
        assert!(strict.max_objects < usize::MAX && strict.request_time.is_some());
        assert!(strict.image_size(2048, 2048).0 <= 1000);
    }

    #[test]
    fn set_limits_reach_the_subsystems_and_survive_a_new_document() {
        let mut s = Session::default();
        let l = SessionLimits {
            script_timeout: Duration::from_millis(1500),
            gis_timeout: Duration::from_secs(3),
            gis_cache_bytes: 1234,
            raster_pixels: 777,
            ..SessionLimits::default()
        };
        s.set_limits(l.clone());
        assert_eq!(s.limits(), &l);
        assert_eq!(s.gis.timeout, Duration::from_secs(3));
        assert_eq!(s.gis.cache_limit, 1234);
        assert_eq!(s.scripting.limits.timeout, Duration::from_millis(1500));
        assert_eq!(s.sheet_cache.raster_pixels(), 777);
        s.replace_document(Default::default());
        assert_eq!(s.limits(), &l);
        assert_eq!(s.sheet_cache.raster_pixels(), 777);
    }

    #[test]
    fn a_call_over_the_object_limit_is_undone() {
        let mut s = Session::default();
        s.set_limits(SessionLimits {
            max_objects: 5,
            ..SessionLimits::default()
        });
        add_circles(&mut s, 5).unwrap();
        let before = s.editor.doc().objects.len();
        let e = add_circles(&mut s, 3).unwrap_err();
        assert_eq!(e.kind, "limit");
        assert_eq!(s.editor.doc().objects.len(), before, "the batch is undone");
        assert!(!s.editor.can_redo(), "a rolled back step cannot be redone");
        // Reads still work, and undo goes back to the earlier step.
        s.handle("objects.list", json!({})).unwrap();
        assert!(s.editor.can_undo());
    }

    #[test]
    fn a_call_over_the_memory_limit_is_undone() {
        let mut s = Session::default();
        let base = s.estimated_bytes();
        s.set_limits(SessionLimits {
            max_memory_bytes: base + 20 * OBJECT_BYTES,
            ..SessionLimits::default()
        });
        add_circles(&mut s, 4).unwrap();
        assert!(s.estimated_bytes() > base);
        let e = add_circles(&mut s, 40).unwrap_err();
        assert_eq!(e.kind, "limit");
        assert_eq!(s.editor.doc().objects.len(), 4);
    }

    #[test]
    fn a_slow_call_gives_a_limit_error() {
        let mut s = Session::default();
        s.set_limits(SessionLimits {
            request_time: Some(Duration::from_secs(5)),
            ..SessionLimits::default()
        });
        // Ask the check directly with a time over the budget.
        let e = s
            .enforce_limits(0, false, Some(Duration::from_secs(6)), Ok(json!({})))
            .unwrap_err();
        assert_eq!(e.kind, "limit");
        assert!(
            s.enforce_limits(0, false, Some(Duration::from_secs(1)), Ok(json!({})))
                .is_ok()
        );
        // Errors pass through unchanged.
        let err = RpcError::limit("x");
        assert!(
            s.enforce_limits(0, false, None, Err(err))
                .unwrap_err()
                .message
                .contains('x')
        );
    }

    #[test]
    fn a_session_over_the_limit_still_reads_and_saves() {
        let mut s = Session::default();
        add_circles(&mut s, 30).unwrap();
        // The limits are set below what the document already needs.
        s.set_limits(SessionLimits {
            max_objects: 5,
            max_memory_bytes: 1000,
            ..SessionLimits::default()
        });
        assert!(s.handle("objects.list", json!({})).is_ok());
        assert!(s.handle("document.get", json!({})).is_ok());
        assert!(s.handle("document.summary", json!({})).is_ok());
        assert_eq!(s.editor.doc().objects.len(), 30, "nothing is undone");
        // A change is refused and undone.
        assert!(add_circles(&mut s, 1).is_err());
        assert_eq!(s.editor.doc().objects.len(), 30);
    }

    #[test]
    fn a_big_cache_is_dropped_and_the_call_succeeds() {
        let mut s = Session::default();
        add_circles(&mut s, 3).unwrap();
        s.handle("render.png", json!({"width": 64, "height": 64}))
            .unwrap();
        let base = s.estimated_bytes();
        s.set_limits(SessionLimits {
            max_memory_bytes: base - 1,
            ..SessionLimits::default()
        });
        assert!(s.handle("selection.get", json!({})).is_ok());
        assert_eq!(s.mesh_cache.approx_bytes(), 0);
    }

    #[test]
    fn rollback_of_nothing_keeps_the_redo_steps() {
        let mut s = Session::default();
        add_circles(&mut s, 2).unwrap();
        s.handle("undo", json!({})).unwrap();
        assert!(s.editor.can_redo());
        s.editor.rollback(0);
        assert!(s.editor.can_redo());
    }

    #[test]
    fn a_document_over_the_limit_is_refused_before_it_replaces_the_open_one() {
        let mut big = Session::default();
        add_circles(&mut big, 20).unwrap();
        let doc = big.handle("document.get", json!({})).unwrap();
        let mut s = Session::default();
        add_circles(&mut s, 2).unwrap();
        s.set_limits(SessionLimits {
            max_objects: 10,
            ..SessionLimits::default()
        });
        let e = s
            .handle("document.load", json!({"document": doc}))
            .unwrap_err();
        assert_eq!(e.kind, "limit");
        assert_eq!(s.editor.doc().objects.len(), 2, "the open document stays");
    }

    #[test]
    fn the_history_counts_in_the_estimate_and_is_capped_in_bytes() {
        let mut s = Session::default();
        let empty = s.estimated_bytes();
        add_circles(&mut s, 50).unwrap();
        assert!(
            s.estimated_bytes() >= empty + 50 * 2 * OBJECT_BYTES,
            "objects and their undo step"
        );
        s.set_limits(SessionLimits {
            max_history_bytes: 10 * OBJECT_BYTES,
            ..SessionLimits::default()
        });
        for _ in 0..5 {
            add_circles(&mut s, 3).unwrap();
        }
        assert!(s.editor.history_bytes() <= 10 * OBJECT_BYTES + 3 * OBJECT_BYTES);
        assert!(s.editor.undo_depth() >= 1 && s.editor.undo_depth() < 6);
    }
}
