//! The rows of Entity Info for a circle or an arc with an exact description (MOD-15): the kind,
//! the true radius, the center, the length and the number of segments, which the person can type.
//! The data and the change are in `kadakas_ui_core::modeling::curve`.

use kadakas_l10n::tr;
use kadakas_ui_core::modeling::{CurveShape, curve::SEGMENTS};

use crate::tray::TrayAction;

/// The name of a kind of curve.
pub fn kind_text(kind: &str) -> String {
    match kind {
        "circle" => tr!("modeler-curve-kind-circle"),
        "arc" => tr!("modeler-curve-kind-arc"),
        "polygon" => tr!("modeler-curve-kind-polygon"),
        other => other.to_string(),
    }
}

/// The rows of the shape, in a two column grid, and the field for the number of segments.
pub fn rows(ui: &mut egui::Ui, shape: &CurveShape, out: &mut Vec<TrayAction>) {
    let u = kadakas_units::current();
    let row = |ui: &mut egui::Ui, label: String, value: String| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value);
        ui.end_row();
    };
    row(ui, tr!("modeler-curve-kind"), kind_text(&shape.kind));
    row(ui, tr!("modeler-curve-radius"), u.length_text(shape.radius));
    row(
        ui,
        tr!("modeler-curve-center"),
        format!(
            "{}, {}, {}",
            u.length_text(shape.center[0]),
            u.length_text(shape.center[1]),
            u.length_text(shape.center[2])
        ),
    );
    if let Some(sweep) = shape.sweep_deg {
        row(ui, tr!("modeler-curve-sweep"), u.angle_text_d(sweep, 1));
    }
    row(
        ui,
        if shape.sweep_deg.is_some() {
            tr!("modeler-curve-length")
        } else {
            tr!("modeler-curve-circumference")
        },
        u.length_text(shape.length),
    );
    // The number of segments: a field with a button. The value waits in the memory of the
    // window until Set is pressed.
    ui.label(egui::RichText::new(tr!("modeler-curve-segments")).weak());
    let id = egui::Id::new(("curve-segments", shape.model.0, shape.curve));
    let current = shape.segments as u32;
    let mut value: u32 = ui.data_mut(|d| d.get_temp(id)).unwrap_or(current);
    ui.horizontal(|ui| {
        let drag = egui::DragValue::new(&mut value).range(SEGMENTS);
        ui.add(drag)
            .on_hover_text(tr!("modeler-curve-segments-hint"));
        if ui.button(tr!("modeler-curve-segments-apply")).clicked() && value != current {
            out.push(TrayAction::CurveSegments {
                model: shape.model,
                curve: shape.curve,
                segments: value,
            });
        }
    });
    ui.data_mut(|d| d.insert_temp(id, value));
    ui.end_row();
}
