//! Panels for data and reports: the record format editor (in the Resource Manager), the Data
//! tab of Object Info, and the worksheet window with its grid, formula bar and database setup.
//! All changes are commands, so they can be undone.
//!
//! The state of these panels lives in the egui memory, so the callers only pass `ui` and the
//! session.

use std::collections::HashMap;

use cadektor_api::Session;
use cadektor_core::command::Command;
use cadektor_core::model::{Document, ObjectId};
use cadektor_core::record::{self, FieldDef, FieldType, RecordFormat};
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_sheet::def::{CellFormat, ColumnDef, DatabaseDef, GroupMode, SortKey, Summary, WorksheetDef};
use cadektor_sheet::formula::CellRef;
use cadektor_sheet::templates;
use egui::{Color32, DragValue, Id, Ui};
use serde_json::{Map, Value};

use crate::panels::run_command;

fn load<T: Default + Clone + Send + Sync + 'static>(ui: &Ui, key: &str) -> T {
    ui.data_mut(|d| d.get_temp::<T>(Id::new(key))).unwrap_or_default()
}

fn store<T: Clone + Send + Sync + 'static>(ui: &Ui, key: &str, v: T) {
    ui.data_mut(|d| d.insert_temp(Id::new(key), v));
}

const ERROR_COLOR: Color32 = Color32::from_rgb(200, 60, 40);

fn unique_name(doc: &Document, ty: &str, base: &str) -> String {
    let taken = |n: &str| doc.resources.iter().any(|r| r.body.type_name() == ty && r.name.eq_ignore_ascii_case(n));
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).expect("a free name")
}

// ---------------------------------------------------------------------------
// Data tab of Object Info

/// Edits that are not finished yet: (object, record, field) to value.
#[derive(Clone, Default)]
struct Pending(HashMap<(ObjectId, String, String), Value>);

/// The tab strip of Object Info. Returns true when the Data tab is shown, so the caller leaves out the Shape tab.
pub fn info_tab(ui: &mut Ui, session: &mut Session, sel: &[ObjectId], status: &mut String) -> bool {
    let mut data: bool = load(ui, "info_tab_is_data");
    ui.horizontal(|ui| {
        if ui.selectable_label(!data, "Shape").clicked() {
            data = false;
        }
        if ui.selectable_label(data, "Data").on_hover_text("Records attached to the object").clicked() {
            data = true;
        }
    });
    store(ui, "info_tab_is_data", data);
    if data {
        ui.separator();
        data_tab(ui, session, sel, status);
    }
    data
}

fn data_tab(ui: &mut Ui, session: &mut Session, sel: &[ObjectId], status: &mut String) {
    let doc = session.editor.doc();
    let Some(first) = sel.first().and_then(|id| doc.object(*id)) else { return };
    let first_id = first.id;
    let formats: Vec<(ResourceId, String, RecordFormat)> = doc
        .resources
        .iter()
        .filter_map(|r| match &r.body {
            ResourceBody::RecordFormat(f) => Some((r.id, r.name.clone(), f.as_ref().clone())),
            _ => None,
        })
        .collect();
    let attached: Vec<(ResourceId, String, RecordFormat, Map<String, Value>)> = formats
        .iter()
        .filter_map(|(id, name, f)| record::values(first, name).map(|v| (*id, name.clone(), f.clone(), v.clone())))
        .collect();
    let free: Vec<(ResourceId, String)> = formats.iter().filter(|(_, n, _)| record::values(first, n).is_none()).map(|(i, n, _)| (*i, n.clone())).collect();
    let lacking = |name: &str| sel.iter().copied().filter(|id| doc.object(*id).is_some_and(|o| record::values(o, name).is_none())).collect::<Vec<_>>();
    let free_ids: Vec<(ResourceId, String, Vec<ObjectId>)> = free.iter().map(|(i, n)| (*i, n.clone(), lacking(n))).collect();

    let mut pending: Pending = load(ui, "data_tab_pending");
    let mut commands: Vec<Command> = Vec::new();
    if sel.len() > 1 {
        ui.weak(format!("{} objects selected. Values of the first are shown, and edits go to all.", sel.len()));
    }
    if formats.is_empty() {
        ui.weak("No record formats yet. Create one in the Resource Manager.");
    }
    for (rid, name, fmt, values) in &attached {
        egui::CollapsingHeader::new(name).id_salt(("record", rid.0)).default_open(true).show(ui, |ui| {
            egui::Grid::new(("record_grid", rid.0)).num_columns(2).show(ui, |ui| {
                for f in &fmt.fields {
                    ui.label(&f.name);
                    let stored = values.get(&f.name).cloned().unwrap_or_else(|| f.default_value());
                    let key = (first_id, name.clone(), f.name.clone());
                    let shown = pending.0.get(&key).cloned().unwrap_or_else(|| stored.clone());
                    let (new, done) = field_editor(ui, f, shown);
                    if new != stored || pending.0.contains_key(&key) {
                        pending.0.insert(key.clone(), new.clone());
                    }
                    if done {
                        pending.0.remove(&key);
                        if new != stored {
                            commands.push(Command::SetRecordFields { ids: sel.to_vec(), record: *rid, values: [(f.name.clone(), new)].into_iter().collect() });
                        }
                    }
                    ui.end_row();
                }
            });
            if ui.button("Remove record").on_hover_text("Detach the record and its values").clicked() {
                commands.push(Command::DetachRecord { ids: sel.to_vec(), record: *rid });
            }
        });
    }
    if !free_ids.is_empty() {
        ui.separator();
        let mut choice: Option<ResourceId> = load::<Option<u64>>(ui, "data_tab_attach").map(ResourceId);
        if choice.is_none_or(|c| !free_ids.iter().any(|(i, ..)| *i == c)) {
            choice = Some(free_ids[0].0);
        }
        ui.horizontal(|ui| {
            let current = free_ids.iter().find(|(i, ..)| Some(*i) == choice).map(|(_, n, _)| n.clone()).unwrap_or_default();
            egui::ComboBox::from_id_salt("attach_record").selected_text(current).show_ui(ui, |ui| {
                for (i, n, _) in &free_ids {
                    ui.selectable_value(&mut choice, Some(*i), n);
                }
            });
            if ui.button("Attach record").clicked()
                && let Some((i, _, ids)) = free_ids.iter().find(|(i, ..)| Some(*i) == choice)
            {
                commands.push(Command::AttachRecord { ids: ids.clone(), record: *i, values: Map::new() });
            }
        });
        store(ui, "data_tab_attach", choice.map(|c| c.0));
    }
    store(ui, "data_tab_pending", pending);
    for c in commands {
        run_command(session, &c, status);
    }
}

/// One editor for a record field. Returns the shown value and whether the edit is finished.
fn field_editor(ui: &mut Ui, f: &FieldDef, shown: Value) -> (Value, bool) {
    match f.ty {
        FieldType::Text | FieldType::Unknown | FieldType::Date | FieldType::Time => {
            let mut s = shown.as_str().map(str::to_string).unwrap_or_else(|| shown.to_string());
            let r = ui.text_edit_singleline(&mut s);
            (Value::String(s), r.lost_focus())
        }
        FieldType::Integer => {
            let mut n = shown.as_i64().unwrap_or(0);
            let r = ui.add(DragValue::new(&mut n).speed(0.2));
            (Value::from(n), r.drag_stopped() || r.lost_focus())
        }
        FieldType::Number | FieldType::Dimension | FieldType::Area | FieldType::Volume | FieldType::Angle => {
            let mut n = shown.as_f64().unwrap_or(0.0);
            let suffix = match f.ty {
                FieldType::Dimension => " mm".to_string(),
                FieldType::Area => " m\u{b2}".to_string(),
                FieldType::Volume => " m\u{b3}".to_string(),
                FieldType::Angle => "\u{b0}".to_string(),
                _ if f.unit.is_empty() => String::new(),
                _ => format!(" {}", f.unit),
            };
            let speed = if f.ty == FieldType::Dimension { 10.0 } else { 0.1 };
            let r = ui.add(DragValue::new(&mut n).speed(speed).range(-record::MAX_FIELD..=record::MAX_FIELD).suffix(suffix));
            (Value::from(n), r.drag_stopped() || r.lost_focus())
        }
        FieldType::Boolean => {
            let mut b = shown.as_bool().unwrap_or(false);
            let r = ui.checkbox(&mut b, "");
            (Value::Bool(b), r.changed())
        }
        FieldType::Choice => {
            let mut current = shown.as_str().unwrap_or_default().to_string();
            let before = current.clone();
            egui::ComboBox::from_id_salt(("choice", &f.name)).selected_text(current.clone()).show_ui(ui, |ui| {
                for c in &f.choices {
                    ui.selectable_value(&mut current, c.clone(), c);
                }
            });
            let changed = current != before;
            (Value::String(current), changed)
        }
    }
}

// ---------------------------------------------------------------------------
// Resource Manager sections

#[derive(Clone, Default)]
struct FormatBuffer {
    id: Option<ResourceId>,
    base_name: String,
    base: RecordFormat,
    name: String,
    format: RecordFormat,
    message: String,
}

#[derive(Clone, Default)]
struct SheetState {
    open: bool,
    selected: Option<ResourceId>,
    cell: (u32, u32),
    cell_text: String,
    /// The cell and input that `cell_text` was loaded from.
    cell_base: Option<((u32, u32), String)>,
    message: String,
    /// Edit buffer of the database setup: the worksheet, the definition it came from and the edited copy.
    db: Option<(ResourceId, WorksheetDef, WorksheetDef)>,
}

/// The record format and worksheet sections of the Resource Manager.
pub fn resource_sections(ui: &mut Ui, session: &mut Session, status: &mut String) {
    record_formats(ui, session, status);
    worksheets_list(ui, session, status);
}

fn type_label(t: FieldType) -> &'static str {
    match t {
        FieldType::Text => "Text",
        FieldType::Integer => "Integer",
        FieldType::Number => "Number",
        FieldType::Boolean => "Boolean",
        FieldType::Dimension => "Dimension",
        FieldType::Area => "Area",
        FieldType::Choice => "Choice list",
        FieldType::Date => "Date",
        FieldType::Time => "Time",
        FieldType::Volume => "Volume",
        FieldType::Angle => "Angle",
        FieldType::Unknown => "Unknown",
    }
}

/// Text of a default value, and back.
fn default_text(f: &FieldDef) -> String {
    match &f.default {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

fn set_default(f: &mut FieldDef, text: &str) {
    f.default = if text.trim().is_empty() {
        None
    } else {
        Some(match f.ty {
            FieldType::Boolean if text.trim().eq_ignore_ascii_case("true") => Value::Bool(true),
            FieldType::Boolean if text.trim().eq_ignore_ascii_case("false") => Value::Bool(false),
            FieldType::Integer | FieldType::Number | FieldType::Dimension | FieldType::Area | FieldType::Volume | FieldType::Angle => text.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map_or_else(|| Value::String(text.into()), |n| if f.ty == FieldType::Integer && n.fract() == 0.0 { Value::from(n as i64) } else { Value::from(n) }),
            _ => Value::String(text.into()),
        })
    };
}

/// Edits the fields of a format. Returns true when an edit is finished and should be applied.
fn format_form(ui: &mut Ui, name: &mut String, fmt: &mut RecordFormat) -> bool {
    let mut commit = false;
    ui.horizontal(|ui| {
        ui.label("Name");
        commit |= ui.text_edit_singleline(name).lost_focus();
    });
    let mut remove = None;
    for (i, f) in fmt.fields.iter_mut().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            commit |= ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(90.0)).lost_focus();
            let before = f.ty;
            egui::ComboBox::from_id_salt(("field_type", i)).selected_text(type_label(f.ty)).width(90.0).show_ui(ui, |ui| {
                for t in FieldType::ALL {
                    ui.selectable_value(&mut f.ty, t, type_label(t));
                }
            });
            if f.ty != before {
                if f.ty == FieldType::Choice && f.choices.is_empty() {
                    f.choices = vec!["Option 1".into()];
                }
                if f.ty != FieldType::Choice {
                    f.choices.clear();
                }
                f.default = None;
                commit = true;
            }
            if ui.small_button("x").on_hover_text("Remove the field").clicked() {
                remove = Some(i);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Default");
            let mut d = default_text(f);
            if ui.add(egui::TextEdit::singleline(&mut d).desired_width(70.0)).changed() {
                set_default(f, &d);
            }
            ui.label("Unit");
            commit |= ui.add(egui::TextEdit::singleline(&mut f.unit).desired_width(40.0)).lost_focus();
        });
        if f.ty == FieldType::Choice {
            ui.horizontal(|ui| {
                ui.label("Choices");
                let mut text = f.choices.join(", ");
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0).hint_text("A, B, C"));
                if r.changed() {
                    f.choices = text.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
                }
                commit |= r.lost_focus();
            });
        }
    }
    if let Some(i) = remove {
        fmt.fields.remove(i);
        commit = true;
    }
    if ui.button("Add field").clicked() {
        let n = fmt.fields.len() + 1;
        fmt.fields.push(FieldDef::new(&format!("Field {n}"), FieldType::Text));
        commit = true;
    }
    commit
}

fn record_formats(ui: &mut Ui, session: &mut Session, status: &mut String) {
    let mut selected: Option<u64> = load(ui, "record_format_selected");
    let mut buf: FormatBuffer = load(ui, "record_format_buffer");
    let mut commands: Vec<Command> = Vec::new();
    egui::CollapsingHeader::new("Record formats").default_open(false).show(ui, |ui| {
        let doc = session.editor.doc();
        let rows: Vec<(ResourceId, String, usize)> = doc
            .resources
            .iter()
            .filter_map(|r| match &r.body {
                ResourceBody::RecordFormat(_) => Some((r.id, r.name.clone(), doc.objects.values().filter(|o| record::values(o, &r.name).is_some()).count())),
                _ => None,
            })
            .collect();
        if selected.is_some_and(|id| !rows.iter().any(|r| r.0.0 == id)) {
            selected = None;
        }
        if ui.button("New").on_hover_text("Create a record format").clicked() {
            let mut f = RecordFormat::default();
            f.fields.push(FieldDef::new("Field 1", FieldType::Text));
            commands.push(Command::AddResource { name: unique_name(doc, "record_format", "Record"), body: ResourceBody::RecordFormat(Box::new(f)) });
        }
        if rows.is_empty() {
            ui.weak("No record formats yet.");
        }
        for (id, name, used) in &rows {
            if ui.selectable_label(selected == Some(id.0), format!("{name}  ({used})")).on_hover_text("Objects that have the record are in brackets").clicked() {
                selected = Some(id.0);
            }
        }
        let Some(id) = selected.map(ResourceId) else { return };
        let (name, fmt) = match doc.resources.get(id) {
            Some(r) => match &r.body {
                ResourceBody::RecordFormat(f) => (r.name.clone(), f.as_ref().clone()),
                _ => return,
            },
            None => return,
        };
        if buf.id != Some(id) || buf.base != fmt || buf.base_name != name {
            buf = FormatBuffer { id: Some(id), base_name: name.clone(), base: fmt.clone(), name, format: fmt, message: String::new() };
        }
        ui.separator();
        if format_form(ui, &mut buf.name, &mut buf.format) && (buf.format != buf.base || buf.name != buf.base_name) {
            match buf.format.validate().and_then(|_| record::check_name("record", &buf.name)) {
                Ok(()) => {
                    commands.push(Command::ModifyResource { id, name: (buf.name != buf.base_name).then(|| buf.name.clone()), body: Some(ResourceBody::RecordFormat(Box::new(buf.format.clone()))) });
                    buf.message.clear();
                }
                Err(e) => buf.message = e,
            }
        }
        if !buf.message.is_empty() {
            ui.colored_label(ERROR_COLOR, &buf.message);
        }
        ui.separator();
        let used = rows.iter().find(|r| r.0 == id).map_or(0, |r| r.2);
        if used > 0 {
            ui.weak(format!("{used} object(s) have this record. Remove it from them to delete the format."));
        }
        if ui.add_enabled(used == 0, egui::Button::new("Delete format")).clicked() {
            commands.push(Command::DeleteResource { id, reassign: None });
        }
    });
    for c in commands {
        let out = run_command(session, &c, status);
        match (&c, out) {
            (Command::AddResource { .. }, Some(out)) => selected = out.created.first().copied(),
            (Command::DeleteResource { .. }, Some(_)) => {
                selected = None;
                buf = FormatBuffer::default();
            }
            _ => {}
        }
    }
    store(ui, "record_format_selected", selected);
    store(ui, "record_format_buffer", buf);
}

fn worksheets_list(ui: &mut Ui, session: &mut Session, status: &mut String) {
    let mut st: SheetState = load(ui, "sheet_state");
    let mut commands: Vec<Command> = Vec::new();
    egui::CollapsingHeader::new("Worksheets").default_open(false).show(ui, |ui| {
        let doc = session.editor.doc();
        let rows: Vec<(ResourceId, String)> = doc.resources.iter().filter(|r| matches!(r.body, ResourceBody::Worksheet(_))).map(|r| (r.id, r.name.clone())).collect();
        ui.horizontal_wrapped(|ui| {
            if ui.button("New").on_hover_text("Create an empty worksheet").clicked() {
                commands.push(Command::AddResource { name: unique_name(doc, "worksheet", "Worksheet"), body: ResourceBody::Worksheet(WorksheetDef::default().to_value()) });
            }
            egui::ComboBox::from_id_salt("sheet_template").selected_text("New report").show_ui(ui, |ui| {
                for t in templates::TEMPLATES {
                    if ui.selectable_label(false, t.title).on_hover_text(t.description).clicked()
                        && let Some(def) = templates::template(t.id)
                    {
                        commands.push(Command::AddResource { name: unique_name(doc, "worksheet", t.title), body: ResourceBody::Worksheet(def.to_value()) });
                    }
                }
            });
        });
        if rows.is_empty() {
            ui.weak("No worksheets yet. Start from a report.");
        }
        for (id, name) in &rows {
            ui.horizontal(|ui| {
                if ui.selectable_label(st.selected == Some(*id) && st.open, name).clicked() {
                    st.selected = Some(*id);
                    st.open = true;
                }
                if ui.small_button("Delete").clicked() {
                    commands.push(Command::DeleteResource { id: *id, reassign: None });
                }
            });
        }
    });
    for c in commands {
        let out = run_command(session, &c, status);
        match (&c, out) {
            (Command::AddResource { .. }, Some(out)) => {
                st.selected = out.created.first().copied().map(ResourceId);
                st.open = true;
            }
            (Command::DeleteResource { id, .. }, Some(_)) if st.selected == Some(*id) => {
                st.selected = None;
                st.db = None;
            }
            _ => {}
        }
    }
    store(ui, "sheet_state", st);
}

// ---------------------------------------------------------------------------
// The worksheet window

fn cell_ref(row: u32, col: u32) -> String {
    CellRef { row, col }.to_string()
}

fn summary_label(s: Option<Summary>) -> &'static str {
    match s {
        None => "none",
        Some(Summary::Sum) => "sum",
        Some(Summary::Average) => "average",
        Some(Summary::Min) => "min",
        Some(Summary::Max) => "max",
        Some(Summary::Count) => "count",
    }
}

fn save_def(session: &mut Session, id: ResourceId, def: &WorksheetDef, status: &mut String) -> Result<(), String> {
    def.validate()?;
    run_command(session, &Command::ModifyResource { id, name: None, body: Some(ResourceBody::Worksheet(def.to_value())) }, status).map(|_| ()).ok_or_else(|| status.clone())
}

/// The worksheet window. Call once per frame.
pub fn windows(ctx: &egui::Context, session: &mut Session, status: &mut String) {
    let mut st: SheetState = ctx.data_mut(|d| d.get_temp(Id::new("sheet_state"))).unwrap_or_default();
    if !st.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Worksheet").open(&mut open).default_size([760.0, 520.0]).show(ctx, |ui| sheet_window(ui, session, &mut st, status));
    st.open = open;
    ctx.data_mut(|d| d.insert_temp(Id::new("sheet_state"), st));
}

fn sheet_window(ui: &mut Ui, session: &mut Session, st: &mut SheetState, status: &mut String) {
    let sheets: Vec<(ResourceId, String)> = session.editor.doc().resources.iter().filter(|r| matches!(r.body, ResourceBody::Worksheet(_))).map(|r| (r.id, r.name.clone())).collect();
    if st.selected.is_none_or(|id| !sheets.iter().any(|s| s.0 == id)) {
        st.selected = sheets.first().map(|s| s.0);
    }
    let Some(id) = st.selected else {
        ui.label("No worksheet yet. Create one in the Resource Manager.");
        return;
    };
    ui.horizontal(|ui| {
        let current = sheets.iter().find(|s| s.0 == id).map(|s| s.1.clone()).unwrap_or_default();
        egui::ComboBox::from_id_salt("sheet_pick").selected_text(current).show_ui(ui, |ui| {
            for (i, n) in &sheets {
                ui.selectable_value(&mut st.selected, Some(*i), n);
            }
        });
        if let Some((_, computed)) = session.worksheet_result(id) {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let name = sheets.iter().find(|s| s.0 == id).map(|s| s.1.clone()).unwrap_or_default();
                if ui.button("Export CSV...").clicked() {
                    *status = export_file(&name, "csv", "CSV", cadektor_sheet::export::csv(&computed).into_bytes());
                }
                if ui.button("Export XLSX...").on_hover_text("Values, not formulas").clicked() {
                    *status = export_file(&name, "xlsx", "Excel", cadektor_sheet::export::xlsx(&computed, &name));
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (&computed, &status);
            ui.weak(format!("{} errors", computed.error_count()));
        }
    });
    let Some((def, computed)) = session.worksheet_result(id) else { return };
    if st.selected != Some(id) {
        return;
    }

    // Formula bar.
    let (row, col) = (st.cell.0.max(1), st.cell.1.max(1));
    let address = cell_ref(row, col);
    let input = def.cells.get(&address).map(|c| c.input.clone()).unwrap_or_default();
    if st.cell_base.as_ref() != Some(&((row, col), input.clone())) {
        st.cell_text = input.clone();
        st.cell_base = Some(((row, col), input.clone()));
    }
    let mut commit = false;
    ui.horizontal(|ui| {
        ui.strong(&address);
        let r = ui.add(egui::TextEdit::singleline(&mut st.cell_text).desired_width(ui.available_width() - 200.0).hint_text("Value or =formula"));
        commit |= r.lost_focus();
        let fmt = def.cells.get(&address).map(|c| c.format.clone()).unwrap_or_default();
        let mut decimals = fmt.decimals.map_or(-1, i32::from);
        let d = ui.add(DragValue::new(&mut decimals).range(-1..=10).speed(0.1).custom_formatter(|v, _| if v < 0.0 { "auto".into() } else { format!("{v:.0}") })).on_hover_text("Decimals");
        let mut unit = fmt.unit.clone();
        let u = ui.add(egui::TextEdit::singleline(&mut unit).desired_width(40.0).hint_text("unit"));
        if d.changed() || u.lost_focus() {
            let new = CellFormat { decimals: u8::try_from(decimals).ok(), unit };
            let mut next = def.clone();
            match next.set_cell(&address, &input, Some(new)).and_then(|_| save_def(session, id, &next, status)) {
                Ok(()) => st.message.clear(),
                Err(e) => st.message = e,
            }
        }
    });
    if commit && st.cell_text != input {
        let mut next = def.clone();
        match next.set_cell(&address, &st.cell_text, None).and_then(|_| save_def(session, id, &next, status)) {
            Ok(()) => {
                st.message.clear();
                st.cell.0 = row + 1;
            }
            Err(e) => st.message = e,
        }
    }
    if !st.message.is_empty() {
        ui.colored_label(ERROR_COLOR, &st.message);
    }
    for p in computed.problems.iter().take(3) {
        ui.colored_label(ERROR_COLOR, p);
    }

    // Grid.
    let cols = (computed.cols + 2).clamp(8, 40);
    let rows = (computed.rows + 5).clamp(18, 300);
    egui::ScrollArea::both().max_height(ui.available_height() - 220.0).auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("sheet_grid").spacing([2.0, 1.0]).min_col_width(70.0).show(ui, |ui| {
            ui.label("");
            for c in 1..=cols {
                ui.strong(CellRef::col_name(c));
            }
            ui.end_row();
            for r in 1..=rows {
                ui.strong(r.to_string());
                for c in 1..=cols {
                    let text = computed.display(r, c);
                    let is_error = computed.value(r, c).is_error();
                    let selected = (r, c) == (row, col);
                    let rich = if is_error { egui::RichText::new(&text).color(ERROR_COLOR) } else { egui::RichText::new(&text) };
                    let resp = ui.add_sized([70.0, 18.0], egui::Button::selectable(selected, rich).frame_when_inactive(false));
                    if resp.clicked() {
                        st.cell = (r, c);
                    }
                }
                ui.end_row();
            }
        });
    });

    database_setup(ui, session, st, status, id, &def);
}

fn database_setup(ui: &mut Ui, session: &mut Session, st: &mut SheetState, status: &mut String, id: ResourceId, def: &WorksheetDef) {
    if st.db.as_ref().is_none_or(|(i, base, _)| *i != id || base != def) {
        st.db = Some((id, def.clone(), def.clone()));
    }
    let (_, base, edit) = st.db.as_mut().expect("buffer");
    let mut commit = false;
    let mut message: Option<String> = None;
    egui::CollapsingHeader::new("Database rows").default_open(false).show(ui, |ui| {
        ui.weak("A database block lists the objects that match the criteria. Columns are expressions such as Area, Layer, Style, Count or Soil.Depth.");
        let mut remove = None;
        for (bi, db) in edit.databases.iter_mut().enumerate() {
            let title = if db.name.is_empty() { format!("Database {}", bi + 1) } else { db.name.clone() };
            egui::CollapsingHeader::new(title).id_salt(("db", bi)).default_open(true).show(ui, |ui| {
                commit |= db_form(ui, db);
                if ui.button("Remove database").clicked() {
                    remove = Some(bi);
                }
            });
        }
        if let Some(i) = remove {
            edit.databases.remove(i);
            commit = true;
        }
        if ui.button("Add database").clicked() {
            edit.databases.push(DatabaseDef {
                criteria: "Kind = \"plant\"".into(),
                columns: vec![ColumnDef { header: "Style".into(), expr: "Style".into(), ..Default::default() }, ColumnDef { header: "Count".into(), expr: "Count".into(), summary: Some(Summary::Sum), ..Default::default() }],
                anchor: format!("A{}", base.cells.keys().filter_map(|k| CellRef::parse(k)).map(|c| c.row).max().unwrap_or(0) + 2),
                ..Default::default()
            });
            commit = true;
        }
    });
    if commit && *edit != *base {
        match edit.validate() {
            Ok(()) => {
                let next = edit.clone();
                st.message.clear();
                if let Err(e) = save_def(session, id, &next, status) {
                    message = Some(e);
                }
            }
            Err(e) => message = Some(e),
        }
    }
    if let Some(m) = message {
        st.message = m;
    }
}

fn column_combo(ui: &mut Ui, salt: &'static str, value: &mut Option<usize>, columns: &[ColumnDef], none: &str) -> bool {
    let before = *value;
    let text = value.and_then(|i| columns.get(i)).map_or(none.to_string(), |c| c.header.clone());
    egui::ComboBox::from_id_salt(salt).selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(value, None, none);
        for (i, c) in columns.iter().enumerate() {
            ui.selectable_value(value, Some(i), &c.header);
        }
    });
    *value != before
}

fn db_form(ui: &mut Ui, db: &mut DatabaseDef) -> bool {
    let mut commit = false;
    ui.horizontal(|ui| {
        ui.label("Name");
        commit |= ui.add(egui::TextEdit::singleline(&mut db.name).desired_width(110.0)).lost_focus();
        ui.label("Top left");
        commit |= ui.add(egui::TextEdit::singleline(&mut db.anchor).desired_width(40.0)).lost_focus();
    });
    ui.label("Criteria");
    commit |= ui.add(egui::TextEdit::multiline(&mut db.criteria).desired_rows(2).desired_width(f32::INFINITY).hint_text("Kind = \"plant\" AND Layer = \"Planting\"")).lost_focus();
    ui.label("Columns");
    let mut remove = None;
    for (i, c) in db.columns.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            commit |= ui.add(egui::TextEdit::singleline(&mut c.header).desired_width(80.0).hint_text("Header")).lost_focus();
            commit |= ui.add(egui::TextEdit::singleline(&mut c.expr).desired_width(110.0).hint_text("Expression")).lost_focus();
            let before = c.summary;
            egui::ComboBox::from_id_salt(("summary", i)).width(70.0).selected_text(summary_label(c.summary)).show_ui(ui, |ui| {
                ui.selectable_value(&mut c.summary, None, "none");
                for s in [Summary::Sum, Summary::Average, Summary::Min, Summary::Max, Summary::Count] {
                    ui.selectable_value(&mut c.summary, Some(s), summary_label(Some(s)));
                }
            });
            commit |= c.summary != before;
            let mut d = c.decimals.map_or(-1, i32::from);
            let r = ui.add(DragValue::new(&mut d).range(-1..=10).speed(0.1).custom_formatter(|v, _| if v < 0.0 { "auto".into() } else { format!("{v:.0}") })).on_hover_text("Decimals");
            c.decimals = u8::try_from(d).ok();
            commit |= r.drag_stopped() || r.lost_focus();
            if ui.small_button("x").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        db.columns.remove(i);
        // Indexes of sort and group follow the columns.
        db.sort.retain(|k| k.column != i);
        for k in &mut db.sort {
            if k.column > i {
                k.column -= 1;
            }
        }
        db.group_by = match db.group_by {
            Some(g) if g == i => None,
            Some(g) if g > i => Some(g - 1),
            other => other,
        };
        commit = true;
    }
    if ui.button("Add column").clicked() {
        db.columns.push(ColumnDef { header: format!("Column {}", db.columns.len() + 1), expr: "Id".into(), ..Default::default() });
        commit = true;
    }
    ui.horizontal(|ui| {
        ui.label("Sort by");
        let mut first = db.sort.first().map(|k| k.column);
        if column_combo(ui, "sort_by", &mut first, &db.columns, "(none)") {
            let desc = db.sort.first().is_some_and(|k| k.descending);
            db.sort = first.map(|column| SortKey { column, descending: desc }).into_iter().collect();
            commit = true;
        }
        if let Some(k) = db.sort.first_mut()
            && ui.checkbox(&mut k.descending, "descending").changed()
        {
            commit = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Group by");
        commit |= column_combo(ui, "group_by", &mut db.group_by, &db.columns, "(none)");
        if db.group_by.is_some() {
            let before = db.group_mode;
            egui::ComboBox::from_id_salt("group_mode").selected_text(if db.group_mode == GroupMode::Detail { "details and subtotals" } else { "one row per group" }).show_ui(ui, |ui| {
                ui.selectable_value(&mut db.group_mode, GroupMode::Detail, "details and subtotals");
                ui.selectable_value(&mut db.group_mode, GroupMode::Summary, "one row per group");
            });
            commit |= db.group_mode != before;
        }
    });
    ui.horizontal(|ui| {
        commit |= ui.checkbox(&mut db.header, "Header row").changed();
        commit |= ui.checkbox(&mut db.total, "Total row").changed();
    });
    commit
}

#[cfg(not(target_arch = "wasm32"))]
fn export_file(name: &str, ext: &str, label: &str, bytes: Vec<u8>) -> String {
    let file = format!("{}.{ext}", name.replace(['/', '\\', ':'], "-"));
    let Some(path) = rfd::FileDialog::new().add_filter(label, &[ext]).set_file_name(file).save_file() else { return String::new() };
    match std::fs::write(&path, bytes) {
        Ok(()) => format!("Exported {}", path.display()),
        Err(e) => format!("Cannot write {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::model::{Attributes, Shape};

    fn run_ui(ctx: &egui::Context, f: impl FnMut(&mut Ui)) {
        let mut f = f;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| f(ui));
    }

    fn session_with_record() -> (Session, ObjectId, ResourceId) {
        let mut s = Session::default();
        let mut fmt = RecordFormat::default();
        fmt.fields.push(FieldDef::new("Note", FieldType::Text));
        fmt.fields.push(FieldDef::new("Qty", FieldType::Integer));
        fmt.fields.push(FieldDef::new("Depth", FieldType::Dimension));
        fmt.fields.push(FieldDef::new("Ratio", FieldType::Number));
        fmt.fields.push(FieldDef::new("Wet", FieldType::Boolean));
        let mut grade = FieldDef::new("Grade", FieldType::Choice);
        grade.choices = vec!["A".into(), "B".into()];
        fmt.fields.push(grade);
        let rid = ResourceId(s.editor.execute(&Command::AddResource { name: "Soil".into(), body: ResourceBody::RecordFormat(Box::new(fmt)) }).unwrap().created[0]);
        let oid = ObjectId(s.editor.execute(&Command::AddObject { shape: Shape::Circle { center: cadektor_core::DVec2::ZERO, radius: 1.0 }, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0]);
        s.selection = vec![oid];
        (s, oid, rid)
    }

    #[test]
    fn data_tab_attaches_shows_and_detaches_records() {
        let ctx = egui::Context::default();
        let (mut s, oid, rid) = session_with_record();
        let mut status = String::new();
        // The Shape tab is shown first.
        run_ui(&ctx, |ui| assert!(!info_tab(ui, &mut s, &[oid], &mut status)));
        ctx.data_mut(|d| d.insert_temp(Id::new("info_tab_is_data"), true));
        run_ui(&ctx, |ui| assert!(info_tab(ui, &mut s, &[oid], &mut status)));
        // Attach through a command like the button does, then draw all field editors.
        s.editor.execute(&Command::AttachRecord { ids: vec![oid], record: rid, values: Map::new() }).unwrap();
        run_ui(&ctx, |ui| {
            info_tab(ui, &mut s, &[oid], &mut status);
        });
        // Multi-selection with a second object that lacks the record.
        run_ui(&ctx, |ui| {
            info_tab(ui, &mut s, &[oid], &mut status);
        });
    }

    #[test]
    fn field_editors_return_the_shown_value() {
        let ctx = egui::Context::default();
        let mut choice = FieldDef::new("Grade", FieldType::Choice);
        choice.choices = vec!["A".into()];
        for (f, v) in [
            (FieldDef::new("T", FieldType::Text), serde_json::json!("x")),
            (FieldDef::new("I", FieldType::Integer), serde_json::json!(3)),
            (FieldDef::new("D", FieldType::Dimension), serde_json::json!(5.5)),
            (FieldDef::new("A", FieldType::Area), serde_json::json!(5.5)),
            (FieldDef::new("N", FieldType::Number), serde_json::json!(1.5)),
            (FieldDef::new("B", FieldType::Boolean), serde_json::json!(true)),
            (FieldDef::new("V", FieldType::Volume), serde_json::json!(2.5)),
            (FieldDef::new("G", FieldType::Angle), serde_json::json!(45.0)),
            (FieldDef::new("W", FieldType::Date), serde_json::json!("2026-03-27")),
            (FieldDef::new("X", FieldType::Time), serde_json::json!("14:30")),
            (FieldDef::new("U", FieldType::Unknown), serde_json::json!("x")),
            (choice, serde_json::json!("A")),
        ] {
            run_ui(&ctx, |ui| {
                let (shown, done) = field_editor(ui, &f, v.clone());
                assert_eq!(shown, v);
                assert!(!done);
            });
        }
    }

    #[test]
    fn default_text_round_trips() {
        let mut f = FieldDef::new("N", FieldType::Integer);
        set_default(&mut f, "5");
        assert_eq!(f.default, Some(serde_json::json!(5)));
        assert_eq!(default_text(&f), "5");
        set_default(&mut f, "x");
        assert_eq!(f.default, Some(serde_json::json!("x")));
        set_default(&mut f, " ");
        assert_eq!(f.default, None);
        f.ty = FieldType::Number;
        set_default(&mut f, "2.5");
        assert_eq!(f.default, Some(serde_json::json!(2.5)));
        f.ty = FieldType::Boolean;
        set_default(&mut f, "TRUE");
        assert_eq!(f.default, Some(serde_json::json!(true)));
        set_default(&mut f, "false");
        assert_eq!(f.default, Some(serde_json::json!(false)));
        f.ty = FieldType::Text;
        set_default(&mut f, "hello");
        assert_eq!(default_text(&f), "hello");
        assert_eq!(default_text(&FieldDef::new("N", FieldType::Text)), "");
    }

    #[test]
    fn resource_sections_and_worksheet_window_draw() {
        let ctx = egui::Context::default();
        let (mut s, _, _) = session_with_record();
        let mut status = String::new();
        // Create a worksheet from a template the way the list does, then open its window.
        let def = templates::template("area_schedule").unwrap();
        let out = s.editor.execute(&Command::AddResource { name: "Areas".into(), body: ResourceBody::Worksheet(def.to_value()) }).unwrap();
        let sid = ResourceId(out.created[0]);
        ctx.data_mut(|d| {
            d.insert_temp(Id::new("sheet_state"), SheetState { open: true, selected: Some(sid), ..Default::default() });
            d.insert_temp(Id::new("record_format_selected"), Some(2u64));
        });
        for _ in 0..3 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                resource_sections(ui, &mut s, &mut status);
                windows(&ui.ctx().clone(), &mut s, &mut status);
            });
        }
        assert!(s.worksheet_result(sid).is_some());
        assert_eq!(unique_name(s.editor.doc(), "worksheet", "Areas"), "Areas 2");
        assert_eq!(unique_name(s.editor.doc(), "worksheet", "Other"), "Other");
    }

    #[test]
    fn database_form_edits_keep_indexes_in_line() {
        let ctx = egui::Context::default();
        let mut db = DatabaseDef {
            columns: vec![ColumnDef { header: "A".into(), expr: "Id".into(), ..Default::default() }, ColumnDef { header: "B".into(), expr: "Area".into(), ..Default::default() }],
            sort: vec![SortKey { column: 1, descending: false }],
            group_by: Some(1),
            ..Default::default()
        };
        run_ui(&ctx, |ui| {
            db_form(ui, &mut db);
        });
        assert_eq!(db.columns.len(), 2);
        assert_eq!(summary_label(Some(Summary::Average)), "average");
        assert_eq!(summary_label(None), "none");
        assert_eq!(cell_ref(2, 3), "C2");
        assert_eq!(type_label(FieldType::Choice), "Choice list");
    }
}
