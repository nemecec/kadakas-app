//! The tray on the right: Entity Info, Materials and Scenes, one under the other, each with a
//! title bar that collapses it. Entity Info shows the basic data of the selection. Materials
//! (paint, MOD-05) and Scenes (saved views, MOD-10) are placeholders that say so.

use kadakas_l10n::tr;
use kadakas_ui_core::modeler::EntityInfo;

/// Which panels of the tray are open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayState {
    pub entity_info: bool,
    pub materials: bool,
    pub scenes: bool,
}

impl Default for TrayState {
    fn default() -> Self {
        TrayState {
            entity_info: true,
            materials: true,
            scenes: true,
        }
    }
}

/// The width of the tray in points.
pub const WIDTH: f32 = 240.0;

/// The name of a kind of object for the panel: the API id with spaces. (The ids stay English in
/// the files and the API; a newer program may know more kinds than this one has names for.)
pub fn kind_text(kind: &str) -> String {
    let s = kind.replace('_', " ");
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => s,
    }
}

fn lengths(size: [f64; 3]) -> String {
    let u = kadakas_units::current();
    tr!(
        "uic-modeler-dimensions-value",
        x = u.length_text(size[0]),
        y = u.length_text(size[1]),
        z = u.length_text(size[2])
    )
}

fn row(ui: &mut egui::Ui, label: String, value: String) {
    ui.label(egui::RichText::new(label).weak());
    ui.label(value);
    ui.end_row();
}

/// The Entity Info panel: what the selection is.
pub fn entity_info(ui: &mut egui::Ui, info: &EntityInfo) {
    match info {
        EntityInfo::Nothing { layer, class } => {
            ui.label(egui::RichText::new(tr!("modeler-entity-nothing")).weak());
            egui::Grid::new("entity-info-nothing")
                .num_columns(2)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-active-layer"), layer.clone());
                    row(ui, tr!("modeler-entity-active-class"), class.clone());
                });
        }
        EntityInfo::One {
            id,
            kind,
            layer,
            class,
            size,
            min,
            surface,
        } => {
            egui::Grid::new("entity-info-one")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-type"), kind_text(kind));
                    row(ui, tr!("modeler-entity-id"), id.clone());
                    row(ui, tr!("modeler-entity-layer"), layer.clone());
                    row(ui, tr!("modeler-entity-class"), class.clone());
                    if let Some(s) = size {
                        row(ui, tr!("modeler-entity-size"), lengths(*s));
                    }
                    if let Some(m) = min {
                        row(ui, tr!("modeler-entity-corner"), lengths(*m));
                    }
                    if let Some(s) = surface {
                        row(ui, tr!("modeler-entity-vertices"), s.vertices.to_string());
                        row(ui, tr!("modeler-entity-edges"), s.edges.to_string());
                        row(ui, tr!("modeler-entity-faces"), s.faces.to_string());
                        row(
                            ui,
                            tr!("modeler-entity-solid"),
                            if s.closed {
                                tr!("modeler-entity-solid-closed")
                            } else {
                                tr!("modeler-entity-solid-open")
                            },
                        );
                        if s.not_intersected > 0 {
                            row(
                                ui,
                                tr!("modeler-entity-not-intersected"),
                                tr!(
                                    "modeler-entity-not-intersected-value",
                                    count = s.not_intersected
                                ),
                            );
                        }
                    }
                });
        }
        EntityInfo::Face {
            model,
            id,
            area,
            normal,
            edges,
            holes,
        } => {
            let u = kadakas_units::current();
            egui::Grid::new("entity-info-face")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-type"), tr!("modeler-entity-face"));
                    row(ui, tr!("modeler-entity-id"), id.to_string());
                    row(ui, tr!("modeler-entity-model"), model.clone());
                    row(ui, tr!("modeler-entity-area"), u.area_text(*area));
                    row(
                        ui,
                        tr!("modeler-entity-normal"),
                        format!("{:.2}, {:.2}, {:.2}", normal[0], normal[1], normal[2]),
                    );
                    row(ui, tr!("modeler-entity-loop-edges"), edges.to_string());
                    row(ui, tr!("modeler-entity-holes"), holes.to_string());
                });
        }
        EntityInfo::Edge {
            model,
            id,
            length,
            start,
            end,
            curve,
            faces,
        } => {
            let u = kadakas_units::current();
            egui::Grid::new("entity-info-edge")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    row(ui, tr!("modeler-entity-type"), tr!("modeler-entity-edge"));
                    row(ui, tr!("modeler-entity-id"), id.to_string());
                    row(ui, tr!("modeler-entity-model"), model.clone());
                    row(ui, tr!("modeler-entity-length"), u.length_text(*length));
                    row(ui, tr!("modeler-entity-start"), lengths(*start));
                    row(ui, tr!("modeler-entity-end"), lengths(*end));
                    if let Some((c, n)) = curve {
                        row(
                            ui,
                            tr!("modeler-entity-curve"),
                            tr!("modeler-entity-curve-value", id = *c, edges = *n),
                        );
                    }
                    row(ui, tr!("modeler-entity-face-uses"), faces.to_string());
                });
        }
        EntityInfo::Parts { faces, edges } => {
            ui.label(tr!("modeler-entity-parts", faces = *faces, edges = *edges));
        }
        EntityInfo::Many { count, size } => {
            ui.label(tr!("modeler-entity-many", count = *count));
            if let Some(s) = size {
                egui::Grid::new("entity-info-many")
                    .num_columns(2)
                    .show(ui, |ui| {
                        row(ui, tr!("modeler-entity-size"), lengths(*s));
                    });
            }
        }
    }
}

/// The Materials panel. Paint comes with the Paint Bucket (MOD-05).
pub fn materials(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(tr!("modeler-materials-later")).weak());
}

/// The Scenes panel. Scenes are saved views (MOD-10).
pub fn scenes(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(tr!("modeler-scenes-later")).weak());
}

/// Draws the whole tray inside `ui`.
pub fn show(ui: &mut egui::Ui, state: &mut TrayState, info: &EntityInfo) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        panel(
            ui,
            "tray-entity-info",
            tr!("modeler-tray-entity-info"),
            &mut state.entity_info,
            |ui| {
                entity_info(ui, info);
            },
        );
        panel(
            ui,
            "tray-materials",
            tr!("modeler-tray-materials"),
            &mut state.materials,
            materials,
        );
        panel(
            ui,
            "tray-scenes",
            tr!("modeler-tray-scenes"),
            &mut state.scenes,
            scenes,
        );
    });
}

/// One panel: a title bar that collapses and expands it.
fn panel(
    ui: &mut egui::Ui,
    id: &str,
    title: String,
    open: &mut bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    let header = egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(id)
        .open(Some(*open))
        .show(ui, body);
    if header.header_response.clicked() {
        *open = !*open;
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_reads_as_words() {
        assert_eq!(kind_text("hardscape"), "Hardscape");
        assert_eq!(kind_text("landscape_area"), "Landscape area");
        assert_eq!(kind_text(""), "");
        assert_eq!(kind_text("ünknown_thing"), "Ünknown thing");
    }
}
