//! JSON-RPC 2.0 API. The CLI serves it on stdio, and the desktop app serves
//! it to AI agents. Each request is one JSON object on one line.

use std::collections::BTreeMap;
use std::sync::Arc;

mod agent_activity_api;
mod agent_view_api;
mod annotation_api;
mod collab_api;
mod unsaved;
pub use unsaved::Blocked;
#[cfg(not(target_arch = "wasm32"))]
mod consent;
pub use collab_api::EndpointFactory;
mod errors;
pub mod events;
pub mod feedback_api;
#[cfg(not(target_arch = "wasm32"))]
pub mod files;
pub use errors::{
    REASON_ALREADY_CONNECTED, REASON_BASE_TOO_LARGE, REASON_BASE_TOO_MANY, REASON_BCF_CONFLICT,
    REASON_BCF_UNFIT, REASON_CONFIRM_NEEDED, REASON_EDITS_WAIT, REASON_FILE_CHANGED,
    REASON_HISTORY_READONLY, REASON_PARTS_MISSING, REASON_PERSON_NAVIGATING,
    REASON_UNSAVED_CHANGES, REASON_VERSION_LIMIT, REASON_VIEW_LOCKED, RpcError,
    error_response_with,
};
#[cfg(test)]
mod care_api_tests;
mod compare_api;
mod data_api;
mod edit2d_api;
#[cfg(not(target_arch = "wasm32"))]
mod gis_api;
mod grading_api;
mod hardscape_api;
mod irrigation_api;
mod issue_api;
#[cfg(not(target_arch = "wasm32"))]
mod libfiles;
mod massing_api;
mod mesh_api;
pub use mesh_api::ERROR_IDS as MESH_ERROR_IDS;
mod pattern_api;
mod picture_api;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod picture_api_tests;
mod plan_api;
mod plants_api;
#[cfg(not(target_arch = "wasm32"))]
mod plugin_api;
mod plugin_util;
mod present_api;
mod query_api;
mod review_api;
mod script_js_api;
pub use compare_api::{CompareCache, CompareMode, CompareSpec, ShownDoc};
pub use review_api::{Side as CompareSide, VERSION_BYTES_MAX, VERSIONS_MAX, VERSIONS_WARN};
pub mod params;
mod rules_api;
mod sheets_api;
mod solid_api;
mod survey_api;
mod symbols_api;
mod wasm_plugin_api;

use serde::Deserialize;
use serde_json::{Value, json};

use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh::{MeshCache, Scene};
use kadakas_core::model::*;
use kadakas_core::present::RenderStyle;
use kadakas_core::sheets::SavedView;
#[cfg(not(target_arch = "wasm32"))]
mod dwg_api;
#[cfg(not(target_arch = "wasm32"))]
mod dxf_compare_api;
#[cfg(not(target_arch = "wasm32"))]
mod update_api;
#[cfg(not(target_arch = "wasm32"))]
pub use kadakas_io::dxf::revision::Change as EntityChange;
#[cfg(not(target_arch = "wasm32"))]
pub use kadakas_io::dxf::update::{
    ChangeItem, Counts as UpdateCounts, DrawingInfo, Kind as ChangeKind, Plan as UpdatePlan,
};
#[cfg(not(target_arch = "wasm32"))]
pub use update_api::{PendingUpdate, UpdatePreview, UpdateState};
#[cfg(not(target_arch = "wasm32"))]
mod ifc_api;
#[cfg(not(target_arch = "wasm32"))]
mod io;
#[cfg(not(target_arch = "wasm32"))]
mod meshio_api;
#[cfg(not(target_arch = "wasm32"))]
mod paths;
#[cfg(not(target_arch = "wasm32"))]
mod x3d_api;
#[cfg(not(target_arch = "wasm32"))]
pub use gis_api::{GisState, JobInfo};
#[cfg(not(target_arch = "wasm32"))]
pub use kadakas_gis::http::UrlPolicy;
pub use kadakas_plugin::ModuleTrust;
#[cfg(not(target_arch = "wasm32"))]
pub use kadakas_script::trust::TrustStore;
#[cfg(not(target_arch = "wasm32"))]
pub use paths::PathPolicy;
#[cfg(not(target_arch = "wasm32"))]
pub use plugin_api::{
    DATA_KEY as PLUGIN_DATA_KEY, ResetJob, ScriptingState, UNTRUSTED_CODE, instance_info,
    trust_plugin, user_finish, user_prepare, user_set_params,
};
#[cfg(not(target_arch = "wasm32"))]
pub use script_js_api::ScriptRun;
pub use wasm_plugin_api::{
    DATA_KEY as WASM_PLUGIN_DATA_KEY, WasmPluginState, commit_tool_result as commit_wasm_tool,
    forget_module as forget_wasm_plugin, install_module as install_wasm_plugin,
    module_review as review_wasm_plugin, take_store_warning as wasm_plugin_warning,
    tool_step_raw as wasm_tool_step, trust_module as trust_wasm_plugin,
};
mod bed_api;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod bed_api_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod guide_export_tests;
#[cfg(test)]
mod io_tests;
mod limits;
mod site;
mod site_texture;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod site_texture_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod x3d_material_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod x3d_tests;

pub use agent_activity_api::{Advice, StepCheck, StepError, Why, may_change_document};
use kadakas_render::{Camera, Projection, RenderMode, RenderOptions, StandardView, ViewSettings};
pub use limits::{DEFAULT_DOCUMENT_FILE_BYTES, STRICT_DOCUMENT_FILE_BYTES, SessionLimits};

/// An open document with its history and selection.
pub struct Session {
    pub editor: Editor,
    pub selection: Vec<ObjectId>,
    /// The groups you have entered, outermost first. A click selects members of the last one.
    pub context: Vec<ObjectId>,
    /// The chosen 3D view. `view.set` changes it, and the app follows it.
    pub view: ViewSettings,
    /// The snapping settings (the Snapping set). The app and `snap.*` share them.
    pub snap: kadakas_core::snapping::Settings,
    /// The smart points and edges you acquired.
    pub smart: kadakas_core::snapping::Smart,
    replaced: bool,
    view_changed: bool,
    /// The collaboration client and where it connects (see `collab_api`).
    collab: collab_api::CollabState,
    /// Who this session is, for the checks that need a name (the consent to delete the work of
    /// somebody else). Set by the host; a caller of the API cannot change it.
    identity: Option<String>,
    /// Who subscribed to the change events (see `events`), and the connection of the current call.
    events: events::EventHub,
    conn: u64,
    /// Memory, time and size limits of this session (see `SessionLimits`).
    limits: SessionLimits,
    /// New IDs count 2, 3, 4 and so on (see `Session::with_sequential_ids`). The mode stays when
    /// the document is replaced.
    sequential_ids: bool,
    /// Refuse calls that would replace a drawing with unsaved changes (see `unsaved`).
    guard_unsaved: bool,
    /// What the last refused call wanted (see `unsaved`).
    blocked: Option<Blocked>,
    mesh_cache: MeshCache,
    /// The triangles of surface models for `mesh.near` and `mesh.pick` (see `mesh_api`).
    mesh_query: mesh_api::QueryCache,
    /// The scene at a coarser detail than the count rule gives, for a view far from the plants.
    lod_scene: Option<(u64, kadakas_core::plants::form3d::Detail, Arc<Scene>)>,
    /// Which files requests may read and write (`import.file`, `export.file`, `file.open`, `file.save`).
    /// A host that serves agents restricts it. Default: any path.
    #[cfg(not(target_arch = "wasm32"))]
    pub paths: PathPolicy,
    /// Folder of the open document. It is always an allowed folder. The host keeps it up to date.
    #[cfg(not(target_arch = "wasm32"))]
    pub doc_dir: Option<std::path::PathBuf>,
    /// The files that the document was opened from or saved to, with what they held then: a save
    /// over one of them that another program changed since asks the person (any form).
    #[cfg(not(target_arch = "wasm32"))]
    pub seen: kadakas_file::SeenFiles,
    /// The container file that the open document was read from (DOC-21). Its pictures stay in the
    /// file until they are used, and a save copies the unchanged parts from it. `None`: the
    /// document is new, came from the plain JSON form, or was loaded from JSON.
    pub file_source: Option<Arc<kadakas_file::FileSource>>,
    /// Map services and GIS calls: the list of addresses that agents may fetch (default: none), the
    /// tile cache and the running network jobs (see `gis_api`).
    #[cfg(not(target_arch = "wasm32"))]
    pub gis: GisState,
    site_cache: kadakas_site::SiteCache,
    /// Annotation primitives and legend rows by document revision (see `render.svg`).
    annot_cache: kadakas_annot::AnnotCache,
    /// The secret of the session and the consent tokens that are alive (see `consent`).
    #[cfg(not(target_arch = "wasm32"))]
    consent: consent::ConsentState,
    /// Site and annotation results for the ghosts of a compare, for the old document that was
    /// drawn last (one document, so it is bounded by the ghost limits of the overlay).
    ghost_caches: GhostCaches,
    /// Worksheet results by document revision.
    worksheet_cache: kadakas_sheet::WorksheetCache,
    /// Display lists of sheets, by document revision (see `sheet.render`).
    #[cfg(not(target_arch = "wasm32"))]
    sheet_cache: kadakas_print::PageCache,
    /// Section drawings by document revision (see x3d_api.rs).
    #[cfg(not(target_arch = "wasm32"))]
    section_cache: kadakas_x3d::section::SharedSections,
    /// The saved view that `saved_view.restore` applied last, until the host takes it.
    applied_view: Option<SavedView>,
    /// The view in `applied_view` came from `view.set`: its camera only, not its visibility.
    applied_camera_only: bool,
    /// Plug-in objects and scripts: the flag for agents, the script host and the limits (see `plugin_api`).
    #[cfg(not(target_arch = "wasm32"))]
    pub scripting: ScriptingState,
    /// WebAssembly plug-ins: the installed modules and their trust (see `wasm_plugin_api`).
    pub wasm_plugins: WasmPluginState,
    /// JavaScript programs: the engine and the state of an open run (see `script_js_api`).
    script_js: script_js_api::ScriptJsState,
    /// The parsed history of versions of the open document (see `review_api`).
    review: review_api::ReviewCache,
    /// States, overlays and one document of the visual compare (see `compare_api`).
    compare: compare_api::CompareCache,
    /// The update from DXF that waits for `import.apply` (see `update_api`).
    #[cfg(not(target_arch = "wasm32"))]
    updates: update_api::UpdateState,
    /// What the load of the open document repaired (see `Session::load_notes`).
    load_notes: Vec<String>,
    /// Feedback reports: the outbox, the recent calls and the last failure (see `feedback_api`).
    pub feedback: feedback_api::FeedbackState,
    /// True after a method panicked (see `Session::guarded`). Error replies say so
    /// (`data.tainted`), and a new document clears it.
    tainted: bool,
    /// The steps of the agents: one for each call or script run that changed the document
    /// (see `agent_activity_api`).
    pub activity: kadakas_core::activity::ActivityLog,
    /// Save the version "Before agent session" before the first change of an agent session (see
    /// `set_session_version`). Off by default; the app turns it on. The setting stays when the
    /// document is replaced.
    session_version: bool,
    /// The person undid steps of their own with Undo to here: calls of the agent that change the
    /// document wait while redo can bring those steps back (see `agent_hold`).
    agent_hold: bool,
}

/// The caches of the ghosts of a compare (see `svg_of`). They belong to one old document, which
/// is kept (so its address cannot be taken by another document); a new old document starts new
/// caches under a new revision number.
#[derive(Default)]
pub(crate) struct GhostCaches {
    site: kadakas_site::SiteCache,
    annot: kadakas_annot::AnnotCache,
    old: std::sync::Mutex<(Option<Arc<Document>>, u64)>,
}

impl GhostCaches {
    /// The caches for `old` and their revision (never 0).
    pub(crate) fn for_doc(
        &self,
        old: &Arc<Document>,
    ) -> (&kadakas_site::SiteCache, &kadakas_annot::AnnotCache, u64) {
        let mut g = self.old.lock().unwrap_or_else(|e| e.into_inner());
        if !g.0.as_ref().is_some_and(|a| Arc::ptr_eq(a, old)) {
            g.1 += 1;
            g.0 = Some(old.clone());
        }
        (&self.site, &self.annot, g.1)
    }

    /// The bytes of the old document that is kept.
    pub(crate) fn held_bytes(&self) -> u64 {
        let g = self.old.lock().unwrap_or_else(|e| e.into_inner());
        g.0.as_ref().map_or(0, |d| limits::document_bytes(d))
    }
}

/// The SVG of a document with the site model and annotation extensions, and the caches that go
/// with a revision of it (revision 0: nothing is kept). `overlay` changes the look of objects
/// (the compare of REV-03).
fn svg_of(
    doc: &Document,
    width: u32,
    revision: u64,
    site_cache: &kadakas_site::SiteCache,
    annot_cache: &kadakas_annot::AnnotCache,
    overlay: Option<&dyn kadakas_core::svg_overlay::SvgOverlay>,
    ghost: Option<(&kadakas_site::SiteCache, &kadakas_annot::AnnotCache, u64)>,
) -> String {
    svg_of_with(
        doc,
        width,
        revision,
        site_cache,
        annot_cache,
        overlay,
        ghost,
        &kadakas_core::svg::RenderOptions::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn svg_of_with(
    doc: &Document,
    width: u32,
    revision: u64,
    site_cache: &kadakas_site::SiteCache,
    annot_cache: &kadakas_annot::AnnotCache,
    overlay: Option<&dyn kadakas_core::svg_overlay::SvgOverlay>,
    ghost: Option<(&kadakas_site::SiteCache, &kadakas_annot::AnnotCache, u64)>,
    opts: &kadakas_core::svg::RenderOptions,
) -> String {
    let site_ext = kadakas_site::draw::svg_ext(site_cache, revision);
    // A ghost of a compare is drawn against the document of the old side. It gets caches of its
    // own, so it never reads what was built for another document. A session passes its ghost
    // caches (kept while the same old document is compared); else they are new for this call and
    // keep nothing (revision 0).
    let local = (
        kadakas_site::SiteCache::default(),
        kadakas_annot::AnnotCache::default(),
    );
    let (ghost_site, ghost_annot, ghost_rev) = ghost.unwrap_or((&local.0, &local.1, 0));
    let ghost_site_ext = kadakas_site::draw::svg_ext(ghost_site, ghost_rev);
    let ext = |d: &Document, o: &Object, style: &str, out: &mut String| {
        if std::ptr::eq(d, doc) {
            site_ext(d, o, style, out);
            kadakas_annot::svg::object_svg_with(d, o, style, out, annot_cache, revision);
        } else {
            ghost_site_ext(d, o, style, out);
            kadakas_annot::svg::object_svg_with(d, o, style, out, ghost_annot, ghost_rev);
        }
    };
    kadakas_core::svg::render_overlay_with(doc, width.clamp(16, 8192), Some(&ext), overlay, opts)
}

/// The picture of an underlay is not sent in lists: it is megabytes of base64 text.
/// The tags of the sources of a site model are long (a tag for every point). Lists and
/// `objects.get` show how many there are; `objects.get {sources: true}` and `document.get` show
/// them.
fn elide_sources(object: &mut Value) {
    if let Some(v) = object.pointer_mut("/data/import_sources") {
        let tagged = |k: &str| {
            v.get(k)
                .and_then(Value::as_array)
                .map_or(0, |a| a.iter().filter(|x| !x.is_null()).count())
        };
        *v = json!({
            "hidden": "objects.get {sources: true} shows the tags of the sources",
            "tagged_points": tagged("points"),
            "tagged_lines": tagged("lines"),
        });
    }
}

fn elide_image(object: &mut Value) {
    if let Some(png) = object.pointer_mut("/shape/png") {
        let n = png.as_str().map_or(0, str::len);
        *png = json!(format!(
            "<PNG picture, {n} characters of base64, not shown>"
        ));
    }
}

/// True when a command (or a command inside a batch) names a layer or a class by a name.
/// The first data key of a `set_data` command (or of a command in a batch) that starts with the
/// reserved prefix `kadakas.`. Agents cannot write or remove these keys. Kadakas itself writes
/// them with commands of its own.
fn reserved_data_key(command: &serde_json::Map<String, Value>) -> Option<String> {
    let reserved = |k: &str| k.starts_with(kadakas_core::visibility::RESERVED_PREFIX);
    match command.get("op").and_then(Value::as_str) {
        Some("set_data") => {
            let set = command.get("set").and_then(Value::as_object);
            let remove = command.get("remove").and_then(Value::as_array);
            set.into_iter()
                .flat_map(|m| m.keys().cloned())
                .chain(
                    remove
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_string)),
                )
                .find(|k| reserved(k))
        }
        Some("batch") => command
            .get("commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .find_map(reserved_data_key),
        _ => None,
    }
}

fn json_command_has_names(c: &serde_json::Map<String, Value>) -> bool {
    params::refs::has_names(c)
        || c.get("commands")
            .and_then(Value::as_array)
            .is_some_and(|list| {
                list.iter()
                    .any(|x| x.as_object().is_some_and(json_command_has_names))
            })
}

/// The names of the layers and classes for the commands of one batch. They are made when a
/// command needs them and again after a command that can change them (an op with `layer` or
/// `class` in its name), so a long batch does not list the layers for every command.
#[derive(Default)]
struct BatchNames(Option<params::Names>);

impl BatchNames {
    fn get(&mut self, doc: &Document) -> &params::Names {
        self.0.get_or_insert_with(|| params::Names::of(doc))
    }

    fn after(&mut self, op: Option<&str>) {
        if op.is_some_and(|o| o.contains("layer") || o.contains("class")) {
            self.0 = None;
        }
    }
}

/// Runs one command of JSON, and the commands inside a batch one by one.
fn run_json_command(
    step: &mut kadakas_core::command::StepRun<'_, '_>,
    mut c: serde_json::Map<String, Value>,
    names: &mut BatchNames,
) -> Result<(), RpcError> {
    let is_batch = c.get("op").and_then(Value::as_str) == Some("batch");
    if is_batch && matches!(c.get("commands"), Some(Value::Array(_))) {
        let Some(Value::Array(list)) = c.remove("commands") else {
            unreachable!("checked above")
        };
        for item in list {
            let Value::Object(inner) = item else {
                // Not a command: let the parser say what is wrong with it.
                let command: Command = params(json!({"op": "batch", "commands": [item]}))?;
                return step.run(&command).map_err(RpcError::from);
            };
            run_json_command(step, inner, names)?;
        }
        return Ok(());
    }
    // A batch whose `commands` is not a list goes to the parser, which says "wrong type".
    if params::refs::has_names(&c) {
        params::refs::resolve_command(names.get(step.doc()), &mut c)?;
    }
    let op = c.get("op").and_then(Value::as_str).map(str::to_string);
    let command: Command = params(Value::Object(c))?;
    step.run(&command).map_err(RpcError::from)?;
    names.after(op.as_deref());
    Ok(())
}

/// The region of `render.svg`: `[x0, y0, x1, y1]` in drawing coordinates, two opposite corners.
fn region_param(r: Option<[f64; 4]>) -> Result<Option<(glam::DVec2, glam::DVec2)>, RpcError> {
    let Some([x0, y0, x1, y1]) = r else {
        return Ok(None);
    };
    if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
        return Err(RpcError::invalid_params(
            "'region' must be four finite numbers",
        ));
    }
    let (lo, hi) = (
        glam::DVec2::new(x0.min(x1), y0.min(y1)),
        glam::DVec2::new(x0.max(x1), y0.max(y1)),
    );
    if hi.x - lo.x < 1e-6 || hi.y - lo.y < 1e-6 {
        return Err(RpcError::invalid_params(
            "'region' must have a width and a height: [x0, y0, x1, y1]",
        ));
    }
    Ok(Some((lo, hi)))
}

fn params<T: for<'de> Deserialize<'de>>(p: Value) -> Result<T, RpcError> {
    let p = if p.is_null() { json!({}) } else { p };
    serde_json::from_value(p).map_err(RpcError::invalid_params)
}

#[derive(Deserialize)]
struct ObjectFilter {
    layer: Option<LayerId>,
    class: Option<ClassId>,
    kind: Option<String>,
}

/// Method names with a short description, for discovery by agents.
pub const METHODS: &[(&str, &str)] = &[
    ("describe", "List methods and command examples."),
    (
        "execute",
        "Run one command. params: {command: Command}. Returns {created: [id]}.",
    ),
    ("undo", "Undo the last command. Returns {done: bool}."),
    (
        "redo",
        "Redo the last undone command. Returns {done: bool}.",
    ),
    (
        "document.summary",
        "Layers, classes, object counts and drawing bounds.",
    ),
    (
        "document.get",
        "The full document as JSON, without its versions (they have their own methods: version.*).",
    ),
    (
        "document.load",
        "Replace the document. params: {document}. Clears the undo history. If the document has no `history` field (document.get leaves it out), the versions of the current document stay: they are written for the new drawing. A document with a `history` field brings its own versions. Returns {ok, notes?}; notes lists values that were repaired instead of refusing the document (an invalid 3D look of a site model goes back to its defaults) and what could not be read of its versions.",
    ),
    #[cfg(not(target_arch = "wasm32"))]
    files::FILE_SAVE,
    #[cfg(not(target_arch = "wasm32"))]
    files::FILE_OPEN,
    ("objects.list", "Objects. params: {layer?, class?, kind?}."),
    (
        "objects.get",
        "One object. params: {id, sources? (true shows the import tags of the sources of a site model: data.import_sources, one entry for each point and line; by default the answer only counts them)}.",
    ),
    ("selection.get", "Selected object IDs."),
    (
        "selection.set",
        "Select objects. params: {ids: [id]}. Objects of grey layers and classes cannot be selected.",
    ),
    (
        "render.svg",
        "SVG image of visible objects. params: {width?: px, region? ([x0, y0, x1, y1] in drawing coordinates, mm: draw only this part, and draw the units of the paved areas in it one by one when there are at most 60,000 (limits.pattern_units); above that the paved areas are a preview that the picture marks, so zoom to a smaller region to see every unit), show_check? (true: mark the findings of unit_pattern.check on the paved areas: overlaps, small pieces, joints, holes), compare?}. With compare {from, to, mode?, hide_unchanged?, ...} (see describe.review.compare) the plan of `to` is drawn with the changes since `from`: added objects green, changed objects orange, removed objects as red dashed ghosts, each with a mark (+, ~, -) and a legend with the counts. Returns {svg, compare?}.",
    ),
    (
        "render.png",
        "PNG image of the 3D model. params: {view?, width?, height?, projection?, mode?, render_style?, samples?, region? ([x0, y0, x1, y1] in drawing coordinates, mm: frame the camera on this part of the drawing instead of the whole model, for a close picture of a part of a big site)}. view is one of view.list (default: the current view), projection is ortho or perspective, mode is shaded, wireframe, hidden_line or dashed_hidden_line, render_style is a render style of the document (ID or name) or a built-in one (Realistic draws materials, the sun and shadows); without it the active style is used. samples is 1 to 3 (anti-aliasing). camera ({target: [x, y, z] in drawing coordinates (mm), yaw, pitch (radians), distance (mm), fov (degrees), projection (perspective | ortho)}) is a free camera that is used as given, instead of the view and the framing of the model; the answer then has the camera that was used. Size is 16 to 2048 px per side and at most 4 million px (default 800x600), larger sizes are scaled down. compare {from, to, mode?, hide_unchanged?, ...} (see describe.review.compare) draws `to` with the changes since `from`: added objects green, changed orange, removed objects as see-through red ghosts; with mode side_by_side it draws both sides next to each other with the same framing. render_style cannot be used with compare. Returns {png_base64, width, height, view, projection, mode, camera? (when you gave one), render_style (not with compare), compare?}.",
    ),
    (
        "view.list",
        "Standard 3D views, projections and render modes, the saved views (saved_views: [{name}]; view.set takes their names) and the current view.",
    ),
    (
        "view.set",
        "Switch the view (also in the live app). params: {view?, projection?, mode?}. view is a name from view.list or the name of a saved view (view.list saved_views); top is the 2D plan in the app. A saved view sets the camera only (the plan middle and zoom in the app, or the 3D view, projection and mode): the layers and classes stay as they are, because changing them is an edit of the document (use saved_view.restore for that), and the answer has layers_not_applied: true when the saved view holds visibility, or an active layer or class, that differs from now (classes and the active layer and class count too). Returns the current view.",
    ),
    (
        "report.plants",
        "Plant schedule: count by species. Rows of plants with a style also have style, style_name, id_code and prices (see report.plant_schedule).",
    ),
    (
        "report.areas",
        "Areas of landscape areas and hardscapes, in m².",
    ),
    #[cfg(not(target_arch = "wasm32"))]
    (
        "import.file",
        "Import a file as one undo step: DXF, DWG (needs a converter), Shapefile, GeoJSON or IFC. params: {path, format?: dxf|dwg|shp|geojson|ifc (default: from the extension), options?}. See describe.io for the options of each format. Returns {imported, skipped, report, georeference}.",
    ),
    #[cfg(not(target_arch = "wasm32"))]
    (
        "export.file",
        "Export the document to a file: DXF, GeoJSON, DWG (needs a converter), IFC (IFC4, IFC2X3 or IFC4X3), glTF, GLB or OBJ (3D), or a sheet as PDF, SVG or PNG. params: {path, format?: dxf|geojson|dwg|ifc|gltf|glb|obj|pdf|sheet_svg|sheet_png (default: from the extension; the sheet formats need format), options?, overwrite? (replace an existing file; a server for agents needs it)}. See describe.io for the options of each format. Returns {path, exported, report}.",
    ),
    // Updating an imported drawing from a new revision (see update_api.rs).
    #[cfg(not(target_arch = "wasm32"))]
    update_api::IMPORT_UPDATE,
    #[cfg(not(target_arch = "wasm32"))]
    update_api::IMPORT_APPLY,
    // Plants (see plants_api.rs).
    plants_api::PLANT_STYLE_LIST,
    plants_api::PLANT_STYLE_GET,
    plants_api::PLANT_STYLE_CREATE,
    plants_api::PLANT_STYLE_UPDATE,
    plants_api::PLANT_STYLE_DUPLICATE,
    plants_api::PLANT_STYLE_DELETE,
    plants_api::PLANT_STYLE_IMPORT_CSV,
    plants_api::PLANT_STYLE_EXPORT_CSV,
    plants_api::PLANT_STYLE_SAMPLE_CSV,
    plants_api::PLANTS_PLACE,
    plants_api::PLANTS_SET_STYLE,
    plants_api::REPORT_PLANT_SCHEDULE,
    plants_api::REPORT_EXISTING_TREES,
    plants_api::PLANTS_DRAPE,
    plants_api::PLANT_TAG_SET,
    picture_api::PLANT_STYLE_SET_PICTURE,
    picture_api::PLANT_STYLE_CLEAR_PICTURE,
    picture_api::PLANT_STYLE_GET_PICTURE,
    // Records, the Data Manager and worksheets (see data_api.rs).
    data_api::RECORD_FORMAT_LIST,
    data_api::RECORD_FORMAT_GET,
    data_api::RECORD_FORMAT_CREATE,
    data_api::RECORD_FORMAT_UPDATE,
    data_api::RECORD_FORMAT_DELETE,
    data_api::RECORD_ATTACH,
    data_api::RECORD_DETACH,
    data_api::RECORD_SET,
    data_api::RECORD_GET,
    data_api::DATA_MANAGER_LIST,
    data_api::DATA_MANAGER_GET,
    data_api::DATA_MANAGER_CREATE,
    data_api::DATA_MANAGER_UPDATE,
    data_api::DATA_MANAGER_DELETE,
    data_api::DATA_MANAGER_CHECK,
    data_api::DATA_MANAGER_EVALUATE,
    data_api::DATA_MANAGER_APPLY,
    data_api::DATA_MANAGER_PROPERTIES,
    data_api::WORKSHEET_LIST,
    data_api::WORKSHEET_TEMPLATES,
    data_api::WORKSHEET_CREATE,
    data_api::WORKSHEET_GET,
    data_api::WORKSHEET_RENAME,
    data_api::WORKSHEET_DELETE,
    data_api::WORKSHEET_SET_CELL,
    data_api::WORKSHEET_SET_CELLS,
    data_api::WORKSHEET_SET_DATABASE,
    data_api::WORKSHEET_REMOVE_DATABASE,
    data_api::WORKSHEET_EVALUATE,
    data_api::WORKSHEET_EXPORT,
    data_api::WORKSHEET_PLACE,
    data_api::WORKSHEET_FIT,
    // Materials, render styles and the sun (see present_api.rs).
    present_api::MATERIAL_LIST,
    present_api::MATERIAL_GET,
    present_api::MATERIAL_CREATE,
    present_api::MATERIAL_UPDATE,
    present_api::MATERIAL_DUPLICATE,
    present_api::MATERIAL_DELETE,
    present_api::MATERIAL_ADD_DEFAULTS,
    present_api::MATERIAL_IMPORT_TEXTURE,
    present_api::MATERIAL_ASSIGN,
    present_api::MATERIAL_SLOTS,
    present_api::RENDER_STYLE_LIST,
    present_api::RENDER_STYLE_GET,
    present_api::RENDER_STYLE_CREATE,
    present_api::RENDER_STYLE_UPDATE,
    present_api::RENDER_STYLE_DUPLICATE,
    present_api::RENDER_STYLE_DELETE,
    present_api::RENDER_STYLE_SET_ACTIVE,
    present_api::SUN_SET,
    present_api::SUN_GET,
    present_api::RENDER_HIDDEN_LINE,
    // GIS services, coordinate conversion and massing models (see gis_api.rs and massing_api.rs).
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_SOURCES,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_CACHE_CLEAR,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_GEOREF_SET,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_COORDS,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_REPROJECT,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_WMS_CAPABILITIES,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_WMS_ADD,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_WMS_REFRESH,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_WMTS_CAPABILITIES,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_WMTS_ADD,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_WFS_CAPABILITIES,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_WFS_IMPORT,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_GEOIMAGE_IMPORT,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_JOB_STATUS,
    #[cfg(not(target_arch = "wasm32"))]
    gis_api::GIS_JOB_CANCEL,
    massing_api::MASSING_ADD,
    massing_api::MASSING_SET,
    massing_api::MASSING_GET,
    massing_api::MASSING_FROM_SHAPES,
    massing_api::MASSING_FOLLOW_SOURCES,
    massing_api::MASSING_REPORT,
    // The surface model and the modeler (see mesh_api).
    mesh_api::MESH_SUMMARY,
    mesh_api::MESH_VERTICES,
    mesh_api::MESH_EDGES,
    mesh_api::MESH_FACES,
    mesh_api::MESH_SOLID_REPORT,
    mesh_api::MESH_BOUNDS,
    mesh_api::MESH_NEAR,
    mesh_api::MESH_PICK,
    mesh_api::MESH_CREATE,
    mesh_api::MESH_ADD_EDGES,
    mesh_api::MESH_ADD_CURVE,
    mesh_api::MESH_ERASE,
    mesh_api::MESH_REVERSE_FACES,
    mesh_api::MESH_PUSH_PULL,
    mesh_api::MESH_FOLLOW_ME,
    mesh_api::MESH_OFFSET,
    mesh_api::MESH_INTERSECT_FACES,
    mesh_api::MESH_TRANSFORM,
    mesh_api::MESH_COPY,
    mesh_api::MESH_PAINT,
    mesh_api::MESH_HIDE,
    mesh_api::MESH_SOFTEN,
    mesh_api::MESH_SOFTEN_EDGES,
    mesh_api::MESH_TO_SURFACE,
    mesh_api::MESH_SOLID,
    mesh_api::MESH_MAKE_GROUP,
    mesh_api::MESH_MAKE_COMPONENT,
    mesh_api::MESH_EXPLODE,
    mesh_api::MESH_BATCH,
    mesh_api::MESH_RENDER,
    // Extrude, sweep and 3D polygon (see solid_api.rs).
    solid_api::EXTRUDE_ADD,
    solid_api::EXTRUDE_SET,
    solid_api::EXTRUDE_GET,
    solid_api::EXTRUDE_FROM_SHAPES,
    solid_api::SWEEP_ADD,
    solid_api::SWEEP_SET,
    solid_api::SWEEP_GET,
    solid_api::SWEEP_FROM_SHAPES,
    solid_api::POLYGON3D_ADD,
    solid_api::POLYGON3D_SET,
    solid_api::POLYGON3D_GET,
    // Hardscapes and landscape areas (see hardscape_api.rs).
    hardscape_api::HARDSCAPE_STYLE_LIST,
    hardscape_api::HARDSCAPE_STYLE_GET,
    hardscape_api::HARDSCAPE_STYLE_CREATE,
    hardscape_api::HARDSCAPE_STYLE_UPDATE,
    hardscape_api::HARDSCAPE_STYLE_DUPLICATE,
    hardscape_api::HARDSCAPE_STYLE_DELETE,
    hardscape_api::LANDSCAPE_AREA_STYLE_LIST,
    hardscape_api::LANDSCAPE_AREA_STYLE_GET,
    hardscape_api::LANDSCAPE_AREA_STYLE_CREATE,
    hardscape_api::LANDSCAPE_AREA_STYLE_UPDATE,
    hardscape_api::LANDSCAPE_AREA_STYLE_DUPLICATE,
    hardscape_api::LANDSCAPE_AREA_STYLE_DELETE,
    hardscape_api::HARDSCAPE_ADD,
    hardscape_api::HARDSCAPE_SET,
    hardscape_api::HARDSCAPE_FROM_SHAPES,
    pattern_api::UNIT_PATTERN_LAYOUT,
    pattern_api::UNIT_PATTERN_TO_OBJECTS,
    pattern_api::UNIT_PATTERN_MODULE_PRESET,
    pattern_api::UNIT_PATTERN_MODULE_FROM_SHAPES,
    #[cfg(not(target_arch = "wasm32"))]
    pattern_api::UNIT_PATTERN_MODULE_FROM_BLOCK,
    pattern_api::REPORT_UNIT_PATTERN,
    plan_api::REGION_FROM_CURVES,
    plan_api::HARDSCAPE_SPLIT,
    plan_api::HARDSCAPE_COURSE,
    plan_api::HARDSCAPE_COURSE_REMOVE,
    plan_api::HARDSCAPE_PLAN,
    plan_api::HARDSCAPE_PLAN_CLEAR,
    plan_api::HARDSCAPE_ZONE_SET,
    plan_api::UNIT_PATTERN_CHECK,
    hardscape_api::LANDSCAPE_AREA_ADD,
    hardscape_api::LANDSCAPE_AREA_SET,
    hardscape_api::REPORT_HARDSCAPE,
    hardscape_api::REPORT_LANDSCAPE_AREAS,
    hardscape_api::REPORT_PLANT_LIST,
    // Irrigation (see irrigation_api.rs).
    irrigation_api::OUTLET_ADD,
    irrigation_api::LINE_ADD,
    irrigation_api::POC_ADD,
    irrigation_api::VALVE_ADD,
    irrigation_api::CONTROLLER_ADD,
    irrigation_api::SET,
    irrigation_api::CATALOG_GET,
    irrigation_api::CATALOG_SET,
    irrigation_api::CATALOG_IMPORT_CSV,
    irrigation_api::CATALOG_EXPORT_CSV,
    irrigation_api::CALCULATE,
    irrigation_api::LAYOUT_FILL,
    irrigation_api::LAYOUT_EDGE,
    irrigation_api::LAYOUT_DRIP,
    irrigation_api::REPORT_ZONES,
    irrigation_api::REPORT_PIPES,
    irrigation_api::REPORT_PRESSURE,
    irrigation_api::REPORT_MATERIALS,
    irrigation_api::REPORT_HYDROZONES,
    // Grading and site analysis (see grading_api.rs).
    grading_api::WALL_STYLE_CREATE,
    grading_api::WALL_STYLE_LIST,
    grading_api::WALL_STYLE_UPDATE,
    grading_api::WALL_STYLE_DELETE,
    grading_api::WALL_ADD,
    grading_api::WALL_SET,
    grading_api::WALL_REPORT,
    grading_api::GRADE_ADD,
    grading_api::GRADE_SET,
    grading_api::GRADE_GET,
    grading_api::SITE_ANALYSIS,
    grading_api::SPOT_ADD,
    grading_api::SPOT_SET,
    grading_api::SPOT_GET,
    grading_api::SECTION_ADD,
    grading_api::SECTION_SET,
    grading_api::SECTION_PROFILE,
    grading_api::SECTION_PLACE,
    grading_api::PAD_ADD,
    grading_api::GRADE_LIMIT_ADD,
    grading_api::PAD_RETAINING_EDGE,
    // Plug-in objects and scripts (see plugin_api.rs).
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_DEFINE,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_UPDATE,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_DELETE,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_LIST,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_GET,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_EXAMPLES,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_INSTALL_EXAMPLE,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_PLACE,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_SET_PARAMS,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_RESET,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_OBJECT,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::PLUGIN_PREVIEW,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::SCRIPT_INFO,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::SCRIPT_RUN,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::SCRIPT_STATUS,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::SCRIPT_STOP,
    #[cfg(not(target_arch = "wasm32"))]
    plugin_api::SCRIPT_LIST,
    // WebAssembly plug-ins (see wasm_plugin_api.rs).
    wasm_plugin_api::PLUGINS_INFO,
    wasm_plugin_api::PLUGINS_LIST,
    wasm_plugin_api::PLUGINS_INSTALL,
    wasm_plugin_api::PLUGINS_REMOVE,
    wasm_plugin_api::PLUGINS_PLACE,
    wasm_plugin_api::PLUGINS_SET_PARAMS,
    wasm_plugin_api::PLUGINS_RESET,
    wasm_plugin_api::PLUGINS_OBJECT,
    wasm_plugin_api::PLUGINS_PREVIEW,
    wasm_plugin_api::PLUGINS_TOOL_STEP,
    // Section and elevation viewports (see x3d_api.rs).
    #[cfg(not(target_arch = "wasm32"))]
    x3d_api::LINE_ADD,
    #[cfg(not(target_arch = "wasm32"))]
    x3d_api::LINE_SET,
    #[cfg(not(target_arch = "wasm32"))]
    x3d_api::CREATE,
    #[cfg(not(target_arch = "wasm32"))]
    x3d_api::UPDATE,
    #[cfg(not(target_arch = "wasm32"))]
    x3d_api::GET,
    // Groups, symbols, patterns and the Resource Manager (see symbols_api.rs).
    symbols_api::GROUP_CREATE,
    symbols_api::GROUP_UNGROUP,
    symbols_api::GROUP_ENTER,
    symbols_api::GROUP_EXIT,
    symbols_api::GROUP_CONTEXT,
    symbols_api::GROUP_MEMBERS,
    symbols_api::GROUP_MOVE_INTO,
    symbols_api::GROUP_MOVE_OUT,
    symbols_api::GROUP_MOVE,
    symbols_api::GROUP_ROTATE,
    symbols_api::GROUP_COPY,
    symbols_api::SELECTION_CLICK,
    symbols_api::SYMBOL_LIST,
    symbols_api::SYMBOL_GET,
    symbols_api::SYMBOL_CREATE,
    symbols_api::SYMBOL_FROM_OBJECTS,
    symbols_api::SYMBOL_PLACE,
    symbols_api::SYMBOL_SET,
    symbols_api::SYMBOL_CONVERT_TO_GROUP,
    symbols_api::SYMBOL_EDIT,
    symbols_api::SYMBOL_EDIT_END,
    symbols_api::RESOURCE_LIST,
    symbols_api::RESOURCE_GET,
    symbols_api::RESOURCE_CREATE,
    symbols_api::RESOURCE_UPDATE,
    symbols_api::RESOURCE_RENAME,
    symbols_api::RESOURCE_DUPLICATE,
    symbols_api::RESOURCE_DELETE,
    symbols_api::RESOURCE_PURGE_UNUSED,
    symbols_api::RESOURCE_THUMBNAIL,
    symbols_api::PATTERN_SEGMENTS,
    #[cfg(not(target_arch = "wasm32"))]
    symbols_api::RESOURCE_LIST_FILE,
    #[cfg(not(target_arch = "wasm32"))]
    symbols_api::RESOURCE_IMPORT_FROM_FILE,
    #[cfg(not(target_arch = "wasm32"))]
    symbols_api::RESOURCE_EXPORT_TO_FILE,
    #[cfg(not(target_arch = "wasm32"))]
    symbols_api::LIBRARY_LIST,
    #[cfg(not(target_arch = "wasm32"))]
    symbols_api::LIBRARY_FAVORITES,
    #[cfg(not(target_arch = "wasm32"))]
    symbols_api::LIBRARY_FAVORITE_ADD,
    #[cfg(not(target_arch = "wasm32"))]
    symbols_api::LIBRARY_FAVORITE_REMOVE,
    (
        "site.create",
        "Create a site model (terrain) from 3D loci and 3D polylines. They move into the site model as its source data. params: {source_ids: [id], layer? (design layer of the site model; default: the layer of the first source), settings?}. settings: minor_interval, major_interval (mm), smooth_contours, label_contours, display_2d and surface_3d (existing | proposed | both), display_3d (mesh | contours | both), datum, existing_color, proposed_color, crop (closed ring [[x, y]], may be concave; only the surface inside stays; [] for none), auto_update (default true; false keeps the old proposed surface until site.refresh), and the 3D look surface_look (color | texture | elevation), texture (material resource id), texture_tile (mm), texture_offset ([x, y]), texture_rotation (degrees), elevation_bands, elevation_ramp ([[r, g, b, a]]), elevation_range ([low, high]), elevation_bands_at_contours; site.texture sets them with checks. Pads and grade limits are objects of kind site_modifier on the same layer. Where pads overlap, the later object wins. A pad without a grade limit has a vertical edge. The sources keep the import tags of the objects they came from (data.import_sources of the site model, one entry for each point and line), so import.update and import.apply replace, add and remove them inside the model. Plants, trees, hardscapes, landscape areas, massing models and walls stand on the site model that covers their place, on any design layer (a layer or an object can name one with the command set_layer_site or data.site_model). Returns {id}.",
    ),
    (
        "site.update",
        "Change a site model. params: {id, settings?, add_source_ids? (3D loci and 3D polylines that move into the model), remove_points?: [index], remove_lines?: [index], replace? ([{point: index | line: index, with: id}]: the locus3d or polyline3d takes the place of that source, with its own import tag, instead of standing next to it)}. settings has the same fields as in site.create, including the 3D look: surface_look (color | texture | elevation), texture (material resource id or null), texture_tile (mm), texture_offset ([x, y] mm), texture_rotation (degrees), elevation_bands (1 to 64), elevation_ramp ([[r, g, b, a]], 1 to 16 colors), elevation_range ([low, high] mm or null), elevation_bands_at_contours (bool); null puts a field back to its default. site.texture sets the look with its own checks. Returns {id, points, lines}.",
    ),
    site_texture::SITE_TEXTURE,
    bed_api::BED_ADD,
    bed_api::BED_SET,
    bed_api::BED_GET,
    bed_api::BED_LIST,
    (
        "site.elevation",
        "Ground height at a point. params: {id, x, y, surface?: existing | proposed}. Returns {z, slope, aspect}; z is null outside the model. aspect is in radians clockwise from +Y.",
    ),
    (
        "site.cut_fill",
        "Cut and fill volumes of the proposed surface against the existing one. params: {id}. Returns {cut_m3, fill_m3, net_m3, area_m2, out_of_date} (net is fill minus cut).",
    ),
    (
        "site.drape",
        "Put a 2D line on the proposed surface, with extra points where it crosses triangle edges. params: {id, points: [[x, y]], closed?: bool}. Returns {points: [[x, y, z]]}. Fails if a point is outside the model.",
    ),
    (
        "site.refresh",
        "Update Site Model: rebuild the surfaces now. Needed when auto_update is false and the source data or a modifier has changed (cut_fill shows out_of_date: true). params: {id}. Returns {id, out_of_date, modifiers, has_proposed}.",
    ),
    (
        "site.contours",
        "Contour lines. params: {id, surface?: existing | proposed, points?: bool (default true)}. Returns {count, levels, contours: [{level, major, closed, points}]}.",
    ),
    // Survey data (see survey_api.rs).
    survey_api::SURVEY_IMPORT_POINTS,
    survey_api::TREE_IMPORT_SURVEY,
    survey_api::SITE_CHECK,
    survey_api::SITE_SIMPLIFY,
    survey_api::GEOREF_SURVEY_POINT,
    // Display rules: grid, phases, visualizations (see rules_api.rs).
    rules_api::VIEW_RULES_GET,
    rules_api::GRID_SET,
    rules_api::PHASE_SET,
    rules_api::PHASE_DISPLAY,
    rules_api::PHASE_REPORT,
    rules_api::VISUALIZATION_SET,
    rules_api::VISUALIZATION_AUTO,
    rules_api::VISUALIZATION_GET,
    rules_api::VISUALIZATION_CLEAR,
    (
        "view.screenshot",
        "A PNG picture of the live drawing area of the app window, as the user sees it (the plan or the 3D view with palettes left out). Only the app (kadakas app with --agent-port) answers; the headless server gives an error. params: {region? (drawing (default) | window: the whole window), max_width? (px, 64 to 4096; larger pictures are scaled down)}. Returns {png_base64, width, height, region, theme (light | dark), view (the current view id)}.",
    ),
    // What the agent changed (see agent_activity_api.rs).
    agent_activity_api::AGENT_ACTIVITY,
    // The agent drives the view of the person and points; only the app answers (see agent_view_api.rs).
    agent_view_api::VIEW_GET,
    agent_view_api::VIEW_ZOOM,
    agent_view_api::VIEW_PAN,
    agent_view_api::VIEW_CAMERA,
    agent_view_api::VIEW_HIGHLIGHT,
    agent_view_api::VIEW_HIGHLIGHT_CLEAR,
    // Editing 2D geometry (see edit2d_api.rs).
    edit2d_api::EDIT_ROTATE,
    edit2d_api::EDIT_SCALE,
    edit2d_api::EDIT_MIRROR,
    edit2d_api::EDIT_ARRAY,
    edit2d_api::EDIT_OFFSET,
    edit2d_api::EDIT_FILLET,
    edit2d_api::EDIT_CHAMFER,
    edit2d_api::EDIT_TRIM,
    edit2d_api::EDIT_EXTEND,
    edit2d_api::EDIT_SPLIT,
    edit2d_api::SURFACE_ADD,
    edit2d_api::SURFACE_SUBTRACT,
    edit2d_api::SURFACE_CLIP,
    edit2d_api::SURFACE_INTERSECT,
    edit2d_api::SURFACE_COMPOSE,
    // Queries, snapping and visibility modes (see query_api.rs).
    query_api::OBJECTS_QUERY,
    query_api::SNAP_GET,
    query_api::SNAP_SET,
    query_api::SNAP_QUERY,
    query_api::SNAP_SMART,
    query_api::VISIBILITY_GET,
    query_api::VISIBILITY_SET,
    // Drawing documentation (see sheets_api.rs).
    sheets_api::SHEET_LIST,
    sheets_api::SHEET_CREATE,
    sheets_api::SHEET_SET,
    sheets_api::SHEET_RENDER,
    sheets_api::PROJECT_GET,
    sheets_api::PROJECT_SET,
    sheets_api::PROJECT_NEW_GUID,
    sheets_api::VIEWPORT_CREATE,
    sheets_api::VIEWPORT_GET,
    sheets_api::VIEWPORT_LIST,
    sheets_api::VIEWPORT_UPDATE,
    sheets_api::VIEWPORT_SET_OVERRIDE,
    sheets_api::VIEWPORT_ANNOTATE,
    sheets_api::TITLE_BLOCK_CREATE,
    sheets_api::TITLE_BLOCK_UPDATE,
    sheets_api::TITLE_BLOCK_RESOLVE,
    sheets_api::SAVED_VIEW_LIST,
    sheets_api::SAVED_VIEW_SAVE,
    sheets_api::SAVED_VIEW_RESTORE,
    sheets_api::SAVED_VIEW_DELETE,
    sheets_api::SAVED_VIEW_RENAME,
    sheets_api::SAVED_VIEW_MOVE,
    // Annotation (see annotation_api.rs).
    annotation_api::TEXT_STYLE_LIST,
    annotation_api::TEXT_STYLE_GET,
    annotation_api::TEXT_STYLE_CREATE,
    annotation_api::TEXT_STYLE_UPDATE,
    annotation_api::TEXT_STYLE_DELETE,
    annotation_api::DIMENSION_STYLE_LIST,
    annotation_api::DIMENSION_STYLE_GET,
    annotation_api::DIMENSION_STYLE_CREATE,
    annotation_api::DIMENSION_STYLE_UPDATE,
    annotation_api::DIMENSION_STYLE_DELETE,
    annotation_api::TAG_STYLE_LIST,
    annotation_api::TAG_STYLE_GET,
    annotation_api::TAG_STYLE_CREATE,
    annotation_api::TAG_STYLE_UPDATE,
    annotation_api::TAG_STYLE_DELETE,
    annotation_api::NOTES_DB_LIST,
    annotation_api::NOTES_DB_GET,
    annotation_api::NOTES_DB_CREATE,
    annotation_api::NOTES_DB_UPDATE,
    annotation_api::NOTES_DB_DELETE,
    annotation_api::DIMENSION_ADD,
    annotation_api::CALLOUT_ADD,
    annotation_api::DATA_TAG_ADD,
    annotation_api::DATA_TAG_TAG_ALL,
    annotation_api::LEGEND_ADD,
    annotation_api::NORTH_ARROW_ADD,
    annotation_api::SCALE_BAR_ADD,
    annotation_api::REVISION_CLOUD_ADD,
    annotation_api::ANNOTATION_TEXT,
    annotation_api::ANNOTATION_UPDATE,
    annotation_api::ANNOTATION_LIST,
    annotation_api::ANNOTATION_EVALUATE,
    annotation_api::ANNOTATION_APPLY_TEXT_STYLE,
    annotation_api::REVISION_LIST,
    // Versions and the diff (see review_api.rs).
    review_api::VERSION_CREATE,
    review_api::VERSION_LIST,
    review_api::VERSION_GET,
    review_api::VERSION_RESTORE,
    review_api::VERSION_DELETE,
    review_api::VERSION_DIFF,
    review_api::VERSION_REPAIR,
    // Issues and BCF files (see issue_api.rs).
    issue_api::ISSUE_LIST,
    issue_api::ISSUE_GET,
    issue_api::ISSUE_CREATE,
    issue_api::ISSUE_UPDATE,
    issue_api::ISSUE_ADD_VIEWPOINT,
    issue_api::ISSUE_COMMENT,
    issue_api::ISSUE_RESOLVE,
    issue_api::ISSUE_DELETE,
    issue_api::ISSUE_IMPORT_BCF,
    issue_api::ISSUE_EXPORT_BCF,
    #[cfg(not(target_arch = "wasm32"))]
    dxf_compare_api::DXF_COMPARE,
    // Collaboration (see collab_api.rs).
    collab_api::STATUS,
    collab_api::CONNECT,
    collab_api::DISCONNECT,
    collab_api::SYNC,
    collab_api::PRESENCE,
    collab_api::UNDO,
    collab_api::REDO,
    (
        "events.subscribe",
        "Get change events. params: {kinds?: [objects, layers, classes, resources, document, issues]}. After each change the server sends a notification (no id) {method: \"events.change\", params: {revision, op, origin, added, changed, removed, document}}; added, changed and removed list IDs by kind. A replaced document gives events.reset {revision}. Over stdio and TCP only (not MCP). Over stdio a notification comes before the reply of the call that caused it; the app also sends the changes of the user at any time. Returns {revision, kinds}.",
    ),
    (
        "events.unsubscribe",
        "Stop the change events. Returns {done}.",
    ),
    (
        "events.poll",
        "Change sets after a revision. params: {since_revision, kinds?}. Returns {revision, truncated, changes}. If truncated is true, older changes were dropped: read the document again.",
    ),
    // Feedback (see feedback_api.rs).
    feedback_api::FEEDBACK_SUBMIT,
    feedback_api::FEEDBACK_LIST,
    feedback_api::FEEDBACK_GET,
    feedback_api::FEEDBACK_WITHDRAW,
    script_js_api::SCRIPT_EVAL,
    script_js_api::SCRIPT_JS_INFO,
];

impl Default for Session {
    /// A session with a new, empty document. The project has a new GUID.
    fn default() -> Self {
        let mut doc = Document::default();
        doc.drawing.project.guid = kadakas_core::guid::random();
        Session {
            editor: Editor::new(doc),
            ..Session::blank()
        }
    }
}

impl Session {
    /// Names the person at this session (the app uses the name of the account). The API checks
    /// the consent to delete the work of somebody else against it.
    pub fn set_identity(&mut self, name: &str) {
        self.identity = Some(name.trim().to_string()).filter(|n| !n.is_empty());
    }

    /// Like `Session::with_sequential_ids()`, but the new IDs count 2, 3, 4 and so on, as small numbers, so a
    /// test or a fixture can predict them. They are not unique in the world: never use this for a
    /// document that is merged with others.
    pub fn with_sequential_ids() -> Self {
        let mut doc = Document::default();
        doc.drawing.project.guid = kadakas_core::guid::random();
        doc.use_sequential_ids(2);
        Session {
            editor: Editor::new(doc),
            sequential_ids: true,
            ..Session::blank()
        }
    }

    /// Switches the predictable IDs on or off for this session. On: the IDs of the open document
    /// continue from its largest small ID, and so do the documents that replace it.
    pub fn set_sequential_ids(&mut self, on: bool) {
        self.sequential_ids = on;
        if on {
            self.editor.use_sequential_ids();
        }
    }

    /// All fields at their defaults, with an empty document that has no project GUID.
    fn blank() -> Self {
        // The sheets and the API share the section drawings.
        #[cfg(not(target_arch = "wasm32"))]
        let sheet_cache = kadakas_print::PageCache::default();
        #[cfg(not(target_arch = "wasm32"))]
        let sections = sheet_cache.sections.clone();
        Session {
            editor: Editor::default(),
            selection: Vec::new(),
            context: Vec::new(),
            view: ViewSettings::default(),
            snap: Default::default(),
            smart: Default::default(),
            replaced: false,
            view_changed: false,
            collab: Default::default(),
            identity: None,
            events: Default::default(),
            conn: events::LOCAL,
            limits: SessionLimits::default(),
            sequential_ids: false,
            guard_unsaved: false,
            blocked: None,
            mesh_cache: MeshCache::default(),
            mesh_query: Default::default(),
            lod_scene: None,
            #[cfg(not(target_arch = "wasm32"))]
            paths: PathPolicy::default(),
            #[cfg(not(target_arch = "wasm32"))]
            doc_dir: None,
            #[cfg(not(target_arch = "wasm32"))]
            seen: Default::default(),
            file_source: None,
            #[cfg(not(target_arch = "wasm32"))]
            gis: GisState::default(),
            site_cache: kadakas_site::SiteCache::default(),
            annot_cache: kadakas_annot::AnnotCache::default(),
            #[cfg(not(target_arch = "wasm32"))]
            consent: consent::ConsentState::default(),
            ghost_caches: GhostCaches::default(),
            worksheet_cache: kadakas_sheet::WorksheetCache::default(),
            #[cfg(not(target_arch = "wasm32"))]
            sheet_cache,
            #[cfg(not(target_arch = "wasm32"))]
            section_cache: sections,
            applied_view: None,
            applied_camera_only: false,
            #[cfg(not(target_arch = "wasm32"))]
            scripting: ScriptingState::default(),
            wasm_plugins: WasmPluginState::default(),
            script_js: Default::default(),
            review: Default::default(),
            compare: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            updates: Default::default(),
            load_notes: Vec::new(),
            feedback: feedback_api::FeedbackState::default(),
            tainted: false,
            activity: Default::default(),
            session_version: false,
            agent_hold: false,
        }
    }

    /// What the load of the open document repaired instead of refusing the file: the notes of the
    /// file format check and of the check of the versions. A host shows them to the person.
    pub fn load_notes(&self) -> &[String] {
        &self.load_notes
    }

    /// How often the annotation cache made primitives and legend rows, as `(primitives, rows)`.
    /// Tests use it to check the work done (a cache hit makes no new entry).
    #[doc(hidden)]
    pub fn annotation_builds(&self) -> (u64, u64) {
        self.annot_cache.builds()
    }

    /// The SVG of the document with the site model and annotation extensions. `render.svg`, the
    /// command `kadakas render` and the app's SVG export all use this one function.
    /// The width is in px, 16 to 8192.
    pub fn render_svg(&self, width: u32) -> String {
        self.render_svg_with(width, &kadakas_core::svg::RenderOptions::default())
    }

    /// Like [`Session::render_svg`], for a region of the drawing and with the findings of the
    /// pattern check marked (`render.svg {region, show_check}`).
    pub fn render_svg_with(&self, width: u32, opts: &kadakas_core::svg::RenderOptions) -> String {
        svg_of_with(
            self.editor.doc(),
            width,
            self.editor.revision(),
            &self.site_cache,
            &self.annot_cache,
            None,
            None,
            &kadakas_core::svg::RenderOptions {
                max_units: opts.max_units.or(Some(self.limits.pattern_units)),
                ..*opts
            },
        )
    }

    /// A session for an opened document, with empty history and selection.
    pub fn with_document(mut doc: Document) -> Self {
        let mut cache = review_api::vet_document(&mut doc, SessionLimits::default().read_caps());
        let mut notes = doc.load_notes().to_vec();
        notes.extend(plants_api::ground_height_note(&doc));
        let (mut s, exact) = Session::with_vetted_document(doc);
        s.load_notes = notes;
        cache.arm(s.editor.revision(), exact);
        s.review = cache;
        s
    }

    /// Like `with_document`, for a document whose history was checked already. The flag is true
    /// when the document is as it was read (an old file gets a project GUID in memory, which it
    /// had not in the file).
    fn with_vetted_document(mut doc: Document) -> (Self, bool) {
        // An old file has no project GUID. It gets one in memory, and the next save writes it.
        let exact = !doc.ensure_project_guid();
        (
            Session {
                editor: Editor::new(doc),
                ..Session::blank()
            },
            exact,
        )
    }

    /// Replaces the document, as `document.load` does. Sets the `replaced` flag. The history of
    /// versions of the new document is checked against the limits of the session; what was
    /// repaired or dropped is returned as notes (they are also the load notes of the document).
    /// Sets the file that the open document was read from, with the limit of the session for the
    /// pictures that a rescue holds in memory (`SessionLimits::rescue_bytes`).
    pub fn set_file_source(&mut self, source: Option<Arc<kadakas_file::FileSource>>) {
        if let Some(src) = &source {
            src.set_rescue_limit(self.limits.rescue_bytes);
        }
        self.file_source = source;
    }

    pub fn replace_document(&mut self, doc: Document) -> Vec<String> {
        self.replace_document_as(doc, true)
    }

    /// [`Session::replace_document`]. `is_file_drawing` is false when the drawing is not the one
    /// that is stored in the file (for example gray placeholders were replaced by the real
    /// pictures at the open): then a history that waits in the file does not take the drawing
    /// for the anchor of its versions, and does not become read only because they differ.
    pub fn replace_document_as(&mut self, mut doc: Document, is_file_drawing: bool) -> Vec<String> {
        let mut cache = review_api::vet_document(&mut doc, self.limits.read_caps());
        let mut notes = doc.load_notes().to_vec();
        notes.extend(doc.placeholder_note());
        notes.extend(plants_api::ground_height_note(&doc));
        #[cfg(not(target_arch = "wasm32"))]
        let (paths, doc_dir) = (std::mem::take(&mut self.paths), self.doc_dir.take());
        #[cfg(not(target_arch = "wasm32"))]
        let gis = std::mem::take(&mut self.gis);
        #[cfg(not(target_arch = "wasm32"))]
        let scripting = std::mem::take(&mut self.scripting);
        let wasm_plugins = std::mem::take(&mut self.wasm_plugins);
        let limits = self.limits.clone();
        let collab = std::mem::take(&mut self.collab);
        // The outbox, the consent, the rate limit and the flags of the run belong to the program,
        // not to the document. The calls and the last failure belong to the old document.
        let mut feedback = std::mem::take(&mut self.feedback);
        feedback.clear_context();
        let mut hub = std::mem::take(&mut self.events);
        hub.document_replaced();
        let sequential = self.sequential_ids;
        let session_version = self.session_version;
        let guard_unsaved = self.guard_unsaved;
        let (fresh, exact) = Session::with_vetted_document(doc);
        *self = Session {
            guard_unsaved,
            replaced: true,
            view: self.view,
            snap: self.snap,
            ..fresh
        };
        self.events = hub;
        self.session_version = session_version;
        cache.arm(self.editor.revision(), exact && is_file_drawing);
        self.review = cache;
        self.feedback = feedback;
        self.collab = collab;
        self.collab_mark_saved();
        self.set_sequential_ids(sequential);
        self.wasm_plugins = wasm_plugins;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.paths = paths;
            self.doc_dir = doc_dir;
            self.gis = gis;
            self.scripting = scripting;
            // Jobs that were started for the old document must not change this one.
            self.gis.epoch += 1;
        }
        self.set_limits(limits);
        self.load_notes = notes.clone();
        notes
    }

    /// The canonical path for a file request, if the path policy allows it. `exts` are the allowed
    /// extensions (lower case, without the dot); the check applies to restricted sessions only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn check_path(
        &self,
        path: &std::path::Path,
        write: bool,
        exts: &[&str],
    ) -> Result<std::path::PathBuf, String> {
        if !self.paths.is_restricted() {
            return Ok(path.to_path_buf());
        }
        {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
            if !ext.as_deref().is_some_and(|e| exts.contains(&e)) {
                return Err(format!(
                    "{} must have the extension {}",
                    path.display(),
                    exts.iter()
                        .map(|e| format!(".{e}"))
                        .collect::<Vec<_>>()
                        .join(" or ")
                ));
            }
        }
        self.paths.resolve(path, self.doc_dir.as_deref(), write)
    }

    /// The canonical path of a folder, if the path policy allows it (no extension rule). A
    /// restricted session may read inside the folders it was given.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn check_dir(&self, path: &std::path::Path) -> Result<std::path::PathBuf, String> {
        if !self.paths.is_restricted() {
            return std::fs::canonicalize(path).map_err(|e| e.to_string());
        }
        self.paths.resolve(path, self.doc_dir.as_deref(), false)
    }

    /// The 3D scene of the current document. It is rebuilt only when the document changed.
    pub fn scene(&mut self) -> Arc<Scene> {
        let revision = self.editor.revision();
        let ext = kadakas_site::draw::scene_ext(&self.site_cache, revision);
        self.mesh_cache
            .get_with(revision, self.editor.doc(), Some(&ext))
    }

    /// The scene for a view in which one pixel is `pixel_size` mm wide at the target: plants that
    /// cover only a few pixels get billboards or coarse meshes (`lod_for`), never finer than the
    /// count rule gives. The scene is kept for the revision and the detail.
    pub fn scene_for_pixel_size(&mut self, pixel_size: f64) -> Arc<Scene> {
        use kadakas_core::mesh::{count_detail, mean_plant_spread};
        use kadakas_core::plants::form3d::lod_for;
        let doc = self.editor.doc();
        let Some(spread) = mean_plant_spread(doc) else {
            return self.scene();
        };
        let want = lod_for(spread, pixel_size).max(count_detail(doc));
        if want == count_detail(doc) {
            return self.scene();
        }
        let revision = self.editor.revision();
        if let Some((r, d, s)) = &self.lod_scene
            && *r == revision
            && *d == want
        {
            return s.clone();
        }
        let scene = self.scene_with_detail(want);
        self.lod_scene = Some((revision, want, scene.clone()));
        scene
    }

    /// The scene with the plants at a given detail (not cached). Exports use it, so that they do
    /// not get the billboards that the 3D view uses for very many plants.
    pub fn scene_with_detail(
        &mut self,
        detail: kadakas_core::plants::form3d::Detail,
    ) -> Arc<Scene> {
        let revision = self.editor.revision();
        let ext = kadakas_site::draw::scene_ext(&self.site_cache, revision);
        let doc = self.editor.doc();
        Arc::new(kadakas_core::mesh::build_with_detail(
            doc,
            kadakas_core::mesh::scene_origin(doc),
            Some(&ext),
            Some(detail),
        ))
    }

    /// The cache of site model results (TIN, contours, volumes), by document revision.
    pub fn site_cache(&self) -> &kadakas_site::SiteCache {
        &self.site_cache
    }

    /// True while a view change (`view.set`, `saved_view.restore`) waits for the host. It does
    /// not take the flag. The app asks it to keep the view from before the change (Back).
    /// True when the pending saved view came from `view.set` (its camera only).
    pub fn applied_camera_only(&self) -> bool {
        self.applied_camera_only
    }

    pub fn has_view_change(&self) -> bool {
        self.view_changed || self.applied_view.is_some()
    }

    /// True once after `view.set` changed the view. A host with a window should
    /// poll this after `handle` and show `self.view`.
    pub fn take_view_changed(&mut self) -> bool {
        std::mem::take(&mut self.view_changed)
    }

    /// True once after the document was replaced by `document.load` (or
    /// `replace_document`). A host that tracks a file path, tool state or
    /// view should poll this after `handle` and reset that state.
    pub fn take_replaced(&mut self) -> bool {
        std::mem::take(&mut self.replaced)
    }

    /// Runs one request. The limits of the session (`SessionLimits`) apply: a call that breaks one
    /// is undone and fails with a limit error.
    pub fn handle(&mut self, method: &str, p: Value) -> Result<Value, RpcError> {
        // The drawings of patterns (sheets, PDF, SVG) lay plans with the unit limit of this session,
        // as the reports do. Every call sets it, also the calls of a script run.
        kadakas_core::hardscape::draw::set_pattern_units(self.limits.pattern_units);
        self.recover_script_run();
        if self.script_js.busy() {
            return Err(RpcError::conflict(
                "a script is running: its calls are the only calls served until it ends",
            ));
        }
        let depth = self.editor.undo_depth();
        let revision = self.editor.revision();
        #[cfg(not(target_arch = "wasm32"))]
        let start = std::time::Instant::now();
        let note = feedback_api::begin(method, &p);
        // The parameter struct of the method (params): unknown parameters are refused, types and
        // required parameters are checked, names of layers and classes become IDs.
        let mut result = match params::prepare(self.editor.doc(), method, p) {
            Ok(p) => self.dispatch(method, p),
            Err(e) => Err(e),
        };
        feedback_api::finish(self, note, &result);
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = Some(start.elapsed());
        #[cfg(target_arch = "wasm32")]
        let elapsed = None;
        let steps = self.editor.undo_depth().saturating_sub(depth);
        let changed = self.editor.revision() != revision;
        // A client that waits for its first snapshot would lose the edit: the snapshot replaces
        // the document. Take the edit back and say why.
        if changed
            && result.is_ok()
            && !method.starts_with("collab.")
            && collab_api::awaiting_snapshot(self)
        {
            self.editor.rollback(steps);
            result = Err(RpcError::conflict(collab_api::WAIT_MESSAGE));
        }
        self.enforce_limits(steps, changed, elapsed, result)
    }

    fn dispatch(&mut self, method: &str, p: Value) -> Result<Value, RpcError> {
        if feedback_api::knows(method) {
            return feedback_api::handle(self, method, p).expect("a feedback method");
        }
        if script_js_api::knows(method) {
            return script_js_api::handle(self, method, p).expect("a script method");
        }
        if let Some(r) = events::handle(self, method, p.clone()) {
            return r;
        }
        if let Some(r) = agent_activity_api::handle(self, method, p.clone()) {
            return r;
        }
        if grading_api::knows(method) {
            return self.handle_grading(method, p);
        }
        if bed_api::knows(method) {
            return self.handle_bed(method, p);
        }
        if let Some(r) = survey_api::handle(self, method, p.clone()) {
            return r;
        }
        if let Some(r) = rules_api::handle(self, method, p.clone()) {
            return r;
        }
        if let Some(r) = review_api::handle(self, method, p.clone()) {
            return r;
        }
        if let Some(r) = self.handle_issue(method, p.clone()) {
            return r;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(r) = update_api::handle(self, method, p.clone()) {
            return r;
        }
        if let Some(r) = collab_api::handle(self, method, p.clone()) {
            return r;
        }
        if method == "document.load" {
            self.refuse_replace(Blocked::Load, method)?;
        }
        if method == "document.load" && collab_api::is_connected(self) {
            return Err(RpcError::conflict(
                "this session is connected to a shared document: call collab.disconnect before you load another document",
            ));
        }
        if matches!(method, "undo" | "redo") && collab_api::is_connected(self) {
            return Err(RpcError::conflict(format!(
                "this session is connected to a shared document: use collab.{method}, which takes back only your own edits"
            )));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if files::is_file_method(method) {
            return files::value_call(self, method, &p);
        }
        if method.starts_with("site.") {
            return self.handle_site(method, p);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if x3d_api::knows(method) {
            return self.handle_x3d(method, p);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if gis_api::is_gis_method(method) {
            return self.gis_call(method, p, false);
        }
        let doc = self.editor.doc();
        Ok(match method {
            "describe" => {
                // One method with the JSON Schema of its parameters (params).
                if let Some(name) = p.get("method").and_then(Value::as_str) {
                    return params::describe_method(name);
                }
                let want_schemas = p.get("schemas").and_then(Value::as_bool) == Some(true);
                let want_schema = p.get("schema").and_then(Value::as_bool) == Some(true);
                #[allow(unused_mut)]
                let mut d = json!({
                "methods": METHODS.iter().map(|(m, d)| json!({"name": m, "doc": d})).collect::<Vec<_>>(),
                "units": "millimeters, Y axis up. Angles are NOT one unit: each parameter states its own. Radians (counterclockwise): drafting and placing in the plan (edit.rotate and group.rotate angle, edit.array angle and rotate, edit.chamfer angle, dimension angle, symbol and plugin rotation, the rotation of a georeference and of an import, the camera yaw and pitch of an issue viewpoint, massing ridge_angle, the aspect that the ground query returns). Degrees (counterclockwise unless said): everything about patterns and hardscapes (hardscape.add, hardscape.set, hardscape.zone_set, hardscape_style joints angle, unit_pattern.* angle, slope angle_deg, split zone angle), irrigation, hatch and tile resources, solids (rotation_deg, sweep_angle, segment_angle), site.texture and texture bed rotation, mapping rotation, snap angle_step_deg, north_angle and every name that ends in _deg or degrees. The reply of a method uses the unit of the same name in its parameters.",
                "vertex_kinds": "Polyline points and landscape/hardscape boundaries are vertices. [x, y] is a corner. Other kinds are {p: [x, y], kind: corner | arc (with radius, a fillet) | point_on_arc | bezier | cubic}.",
                "command_examples": command_examples(),
                "plants": plants_api::describe(),
                "plant_pictures": picture_api::describe(),
                "data": data_api::describe(),
                "hardscapes": hardscape_api::describe(),
                "unit_patterns": pattern_api::describe(),
                "presentation": present_api::describe(),
                "massing": massing_api::describe(),
                "solids": solid_api::describe(),
                "mesh": mesh_api::describe(),
                "irrigation": irrigation_api::describe(),
                "grading": grading_api::describe(),
                "texture_beds": bed_api::describe(),
                    "sheets": sheets_api::describe(),
                "resources": symbols_api::describe(),
                "editing": edit2d_api::describe(),
                "queries": query_api::describe(),
                "annotation": annotation_api::describe(),
                "survey": survey_api::describe(),
                "display_rules": rules_api::describe(),
                "collaboration": collab_api::describe(),
                "review": review_api::describe(),
                "errors": errors::describe(),
                "files": limits::describe_files(),
                "feedback": feedback_api::describe(),
                "unknown_shapes": "A file from a newer Kadakas may hold shapes of a kind that this version does not know. They load as kind 'unknown' (the original kind is in shape.kind of objects.get). They are listed in document.summary (objects_by_kind.unknown and unknown_shapes), they are not drawn, they save back unchanged, and only delete_objects can change them.",
                });
                d["scripting"] = json!({"javascript": script_js_api::describe()});
                #[cfg(not(target_arch = "wasm32"))]
                {
                    d["io"] = io::describe();
                    d["io"]["update"] = update_api::describe();
                    d["review"]["dxf_compare"] = dxf_compare_api::describe();
                    d["gis"] = gis_api::describe();
                    let js = d["scripting"]["javascript"].take();
                    d["scripting"] = plugin_api::describe();
                    d["scripting"]["javascript"] = js;
                }
                d["plugins"] = wasm_plugin_api::describe();
                d["review"]["issues"] = issue_api::describe();
                d["review"]["agent_view"] = agent_view_api::describe();
                d["review"]["agent_activity"] = agent_activity_api::describe();
                d["schemas"] = json!({
                    "command": "The JSON Schema (draft 2020-12) of every command that `execute` takes. Call describe with {\"schema\": true} to get it in `command_schema` (about 30 KB). Everything the schema accepts for add_object, add_object_in, modify_object and set_vertex with an exact shape kind is accepted by the server too (a test fuzzes this). Other parts are structural only: they check the shape of the object and the kind, not every field, so the server may still refuse what the schema accepts. Their fields are in command_examples and in the sections of this answer.",
                    "exact_shape_kinds": kadakas_core::schema::EXACT_SHAPE_KINDS,
                    "structural_only_shape_kinds": kadakas_core::schema::structural_shape_kinds(),
                    "structural_only_commands": "Commands with a resource body (add_resource, modify_resource), a patch (set_hardscape, set_landscape_area), a saved view, a georeference, sun settings, a material slot target or a plant variation take an object for that field that the schema does not describe.",
                });
                if want_schema {
                    d["command_schema"] = kadakas_core::schema::command_schema();
                }
                d["parameters"] = json!(
                    "Every method refuses a parameter that it does not take, and says which one and what the nearest known name is. A wrong type, a missing required parameter or a value that is not allowed is an error that names the parameter and what it expects. The JSON Schema of the parameters of a method (names, types, required, allowed values, defaults) comes from describe {method: \"plants.place\"}; describe {schemas: true} returns them all in method_schemas (about 460 KB). A parameter that is a layer, class or sheet takes an ID or the unique name of the layer or class (also in the commands of execute, one command at a time, so a command of a batch can name a layer that an earlier command added); a name that matches none or several is an error that lists the candidates."
                );
                if want_schemas {
                    d["method_schemas"] = Value::Object(params::all_schemas());
                }
                d
            }
            "execute" => {
                #[derive(Deserialize)]
                struct P {
                    command: serde_json::Map<String, Value>,
                }
                let P { command } = params(p)?;
                let out = self.execute_json(command)?;
                self.prune_selection();
                serde_json::to_value(out).unwrap()
            }
            "undo" => {
                let done = self.editor.undo();
                self.prune_selection();
                json!({ "done": done })
            }
            "redo" => {
                let done = self.editor.redo();
                self.prune_selection();
                json!({ "done": done })
            }
            "document.summary" => {
                let mut kinds = BTreeMap::<&str, usize>::new();
                for o in doc
                    .objects
                    .values()
                    .filter(|o| !kadakas_core::guide::is_guide(o))
                {
                    *kinds.entry(o.shape.kind()).or_default() += 1;
                }
                let bounds = kadakas_core::svg::drawing_bounds(doc)
                    .map(|(a, b)| json!({"min": a, "max": b}));
                json!({
                    "layers": doc.layers,
                    "classes": doc.classes,
                    "active_layer": doc.active_layer,
                    "active_class": doc.active_class,
                    // Guides are counted in `guide_count` and not here.
                    "object_count": doc.objects.len() - doc.guide_count(),
                    "guide_count": doc.guide_count(),
                    "objects_by_kind": kinds,
                    "unknown_shapes": doc.objects.values().filter_map(|o| match &o.shape {
                        Shape::Unknown { kind, .. } => Some(json!({"id": o.id, "kind": kind})),
                        _ => None,
                    }).collect::<Vec<_>>(),
                    "bounds": bounds,
                    // Component instances that cannot be drawn, with the reason (a missing
                    // definition, a cycle, a limit). They show as a marked cross.
                    "broken_components": kadakas_core::component::findings(doc)
                        .into_iter()
                        .map(|f| json!({"id": f.object, "problem": f.message}))
                        .collect::<Vec<_>>(),
                    "georeference": doc.georef,
                    "grey": doc.drawing.grey,
                    "can_undo": self.editor.can_undo(),
                    "can_redo": self.editor.can_redo(),
                })
            }
            "document.get" => {
                // The versions have their own methods (version.*) and can be large.
                // A picture that stays in the file is read now; the file may have changed.
                let mut v = serde_json::to_value(doc).map_err(|e| {
                    let why = format!("the document cannot be read: {e}");
                    if why.contains("changed on disk after it was opened") {
                        RpcError::conflict(why)
                    } else {
                        RpcError::app(why)
                    }
                })?;
                if let Some(m) = v.as_object_mut() {
                    m.remove("history");
                }
                v
            }
            "document.load" => {
                #[derive(Deserialize)]
                struct P {
                    document: Value,
                }
                let P { document } = params(p)?;
                let mut document = Document::from_value(document).map_err(|e| match e {
                    LoadError::Parse(_) => RpcError::invalid_params(e),
                    e => RpcError::app(e),
                })?;
                // `document.get` leaves the versions out: a document without them keeps the
                // versions of the session.
                let brings_versions = document.history.is_some();
                self.keep_versions_for(&mut document)?;
                self.check_document_fits(&document)?;
                // A document that brings its own versions replaces the ones of the session.
                let replaced = brings_versions
                    .then(|| {
                        self.editor
                            .doc()
                            .history
                            .as_ref()
                            .and_then(|h| h.version_count())
                    })
                    .flatten()
                    .filter(|n| *n > 0);
                let mut notes = self.replace_document(document);
                if let Some(n) = replaced {
                    notes.push(format!(
                        "the {n} versions of the document that was open are gone: the loaded document has its own versions"
                    ));
                }
                if notes.is_empty() {
                    json!({ "ok": true })
                } else {
                    // Values that were repaired instead of refusing the file.
                    json!({ "ok": true, "notes": notes })
                }
            }
            "objects.list" => {
                let f: ObjectFilter = params(p)?;
                let list: Vec<&Object> = doc
                    .objects
                    .values()
                    .filter(|o| f.layer.is_none_or(|l| o.layer == l))
                    .filter(|o| f.class.is_none_or(|c| o.class == c))
                    .filter(|o| f.kind.as_deref().is_none_or(|k| o.shape.kind() == k))
                    // Guides are construction geometry: only a request for the kind lists them.
                    .filter(|o| {
                        f.kind.as_deref() == Some("guide") || !kadakas_core::guide::is_guide(o)
                    })
                    .collect();
                let mut v = serde_json::to_value(list).unwrap();
                v.as_array_mut().into_iter().flatten().for_each(|o| {
                    elide_image(o);
                    elide_sources(o);
                });
                v
            }
            "objects.get" => {
                #[derive(Deserialize)]
                struct P {
                    id: ObjectId,
                    #[serde(default)]
                    sources: bool,
                }
                let P { id, sources } = params(p)?;
                let o = doc
                    .object(id)
                    .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
                let mut v = serde_json::to_value(o).unwrap();
                elide_image(&mut v);
                if !sources {
                    elide_sources(&mut v);
                }
                v
            }
            "selection.get" => json!({ "ids": self.selection }),
            "selection.set" => {
                #[derive(Deserialize)]
                struct P {
                    ids: Vec<ObjectId>,
                }
                let P { ids } = params(p)?;
                if let Some(bad) = ids.iter().find(|id| doc.object(**id).is_none()) {
                    return Err(RpcError::not_found(format!("object {bad} does not exist")));
                }
                if let Some(g) = ids
                    .iter()
                    .find(|id| doc.object(**id).is_some_and(|o| doc.is_grey(o)))
                {
                    return Err(RpcError::validation(format!(
                        "object {g} is on a grey layer or in a grey class and cannot be selected"
                    )));
                }
                self.selection = ids;
                json!({ "ids": self.selection })
            }
            "render.svg" => {
                #[derive(Deserialize)]
                struct P {
                    width: Option<u32>,
                    compare: Option<Value>,
                    region: Option<[f64; 4]>,
                    show_check: Option<bool>,
                }
                let P {
                    width,
                    compare,
                    region,
                    show_check,
                } = params(p)?;
                let region = region_param(region)?;
                match compare_api::CompareSpec::parse(compare.as_ref())? {
                    Some(spec) => self.render_svg_compare(width.unwrap_or(800), &spec)?,
                    None => json!({ "svg": self.render_svg_with(
                        width.unwrap_or(800),
                        &kadakas_core::svg::RenderOptions {
                            region,
                            show_check: show_check.unwrap_or(false),
                            max_units: Some(self.limits.pattern_units),
                        },
                    ) }),
                }
            }
            "render.png" => {
                #[derive(Deserialize)]
                struct P {
                    view: Option<String>,
                    width: Option<u32>,
                    height: Option<u32>,
                    projection: Option<String>,
                    mode: Option<String>,
                    render_style: Option<Value>,
                    samples: Option<u32>,
                    compare: Option<Value>,
                    region: Option<[f64; 4]>,
                    camera: Option<kadakas_core::sheets::ViewCamera>,
                    // Set by mesh.render only: the answer says which camera was used.
                    with_camera: Option<bool>,
                }
                let P {
                    view,
                    width,
                    height,
                    projection,
                    mode,
                    render_style,
                    samples,
                    compare,
                    region,
                    camera: free_camera,
                    with_camera,
                } = params(p)?;
                let region = region_param(region)?;
                let settings =
                    self.resolve_view(view.as_deref(), projection.as_deref(), mode.as_deref())?;
                let (width, height) = self
                    .limits
                    .image_size(width.unwrap_or(800), height.unwrap_or(600));
                if let Some(spec) = compare_api::CompareSpec::parse(compare.as_ref())? {
                    if render_style.as_ref().is_some_and(|v| !v.is_null()) {
                        return Err(RpcError::invalid_params(
                            "render_style cannot be used with compare: the compare draws its own tints",
                        ));
                    }
                    let req = compare_api::PngRequest {
                        settings,
                        width,
                        height,
                        samples: samples.unwrap_or(1),
                    };
                    return self.render_png_compare(&req, &spec);
                }
                let scene = self.scene();
                let doc = self.editor.doc();
                let mut camera = Camera::standard(settings.view, settings.projection);
                // The camera frames the whole model, or the box of `region` (over the height of
                // the model).
                let framed = match (region, scene.bounds) {
                    (Some((lo, hi)), bounds) => {
                        let (zlo, zhi) = bounds.map_or((0.0, 0.0), |(l, h)| (l.z, h.z));
                        let o = scene.origin;
                        Some((
                            glam::Vec3::new((lo.x - o.x) as f32, (lo.y - o.y) as f32, zlo),
                            glam::Vec3::new((hi.x - o.x) as f32, (hi.y - o.y) as f32, zhi),
                        ))
                    }
                    (None, bounds) => bounds,
                };
                // A free camera is used as it is; the others frame the model.
                match &free_camera {
                    Some(c) => camera = mesh_api::camera_of(c, scene.origin)?,
                    None => camera.fit(framed, width as f32 / height as f32),
                }
                // A named style, else the active style of the document, else none.
                let style = match render_style.filter(|v| !v.is_null()) {
                    Some(v) => Some(present_api::style_param(doc, &v)?),
                    None => doc
                        .drawing
                        .presentation
                        .active_style
                        .and_then(|id| kadakas_core::present::render_style(doc, id))
                        .cloned(),
                };
                let slots = kadakas_core::present::SlotIndex::new(doc);
                let plain = style.is_none()
                    && slots.is_empty()
                    && matches!(settings.mode, RenderMode::Shaded | RenderMode::Wireframe);
                let mut used_style = RenderStyle::default();
                let (img, used_mode) = if plain {
                    let img = kadakas_render::render(
                        &scene,
                        &camera,
                        &RenderOptions {
                            width,
                            height,
                            mode: settings.mode,
                            ..Default::default()
                        },
                    );
                    (img, settings.mode)
                } else {
                    let mut st = style.clone().unwrap_or_default();
                    // A mode in the request, or a mode chosen with view.set, wins over a style without
                    // a name in the request. A named style keeps its own mode unless the request gives one.
                    if mode.is_some() || style.is_none() {
                        st.mode = settings.mode;
                    }
                    let img = kadakas_render::styled::render_styled(
                        doc,
                        &scene,
                        &camera,
                        &st,
                        width,
                        height,
                        samples.unwrap_or(1),
                    );
                    let mode = st.mode;
                    used_style = st;
                    (img, mode)
                };
                use base64::Engine;
                let mut out = json!({
                    "png_base64": base64::engine::general_purpose::STANDARD.encode(img.to_png()),
                    "width": width,
                    "height": height,
                    "view": settings.view.id(),
                    "projection": camera.projection.id(),
                    "mode": used_mode.id(),
                });
                if free_camera.is_some() || with_camera == Some(true) {
                    out["camera"] = mesh_api::camera_json(&camera, scene.origin);
                }
                // The settings of the style are only given when materials or a style were used.
                if !plain {
                    out["render_style"] = present_api::style_summary(&used_style);
                }
                let n = kadakas_render::look::patterned_untextured(doc);
                if n > 0 && !plain {
                    out["notes"] = json!([kadakas_render::look::pattern_not_in_3d_note(n)]);
                }
                out
            }
            m if agent_view_api::APP_ONLY.contains(&m) => {
                return agent_view_api::handle(m).expect("an app method");
            }
            "view.screenshot" => {
                return Err(RpcError::app(
                    "view.screenshot needs the live window of the app. Start the app with --agent-port; the headless server has no window. Use render.png for a picture of the 3D model, or render.svg for the plan",
                ));
            }
            "view.list" => json!({
                "views": StandardView::ALL.iter().map(|v| json!({"id": v.id(), "label": v.label()})).collect::<Vec<_>>(),
                "saved_views": doc.drawing.saved_views.iter().map(|v| json!({"name": v.name})).collect::<Vec<_>>(),
                "projections": [Projection::Orthographic.id(), Projection::Perspective.id()],
                "modes": RenderMode::ALL.map(|m| m.id()),
                "current": view_json(self.view),
            }),
            "view.set" => {
                #[derive(Deserialize)]
                struct P {
                    view: Option<String>,
                    projection: Option<String>,
                    mode: Option<String>,
                }
                let P {
                    view,
                    projection,
                    mode,
                } = params(p)?;
                if view.is_none() && projection.is_none() && mode.is_none() {
                    return Err(RpcError::invalid_params("give view, projection or mode"));
                }
                // The name of a saved view: its camera, not its visibility (that edits the document).
                if let Some(name) = view.as_deref()
                    && StandardView::from_name(name).is_none()
                    && let Some(saved) = doc.drawing.saved_view(name).cloned()
                {
                    // Visibility is an edit of the document: only say when it would differ.
                    let layers_not_applied = saved
                        .layers
                        .iter()
                        .any(|(id, on)| doc.layer(*id).is_some_and(|l| l.visible != *on))
                        || saved
                            .classes
                            .iter()
                            .any(|(id, on)| doc.class(*id).is_some_and(|c| c.visible != *on))
                        || saved.active_layer.is_some_and(|l| l != doc.active_layer)
                        || saved.active_class.is_some_and(|c| c != doc.active_class);
                    let camera_only = SavedView {
                        layers: Default::default(),
                        classes: Default::default(),
                        active_layer: None,
                        active_class: None,
                        ..saved.clone()
                    };
                    self.apply_saved_view(&camera_only);
                    self.applied_camera_only = true;
                    if projection.is_some() || mode.is_some() {
                        self.view =
                            self.resolve_view(None, projection.as_deref(), mode.as_deref())?;
                        self.view_changed = true;
                    }
                    let mut out = view_json(self.view);
                    out["saved_view"] = json!(saved.name);
                    out["layers_not_applied"] = json!(layers_not_applied);
                    return Ok(out);
                }
                self.view =
                    self.resolve_view(view.as_deref(), projection.as_deref(), mode.as_deref())?;
                self.view_changed = true;
                view_json(self.view)
            }
            "report.plants" => plants_api::report_plants(doc),
            "report.areas" => {
                let rows: Vec<Value> = doc
                    .objects
                    .values()
                    .filter_map(|o| {
                        let name = match &o.shape {
                            Shape::LandscapeArea { name, .. } => name.clone(),
                            Shape::Hardscape { material, .. } => {
                                kadakas_core::props::hardscape_label(doc, o, material)
                            }
                            _ => return None,
                        };
                        let m2 = o.shape.area().unwrap_or(0.0) / 1e6;
                        Some(json!({"id": o.id, "kind": o.shape.kind(), "name": name, "area_m2": m2}))
                    })
                    .collect();
                json!({ "rows": rows })
            }
            #[cfg(not(target_arch = "wasm32"))]
            "import.file" => self.import_file(p)?,
            #[cfg(not(target_arch = "wasm32"))]
            "export.file" => self.export_file(p)?,
            _ => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(r) = plugin_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = wasm_plugin_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = present_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = hardscape_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = plan_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = pattern_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = massing_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = solid_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = mesh_api::handle(self, method, p.clone()) {
                    return r;
                }
                if let Some(r) = irrigation_api::handle(self, method, p.clone()) {
                    return r;
                }
                return plants_api::handle(self, method, p.clone())
                    .or_else(|| picture_api::handle(self, method, p.clone()))
                    .or_else(|| sheets_api::handle(self, method, p.clone()))
                    .or_else(|| symbols_api::handle(self, method, p.clone()))
                    .or_else(|| edit2d_api::handle(self, method, p.clone()))
                    .or_else(|| query_api::handle(self, method, p.clone()))
                    .or_else(|| annotation_api::handle(self, method, p.clone()))
                    .or_else(|| data_api::handle(self, method, p))
                    .unwrap_or_else(|| Err(RpcError::method_not_found(method)));
            }
        })
    }

    /// The current view with the given names applied. Fails on an unknown name.
    fn resolve_view(
        &self,
        view: Option<&str>,
        projection: Option<&str>,
        mode: Option<&str>,
    ) -> Result<ViewSettings, RpcError> {
        let mut v = self.view;
        if let Some(n) = view {
            v.view = StandardView::from_name(n).ok_or_else(|| {
                let all: Vec<&str> = StandardView::ALL.iter().map(|v| v.id()).collect();
                RpcError::invalid_params(format!("unknown view '{n}'. Views: {}", all.join(", ")))
            })?;
        }
        if let Some(n) = projection {
            v.projection = Projection::from_name(n).ok_or_else(|| {
                RpcError::invalid_params(format!(
                    "unknown projection '{n}'. Use ortho or perspective"
                ))
            })?;
        }
        if let Some(n) = mode {
            v.mode = RenderMode::from_name(n).ok_or_else(|| {
                RpcError::invalid_params(format!(
                    "unknown mode '{n}'. Use {}",
                    RenderMode::ALL.map(|m| m.id()).join(", ")
                ))
            })?;
        }
        Ok(v)
    }

    /// Drops selected IDs that no longer exist.
    pub fn prune_selection(&mut self) {
        let doc = self.editor.doc();
        self.selection.retain(|id| doc.object(*id).is_some());
        // Leave groups that are gone.
        if let Some(i) = self.context.iter().position(|g| !doc.is_group(*g)) {
            self.context.truncate(i);
        }
    }

    /// The group you are in, if any.
    pub fn edit_context(&self) -> Option<ObjectId> {
        self.context.last().copied()
    }

    /// The command to run for `cmd` while you are in a group: a new object becomes a member of the group
    /// (also in the edit group of a symbol, so it is part of the symbol after you exit). Other commands,
    /// and objects for another layer, are unchanged.
    /// Runs a command given as JSON. The names of layers and classes in `layer` and `class` become
    /// IDs, for each command when its turn comes: a command of a batch can name a layer or a
    /// class that an earlier command of the same batch added.
    fn execute_json(
        &mut self,
        command: serde_json::Map<String, Value>,
    ) -> Result<kadakas_core::command::Outcome, RpcError> {
        use params::refs;
        if let Some(key) = reserved_data_key(&command) {
            return Err(RpcError::invalid_params(format!(
                "the data key '{key}' is reserved: keys that start with '{}' belong to Kadakas and only its own commands write them (reading is free). For hiding use set_hidden",
                kadakas_core::visibility::RESERVED_PREFIX
            )));
        }
        if !json_command_has_names(&command) {
            let command: Command = params(Value::Object(command))?;
            let command = self.in_context(command);
            return self.editor.execute(&command).map_err(RpcError::from);
        }
        if command.get("op").and_then(Value::as_str) != Some("batch") {
            let mut command = command;
            refs::resolve_command(&refs::Names::of(self.editor.doc()), &mut command)?;
            let command: Command = params(Value::Object(command))?;
            let command = self.in_context(command);
            return self.editor.execute(&command).map_err(RpcError::from);
        }
        // A batch with names: one command at a time, each against the document as the commands
        // before it left it.
        let mut failure: Option<RpcError> = None;
        let result = self.editor.execute_stepwise(|step| {
            run_json_command(step, command, &mut BatchNames::default()).map_err(|e| {
                let message = e.message.clone();
                failure = Some(e);
                kadakas_core::command::CommandError::Invalid(message)
            })
        });
        match (result, failure) {
            (Ok(out), _) => Ok(out),
            (Err(_), Some(e)) => Err(e),
            (Err(e), None) => Err(RpcError::from(e)),
        }
    }

    pub fn in_context(&self, cmd: Command) -> Command {
        let doc = self.editor.doc();
        match (
            &cmd,
            self.edit_context()
                .and_then(|g| doc.object(g).map(|o| (g, o.layer))),
        ) {
            (
                Command::AddObject {
                    shape,
                    layer,
                    class,
                    attrs,
                },
                Some((parent, group_layer)),
            ) if layer.unwrap_or(doc.active_layer) == group_layer => Command::AddObjectIn {
                parent,
                shape: shape.clone(),
                class: *class,
                attrs: attrs.clone(),
                id: None,
            },
            _ => cmd,
        }
    }

    /// Handles one JSON-RPC 2.0 request line and returns the response line.
    /// Returns `None` for a notification (a request without an id).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        self.handle_line_as(events::LOCAL, line)
    }

    /// Like `handle_line`, for a request from the connection `conn` (see `events`).
    pub fn handle_line_as(&mut self, conn: u64, line: &str) -> Option<String> {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(error_response(
                    Value::Null,
                    -32700,
                    &format!("parse error: {e}"),
                ));
            }
        };
        let id = req.get("id").cloned();
        let Some(method) = req.get("method").and_then(Value::as_str) else {
            return Some(error_response(
                id.unwrap_or(Value::Null),
                -32600,
                "missing method",
            ));
        };
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        // A panic in a method becomes an internal error with a hint to file a bug, so a long
        // session survives it. The document may be in a state the method did not expect.
        let result = self.guarded(method, |s| {
            s.handle_as(conn, kadakas_core::change::Origin::Agent, method, params)
        });
        let id = id?;
        Some(match result {
            Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}).to_string(),
            Err(e) => {
                let hint = feedback_api::hint(method, e.kind, &e.message);
                errors::error_response_with(id, &e, hint, self.tainted)
            }
        })
    }
}

fn view_json(v: ViewSettings) -> Value {
    json!({"view": v.view.id(), "projection": v.projection.id(), "mode": v.mode.id()})
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    let kind = errors::CODES
        .iter()
        .find(|c| c.0 == code)
        .map_or("error", |c| c.1);
    errors::error_response(id, code, kind, message)
}

fn command_examples() -> Value {
    json!([
        {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5000, 3000]}},
        {"op": "add_object", "shape": {"kind": "plant", "at": [1000, 1000], "species": "Acer campestre", "spread": 4000, "height": 8000}},
        {"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0,0],[3000,0],[3000,1200],[0,1200]], "material": "Granite setts"}},
        {"op": "add_object", "shape": {"kind": "polyline", "closed": true, "points": [[0,0], {"p": [3000,0], "kind": "arc", "radius": 400}, {"p": [3000,2000], "kind": "arc", "radius": 400}, [0,2000]]}},
        {"op": "add_resource", "name": "Soil", "body": {"type": "record_format", "fields": [{"name": "Depth", "type": "dimension", "default": 300}, {"name": "Grade", "type": "choice", "choices": ["A", "B"]}]}},
        {"op": "attach_record", "ids": ["00000000-0000-0000-0000-000000000002"], "record": "00000000-0000-0000-0000-000000000001", "values": {"Depth": 250}},
        {"op": "set_record_fields", "ids": ["00000000-0000-0000-0000-000000000002"], "record": "00000000-0000-0000-0000-000000000001", "values": {"Grade": "B"}},
        {"op": "detach_record", "ids": ["00000000-0000-0000-0000-000000000002"], "record": "00000000-0000-0000-0000-000000000001"},
        {"op": "add_worksheet_table", "worksheet": "00000000-0000-0000-0000-000000000003", "at": [10, 280], "size": [120, 40], "text_height": 3.5},
        {"op": "set_worksheet_table", "ids": ["00000000-0000-0000-0000-000000000002"], "size": [150, 60]},
        {"op": "set_vertex", "id": "00000000-0000-0000-0000-000000000002", "index": 1, "kind": {"kind": "arc", "radius": 250}},
        {"op": "move_objects", "ids": ["00000000-0000-0000-0000-000000000002"], "delta": [100, 0]},
        {"op": "add_layer", "name": "Planting", "elevation": 0},
        {"op": "arrange", "ids": ["00000000-0000-0000-0000-000000000002"], "how": "front"},
        {"op": "move_layer", "id": "00000000-0000-0000-0000-000000000001", "index": 0},
        {"op": "batch", "commands": [{"op": "delete_objects", "ids": ["00000000-0000-0000-0000-000000000002"]}]},
        {"op": "add_object", "shape": {"kind": "locus3d", "p": [1000, 2000, 350]}},
        {"op": "add_object", "shape": {"kind": "polyline3d", "closed": false, "points": [[0, 0, 100], [2000, 0, 150], [4000, 500, 220]]}},
        {"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "pad", "elevation": 1200}, "boundary": [[2000,2000],[6000,2000],[6000,5000],[2000,5000]]}},
        {"op": "add_sheet", "name": "Planting plan", "paper": "a1", "number": "L-101"},
        {"op": "set_project", "name": "Riverside garden", "drawn_by": "NM"},
        {"op": "add_object", "layer": "00000000-0000-0000-0000-000000000003", "shape": {"kind": "viewport", "min": [15, 55], "max": [405, 282], "center": [10000, 5000], "scale": 100, "layers": ["00000000-0000-0000-0000-000000000001"]}},
        {"op": "add_object", "layer": "00000000-0000-0000-0000-000000000003", "shape": {"kind": "title_block", "grid_columns": 6, "grid_rows": 4}},
        {"op": "restore_view", "name": "Planting only"},
        {"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "grade_limit"}, "boundary": [[1000,1000],[7000,1000],[7000,6000],[1000,6000]]}},
        {"op": "set_layer_site", "id": "00000000-0000-0000-0000-000000000001", "site": "00000000-0000-0000-0000-000000000002"}
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::uid::AsUid;

    fn call(s: &mut Session, line: &str) -> Value {
        serde_json::from_str(&s.handle_line(line).unwrap()).unwrap()
    }

    #[test]
    fn rpc_roundtrip() {
        let mut s = Session::with_sequential_ids();
        let r = call(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"execute","params":{"command":{"op":"add_object","shape":{"kind":"plant","at":[0,0],"species":"Oak","spread":3000,"height":9000}}}}"#,
        );
        let id = r["result"]["created"][0].as_uid().unwrap();
        let r = call(
            &mut s,
            r#"{"jsonrpc":"2.0","id":2,"method":"report.plants"}"#,
        );
        assert_eq!(r["result"]["rows"][0]["count"], 1);
        call(
            &mut s,
            &format!(r#"{{"id":3,"method":"selection.set","params":{{"ids":[{id}]}}}}"#),
        );
        call(&mut s, r#"{"id":4,"method":"undo"}"#);
        let r = call(&mut s, r#"{"id":5,"method":"selection.get"}"#);
        assert_eq!(r["result"]["ids"], json!([]));
    }

    #[test]
    fn rpc_errors() {
        let mut s = Session::with_sequential_ids();
        assert_eq!(call(&mut s, "{nope")["error"]["code"], -32700);
        assert_eq!(call(&mut s, r#"{"id":1}"#)["error"]["code"], -32600);
        assert_eq!(
            call(&mut s, r#"{"id":1,"method":"x"}"#)["error"]["code"],
            -32601
        );
        assert_eq!(
            call(&mut s, r#"{"id":1,"method":"objects.get","params":{}}"#)["error"]["code"],
            -32602
        );
        assert_eq!(
            call(
                &mut s,
                r#"{"id":1,"method":"objects.get","params":{"id":"00000000-0000-0000-0000-000000000009"}}"#
            )["error"]["code"],
            2
        );
        assert!(s.handle_line(r#"{"method":"undo"}"#).is_none());
    }

    fn ok(s: &mut Session, method: &str, params: Value) -> Value {
        let r = call(
            s,
            &json!({"id": 1, "method": method, "params": params}).to_string(),
        );
        assert!(r.get("error").is_none(), "{r}");
        r["result"].clone()
    }

    fn fail(s: &mut Session, method: &str, params: Value) -> (i64, String) {
        let r = call(
            s,
            &json!({"id": 1, "method": method, "params": params}).to_string(),
        );
        (
            r["error"]["code"].as_i64().unwrap(),
            r["error"]["message"].as_str().unwrap().to_string(),
        )
    }

    /// Runs one `execute` call and returns the created IDs.
    fn exec(s: &mut Session, command: Value) -> Vec<kadakas_core::uid::Uid> {
        let r = ok(s, "execute", json!({"command": command}));
        r["created"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_uid().unwrap())
            .collect()
    }

    fn xy(v: &Value) -> (f64, f64) {
        (v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
    }

    fn circle_cmd(x: f64) -> Value {
        json!({"op": "add_object", "shape": {"kind": "circle", "center": [x, 0], "radius": 1}})
    }

    #[test]
    fn render_png_frames_a_region_and_the_docs_name_region_and_show_check() {
        let mut s = Session::with_sequential_ids();
        for x in [0.0, 50_000.0] {
            ok(
                &mut s,
                "hardscape.add",
                json!({"boundary": [[x, 0], [x + 4000.0, 0], [x + 4000.0, 4000.0], [x, 4000.0]],
                       "material": "Granite", "thickness": 60}),
            );
        }
        let whole = ok(
            &mut s,
            "render.png",
            json!({"view": "top", "projection": "ortho", "width": 96, "height": 96}),
        );
        let part = ok(
            &mut s,
            "render.png",
            json!({"view": "top", "projection": "ortho", "width": 96, "height": 96,
                   "region": [-500, -500, 4500, 4500]}),
        );
        assert_ne!(whole["png_base64"], part["png_base64"]);
        let e = s
            .handle("render.png", json!({"region": [0, 0, 0, 5]}))
            .unwrap_err();
        assert!(e.message.contains("region"), "{}", e.message);
        for m in ["render.svg", "render.png"] {
            let doc = METHODS.iter().find(|(n, _)| *n == m).unwrap().1;
            assert!(doc.contains("region?"), "{m}");
        }
        let doc = METHODS.iter().find(|(n, _)| *n == "render.svg").unwrap().1;
        assert!(doc.contains("show_check?"));
    }

    #[test]
    fn describe_lists_every_method_and_every_method_is_routed() {
        let mut s = Session::with_sequential_ids();
        let d = ok(&mut s, "describe", json!(null));
        let names: Vec<&str> = d["methods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect();
        let expected: Vec<&str> = METHODS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, expected);
        assert!(d["units"].as_str().unwrap().contains("millimeters"));
        assert!(
            d["files"]["forms"]["container"]
                .as_str()
                .unwrap()
                .contains("ZIP")
        );
        assert!(d["files"]["file.save"].as_str().unwrap().contains("format"));
        assert!(d["command_examples"].as_array().unwrap().len() >= 10);

        // Every advertised method is handled: none of them is "unknown method".
        for (name, _) in METHODS {
            let r = call(
                &mut s,
                &json!({"id": 1, "method": name, "params": {}}).to_string(),
            );
            let code = r["error"]["code"].as_i64().unwrap_or(0);
            assert_ne!(code, -32601, "{name} is advertised but not routed");
        }
    }

    #[test]
    fn execute_reports_app_errors_and_bad_params() {
        let mut s = Session::with_sequential_ids();
        let (code, msg) = fail(
            &mut s,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 0}}}),
        );
        assert_eq!(
            (code, msg.as_str()),
            (3, "invalid value: radius must be positive")
        );

        let (code, msg) = fail(&mut s, "execute", json!({"command": {"op": "no_such_op"}}));
        assert_eq!(code, -32602);
        assert!(msg.contains("no_such_op"), "{msg}");

        let (code, _) = fail(&mut s, "execute", json!({}));
        assert_eq!(code, -32602);

        let (code, msg) = fail(
            &mut s,
            "execute",
            json!({"command": {"op": "delete_objects", "ids": ["00000000-0000-0000-0000-00000000002a"]}}),
        );
        assert_eq!(
            (code, msg.as_str()),
            (
                2,
                format!(
                    "object {} does not exist",
                    kadakas_core::uid::Uid::from_num(42)
                )
                .as_str()
            )
        );
        assert!(ok(&mut s, "document.summary", json!({}))["object_count"] == 0);
    }

    #[test]
    fn undo_and_redo_report_whether_anything_changed() {
        let mut s = Session::with_sequential_ids();
        assert_eq!(ok(&mut s, "undo", json!(null)), json!({"done": false}));
        assert_eq!(ok(&mut s, "redo", json!(null)), json!({"done": false}));

        exec(&mut s, circle_cmd(0.0));
        assert_eq!(ok(&mut s, "undo", json!(null)), json!({"done": true}));
        assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 0);
        assert_eq!(ok(&mut s, "redo", json!(null)), json!({"done": true}));
        assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 1);
        assert_eq!(ok(&mut s, "redo", json!(null)), json!({"done": false}));
    }

    #[test]
    fn document_summary_counts_kinds_and_bounds() {
        let mut s = Session::with_sequential_ids();
        let empty = ok(&mut s, "document.summary", json!({}));
        assert!(empty["bounds"].is_null());
        assert_eq!(empty["can_undo"], false);
        assert_eq!(empty["layers"].as_array().unwrap().len(), 1);
        assert_eq!(empty["active_class"], kadakas_core::uid::Uid::from_num(0));

        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5, 3]}}),
        );
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "plant", "at": [10, 10], "species": "Oak", "spread": 4, "height": 8}}),
        );
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 2, "height": 8}}),
        );
        let sum = ok(&mut s, "document.summary", json!({}));
        assert_eq!(sum["object_count"], 3);
        assert_eq!(sum["objects_by_kind"], json!({"plant": 2, "rect": 1}));
        assert_eq!(sum["can_undo"], true);
        assert_eq!(sum["can_redo"], false);
        assert_eq!(xy(&sum["bounds"]["min"]), (-1.0, -1.0));
        assert_eq!(xy(&sum["bounds"]["max"]), (12.0, 12.0));
    }

    #[test]
    fn document_get_returns_the_whole_document() {
        let mut s = Session::with_sequential_ids();
        exec(&mut s, circle_cmd(0.0));
        let d = ok(&mut s, "document.get", json!(null));
        assert_eq!(d["format_version"], FORMAT_VERSION);
        assert_eq!(d["objects"].as_array().unwrap().len(), 1);
        assert_eq!(d["objects"][0]["shape"]["kind"], "circle");
        assert_eq!(d["layers"][0]["name"], "Layer 1");
    }

    #[test]
    fn the_summary_lists_component_instances_that_cannot_be_drawn() {
        let mut s = Session::with_sequential_ids();
        let mut doc = Document::default();
        let id = kadakas_core::model::ObjectId(doc.alloc_id());
        let (layer, class) = (doc.active_layer, doc.active_class);
        doc.objects.insert(
            id,
            kadakas_core::model::Object::new(
                id,
                layer,
                class,
                Shape::Component {
                    symbol: kadakas_core::resource::ResourceId::from_num(31337),
                    transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
                },
            ),
        );
        ok(
            &mut s,
            "document.load",
            json!({"document": serde_json::to_value(&doc).unwrap()}),
        );
        let sum = ok(&mut s, "document.summary", json!({}));
        let rows = sum["broken_components"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{sum}");
        assert!(
            rows[0]["problem"]
                .as_str()
                .unwrap()
                .contains("does not exist")
        );
    }

    #[test]
    fn document_load_replaces_document_and_clears_history() {
        let mut s = Session::with_sequential_ids();
        let id = exec(&mut s, circle_cmd(0.0))[0];
        ok(&mut s, "selection.set", json!({"ids": [id]}));

        let blank = serde_json::to_value(Document::default()).unwrap();
        assert_eq!(
            ok(&mut s, "document.load", json!({"document": blank})),
            json!({"ok": true})
        );
        let sum = ok(&mut s, "document.summary", json!({}));
        assert_eq!(sum["object_count"], 0);
        assert_eq!(sum["can_undo"], false);
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": []}));
    }

    #[test]
    fn document_load_rejects_newer_format_and_bad_shape() {
        let mut s = Session::with_sequential_ids();
        let mut newer = serde_json::to_value(Document::default()).unwrap();
        newer["format_version"] = json!(FORMAT_VERSION + 1);
        let (code, msg) = fail(&mut s, "document.load", json!({"document": newer}));
        assert_eq!(code, 1);
        assert_eq!(
            msg,
            format!(
                "file was made by a newer Kadakas (format {})",
                FORMAT_VERSION + 1
            )
        );

        let (code, _) = fail(
            &mut s,
            "document.load",
            json!({"document": {"format_version": 1}}),
        );
        assert_eq!(code, 1, "an old format is refused");
        let (code, _) = fail(&mut s, "document.load", json!({}));
        assert_eq!(code, -32602);
    }

    #[test]
    fn objects_list_filters_by_layer_class_and_kind() {
        let mut s = Session::with_sequential_ids();
        let layer = exec(&mut s, json!({"op": "add_layer", "name": "Planting"}))[0];
        let class = exec(&mut s, json!({"op": "add_class", "name": "Trees"}))[0];
        let c1 = exec(&mut s, circle_cmd(0.0))[0];
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1, 1]}}),
        );
        let c3 = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 1}, "layer": layer, "class": class}),
        )[0];
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 1, "height": 1}, "layer": layer}),
        );

        let ids = |v: Value| -> Vec<kadakas_core::uid::Uid> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|o| o["id"].as_uid().unwrap())
                .collect()
        };
        assert_eq!(ids(ok(&mut s, "objects.list", json!({}))).len(), 4);
        assert_eq!(
            ids(ok(&mut s, "objects.list", json!({"layer": layer}))).len(),
            2
        );
        assert_eq!(
            ids(ok(&mut s, "objects.list", json!({"class": class}))),
            vec![c3]
        );
        assert_eq!(
            ids(ok(&mut s, "objects.list", json!({"kind": "circle"}))),
            vec![c1, c3]
        );
        assert_eq!(
            ids(ok(
                &mut s,
                "objects.list",
                json!({"layer": layer, "kind": "circle"})
            )),
            vec![c3]
        );
        assert!(ids(ok(&mut s, "objects.list", json!({"kind": "text"}))).is_empty());
        assert_eq!(
            ids(ok(&mut s, "objects.list", json!({"layer": null}))).len(),
            4
        );
    }

    #[test]
    fn objects_get_returns_object_or_error() {
        let mut s = Session::with_sequential_ids();
        let id = exec(&mut s, circle_cmd(3.0))[0];
        let o = ok(&mut s, "objects.get", json!({"id": id}));
        assert_eq!(o["id"], id);
        assert_eq!(o["layer"], kadakas_core::uid::Uid::from_num(1));
        assert_eq!(o["shape"]["radius"], 1.0);

        assert_eq!(
            fail(
                &mut s,
                "objects.get",
                json!({"id": kadakas_core::uid::Uid::from_num(999)})
            ),
            (
                2,
                format!(
                    "object {} does not exist",
                    kadakas_core::uid::Uid::from_num(999)
                )
            )
        );
        assert_eq!(fail(&mut s, "objects.get", json!({})).0, -32602);
    }

    #[test]
    fn selection_is_validated_and_pruned_when_objects_go_away() {
        let mut s = Session::with_sequential_ids();
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": []}));
        let a = exec(&mut s, circle_cmd(0.0))[0];
        let b = exec(&mut s, circle_cmd(5.0))[0];
        assert_eq!(
            ok(&mut s, "selection.set", json!({"ids": [a, b]})),
            json!({"ids": [a, b]})
        );

        // An unknown ID is rejected and the selection stays as it was.
        assert_eq!(
            fail(
                &mut s,
                "selection.set",
                json!({"ids": [a.to_string(), kadakas_core::uid::Uid::from_num(999).to_string()]})
            ),
            (
                2,
                format!(
                    "object {} does not exist",
                    kadakas_core::uid::Uid::from_num(999)
                )
            )
        );
        assert_eq!(
            ok(&mut s, "selection.get", json!({})),
            json!({"ids": [a, b]})
        );

        // Deleting a selected object drops it from the selection.
        exec(&mut s, json!({"op": "delete_objects", "ids": [a]}));
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": [b]}));

        // Undo that brings the object back, but the old selection is not restored.
        ok(&mut s, "undo", json!(null));
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": [b]}));
    }

    #[test]
    fn render_svg_width_is_clamped() {
        let mut s = Session::with_sequential_ids();
        exec(&mut s, circle_cmd(0.0));
        let width_of = |svg: &str| -> String {
            let start = svg.find(r#"width=""#).unwrap() + 7;
            svg[start..].split('"').next().unwrap().to_string()
        };
        let render =
            |s: &mut Session, p: Value| ok(s, "render.svg", p)["svg"].as_str().unwrap().to_string();

        assert_eq!(width_of(&render(&mut s, json!({}))), "800");
        assert_eq!(width_of(&render(&mut s, json!({"width": 300}))), "300");
        assert_eq!(width_of(&render(&mut s, json!({"width": 0}))), "16");
        assert_eq!(width_of(&render(&mut s, json!({"width": 1}))), "16");
        assert_eq!(width_of(&render(&mut s, json!({"width": 100000}))), "8192");
        assert!(render(&mut s, json!({})).contains(r#"data-id="#));
        assert_eq!(
            fail(&mut s, "render.svg", json!({"width": "wide"})).0,
            -32602
        );
    }

    #[test]
    fn report_plants_counts_by_species_sorted() {
        let mut s = Session::with_sequential_ids();
        assert_eq!(ok(&mut s, "report.plants", json!({})), json!({"rows": []}));
        for species in ["Oak", "Acer", "Oak"] {
            exec(
                &mut s,
                json!({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": species, "spread": 1, "height": 1}}),
            );
        }
        exec(&mut s, circle_cmd(0.0));
        assert_eq!(
            ok(&mut s, "report.plants", json!({})),
            json!({"rows": [{"species": "Acer", "count": 1}, {"species": "Oak", "count": 2}]})
        );
    }

    #[test]
    fn report_areas_gives_square_meters() {
        let mut s = Session::with_sequential_ids();
        let meadow = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "landscape_area", "boundary": [[0, 0], [2000, 0], [2000, 3000], [0, 3000]], "name": "Meadow"}}),
        )[0];
        let setts = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [3000, 0], [0, 2000]], "material": "Setts"}}),
        )[0];
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [9000, 9000]}}),
        );

        let rows = ok(&mut s, "report.areas", json!({}))["rows"].clone();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 2, "rects are not areas for this report");
        assert_eq!(rows[0]["id"], meadow);
        assert_eq!(rows[0]["kind"], "landscape_area");
        assert_eq!(rows[0]["name"], "Meadow");
        assert!((rows[0]["area_m2"].as_f64().unwrap() - 6.0).abs() < 1e-9);
        assert_eq!(rows[1]["id"], setts);
        assert_eq!(rows[1]["kind"], "hardscape");
        assert_eq!(rows[1]["name"], "Setts");
        assert!((rows[1]["area_m2"].as_f64().unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn notifications_and_bad_request_ids() {
        let mut s = Session::with_sequential_ids();
        // A request without an id is a notification and gets no reply, even on error.
        assert!(s.handle_line(r#"{"method":"nope"}"#).is_none());
        // A missing method is reported with a null id when there is no id.
        let r: Value = serde_json::from_str(&s.handle_line(r#"{"params":{}}"#).unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32600);
        assert!(r["id"].is_null());
        // A parse error has a null id.
        let r: Value = serde_json::from_str(&s.handle_line("[1,").unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32700);
        assert!(
            r["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("parse error")
        );
    }

    #[test]
    fn load_with_stale_next_id_does_not_reuse_ids() {
        let mut doc = serde_json::to_value(Document::default()).unwrap();
        doc["objects"] = json!([{"id": "00000000-0000-0000-0000-000000000002", "layer": "00000000-0000-0000-0000-000000000001", "class": "00000000-0000-0000-0000-000000000000", "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        // next_id stays at 2, the ID the object above already uses.
        let mut s = Session::with_sequential_ids();
        ok(&mut s, "document.load", json!({"document": doc}));
        let new_id = exec(&mut s, circle_cmd(5.0))[0];
        assert_ne!(new_id, kadakas_core::uid::Uid::from_num(2));
    }

    #[test]
    fn command_examples_parse() {
        for ex in command_examples().as_array().unwrap() {
            serde_json::from_value::<Command>(ex.clone()).unwrap();
        }
    }

    #[test]
    fn document_load_signals_that_the_document_was_replaced() {
        let mut s = Session::with_sequential_ids();
        assert!(!s.take_replaced());
        exec(&mut s, circle_cmd(0.0));
        assert!(
            !s.take_replaced(),
            "normal edits do not replace the document"
        );
        ok(
            &mut s,
            "document.load",
            json!({"document": serde_json::to_value(Document::default()).unwrap()}),
        );
        assert!(s.take_replaced());
        assert!(!s.take_replaced(), "the flag is cleared when read");

        // A refused load keeps the old document and does not set the flag.
        exec(&mut s, circle_cmd(0.0));
        let mut newer = serde_json::to_value(Document::default()).unwrap();
        newer["format_version"] = json!(FORMAT_VERSION + 1);
        fail(&mut s, "document.load", json!({"document": newer}));
        assert!(!s.take_replaced());
        assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 1);
    }

    #[test]
    fn document_load_rejects_invalid_documents_and_repairs_next_id() {
        let mut s = Session::with_sequential_ids();
        let mut bad = serde_json::to_value(Document::default()).unwrap();
        bad["active_layer"] = json!(kadakas_core::uid::Uid::from_num(42));
        assert_eq!(fail(&mut s, "document.load", json!({"document": bad})).0, 1);

        let mut doc = serde_json::to_value(Document::default()).unwrap();
        doc.as_object_mut().unwrap().remove("next_id");
        doc["objects"] = json!([{"id": "00000000-0000-0000-0000-000000000002", "layer": "00000000-0000-0000-0000-000000000001", "class": "00000000-0000-0000-0000-000000000000", "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        ok(&mut s, "document.load", json!({"document": doc}));
        assert_ne!(
            exec(&mut s, circle_cmd(1.0))[0],
            kadakas_core::uid::Uid::from_num(2)
        );
    }

    #[test]
    fn replace_document_and_prune_selection_are_public() {
        let mut s = Session::with_sequential_ids();
        let id = exec(&mut s, circle_cmd(0.0))[0];
        s.selection = vec![ObjectId(id), ObjectId::from_num(999)];
        s.prune_selection();
        assert_eq!(s.selection, vec![ObjectId(id)]);
        s.replace_document(Document::default());
        assert!(s.selection.is_empty() && s.take_replaced());
        assert!(
            Session::with_document(Document::default())
                .editor
                .doc()
                .objects
                .is_empty()
        );
    }

    #[test]
    fn execute_set_data_over_rpc() {
        let mut s = Session::with_sequential_ids();
        let id = exec(&mut s, circle_cmd(0.0))[0];
        exec(
            &mut s,
            json!({"op": "set_data", "id": id, "set": {"Pset": {"a": 1}}}),
        );
        assert_eq!(
            ok(&mut s, "objects.get", json!({"id": id}))["data"],
            json!({"Pset": {"a": 1}})
        );
        exec(
            &mut s,
            json!({"op": "set_data", "id": id, "remove": ["Pset"]}),
        );
        assert!(
            ok(&mut s, "objects.get", json!({"id": id}))
                .get("data")
                .is_none()
        );
    }

    fn decode_png(result: &Value) -> (u32, u32, Vec<u8>) {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result["png_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        (info.width, info.height, buf)
    }

    #[test]
    fn render_png_draws_the_model_and_clamps_the_size() {
        let mut s = Session::with_sequential_ids();
        let empty = ok(&mut s, "render.png", json!({"width": 64, "height": 48}));
        let (w, h, px) = decode_png(&empty);
        assert_eq!((w, h), (64, 48));
        assert!(
            px.chunks(4).all(|p| p == kadakas_render::BACKGROUND),
            "empty drawing is all background"
        );

        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "material": "Setts", "thickness": 100}}),
        );
        let r = ok(
            &mut s,
            "render.png",
            json!({"view": "Right Isometric", "width": 100, "height": 80, "projection": "perspective", "mode": "wireframe"}),
        );
        assert_eq!(
            (
                r["view"].as_str(),
                r["projection"].as_str(),
                r["mode"].as_str()
            ),
            (
                Some("right_isometric"),
                Some("perspective"),
                Some("wireframe")
            )
        );
        let (w, h, px) = decode_png(&r);
        assert_eq!((w, h), (100, 80));
        assert!(px.chunks(4).any(|p| p != kadakas_render::BACKGROUND));

        let r = ok(&mut s, "render.png", json!({"width": 1, "height": 100000}));
        assert_eq!(
            (r["width"].as_u64(), r["height"].as_u64()),
            (Some(16), Some(2048))
        );
        // Too many pixels: both sides shrink, keeping the shape.
        let r = ok(
            &mut s,
            "render.png",
            json!({"width": 99999, "height": 99999}),
        );
        assert_eq!(
            (r["width"].as_u64(), r["height"].as_u64()),
            (Some(2000), Some(2000))
        );
        assert_eq!(
            fail(&mut s, "render.png", json!({"view": "sideways"})).0,
            -32602
        );
        assert!(
            fail(&mut s, "render.png", json!({"view": "sideways"}))
                .1
                .contains("right_isometric")
        );
        assert_eq!(
            fail(&mut s, "render.png", json!({"projection": "fisheye"})).0,
            -32602
        );
        assert_eq!(
            fail(&mut s, "render.png", json!({"mode": "xray"})).0,
            -32602
        );
        assert_eq!(
            fail(&mut s, "render.png", json!({"width": "wide"})).0,
            -32602
        );
    }

    #[test]
    fn view_list_and_view_set() {
        let mut s = Session::with_sequential_ids();
        let l = ok(&mut s, "view.list", json!({}));
        let ids: Vec<&str> = l["views"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 11);
        assert!(
            ids.contains(&"plan") && ids.contains(&"top") && ids.contains(&"left_rear_isometric")
        );
        assert_eq!(
            l["current"],
            json!({"view": "plan", "projection": "ortho", "mode": "shaded"})
        );
        assert!(!s.take_view_changed());

        let r = ok(&mut s, "view.set", json!({"view": "front"}));
        assert_eq!(
            r,
            json!({"view": "front", "projection": "ortho", "mode": "shaded"})
        );
        assert!(s.take_view_changed());
        assert!(!s.take_view_changed());
        // Fields that are not given stay as they were.
        ok(
            &mut s,
            "view.set",
            json!({"projection": "perspective", "mode": "wireframe"}),
        );
        assert_eq!(
            ok(&mut s, "view.list", json!({}))["current"],
            json!({"view": "front", "projection": "perspective", "mode": "wireframe"})
        );
        // render.png without a view uses the current one.
        assert_eq!(
            ok(&mut s, "render.png", json!({"width": 16, "height": 16}))["view"],
            "front"
        );
        // Nothing given, or a bad name: refused, nothing changes.
        s.take_view_changed();
        assert_eq!(fail(&mut s, "view.set", json!({})).0, -32602);
        assert_eq!(fail(&mut s, "view.set", json!({"view": "nope"})).0, -32602);
        assert!(!s.take_view_changed());
        assert_eq!(s.view.view, StandardView::Front);
        // A new document keeps the view.
        s.replace_document(Document::default());
        assert_eq!(s.view.view, StandardView::Front);
    }

    #[test]
    fn render_png_reuses_the_mesh_until_the_document_changes() {
        let mut s = Session::with_sequential_ids();
        ok(&mut s, "render.png", json!({"width": 16, "height": 16}));
        ok(
            &mut s,
            "render.png",
            json!({"width": 16, "height": 16, "view": "front"}),
        );
        assert_eq!(s.mesh_cache.builds, 1);
        exec(&mut s, circle_cmd(0.0));
        ok(&mut s, "render.png", json!({"width": 16, "height": 16}));
        assert_eq!(s.mesh_cache.builds, 2);
    }
}
