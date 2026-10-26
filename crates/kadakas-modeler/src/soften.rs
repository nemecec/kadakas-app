//! Soft and smooth edges in the window (MOD-08): the Soften Edges panel of the tray and the
//! Soften, Smooth and Unsoften items of the context menu of the 3D view. The logic is
//! `kadakas_ui_core::modeling::soften`.

use kadakas_l10n::tr;
use kadakas_ui_core::modeling::{EdgeStyle, SoftenPanel};
use kadakas_view3d::numfmt::UiNumber;

use crate::app::Cmd;

/// What a change of the panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The slider is being dragged: show what it would soften, change nothing yet.
    Preview,
    /// Apply the values (the slider was let go, a box was clicked): one undo step.
    Apply,
}

/// The panel: the angle slider and the two check boxes. While the slider is dragged the window
/// only draws what it would soften; the edit runs when it is let go, so a drag is one edit, one
/// undo step and one message to the other users. `enabled` is false when no edge or face is
/// selected.
pub fn panel(ui: &mut egui::Ui, p: &mut SoftenPanel, enabled: bool) -> Option<Change> {
    let mut out = None;
    if !enabled {
        ui.label(egui::RichText::new(tr!("modeler-soften-select")).weak());
    }
    ui.add_enabled_ui(enabled, |ui| {
        ui.label(tr!("modeler-soften-angle"));
        let r = ui.add(
            egui::Slider::new(&mut p.angle, 0.0..=180.0)
                .suffix("°")
                .fixed_decimals(1)
                .ui_number(),
        );
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            out = Some(Change::Apply);
        } else if r.changed() {
            out = Some(Change::Preview);
        }
        if ui
            .checkbox(&mut p.smooth, tr!("modeler-soften-smooth-normals"))
            .changed()
            || ui
                .checkbox(&mut p.coplanar, tr!("modeler-soften-coplanar"))
                .changed()
        {
            out = Some(Change::Apply);
        }
    });
    out.filter(|_| enabled)
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
