//! File > Import and File > Export: pick a file, set the options, run the API method.
//!
//! The dialog builds the same JSON as an agent would send to `import.file` and
//! `export.file`, so the app and the API use one code path.

use std::path::{Path, PathBuf};

use cadektor_api::Session;
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
                if self.only_points_with_z {
                    o.insert("points".into(), json!("with_z"));
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
                "DXF, DWG, IFC, Shapefile, GeoJSON",
                &["dxf", "dwg", "ifc", "shp", "geojson", "json"],
            )
            .add_filter("DXF", &["dxf"])
            .add_filter("DWG (needs a converter, see docs/dwg.md)", &["dwg"])
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
            .add_filter("DWG (needs a converter, see docs/dwg.md)", &["dwg"])
            .set_file_name("drawing.dxf")
            .save_file();
        if let Some(p) = picked {
            self.export = Some(ExportForm::new(p));
        }
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
            egui::Window::new("Import")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(form.path.display().to_string());
                    ui.separator();
                    match kind_of(&form.path) {
                        Kind::Dxf => {
                            ui.checkbox(
                                &mut form.layers_as_layers,
                                "DXF layers become design layers (default: classes)",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Unit");
                                let label = form.unit.map_or("From file", |u| UNITS[u]);
                                egui::ComboBox::from_id_salt("import_unit")
                                    .selected_text(label)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut form.unit, None, "From file");
                                        for (i, u) in UNITS.iter().enumerate() {
                                            ui.selectable_value(&mut form.unit, Some(i), *u);
                                        }
                                    });
                            });
                            ui.checkbox(&mut form.explode_blocks, "Explode blocks");
                            ui.checkbox(&mut form.hatches, "Import hatch boundaries");
                            ui.checkbox(&mut form.only_points_with_z, "Only points with a Z value");
                        }
                        Kind::Ifc => {
                            ui.label("IFC2x3, IFC4 or IFC4x3");
                            ui.checkbox(
                                &mut form.ifc_storeys,
                                "Building storeys become design layers (default: all on the layer Site)",
                            );
                            ui.checkbox(
                                &mut form.ifc_terrain_mesh,
                                "Terrain as a 3D mesh (default: a site model)",
                            );
                        }
                        Kind::Gis => {
                            for (label, value) in [
                                ("File CRS (if the file has none)", &mut form.source_crs),
                                ("Document CRS (default: UTM zone)", &mut form.target_crs),
                                ("Class from attribute", &mut form.class_field),
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
                            "Keep the coordinates of the file (default: center the drawing)",
                        );
                        ui.horizontal(|ui| {
                            ui.label("Design layer");
                            ui.text_edit_singleline(&mut form.layer);
                        });
                    }
                    ui.horizontal(|ui| {
                        run = ui.button("Import").clicked();
                        close = ui.button("Cancel").clicked();
                    });
                });
            if run {
                let params = form.params();
                match session.import_file_trusted(params) {
                    Ok(r) => {
                        *status = format!(
                            "Imported {} objects, skipped {}",
                            r["imported"], r["skipped"]
                        );
                        event = IoEvent::Imported;
                    }
                    Err(e) => *status = format!("Import failed: {}", e.message),
                }
            }
            if run || close {
                self.import = None;
            }
        }
        if let Some(form) = self.export.as_mut() {
            let mut run = false;
            let mut close = false;
            egui::Window::new("Export").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(form.path.display().to_string());
                ui.separator();
                if is_ifc(&form.path) {
                    ui.label("IFC4 (IFC4 ADD2 TC1)");
                    ui.checkbox(&mut form.ifc_classes_as_layers, "Classes become IFC presentation layers");
                    ui.checkbox(&mut form.ifc_storeys, "Design layers become building storeys (default: everything in the site)");
                    ui.checkbox(&mut form.ifc_include_2d, "Include 2D objects as annotations");
                } else if kind_of(&form.path) == Kind::Dxf {
                    ui.checkbox(&mut form.layers_as_layers, "One DXF layer per design layer (default: per class)");
                    ui.horizontal(|ui| {
                        ui.label("Unit");
                        egui::ComboBox::from_id_salt("export_unit").selected_text(UNITS[form.unit]).show_ui(ui, |ui| {
                            for (i, u) in UNITS.iter().enumerate() {
                                ui.selectable_value(&mut form.unit, i, *u);
                            }
                        });
                    });
                }
                ui.checkbox(&mut form.visible_only, "Only visible layers and classes");
                ui.horizontal(|ui| {
                    run = ui.button("Export").clicked();
                    close = ui.button("Cancel").clicked();
                });
            });
            if run {
                let params = form.params();
                *status = match session.export_file_trusted(params) {
                    Ok(r) => format!(
                        "Exported {} objects to {}",
                        r["exported"],
                        form.path.display()
                    ),
                    Err(e) => format!("Export failed: {}", e.message),
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
