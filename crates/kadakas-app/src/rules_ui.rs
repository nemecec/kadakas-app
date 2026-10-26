//! The "Display rules" section: the northing/easting grid, project phases, the data visualization
//! (color by a property) and the survey point of the georeference. Every action is an API call, so the
//! window, the API and scripts do the same thing and each action is one undo step.

use crate::units_ui::DragValueExt as _;
use egui::{DragValue, Ui};
use kadakas_api::Session;
use kadakas_core::viewrules::Phase;
use kadakas_l10n::tr;
use kadakas_view3d::numfmt::UiNumber;
use serde_json::json;

/// What the section remembers between frames.
#[derive(Clone, Debug)]
pub struct RulesState {
    grid_spacing: f64,
    phase: Phase,
    field: String,
    bins: usize,
    easting: f64,
    northing: f64,
}

impl Default for RulesState {
    fn default() -> Self {
        RulesState {
            grid_spacing: 10.0,
            phase: Phase::Demolished,
            field: "Kind".into(),
            bins: 5,
            easting: 0.0,
            northing: 0.0,
        }
    }
}

/// The properties that are offered to color by.
fn fields() -> Vec<String> {
    let mut v: Vec<String> = kadakas_core::props::NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    v.extend(
        [
            "Plant.category",
            "Plant.irrigation.water_need",
            "Plant.irrigation.hydrozone",
        ]
        .map(String::from),
    );
    v
}

fn report(status: &mut String, r: Result<serde_json::Value, kadakas_api::RpcError>) {
    if let Err(e) = r {
        *status = tr!("rules-ui-error", message = e.message);
    }
}

/// Draws the section.
pub fn section(ui: &mut Ui, session: &mut Session, status: &mut String) {
    let key = egui::Id::new("rules_ui_state");
    let mut st: RulesState = ui.data_mut(|d| d.get_temp(key)).unwrap_or_default();
    egui::CollapsingHeader::new(tr!("rules-ui-display-rules"))
        .id_salt("rules-ui-display-rules")
        .default_open(false)
        .show(ui, |ui| {
            let doc = session.editor.doc();
            let rules = doc.drawing.view_rules.clone();
            let georeferenced = doc.georef.is_some();
            let selection: Vec<kadakas_core::uid::Uid> =
                session.selection.iter().map(|i| i.0).collect();

            ui.strong(tr!("rules-ui-northing-easting-grid"));
            ui.horizontal(|ui| {
                let mut on = rules.ne_grid.is_some();
                if ui
                    .add_enabled(
                        georeferenced,
                        egui::Checkbox::new(&mut on, tr!("rules-ui-show")),
                    )
                    .on_hover_text(tr!("rules-ui-needs-a-georeference-set-it-with-the"))
                    .changed()
                {
                    let spacing = if on {
                        json!(st.grid_spacing)
                    } else {
                        json!(null)
                    };
                    report(
                        status,
                        session.handle("grid.set", json!({"spacing": spacing})),
                    );
                }
                let r = ui.add(
                    DragValue::new(&mut st.grid_spacing)
                        .speed(1.0)
                        .range(0.01..=100_000.0)
                        .length_m(),
                );
                if (r.drag_stopped() || r.lost_focus()) && rules.ne_grid.is_some() {
                    report(
                        status,
                        session.handle("grid.set", json!({"spacing": st.grid_spacing})),
                    );
                }
            });

            ui.add_space(6.0);
            ui.strong(tr!("rules-ui-survey-point"));
            ui.horizontal(|ui| {
                ui.label(tr!("rules-ui-e"));
                ui.add(DragValue::new(&mut st.easting).speed(1.0).ui_number());
                ui.label(tr!("rules-ui-n"));
                ui.add(DragValue::new(&mut st.northing).speed(1.0).ui_number());
            });
            let one = selection.len() == 1;
            if ui
                .add_enabled(
                    one,
                    egui::Button::new(tr!("rules-ui-set-from-selected-point")),
                )
                .on_hover_text(tr!("rules-ui-the-selected-survey-point-plant-or-circle"))
                .clicked()
            {
                report(
                    status,
                    session.handle(
                        "georef.survey_point",
                        json!({"object": selection[0], "map": [st.easting, st.northing]}),
                    ),
                );
            }

            ui.add_space(6.0);
            ui.strong(tr!("rules-ui-phases"));
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("rules_phase")
                    .selected_text(crate::enum_names::name("phase", st.phase.id()))
                    .show_ui(ui, |ui| {
                        for p in Phase::ALL {
                            ui.selectable_value(
                                &mut st.phase,
                                p,
                                crate::enum_names::name("phase", p.id()),
                            );
                        }
                    });
                if ui
                    .add_enabled(
                        !selection.is_empty(),
                        egui::Button::new(tr!("rules-ui-set-phase")),
                    )
                    .clicked()
                {
                    report(
                        status,
                        session.handle(
                            "phase.set",
                            json!({"ids": selection, "phase": st.phase.id()}),
                        ),
                    );
                }
            });
            let mut show = rules.phases.is_some_and(|p| p.enabled);
            if ui
                .checkbox(&mut show, tr!("rules-ui-show-phase-display"))
                .on_hover_text(tr!("rules-ui-existing-objects-grey-demolished-objects-red"))
                .changed()
            {
                let p = if rules.phases.is_some() {
                    json!({"enabled": show})
                } else {
                    json!({})
                };
                report(status, session.handle("phase.display", p));
            }
            if let Some(p) = rules.phases {
                let mut hide = !p.demolished.visible;
                if ui
                    .checkbox(&mut hide, tr!("rules-ui-hide-demolished"))
                    .changed()
                {
                    report(
                        status,
                        session.handle("phase.display", json!({"demolished": {"visible": !hide}})),
                    );
                }
            }

            ui.add_space(6.0);
            ui.strong(tr!("rules-ui-data-visualization"));
            egui::ComboBox::from_id_salt("rules_field")
                .selected_text(st.field.clone())
                .show_ui(ui, |ui| {
                    for f in fields() {
                        ui.selectable_value(&mut st.field, f.clone(), f);
                    }
                });
            ui.horizontal(|ui| {
                if ui.button(tr!("rules-ui-by-value")).clicked() {
                    report(
                        status,
                        session.handle(
                            "visualization.auto",
                            json!({"field": st.field, "target": "both"}),
                        ),
                    );
                }
                if ui.button(tr!("rules-ui-by-range")).clicked() {
                    report(
                        status,
                        session.handle(
                            "visualization.auto",
                            json!({"field": st.field, "bins": st.bins, "target": "both"}),
                        ),
                    );
                }
                ui.add(
                    DragValue::new(&mut st.bins)
                        .speed(0.2)
                        .range(2..=12)
                        .ui_number(),
                );
                if ui
                    .add_enabled(
                        rules.visualization.is_some(),
                        egui::Button::new(tr!("rules-ui-clear")),
                    )
                    .clicked()
                {
                    report(status, session.handle("visualization.clear", json!({})));
                }
            });
            if let Some(v) = &rules.visualization {
                let (rows, none) = kadakas_core::viewrules::legend(session.editor.doc(), v);
                ui.label(tr!("rules-ui-colored-by", field = v.field));
                for r in rows {
                    ui.horizontal(|ui| {
                        let [cr, cg, cb, _] = r.color.0;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 2.0, egui::Color32::from_rgb(cr, cg, cb));
                        ui.label(format!("{}  ({})", r.label, r.count));
                    });
                }
                if none > 0 {
                    ui.weak(tr!("rules-ui-object-s-stay-as-they-are", none = none));
                }
            }
        });
    ui.data_mut(|d| d.insert_temp(key, st));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(s: &mut Session) -> String {
        let ctx = egui::Context::default();
        let mut status = String::new();
        // The section is closed at first. Open it by clicking is not possible here, so draw twice.
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                section(ui, s, &mut status);
            });
        }
        status
    }

    #[test]
    fn the_section_draws_with_and_without_rules() {
        let mut s = Session::default();
        assert_eq!(frame(&mut s), "");
        s.handle(
            "georef.survey_point",
            json!({"model": [0, 0], "map": [100, 200], "crs": "EPSG:3067"}),
        )
        .unwrap();
        s.handle("grid.set", json!({"spacing": 5})).unwrap();
        s.handle("phase.display", json!({})).unwrap();
        s.handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [10, 0]}}}),
        )
        .unwrap();
        s.handle("visualization.auto", json!({"field": "Kind"}))
            .unwrap();
        assert_eq!(frame(&mut s), "");
        assert!(fields().iter().any(|f| f == "Plant.category"));
    }

    #[test]
    fn errors_go_to_the_status() {
        let mut status = String::new();
        report(&mut status, Err(kadakas_api::RpcError::app("boom")));
        assert_eq!(status, "Error: boom");
        report(&mut status, Ok(json!({})));
        assert_eq!(status, "Error: boom");
        assert_eq!(RulesState::default().phase, Phase::Demolished);
    }
}
