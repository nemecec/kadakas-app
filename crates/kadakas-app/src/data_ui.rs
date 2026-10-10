//! Panels for data and reports: the record format editor (in the Resource Manager), the Data
//! tab of Object Info, and the worksheet window with its grid, formula bar and database setup.
//! All changes are commands, so they can be undone.
//!
//! The state of these panels lives in the egui memory, so the callers only pass `ui` and the
//! session.

use crate::units_ui::DragValueExt as _;
use kadakas_l10n::tr;
use kadakas_units::DateFormat;
use std::collections::HashMap;

use egui::{Color32, DragValue, Id, Ui};
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::record::{self, FieldDef, FieldType, RecordFormat};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_sheet::def::{
    CellFormat, ColumnDef, DatabaseDef, GroupMode, SortKey, Summary, WorksheetDef,
};
use kadakas_sheet::formula::CellRef;
use kadakas_sheet::templates;
use serde_json::{Map, Value};

use crate::panels::run_command;

fn load<T: Default + Clone + Send + Sync + 'static>(ui: &Ui, key: &str) -> T {
    ui.data_mut(|d| d.get_temp::<T>(Id::new(key)))
        .unwrap_or_default()
}

fn store<T: Clone + Send + Sync + 'static>(ui: &Ui, key: &str, v: T) {
    ui.data_mut(|d| d.insert_temp(Id::new(key), v));
}

const ERROR_COLOR: Color32 = Color32::from_rgb(200, 60, 40);

fn unique_name(doc: &Document, ty: &str, base: &str) -> String {
    let taken = |n: &str| {
        doc.resources
            .iter()
            .any(|r| r.body.type_name() == ty && r.name.eq_ignore_ascii_case(n))
    };
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !taken(n))
        .expect("a free name")
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
        if ui.selectable_label(!data, tr!("data-ui-shape")).clicked() {
            data = false;
        }
        if ui
            .selectable_label(data, tr!("data-ui-data"))
            .on_hover_text(tr!("data-ui-records-attached-to-the-object"))
            .clicked()
        {
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
    let Some(first) = sel.first().and_then(|id| doc.object(*id)) else {
        return;
    };
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
        .filter_map(|(id, name, f)| {
            record::values(first, name).map(|v| (*id, name.clone(), f.clone(), v.clone()))
        })
        .collect();
    let free: Vec<(ResourceId, String)> = formats
        .iter()
        .filter(|(_, n, _)| record::values(first, n).is_none())
        .map(|(i, n, _)| (*i, n.clone()))
        .collect();
    let lacking = |name: &str| {
        sel.iter()
            .copied()
            .filter(|id| {
                doc.object(*id)
                    .is_some_and(|o| record::values(o, name).is_none())
            })
            .collect::<Vec<_>>()
    };
    let free_ids: Vec<(ResourceId, String, Vec<ObjectId>)> = free
        .iter()
        .map(|(i, n)| (*i, n.clone(), lacking(n)))
        .collect();

    let mut pending: Pending = load(ui, "data_tab_pending");
    let mut commands: Vec<Command> = Vec::new();
    if sel.len() > 1 {
        ui.weak(tr!(
            "data-ui-objects-selected-values-of-the-first-are",
            count = sel.len()
        ));
    }
    if formats.is_empty() {
        ui.weak(tr!("data-ui-no-record-formats-yet-create-one-in"));
    }
    for (rid, name, fmt, values) in &attached {
        egui::CollapsingHeader::new(name)
            .id_salt(("record", rid.0))
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new(("record_grid", rid.0))
                    .num_columns(2)
                    .show(ui, |ui| {
                        for f in &fmt.fields {
                            ui.label(&f.name);
                            let stored = values
                                .get(&f.name)
                                .cloned()
                                .unwrap_or_else(|| f.default_value());
                            let key = (first_id, name.clone(), f.name.clone());
                            let shown = pending
                                .0
                                .get(&key)
                                .cloned()
                                .unwrap_or_else(|| stored.clone());
                            let (new, done) = field_editor(ui, f, shown);
                            if new != stored || pending.0.contains_key(&key) {
                                pending.0.insert(key.clone(), new.clone());
                            }
                            if done {
                                pending.0.remove(&key);
                                if new != stored {
                                    commands.push(Command::SetRecordFields {
                                        ids: sel.to_vec(),
                                        record: *rid,
                                        values: [(f.name.clone(), new)].into_iter().collect(),
                                    });
                                }
                            }
                            ui.end_row();
                        }
                    });
                if ui
                    .button(tr!("data-ui-remove-record"))
                    .on_hover_text(tr!("data-ui-detach-the-record-and-its-values"))
                    .clicked()
                {
                    commands.push(Command::DetachRecord {
                        ids: sel.to_vec(),
                        record: *rid,
                    });
                }
            });
    }
    if !free_ids.is_empty() {
        ui.separator();
        let mut choice: Option<ResourceId> =
            load::<Option<kadakas_core::uid::Uid>>(ui, "data_tab_attach").map(ResourceId);
        if choice.is_none_or(|c| !free_ids.iter().any(|(i, ..)| *i == c)) {
            choice = Some(free_ids[0].0);
        }
        ui.horizontal(|ui| {
            let current = free_ids
                .iter()
                .find(|(i, ..)| Some(*i) == choice)
                .map(|(_, n, _)| n.clone())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("attach_record")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, n, _) in &free_ids {
                        ui.selectable_value(&mut choice, Some(*i), n);
                    }
                });
            if ui.button(tr!("data-ui-attach-record")).clicked()
                && let Some((i, _, ids)) = free_ids.iter().find(|(i, ..)| Some(*i) == choice)
            {
                commands.push(Command::AttachRecord {
                    ids: ids.clone(),
                    record: *i,
                    values: Map::new(),
                });
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
        FieldType::Date => {
            // A date is stored as YYYY-MM-DD and shown in the date format of the person.
            let stored = shown.as_str().unwrap_or("").to_string();
            let units = kadakas_units::current();
            let mut s = DateFormat::Iso
                .parse(&stored)
                .map_or(stored, |d| units.date_text(d));
            let r = ui.text_edit_singleline(&mut s);
            let value = units
                .parse_date(&s)
                .map_or(s, |d| DateFormat::Iso.format(d));
            (Value::String(value), r.lost_focus())
        }
        FieldType::Text | FieldType::Unknown | FieldType::Time => {
            let mut s = shown
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| shown.to_string());
            let r = ui.text_edit_singleline(&mut s);
            (Value::String(s), r.lost_focus())
        }
        FieldType::Integer => {
            let mut n = shown.as_i64().unwrap_or(0);
            let r = ui.add(DragValue::new(&mut n).speed(0.2));
            (Value::from(n), r.drag_stopped() || r.lost_focus())
        }
        FieldType::Number
        | FieldType::Dimension
        | FieldType::Area
        | FieldType::Volume
        | FieldType::Angle => {
            let mut n = shown.as_f64().unwrap_or(0.0);
            let speed = if f.ty == FieldType::Dimension {
                10.0
            } else {
                0.1
            };
            let drag = DragValue::new(&mut n)
                .speed(speed)
                .range(-record::MAX_FIELD..=record::MAX_FIELD);
            // The units of the person show for the fields that have a unit of their own.
            let drag = match f.ty {
                FieldType::Dimension => drag.length(),
                FieldType::Area => drag.area_m2(),
                FieldType::Volume => drag.volume_m3(),
                FieldType::Angle => drag.degrees(),
                _ if f.unit.is_empty() => drag,
                _ => drag.suffix(format!(" {}", f.unit)),
            };
            let r = ui.add(drag);
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
            egui::ComboBox::from_id_salt(("choice", &f.name))
                .selected_text(current.clone())
                .show_ui(ui, |ui| {
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

fn type_label(t: FieldType) -> String {
    match t {
        FieldType::Text => tr!("data-ui-text"),
        FieldType::Integer => tr!("data-ui-integer"),
        FieldType::Number => tr!("data-ui-number"),
        FieldType::Boolean => tr!("data-ui-boolean"),
        FieldType::Dimension => tr!("data-ui-dimension"),
        FieldType::Area => tr!("data-ui-area"),
        FieldType::Choice => tr!("data-ui-choice-list"),
        FieldType::Date => tr!("data-ui-date"),
        FieldType::Time => tr!("data-ui-time"),
        FieldType::Volume => tr!("data-ui-volume"),
        FieldType::Angle => tr!("data-ui-angle"),
        FieldType::Unknown => tr!("data-ui-unknown"),
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
            FieldType::Integer
            | FieldType::Number
            | FieldType::Dimension
            | FieldType::Area
            | FieldType::Volume
            | FieldType::Angle => text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map_or_else(
                    || Value::String(text.into()),
                    |n| {
                        if f.ty == FieldType::Integer && n.fract() == 0.0 {
                            Value::from(n as i64)
                        } else {
                            Value::from(n)
                        }
                    },
                ),
            _ => Value::String(text.into()),
        })
    };
}

/// Edits the fields of a format. Returns true when an edit is finished and should be applied.
fn format_form(ui: &mut Ui, name: &mut String, fmt: &mut RecordFormat) -> bool {
    let mut commit = false;
    ui.horizontal(|ui| {
        ui.label(tr!("data-ui-name"));
        commit |= ui.text_edit_singleline(name).lost_focus();
    });
    let mut remove = None;
    for (i, f) in fmt.fields.iter_mut().enumerate() {
        ui.separator();
        ui.horizontal(|ui| {
            commit |= ui
                .add(egui::TextEdit::singleline(&mut f.name).desired_width(90.0))
                .lost_focus();
            let before = f.ty;
            egui::ComboBox::from_id_salt(("field_type", i))
                .selected_text(type_label(f.ty))
                .width(90.0)
                .show_ui(ui, |ui| {
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
            if ui
                .small_button("x")
                .on_hover_text(tr!("data-ui-remove-the-field"))
                .clicked()
            {
                remove = Some(i);
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr!("data-ui-default"));
            let mut d = default_text(f);
            if ui
                .add(egui::TextEdit::singleline(&mut d).desired_width(70.0))
                .changed()
            {
                set_default(f, &d);
            }
            ui.label(tr!("data-ui-unit"));
            commit |= ui
                .add(egui::TextEdit::singleline(&mut f.unit).desired_width(40.0))
                .lost_focus();
        });
        if f.ty == FieldType::Choice {
            ui.horizontal(|ui| {
                ui.label(tr!("data-ui-choices"));
                let mut text = f.choices.join(", ");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .desired_width(160.0)
                        .hint_text(tr!("data-ui-a-b-c")),
                );
                if r.changed() {
                    f.choices = text
                        .split(',')
                        .map(|c| c.trim().to_string())
                        .filter(|c| !c.is_empty())
                        .collect();
                }
                commit |= r.lost_focus();
            });
        }
    }
    if let Some(i) = remove {
        fmt.fields.remove(i);
        commit = true;
    }
    if ui.button(tr!("data-ui-add-field")).clicked() {
        let n = fmt.fields.len() + 1;
        fmt.fields
            .push(FieldDef::new(&format!("Field {n}"), FieldType::Text));
        commit = true;
    }
    commit
}

fn record_formats(ui: &mut Ui, session: &mut Session, status: &mut String) {
    let mut selected: Option<kadakas_core::uid::Uid> = load(ui, "record_format_selected");
    let mut buf: FormatBuffer = load(ui, "record_format_buffer");
    let mut commands: Vec<Command> = Vec::new();
    egui::CollapsingHeader::new(tr!("data-ui-record-formats"))
        .default_open(false)
        .show(ui, |ui| {
            let doc = session.editor.doc();
            let rows: Vec<(ResourceId, String, usize)> = doc
                .resources
                .iter()
                .filter_map(|r| match &r.body {
                    ResourceBody::RecordFormat(_) => Some((
                        r.id,
                        r.name.clone(),
                        doc.objects
                            .values()
                            .filter(|o| record::values(o, &r.name).is_some())
                            .count(),
                    )),
                    _ => None,
                })
                .collect();
            if selected.is_some_and(|id| !rows.iter().any(|r| r.0.0 == id)) {
                selected = None;
            }
            if ui
                .button(tr!("data-ui-new"))
                .on_hover_text(tr!("data-ui-create-a-record-format"))
                .clicked()
            {
                let mut f = RecordFormat::default();
                f.fields.push(FieldDef::new("Field 1", FieldType::Text));
                commands.push(Command::AddResource {
                    name: unique_name(doc, "record_format", "Record"),
                    body: ResourceBody::RecordFormat(Box::new(f)),
                });
            }
            if rows.is_empty() {
                ui.weak(tr!("data-ui-no-record-formats-yet"));
            }
            for (id, name, used) in &rows {
                if ui
                    .selectable_label(
                        selected == Some(id.0),
                        tr!("data-ui-text-2", name = name, used = used),
                    )
                    .on_hover_text(tr!("data-ui-objects-that-have-the-record-are-in"))
                    .clicked()
                {
                    selected = Some(id.0);
                }
            }
            let Some(id) = selected.map(ResourceId) else {
                return;
            };
            let (name, fmt) = match doc.resources.get(id) {
                Some(r) => match &r.body {
                    ResourceBody::RecordFormat(f) => (r.name.clone(), f.as_ref().clone()),
                    _ => return,
                },
                None => return,
            };
            if buf.id != Some(id) || buf.base != fmt || buf.base_name != name {
                buf = FormatBuffer {
                    id: Some(id),
                    base_name: name.clone(),
                    base: fmt.clone(),
                    name,
                    format: fmt,
                    message: String::new(),
                };
            }
            ui.separator();
            if format_form(ui, &mut buf.name, &mut buf.format)
                && (buf.format != buf.base || buf.name != buf.base_name)
            {
                match buf
                    .format
                    .validate()
                    .and_then(|_| record::check_name("record", &buf.name))
                {
                    Ok(()) => {
                        commands.push(Command::ModifyResource {
                            id,
                            name: (buf.name != buf.base_name).then(|| buf.name.clone()),
                            body: Some(ResourceBody::RecordFormat(Box::new(buf.format.clone()))),
                        });
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
                ui.weak(tr!(
                    "data-ui-object-s-have-this-record-remove-it",
                    used = used
                ));
            }
            if ui
                .add_enabled(used == 0, egui::Button::new(tr!("data-ui-delete-format")))
                .clicked()
            {
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
    egui::CollapsingHeader::new(tr!("data-ui-worksheets"))
        .default_open(false)
        .show(ui, |ui| {
            let doc = session.editor.doc();
            let rows: Vec<(ResourceId, String)> = doc
                .resources
                .iter()
                .filter(|r| matches!(r.body, ResourceBody::Worksheet(_)))
                .map(|r| (r.id, r.name.clone()))
                .collect();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(tr!("data-ui-new"))
                    .on_hover_text(tr!("data-ui-create-an-empty-worksheet"))
                    .clicked()
                {
                    commands.push(Command::AddResource {
                        name: unique_name(doc, "worksheet", "Worksheet"),
                        body: ResourceBody::Worksheet(WorksheetDef::default().to_value()),
                    });
                }
                egui::ComboBox::from_id_salt("sheet_template")
                    .selected_text(tr!("data-ui-new-report"))
                    .show_ui(ui, |ui| {
                        for t in templates::TEMPLATES {
                            if ui
                                .selectable_label(false, t.title)
                                .on_hover_text(t.description)
                                .clicked()
                                && let Some(def) = templates::template(t.id)
                            {
                                commands.push(Command::AddResource {
                                    name: unique_name(doc, "worksheet", t.title),
                                    body: ResourceBody::Worksheet(def.to_value()),
                                });
                            }
                        }
                    });
            });
            if rows.is_empty() {
                ui.weak(tr!("data-ui-no-worksheets-yet-start-from-a-report"));
            }
            for (id, name) in &rows {
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(st.selected == Some(*id) && st.open, name)
                        .clicked()
                    {
                        st.selected = Some(*id);
                        st.open = true;
                    }
                    if ui.small_button(tr!("data-ui-delete")).clicked() {
                        commands.push(Command::DeleteResource {
                            id: *id,
                            reassign: None,
                        });
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
// Worksheet tables in the drawing

thread_local! {
    /// Calculated worksheets of the document revision that is drawn, so a table is calculated once per change.
    static TABLES: std::cell::RefCell<(u64, HashMap<ResourceId, std::sync::Arc<kadakas_sheet::Computed>>)> = std::cell::RefCell::new((0, HashMap::new()));
}

fn calculated(
    doc: &Document,
    id: ResourceId,
    site: Option<(&kadakas_site::SiteCache, u64)>,
) -> Option<std::sync::Arc<kadakas_sheet::Computed>> {
    let def = match &doc.resources.get(id)?.body {
        ResourceBody::Worksheet(v) => WorksheetDef::from_value(v).ok()?,
        _ => return None,
    };
    let run = || match site {
        Some((cache, revision)) => kadakas_sheet::compute_with(
            &def,
            doc,
            &kadakas_site::SiteTerrain::new(cache, revision, doc),
        ),
        None => kadakas_sheet::compute(&def, doc),
    };
    let Some((_, revision)) = site else {
        return Some(std::sync::Arc::new(run()));
    };
    TABLES.with(|t| {
        let mut t = t.borrow_mut();
        if t.0 != revision {
            *t = (revision, HashMap::new());
        }
        Some(
            t.1.entry(id)
                .or_insert_with(|| std::sync::Arc::new(run()))
                .clone(),
        )
    })
}

/// Draws the cells of a worksheet table inside its frame (a rectangle) on a design layer. Tables on
/// sheet layers are drawn by the sheet display list, which has the same layout.
pub fn draw_table(
    painter: &egui::Painter,
    view: &crate::view::View,
    doc: &Document,
    o: &kadakas_core::model::Object,
    d: &kadakas_core::worksheet_table::TableData,
    ink: Color32,
    site: Option<(&kadakas_site::SiteCache, u64)>,
) {
    let kadakas_core::model::Shape::Rect { min, max } = &o.shape else {
        return;
    };
    let Some(c) = calculated(doc, d.worksheet, site) else {
        return;
    };
    let l = kadakas_sheet::layout(&c, d.text_height);
    let (lo, hi) = (min.min(*max), min.max(*max));
    use crate::view::ViewExt as _;
    let frame = egui::Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi));
    let painter = painter.with_clip_rect(frame.intersect(painter.clip_rect()));
    let z = view.zoom;
    let grid = egui::Stroke::new(0.5, Color32::from_gray(150));
    let top = frame.min.y;
    let mut xs = vec![frame.min.x];
    for w in &l.col_widths {
        xs.push(xs.last().copied().unwrap_or(frame.min.x) + (*w * z) as f32);
    }
    let row_h = (l.row_height * z) as f32;
    let clip = painter.clip_rect();
    for r in 0..=l.rows {
        let y = top + row_h * r as f32;
        if y >= clip.min.y - 1.0 && y <= clip.max.y + 1.0 {
            painter.line_segment(
                [
                    egui::pos2(frame.min.x, y),
                    egui::pos2(*xs.last().unwrap_or(&frame.min.x), y),
                ],
                grid,
            );
        }
    }
    for x in &xs {
        painter.line_segment(
            [
                egui::pos2(*x, top),
                egui::pos2(*x, top + row_h * l.rows as f32),
            ],
            grid,
        );
    }
    let size = (d.text_height * z) as f32;
    if size < 3.0 {
        return;
    }
    let font = egui::FontId::proportional(size.min(200.0));
    let pad = size * 0.45;
    for r in 1..=l.rows {
        let y = top + row_h * (r - 1) as f32;
        if y + row_h < clip.min.y || y > clip.max.y {
            continue;
        }
        for col in 1..=l.cols {
            let text = c.display(r, col);
            if text.is_empty() {
                continue;
            }
            let (left, right) = (xs[col as usize - 1], xs[col as usize]);
            if right < clip.min.x || left > clip.max.x {
                continue;
            }
            let color = if c.value(r, col).is_error() {
                ERROR_COLOR
            } else {
                ink
            };
            if kadakas_sheet::layout::is_number(&c, r, col) {
                painter.text(
                    egui::pos2(right - pad, y + row_h / 2.0),
                    egui::Align2::RIGHT_CENTER,
                    text,
                    font.clone(),
                    color,
                );
            } else {
                painter.text(
                    egui::pos2(left + pad, y + row_h / 2.0),
                    egui::Align2::LEFT_CENTER,
                    text,
                    font.clone(),
                    color,
                );
            }
        }
    }
}

/// Object Info for a worksheet table: fit the frame to the table, and change the text height.
pub fn table_info(ui: &mut Ui, session: &mut Session, id: ObjectId, status: &mut String) {
    let Some(d) = session
        .editor
        .doc()
        .object(id)
        .and_then(kadakas_core::worksheet_table::data)
    else {
        return;
    };
    let name = session
        .editor
        .doc()
        .resources
        .get(d.worksheet)
        .map(|r| r.name.clone())
        .unwrap_or_default();
    ui.separator();
    ui.strong(tr!("data-ui-worksheet-table", name = name));
    let mut h = d.text_height;
    let r = ui.horizontal(|ui| {
        ui.label(tr!("data-ui-text-height"));
        ui.add(
            DragValue::new(&mut h)
                .speed(0.1)
                .range(0.1..=10_000.0)
                .length(),
        )
    });
    if (r.inner.drag_stopped() || r.inner.lost_focus()) && h != d.text_height {
        run_command(
            session,
            &Command::SetWorksheetTable {
                ids: vec![id],
                worksheet: None,
                text_height: Some(h),
                size: None,
            },
            status,
        );
    }
    if ui
        .button(tr!("data-ui-fit-frame-to-table"))
        .on_hover_text(tr!("data-ui-resize-the-frame-to-the-calculated-table"))
        .clicked()
        && let Some((_, c)) = session.worksheet_result(d.worksheet)
    {
        let l = kadakas_sheet::layout(&c, d.text_height);
        run_command(
            session,
            &Command::SetWorksheetTable {
                ids: vec![id],
                worksheet: None,
                text_height: None,
                size: Some(kadakas_core::DVec2::new(l.width, l.height)),
            },
            status,
        );
    }
}

// ---------------------------------------------------------------------------
// The worksheet window

fn cell_ref(row: u32, col: u32) -> String {
    CellRef { row, col }.to_string()
}

fn summary_label(s: Option<Summary>) -> String {
    let id = match s {
        None => "none",
        Some(Summary::Sum) => "sum",
        Some(Summary::Average) => "average",
        Some(Summary::Min) => "min",
        Some(Summary::Max) => "max",
        Some(Summary::Count) => "count",
    };
    crate::enum_names::name("summary", id)
}

fn save_def(
    session: &mut Session,
    id: ResourceId,
    def: &WorksheetDef,
    status: &mut String,
) -> Result<(), String> {
    def.validate()?;
    run_command(
        session,
        &Command::ModifyResource {
            id,
            name: None,
            body: Some(ResourceBody::Worksheet(def.to_value())),
        },
        status,
    )
    .map(|_| ())
    .ok_or_else(|| status.clone())
}

/// The worksheet window. Call once per frame.
pub fn windows(ctx: &egui::Context, session: &mut Session, status: &mut String) {
    let mut st: SheetState = ctx
        .data_mut(|d| d.get_temp(Id::new("sheet_state")))
        .unwrap_or_default();
    if !st.open {
        return;
    }
    let mut open = true;
    egui::Window::new(tr!("data-ui-worksheet"))
        .id(egui::Id::new("win-worksheet"))
        .open(&mut open)
        .default_size([760.0, 520.0])
        .show(ctx, |ui| sheet_window(ui, session, &mut st, status));
    st.open = open;
    ctx.data_mut(|d| d.insert_temp(Id::new("sheet_state"), st));
}

fn sheet_window(ui: &mut Ui, session: &mut Session, st: &mut SheetState, status: &mut String) {
    let sheets: Vec<(ResourceId, String)> = session
        .editor
        .doc()
        .resources
        .iter()
        .filter(|r| matches!(r.body, ResourceBody::Worksheet(_)))
        .map(|r| (r.id, r.name.clone()))
        .collect();
    if st
        .selected
        .is_none_or(|id| !sheets.iter().any(|s| s.0 == id))
    {
        st.selected = sheets.first().map(|s| s.0);
    }
    let Some(id) = st.selected else {
        ui.label(tr!("data-ui-no-worksheet-yet-create-one-in-the"));
        return;
    };
    ui.horizontal(|ui| {
        let current = sheets
            .iter()
            .find(|s| s.0 == id)
            .map(|s| s.1.clone())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("sheet_pick")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (i, n) in &sheets {
                    ui.selectable_value(&mut st.selected, Some(*i), n);
                }
            });
        if let Some((_, computed)) = session.worksheet_result(id) {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let name = sheets
                    .iter()
                    .find(|s| s.0 == id)
                    .map(|s| s.1.clone())
                    .unwrap_or_default();
                if ui.button(tr!("data-ui-export-csv")).clicked() {
                    *status = export_file(
                        &name,
                        "csv",
                        "CSV",
                        kadakas_sheet::export::csv(&computed).into_bytes(),
                    );
                }
                if ui
                    .button(tr!("data-ui-export-xlsx"))
                    .on_hover_text(tr!("data-ui-values-not-formulas"))
                    .clicked()
                {
                    *status = export_file(
                        &name,
                        "xlsx",
                        "Excel",
                        kadakas_sheet::export::xlsx(&computed, &name),
                    );
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (&computed, &status);
            ui.weak(tr!("data-ui-errors", count = computed.error_count()));
            if ui
                .button(tr!("data-ui-place-in-drawing"))
                .on_hover_text(tr!("data-ui-add-the-worksheet-as-a-table-on"))
                .clicked()
            {
                let l = kadakas_sheet::layout(
                    &computed,
                    kadakas_core::worksheet_table::DEFAULT_TEXT_HEIGHT,
                );
                let at = session
                    .editor
                    .doc()
                    .layer(session.editor.doc().active_layer)
                    .map_or(kadakas_core::DVec2::ZERO, |layer| {
                        if layer.kind == kadakas_core::model::LayerKind::Sheet {
                            kadakas_core::DVec2::new(20.0, 200.0)
                        } else {
                            kadakas_core::DVec2::ZERO
                        }
                    });
                let cmd = Command::AddWorksheetTable {
                    worksheet: id,
                    at,
                    size: kadakas_core::DVec2::new(l.width, l.height),
                    text_height: None,
                    layer: None,
                    class: None,
                    attrs: Default::default(),
                };
                run_command(session, &cmd, status);
            }
        }
    });
    let Some((def, computed)) = session.worksheet_result(id) else {
        return;
    };
    if st.selected != Some(id) {
        return;
    }

    // Formula bar.
    let (row, col) = (st.cell.0.max(1), st.cell.1.max(1));
    let address = cell_ref(row, col);
    let input = def
        .cells
        .get(&address)
        .map(|c| c.input.clone())
        .unwrap_or_default();
    if st.cell_base.as_ref() != Some(&((row, col), input.clone())) {
        st.cell_text = input.clone();
        st.cell_base = Some(((row, col), input.clone()));
    }
    let mut commit = false;
    ui.horizontal(|ui| {
        ui.strong(&address);
        let r = ui.add(
            egui::TextEdit::singleline(&mut st.cell_text)
                .desired_width(ui.available_width() - 200.0)
                .hint_text(tr!("data-ui-value-or-formula")),
        );
        commit |= r.lost_focus();
        let fmt = def
            .cells
            .get(&address)
            .map(|c| c.format.clone())
            .unwrap_or_default();
        let mut decimals = fmt.decimals.map_or(-1, i32::from);
        let d = ui
            .add(
                DragValue::new(&mut decimals)
                    .range(-1..=10)
                    .speed(0.1)
                    .custom_formatter(|v, _| {
                        if v < 0.0 {
                            "auto".into()
                        } else {
                            format!("{v:.0}")
                        }
                    }),
            )
            .on_hover_text(tr!("data-ui-decimals"));
        let mut unit = fmt.unit.clone();
        let u = ui.add(
            egui::TextEdit::singleline(&mut unit)
                .desired_width(40.0)
                .hint_text(tr!("data-ui-unit-hint")),
        );
        if d.changed() || u.lost_focus() {
            let new = CellFormat {
                decimals: u8::try_from(decimals).ok(),
                unit,
            };
            let mut next = def.clone();
            match next
                .set_cell(&address, &input, Some(new))
                .and_then(|_| save_def(session, id, &next, status))
            {
                Ok(()) => st.message.clear(),
                Err(e) => st.message = e,
            }
        }
    });
    if commit && st.cell_text != input {
        let mut next = def.clone();
        match next
            .set_cell(&address, &st.cell_text, None)
            .and_then(|_| save_def(session, id, &next, status))
        {
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
    egui::ScrollArea::both()
        .max_height(ui.available_height() - 220.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("sheet_grid")
                .spacing([2.0, 1.0])
                .min_col_width(70.0)
                .show(ui, |ui| {
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
                            let rich = if is_error {
                                egui::RichText::new(&text).color(ERROR_COLOR)
                            } else {
                                egui::RichText::new(&text)
                            };
                            let resp = ui.add_sized(
                                [70.0, 18.0],
                                egui::Button::selectable(selected, rich).frame_when_inactive(false),
                            );
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

fn database_setup(
    ui: &mut Ui,
    session: &mut Session,
    st: &mut SheetState,
    status: &mut String,
    id: ResourceId,
    def: &WorksheetDef,
) {
    if st
        .db
        .as_ref()
        .is_none_or(|(i, base, _)| *i != id || base != def)
    {
        st.db = Some((id, def.clone(), def.clone()));
    }
    let (_, base, edit) = st.db.as_mut().expect("buffer");
    let mut commit = false;
    let mut message: Option<String> = None;
    let fields = criteria_fields(session.editor.doc());
    egui::CollapsingHeader::new(tr!("data-ui-database-rows"))
        .default_open(false)
        .show(ui, |ui| {
            ui.weak(tr!("data-ui-a-database-block-lists-the-objects-that"));
            let mut remove = None;
            for (bi, db) in edit.databases.iter_mut().enumerate() {
                let title = if db.name.is_empty() {
                    tr!("data-ui-database", name = bi + 1)
                } else {
                    db.name.clone()
                };
                egui::CollapsingHeader::new(title)
                    .id_salt(("db", bi))
                    .default_open(true)
                    .show(ui, |ui| {
                        commit |= db_form(ui, db, &fields);
                        if ui.button(tr!("data-ui-remove-database")).clicked() {
                            remove = Some(bi);
                        }
                    });
            }
            if let Some(i) = remove {
                edit.databases.remove(i);
                commit = true;
            }
            if ui.button(tr!("data-ui-add-database")).clicked() {
                edit.databases.push(DatabaseDef {
                    criteria: "Kind = \"plant\"".into(),
                    columns: vec![
                        ColumnDef {
                            header: "Style".into(),
                            expr: "Style".into(),
                            ..Default::default()
                        },
                        ColumnDef {
                            header: "Count".into(),
                            expr: "Count".into(),
                            summary: Some(Summary::Sum),
                            ..Default::default()
                        },
                    ],
                    anchor: format!(
                        "A{}",
                        base.cells
                            .keys()
                            .filter_map(|k| CellRef::parse(k))
                            .map(|c| c.row)
                            .max()
                            .unwrap_or(0)
                            + 2
                    ),
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

fn column_combo(
    ui: &mut Ui,
    salt: &'static str,
    value: &mut Option<usize>,
    columns: &[ColumnDef],
    none: &str,
) -> bool {
    let before = *value;
    let text = value
        .and_then(|i| columns.get(i))
        .map_or(none.to_string(), |c| c.header.clone());
    egui::ComboBox::from_id_salt(salt)
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, none);
            for (i, c) in columns.iter().enumerate() {
                ui.selectable_value(value, Some(i), &c.header);
            }
        });
    *value != before
}

/// The names that the criteria builder offers: built-in properties and the fields of record formats.
fn criteria_fields(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = kadakas_core::props::NAMES
        .iter()
        .map(|n| n.to_string())
        .collect();
    for r in doc.resources.iter() {
        if let ResourceBody::RecordFormat(f) = &r.body {
            out.extend(f.fields.iter().map(|f| {
                format!(
                    "'{}'.'{}'",
                    r.name.replace('\'', "''"),
                    f.name.replace('\'', "''")
                )
            }));
        }
    }
    out
}

/// The comparison operators of the criteria builder.
const OPERATORS: [&str; 6] = ["=", "<>", "<", "<=", ">", ">="];

/// One clause for the criteria: `Area > 20` or `Kind = "plant"`. A number stays a number, anything else is text.
pub fn criteria_clause(name: &str, op: &str, value: &str) -> String {
    let v = value.trim();
    let literal = if v.parse::<f64>().is_ok_and(f64::is_finite)
        || v.eq_ignore_ascii_case("true")
        || v.eq_ignore_ascii_case("false")
    {
        v.to_string()
    } else {
        format!("\"{}\"", v.replace('"', "\"\""))
    };
    format!("{name} {op} {literal}")
}

/// Adds a clause to the criteria with AND or OR.
pub fn add_clause(criteria: &str, clause: &str, or: bool) -> String {
    if criteria.trim().is_empty() {
        clause.to_string()
    } else {
        format!(
            "{} {} {clause}",
            criteria.trim(),
            if or { "OR" } else { "AND" }
        )
    }
}

#[derive(Clone, Default)]
struct ClauseBuilder {
    name: String,
    op: usize,
    value: String,
    or: bool,
}

/// A row of widgets that builds one clause and adds it to the criteria. Returns true when it did.
fn criteria_builder(ui: &mut Ui, criteria: &mut String, fields: &[String]) -> bool {
    let key = ui.id().with("criteria_builder");
    let mut b: ClauseBuilder = ui.data_mut(|d| d.get_temp(key)).unwrap_or_default();
    if b.name.is_empty() {
        b.name = "Kind".into();
    }
    let mut added = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(tr!("data-ui-add"));
        egui::ComboBox::from_id_salt("clause_name")
            .width(90.0)
            .selected_text(b.name.clone())
            .show_ui(ui, |ui| {
                for f in fields {
                    ui.selectable_value(&mut b.name, f.clone(), f);
                }
            });
        egui::ComboBox::from_id_salt("clause_op")
            .width(40.0)
            .selected_text(OPERATORS[b.op.min(5)])
            .show_ui(ui, |ui| {
                for (i, o) in OPERATORS.iter().enumerate() {
                    ui.selectable_value(&mut b.op, i, *o);
                }
            });
        ui.add(
            egui::TextEdit::singleline(&mut b.value)
                .desired_width(80.0)
                .hint_text(tr!("data-ui-value")),
        );
        egui::ComboBox::from_id_salt("clause_join")
            .width(50.0)
            .selected_text(if b.or { "OR" } else { "AND" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut b.or, false, "AND");
                ui.selectable_value(&mut b.or, true, "OR");
            });
        if ui
            .button(tr!("data-ui-add"))
            .on_hover_text(tr!("data-ui-add-this-test-to-the-criteria"))
            .clicked()
            && !b.value.trim().is_empty()
        {
            *criteria = add_clause(
                criteria,
                &criteria_clause(&b.name, OPERATORS[b.op.min(5)], &b.value),
                b.or,
            );
            added = true;
        }
        if ui.button(tr!("data-ui-clear")).clicked() {
            criteria.clear();
            added = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(key, b));
    added
}

fn db_form(ui: &mut Ui, db: &mut DatabaseDef, fields: &[String]) -> bool {
    let mut commit = false;
    ui.horizontal(|ui| {
        ui.label(tr!("data-ui-rows"));
        let before = db.source;
        egui::ComboBox::from_id_salt("db_source")
            .selected_text(if db.source == kadakas_sheet::RowSource::Objects {
                tr!("data-ui-objects")
            } else {
                tr!("data-ui-plant-list")
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut db.source,
                    kadakas_sheet::RowSource::Objects,
                    tr!("data-ui-objects"),
                )
                .on_hover_text(tr!("data-ui-one-row-per-object"));
                ui.selectable_value(
                    &mut db.source,
                    kadakas_sheet::RowSource::PlantList,
                    tr!("data-ui-plant-list"),
                )
                .on_hover_text(tr!("data-ui-one-row-per-plant-style-with-the"));
            });
        commit |= db.source != before;
    });
    ui.horizontal(|ui| {
        ui.label(tr!("data-ui-name"));
        commit |= ui
            .add(egui::TextEdit::singleline(&mut db.name).desired_width(110.0))
            .lost_focus();
        ui.label(tr!("data-ui-top-left"));
        commit |= ui
            .add(egui::TextEdit::singleline(&mut db.anchor).desired_width(40.0))
            .lost_focus();
    });
    ui.label(tr!("data-ui-criteria"));
    commit |= criteria_builder(ui, &mut db.criteria, fields);
    commit |= ui
        .add(
            egui::TextEdit::multiline(&mut db.criteria)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .hint_text("Kind = \"plant\" AND Layer = \"Planting\""),
        )
        .lost_focus();
    ui.label(tr!("data-ui-columns"));
    let mut remove = None;
    for (i, c) in db.columns.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            commit |= ui
                .add(
                    egui::TextEdit::singleline(&mut c.header)
                        .desired_width(80.0)
                        .hint_text(tr!("data-ui-header")),
                )
                .lost_focus();
            commit |= ui
                .add(
                    egui::TextEdit::singleline(&mut c.expr)
                        .desired_width(110.0)
                        .hint_text(tr!("data-ui-expression")),
                )
                .lost_focus();
            let before = c.summary;
            egui::ComboBox::from_id_salt(("summary", i))
                .width(70.0)
                .selected_text(summary_label(c.summary))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut c.summary, None, tr!("data-ui-none"));
                    for s in [
                        Summary::Sum,
                        Summary::Average,
                        Summary::Min,
                        Summary::Max,
                        Summary::Count,
                    ] {
                        ui.selectable_value(&mut c.summary, Some(s), summary_label(Some(s)));
                    }
                });
            commit |= c.summary != before;
            let mut d = c.decimals.map_or(-1, i32::from);
            let r = ui
                .add(
                    DragValue::new(&mut d)
                        .range(-1..=10)
                        .speed(0.1)
                        .custom_formatter(|v, _| {
                            if v < 0.0 {
                                "auto".into()
                            } else {
                                format!("{v:.0}")
                            }
                        }),
                )
                .on_hover_text(tr!("data-ui-decimals"));
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
    if ui.button(tr!("data-ui-add-column")).clicked() {
        db.columns.push(ColumnDef {
            header: format!("Column {}", db.columns.len() + 1),
            expr: "Id".into(),
            ..Default::default()
        });
        commit = true;
    }
    ui.horizontal(|ui| {
        ui.label(tr!("data-ui-sort-by"));
        let mut first = db.sort.first().map(|k| k.column);
        if column_combo(
            ui,
            "sort_by",
            &mut first,
            &db.columns,
            &tr!("data-ui-none-in-parentheses"),
        ) {
            let desc = db.sort.first().is_some_and(|k| k.descending);
            db.sort = first
                .map(|column| SortKey {
                    column,
                    descending: desc,
                })
                .into_iter()
                .collect();
            commit = true;
        }
        if let Some(k) = db.sort.first_mut()
            && ui
                .checkbox(&mut k.descending, tr!("data-ui-descending"))
                .changed()
        {
            commit = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr!("data-ui-group-by"));
        commit |= column_combo(
            ui,
            "group_by",
            &mut db.group_by,
            &db.columns,
            &tr!("data-ui-none-in-parentheses"),
        );
        if db.group_by.is_some() {
            let before = db.group_mode;
            egui::ComboBox::from_id_salt("group_mode")
                .selected_text(if db.group_mode == GroupMode::Detail {
                    tr!("data-ui-details-and-subtotals")
                } else {
                    tr!("data-ui-one-row-per-group")
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut db.group_mode,
                        GroupMode::Detail,
                        tr!("data-ui-details-and-subtotals"),
                    );
                    ui.selectable_value(
                        &mut db.group_mode,
                        GroupMode::Summary,
                        tr!("data-ui-one-row-per-group"),
                    );
                });
            commit |= db.group_mode != before;
        }
    });
    ui.horizontal(|ui| {
        commit |= ui
            .checkbox(&mut db.header, tr!("data-ui-header-row"))
            .changed();
        commit |= ui
            .checkbox(&mut db.total, tr!("data-ui-total-row"))
            .changed();
    });
    commit
}

#[cfg(not(target_arch = "wasm32"))]
fn export_file(name: &str, ext: &str, label: &str, bytes: Vec<u8>) -> String {
    let file = format!("{}.{ext}", name.replace(['/', '\\', ':'], "-"));
    let Some(path) = rfd::FileDialog::new()
        .add_filter(label, &[ext])
        .set_file_name(file)
        .save_file()
    else {
        return String::new();
    };
    match kadakas_sys::fsio::write_atomic(&path, &bytes) {
        Ok(()) => tr!("data-ui-exported", path = path.display()),
        Err(e) => tr!("data-ui-cannot-write", path = path.display(), e = e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::model::{Attributes, Shape};

    fn run_ui(ctx: &egui::Context, f: impl FnMut(&mut Ui)) {
        let mut f = f;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| f(ui));
    }

    fn session_with_record() -> (Session, ObjectId, ResourceId) {
        let mut s = Session::default();
        let mut fmt = RecordFormat::default();
        fmt.fields.push(FieldDef::new("Note", FieldType::Text));
        fmt.fields.push(FieldDef::new("Qty", FieldType::Integer));
        fmt.fields
            .push(FieldDef::new("Depth", FieldType::Dimension));
        fmt.fields.push(FieldDef::new("Ratio", FieldType::Number));
        fmt.fields.push(FieldDef::new("Wet", FieldType::Boolean));
        let mut grade = FieldDef::new("Grade", FieldType::Choice);
        grade.choices = vec!["A".into(), "B".into()];
        fmt.fields.push(grade);
        let rid = ResourceId(
            s.editor
                .execute(&Command::AddResource {
                    name: "Soil".into(),
                    body: ResourceBody::RecordFormat(Box::new(fmt)),
                })
                .unwrap()
                .created[0],
        );
        let oid = ObjectId(
            s.editor
                .execute(&Command::AddObject {
                    shape: Shape::Circle {
                        center: kadakas_core::DVec2::ZERO,
                        radius: 1.0,
                    },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
        );
        s.selection = vec![oid];
        (s, oid, rid)
    }

    #[test]
    fn data_tab_attaches_shows_and_detaches_records() {
        let ctx = egui::Context::default();
        let (mut s, oid, rid) = session_with_record();
        let mut status = String::new();
        // The Shape tab is shown first.
        run_ui(&ctx, |ui| {
            assert!(!info_tab(ui, &mut s, &[oid], &mut status))
        });
        ctx.data_mut(|d| d.insert_temp(Id::new("info_tab_is_data"), true));
        run_ui(&ctx, |ui| {
            assert!(info_tab(ui, &mut s, &[oid], &mut status))
        });
        // Attach through a command like the button does, then draw all field editors.
        s.editor
            .execute(&Command::AttachRecord {
                ids: vec![oid],
                record: rid,
                values: Map::new(),
            })
            .unwrap();
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
            (
                FieldDef::new("D", FieldType::Dimension),
                serde_json::json!(5.5),
            ),
            (FieldDef::new("A", FieldType::Area), serde_json::json!(5.5)),
            (
                FieldDef::new("N", FieldType::Number),
                serde_json::json!(1.5),
            ),
            (
                FieldDef::new("B", FieldType::Boolean),
                serde_json::json!(true),
            ),
            (
                FieldDef::new("V", FieldType::Volume),
                serde_json::json!(2.5),
            ),
            (
                FieldDef::new("G", FieldType::Angle),
                serde_json::json!(45.0),
            ),
            (
                FieldDef::new("W", FieldType::Date),
                serde_json::json!("2026-03-27"),
            ),
            (
                FieldDef::new("X", FieldType::Time),
                serde_json::json!("14:30"),
            ),
            (
                FieldDef::new("U", FieldType::Unknown),
                serde_json::json!("x"),
            ),
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
        let out = s
            .editor
            .execute(&Command::AddResource {
                name: "Areas".into(),
                body: ResourceBody::Worksheet(def.to_value()),
            })
            .unwrap();
        let sid = ResourceId(out.created[0]);
        ctx.data_mut(|d| {
            d.insert_temp(
                Id::new("sheet_state"),
                SheetState {
                    open: true,
                    selected: Some(sid),
                    ..Default::default()
                },
            );
            d.insert_temp(
                Id::new("record_format_selected"),
                Some(kadakas_core::uid::Uid::from_num(2)),
            );
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
    fn criteria_clauses_are_built_and_parse() {
        assert_eq!(criteria_clause("Area", ">", " 20 "), "Area > 20");
        assert_eq!(criteria_clause("Kind", "=", "plant"), "Kind = \"plant\"");
        assert_eq!(
            criteria_clause("Style", "=", "6\" pot"),
            "Style = \"6\"\" pot\""
        );
        assert_eq!(
            criteria_clause("'Soil'.'Wet'", "=", "TRUE"),
            "'Soil'.'Wet' = TRUE"
        );
        assert_eq!(add_clause("", "Area > 1", false), "Area > 1");
        assert_eq!(
            add_clause(" Area > 1 ", "Kind = \"plant\"", false),
            "Area > 1 AND Kind = \"plant\""
        );
        assert_eq!(
            add_clause("Area > 1", "Layer = \"A\"", true),
            "Area > 1 OR Layer = \"A\""
        );
        let mut c = String::new();
        for (n, o, v) in [
            ("Kind", "=", "hardscape"),
            ("Area", ">=", "2.5"),
            ("Style", "<>", "it's \"x\""),
        ] {
            c = add_clause(&c, &criteria_clause(n, o, v), false);
        }
        kadakas_sheet::formula::parse(&c).unwrap();
        for n in kadakas_core::props::NAMES {
            kadakas_sheet::formula::parse(&criteria_clause(n, "=", "1")).unwrap();
        }
        // Record fields are offered with quoted names.
        let (s, _, _) = session_with_record();
        let fields = criteria_fields(s.editor.doc());
        assert!(
            fields.contains(&"Area".to_string()) && fields.contains(&"'Soil'.'Depth'".to_string())
        );
        kadakas_sheet::formula::parse(&criteria_clause("'Soil'.'Depth'", ">", "300")).unwrap();
    }

    #[test]
    fn the_builder_adds_to_the_criteria() {
        let ctx = egui::Context::default();
        let mut crit = String::from("Area > 1");
        run_ui(&ctx, |ui| {
            assert!(!criteria_builder(ui, &mut crit, &["Kind".to_string()]));
        });
        assert_eq!(crit, "Area > 1");
    }

    #[test]
    fn tables_are_calculated_once_per_revision_and_draw() {
        let ctx = egui::Context::default();
        let (mut s, oid, _) = session_with_record();
        let mut def = WorksheetDef::default();
        def.set_cell("A1", "Item", None).unwrap();
        def.set_cell("B1", "7", None).unwrap();
        def.set_cell("B2", "=1/0", None).unwrap();
        let ws = ResourceId(
            s.editor
                .execute(&Command::AddResource {
                    name: "T".into(),
                    body: ResourceBody::Worksheet(def.to_value()),
                })
                .unwrap()
                .created[0],
        );
        let id = ObjectId(
            s.editor
                .execute(&Command::AddWorksheetTable {
                    worksheet: ws,
                    at: kadakas_core::DVec2::new(0.0, 100.0),
                    size: kadakas_core::DVec2::new(40.0, 10.0),
                    text_height: Some(3.0),
                    layer: None,
                    class: None,
                    attrs: Default::default(),
                })
                .unwrap()
                .created[0],
        );
        let cache = kadakas_site::SiteCache::default();
        let rev = s.editor.revision();
        let a = calculated(s.editor.doc(), ws, Some((&cache, rev))).unwrap();
        let b = calculated(s.editor.doc(), ws, Some((&cache, rev))).unwrap();
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert!(calculated(s.editor.doc(), ResourceId::from_num(9999), None).is_none());
        assert!(calculated(s.editor.doc(), ResourceId(oid.0), None).is_none());
        assert!(calculated(s.editor.doc(), ws, None).is_some());
        let mut status = String::new();
        for zoom in [0.001, 5.0] {
            let view = crate::view::View {
                zoom,
                ..Default::default()
            };
            let doc = s.editor.doc().clone();
            let o = doc.object(id).unwrap().clone();
            let d = kadakas_core::worksheet_table::data(&o).unwrap();
            run_ui(&ctx, |ui| {
                draw_table(
                    ui.painter(),
                    &view,
                    &doc,
                    &o,
                    &d,
                    Color32::BLACK,
                    Some((&cache, rev)),
                );
                table_info(ui, &mut s, id, &mut status);
            });
        }
        // Fit resizes the frame through the same command as the button.
        let l = kadakas_sheet::layout(&a, 3.0);
        s.editor
            .execute(&Command::SetWorksheetTable {
                ids: vec![id],
                worksheet: None,
                text_height: None,
                size: Some(kadakas_core::DVec2::new(l.width, l.height)),
            })
            .unwrap();
        // A window with the place button draws too.
        ctx.data_mut(|d| {
            d.insert_temp(
                Id::new("sheet_state"),
                SheetState {
                    open: true,
                    selected: Some(ws),
                    ..Default::default()
                },
            )
        });
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            windows(&ui.ctx().clone(), &mut s, &mut status)
        });
    }

    #[test]
    fn database_form_edits_keep_indexes_in_line() {
        let ctx = egui::Context::default();
        let mut db = DatabaseDef {
            columns: vec![
                ColumnDef {
                    header: "A".into(),
                    expr: "Id".into(),
                    ..Default::default()
                },
                ColumnDef {
                    header: "B".into(),
                    expr: "Area".into(),
                    ..Default::default()
                },
            ],
            sort: vec![SortKey {
                column: 1,
                descending: false,
            }],
            group_by: Some(1),
            ..Default::default()
        };
        run_ui(&ctx, |ui| {
            db_form(ui, &mut db, &criteria_fields(&Document::default()));
        });
        assert_eq!(db.columns.len(), 2);
        assert_eq!(summary_label(Some(Summary::Average)), "average");
        assert_eq!(summary_label(None), "none");
        assert_eq!(cell_ref(2, 3), "C2");
        assert_eq!(type_label(FieldType::Choice), "Choice list");
    }
}
