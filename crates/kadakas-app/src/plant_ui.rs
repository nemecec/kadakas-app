//! Panels for plants: the Resource Manager (plant styles), the plant schedule, the plant part of
//! Object Info, and the placement controls of the Plant tool. All changes are commands.

use egui::{DragValue, Ui};
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::model::{Color, Document, ObjectId, Shape};
use kadakas_core::plants::link;
use kadakas_core::plants::style::{
    PlantCategory, PlantForm, PlantGraphic, PlantStyle, QuantityType,
};
use kadakas_core::plants::tree::{self, DEFAULT_RPZ_FACTOR};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_plants::placement::Pattern;
use kadakas_plants::schedule;
use serde_json::{Map, Value, json};

use crate::canvas::color32;
use crate::panels::run_command;
use crate::tools::{By, PlantMode, ToolSettings};

// ---------------------------------------------------------------------------
// Tool bar

/// The spacing field, with the choice to use the spread of the plant as spacing.
fn spacing_row(ui: &mut Ui, ps: &mut crate::tools::PlantSettings, label: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add_enabled(!ps.spacing_is_spread, mm(&mut ps.spacing));
    });
    ui.checkbox(&mut ps.spacing_is_spread, "Spacing = spread")
        .on_hover_text("Plants are placed one canopy diameter apart");
}

fn mm(v: &mut f64) -> DragValue<'_> {
    DragValue::new(v)
        .speed(25.0)
        .range(10.0..=1.0e6)
        .suffix(" mm")
}

/// Placement controls of the Plant tool. Returns true if the mode changed.
pub fn plant_mode_bar(ui: &mut Ui, settings: &mut ToolSettings, doc: &Document) -> bool {
    let before = settings.plant.mode;
    ui.add_space(8.0);
    ui.label("Placement mode");
    ui.horizontal_wrapped(|ui| {
        for m in PlantMode::ALL {
            if ui
                .selectable_label(settings.plant.mode == m, m.label())
                .on_hover_text(m.help())
                .clicked()
            {
                settings.plant.mode = m;
            }
        }
    });
    let ps = &mut settings.plant;
    ui.label("Plant style");
    let name = ps
        .style
        .and_then(|id| doc.resources.get(id))
        .map_or("(none: use the Data bar)".to_string(), |r| r.name.clone());
    egui::ComboBox::from_id_salt("plant_tool_style")
        .selected_text(name)
        .width(150.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut ps.style, None, "(none: use the Data bar)");
            for (id, name, _) in doc.resources.plant_styles() {
                ui.selectable_value(&mut ps.style, Some(id), name);
            }
        });
    match ps.mode {
        PlantMode::Single => {
            ui.checkbox(&mut ps.existing_tree, "Existing (survey) tree")
                .on_hover_text("Place an existing tree with a root protection zone, not a plant");
            if ps.existing_tree {
                ui.horizontal(|ui| {
                    ui.label("Trunk diameter");
                    ui.add(
                        DragValue::new(&mut ps.tree_dbh)
                            .speed(10.0)
                            .range(10.0..=5000.0)
                            .suffix(" mm"),
                    );
                });
            }
        }
        PlantMode::Poly => {}
        PlantMode::Path | PlantMode::Fill => {
            let choices = ps.by_choices();
            if !choices.contains(&ps.by) {
                ps.by = choices[0];
            }
            egui::ComboBox::from_id_salt("plant_tool_by")
                .selected_text(ps.by.label())
                .show_ui(ui, |ui| {
                    for b in choices {
                        ui.selectable_value(&mut ps.by, *b, b.label());
                    }
                });
            if matches!(ps.by, By::Spacing | By::SpacingAndCount) {
                spacing_row(ui, ps, "Spacing");
            }
            if matches!(ps.by, By::Count | By::SpacingAndCount) {
                ui.horizontal(|ui| {
                    ui.label("Count");
                    ui.add(DragValue::new(&mut ps.count).speed(0.2).range(1..=20000));
                });
            }
            if ps.by == By::Density {
                ui.horizontal(|ui| {
                    ui.label("Density");
                    ui.add(
                        DragValue::new(&mut ps.density)
                            .speed(0.05)
                            .range(0.01..=400.0)
                            .suffix(" /m\u{b2}"),
                    );
                });
            }
        }
        PlantMode::Hedgerow => {
            spacing_row(ui, ps, "Spacing in a row");
            ui.horizontal(|ui| {
                ui.label("Rows");
                ui.add(DragValue::new(&mut ps.rows).speed(0.2).range(1..=20));
            });
            ui.horizontal(|ui| {
                ui.label("Row spacing");
                ui.add(mm(&mut ps.row_spacing));
            });
            ui.checkbox(&mut ps.stagger, "Stagger the rows");
        }
        PlantMode::Grid | PlantMode::Triangular => {
            spacing_row(
                ui,
                ps,
                if ps.mode == PlantMode::Grid {
                    "Spacing X"
                } else {
                    "Spacing"
                },
            );
            if ps.mode == PlantMode::Grid && !ps.spacing_is_spread {
                ui.horizontal(|ui| {
                    ui.label("Spacing Y");
                    ui.add(mm(&mut ps.spacing_y));
                });
            }
        }
    }
    if ps.mode == PlantMode::Fill {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut ps.pattern, Pattern::Triangular, "Staggered");
            ui.selectable_value(&mut ps.pattern, Pattern::Rectangular, "Grid");
        });
    }
    if matches!(
        ps.mode,
        PlantMode::Grid | PlantMode::Triangular | PlantMode::Fill
    ) {
        ui.horizontal(|ui| {
            ui.label("Edge margin");
            ui.add(
                DragValue::new(&mut ps.margin)
                    .speed(25.0)
                    .range(0.0..=1.0e6)
                    .suffix(" mm"),
            );
        });
    }
    ui.horizontal(|ui| {
        ui.label("Size variation");
        let mut percent = ps.size_variation * 100.0;
        if ui
            .add(
                DragValue::new(&mut percent)
                    .speed(1.0)
                    .range(0.0..=90.0)
                    .suffix(" %"),
            )
            .changed()
        {
            ps.size_variation = percent / 100.0;
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut ps.random_rotation, "Random rotation");
        ui.add(DragValue::new(&mut ps.seed).speed(1.0).prefix("seed "));
    });
    ui.add_enabled(
        ps.style.is_some(),
        egui::Checkbox::new(&mut ps.tag, "Add plant tag"),
    )
    .on_hover_text("A tag with the ID code and the count. Needs a plant style.");
    ps.mode != before
}

// ---------------------------------------------------------------------------
// Resource Manager

#[derive(Default)]
pub struct PlantUi {
    sheet: crate::plant_sheet_ui::PlantSheet,
    selected: Option<ResourceId>,
    buffer: Option<Buffer>,
    reassign: Option<ResourceId>,
    new_key: String,
}

struct Buffer {
    id: ResourceId,
    base_name: String,
    base: PlantStyle,
    name: String,
    style: PlantStyle,
}

fn unique_name(doc: &Document, base: &str) -> String {
    if doc.resources.find_plant_style(base).is_none() {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| doc.resources.find_plant_style(n).is_none())
        .expect("a free name")
}

fn used_by(doc: &Document, id: ResourceId) -> usize {
    doc.objects
        .values()
        .filter(|o| {
            link::link(o).is_some_and(|l| l.style == id)
                || matches!(o.shape, Shape::PlantTag { style, .. } if style == id)
        })
        .count()
}

fn color_row(ui: &mut Ui, label: &str, value: &mut Option<Color>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui
            .checkbox(&mut on, label)
            .on_hover_text("Off: use the class color")
            .changed()
        {
            *value = on.then_some(Color([90, 160, 80, 255]));
            changed = true;
        }
        if let Some(c) = value {
            let mut c32 = color32(*c);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                *c = Color(c32.to_srgba_unmultiplied());
                changed = true;
            }
        }
    });
    changed
}

/// The edit form of one plant style. Returns true when a field edit is finished.
fn style_form(ui: &mut Ui, st: &mut PlantStyle, name: &mut String, new_key: &mut String) -> bool {
    let mut commit = false;
    let mut text = |ui: &mut Ui, label: &str, s: &mut String| {
        ui.label(label);
        commit |= ui.text_edit_singleline(s).lost_focus();
        ui.end_row();
    };
    egui::Grid::new("plant_style_form")
        .num_columns(2)
        .show(ui, |ui| {
            text(ui, "Style name", name);
            text(ui, "Botanical name", &mut st.botanical_name);
            text(ui, "Common name", &mut st.common_name);
            text(ui, "ID code", &mut st.id_code);
            text(ui, "Size", &mut st.size);
            text(ui, "Container", &mut st.container);
            text(ui, "Root", &mut st.root);
        });
    egui::Grid::new("plant_style_numbers")
        .num_columns(2)
        .show(ui, |ui| {
            for (label, v, speed, min) in [
                ("Spread (mm)", &mut st.spread, 50.0, 1.0),
                ("Height (mm)", &mut st.height, 50.0, 1.0),
                ("Price", &mut st.price, 0.5, 0.0),
            ] {
                ui.label(label);
                let r = ui.add(DragValue::new(v).speed(speed).range(min..=1.0e9));
                commit |= r.drag_stopped() || r.lost_focus();
                ui.end_row();
            }
            ui.label("Category");
            let before = st.category;
            egui::ComboBox::from_id_salt("plant_category")
                .selected_text(st.category.id())
                .show_ui(ui, |ui| {
                    for c in PlantCategory::ALL {
                        ui.selectable_value(&mut st.category, c, c.id());
                    }
                });
            commit |= st.category != before;
            ui.end_row();
            ui.label("2D graphic");
            let before = st.graphic;
            egui::ComboBox::from_id_salt("plant_graphic")
                .selected_text(st.graphic.id())
                .show_ui(ui, |ui| {
                    for g in PlantGraphic::ALL {
                        ui.selectable_value(&mut st.graphic, g, g.id());
                    }
                });
            commit |= st.graphic != before;
            ui.end_row();
            ui.label("3D form");
            let before = st.form;
            egui::ComboBox::from_id_salt("plant_form")
                .selected_text(st.form.id())
                .show_ui(ui, |ui| {
                    for f in PlantForm::ALL {
                        ui.selectable_value(&mut st.form, f, f.id());
                    }
                });
            commit |= st.form != before;
            ui.end_row();
        });
    egui::Grid::new("plant_style_more")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Quantity");
            let before = st.quantity;
            egui::ComboBox::from_id_salt("plant_quantity")
                .selected_text(st.quantity.id())
                .show_ui(ui, |ui| {
                    for q in [QuantityType::Count, QuantityType::Area] {
                        ui.selectable_value(&mut st.quantity, q, q.id());
                    }
                });
            commit |= st.quantity != before;
            ui.end_row();
            ui.label("Trunk diameter (mm)");
            let r = ui.add(
                DragValue::new(&mut st.trunk_diameter)
                    .speed(10.0)
                    .range(0.0..=1.0e5),
            );
            commit |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
            ui.label("Tree protection zone");
            commit |= ui
                .checkbox(&mut st.protection_zone, "")
                .on_hover_text("Trees only. Radius is 12 x trunk diameter, at most 15 m.")
                .changed();
            ui.end_row();
            ui.label("Locked fields");
            let mut text = st.locked.join(", ");
            let r = ui
                .text_edit_singleline(&mut text)
                .on_hover_text("Fields that plants cannot override, for example: price, spread");
            if r.changed() {
                st.locked = text
                    .split(',')
                    .map(|f| f.trim().to_string())
                    .filter(|f| !f.is_empty())
                    .collect();
            }
            commit |= r.lost_focus();
            ui.end_row();
        });
    ui.label("Irrigation and care");
    egui::Grid::new("plant_style_care")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Water need");
            let before = st.irrigation.water_need;
            egui::ComboBox::from_id_salt("plant_water_need")
                .selected_text(st.irrigation.water_need.id())
                .show_ui(ui, |ui| {
                    for w in kadakas_core::plants::care::WaterNeed::ALL {
                        ui.selectable_value(&mut st.irrigation.water_need, w, w.id());
                    }
                });
            commit |= st.irrigation.water_need != before;
            ui.end_row();
            ui.label("Hydrozone");
            commit |= ui
                .text_edit_singleline(&mut st.irrigation.hydrozone)
                .on_hover_text("Z3 matches irrigation zone 3")
                .lost_focus();
            ui.end_row();
            ui.label("Irrigated");
            commit |= ui.checkbox(&mut st.irrigation.irrigated, "").changed();
            ui.end_row();
            ui.label("Maintenance interval");
            commit |= ui
                .text_edit_singleline(&mut st.maintenance.interval)
                .lost_focus();
            ui.end_row();
            ui.label("Pruning");
            commit |= ui
                .text_edit_singleline(&mut st.maintenance.pruning)
                .lost_focus();
            ui.end_row();
            ui.label("Hours per year");
            let r = ui.add(
                DragValue::new(&mut st.maintenance.hours_per_year)
                    .speed(0.1)
                    .range(0.0..=100_000.0),
            );
            commit |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
        });
    commit |= color_row(ui, "Fill color", &mut st.fill);
    commit |= color_row(ui, "Pen color", &mut st.pen);
    ui.label("Custom data");
    let mut remove = None;
    for (k, v) in st.custom.iter_mut() {
        ui.horizontal(|ui| {
            ui.label(k.as_str());
            commit |= ui.text_edit_singleline(v).lost_focus();
            if ui
                .small_button("x")
                .on_hover_text("Remove this field")
                .clicked()
            {
                remove = Some(k.clone());
            }
        });
    }
    if let Some(k) = remove {
        st.custom.remove(&k);
        commit = true;
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(new_key)
                .hint_text("new field")
                .desired_width(110.0),
        );
        if ui.small_button("Add").clicked() && !new_key.trim().is_empty() {
            st.custom.entry(new_key.trim().to_string()).or_default();
            new_key.clear();
            commit = true;
        }
    });
    commit
}

/// The Resource Manager palette: lists the plant styles, and creates, edits, duplicates and deletes them.
pub fn resource_manager(ui: &mut Ui, session: &mut Session, st: &mut PlantUi, status: &mut String) {
    ui.heading("Resource Manager");
    ui.separator();
    let mut commands: Vec<Command> = Vec::new();
    egui::CollapsingHeader::new("Plant styles")
        .default_open(true)
        .show(ui, |ui| {
            let doc = session.editor.doc();
            let rows: Vec<(ResourceId, String, String, usize)> = doc
                .resources
                .plant_styles()
                .map(|(id, name, s)| (id, name.to_string(), s.id_code.clone(), used_by(doc, id)))
                .collect();
            if st
                .selected
                .is_some_and(|id| doc.resources.get(id).is_none())
            {
                st.selected = None;
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("New")
                    .on_hover_text("Create a plant style")
                    .clicked()
                {
                    let name = unique_name(doc, "Plant style");
                    commands.push(Command::AddResource {
                        name,
                        body: ResourceBody::plant_style(PlantStyle {
                            botanical_name: "New plant".into(),
                            ..PlantStyle::default()
                        }),
                    });
                }
                if ui
                    .add_enabled(st.selected.is_some(), egui::Button::new("Duplicate"))
                    .clicked()
                    && let Some(id) = st.selected
                    && let (Some(r), Some(s)) =
                        (doc.resources.get(id), doc.resources.plant_style(id))
                {
                    commands.push(Command::AddResource {
                        name: unique_name(doc, &format!("{} copy", r.name)),
                        body: ResourceBody::plant_style(s.clone()),
                    });
                }
            });
            if ui
                .button("Spreadsheet view...")
                .on_hover_text("All plant styles in one table, for batch editing")
                .clicked()
            {
                st.sheet.open = true;
            }
            if rows.is_empty() {
                ui.weak("No plant styles yet. Create one, or import a CSV list.");
            }
            for (id, name, code, used) in &rows {
                let label = if code.is_empty() {
                    format!("{name}  ({used})")
                } else {
                    format!("{name}  [{code}]  ({used})")
                };
                if ui
                    .selectable_label(st.selected == Some(*id), label)
                    .on_hover_text("Plants and tags that use the style are in brackets")
                    .clicked()
                {
                    st.selected = Some(*id);
                }
            }
            if let Some(id) = st.selected {
                ui.separator();
                // Load the edit buffer when another style is selected or the style changed elsewhere (undo, an agent).
                let (name, style) = (
                    doc.resources
                        .get(id)
                        .map(|r| r.name.clone())
                        .unwrap_or_default(),
                    doc.resources.plant_style(id).cloned(),
                );
                if let Some(style) = style
                    && !st
                        .buffer
                        .as_ref()
                        .is_some_and(|b| b.id == id && b.base == style && b.base_name == name)
                {
                    st.buffer = Some(Buffer {
                        id,
                        base_name: name.clone(),
                        base: style.clone(),
                        name,
                        style,
                    });
                }
                if let Some(b) = st.buffer.as_mut().filter(|b| b.id == id) {
                    let commit = style_form(ui, &mut b.style, &mut b.name, &mut st.new_key);
                    if commit && (b.style != b.base || b.name != b.base_name) {
                        commands.push(Command::ModifyResource {
                            id,
                            name: (b.name != b.base_name).then(|| b.name.clone()),
                            body: Some(ResourceBody::plant_style(b.style.clone())),
                        });
                    }
                }
                ui.separator();
                let n = used_by(doc, id);
                if n > 0 {
                    ui.label(format!(
                        "In use by {n} object(s). Choose a style to take them over, then delete."
                    ));
                    let others: Vec<(ResourceId, &str)> = doc
                        .resources
                        .plant_styles()
                        .filter(|(o, _, _)| *o != id)
                        .map(|(o, n, _)| (o, n))
                        .collect();
                    let current = st
                        .reassign
                        .and_then(|r| doc.resources.get(r))
                        .map_or("(choose)".to_string(), |r| r.name.clone());
                    egui::ComboBox::from_id_salt("reassign_style")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (o, name) in others {
                                ui.selectable_value(&mut st.reassign, Some(o), name);
                            }
                        });
                }
                if ui.button("Delete style").clicked() {
                    commands.push(Command::DeleteResource {
                        id,
                        reassign: st.reassign.filter(|r| *r != id),
                    });
                }
            }
            ui.separator();
            #[cfg(not(target_arch = "wasm32"))]
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Import CSV...")
                    .on_hover_text(
                        "Create plant styles from a CSV list. Columns are matched by header.",
                    )
                    .clicked()
                {
                    *status = import_csv_file(session);
                }
                if ui.button("Export CSV...").clicked() {
                    *status = export_csv_file(session);
                }
            });
        });
    crate::plant_sheet_ui::window(ui.ctx(), session, &mut st.sheet, status);
    for c in commands {
        let out = run_command(session, &c, status);
        match (&c, out) {
            (Command::AddResource { .. }, Some(out)) => {
                st.selected = out.created.first().map(|n| ResourceId(*n))
            }
            (Command::DeleteResource { .. }, Some(_)) => {
                st.selected = None;
                st.buffer = None;
            }
            _ => {}
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn import_csv_file(session: &mut Session) -> String {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("CSV", &["csv", "txt"])
        .pick_file()
    else {
        return String::new();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return format!("Cannot read {}: {e}", path.display()),
    };
    match session.handle("plant_style.import_csv", json!({ "csv": text })) {
        Ok(v) => {
            let warnings = v["warnings"].as_array().map_or(0, Vec::len);
            format!(
                "Imported {} plant style(s), {} skipped, {warnings} warning(s)",
                v["created_count"],
                v["skipped"].as_array().map_or(0, Vec::len)
            )
        }
        Err(e) => format!("Import failed: {}", e.message),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn export_csv_file(session: &mut Session) -> String {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("CSV", &["csv"])
        .set_file_name("plant-styles.csv")
        .save_file()
    else {
        return String::new();
    };
    let csv = session
        .handle("plant_style.export_csv", json!({}))
        .ok()
        .and_then(|v| v["csv"].as_str().map(String::from))
        .unwrap_or_default();
    match kadakas_sys::fsio::write_atomic(&path, csv.as_bytes()) {
        Ok(()) => format!("Exported to {}", path.display()),
        Err(e) => format!("Cannot write {}: {e}", path.display()),
    }
}

// ---------------------------------------------------------------------------
// Plant schedule

/// The plant schedule palette: a live table with counts and prices.
pub fn plant_schedule(ui: &mut Ui, session: &mut Session, status: &mut String) {
    ui.heading("Plant schedule");
    ui.separator();
    let sch = schedule::schedule(session.editor.doc());
    if sch.rows.is_empty() {
        ui.weak("No plants in the drawing.");
        return;
    }
    egui::Grid::new("plant_schedule_grid")
        .striped(true)
        .show(ui, |ui| {
            for h in ["ID", "Name", "Size", "Count", "Price", "Total"] {
                ui.strong(h);
            }
            ui.end_row();
            for r in &sch.rows {
                ui.label(&r.id_code);
                ui.label(&r.name);
                ui.label(&r.size);
                ui.label(r.count.to_string());
                ui.label(format!("{:.2}", r.unit_price));
                ui.label(format!("{:.2}", r.total_price));
                ui.end_row();
            }
            ui.strong("");
            ui.strong("Total");
            ui.strong("");
            ui.strong(sch.total_count.to_string());
            ui.strong("");
            ui.strong(format!("{:.2}", sch.total_price));
            ui.end_row();
        });
    #[cfg(not(target_arch = "wasm32"))]
    if ui.button("Export CSV...").clicked() {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .set_file_name("plant-schedule.csv")
            .save_file()
        else {
            return;
        };
        *status =
            match kadakas_sys::fsio::write_atomic(&path, schedule::schedule_csv(&sch).as_bytes()) {
                Ok(()) => format!("Exported to {}", path.display()),
                Err(e) => format!("Cannot write {}: {e}", path.display()),
            };
    }
    #[cfg(target_arch = "wasm32")]
    let _ = status;
}

// ---------------------------------------------------------------------------
// Object Info

/// Fields of a plant that an instance can override, with their labels.
const OVERRIDES: [(&str, &str); 5] = [
    ("spread", "Spread"),
    ("height", "Height"),
    ("price", "Price"),
    ("botanical_name", "Botanical name"),
    ("id_code", "ID code"),
];

/// The plant style part of Object Info: the style, and which fields the plant overrides.
/// Stand the plant on the terrain height of the site model, or let it go back to its layer.
pub fn terrain_buttons(ui: &mut Ui, session: &mut Session, id: ObjectId, status: &mut String) {
    let has_ground = session
        .editor
        .doc()
        .object(id)
        .is_some_and(|o| link::ground_z(o).is_some());
    ui.horizontal(|ui| {
        if ui
            .button("Place on terrain")
            .on_hover_text("Stand the plant on the ground height of the site model")
            .clicked()
        {
            match session.handle("plants.drape", json!({"ids": [id]})) {
                Ok(v) if v["updated"] == 0 => {
                    *status = "No site model on the layer of this plant".into()
                }
                Ok(_) => {}
                Err(e) => *status = format!("Error: {}", e.message),
            }
        }
        if ui
            .add_enabled(has_ground, egui::Button::new("Release"))
            .on_hover_text("Back to the elevation of the layer")
            .clicked()
            && let Err(e) = session.handle("plants.drape", json!({"ids": [id], "release": true}))
        {
            *status = format!("Error: {}", e.message);
        }
    });
}

/// The format of a plant tag: how many levels it shows, and the ID bubble.
pub fn plant_tag_info(ui: &mut Ui, session: &mut Session, id: ObjectId, status: &mut String) {
    let Some(obj) = session.editor.doc().object(id) else {
        return;
    };
    let current = kadakas_core::plants::tag::TagFormat::of(obj);
    let mut f = current;
    ui.separator();
    ui.strong("Tag format");
    egui::ComboBox::from_id_salt(("plant_tag_levels", id))
        .selected_text(match f.levels {
            1 => "Level 1: ID and count",
            2 => "Level 2: plus plant name",
            _ => "Level 3: plus size and container",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut f.levels, 1, "Level 1: ID and count");
            ui.selectable_value(&mut f.levels, 2, "Level 2: plus plant name");
            ui.selectable_value(&mut f.levels, 3, "Level 3: plus size and container");
        });
    ui.checkbox(&mut f.bubble, "ID bubble");
    if f != current {
        let cmd = Command::SetData {
            id,
            set: [(
                kadakas_core::plants::tag::TAG_FORMAT_KEY.to_string(),
                json!({"levels": f.levels, "bubble": f.bubble}),
            )]
            .into(),
            remove: Vec::new(),
        };
        run_command(session, &cmd, status);
    }
}

pub fn plant_object_info(ui: &mut Ui, session: &mut Session, id: ObjectId, status: &mut String) {
    let doc = session.editor.doc();
    let Some(obj) = doc.object(id) else { return };
    let Some(eff) = link::effective(doc, obj) else {
        existing_tree_info(ui, &obj.shape);
        return;
    };
    let current = link::link(obj);
    let styles: Vec<(ResourceId, String)> = doc
        .resources
        .plant_styles()
        .map(|(i, n, _)| (i, n.to_string()))
        .collect();
    let mut chosen = eff.style_id;
    let selected_text = eff.style_id.map_or("(none: by instance)".to_string(), |_| {
        eff.style_name.clone()
    });
    ui.separator();
    ui.strong("Plant style");
    egui::ComboBox::from_id_salt(("plant_style_of", id))
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, "(none: by instance)");
            for (sid, name) in &styles {
                ui.selectable_value(&mut chosen, Some(*sid), name);
            }
        });
    let mut commands = Vec::new();
    if chosen != eff.style_id {
        commands.push(match chosen {
            Some(sid) => Command::SetPlantStyle {
                ids: vec![id],
                style: Some(sid),
                overrides: None,
                unlink: false,
            },
            None => Command::SetPlantStyle {
                ids: vec![id],
                style: None,
                overrides: None,
                unlink: true,
            },
        });
    }
    if let (Some(l), Some(_)) = (&current, eff.style_id) {
        let mut overrides: Map<String, Value> = l.overrides.clone();
        let mut changed = false;
        let effective = serde_json::to_value(&eff.style).unwrap_or_default();
        egui::Grid::new(("plant_overrides", id)).num_columns(2).show(ui, |ui| {
            for (key, label) in OVERRIDES {
                if doc.resources.plant_style(l.style).is_some_and(|s| s.is_locked(key)) {
                    ui.label(format!("{label} (locked)"));
                    ui.label(match &effective[key] {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    });
                    ui.end_row();
                    continue;
                }
                let mut over = overrides.contains_key(key);
                if ui.checkbox(&mut over, label).on_hover_text("Checked: this plant has its own value (by instance). Unchecked: the style gives it (by style).").changed() {
                    if over {
                        overrides.insert(key.into(), effective[key].clone());
                    } else {
                        overrides.remove(key);
                    }
                    changed = true;
                }
                match (over, &mut overrides.get_mut(key)) {
                    (true, Some(v)) if v.is_number() => {
                        let mut x = v.as_f64().unwrap_or(0.0);
                        let r = ui.add(DragValue::new(&mut x).speed(25.0).range(if key == "price" { 0.0 } else { 1.0 }..=1.0e9));
                        if r.drag_stopped() || r.lost_focus() {
                            **v = json!(x);
                            changed = true;
                        }
                    }
                    (true, Some(v)) => {
                        let mut s = v.as_str().unwrap_or("").to_string();
                        let r = ui.text_edit_singleline(&mut s);
                        if r.lost_focus() {
                            **v = json!(s);
                            changed = true;
                        }
                    }
                    _ => {
                        ui.label(match &effective[key] {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        });
                    }
                }
                ui.end_row();
            }
        });
        if changed && overrides != l.overrides {
            commands.push(Command::SetPlantStyle {
                ids: vec![id],
                style: None,
                overrides: Some(overrides),
                unlink: false,
            });
        }
    }
    for c in commands {
        run_command(session, &c, status);
    }
}

fn existing_tree_info(ui: &mut Ui, shape: &Shape) {
    if let Shape::ExistingTree {
        dbh,
        rpz_factor,
        rpz_cap,
        ..
    } = shape
    {
        ui.separator();
        let r = tree::rpz_radius(*dbh, *rpz_factor, *rpz_cap);
        ui.label(format!(
            "Root protection radius {:.0} mm ({} x trunk diameter)",
            r,
            rpz_factor.unwrap_or(DEFAULT_RPZ_FACTOR)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::uid::AsUid;

    fn frame(f: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        let mut f = f;
        let _ = ctx.run_ui(Default::default(), |ui| f(ui));
    }

    fn with_styles() -> (Session, ResourceId) {
        let mut s = Session::default();
        let id = s
            .handle(
                "plant_style.create",
                json!({"name": "Maple", "id_code": "AC", "price": 10}),
            )
            .unwrap()["id"]
            .as_uid()
            .unwrap();
        s.handle(
            "plants.place",
            json!({"mode": "poly", "style": id, "points": [[0, 0], [100, 100]]}),
        )
        .unwrap();
        (s, ResourceId(id))
    }

    #[test]
    fn panels_draw_without_a_selection_and_with_one() {
        let (mut s, id) = with_styles();
        let mut st = PlantUi::default();
        let mut status = String::new();
        frame(|ui| resource_manager(ui, &mut s, &mut st, &mut status));
        st.selected = Some(id);
        frame(|ui| resource_manager(ui, &mut s, &mut st, &mut status));
        assert!(st.buffer.is_some());
        frame(|ui| plant_schedule(ui, &mut s, &mut status));
        let plant = *s.editor.doc().objects.keys().next().unwrap();
        frame(|ui| plant_object_info(ui, &mut s, plant, &mut status));
        let mut empty = Session::default();
        frame(|ui| plant_schedule(ui, &mut empty, &mut status));
        frame(|ui| resource_manager(ui, &mut empty, &mut PlantUi::default(), &mut status));
    }

    #[test]
    fn object_info_for_trees_and_missing_objects() {
        let mut s = Session::default();
        s.handle("execute", json!({"command": {"op": "add_object", "shape": {"kind": "existing_tree", "at": [0, 0], "dbh": 300, "spread": 5000, "height": 9000}}})).unwrap();
        let id = *s.editor.doc().objects.keys().next().unwrap();
        let mut status = String::new();
        frame(|ui| plant_object_info(ui, &mut s, id, &mut status));
        frame(|ui| plant_object_info(ui, &mut s, ObjectId::from_num(999), &mut status));
    }

    #[test]
    fn tool_bar_draws_every_mode() {
        let (s, id) = with_styles();
        let mut settings = ToolSettings::default();
        settings.plant.style = Some(id);
        for m in PlantMode::ALL {
            settings.plant.mode = m;
            frame(|ui| {
                plant_mode_bar(ui, &mut settings, s.editor.doc());
            });
        }
        settings.plant.mode = PlantMode::Fill;
        settings.plant.by = By::Density;
        frame(|ui| {
            plant_mode_bar(ui, &mut settings, s.editor.doc());
        });
    }

    #[test]
    fn names_and_usage() {
        let (s, id) = with_styles();
        let doc = s.editor.doc();
        assert_eq!(unique_name(doc, "Maple"), "Maple 2");
        assert_eq!(unique_name(doc, "Oak"), "Oak");
        assert_eq!(used_by(doc, id), 2);
    }

    #[test]
    fn style_form_commits_on_color_toggle() {
        let mut st = PlantStyle::default();
        let mut name = "X".to_string();
        let mut key = String::new();
        frame(|ui| {
            style_form(ui, &mut st, &mut name, &mut key);
        });
        let mut c = None;
        frame(|ui| {
            color_row(ui, "Fill", &mut c);
        });
    }
}
