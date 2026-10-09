//! Side panels, menu and data bar. Every document change goes through
//! `run_command`, which uses `Session::editor.execute`.

use cadektor_core::api::Session;
use cadektor_core::command::{Command, Outcome};
use cadektor_core::model::{Attributes, Color, ObjectId, Shape};
use egui::{DragValue, Ui};
use glam::DVec2;

use crate::canvas::color32;
use crate::databar::{self, DataEntry};
use crate::keymap::{Action, Keymap};
use crate::snap::{SnapConfig, SnapResult};
use crate::tools::{Tool, ToolMachine, ToolSettings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    New,
    Open,
    Save,
    SaveAs,
    ExportSvg,
    Undo,
    Redo,
    Delete,
    SelectAll,
    ZoomFit,
}

/// Runs a command on the session. Errors go to the status line.
pub fn run_command(session: &mut Session, cmd: &Command, status: &mut String) -> Option<Outcome> {
    match session.editor.execute(cmd) {
        Ok(out) => {
            let doc = &session.editor.doc;
            session.selection.retain(|id| doc.object(*id).is_some());
            Some(out)
        }
        Err(e) => {
            *status = format!("Error: {e}");
            None
        }
    }
}

pub fn menu_bar(ui: &mut Ui, keys: &Keymap, can_undo: bool, can_redo: bool, snap: &mut SnapConfig, show_grid: &mut bool) -> Vec<MenuAction> {
    use MenuAction::*;
    let mut out = Vec::new();
    egui::MenuBar::new().ui(ui, |ui| {
        let item = |ui: &mut Ui, label: &str, enabled: bool, a: MenuAction, out: &mut Vec<MenuAction>| {
            let button = egui::Button::new(label).shortcut_text(menu_key(a).map(|k| keys.hint(k)).unwrap_or_default());
            if ui.add_enabled(enabled, button).clicked() {
                out.push(a);
                ui.close();
            }
        };
        ui.menu_button("File", |ui| {
            item(ui, "New", true, New, &mut out);
            #[cfg(not(target_arch = "wasm32"))]
            {
                item(ui, "Open...", true, Open, &mut out);
                item(ui, "Save", true, Save, &mut out);
                item(ui, "Save As...", true, SaveAs, &mut out);
                item(ui, "Export SVG...", true, ExportSvg, &mut out);
            }
        });
        ui.menu_button("Edit", |ui| {
            item(ui, "Undo", can_undo, Undo, &mut out);
            item(ui, "Redo", can_redo, Redo, &mut out);
            ui.separator();
            item(ui, "Delete", true, Delete, &mut out);
            item(ui, "Select All", true, SelectAll, &mut out);
        });
        ui.menu_button("View", |ui| {
            item(ui, "Zoom to Fit", true, ZoomFit, &mut out);
            ui.separator();
            ui.checkbox(show_grid, "Show grid");
            ui.checkbox(&mut snap.grid, "Snap to grid");
            ui.checkbox(&mut snap.objects, "Snap to objects");
        });
    });
    out
}

/// The key action that does the same as a menu item.
pub fn menu_key(a: MenuAction) -> Option<Action> {
    Some(match a {
        MenuAction::New => Action::FileNew,
        MenuAction::Open => Action::FileOpen,
        MenuAction::Save => Action::FileSave,
        MenuAction::SaveAs => Action::FileSaveAs,
        MenuAction::Undo => Action::Undo,
        MenuAction::Redo => Action::Redo,
        MenuAction::Delete => Action::Delete,
        MenuAction::SelectAll => Action::SelectAll,
        MenuAction::ZoomFit => Action::ZoomFit,
        MenuAction::ExportSvg => return None,
    })
}

/// Button label and tooltip text of a tool, with its bound key.
pub fn tool_label(tool: Tool, keys: &Keymap) -> (String, String) {
    let key = keys.hint(Action::for_tool(tool));
    if key.is_empty() { (tool.name().to_string(), tool.name().to_string()) } else { (format!("{}  [{key}]", tool.name()), format!("{} ({key})", tool.name())) }
}

fn tool_button(ui: &mut Ui, m: &mut ToolMachine, keys: &Keymap, tool: Tool) {
    let (label, tip) = tool_label(tool, keys);
    let r = ui.selectable_label(m.tool() == tool, label).on_hover_text(tip);
    if r.clicked() {
        m.set_tool(tool);
    }
}

pub fn tool_panel(ui: &mut Ui, m: &mut ToolMachine, keys: &Keymap) {
    ui.heading("Tools");
    ui.separator();
    ui.label("Basic");
    for t in Tool::BASIC {
        tool_button(ui, m, keys, t);
    }
    ui.add_space(8.0);
    ui.label("Landscape");
    for t in Tool::LANDSCAPE {
        tool_button(ui, m, keys, t);
    }
}

/// Draws the Data bar. Returns true when Enter was pressed in an entry field.
pub fn data_bar(
    ui: &mut Ui,
    m: &ToolMachine,
    s: &mut ToolSettings,
    entry: &mut DataEntry,
    cursor: DVec2,
    snap: Option<SnapResult>,
    snap_cfg: SnapConfig,
) -> bool {
    let mut commit = false;
    ui.horizontal_wrapped(|ui| {
        ui.monospace(format!("X {:>10.1}  Y {:>10.1} mm", cursor.x, cursor.y));
        ui.separator();
        ui.label(format!("Tool: {}", m.tool().name()));
        ui.separator();
        match m.tool() {
            Tool::Text => {
                ui.label("Text");
                ui.add(egui::TextEdit::singleline(&mut s.text).desired_width(120.0));
                ui.label("Height");
                ui.add(DragValue::new(&mut s.text_height).speed(5.0).range(1.0..=100000.0));
            }
            Tool::Plant => {
                ui.label("Species");
                ui.add(egui::TextEdit::singleline(&mut s.species).desired_width(140.0));
                ui.label("Spread");
                ui.add(DragValue::new(&mut s.spread).speed(50.0).range(1.0..=100000.0));
                ui.label("Height");
                ui.add(DragValue::new(&mut s.plant_height).speed(50.0).range(0.0..=100000.0));
            }
            Tool::LandscapeArea => {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut s.area_name).desired_width(140.0));
            }
            Tool::Hardscape => {
                ui.label("Material");
                ui.add(egui::TextEdit::singleline(&mut s.material).desired_width(140.0));
            }
            _ => {}
        }
        let anchor = m.anchor();
        let fields = databar::fields(m.tool(), anchor.is_some());
        if !fields.is_empty() {
            let live = databar::live_values(m.tool(), anchor, cursor);
            let mut focused = false;
            let target = entry.want_focus.then_some(entry.focus);
            for (i, f) in fields.iter().enumerate() {
                ui.label(f.label());
                let Some(text) = entry.text.get_mut(i) else { continue };
                let hint = format!("{:.1}", live[i]);
                let r = ui.add(egui::TextEdit::singleline(text).id(databar::field_id(i)).desired_width(70.0).hint_text(hint));
                ui.label(f.unit());
                if target == Some(i) {
                    r.request_focus();
                    entry.want_focus = false;
                }
                if r.has_focus() {
                    focused = true;
                    if target.is_none() {
                        entry.focus = i;
                    }
                    // Keep Tab for the entry: it moves between fields instead of egui's focus order.
                    ui.memory_mut(|mem| {
                        mem.set_focus_lock_filter(r.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: false, escape: false })
                    });
                }
                if r.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                    commit = true;
                }
            }
            if !entry.want_focus {
                entry.active = focused || commit;
            }
        } else {
            entry.active = false;
        }
        let readout = m.readout(cursor);
        if !readout.is_empty() {
            ui.monospace(readout);
        }
        ui.separator();
        let snap_text = match (snap, snap_cfg.grid || snap_cfg.objects) {
            (Some(r), _) => format!("Snap: {}", r.kind.label()),
            (None, true) => "Snap: none".to_string(),
            (None, false) => "Snap: off".to_string(),
        };
        ui.label(snap_text);
    });
    commit
}

/// Edit buffer of the Object Info palette. Changes are committed when the
/// user finishes editing a field, so a drag is one undo step.
#[derive(Default)]
pub struct InfoEdit {
    id: ObjectId,
    base: Option<Shape>,
    edited: Option<Shape>,
}

struct Fields {
    commit: bool,
}

impl Fields {
    fn num(&mut self, ui: &mut Ui, v: &mut f64, speed: f64) {
        let r = ui.add(DragValue::new(v).speed(speed));
        self.commit |= r.drag_stopped() || r.lost_focus();
    }
    fn xy(&mut self, ui: &mut Ui, label: &str, p: &mut DVec2) {
        ui.label(label);
        ui.horizontal(|ui| {
            self.num(ui, &mut p.x, 10.0);
            self.num(ui, &mut p.y, 10.0);
        });
        ui.end_row();
    }
    fn scalar(&mut self, ui: &mut Ui, label: &str, v: &mut f64, speed: f64) {
        ui.label(label);
        self.num(ui, v, speed);
        ui.end_row();
    }
    fn text(&mut self, ui: &mut Ui, label: &str, s: &mut String) {
        ui.label(label);
        let r = ui.text_edit_singleline(s);
        self.commit |= r.lost_focus();
        ui.end_row();
    }
    fn angle(&mut self, ui: &mut Ui, label: &str, rad: &mut f64) {
        let mut deg = rad.to_degrees();
        ui.label(label);
        let r = ui.add(DragValue::new(&mut deg).speed(1.0).suffix("\u{b0}"));
        self.commit |= r.drag_stopped() || r.lost_focus();
        *rad = deg.to_radians();
        ui.end_row();
    }
    fn points(&mut self, ui: &mut Ui, pts: &mut [DVec2]) {
        ui.label("Points");
        ui.label(format!("{}", pts.len()));
        ui.end_row();
        for (i, p) in pts.iter_mut().enumerate().take(64) {
            self.xy(ui, &format!("  {}", i + 1), p);
        }
    }
}

fn edit_shape(ui: &mut Ui, shape: &mut Shape) -> bool {
    let mut f = Fields { commit: false };
    egui::Grid::new("shape_fields").num_columns(2).show(ui, |ui| match shape {
        Shape::Line { a, b } => {
            f.xy(ui, "Start", a);
            f.xy(ui, "End", b);
        }
        Shape::Rect { min, max } => {
            f.xy(ui, "Corner 1", min);
            f.xy(ui, "Corner 2", max);
        }
        Shape::Circle { center, radius } => {
            f.xy(ui, "Center", center);
            f.scalar(ui, "Radius", radius, 10.0);
        }
        Shape::Arc { center, radius, start, end } => {
            f.xy(ui, "Center", center);
            f.scalar(ui, "Radius", radius, 10.0);
            f.angle(ui, "Start", start);
            f.angle(ui, "End", end);
        }
        Shape::Polyline { points, closed } => {
            ui.label("Closed");
            f.commit |= ui.checkbox(closed, "").changed();
            ui.end_row();
            f.points(ui, points);
        }
        Shape::Text { at, text, height } => {
            f.xy(ui, "Position", at);
            f.text(ui, "Text", text);
            f.scalar(ui, "Height", height, 5.0);
        }
        Shape::Plant { at, species, spread, height } => {
            f.xy(ui, "Position", at);
            f.text(ui, "Species", species);
            f.scalar(ui, "Spread", spread, 50.0);
            f.scalar(ui, "Height", height, 50.0);
        }
        Shape::LandscapeArea { boundary, name } => {
            f.text(ui, "Name", name);
            f.points(ui, boundary);
        }
        Shape::Hardscape { boundary, material } => {
            f.text(ui, "Material", material);
            f.points(ui, boundary);
        }
    });
    f.commit
}

fn attr_row(ui: &mut Ui, label: &str, value: &mut Option<Color>, resolved: Option<Color>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut over = value.is_some();
        if ui.checkbox(&mut over, label).on_hover_text("Override the class attribute").changed() {
            *value = over.then(|| resolved.unwrap_or(Color::BLACK));
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

pub fn object_info(ui: &mut Ui, session: &mut Session, edit: &mut InfoEdit, status: &mut String) {
    ui.heading("Object Info");
    ui.separator();
    let sel = session.selection.clone();
    let Some(&id) = sel.first() else {
        let d = &session.editor.doc;
        let layer = d.layer(d.active_layer).map_or("?", |l| l.name.as_str());
        let class = d.class(d.active_class).map_or("?", |c| c.name.as_str());
        ui.label(format!("Nothing selected.\nActive layer: {layer}\nActive class: {class}"));
        *edit = InfoEdit::default();
        return;
    };
    if sel.len() > 1 {
        ui.label(format!("{} objects selected", sel.len()));
        *edit = InfoEdit::default();
        class_layer_combos(ui, session, &sel, status);
        return;
    }
    let Some(obj) = session.editor.doc.object(id).cloned() else { return };
    // Reset the buffer when another object is selected or the shape changed elsewhere.
    if edit.id != id || edit.base.as_ref() != Some(&obj.shape) {
        *edit = InfoEdit { id, base: Some(obj.shape.clone()), edited: Some(obj.shape.clone()) };
    }
    ui.label(format!("{}  (id {})", obj.shape.kind(), id));
    let edited = edit.edited.as_mut().expect("edit buffer");
    let commit = edit_shape(ui, edited);
    if commit && Some(&*edited) != edit.base.as_ref() {
        let cmd = Command::ModifyObject { id, shape: Some(edited.clone()), layer: None, class: None, attrs: None };
        if run_command(session, &cmd, status).is_none() {
            *edit = InfoEdit::default();
        }
    }
    if let Some(area) = obj.shape.area() {
        ui.label(format!("Area {:.2} m\u{b2}", area / 1e6));
    }
    ui.separator();
    class_layer_combos(ui, session, &sel, status);
    let doc = &session.editor.doc;
    let res = doc.resolved_attrs(&obj);
    let mut attrs: Attributes = obj.attrs.clone();
    let mut changed = attr_row(ui, "Pen", &mut attrs.pen, res.pen);
    changed |= attr_row(ui, "Fill", &mut attrs.fill, res.fill);
    ui.horizontal(|ui| {
        let mut over = attrs.line_weight.is_some();
        if ui.checkbox(&mut over, "Line weight").changed() {
            attrs.line_weight = over.then(|| res.line_weight.unwrap_or(0.25));
            changed = true;
        }
        if let Some(w) = &mut attrs.line_weight {
            let r = ui.add(DragValue::new(w).speed(0.05).range(0.01..=10.0).suffix(" mm"));
            changed |= r.drag_stopped() || r.lost_focus();
        }
    });
    if changed && attrs != obj.attrs {
        let cmd = Command::ModifyObject { id, shape: None, layer: None, class: None, attrs: Some(attrs) };
        run_command(session, &cmd, status);
    }
}

fn class_layer_combos(ui: &mut Ui, session: &mut Session, sel: &[ObjectId], status: &mut String) {
    let doc = &session.editor.doc;
    let first = doc.object(sel[0]).cloned();
    let Some(first) = first else { return };
    let layers: Vec<_> = doc.layers.iter().map(|l| (l.id, l.name.clone())).collect();
    let layer_names: Vec<_> = layers.iter().map(|(id, n)| (id.0, n.clone())).collect();
    let classes: Vec<_> = doc.classes.iter().map(|c| (c.id, c.name.clone())).collect();
    let class_names: Vec<_> = classes.iter().map(|(id, n)| (id.0, n.clone())).collect();
    let mut layer = first.layer;
    let mut class = first.class;
    let name_of = |v: &[(u64, String)], id: u64| v.iter().find(|x| x.0 == id).map(|x| x.1.clone()).unwrap_or_default();
    egui::ComboBox::from_label("Layer").selected_text(name_of(&layer_names, layer.0)).show_ui(ui, |ui| {
        for (id, n) in &layers {
            ui.selectable_value(&mut layer, *id, n);
        }
    });
    egui::ComboBox::from_label("Class").selected_text(name_of(&class_names, class.0)).show_ui(ui, |ui| {
        for (id, n) in &classes {
            ui.selectable_value(&mut class, *id, n);
        }
    });
    if layer != first.layer || class != first.class {
        let cmds = sel
            .iter()
            .map(|&id| Command::ModifyObject {
                id,
                shape: None,
                layer: (layer != first.layer).then_some(layer),
                class: (class != first.class).then_some(class),
                attrs: None,
            })
            .collect();
        run_command(session, &Command::Batch { commands: cmds }, status);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenameKind {
    Layer,
    Class,
}

#[derive(Default)]
pub struct NavState {
    renaming: Option<(RenameKind, u64, String)>,
}

pub fn navigation(ui: &mut Ui, session: &mut Session, nav: &mut NavState, status: &mut String) {
    ui.heading("Navigation");
    ui.separator();
    let mut cmds: Vec<Command> = Vec::new();
    let doc = &session.editor.doc;

    ui.horizontal(|ui| {
        ui.strong("Design Layers");
        if ui.small_button("+").on_hover_text("Add Design Layer").clicked() {
            cmds.push(Command::AddLayer { name: format!("Layer {}", doc.layers.len() + 1), elevation: 0.0, scale: None, kind: None });
        }
    });
    for l in &doc.layers {
        ui.horizontal(|ui| {
            let mut vis = l.visible;
            if ui.checkbox(&mut vis, "").on_hover_text("Visible").changed() {
                cmds.push(Command::ModifyLayer { id: l.id, name: None, visible: Some(vis), locked: None, elevation: None, scale: None });
            }
            let mut locked = l.locked;
            if ui.toggle_value(&mut locked, "Lock").on_hover_text("Lock layer").changed() {
                cmds.push(Command::ModifyLayer { id: l.id, name: None, visible: None, locked: Some(locked), elevation: None, scale: None });
            }
            if let Some((_, _, buf)) = nav.renaming.as_mut().filter(|r| r.0 == RenameKind::Layer && r.1 == l.id.0) {
                let r = ui.text_edit_singleline(buf);
                r.request_focus();
                if r.lost_focus() {
                    if !buf.is_empty() {
                        cmds.push(Command::ModifyLayer { id: l.id, name: Some(buf.clone()), visible: None, locked: None, elevation: None, scale: None });
                    }
                    nav.renaming = None;
                }
            } else {
                let r = ui.selectable_label(l.id == doc.active_layer, &l.name);
                if r.clicked() {
                    cmds.push(Command::SetActive { layer: Some(l.id), class: None });
                }
                if r.double_clicked() {
                    nav.renaming = Some((RenameKind::Layer, l.id.0, l.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        nav.renaming = Some((RenameKind::Layer, l.id.0, l.name.clone()));
                        ui.close();
                    }
                    if ui.button("Delete (if empty)").clicked() {
                        cmds.push(Command::DeleteLayer { id: l.id });
                        ui.close();
                    }
                });
            }
        });
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong("Classes");
        if ui.small_button("+").on_hover_text("Add Class").clicked() {
            cmds.push(Command::AddClass { name: format!("Class-{}", doc.classes.len()), attrs: Attributes::default() });
        }
    });
    for c in &doc.classes {
        ui.horizontal(|ui| {
            let mut vis = c.visible;
            if ui.checkbox(&mut vis, "").on_hover_text("Visible").changed() {
                cmds.push(Command::ModifyClass { id: c.id, name: None, visible: Some(vis), attrs: None });
            }
            if let Some((_, _, buf)) = nav.renaming.as_mut().filter(|r| r.0 == RenameKind::Class && r.1 == c.id.0) {
                let r = ui.text_edit_singleline(buf);
                r.request_focus();
                if r.lost_focus() {
                    if !buf.is_empty() {
                        cmds.push(Command::ModifyClass { id: c.id, name: Some(buf.clone()), visible: None, attrs: None });
                    }
                    nav.renaming = None;
                }
            } else {
                let r = ui.selectable_label(c.id == doc.active_class, &c.name);
                if r.clicked() {
                    cmds.push(Command::SetActive { layer: None, class: Some(c.id) });
                }
                if r.double_clicked() {
                    nav.renaming = Some((RenameKind::Class, c.id.0, c.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        nav.renaming = Some((RenameKind::Class, c.id.0, c.name.clone()));
                        ui.close();
                    }
                });
            }
        });
    }
    for c in cmds {
        run_command(session, &c, status);
    }
}
