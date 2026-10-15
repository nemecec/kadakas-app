//! File > Import and File > Export: pick a file, set the options, run the API method.
//!
//! The dialog builds the same JSON as an agent would send to `import.file` and
//! `export.file`, so the app and the API use one code path.

use kadakas_l10n::tr;
use std::path::{Path, PathBuf};

use kadakas_api::Session;
use serde_json::{Map, Value, json};

const UNITS: &[&str] = &["mm", "cm", "m", "km", "inch", "foot", "yard", "mile"];

/// Which kind of file an import has, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// DXF, and DWG (read through a converter that becomes DXF first).
    Dxf,
    Gis,
    Ifc,
}

fn kind_of(path: &Path) -> Kind {
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
}

impl ImportForm {
    pub fn new(path: PathBuf) -> Self {
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
        }
    }

    /// The `import.file` parameters.
    pub fn params(&self) -> Value {
        let mut o = Map::new();
        let text = |o: &mut Map<String, Value>, key: &str, v: &str| {
            if !v.trim().is_empty() {
                o.insert(key.into(), json!(v.trim()));
            }
        };
        if kind_of(&self.path) == Kind::Ifc {
            o.insert("layers_from_storeys".into(), json!(self.ifc_storeys));
            if self.ifc_terrain_mesh {
                o.insert("terrain".into(), json!("mesh"));
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
            Kind::Ifc => {}
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
    /// IFC: one presentation layer per class.
    pub ifc_classes_as_layers: bool,
    /// IFC: write 2D objects as annotations.
    pub ifc_include_2d: bool,
    /// IFC: one building storey per design layer.
    pub ifc_storeys: bool,
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
            ifc_classes_as_layers: true,
            ifc_include_2d: false,
            ifc_storeys: false,
        }
    }

    pub fn params(&self) -> Value {
        let mut o = Map::new();
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
        json!({"path": self.path, "options": o})
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
}

/// What a dialog did.
#[derive(Debug, PartialEq, Eq)]
pub enum IoEvent {
    None,
    /// The document changed.
    Imported,
}

impl IoUi {
    /// File > Import...: asks for a file, then shows the options.
    pub fn start_import(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter(
                tr!("io-dialog-dxf-dwg-ifc-shapefile-geojson"),
                &["dxf", "dwg", "ifc", "shp", "geojson", "json"],
            )
            .add_filter("DXF", &["dxf"])
            .add_filter(tr!("io-dialog-dwg-needs-a-converter"), &["dwg"])
            .add_filter("IFC", &["ifc"])
            .add_filter("Shapefile", &["shp"])
            .add_filter("GeoJSON", &["geojson", "json"])
            .pick_file();
        if let Some(p) = picked {
            self.import = Some(ImportForm::new(p));
        }
    }

    /// File > Export...: asks for a file name, then shows the options.
    pub fn start_export(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf"])
            .add_filter("GeoJSON", &["geojson"])
            .add_filter("IFC4", &["ifc"])
            .add_filter(tr!("io-dialog-dwg-needs-a-converter"), &["dwg"])
            .set_file_name("drawing.dxf")
            .save_file();
        if let Some(p) = picked {
            self.export = Some(ExportForm::new(p));
        }
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
                *status = tr!(
                    "io-dialog-imported-objects-skipped",
                    imported = r["imported"].as_u64().unwrap_or(0),
                    skipped = r["skipped"].as_u64().unwrap_or(0)
                );
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
                                    .map_or(tr!("io-dialog-from-file"), |u| UNITS[u].to_string());
                                egui::ComboBox::from_id_salt("import_unit")
                                    .selected_text(label)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut form.unit,
                                            None,
                                            tr!("io-dialog-from-file"),
                                        );
                                        for (i, u) in UNITS.iter().enumerate() {
                                            ui.selectable_value(&mut form.unit, Some(i), *u);
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
                        ui.checkbox(
                            &mut form.keep_origin,
                            tr!("io-dialog-keep-the-coordinates-of-the-file-default"),
                        );
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
            if run {
                let form = form.clone();
                event = self.run_import(session, form, status);
            }
            if run || close {
                self.import = None;
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
                    } else if kind_of(&form.path) == Kind::Dxf {
                        ui.checkbox(
                            &mut form.layers_as_layers,
                            tr!("io-dialog-one-dxf-layer-per-design-layer-default"),
                        );
                        ui.horizontal(|ui| {
                            ui.label(tr!("io-dialog-unit"));
                            egui::ComboBox::from_id_salt("export_unit")
                                .selected_text(UNITS[form.unit])
                                .show_ui(ui, |ui| {
                                    for (i, u) in UNITS.iter().enumerate() {
                                        ui.selectable_value(&mut form.unit, i, *u);
                                    }
                                });
                        });
                    }
                    ui.checkbox(
                        &mut form.visible_only,
                        tr!("io-dialog-only-visible-layers-and-classes"),
                    );
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
            assert_eq!(ui.show(u.ctx(), &mut s, &mut String::new()), IoEvent::None);
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
        // Without open dialogs, nothing happens.
        let ctx = egui::Context::default();
        let mut ui = IoUi::default();
        let mut status = String::new();
        let mut session = Session::default();
        assert_eq!(ui.show(&ctx, &mut session, &mut status), IoEvent::None);
    }
}
