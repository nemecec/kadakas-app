//! The status bar: the instruction for the active tool at the left (or the last message), and the
//! Measurements box at the right. The box only displays today: it shows the size of the
//! selection, or the field of view while the Zoom tool is active. MOD-04 lets the person type
//! into it.

use kadakas_ui_core::modeler::{Measurement, ModelerTool};

/// The width of the Measurements box in points.
pub const BOX_WIDTH: f32 = 150.0;

/// Draws the status bar. With `snap` (MOD-19) the refusal of a solid tool that a snap would
/// resolve has the button "Snap and retry" next to it. Gives true when the button was pressed.
pub fn show(
    ui: &mut egui::Ui,
    status: &str,
    tool: ModelerTool,
    measurement: &Measurement,
    snap: bool,
) -> bool {
    let mut pressed = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The box first: the instruction at the left gets the room that is left.
            let mut text = measurement.text().to_string();
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id_salt("measurements-box")
                    .desired_width(BOX_WIDTH)
                    .interactive(false),
            );
            ui.label(tr_id(measurement));
            ui.separator();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if snap {
                    pressed = ui
                        .button(kadakas_l10n::translate("modeler-solid-snap-retry", &[]))
                        .on_hover_text(kadakas_l10n::translate("modeler-solid-snap-retry-tip", &[]))
                        .clicked();
                }
                let line = if status.is_empty() {
                    tool.hint()
                } else {
                    status.to_string()
                };
                ui.add(egui::Label::new(line).truncate());
            });
        });
    });
    pressed
}

/// The name in front of the box, in the language of the UI.
fn tr_id(m: &Measurement) -> String {
    kadakas_l10n::translate(m.label_id(), &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_follows_what_the_box_shows() {
        let dims = Measurement::Dimensions("1 x 2 x 3".into());
        let fov = Measurement::FieldOfView("35".into());
        assert_ne!(tr_id(&dims), tr_id(&fov));
        assert_eq!(tr_id(&Measurement::Empty), tr_id(&dims));
    }
}
