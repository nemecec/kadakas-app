//! The plant style spreadsheet: all plant styles in one table for batch editing. A cell edit is a
//! command (`ModifyResource`), and "Apply to shown rows" changes one field of many styles in one
//! undo step.

use cadektor_api::Session;
use cadektor_core::command::Command;
use cadektor_core::plants::care::WaterNeed;
use cadektor_core::plants::style::{PlantCategory, PlantStyle};
use cadektor_core::resource::{ResourceBody, ResourceId};
use egui::{DragValue, Ui};

use crate::panels::run_command;

/// A column of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Name,
    IdCode,
    Botanical,
    Common,
    Category,
    Spread,
    Height,
    Price,
    Size,
    Container,
    Water,
    Hydrozone,
    Hours,
    Used,
}

const COLS: [Col; 14] = [
    Col::Name,
    Col::IdCode,
    Col::Botanical,
    Col::Common,
    Col::Category,
    Col::Spread,
    Col::Height,
    Col::Price,
    Col::Size,
    Col::Container,
    Col::Water,
    Col::Hydrozone,
    Col::Hours,
    Col::Used,
];

impl Col {
    fn label(self) -> &'static str {
        match self {
            Col::Name => "Style name",
            Col::IdCode => "ID",
            Col::Botanical => "Botanical name",
            Col::Common => "Common name",
            Col::Category => "Category",
            Col::Spread => "Spread mm",
            Col::Height => "Height mm",
            Col::Price => "Price",
            Col::Size => "Size",
            Col::Container => "Container",
            Col::Water => "Water need",
            Col::Hydrozone => "Hydrozone",
            Col::Hours => "Hours/yr",
            Col::Used => "Used",
        }
    }
}

/// The field that "Apply to shown rows" sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Bulk {
    #[default]
    Water,
    Hydrozone,
    Category,
    Container,
    Size,
}

/// The state of the window.
pub struct PlantSheet {
    pub open: bool,
    filter: String,
    sort: Col,
    ascending: bool,
    bulk: Bulk,
    bulk_text: String,
    bulk_need: WaterNeed,
    bulk_category: PlantCategory,
}

impl Default for PlantSheet {
    fn default() -> Self {
        PlantSheet {
            open: false,
            filter: String::new(),
            sort: Col::Name,
            ascending: true,
            bulk: Bulk::default(),
            bulk_text: String::new(),
            bulk_need: WaterNeed::default(),
            bulk_category: PlantCategory::default(),
        }
    }
}

struct Row {
    id: ResourceId,
    name: String,
    style: PlantStyle,
    used: usize,
}

fn sort_key(r: &Row, c: Col) -> (String, f64) {
    let s = &r.style;
    match c {
        Col::Name => (r.name.to_lowercase(), 0.0),
        Col::IdCode => (s.id_code.to_lowercase(), 0.0),
        Col::Botanical => (s.botanical_name.to_lowercase(), 0.0),
        Col::Common => (s.common_name.to_lowercase(), 0.0),
        Col::Category => (s.category.id().into(), 0.0),
        Col::Spread => (String::new(), s.spread),
        Col::Height => (String::new(), s.height),
        Col::Price => (String::new(), s.price),
        Col::Size => (s.size.to_lowercase(), 0.0),
        Col::Container => (s.container.to_lowercase(), 0.0),
        Col::Water => (String::new(), s.irrigation.water_need as u8 as f64),
        Col::Hydrozone => (s.irrigation.hydrozone.to_lowercase(), 0.0),
        Col::Hours => (String::new(), s.maintenance.hours_per_year),
        Col::Used => (String::new(), r.used as f64),
    }
}

fn matches(r: &Row, filter: &str) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty()
        || [
            &r.name,
            &r.style.botanical_name,
            &r.style.common_name,
            &r.style.id_code,
            &r.style.irrigation.hydrozone,
            &r.style.container,
        ]
        .iter()
        .any(|t| t.to_lowercase().contains(&f))
}

/// A text cell. Returns the new text when the edit is finished and the text changed.
fn text_cell(ui: &mut Ui, key: egui::Id, current: &str, width: f32) -> Option<String> {
    let mut buf: String = ui
        .data_mut(|d| d.get_temp(key))
        .unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(width));
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    (r.lost_focus() && buf != current).then_some(buf)
}

/// A number cell. Returns the new value when the edit is finished and the value changed.
fn number_cell(ui: &mut Ui, key: egui::Id, current: f64, speed: f64, min: f64) -> Option<f64> {
    let mut v: f64 = ui.data_mut(|d| d.get_temp(key)).unwrap_or(current);
    let r = ui.add(DragValue::new(&mut v).speed(speed).range(min..=1.0e9));
    if r.dragged() || r.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, v));
    } else {
        ui.data_mut(|d| d.remove::<f64>(key));
    }
    ((r.drag_stopped() || r.lost_focus()) && (v - current).abs() > 1e-12).then_some(v)
}

/// What a row edit changes. `None` leaves the style as it is.
fn edit_row(ui: &mut Ui, r: &Row) -> Option<(String, PlantStyle)> {
    let (mut name, mut s) = (r.name.clone(), r.style.clone());
    let key = |c: usize| egui::Id::new(("plant_sheet", r.id.0, c));
    let mut changed = false;
    if let Some(v) = text_cell(ui, key(0), &r.name, 130.0) {
        name = v;
        changed = true;
    }
    if let Some(v) = text_cell(ui, key(1), &r.style.id_code, 40.0) {
        s.id_code = v;
        changed = true;
    }
    if let Some(v) = text_cell(ui, key(2), &r.style.botanical_name, 150.0) {
        s.botanical_name = v;
        changed = true;
    }
    if let Some(v) = text_cell(ui, key(3), &r.style.common_name, 120.0) {
        s.common_name = v;
        changed = true;
    }
    let before = s.category;
    egui::ComboBox::from_id_salt(key(4))
        .selected_text(s.category.id())
        .width(90.0)
        .show_ui(ui, |ui| {
            for c in PlantCategory::ALL {
                ui.selectable_value(&mut s.category, c, c.id());
            }
        });
    changed |= s.category != before;
    if let Some(v) = number_cell(ui, key(5), r.style.spread, 50.0, 1.0) {
        s.spread = v;
        changed = true;
    }
    if let Some(v) = number_cell(ui, key(6), r.style.height, 50.0, 1.0) {
        s.height = v;
        changed = true;
    }
    if let Some(v) = number_cell(ui, key(7), r.style.price, 0.5, 0.0) {
        s.price = v;
        changed = true;
    }
    if let Some(v) = text_cell(ui, key(8), &r.style.size, 90.0) {
        s.size = v;
        changed = true;
    }
    if let Some(v) = text_cell(ui, key(9), &r.style.container, 50.0) {
        s.container = v;
        changed = true;
    }
    let before = s.irrigation.water_need;
    egui::ComboBox::from_id_salt(key(10))
        .selected_text(s.irrigation.water_need.id())
        .width(90.0)
        .show_ui(ui, |ui| {
            for w in WaterNeed::ALL {
                ui.selectable_value(&mut s.irrigation.water_need, w, w.id());
            }
        });
    changed |= s.irrigation.water_need != before;
    if let Some(v) = text_cell(ui, key(11), &r.style.irrigation.hydrozone, 70.0) {
        s.irrigation.hydrozone = v;
        changed = true;
    }
    if let Some(v) = number_cell(ui, key(12), r.style.maintenance.hours_per_year, 0.1, 0.0) {
        s.maintenance.hours_per_year = v;
        changed = true;
    }
    ui.label(r.used.to_string());
    changed.then_some((name, s))
}

/// The commands that set one field of the given rows.
fn bulk_commands(state: &PlantSheet, rows: &[&Row]) -> Vec<Command> {
    rows.iter()
        .filter_map(|r| {
            let mut s = r.style.clone();
            match state.bulk {
                Bulk::Water => s.irrigation.water_need = state.bulk_need,
                Bulk::Hydrozone => s.irrigation.hydrozone = state.bulk_text.trim().to_string(),
                Bulk::Category => s.category = state.bulk_category,
                Bulk::Container => s.container = state.bulk_text.trim().to_string(),
                Bulk::Size => s.size = state.bulk_text.trim().to_string(),
            }
            (s != r.style).then(|| Command::ModifyResource {
                id: r.id,
                name: None,
                body: Some(ResourceBody::plant_style(s)),
            })
        })
        .collect()
}

fn collect(session: &Session, state: &PlantSheet) -> Vec<Row> {
    let doc = session.editor.doc();
    let mut rows: Vec<Row> = doc
        .resources
        .plant_styles()
        .map(|(id, name, s)| Row {
            id,
            name: name.to_string(),
            style: s.clone(),
            used: doc
                .objects
                .values()
                .filter(|o| cadektor_core::plants::link::style_of(o) == Some(id))
                .count(),
        })
        .collect();
    rows.sort_by(|a, b| {
        let (ka, kb) = (sort_key(a, state.sort), sort_key(b, state.sort));
        let o = ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1));
        if state.ascending { o } else { o.reverse() }
    });
    rows
}

/// Shows the window if it is open.
pub fn window(
    ctx: &egui::Context,
    session: &mut Session,
    state: &mut PlantSheet,
    status: &mut String,
) {
    if !state.open {
        return;
    }
    let mut open = state.open;
    let mut commands: Vec<Command> = Vec::new();
    egui::Window::new("Plant style spreadsheet")
        .open(&mut open)
        .default_size([1150.0, 420.0])
        .show(ctx, |ui| {
            let rows = collect(session, state);
            ui.horizontal_wrapped(|ui| {
                ui.label("Filter");
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter)
                        .hint_text("name, code, hydrozone")
                        .desired_width(160.0),
                );
                ui.separator();
                ui.label("Set");
                egui::ComboBox::from_id_salt("plant_sheet_bulk")
                    .selected_text(match state.bulk {
                        Bulk::Water => "Water need",
                        Bulk::Hydrozone => "Hydrozone",
                        Bulk::Category => "Category",
                        Bulk::Container => "Container",
                        Bulk::Size => "Size",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut state.bulk, Bulk::Water, "Water need");
                        ui.selectable_value(&mut state.bulk, Bulk::Hydrozone, "Hydrozone");
                        ui.selectable_value(&mut state.bulk, Bulk::Category, "Category");
                        ui.selectable_value(&mut state.bulk, Bulk::Container, "Container");
                        ui.selectable_value(&mut state.bulk, Bulk::Size, "Size");
                    });
                match state.bulk {
                    Bulk::Water => {
                        egui::ComboBox::from_id_salt("plant_sheet_bulk_need")
                            .selected_text(state.bulk_need.id())
                            .show_ui(ui, |ui| {
                                for w in WaterNeed::ALL {
                                    ui.selectable_value(&mut state.bulk_need, w, w.id());
                                }
                            });
                    }
                    Bulk::Category => {
                        egui::ComboBox::from_id_salt("plant_sheet_bulk_cat")
                            .selected_text(state.bulk_category.id())
                            .show_ui(ui, |ui| {
                                for c in PlantCategory::ALL {
                                    ui.selectable_value(&mut state.bulk_category, c, c.id());
                                }
                            });
                    }
                    _ => {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.bulk_text).desired_width(90.0),
                        );
                    }
                }
                let shown: Vec<&Row> = rows.iter().filter(|r| matches(r, &state.filter)).collect();
                if ui
                    .button(format!("Apply to {} shown", shown.len()))
                    .on_hover_text("Change this field of every shown row. One undo step.")
                    .clicked()
                {
                    let cmds = bulk_commands(state, &shown);
                    if !cmds.is_empty() {
                        commands.push(Command::Batch { commands: cmds });
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                egui::Grid::new("plant_sheet_grid")
                    .striped(true)
                    .num_columns(COLS.len())
                    .show(ui, |ui| {
                        for c in COLS {
                            let arrow = if state.sort == c {
                                if state.ascending { " ^" } else { " v" }
                            } else {
                                ""
                            };
                            if ui
                                .button(format!("{}{arrow}", c.label()))
                                .on_hover_text("Sort by this column")
                                .clicked()
                            {
                                if state.sort == c {
                                    state.ascending = !state.ascending;
                                } else {
                                    state.sort = c;
                                    state.ascending = true;
                                }
                            }
                        }
                        ui.end_row();
                        for r in rows.iter().filter(|r| matches(r, &state.filter)) {
                            if let Some((name, s)) = edit_row(ui, r) {
                                commands.push(Command::ModifyResource {
                                    id: r.id,
                                    name: (name != r.name).then_some(name),
                                    body: Some(ResourceBody::plant_style(s)),
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
            if rows.is_empty() {
                ui.weak("No plant styles yet. Create one in the Resource Manager, or import a CSV list.");
            }
        });
    state.open = open;
    for c in commands {
        run_command(session, &c, status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session() -> Session {
        let mut s = Session::default();
        for (n, need) in [("Lavender", "low"), ("Hydrangea", "high")] {
            s.handle(
                "plant_style.create",
                json!({"name": n, "irrigation": {"water_need": need}}),
            )
            .unwrap();
        }
        s
    }

    fn draw(s: &mut Session, state: &mut PlantSheet) -> String {
        let ctx = egui::Context::default();
        let mut status = String::new();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            window(ui.ctx(), s, state, &mut status);
        });
        status
    }

    #[test]
    fn the_window_draws_closed_and_open() {
        let mut s = session();
        let mut state = PlantSheet::default();
        assert_eq!(draw(&mut s, &mut state), "");
        state.open = true;
        state.filter = "lav".into();
        assert_eq!(draw(&mut s, &mut state), "");
        assert!(state.open);
    }

    #[test]
    fn rows_sort_filter_and_bulk_edit() {
        let s = session();
        let mut state = PlantSheet::default();
        let rows = collect(&s, &state);
        assert_eq!(rows[0].name, "Hydrangea");
        state.ascending = false;
        assert_eq!(collect(&s, &state)[0].name, "Lavender");
        state.sort = Col::Water;
        state.ascending = true;
        assert_eq!(collect(&s, &state)[0].name, "Lavender");
        for c in COLS {
            let _ = sort_key(&rows[0], c);
            assert!(!c.label().is_empty());
        }
        assert!(matches(&rows[0], "HYDR"));
        assert!(!matches(&rows[0], "lavender"));
        assert!(matches(&rows[0], ""));
        // The bulk edit changes only the rows that differ.
        state.bulk = Bulk::Water;
        state.bulk_need = WaterNeed::High;
        let refs: Vec<&Row> = rows.iter().collect();
        assert_eq!(bulk_commands(&state, &refs).len(), 1);
        state.bulk = Bulk::Hydrozone;
        state.bulk_text = " Z4 ".into();
        assert_eq!(bulk_commands(&state, &refs).len(), 2);
        for b in [Bulk::Category, Bulk::Container, Bulk::Size] {
            state.bulk = b;
            state.bulk_text = "x".into();
            state.bulk_category = PlantCategory::Grass;
            assert!(!bulk_commands(&state, &refs).is_empty());
        }
    }

    #[test]
    fn a_batch_of_bulk_edits_is_one_undo_step() {
        let mut s = session();
        let state = PlantSheet {
            bulk: Bulk::Hydrozone,
            bulk_text: "Z7".into(),
            ..PlantSheet::default()
        };
        let rows = collect(&s, &state);
        let refs: Vec<&Row> = rows.iter().collect();
        let cmds = bulk_commands(&state, &refs);
        let mut status = String::new();
        run_command(&mut s, &Command::Batch { commands: cmds }, &mut status);
        assert!(
            s.editor
                .doc()
                .resources
                .plant_styles()
                .all(|(_, _, st)| st.irrigation.hydrozone == "Z7")
        );
        assert!(s.editor.undo());
        assert!(
            s.editor
                .doc()
                .resources
                .plant_styles()
                .all(|(_, _, st)| st.irrigation.hydrozone.is_empty())
        );
    }
}
