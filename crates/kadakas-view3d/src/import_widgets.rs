//! The widgets of the import window of mesh files (MOD-12), shared by `kadakas-app` and
//! `kadakas-modeler`: the unit and the up axis of the file, and the question "Soften edges
//! under an angle?". The logic is in `kadakas_ui_core::mesh_import`.

use kadakas_l10n::tr;
use kadakas_ui_core::mesh_import::{self, MeshImportForm, SoftenPref};

/// The unit and the up axis of the file, and moving the model to the origin. `salt` makes the ids
/// of the combo boxes different for two windows.
pub fn mesh_options(ui: &mut egui::Ui, m: &mut MeshImportForm, salt: &str) {
    if m.kind.takes_frame() || m.kind.is_step() {
        ui.horizontal(|ui| {
            ui.label(tr!("meshio-ui-unit"));
            let default = if m.kind.is_step() {
                tr!("meshio-ui-unit-from-file")
            } else {
                tr!(
                    "meshio-ui-unit-default",
                    unit = mesh_import::unit_name(m.kind.default_unit())
                )
            };
            let label = match m.unit {
                None => default.clone(),
                Some(i) => mesh_import::unit_name(mesh_import::UNITS[i]),
            };
            egui::ComboBox::from_id_salt((salt, "unit"))
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut m.unit, None, default);
                    for (i, u) in mesh_import::UNITS.iter().enumerate() {
                        ui.selectable_value(&mut m.unit, Some(i), mesh_import::unit_name(u));
                    }
                });
        });
    }
    if m.kind.takes_frame() {
        ui.horizontal(|ui| {
            ui.label(tr!("meshio-ui-up-axis"));
            let default = if m.kind.default_y_up() {
                tr!("meshio-ui-up-default-y")
            } else {
                tr!("meshio-ui-up-default-z")
            };
            let label = match m.y_up {
                None => default.clone(),
                Some(true) => tr!("meshio-ui-up-y"),
                Some(false) => tr!("meshio-ui-up-z"),
            };
            egui::ComboBox::from_id_salt((salt, "up"))
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut m.y_up, None, default);
                    ui.selectable_value(&mut m.y_up, Some(true), tr!("meshio-ui-up-y"));
                    ui.selectable_value(&mut m.y_up, Some(false), tr!("meshio-ui-up-z"));
                });
        });
    } else if m.kind.is_step() {
        ui.label(egui::RichText::new(tr!("meshio-ui-step-hint")).weak());
        // The tolerance: automatic, or a number.
        let mut auto = m.tolerance.is_none();
        if ui
            .checkbox(&mut auto, tr!("meshio-ui-step-tolerance-auto"))
            .changed()
        {
            m.tolerance = if auto { None } else { Some(0.1) };
        }
        if let Some(t) = m.tolerance.as_mut() {
            ui.horizontal(|ui| {
                ui.label(tr!("meshio-ui-step-tolerance"));
                ui.add(
                    egui::DragValue::new(t)
                        .range(mesh_import::TOLERANCE_RANGE.0..=mesh_import::TOLERANCE_RANGE.1)
                        .speed(0.01),
                );
            });
        }
        ui.checkbox(&mut m.per_product, tr!("meshio-ui-step-per-product"));
        ui.checkbox(&mut m.keep_exact, tr!("meshio-ui-step-keep-exact"));
    } else {
        ui.label(tr!("meshio-ui-gltf-fixed"));
    }
    ui.checkbox(&mut m.center, tr!("meshio-ui-center"));
}

/// The question "Soften edges under an angle?": a check box, the angle, the hint, and under it
/// the line of the preview if there is one (only while the question is on).
pub fn soften_question(ui: &mut egui::Ui, pref: &mut SoftenPref, preview: Option<String>) {
    ui.separator();
    ui.checkbox(&mut pref.on, tr!("meshio-ui-soften-question"));
    if pref.on {
        ui.horizontal(|ui| {
            ui.label(tr!("meshio-ui-soften-angle"));
            ui.add(
                egui::DragValue::new(&mut pref.angle)
                    .range(0.0..=180.0)
                    .speed(0.5)
                    .suffix("\u{b0}"),
            );
        });
        ui.label(egui::RichText::new(tr!("meshio-ui-soften-hint")).weak());
        if let Some(text) = preview {
            ui.label(text);
        }
    }
}
