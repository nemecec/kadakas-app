//! Side panels, menu and data bar. Every document change goes through
//! `run_command`, which uses `Session::editor.execute`.

use cadektor_api::Session;
use cadektor_core::command::{Command, Outcome};
use cadektor_core::model::{Attributes, Color, ObjectId, Shape, Vertex, VertexKind};
use cadektor_render::{Projection, RenderMode, StandardView, ViewSettings};
use egui::{DragValue, Ui};
use glam::DVec2;

use cadektor_core::plants::tree::{DEFAULT_RPZ_CAP, DEFAULT_RPZ_FACTOR, TreeAction, TreeCondition};

use crate::canvas::color32;
use crate::databar::{self, DataEntry};
use crate::keymap::{Action, Keymap};
use crate::snap::{SnapConfig, SnapResult};
use crate::tools::{DEFAULT_ARC_RADIUS, Tool, ToolMachine, ToolSettings, VertexMode, has_vertex_modes};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    New,
    Open,
    Save,
    SaveAs,
    ExportSvg,
    ExportPdf,
    Import,
    Export,
    Undo,
    Redo,
    Delete,
    SelectAll,
    ZoomFit,
    CreateSiteModel,
    UpdateSiteModel,
}

/// Runs a command on the session. Errors go to the status line.
pub fn run_command(session: &mut Session, cmd: &Command, status: &mut String) -> Option<Outcome> {
    match session.editor.execute(cmd) {
        Ok(out) => {
            let doc = &session.editor.doc();
            session.selection.retain(|id| doc.object(*id).is_some());
            Some(out)
        }
        Err(e) => {
            *status = format!("Error: {e}");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn menu_bar(ui: &mut Ui, keys: &Keymap, can_undo: bool, can_redo: bool, snap: &mut SnapConfig, show_grid: &mut bool, can_site: bool, can_update_site: bool) -> Vec<MenuAction> {
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
                item(ui, "Export PDF...", true, ExportPdf, &mut out);
                ui.separator();
                item(ui, "Import...", true, Import, &mut out);
                item(ui, "Export...", true, Export, &mut out);
            }
        });
        ui.menu_button("Edit", |ui| {
            item(ui, "Undo", can_undo, Undo, &mut out);
            item(ui, "Redo", can_redo, Redo, &mut out);
            ui.separator();
            item(ui, "Delete", true, Delete, &mut out);
            item(ui, "Select All", true, SelectAll, &mut out);
        });
        ui.menu_button("Site", |ui| {
            item(ui, "Create Site Model from Selection", can_site, CreateSiteModel, &mut out);
            item(ui, "Update Site Model", can_update_site, UpdateSiteModel, &mut out);
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

/// The key of a view, for a tooltip. Empty for a view without a key.
fn view_hint(keys: &Keymap, view: StandardView) -> String {
    Action::for_view(view).map(|a| keys.hint(a)).unwrap_or_default()
}

/// The view bar: standard view, projection and render mode. The projection and the render
/// mode apply to the 3D views only; Top/Plan is the 2D drawing.
pub fn view_bar(ui: &mut Ui, v: &mut ViewSettings, keys: &Keymap, gpu_ready: bool) {
    ui.horizontal(|ui| {
        ui.label("View");
        egui::ComboBox::from_id_salt("standard_view").selected_text(v.view.label()).show_ui(ui, |ui| {
            for s in StandardView::ALL {
                ui.selectable_value(&mut v.view, s, s.label()).on_hover_text(view_hint(keys, s));
            }
        });
        for s in [StandardView::Plan, StandardView::Front, StandardView::RightIsometric] {
            if ui.selectable_label(v.view == s, s.label()).on_hover_text(view_hint(keys, s)).clicked() {
                v.view = s;
            }
        }
        ui.separator();
        ui.add_enabled_ui(!v.view.is_plan() && gpu_ready, |ui| {
            ui.selectable_value(&mut v.projection, Projection::Orthographic, "Parallel");
            ui.selectable_value(&mut v.projection, Projection::Perspective, "Perspective")
                .on_hover_text(keys.hint(Action::ViewProjection));
            ui.separator();
            ui.selectable_value(&mut v.mode, RenderMode::Wireframe, "Wireframe")
                .on_hover_text(keys.hint(Action::ViewWireframe));
            ui.selectable_value(&mut v.mode, RenderMode::Shaded, "Shaded").on_hover_text(keys.hint(Action::ViewShaded));
        });
    });
}

/// The key action that does the same as a menu item.
pub fn menu_key(a: MenuAction) -> Option<Action> {
    Some(match a {
        MenuAction::New => Action::FileNew,
        MenuAction::Open => Action::FileOpen,
        MenuAction::Save => Action::FileSave,
        MenuAction::SaveAs => Action::FileSaveAs,
        MenuAction::ExportPdf => Action::FileExportPdf,
        MenuAction::Undo => Action::Undo,
        MenuAction::Redo => Action::Redo,
        MenuAction::Delete => Action::Delete,
        MenuAction::SelectAll => Action::SelectAll,
        MenuAction::ZoomFit => Action::ZoomFit,
        MenuAction::ExportSvg | MenuAction::Import | MenuAction::Export | MenuAction::CreateSiteModel | MenuAction::UpdateSiteModel => return None,
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

/// The vertex mode buttons and the default arc radius, for the tools that draw paths.
pub fn vertex_mode_bar(ui: &mut Ui, settings: &mut ToolSettings, keys: &Keymap) {
    ui.add_space(8.0);
    ui.label("Vertex mode");
    let key = keys.hint(Action::ToolMode1);
    let tip = if key.is_empty() { String::new() } else { format!(" ({key} cycles the modes)") };
    ui.horizontal_wrapped(|ui| {
        for mode in VertexMode::ALL {
            let r = ui.selectable_label(settings.vertex_mode == mode, mode.label());
            if r.on_hover_text(format!("{} vertex{tip}", mode.label())).clicked() {
                settings.vertex_mode = mode;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Arc radius");
        let r = ui.add(DragValue::new(&mut settings.arc_radius).speed(10.0).range(0.0..=1.0e7).suffix(" mm"));
        r.on_hover_text("Radius of new arc vertices. Type a value in the Data bar (Tab) in Arc mode.");
    });
}

pub fn tool_panel(ui: &mut Ui, m: &mut ToolMachine, keys: &Keymap, settings: &mut ToolSettings, doc: &cadektor_core::model::Document) {
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
    ui.add_space(8.0);
    ui.label("Site");
    for t in Tool::SITE {
        tool_button(ui, m, keys, t);
    }
    if has_vertex_modes(m.tool()) {
        vertex_mode_bar(ui, settings, keys);
    }
    if matches!(m.tool(), Tool::Hardscape | Tool::LandscapeArea) {
        crate::area_ui::tool_options(ui, m.tool(), settings, doc);
    }
    if m.tool() == Tool::Plant && crate::plant_ui::plant_mode_bar(ui, settings, doc) {
        // A new mode starts a new placement.
        m.cancel();
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
            Tool::SiteModifier => {
                for mode in crate::tools::ModifierMode::ALL {
                    ui.selectable_value(&mut s.modifier_mode, mode, mode.name());
                }
                if s.modifier_mode == crate::tools::ModifierMode::Pad {
                    ui.label("Elevation");
                    ui.add(DragValue::new(&mut s.pad_elevation).speed(10.0).range(-1.0e8..=1.0e8).suffix(" mm"));
                    let mut sloped = s.pad_end_elevation.is_some();
                    if ui.checkbox(&mut sloped, "Slope").changed() {
                        s.pad_end_elevation = sloped.then_some(s.pad_elevation);
                    }
                    if let Some(end) = &mut s.pad_end_elevation {
                        ui.label("Elevation at end");
                        ui.add(DragValue::new(end).speed(10.0).range(-1.0e8..=1.0e8).suffix(" mm"));
                        ui.label("Falls toward");
                        ui.add(DragValue::new(&mut s.pad_direction_deg).speed(1.0).suffix("\u{b0}"));
                    }
                }
            }
            _ => {}
        }
        let anchor = m.anchor();
        let arc = has_vertex_modes(m.tool()) && s.vertex_mode == VertexMode::Arc;
        let fields = databar::fields(m.tool(), anchor.is_some(), arc);
        if !fields.is_empty() {
            let live = databar::live_values(m.tool(), anchor, cursor, arc, s.arc_radius);
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
        let readout = m.readout(cursor, s);
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
    fn num_range(&mut self, ui: &mut Ui, v: &mut f64) {
        let r = ui.add(DragValue::new(v).speed(10.0).range(0.0..=1.0e9));
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
    fn count(&mut self, ui: &mut Ui, n: usize) {
        ui.label("Vertices");
        ui.label(format!("{n}"));
        ui.end_row();
    }
}

/// Names of the vertex kinds in the vertex table.
pub fn kind_label(kind: VertexKind) -> &'static str {
    match kind {
        VertexKind::Corner => "Corner",
        VertexKind::Arc { .. } => "Arc",
        VertexKind::PointOnArc => "Point on arc",
        VertexKind::Bezier => "B\u{e9}zier",
        VertexKind::Cubic => "Cubic",
    }
}

const KIND_CHOICES: [VertexKind; 5] = [
    VertexKind::Corner,
    VertexKind::Arc { radius: DEFAULT_ARC_RADIUS },
    VertexKind::PointOnArc,
    VertexKind::Bezier,
    VertexKind::Cubic,
];

/// The most vertex rows that the palette shows.
const MAX_VERTEX_ROWS: usize = 500;

/// The vertex list: index, X, Y, kind and radius. Returns true when an edit is finished.
fn vertex_table(ui: &mut Ui, id: ObjectId, vertices: &mut [Vertex]) -> bool {
    let mut f = Fields { commit: false };
    egui::ScrollArea::vertical().max_height(260.0).id_salt("vertex_scroll").show(ui, |ui| {
        egui::Grid::new("vertex_table").num_columns(5).striped(true).show(ui, |ui| {
            for h in ["#", "X", "Y", "Kind", "Radius"] {
                ui.strong(h);
            }
            ui.end_row();
            for (i, v) in vertices.iter_mut().enumerate().take(MAX_VERTEX_ROWS) {
                ui.label(format!("{i}"));
                f.num(ui, &mut v.p.x, 10.0);
                f.num(ui, &mut v.p.y, 10.0);
                let before = kind_label(v.kind);
                egui::ComboBox::from_id_salt(("vertex_kind", id, i)).selected_text(before).width(90.0).show_ui(ui, |ui| {
                    for choice in KIND_CHOICES {
                        let same = kind_label(choice) == before;
                        if ui.selectable_label(same, kind_label(choice)).clicked() && !same {
                            v.kind = choice;
                            f.commit = true;
                        }
                    }
                });
                if let VertexKind::Arc { radius } = &mut v.kind {
                    f.num_range(ui, radius);
                } else {
                    ui.label("-");
                }
                ui.end_row();
            }
        });
    });
    f.commit
}

/// The commands that turn the vertices `base` into `edited`: one `SetVertex` per changed vertex.
pub fn vertex_edit_commands(id: ObjectId, base: &[Vertex], edited: &[Vertex]) -> Vec<Command> {
    base.iter()
        .zip(edited)
        .enumerate()
        .filter(|(_, (b, e))| b != e)
        .map(|(index, (b, e))| Command::SetVertex {
            id,
            index,
            kind: (b.kind != e.kind).then_some(e.kind),
            p: (b.p != e.p).then_some(e.p),
        })
        .collect()
}

fn edit_shape(ui: &mut Ui, shape: &mut Shape) -> bool {
    let mut f = Fields { commit: false };
    egui::Grid::new("shape_fields").num_columns(2).show(ui, |ui| match shape {
        Shape::Viewport(vp) => {
            f.text(ui, "Name", &mut vp.name);
            f.scalar(ui, "Scale 1:", &mut vp.scale, 1.0);
            f.xy(ui, "Frame min", &mut vp.min);
            f.xy(ui, "Frame max", &mut vp.max);
            f.xy(ui, "Model center", &mut vp.center);
            ui.label("Frame");
            f.commit |= ui.checkbox(&mut vp.show_frame, "Show").changed();
            ui.end_row();
        }
        Shape::TitleBlock(tb) => f.scalar(ui, "Margin", &mut tb.margin, 1.0),
        Shape::Group { name } => f.text(ui, "Name", name),
        Shape::Symbol { at, rotation, scale, scale_y, .. } => {
            f.xy(ui, "Position", at);
            f.scalar(ui, "Rotation (rad)", rotation, 0.1);
            f.scalar(ui, "Scale", scale, 0.1);
            let mut sy = scale_y.unwrap_or(*scale);
            f.scalar(ui, "Scale Y", &mut sy, 0.1);
            *scale_y = (sy != *scale).then_some(sy);
        }
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
            f.count(ui, points.len());
        }
        Shape::Locus3d { p } => {
            ui.label("Position");
            ui.horizontal(|ui| {
                f.num(ui, &mut p.x, 10.0);
                f.num(ui, &mut p.y, 10.0);
            });
            ui.end_row();
            f.scalar(ui, "Z", &mut p.z, 10.0);
        }
        Shape::Polyline3d { points, closed } => {
            ui.label("Closed");
            f.commit |= ui.checkbox(closed, "").changed();
            ui.end_row();
            f.count(ui, points.len());
            let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.z), hi.max(p.z)));
            ui.label("Z range");
            ui.label(if points.is_empty() { "-".to_string() } else { format!("{lo:.1} to {hi:.1}") });
            ui.end_row();
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
        Shape::ExistingTree { at, species, dbh, spread, height, condition, action, rpz_factor, rpz_cap } => {
            f.xy(ui, "Position", at);
            f.text(ui, "Species", species);
            f.scalar(ui, "Trunk diameter", dbh, 10.0);
            f.scalar(ui, "Canopy spread", spread, 50.0);
            f.scalar(ui, "Height", height, 50.0);
            ui.label("Condition");
            let before = *condition;
            egui::ComboBox::from_id_salt("tree_condition").selected_text(condition.id()).show_ui(ui, |ui| {
                for c in TreeCondition::ALL {
                    ui.selectable_value(condition, c, c.id());
                }
            });
            f.commit |= *condition != before;
            ui.end_row();
            ui.label("Action");
            let before = *action;
            egui::ComboBox::from_id_salt("tree_action").selected_text(action.id()).show_ui(ui, |ui| {
                for a in TreeAction::ALL {
                    ui.selectable_value(action, a, a.id());
                }
            });
            f.commit |= *action != before;
            ui.end_row();
            ui.label("Root zone factor");
            let mut factor = rpz_factor.unwrap_or(DEFAULT_RPZ_FACTOR);
            let r = ui.add(DragValue::new(&mut factor).speed(0.1).range(0.0..=100.0).suffix(" x DBH"));
            if r.changed() {
                *rpz_factor = Some(factor);
            }
            f.commit |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
            ui.label("Root zone cap");
            let mut cap = rpz_cap.unwrap_or(DEFAULT_RPZ_CAP);
            let r = ui.add(DragValue::new(&mut cap).speed(100.0).range(0.0..=1.0e6).suffix(" mm"));
            if r.changed() {
                *rpz_cap = Some(cap);
            }
            f.commit |= r.drag_stopped() || r.lost_focus();
            ui.end_row();
        }
        Shape::PlantTag { at, anchor, height, .. } => {
            f.xy(ui, "Label position", at);
            f.xy(ui, "Leader end", anchor);
            f.scalar(ui, "Text height", height, 5.0);
        }
        Shape::LandscapeArea { boundary, name } => {
            f.text(ui, "Name", name);
            f.count(ui, boundary.len());
        }
        Shape::Hardscape { boundary, material, .. } => {
            f.text(ui, "Material", material);
            f.count(ui, boundary.len());
        }
        Shape::SiteModel { points, lines, settings } => f.commit |= crate::site_ui::model_fields(ui, points.len(), lines.len(), settings),
        Shape::SiteModifier { modifier, boundary } => f.commit |= crate::site_ui::modifier_fields(ui, modifier, boundary),
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
        let d = &session.editor.doc();
        let layer = d.layer(d.active_layer).map_or("?", |l| l.name.as_str());
        let class = d.class(d.active_class).map_or("?", |c| c.name.as_str());
        ui.label(format!("Nothing selected.\nActive layer: {layer}\nActive class: {class}"));
        *edit = InfoEdit::default();
        return;
    };
    if crate::data_ui::info_tab(ui, session, &sel, status) {
        *edit = InfoEdit::default();
        return;
    }
    if sel.len() > 1 {
        ui.label(format!("{} objects selected", sel.len()));
        *edit = InfoEdit::default();
        class_layer_combos(ui, session, &sel, status);
        return;
    }
    let Some(obj) = session.editor.doc().object(id).cloned() else { return };
    // Reset the buffer when another object is selected or the shape changed elsewhere.
    if edit.id != id || edit.base.as_ref() != Some(&obj.shape) {
        *edit = InfoEdit { id, base: Some(obj.shape.clone()), edited: Some(obj.shape.clone()) };
    }
    ui.label(format!("{}  (id {})", obj.shape.kind(), id));
    let edited = edit.edited.as_mut().expect("edit buffer");
    let commit = edit_shape(ui, edited);
    // Vertex edits are sent as `set_vertex` commands, one per changed vertex.
    if edited.vertices().is_some() {
        let base_vertices = edit.base.as_ref().and_then(Shape::vertices).cloned().unwrap_or_default();
        let finished = vertex_table(ui, id, edited.vertices_mut().expect("has vertices"));
        let cmds = vertex_edit_commands(id, &base_vertices, edited.vertices().expect("has vertices"));
        if finished && !cmds.is_empty() {
            let cmd = if cmds.len() == 1 { cmds[0].clone() } else { Command::Batch { commands: cmds } };
            run_command(session, &cmd, status);
            *edit = InfoEdit::default();
            return;
        }
    }
    if commit && Some(&*edited) != edit.base.as_ref() {
        let cmd = Command::ModifyObject { id, shape: Some(edited.clone()), layer: None, class: None, attrs: None };
        if run_command(session, &cmd, status).is_none() {
            *edit = InfoEdit::default();
        }
    }
    if let Some(area) = obj.shape.area() {
        ui.label(format!("Area {:.2} m\u{b2}", area / 1e6));
    }
    if matches!(obj.shape, Shape::Plant { .. } | Shape::ExistingTree { .. }) {
        crate::plant_ui::plant_object_info(ui, session, id, status);
    }
    if matches!(obj.shape, Shape::Hardscape { .. } | Shape::LandscapeArea { .. }) {
        crate::area_ui::object_info(ui, session, id, status);
    }
    crate::data_ui::table_info(ui, session, id, status);
    crate::site_ui::cut_fill_readout(ui, session, id);
    ui.separator();
    class_layer_combos(ui, session, &sel, status);
    let doc = &session.editor.doc();
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
    let doc = &session.editor.doc();
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
    pub sheets: crate::sheets_ui::SheetUi,
    renaming: Option<(RenameKind, u64, String)>,
}

pub fn navigation(ui: &mut Ui, session: &mut Session, nav: &mut NavState, status: &mut String) {
    ui.heading("Navigation");
    ui.separator();
    let mut cmds: Vec<Command> = Vec::new();
    let doc = &session.editor.doc();

    ui.horizontal(|ui| {
        ui.strong("Design Layers");
        if ui.small_button("+").on_hover_text("Add Design Layer").clicked() {
            cmds.push(Command::AddLayer { name: format!("Layer {}", doc.layers.len() + 1), elevation: 0.0, scale: None, kind: None });
        }
    });
    for l in doc.layers.iter().filter(|l| l.kind == cadektor_core::model::LayerKind::Design) {
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
    crate::sheets_ui::navigation_sections(ui, session, &mut nav.sheets, status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::curve;

    fn rounded() -> Shape {
        let arc = |x: f64, y: f64| Vertex::new(DVec2::new(x, y), VertexKind::Arc { radius: 100.0 });
        Shape::Hardscape { boundary: vec![arc(0.0, 0.0), arc(1000.0, 0.0), arc(1000.0, 1000.0), arc(0.0, 1000.0)], material: "m".into(), thickness: None, elevation: None }
    }

    fn add(session: &mut Session, shape: Shape) -> ObjectId {
        let cmd = Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() };
        ObjectId(session.editor.execute(&cmd).unwrap().created[0])
    }

    #[test]
    fn vertex_edits_become_set_vertex_commands() {
        let mut s = Session::default();
        let id = add(&mut s, rounded());
        let base = s.editor.doc().object(id).unwrap().shape.vertices().unwrap().clone();
        assert!(vertex_edit_commands(id, &base, &base).is_empty());
        let mut edited = base.clone();
        edited[1].kind = VertexKind::Arc { radius: 250.0 };
        edited[2].p = DVec2::new(1100.0, 1000.0);
        edited[3].kind = VertexKind::Bezier;
        edited[3].p = DVec2::new(-10.0, 1000.0);
        let cmds = vertex_edit_commands(id, &base, &edited);
        assert_eq!(
            cmds,
            vec![
                Command::SetVertex { id, index: 1, kind: Some(VertexKind::Arc { radius: 250.0 }), p: None },
                Command::SetVertex { id, index: 2, kind: None, p: Some(DVec2::new(1100.0, 1000.0)) },
                Command::SetVertex { id, index: 3, kind: Some(VertexKind::Bezier), p: Some(DVec2::new(-10.0, 1000.0)) },
            ]
        );
        let mut status = String::new();
        assert!(run_command(&mut s, &Command::Batch { commands: cmds }, &mut status).is_some());
        assert_eq!(s.editor.doc().object(id).unwrap().shape.vertices().unwrap(), &edited);
        // One undo step brings all three back.
        assert!(s.editor.undo());
        assert_eq!(s.editor.doc().object(id).unwrap().shape.vertices().unwrap(), &base);
    }

    #[test]
    fn kind_labels() {
        assert_eq!(kind_label(VertexKind::Corner), "Corner");
        assert_eq!(kind_label(VertexKind::Arc { radius: 1.0 }), "Arc");
        assert_eq!(kind_label(VertexKind::PointOnArc), "Point on arc");
        assert_eq!(kind_label(VertexKind::Bezier), "B\u{e9}zier");
        assert_eq!(kind_label(VertexKind::Cubic), "Cubic");
        assert_eq!(KIND_CHOICES.len(), 5);
    }

    #[test]
    fn palettes_draw_for_a_curved_selection() {
        let ctx = egui::Context::default();
        let mut s = Session::default();
        let id = add(&mut s, rounded());
        let poly = add(
            &mut s,
            Shape::Polyline {
                points: vec![Vertex::corner(DVec2::ZERO), Vertex::new(DVec2::new(5.0, 5.0), VertexKind::Bezier), Vertex::corner(DVec2::new(10.0, 0.0))],
                closed: false,
            },
        );
        let (mut edit, mut status) = (InfoEdit::default(), String::new());
        let mut settings = ToolSettings::default();
        let mut machine = ToolMachine::default();
        let keys = Keymap::defaults();
        for sel in [id, poly] {
            s.selection = vec![sel];
            for tool in [Tool::Select, Tool::Polyline] {
                machine.set_tool(tool);
                let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                    object_info(ui, &mut s, &mut edit, &mut status);
                    tool_panel(ui, &mut machine, &keys, &mut settings, &Default::default());
                });
            }
        }
        assert!(curve::path(s.editor.doc().object(poly).unwrap().shape.vertices().unwrap(), false).segs.len() == 1);
    }
}
