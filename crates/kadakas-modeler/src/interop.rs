//! File > Import and File > Export of the modeler (MOD-12): OBJ, STL, glTF, GLB and IFC in, STL,
//! OBJ, glTF, GLB, IFC and DXF out. The windows build the same JSON as an agent would send to
//! `import.file` and `export.file`, so the modeler and the API use one code path.
//!
//! An import of a mesh file asks the question "Soften edges under an angle?" (off by default, the
//! angle 20 degrees) and shows how many edges the angle would soften. The answer goes to the
//! prefs and comes back at the next import.

use std::path::{Path, PathBuf};

use kadakas_api::Session;
use kadakas_l10n::tr;
use kadakas_ui_core::mesh_import::{self, MeshImportForm, Preview, SoftenPref};
use kadakas_view3d::import_widgets::{mesh_options, soften_question};
use serde_json::{Value, json};

/// What the person chose for a file to import.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportChoice {
    pub path: PathBuf,
    /// An OBJ, STL or glTF file: the unit, the axis and the soften answer.
    pub mesh: Option<MeshImportForm>,
    /// An IFC file: the soften answer.
    pub ifc_soften: SoftenPref,
}

impl ImportChoice {
    pub fn new(path: PathBuf, soften: SoftenPref) -> Self {
        ImportChoice {
            mesh: MeshImportForm::new(path.clone(), soften),
            ifc_soften: soften.valid(),
            path,
        }
    }

    /// The parameters of `import.file`.
    pub fn params(&self) -> Value {
        if let Some(m) = &self.mesh {
            return m.params(false);
        }
        let mut o = serde_json::Map::new();
        if self.ifc_soften.on {
            o.insert(
                "soften_below_angle".into(),
                json!(self.ifc_soften.valid().angle),
            );
        }
        json!({"path": self.path, "options": o})
    }
}

/// The format of a file that an export writes, by the extension, with its options.
pub fn export_params(path: &Path, stl_ascii: bool) -> Option<Value> {
    let format = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()?
    {
        "stl" => "stl",
        "obj" => "obj",
        "gltf" => "gltf",
        "glb" => "glb",
        "ifc" => "ifc",
        "dxf" => "dxf",
        _ => return None,
    };
    let options = if format == "stl" {
        mesh_import::stl_export_options(stl_ascii)
    } else {
        json!({})
    };
    Some(json!({"path": path, "format": format, "options": options, "overwrite": true}))
}

/// What a window did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    /// The document changed: the view shows all of it.
    Imported,
}

type PreviewKey = (PathBuf, mesh_import::PreviewKey);

/// The open windows of File > Import and File > Export.
#[derive(Default)]
pub struct Interop {
    import: Option<ImportChoice>,
    export: Option<(PathBuf, bool)>,
    preview: Option<(PreviewKey, Preview)>,
}

/// The file picker of File > Import.
pub fn pick_import() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(
            tr!("meshio-ui-all-filter"),
            &["obj", "stl", "gltf", "glb", "step", "stp", "ifc"],
        )
        .add_filter(tr!("meshio-ui-file-filter"), &["obj", "stl", "gltf", "glb"])
        .add_filter(tr!("meshio-ui-step-filter"), &["step", "stp"])
        .add_filter("IFC", &["ifc"])
        .pick_file()
}

/// The file picker of File > Export.
pub fn pick_export(current: Option<&Path>) -> Option<PathBuf> {
    let stem = current
        .and_then(|p| p.file_stem())
        .and_then(|n| n.to_str())
        .unwrap_or("model");
    rfd::FileDialog::new()
        .add_filter("STL", &["stl"])
        .add_filter("OBJ", &["obj"])
        .add_filter("glTF", &["gltf"])
        .add_filter("GLB", &["glb"])
        .add_filter("IFC4", &["ifc"])
        .add_filter("DXF", &["dxf"])
        .set_file_name(format!("{stem}.stl"))
        .save_file()
}

impl Interop {
    /// Shows the options of an import for a file that is chosen.
    pub fn start_import(&mut self, path: PathBuf, soften: SoftenPref) {
        self.preview = None;
        self.import = Some(ImportChoice::new(path, soften));
    }

    /// Shows the options of an export to a file that is chosen.
    pub fn start_export(&mut self, path: PathBuf) {
        self.export = Some((path, false));
    }

    /// True while a window is open.
    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.import.is_some() || self.export.is_some()
    }

    /// The choices of the import window, for the tests.
    #[cfg(test)]
    pub fn import_choice(&self) -> Option<&ImportChoice> {
        self.import.as_ref()
    }

    /// Draws the windows and runs the method when the person presses the button. `soften` is the
    /// answer to the question in the prefs: a change goes there at once.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        status: &mut String,
        soften: &mut SoftenPref,
    ) -> Outcome {
        let mut outcome = Outcome::None;
        if let Some(choice) = self.import.as_mut() {
            let (mut run, mut close) = (false, false);
            egui::Window::new(tr!("modeler-import-title"))
                .id(egui::Id::new("win-modeler-import"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(choice.path.display().to_string());
                    ui.separator();
                    if let Some(m) = choice.mesh.as_mut() {
                        mesh_options(ui, m, "modeler_import");
                        let text = match &self.preview {
                            Some((k, p)) if *k == (m.path.clone(), m.preview_key()) => {
                                Some(p.text())
                            }
                            _ => Some(tr!("meshio-ui-preview-counting")),
                        };
                        soften_question(ui, &mut m.soften, text);
                    } else {
                        ui.label(tr!("io-dialog-ifc2x3-ifc4-or-ifc4x3"));
                        soften_question(ui, &mut choice.ifc_soften, None);
                        if choice.ifc_soften.on {
                            ui.label(tr!("meshio-ui-ifc-soften-hint"));
                        }
                    }
                    ui.horizontal(|ui| {
                        run = ui.button(tr!("modeler-import-button")).clicked();
                        close = ui.button(tr!("modeler-import-cancel")).clicked();
                    });
                });
            let idle = !ctx.input(|i| i.pointer.any_down());
            let wanted = choice.mesh.clone().filter(|_| !run && !close);
            if run {
                // The answer is kept only when the person imports.
                *soften = choice.mesh.as_ref().map_or(choice.ifc_soften, |m| m.soften);
                outcome = run_import(session, choice, status);
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
        if let Some((path, ascii)) = self.export.as_mut() {
            let (mut run, mut close) = (false, false);
            egui::Window::new(tr!("modeler-export-title"))
                .id(egui::Id::new("win-modeler-export"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(path.display().to_string());
                    ui.separator();
                    match path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_ascii_lowercase)
                        .as_deref()
                    {
                        Some("stl") => {
                            ui.label(tr!("meshio-ui-export-stl-hint"));
                            ui.checkbox(ascii, tr!("meshio-ui-export-ascii"));
                        }
                        Some("obj") => {
                            ui.label(tr!("meshio-ui-export-obj-hint"));
                        }
                        Some("gltf" | "glb") => {
                            ui.label(tr!("meshio-ui-export-gltf-hint"));
                        }
                        Some("ifc") => {
                            ui.label(tr!("meshio-ui-export-ifc-hint"));
                        }
                        Some("dxf") => {
                            ui.label(tr!("meshio-ui-export-dxf-hint"));
                        }
                        _ => {}
                    }
                    ui.horizontal(|ui| {
                        run = ui.button(tr!("meshio-ui-export")).clicked();
                        close = ui.button(tr!("modeler-import-cancel")).clicked();
                    });
                });
            if run {
                *status = run_export(session, path, *ascii);
            }
            if run || close {
                self.export = None;
            }
        }
        outcome
    }
}

/// Runs `import.file` for the choices and writes the result on the status line.
pub fn run_import(session: &mut Session, choice: &ImportChoice, status: &mut String) -> Outcome {
    match session.import_file_trusted(choice.params()) {
        Ok(r) => {
            *status = if choice.mesh.is_some() {
                mesh_import::imported_text(&r)
            } else {
                tr!(
                    "io-dialog-imported-objects-skipped",
                    imported = r["imported"].as_u64().unwrap_or(0),
                    skipped = r["skipped"].as_u64().unwrap_or(0)
                )
            };
            Outcome::Imported
        }
        Err(e) => {
            *status = tr!("io-dialog-import-failed", message = e.message);
            Outcome::None
        }
    }
}

/// Runs `export.file` for the file and returns the line for the status line.
pub fn run_export(session: &mut Session, path: &Path, ascii: bool) -> String {
    match export_params(path, ascii) {
        Some(params) => match session.export_file_trusted(params) {
            Ok(r) => tr!(
                "io-dialog-exported-objects-to",
                count = r["exported"].as_u64().unwrap_or(0),
                path = path.display()
            ),
            Err(e) => tr!("io-dialog-export-failed", message = e.message),
        },
        None => tr!(
            "io-dialog-export-failed",
            message = path.display().to_string()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE: &str = "o Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kadakas-modeler-interop-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_choices_build_the_import_call() {
        let c = ImportChoice::new("a.obj".into(), SoftenPref::default());
        assert!(c.mesh.is_some());
        assert_eq!(c.params()["options"], json!({}));
        let on = SoftenPref {
            on: true,
            angle: 25.0,
        };
        let c = ImportChoice::new("a.STL".into(), on);
        assert_eq!(c.params()["options"], json!({"soften_below_angle": 25.0}));
        let ifc = ImportChoice::new("site.ifc".into(), on);
        assert!(ifc.mesh.is_none());
        assert_eq!(ifc.params()["options"], json!({"soften_below_angle": 25.0}));
        let ifc = ImportChoice::new("site.ifc".into(), SoftenPref::default());
        assert_eq!(ifc.params()["options"], json!({}));
    }

    #[test]
    fn the_extension_picks_the_export_format() {
        for (name, format) in [
            ("a.stl", "stl"),
            ("a.OBJ", "obj"),
            ("a.gltf", "gltf"),
            ("a.glb", "glb"),
            ("a.ifc", "ifc"),
            ("a.dxf", "dxf"),
        ] {
            let p = export_params(Path::new(name), false).unwrap();
            assert_eq!(p["format"], format);
            assert_eq!(p["overwrite"], true);
        }
        assert_eq!(
            export_params(Path::new("a.stl"), true).unwrap()["options"],
            json!({"encoding": "ascii"})
        );
        assert_eq!(
            export_params(Path::new("a.obj"), true).unwrap()["options"],
            json!({})
        );
        assert!(export_params(Path::new("a.txt"), false).is_none());
        assert!(export_params(Path::new("noext"), false).is_none());
    }

    #[test]
    fn an_import_and_an_export_run_and_say_so() {
        let dir = temp("run");
        let obj = dir.join("cube.obj");
        std::fs::write(&obj, CUBE).unwrap();
        let mut session = Session::default();
        let mut status = String::new();
        let mut choice = ImportChoice::new(
            obj.clone(),
            SoftenPref {
                on: true,
                angle: 20.0,
            },
        );
        assert_eq!(
            run_import(&mut session, &choice, &mut status),
            Outcome::Imported
        );
        assert!(
            status.contains("12 triangles") && status.contains('0'),
            "{status}"
        );
        // A file that is not there: the error goes to the status line, nothing is imported.
        choice.path = dir.join("none.obj");
        choice.mesh.as_mut().unwrap().path = choice.path.clone();
        assert_eq!(
            run_import(&mut session, &choice, &mut status),
            Outcome::None
        );
        assert!(
            status.contains("none.obj") || status.contains("Import failed"),
            "{status}"
        );
        let out = dir.join("cube.stl");
        let line = run_export(&mut session, &out, true);
        assert!(line.contains("cube.stl"), "{line}");
        assert!(std::fs::read_to_string(&out).unwrap().starts_with("solid"));
        let bad = run_export(&mut session, &dir.join("cube.xyz"), false);
        assert!(bad.contains("cube.xyz"), "{bad}");
        // An IFC import of a missing file says so too.
        let ifc = ImportChoice::new(dir.join("none.ifc"), SoftenPref::default());
        assert_eq!(run_import(&mut session, &ifc, &mut status), Outcome::None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_window_counts_the_edges_before_the_import() {
        let dir = temp("window");
        let obj = dir.join("cube.obj");
        std::fs::write(&obj, CUBE).unwrap();
        let mut ui = Interop::default();
        assert!(!ui.is_open());
        ui.start_import(
            obj.clone(),
            SoftenPref {
                on: true,
                angle: 100.0,
            },
        );
        assert!(ui.is_open());
        assert!(ui.import_choice().is_some());
        let mut session = Session::default();
        let mut status = String::new();
        let mut soften = SoftenPref {
            on: true,
            angle: 100.0,
        };
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run_ui(Default::default(), |u| {
                assert_eq!(
                    ui.show(u.ctx(), &mut session, &mut status, &mut soften),
                    Outcome::None
                );
            });
        }
        // The preview ran for the choices: a cube with 12 edges, all at 90 degrees, under 100.
        let (_, p) = ui.preview.clone().expect("a preview");
        assert_eq!(
            p,
            Preview::Counted {
                softened: 12,
                edges: 12,
                faces: 6
            }
        );
        // It changed nothing.
        assert_eq!(session.editor.doc().objects.len(), 0);
        // A change of the angle makes a new preview.
        ui.import
            .as_mut()
            .unwrap()
            .mesh
            .as_mut()
            .unwrap()
            .soften
            .angle = 20.0;
        for _ in 0..2 {
            let _ = ctx.run_ui(Default::default(), |u| {
                ui.show(u.ctx(), &mut session, &mut status, &mut soften);
            });
        }
        let (_, p) = ui.preview.clone().unwrap();
        assert_eq!(
            p,
            Preview::Counted {
                softened: 0,
                edges: 12,
                faces: 6
            }
        );
        // The export window shows too.
        ui.start_export(dir.join("out.stl"));
        let _ = ctx.run_ui(Default::default(), |u| {
            ui.show(u.ctx(), &mut session, &mut status, &mut soften);
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_file_makes_a_failed_preview() {
        let mut ui = Interop::default();
        let on = SoftenPref {
            on: true,
            angle: 20.0,
        };
        ui.start_import(PathBuf::from("/no/such/file.stl"), on);
        let mut session = Session::default();
        let mut status = String::new();
        let mut soften = SoftenPref::default();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run_ui(Default::default(), |u| {
                ui.show(u.ctx(), &mut session, &mut status, &mut soften);
            });
        }
        assert!(matches!(ui.preview, Some((_, Preview::Failed(_)))));
        // Cancelled or not run, the answer in the prefs is not changed by the window.
        assert_eq!(soften, SoftenPref::default());
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn nothing_is_counted_while_the_question_is_off() {
        let dir = std::env::temp_dir().join(format!("kadakas-modeler-off-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let obj = dir.join("a.obj");
        std::fs::write(&obj, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        let mut ui = Interop::default();
        ui.start_import(obj, SoftenPref::default());
        let mut session = Session::default();
        let (mut status, mut soften) = (String::new(), SoftenPref::default());
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run_ui(Default::default(), |u| {
                ui.show(u.ctx(), &mut session, &mut status, &mut soften);
            });
        }
        assert!(ui.preview.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
