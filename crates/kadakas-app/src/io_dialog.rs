//! File > Import and File > Export: pick a file, set the options, run the API method.
//!
//! The dialog builds the same JSON as an agent would send to `import.file` and
//! `export.file`, so the app and the API use one code path.

use kadakas_l10n::tr;
use std::path::{Path, PathBuf};

use kadakas_api::Session;
use kadakas_ui_core::mesh_import::{self, MeshImportForm, Preview, SoftenPref};
use kadakas_view3d::import_widgets::{mesh_options, soften_question};
use serde_json::{Map, Value, json};

pub(crate) const UNITS: &[&str] = mesh_import::UNITS;

/// Which kind of file an import has, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// DXF, and DWG (read through a converter that becomes DXF first).
    Dxf,
    Gis,
    Ifc,
    /// OBJ, STL, glTF and GLB (MOD-12).
    Mesh,
}

fn kind_of(path: &Path) -> Kind {
    if mesh_import::kind_of(path).is_some() {
        return Kind::Mesh;
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("dxf" | "dwg") => Kind::Dxf,
        Some("ifc") => Kind::Ifc,
        _ => Kind::Gis,
    }
}

/// What the person chose for a drawing with more entities than an import keeps for updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseChoice {
    /// Keep a link to the original file (`base: "link"`).
    Link,
    /// Import without update data (`base: "none"`).
    NoUpdateData,
}

impl BaseChoice {
    /// The value of the import option `base`.
    fn option(self) -> &'static str {
        match self {
            BaseChoice::Link => "link",
            BaseChoice::NoUpdateData => "none",
        }
    }
}

/// The question about a drawing over the cap: the form that waits, and the numbers for the text.
#[derive(Clone, Debug, PartialEq)]
struct BaseQuestion {
    form: ImportForm,
    entities: u64,
    cap: u64,
    /// The size of the summaries in bytes, when the drawing is over the size limit and not over the
    /// number of entities.
    bytes: Option<u64>,
}

/// The numbers of the question, when the error of the import is the refusal of a drawing over the
/// limits of the update data: `(entities, cap, bytes)`. The reason says which limit: too many
/// entities (`cap` is a number of entities) or too large (`cap` and `bytes` are sizes).
pub(crate) fn base_choice_of(e: &kadakas_api::RpcError) -> Option<(u64, u64, Option<u64>)> {
    let d = e.details.as_ref()?;
    let entities = d["entities"].as_u64()?;
    let cap = d["cap"].as_u64()?;
    match e.reason {
        Some(kadakas_api::REASON_BASE_TOO_MANY) => Some((entities, cap, None)),
        Some(kadakas_api::REASON_BASE_TOO_LARGE) => {
            Some((entities, cap, Some(d["bytes"].as_u64()?)))
        }
        _ => None,
    }
}

/// The question for the person, from the numbers of `base_choice_of`. A link keeps the full path
/// of the file, which can contain the user name and which the people who share the document can
/// see (the text says so).
pub(crate) fn base_question_text(entities: u64, cap: u64, bytes: Option<u64>) -> String {
    let units = kadakas_units::current();
    let count = |v: u64| units.number_text(v as f64, 0);
    match bytes {
        None => tr!(
            "io-dialog-base-question",
            entities = count(entities),
            cap = count(cap)
        ),
        Some(bytes) => tr!(
            "io-dialog-base-question-large",
            entities = count(entities),
            size = count(bytes / 1_000_000),
            cap = count(cap / 1_000_000)
        ),
    }
}

impl BaseQuestion {
    fn text(&self) -> String {
        base_question_text(self.entities, self.cap, self.bytes)
    }
}

/// Options of the import dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportForm {
    pub path: PathBuf,
    pub layers_as_layers: bool,
    pub layer: String,
    /// Index in `UNITS`, or `None` for the unit of the file.
    pub unit: Option<usize>,
    pub explode_blocks: bool,
    pub only_points_with_z: bool,
    pub hatches: bool,
    /// DXF and DWG: keep the source of each object, for updates (IOP-22).
    pub track_source: bool,
    /// DXF and DWG: the answer to the question about a drawing with too many entities for an
    /// update base. `None`: not asked yet (the API default, `auto`).
    pub base: Option<BaseChoice>,
    pub source_crs: String,
    pub target_crs: String,
    pub class_field: String,
    /// Keep the coordinates of the file instead of centering the drawing.
    pub keep_origin: bool,
    /// IFC: one design layer per building storey.
    pub ifc_storeys: bool,
    /// IFC: terrain as a 3D mesh instead of a site model.
    pub ifc_terrain_mesh: bool,
    /// IFC: the answer to "Soften edges under an angle?".
    pub ifc_soften: SoftenPref,
    /// OBJ, STL, glTF and GLB: the unit, the axis and the answer to "Soften edges under an
    /// angle?". `None` for the other files.
    pub mesh: Option<MeshImportForm>,
}

impl ImportForm {
    pub fn new(path: PathBuf) -> Self {
        let path_for_mesh = path.clone();
        ImportForm {
            path,
            layers_as_layers: false,
            layer: String::new(),
            unit: None,
            explode_blocks: true,
            only_points_with_z: false,
            hatches: true,
            track_source: true,
            base: None,
            source_crs: String::new(),
            target_crs: String::new(),
            class_field: String::new(),
            keep_origin: false,
            ifc_storeys: true,
            ifc_terrain_mesh: false,
            ifc_soften: SoftenPref::default(),
            mesh: MeshImportForm::new(path_for_mesh, SoftenPref::default()),
        }
    }

    /// The form with the answer about softening that the prefs remember.
    pub fn with_soften(path: PathBuf, soften: SoftenPref) -> Self {
        let mut f = ImportForm::new(path);
        f.ifc_soften = soften.valid();
        if let Some(m) = f.mesh.as_mut() {
            m.soften = soften.valid();
        }
        f
    }

    /// The `import.file` parameters.
    pub fn params(&self) -> Value {
        let mut o = Map::new();
        let text = |o: &mut Map<String, Value>, key: &str, v: &str| {
            if !v.trim().is_empty() {
                o.insert(key.into(), json!(v.trim()));
            }
        };
        if let Some(m) = self
            .mesh
            .as_ref()
            .filter(|_| kind_of(&self.path) == Kind::Mesh)
        {
            let mut p = m.params(false);
            if !self.layer.trim().is_empty() {
                p["options"]["layer"] = json!(self.layer.trim());
            }
            return p;
        }
        if kind_of(&self.path) == Kind::Ifc {
            o.insert("layers_from_storeys".into(), json!(self.ifc_storeys));
            if self.ifc_terrain_mesh {
                o.insert("terrain".into(), json!("mesh"));
            }
            if self.ifc_soften.on {
                o.insert(
                    "soften_below_angle".into(),
                    json!(self.ifc_soften.valid().angle),
                );
            }
            return json!({"path": self.path, "options": o});
        }
        text(&mut o, "layer", &self.layer);
        if self.keep_origin {
            o.insert("origin".into(), json!("file"));
        }
        match kind_of(&self.path) {
            Kind::Dxf => {
                if self.layers_as_layers {
                    o.insert("layer_mode".into(), json!("layers"));
                }
                if let Some(u) = self.unit {
                    o.insert("unit".into(), json!(UNITS[u.min(UNITS.len() - 1)]));
                }
                o.insert("explode_blocks".into(), json!(self.explode_blocks));
                o.insert("hatches".into(), json!(self.hatches));
                // The default is on, so the option is only sent to turn it off.
                if !self.track_source {
                    o.insert("track_source".into(), json!(false));
                }
                if self.only_points_with_z {
                    o.insert("points".into(), json!("with_z"));
                }
                if let Some(base) = self.base {
                    o.insert("base".into(), json!(base.option()));
                }
            }
            Kind::Ifc | Kind::Mesh => {}
            Kind::Gis => {
                text(&mut o, "source_crs", &self.source_crs);
                text(&mut o, "target_crs", &self.target_crs);
                text(&mut o, "class_field", &self.class_field);
            }
        }
        json!({"path": self.path, "options": o})
    }
}

/// Options of the export dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportForm {
    pub path: PathBuf,
    pub layers_as_layers: bool,
    pub unit: usize,
    pub visible_only: bool,
    /// DXF: the unit pattern of hardscapes. 0 nothing, 1 gap lines, 2 units.
    pub patterns: usize,
    /// DXF: the release of the file, an index in `VERSIONS` (0 is 2004, the default).
    pub version: usize,
    /// IFC: one presentation layer per class.
    pub ifc_classes_as_layers: bool,
    /// IFC: write 2D objects as annotations.
    pub ifc_include_2d: bool,
    /// IFC: one building storey per design layer.
    pub ifc_storeys: bool,
    /// STL: ASCII text instead of binary.
    pub stl_ascii: bool,
}

/// The format of a 3D mesh file that an export writes, by the extension.
fn mesh_export_format(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("stl") => Some("stl"),
        Some("obj") => Some("obj"),
        Some("gltf") => Some("gltf"),
        Some("glb") => Some("glb"),
        _ => None,
    }
}

/// The name of a length unit of `UNITS` in the language of the UI.
fn unit_name(id: &str) -> String {
    crate::enum_names::name("length-unit", id)
}

fn pattern_name(i: usize) -> String {
    match i {
        1 => tr!("io-dialog-patterns-gaps"),
        2 => tr!("io-dialog-patterns-units"),
        _ => tr!("io-dialog-patterns-none"),
    }
}

fn is_ifc(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ifc"))
}

impl ExportForm {
    pub fn new(path: PathBuf) -> Self {
        ExportForm {
            path,
            layers_as_layers: false,
            unit: 0,
            visible_only: false,
            patterns: 0,
            version: 0,
            ifc_classes_as_layers: true,
            ifc_include_2d: false,
            ifc_storeys: false,
            stl_ascii: false,
        }
    }

    pub fn params(&self) -> Value {
        let mut o = Map::new();
        if let Some(f) = mesh_export_format(&self.path) {
            let options = if f == "stl" {
                mesh_import::stl_export_options(self.stl_ascii)
            } else {
                json!({})
            };
            return json!({"path": self.path, "format": f, "options": options, "overwrite": true});
        }
        if is_ifc(&self.path) {
            o.insert(
                "classes_as_layers".into(),
                json!(self.ifc_classes_as_layers),
            );
            o.insert("include_2d".into(), json!(self.ifc_include_2d));
            o.insert("layers_as_storeys".into(), json!(self.ifc_storeys));
            o.insert("visible_only".into(), json!(self.visible_only));
            return json!({"path": self.path, "format": "ifc", "options": o});
        }
        if self.layers_as_layers {
            o.insert("layer_mode".into(), json!("layers"));
        }
        o.insert("unit".into(), json!(UNITS[self.unit.min(UNITS.len() - 1)]));
        o.insert("visible_only".into(), json!(self.visible_only));
        if let Some(p) = PATTERNS.get(self.patterns).filter(|p| **p != "none") {
            o.insert("patterns".into(), json!(p));
        }
        // The release is sent only when it is not the default.
        if let Some(v) = VERSIONS.get(self.version).filter(|v| **v != VERSIONS[0]) {
            o.insert("version".into(), json!(v));
        }
        json!({"path": self.path, "options": o})
    }
}

/// The choices of the DXF option `patterns`, in the order of the dialog.
const PATTERNS: [&str; 3] = ["none", "gaps", "units"];

/// The choices of the DXF option `version`, in the order of the dialog. The first is the default.
const VERSIONS: [&str; 5] = ["AC1018", "AC1015", "AC1027", "AC1032", "AC1009"];

fn version_name(i: usize) -> String {
    match i {
        1 => tr!("io-dialog-version-2000"),
        2 => tr!("io-dialog-version-2013"),
        3 => tr!("io-dialog-version-2018"),
        4 => tr!("io-dialog-version-r12"),
        _ => tr!("io-dialog-version-2004"),
    }
}

/// The dialogs that are open.
#[derive(Default)]
pub struct IoUi {
    import: Option<ImportForm>,
    export: Option<ExportForm>,
    /// Drawings with too much update data wait for the choice of the person, one question at a
    /// time. A second one that comes meanwhile waits in the queue.
    base_questions: std::collections::VecDeque<BaseQuestion>,
    /// What the soften question of a mesh file would do, for the choices it was made for.
    preview: Option<(PreviewKey, Preview)>,
}

/// The choices that a preview was made for: the file and the choices of the form.
type PreviewKey = (PathBuf, (Option<usize>, Option<bool>, bool, bool, u64));

/// What a dialog did.
#[derive(Debug, PartialEq, Eq)]
pub enum IoEvent {
    None,
    /// The document changed.
    Imported,
}

impl IoUi {
    /// File > Import...: asks for a file, then shows the options.
    pub fn start_import(&mut self, soften: SoftenPref) {
        let picked = rfd::FileDialog::new()
            .add_filter(
                tr!("io-dialog-dxf-dwg-ifc-shapefile-geojson"),
                &["dxf", "dwg", "ifc", "shp", "geojson", "json"],
            )
            .add_filter(tr!("meshio-ui-file-filter"), &["obj", "stl", "gltf", "glb"])
            .add_filter("DXF", &["dxf"])
            .add_filter(tr!("io-dialog-dwg-needs-a-converter"), &["dwg"])
            .add_filter("IFC", &["ifc"])
            .add_filter("Shapefile", &["shp"])
            .add_filter("GeoJSON", &["geojson", "json"])
            .pick_file();
        if let Some(p) = picked {
            self.open_import(p, soften);
        }
    }

    /// Shows the options for a file that is already chosen.
    pub fn open_import(&mut self, path: PathBuf, soften: SoftenPref) {
        self.preview = None;
        self.import = Some(ImportForm::with_soften(path, soften));
    }

    /// File > Export...: asks for a file name, then shows the options.
    pub fn start_export(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf"])
            .add_filter("GeoJSON", &["geojson"])
            .add_filter("IFC4", &["ifc"])
            .add_filter("STL", &["stl"])
            .add_filter("OBJ", &["obj"])
            .add_filter("glTF", &["gltf"])
            .add_filter("GLB", &["glb"])
            .add_filter(tr!("io-dialog-dwg-needs-a-converter"), &["dwg"])
            .set_file_name("drawing.dxf")
            .save_file();
        if let Some(p) = picked {
            self.export = Some(ExportForm::new(p));
        }
    }

    /// Opens the export window for a file that is already chosen (for the tests).
    #[cfg(test)]
    fn start_export_for_test(&mut self, path: PathBuf) {
        self.export = Some(ExportForm::new(path));
    }

    /// Runs `import.file` for the form. A drawing over the cap is not an error for the person: the
    /// question comes next (`show_base_question`), and the import runs again with the answer.
    fn run_import(
        &mut self,
        session: &mut Session,
        form: ImportForm,
        status: &mut String,
    ) -> IoEvent {
        match session.import_file_trusted(form.params()) {
            Ok(r) => {
                *status = if kind_of(&form.path) == Kind::Mesh {
                    mesh_import::imported_text(&r)
                } else {
                    tr!(
                        "io-dialog-imported-objects-skipped",
                        imported = r["imported"].as_u64().unwrap_or(0),
                        skipped = r["skipped"].as_u64().unwrap_or(0)
                    )
                };
                IoEvent::Imported
            }
            Err(e) => {
                if let Some((entities, cap, bytes)) = base_choice_of(&e) {
                    self.base_questions.push_back(BaseQuestion {
                        form,
                        entities,
                        cap,
                        bytes,
                    });
                } else {
                    *status = tr!("io-dialog-import-failed", message = e.message);
                }
                IoEvent::None
            }
        }
    }

    /// The question about a drawing with more entities than Kadakas keeps for updates. Returns
    /// `Some(Some(choice))` for a choice, `Some(None)` for Cancel and `None` while it waits.
    fn show_base_question(&self, ctx: &egui::Context) -> Option<Option<BaseChoice>> {
        let q = self.base_questions.front()?;
        let text = q.text();
        let mut answer = None;
        egui::Modal::new(egui::Id::new("win-import-base")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading(tr!("io-dialog-base-question-title"));
            ui.label(text);
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr!("io-dialog-base-keep-link")).clicked() {
                    answer = Some(Some(BaseChoice::Link));
                }
                if ui.button(tr!("io-dialog-base-no-update-data")).clicked() {
                    answer = Some(Some(BaseChoice::NoUpdateData));
                }
                if ui.button(tr!("io-dialog-cancel")).clicked() {
                    answer = Some(None);
                }
            });
        });
        answer
    }

    /// Draws the open dialogs. Runs the method when the user presses the button.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        status: &mut String,
        soften: &mut SoftenPref,
    ) -> IoEvent {
        let mut event = IoEvent::None;
        if let Some(form) = self.import.as_mut() {
            let mut run = false;
            let mut close = false;
            egui::Window::new(tr!("io-dialog-import"))
                .id(egui::Id::new("win-import"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(form.path.display().to_string());
                    ui.separator();
                    match kind_of(&form.path) {
                        Kind::Dxf => {
                            ui.checkbox(
                                &mut form.layers_as_layers,
                                tr!("io-dialog-dxf-layers-become-design-layers-default-classes"),
                            );
                            ui.horizontal(|ui| {
                                ui.label(tr!("io-dialog-unit"));
                                let label = form
                                    .unit
                                    .map_or(tr!("io-dialog-from-file"), |u| unit_name(UNITS[u]));
                                egui::ComboBox::from_id_salt("import_unit")
                                    .selected_text(label)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut form.unit,
                                            None,
                                            tr!("io-dialog-from-file"),
                                        );
                                        for (i, u) in UNITS.iter().enumerate() {
                                            ui.selectable_value(
                                                &mut form.unit,
                                                Some(i),
                                                unit_name(u),
                                            );
                                        }
                                    });
                            });
                            ui.checkbox(&mut form.explode_blocks, tr!("io-dialog-explode-blocks"));
                            ui.checkbox(
                                &mut form.hatches,
                                tr!("io-dialog-import-hatch-boundaries"),
                            );
                            ui.checkbox(
                                &mut form.only_points_with_z,
                                tr!("io-dialog-only-points-with-a-z-value"),
                            );
                            ui.checkbox(
                                &mut form.track_source,
                                tr!("io-dialog-keep-the-source-for-updates"),
                            );
                        }
                        Kind::Ifc => {
                            ui.label(tr!("io-dialog-ifc2x3-ifc4-or-ifc4x3"));
                            ui.checkbox(
                                &mut form.ifc_storeys,
                                tr!("io-dialog-building-storeys-become-design-layers-off-all"),
                            );
                            ui.checkbox(
                                &mut form.ifc_terrain_mesh,
                                tr!("io-dialog-terrain-as-a-3d-mesh-default-a"),
                            );
                            soften_question(ui, &mut form.ifc_soften, None);
                            if form.ifc_soften.on {
                                ui.label(tr!("meshio-ui-ifc-soften-hint"));
                            }
                        }
                        Kind::Mesh => {
                            if let Some(m) = form.mesh.as_mut() {
                                mesh_options(ui, m, "app_import");
                                let text = match &self.preview {
                                    Some((k, p)) if *k == (m.path.clone(), m.preview_key()) => {
                                        Some(p.text())
                                    }
                                    _ => Some(tr!("meshio-ui-preview-counting")),
                                };
                                soften_question(ui, &mut m.soften, text);
                            }
                        }
                        Kind::Gis => {
                            for (label, value) in [
                                (
                                    tr!("io-dialog-file-crs-if-the-file-has-none"),
                                    &mut form.source_crs,
                                ),
                                (
                                    tr!("io-dialog-document-crs-default-utm-zone"),
                                    &mut form.target_crs,
                                ),
                                (tr!("io-dialog-class-from-attribute"), &mut form.class_field),
                            ] {
                                ui.horizontal(|ui| {
                                    ui.label(label);
                                    ui.text_edit_singleline(value);
                                });
                            }
                        }
                    }
                    if kind_of(&form.path) != Kind::Ifc {
                        if kind_of(&form.path) != Kind::Mesh {
                            ui.checkbox(
                                &mut form.keep_origin,
                                tr!("io-dialog-keep-the-coordinates-of-the-file-default"),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui.label(tr!("io-dialog-design-layer"));
                            let mut edit = egui::TextEdit::singleline(&mut form.layer);
                            // For DXF and DWG an empty name makes a layer named after the file.
                            if kind_of(&form.path) == Kind::Dxf && form.track_source {
                                edit = edit.hint_text(if form.layers_as_layers {
                                    tr!("io-dialog-each-dxf-layer-becomes-its-own-layer")
                                } else {
                                    tr!("io-dialog-empty-a-new-layer-named-after-the-file")
                                });
                            }
                            ui.add(edit);
                        });
                    }
                    ui.horizontal(|ui| {
                        run = ui.button(tr!("io-dialog-import")).clicked();
                        close = ui.button(tr!("io-dialog-cancel")).clicked();
                    });
                });
            // The preview of the soften question: when the choices changed and no mouse button is
            // down (a drag of the angle does not count the edges at each step).
            let idle = !ctx.input(|i| i.pointer.any_down());
            let wanted = form
                .mesh
                .as_ref()
                .filter(|_| kind_of(&form.path) == Kind::Mesh && !run && !close)
                .cloned();
            if run {
                // The answer is kept only when the person imports.
                *soften = match &form.mesh {
                    Some(m) => m.soften,
                    None => form.ifc_soften,
                };
                let form = form.clone();
                event = self.run_import(session, form, status);
            }
            if run || close {
                self.import = None;
            } else if let Some(m) = wanted.filter(|m| idle && m.soften.on) {
                let key = (m.path.clone(), m.preview_key());
                if self.preview.as_ref().map(|(k, _)| k) != Some(&key) {
                    let p = mesh_import::compute_preview(&m, |params| {
                        session.import_file_trusted(params).map_err(|e| e.message)
                    });
                    if let Some(p) = p {
                        self.preview = Some((key, p));
                        ctx.request_repaint();
                    }
                }
            }
        }
        match self.show_base_question(ctx) {
            None => {}
            // Cancel: the question is gone and nothing was imported.
            Some(None) => {
                self.base_questions.pop_front();
            }
            Some(Some(choice)) => {
                if let Some(q) = self.base_questions.pop_front() {
                    let mut form = q.form;
                    form.base = Some(choice);
                    event = self.run_import(session, form, status);
                }
            }
        }
        if let Some(form) = self.export.as_mut() {
            let mut run = false;
            let mut close = false;
            egui::Window::new(tr!("io-dialog-export"))
                .id(egui::Id::new("win-export"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(form.path.display().to_string());
                    ui.separator();
                    if is_ifc(&form.path) {
                        ui.label(tr!("io-dialog-ifc4-ifc4-add2-tc1"));
                        ui.checkbox(
                            &mut form.ifc_classes_as_layers,
                            tr!("io-dialog-classes-become-ifc-presentation-layers"),
                        );
                        ui.checkbox(
                            &mut form.ifc_storeys,
                            tr!("io-dialog-design-layers-become-building-storeys-default-everyt"),
                        );
                        ui.checkbox(
                            &mut form.ifc_include_2d,
                            tr!("io-dialog-include-2d-objects-as-annotations"),
                        );
                    } else if let Some(f) = mesh_export_format(&form.path) {
                        match f {
                            "stl" => {
                                ui.label(tr!("meshio-ui-export-stl-hint"));
                                ui.checkbox(&mut form.stl_ascii, tr!("meshio-ui-export-ascii"));
                            }
                            "obj" => {
                                ui.label(tr!("meshio-ui-export-obj-hint"));
                            }
                            _ => {
                                ui.label(tr!("meshio-ui-export-gltf-hint"));
                            }
                        }
                    } else if kind_of(&form.path) == Kind::Dxf {
                        ui.checkbox(
                            &mut form.layers_as_layers,
                            tr!("io-dialog-one-dxf-layer-per-design-layer-default"),
                        );
                        ui.horizontal(|ui| {
                            ui.label(tr!("io-dialog-unit"));
                            egui::ComboBox::from_id_salt("export_unit")
                                .selected_text(unit_name(UNITS[form.unit]))
                                .show_ui(ui, |ui| {
                                    for (i, u) in UNITS.iter().enumerate() {
                                        ui.selectable_value(&mut form.unit, i, unit_name(u));
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr!("io-dialog-patterns"));
                            egui::ComboBox::from_id_salt("export_patterns")
                                .selected_text(pattern_name(form.patterns))
                                .show_ui(ui, |ui| {
                                    for i in 0..PATTERNS.len() {
                                        ui.selectable_value(&mut form.patterns, i, pattern_name(i));
                                    }
                                });
                        })
                        .response
                        .on_hover_text(tr!("io-dialog-patterns-hint"));
                        ui.horizontal(|ui| {
                            ui.label(tr!("io-dialog-version"));
                            egui::ComboBox::from_id_salt("export_version")
                                .selected_text(version_name(form.version))
                                .show_ui(ui, |ui| {
                                    for i in 0..VERSIONS.len() {
                                        ui.selectable_value(&mut form.version, i, version_name(i));
                                    }
                                });
                        })
                        .response
                        .on_hover_text(tr!("io-dialog-version-hint"));
                    }
                    if mesh_export_format(&form.path).is_none() {
                        ui.checkbox(
                            &mut form.visible_only,
                            tr!("io-dialog-only-visible-layers-and-classes"),
                        );
                    }
                    ui.horizontal(|ui| {
                        run = ui.button(tr!("io-dialog-export")).clicked();
                        close = ui.button(tr!("io-dialog-cancel")).clicked();
                    });
                });
            if run {
                let params = form.params();
                *status = match session.export_file_trusted(params) {
                    Ok(r) => tr!(
                        "io-dialog-exported-objects-to",
                        count = r["exported"].as_u64().unwrap_or(0),
                        path = form.path.display()
                    ),
                    Err(e) => tr!("io-dialog-export-failed", message = e.message),
                };
            }
            if run || close {
                self.export = None;
            }
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxf_import_params() {
        let mut f = ImportForm::new("/a/site.DXF".into());
        assert_eq!(
            f.params()["options"],
            json!({"explode_blocks": true, "hatches": true})
        );
        f.track_source = false;
        assert_eq!(
            f.params()["options"],
            json!({"explode_blocks": true, "hatches": true, "track_source": false})
        );
        f.track_source = true;
        f.layers_as_layers = true;
        f.unit = Some(2);
        f.layer = " Site ".into();
        f.only_points_with_z = true;
        f.source_crs = "ignored".into();
        f.keep_origin = true;
        let p = f.params();
        assert_eq!(p["path"], "/a/site.DXF");
        assert_eq!(p["options"]["layer_mode"], "layers");
        assert_eq!(p["options"]["unit"], "m");
        assert_eq!(p["options"]["layer"], "Site");
        assert_eq!(p["options"]["points"], "with_z");
        assert_eq!(p["options"]["origin"], "file");
        assert!(p["options"].get("source_crs").is_none());
    }

    #[test]
    fn the_answer_about_a_big_drawing_goes_into_the_options() {
        let mut f = ImportForm::new("/a/site.dxf".into());
        assert!(f.params()["options"].get("base").is_none());
        f.base = Some(BaseChoice::Link);
        assert_eq!(f.params()["options"]["base"], "link");
        f.base = Some(BaseChoice::NoUpdateData);
        assert_eq!(f.params()["options"]["base"], "none");
        // Files that have no update data ignore it.
        let mut g = ImportForm::new("/a/trees.geojson".into());
        g.base = Some(BaseChoice::Link);
        assert!(g.params()["options"].get("base").is_none());
    }

    #[test]
    fn a_drawing_over_the_cap_raises_the_question_and_cancel_imports_nothing() {
        let dir = std::env::temp_dir().join(format!("kadakas-iodlg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.dxf");
        let mut text = String::from("0\nSECTION\n2\nENTITIES\n");
        for k in 0..11 {
            text.push_str(&format!("0\nPOINT\n10\n{k}\n20\n0\n"));
        }
        text.push_str("0\nENDSEC\n0\nEOF\n");
        std::fs::write(&path, text).unwrap();
        let mut ui = IoUi::default();
        let mut session = Session::default();
        // The session keeps the update data of at most 10 entities.
        session.set_limits(kadakas_api::SessionLimits {
            base_records: 10,
            ..Default::default()
        });
        let mut status = String::new();
        let before = session.editor.doc().clone();
        let event = ui.run_import(&mut session, ImportForm::new(path.clone()), &mut status);
        assert_eq!(event, IoEvent::None);
        assert!(status.is_empty(), "no error message: the person is asked");
        let q = ui
            .base_questions
            .front()
            .cloned()
            .expect("the question waits");
        assert_eq!((q.entities, q.cap, q.bytes), (11, 10, None));
        assert_eq!(session.editor.doc(), &before);
        // A second question while one is on the screen waits in the queue.
        ui.run_import(&mut session, ImportForm::new(path.clone()), &mut status);
        assert_eq!(ui.base_questions.len(), 2);
        // The window shows the question in a frame and waits for a click.
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |u| {
            assert_eq!(ui.show_base_question(u.ctx()), None);
            let mut s = Session::default();
            assert_eq!(
                ui.show(
                    u.ctx(),
                    &mut s,
                    &mut String::new(),
                    &mut SoftenPref::default()
                ),
                IoEvent::None
            );
        });
        assert_eq!(ui.base_questions.len(), 2);
        // The text names the numbers, says that a link has the full path and that others see it,
        // and exists in Estonian.
        let text = q.text();
        assert!(text.contains("11") && text.contains("10"), "{text}");
        assert!(text.contains("full path") && text.contains("see"), "{text}");
        let et = kadakas_l10n::with_language("et", || q.text());
        assert!(et.contains("11") && et != text, "{et}");
        // The answer "without update data" imports, and the next question stays.
        let answered = ui.base_questions.pop_front().unwrap();
        let mut form = answered.form;
        form.base = Some(BaseChoice::NoUpdateData);
        assert_eq!(
            ui.run_import(&mut session, form, &mut status),
            IoEvent::Imported
        );
        assert_eq!(ui.base_questions.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_question_about_too_large_summaries_names_sizes() {
        let q = BaseQuestion {
            form: ImportForm::new("/a/b.dxf".into()),
            entities: 150_000,
            cap: 67_108_864,
            bytes: Some(80_000_000),
        };
        let text = q.text();
        assert!(
            text.contains("150000") && text.contains("80") && text.contains("67"),
            "{text}"
        );
        assert!(text.contains("full path"), "{text}");
        let et = kadakas_l10n::with_language("et", || q.text());
        assert!(et != text && et.contains("150000"), "{et}");
    }

    #[test]
    fn only_the_refusal_of_a_big_drawing_is_a_question() {
        let plain = kadakas_api::RpcError::validation("bad file");
        assert_eq!(base_choice_of(&plain), None);
        let no_numbers =
            kadakas_api::RpcError::validation("x").with_reason(kadakas_api::REASON_BASE_TOO_MANY);
        assert_eq!(base_choice_of(&no_numbers), None);
        let many = kadakas_api::RpcError::validation("x")
            .with_reason(kadakas_api::REASON_BASE_TOO_MANY)
            .with_details(json!({"entities": 300000, "cap": 200000}));
        assert_eq!(base_choice_of(&many), Some((300_000, 200_000, None)));
        let large = kadakas_api::RpcError::validation("x")
            .with_reason(kadakas_api::REASON_BASE_TOO_LARGE)
            .with_details(json!({"entities": 5, "bytes": 99, "cap": 64}));
        assert_eq!(base_choice_of(&large), Some((5, 64, Some(99))));
        let no_bytes = kadakas_api::RpcError::validation("x")
            .with_reason(kadakas_api::REASON_BASE_TOO_LARGE)
            .with_details(json!({"entities": 5, "cap": 64}));
        assert_eq!(base_choice_of(&no_bytes), None);
    }

    #[test]
    fn ifc_and_dwg_import_params() {
        let mut f = ImportForm::new("/a/site.IFC".into());
        f.layer = "ignored".into();
        f.keep_origin = true;
        assert_eq!(f.params()["options"], json!({"layers_from_storeys": true}));
        f.ifc_storeys = false;
        f.ifc_terrain_mesh = true;
        assert_eq!(
            f.params()["options"],
            json!({"layers_from_storeys": false, "terrain": "mesh"})
        );
        // DWG goes through the DXF options.
        let f = ImportForm::new("/a/survey.dwg".into());
        assert_eq!(
            f.params()["options"],
            json!({"explode_blocks": true, "hatches": true})
        );
    }

    #[test]
    fn gis_import_params() {
        let mut f = ImportForm::new("/a/trees.geojson".into());
        assert_eq!(f.params()["options"], json!({}));
        f.source_crs = "EPSG:3067".into();
        f.class_field = "species".into();
        let p = f.params();
        assert_eq!(p["options"]["source_crs"], "EPSG:3067");
        assert_eq!(p["options"]["class_field"], "species");
        assert!(p["options"].get("unit").is_none());
    }

    #[test]
    fn ifc_export_params() {
        let mut f = ExportForm::new("/a/site.IFC".into());
        let p = f.params();
        assert_eq!(p["format"], "ifc");
        assert_eq!(
            p["options"],
            json!({"classes_as_layers": true, "include_2d": false, "layers_as_storeys": false, "visible_only": false})
        );
        f.ifc_storeys = true;
        f.ifc_include_2d = true;
        f.ifc_classes_as_layers = false;
        f.visible_only = true;
        assert_eq!(
            f.params()["options"],
            json!({"classes_as_layers": false, "include_2d": true, "layers_as_storeys": true, "visible_only": true})
        );
        assert!(!is_ifc(Path::new("/a/site.dxf")));
    }

    #[test]
    fn export_params_and_dialog_state() {
        let mut f = ExportForm::new("/a/out.dxf".into());
        f.unit = 2;
        f.layers_as_layers = true;
        f.visible_only = true;
        let p = f.params();
        assert_eq!(
            p["options"],
            json!({"layer_mode": "layers", "unit": "m", "visible_only": true})
        );
        // The pattern option is sent only when it is asked for.
        f.patterns = 2;
        assert_eq!(f.params()["options"]["patterns"], "units");
        f.patterns = 1;
        assert_eq!(f.params()["options"]["patterns"], "gaps");
        f.patterns = 0;
        assert!(f.params()["options"].get("patterns").is_none());
        // The release is sent only when it is not the default.
        assert!(f.params()["options"].get("version").is_none());
        f.version = 1;
        assert_eq!(f.params()["options"]["version"], "AC1015");
        f.version = 4;
        assert_eq!(f.params()["options"]["version"], "AC1009");
        f.version = 99;
        assert!(f.params()["options"].get("version").is_none());
        for i in 0..VERSIONS.len() {
            assert!(!version_name(i).is_empty());
        }
        // Without open dialogs, nothing happens.
        let ctx = egui::Context::default();
        let mut ui = IoUi::default();
        let mut status = String::new();
        let mut session = Session::default();
        assert_eq!(
            ui.show(&ctx, &mut session, &mut status, &mut SoftenPref::default()),
            IoEvent::None
        );
    }

    const CUBE_OBJ: &str = "o Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

    #[test]
    fn a_mesh_file_has_its_own_options_and_the_soften_answer() {
        let mut f = ImportForm::new("/a/part.STL".into());
        assert_eq!(kind_of(&f.path), Kind::Mesh);
        assert_eq!(f.params()["options"], json!({}));
        f.layer = " Parts ".into();
        {
            let m = f.mesh.as_mut().unwrap();
            m.unit = Some(0);
            m.y_up = Some(false);
            m.center = true;
            m.soften = SoftenPref {
                on: true,
                angle: 30.0,
            };
        }
        assert_eq!(
            f.params()["options"],
            json!({"unit": "mm", "up_axis": "z", "center": true, "soften_below_angle": 30.0, "layer": "Parts"})
        );
        // The answer of the prefs starts the form.
        let g = ImportForm::with_soften(
            "/a/ball.obj".into(),
            SoftenPref {
                on: true,
                angle: 12.0,
            },
        );
        assert_eq!(g.params()["options"], json!({"soften_below_angle": 12.0}));
        // An IFC file takes the answer too, and the other files do not.
        let ifc = ImportForm::with_soften(
            "/a/site.ifc".into(),
            SoftenPref {
                on: true,
                angle: 25.0,
            },
        );
        assert_eq!(
            ifc.params()["options"],
            json!({"layers_from_storeys": true, "soften_below_angle": 25.0})
        );
        let dxf = ImportForm::with_soften(
            "/a/site.dxf".into(),
            SoftenPref {
                on: true,
                angle: 25.0,
            },
        );
        assert!(dxf.params()["options"].get("soften_below_angle").is_none());
        assert!(dxf.mesh.is_none());
        // The default answer is off.
        assert_eq!(
            ImportForm::new("/a/site.ifc".into()).params()["options"],
            json!({"layers_from_storeys": true})
        );
    }

    #[test]
    fn mesh_exports_pick_the_format_by_the_extension() {
        let mut f = ExportForm::new("/a/out.stl".into());
        let p = f.params();
        assert_eq!(
            (p["format"].as_str(), p["overwrite"].as_bool()),
            (Some("stl"), Some(true))
        );
        assert_eq!(p["options"], json!({}));
        f.stl_ascii = true;
        assert_eq!(f.params()["options"], json!({"encoding": "ascii"}));
        for (name, format) in [("a.OBJ", "obj"), ("a.gltf", "gltf"), ("a.glb", "glb")] {
            assert_eq!(ExportForm::new(name.into()).params()["format"], format);
        }
        assert_eq!(mesh_export_format(Path::new("a.dxf")), None);
        // A DXF export keeps its own options.
        assert_eq!(
            ExportForm::new("/a/out.dxf".into()).params()["options"]["unit"],
            "mm"
        );
    }

    #[test]
    fn the_window_counts_the_edges_of_a_mesh_import_and_imports_with_the_answer() {
        let dir = std::env::temp_dir().join(format!("kadakas-iodlg-mesh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cube.obj");
        std::fs::write(&path, CUBE_OBJ).unwrap();
        let mut ui = IoUi::default();
        ui.open_import(
            path.clone(),
            SoftenPref {
                on: true,
                angle: 100.0,
            },
        );
        let mut session = Session::default();
        let mut status = String::new();
        let mut soften = SoftenPref::default();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run_ui(Default::default(), |u| {
                ui.show(u.ctx(), &mut session, &mut status, &mut soften);
            });
        }
        let (_, p) = ui.preview.clone().expect("a preview");
        assert_eq!(
            p,
            Preview::Counted {
                softened: 12,
                edges: 12,
                faces: 6
            }
        );
        assert_eq!(session.editor.doc().objects.len(), 0);
        // The import itself reports the count in the status line.
        let form = ui.import.clone().unwrap();
        assert_eq!(
            ui.run_import(&mut session, form, &mut status),
            IoEvent::Imported
        );
        assert!(
            status.contains("12 triangles") && status.contains("12 edges"),
            "{status}"
        );
        assert_eq!(session.editor.doc().objects.len(), 1);
        // A mesh export through the dialog code.
        let out = dir.join("cube.stl");
        let r = session
            .export_file_trusted(ExportForm::new(out.clone()).params())
            .unwrap();
        assert_eq!(r["report"]["triangles"], 12);
        // The export window draws for each kind.
        for name in ["a.stl", "a.obj", "a.glb", "a.dxf", "a.ifc"] {
            ui.start_export_for_test(dir.join(name));
            let _ = ctx.run_ui(Default::default(), |u| {
                ui.show(u.ctx(), &mut session, &mut status, &mut soften);
            });
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
