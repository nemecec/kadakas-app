//! Soft and smooth edges in the window (MOD-08): the Soften Edges panel of the tray and the
//! Soften, Smooth and Unsoften items of the context menu of the 3D view. The logic is
//! `kadakas_ui_core::modeling::soften`.

use kadakas_l10n::tr;
use kadakas_ui_core::modeling::{EdgeStyle, SoftenPanel};

use crate::app::Cmd;

/// The panel: the angle slider and the two check boxes. True when the person changed a value
/// (the change applies at once, so the model shows it while the slider moves). `enabled` is
/// false when no edge or face is selected.
pub fn panel(ui: &mut egui::Ui, p: &mut SoftenPanel, enabled: bool) -> bool {
    let mut changed = false;
    if !enabled {
        ui.label(egui::RichText::new(tr!("modeler-soften-select")).weak());
    }
    ui.add_enabled_ui(enabled, |ui| {
        ui.label(tr!("modeler-soften-angle"));
        let r = ui.add(
            egui::Slider::new(&mut p.angle, 0.0..=180.0)
                .suffix("°")
                .fixed_decimals(1),
        );
        changed |= r.changed();
        changed |= ui
            .checkbox(&mut p.smooth, tr!("modeler-soften-smooth-normals"))
            .changed();
        changed |= ui
            .checkbox(&mut p.coplanar, tr!("modeler-soften-coplanar"))
            .changed();
    });
    changed && enabled
}

/// The items of the context menu for selected edges (and the edges of selected faces).
pub fn context_menu(ui: &mut egui::Ui, enabled: bool, cmds: &mut Vec<Cmd>) {
    ui.separator();
    for (label, style) in [
        (tr!("modeler-edge-soften"), EdgeStyle::Soften),
        (tr!("modeler-edge-smooth"), EdgeStyle::Smooth),
        (tr!("modeler-edge-unsoften"), EdgeStyle::Unsoften),
    ] {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            cmds.push(Cmd::EdgeStyle(style));
            ui.close();
        }
    }
}
