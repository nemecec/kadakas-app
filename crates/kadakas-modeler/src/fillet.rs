//! Fillet and Chamfer in the window (MOD-21): the panel of the tray, the items of the Edit menu
//! and of the context menu of the 3D view. The logic is `kadakas_ui_core::modeling::fillet`.

use kadakas_l10n::tr;
use kadakas_ui_core::modeling::{FilletKind, FilletPanel};
use kadakas_ui_number::UiNumber;

use crate::app::Cmd;

/// The panel: the size, the segments, Smooth and the two buttons. `enabled` is false when no edge
/// or face is selected. The size and the segments change nothing until a button is clicked.
pub fn panel(ui: &mut egui::Ui, p: &mut FilletPanel, enabled: bool) -> Option<FilletKind> {
    let mut out = None;
    if !enabled {
        ui.label(egui::RichText::new(tr!("modeler-fillet-select")).weak());
    }
    ui.add_enabled_ui(enabled, |ui| {
        ui.label(tr!("modeler-fillet-size"));
        ui.add(
            egui::DragValue::new(&mut p.size)
                .range(0.01..=100_000.0)
                .speed(0.1)
                .suffix(" mm")
                .ui_number(),
        );
        ui.label(tr!("modeler-fillet-segments"));
        ui.add(
            egui::DragValue::new(&mut p.segments)
                .range(0..=kadakas_core::surface::FILLET_MAX_SEGMENTS)
                .ui_number(),
        );
        ui.checkbox(&mut p.smooth, tr!("modeler-fillet-smooth"));
        ui.horizontal(|ui| {
            if ui.button(tr!("modeler-edit-fillet")).clicked() {
                out = Some(FilletKind::Fillet);
            }
            if ui.button(tr!("modeler-edit-chamfer")).clicked() {
                out = Some(FilletKind::Chamfer);
            }
        });
    });
    out.filter(|_| enabled)
}

/// The two items, for the Edit menu and the context menu of the 3D view.
pub fn entries(ui: &mut egui::Ui, enabled: bool, cmds: &mut Vec<Cmd>) {
    for (label, kind) in [
        (tr!("modeler-edit-fillet"), FilletKind::Fillet),
        (tr!("modeler-edit-chamfer"), FilletKind::Chamfer),
    ] {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            cmds.push(Cmd::Fillet(kind));
            ui.close();
        }
    }
}
