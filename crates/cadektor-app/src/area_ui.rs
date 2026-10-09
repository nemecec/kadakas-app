//! The window part of hardscapes and landscape areas: the style pickers of their tools, the edit
//! modes in Object Info, and the 2D extras (joint pattern, border, holes and plants). All changes
//! are commands. The geometry comes from `cadektor_core::hardscape` and `landscape`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use cadektor_api::Session;
use cadektor_core::command::Command;
use cadektor_core::curve;
use cadektor_core::hardscape::{self, HardscapePatch, Join, Mode, Slope};
use cadektor_core::landscape::{self, LandscapePatch};
use cadektor_core::model::{Document, Object, ObjectId, Shape};
use cadektor_core::resource::ResourceId;
use cadektor_site::SiteTerrain;
use egui::{Color32, DragValue, Painter, Stroke, Ui};
use glam::DVec2;

use crate::canvas::{Style, color32};
use crate::panels::run_command;
use crate::tools::{Tool, ToolSettings};
use crate::view::View;

/// Settings of the Hardscape tool and the Landscape Area tool.
#[derive(Clone, Debug, PartialEq)]
pub struct AreaToolSettings {
    /// Hardscape style of new hardscapes. `None`: a plain slab with the material of the Data bar.
    pub hardscape_style: Option<ResourceId>,
    /// Draw a path (a center line with a width) instead of a slab.
    pub path: bool,
    pub width: f64,
    pub join: Join,
    /// Landscape area style of new areas.
    pub landscape_style: Option<ResourceId>,
}

impl Default for AreaToolSettings {
    fn default() -> Self {
        AreaToolSettings { hardscape_style: None, path: false, width: 1200.0, join: Join::Miter, landscape_style: None }
    }
}

impl AreaToolSettings {
    /// The command that adds a hardscape drawn with these settings. `None` means: add the plain
    /// shape, as before hardscape styles.
    pub fn hardscape_command(&self, points: &[cadektor_core::curve::Vertex]) -> Option<Command> {
        if !self.path && self.hardscape_style.is_none() {
            return None;
        }
        let mut patch = HardscapePatch { style: self.hardscape_style, ..Default::default() };
        let boundary = if self.path {
            patch.center = Some(points.to_vec());
            patch.width = Some(self.width);
            patch.join = Some(self.join);
            None
        } else {
            Some(points.to_vec())
        };
        Some(Command::AddHardscape { boundary, patch: Box::new(patch), layer: None, class: None, attrs: Default::default() })
    }

    /// The command that adds a landscape area drawn with these settings, or `None` for a plain area.
    pub fn landscape_command(&self, points: &[cadektor_core::curve::Vertex], name: &str) -> Option<Command> {
        let style = self.landscape_style?;
        let patch = LandscapePatch { style: Some(style), ..Default::default() };
        Some(Command::AddLandscapeArea { boundary: points.to_vec(), name: name.into(), patch: Box::new(patch), layer: None, class: None, attrs: Default::default() })
    }
}

fn style_combo(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, current: Option<ResourceId>, styles: &[(ResourceId, String)], none_label: &str) -> Option<Option<ResourceId>> {
    let mut chosen = current;
    let text = current.and_then(|c| styles.iter().find(|(i, _)| *i == c)).map_or(none_label.to_string(), |(_, n)| n.clone());
    egui::ComboBox::from_id_salt(salt).selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(&mut chosen, None, none_label);
        for (id, name) in styles {
            ui.selectable_value(&mut chosen, Some(*id), name);
        }
    });
    (chosen != current).then_some(chosen)
}

fn hardscape_styles(doc: &Document) -> Vec<(ResourceId, String)> {
    doc.resources.hardscape_styles().map(|(i, n, _)| (i, n.to_string())).collect()
}

fn landscape_styles(doc: &Document) -> Vec<(ResourceId, String)> {
    doc.resources.landscape_area_styles().map(|(i, n, _)| (i, n.to_string())).collect()
}

/// The style pickers in the Tool bar for the Hardscape and Landscape Area tools.
pub fn tool_options(ui: &mut Ui, tool: Tool, settings: &mut ToolSettings, doc: &Document) {
    ui.add_space(6.0);
    let s = &mut settings.areas;
    match tool {
        Tool::Hardscape => {
            ui.label("Hardscape style");
            if let Some(c) = style_combo(ui, "hardscape_tool_style", s.hardscape_style, &hardscape_styles(doc), "(none: plain slab)") {
                s.hardscape_style = c;
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.path, false, "Slab");
                ui.selectable_value(&mut s.path, true, "Path");
            });
            if s.path {
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(DragValue::new(&mut s.width).speed(10.0).range(1.0..=1.0e6).suffix(" mm"));
                });
                egui::ComboBox::from_id_salt("hardscape_tool_join").selected_text(format!("{} join", s.join.id())).show_ui(ui, |ui| {
                    for j in Join::ALL {
                        ui.selectable_value(&mut s.join, j, j.id());
                    }
                });
            }
        }
        Tool::LandscapeArea => {
            ui.label("Landscape area style");
            if let Some(c) = style_combo(ui, "landscape_tool_style", s.landscape_style, &landscape_styles(doc), "(none)") {
                s.landscape_style = c;
            }
        }
        _ => {}
    }
}

/// The 3D-aware quantity readout and edit modes of a hardscape or landscape area in Object Info.
pub fn object_info(ui: &mut Ui, session: &mut Session, id: ObjectId, status: &mut String) {
    let Some(obj) = session.editor.doc().object(id).cloned() else { return };
    match obj.shape {
        Shape::Hardscape { .. } => hardscape_info(ui, session, &obj, status),
        Shape::LandscapeArea { .. } => landscape_info(ui, session, &obj, status),
        _ => {}
    }
}

fn apply(session: &mut Session, id: ObjectId, patch: HardscapePatch, status: &mut String) {
    run_command(session, &Command::SetHardscape { ids: vec![id], patch }, status);
}

fn hardscape_info(ui: &mut Ui, session: &mut Session, obj: &Object, status: &mut String) {
    let id = obj.id;
    let doc = session.editor.doc();
    let d = hardscape::data(obj).unwrap_or_default();
    let styles = hardscape_styles(doc);
    ui.separator();
    ui.strong("Hardscape");
    let mut patch = HardscapePatch::default();
    ui.horizontal(|ui| {
        ui.label("Style");
        if let Some(c) = style_combo(ui, ("hardscape_style_of", id), d.style, &styles, "(none)") {
            match c {
                Some(s) => patch.style = Some(s),
                None => patch.unstyle = true,
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Mode");
        let can_path = !d.center.is_empty();
        if ui.selectable_label(d.mode == Mode::Slab, "Slab").clicked() && d.mode != Mode::Slab {
            patch.mode = Some(Mode::Slab);
        }
        if ui.add_enabled(can_path, egui::Button::selectable(d.mode == Mode::Path, "Path")).on_disabled_hover_text("Draw a path with the Hardscape tool in Path mode").clicked() && d.mode != Mode::Path {
            patch.mode = Some(Mode::Path);
        }
    });
    if d.mode == Mode::Path {
        let mut width = d.width;
        let r = ui.horizontal(|ui| {
            ui.label("Width");
            ui.add(DragValue::new(&mut width).speed(10.0).range(1.0..=1.0e6).suffix(" mm"))
        });
        if (r.inner.drag_stopped() || r.inner.lost_focus()) && width != d.width {
            patch.width = Some(width);
        }
        let mut join = d.join;
        egui::ComboBox::from_id_salt(("hardscape_join", id)).selected_text(format!("{} join", join.id())).show_ui(ui, |ui| {
            for j in Join::ALL {
                ui.selectable_value(&mut join, j, j.id());
            }
        });
        if join != d.join {
            patch.join = Some(join);
        }
    }
    // Slope.
    let mut kind = d.slope.id();
    ui.horizontal(|ui| {
        ui.label("Slope");
        egui::ComboBox::from_id_salt(("hardscape_slope", id)).selected_text(kind).show_ui(ui, |ui| {
            ui.selectable_value(&mut kind, "none", "none");
            ui.selectable_value(&mut kind, "cross", "cross slope");
            if matches!(d.slope, Slope::Edges { .. }) {
                ui.label("edges (set by the API)");
            }
        });
    });
    if kind != d.slope.id() {
        patch.slope = Some(if kind == "cross" { Slope::Cross { percent: 2.0, angle_deg: 0.0 } } else { Slope::None });
    }
    if let Slope::Cross { percent, angle_deg } = &d.slope {
        let (mut p, mut a) = (*percent, *angle_deg);
        let r = ui.horizontal(|ui| {
            let r1 = ui.add(DragValue::new(&mut p).speed(0.1).range(-100.0..=100.0).suffix(" %"));
            let r2 = if d.mode == Mode::Slab { Some(ui.add(DragValue::new(&mut a).speed(1.0).suffix("\u{b0}"))) } else { None };
            (r1, r2)
        });
        let done = r.inner.0.drag_stopped() || r.inner.0.lost_focus() || r.inner.1.is_some_and(|r| r.drag_stopped() || r.lost_focus());
        if done && (p != *percent || a != *angle_deg) {
            patch.slope = Some(Slope::Cross { percent: p, angle_deg: a });
        }
    }
    let elevation = match &obj.shape {
        Shape::Hardscape { elevation, .. } => elevation.unwrap_or(0.0),
        _ => 0.0,
    };
    let mut e = elevation;
    let r = ui.horizontal(|ui| {
        ui.label(if d.drape { "Height above ground" } else { "Elevation" });
        ui.add(DragValue::new(&mut e).speed(5.0).range(-1.0e8..=1.0e8).suffix(" mm"))
    });
    if (r.inner.drag_stopped() || r.inner.lost_focus()) && e != elevation {
        patch.elevation = Some(e);
    }
    let mut drape = d.drape;
    if ui.checkbox(&mut drape, "Drape on site model").changed() {
        patch.drape = Some(drape);
    }
    let mut pad = d.site_modifier;
    if ui.checkbox(&mut pad, "Site modifier (acts as a pad)").on_hover_text("The formation level under the build-up changes the proposed surface of the site model on this layer").changed() {
        patch.site_modifier = Some(pad);
    }
    // Quantities.
    let terrain = SiteTerrain::new(session.site_cache(), session.editor.revision(), doc);
    use cadektor_core::hardscape::TerrainSource;
    let at = |p: DVec2| terrain.elevation(obj, p);
    if let Some(q) = hardscape::report::quantities(doc, obj, Some(&at)) {
        egui::Grid::new(("hardscape_qty", id)).num_columns(2).show(ui, |ui| {
            ui.label("Plan area");
            ui.label(format!("{:.2} m\u{b2}", q.plan_area_m2));
            ui.end_row();
            if (q.surface_area_m2 - q.plan_area_m2).abs() > 1e-6 {
                ui.label(if q.draped { "Draped area" } else { "Surface area" });
                ui.label(format!("{:.2} m\u{b2}", q.surface_area_m2));
                ui.end_row();
            }
            ui.label("Perimeter");
            ui.label(format!("{:.2} m", q.perimeter_m));
            ui.end_row();
            for l in &q.layers {
                ui.label(format!("{} {}", l.kind, l.material));
                ui.label(format!("{:.3} m\u{b3}", l.volume_m3));
                ui.end_row();
            }
            ui.label("Volume");
            ui.label(format!("{:.3} m\u{b3}", q.volume_m3));
            ui.end_row();
        });
    }
    if !patch.is_empty() {
        apply(session, id, patch, status);
    }
}

fn landscape_info(ui: &mut Ui, session: &mut Session, obj: &Object, status: &mut String) {
    let id = obj.id;
    let doc = session.editor.doc();
    let d = landscape::data(obj).unwrap_or_default();
    let styles = landscape_styles(doc);
    ui.separator();
    ui.strong("Landscape area");
    let mut patch = LandscapePatch::default();
    ui.horizontal(|ui| {
        ui.label("Style");
        if let Some(c) = style_combo(ui, ("landscape_style_of", id), d.style, &styles, "(none)") {
            match c {
                Some(s) => patch.style = Some(s),
                None => patch.unstyle = true,
            }
        }
    });
    let mut show = landscape::shows_plants(doc, obj);
    if ui.checkbox(&mut show, "Show plants").changed() {
        patch.show_plants = Some(show);
    }
    let mut drape = d.drape;
    if ui.checkbox(&mut drape, "Drape on site model").changed() {
        patch.drape = Some(drape);
    }
    if !d.holes.is_empty() {
        ui.horizontal(|ui| {
            ui.label(format!("{} hole(s)", d.holes.len()));
            if ui.button("Clear holes").clicked() {
                patch.holes = Some(Vec::new());
            }
        });
    }
    if let Some(q) = landscape::quantities(doc, obj) {
        egui::Grid::new(("landscape_qty", id)).num_columns(2).show(ui, |ui| {
            ui.label("Net area");
            ui.label(format!("{:.2} m\u{b2}  (holes {:.2})", q.net_area_m2, q.holes_area_m2));
            ui.end_row();
            for r in &q.rows {
                ui.label(&r.name);
                ui.label(format!("{} plants  ({:.1} %, {:.1} per m\u{b2})", r.count, r.percent, r.density_per_m2));
                ui.end_row();
            }
        });
    }
    if patch != LandscapePatch::default() {
        run_command(session, &Command::SetLandscapeArea { ids: vec![id], patch }, status);
    }
}

// ---------------------------------------------------------------------------
// Drawing in 2D

/// What the plan view draws on top of a hardscape or a landscape area. It is computed when the
/// object or a style changes, not at every frame.
#[derive(Default)]
struct Extras {
    joints: Vec<(DVec2, DVec2)>,
    plants: Vec<(DVec2, f64, Option<cadektor_core::model::Color>, Option<f64>)>,
}

thread_local! {
    static CACHE: RefCell<HashMap<ObjectId, (u64, Rc<Extras>)>> = RefCell::new(HashMap::new());
}

fn extras(doc: &Document, o: &Object) -> Rc<Extras> {
    let mut h = DefaultHasher::new();
    format!("{:?}|{:?}", o, doc.resources).hash(&mut h);
    let key = h.finish();
    if let Some(found) = CACHE.with(|c| c.borrow().get(&o.id).filter(|(k, _)| *k == key).map(|(_, e)| e.clone())) {
        return found;
    }
    let mut e = Extras::default();
    match &o.shape {
        Shape::Hardscape { boundary, .. } => {
            if let Some(j) = hardscape::data::style_of(doc, o).and_then(|(_, s)| s.joints) {
                e.joints = hardscape::joints::joint_lines(&curve::flatten(boundary, true, 2.0), &j).segments;
            }
        }
        Shape::LandscapeArea { .. } if landscape::shows_plants(doc, o) => {
            for p in landscape::scatter(doc, o) {
                if let Some(s) = doc.resources.plant_style(p.plant) {
                    e.plants.push((p.at, s.spread / 2.0, s.fill, s.protection_radius()));
                }
            }
        }
        _ => {}
    }
    let e = Rc::new(e);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 2000 {
            c.clear();
        }
        c.insert(o.id, (key, e.clone()));
    });
    e
}

/// Draws the joint pattern, the border, the holes and the plants of a hardscape or a landscape area.
pub fn draw_extras(painter: &Painter, view: &View, doc: &Document, o: &Object, st: Style, offset: DVec2) {
    let s = |p: DVec2| view.to_screen(p + offset);
    match &o.shape {
        Shape::Hardscape { boundary, .. } => {
            let Some((_, style)) = hardscape::data::style_of(doc, o) else { return };
            if let Some(j) = &style.joints
                && j.unit_width.min(j.unit_length) * view.zoom >= 3.0
            {
                let color = j.color.map_or(st.pen.gamma_multiply(0.5), color32);
                let stroke = Stroke::new(1.0, color);
                for (a, b) in extras(doc, o).joints.iter().take(40_000) {
                    painter.line_segment([s(*a), s(*b)], stroke);
                }
            }
            if let Some(b) = &style.border {
                let color = b.color.map_or(st.pen, color32);
                let ring: Vec<egui::Pos2> = curve::flatten(boundary, true, (0.25 / view.zoom).max(1e-3)).iter().map(|p| s(*p)).collect();
                let w = (b.width * view.zoom * 0.5).clamp(1.5, 8.0) as f32;
                painter.add(egui::epaint::PathShape { points: ring, closed: true, fill: Color32::TRANSPARENT, stroke: egui::epaint::PathStroke::new(w, color.gamma_multiply(0.6)) });
            }
        }
        Shape::LandscapeArea { .. } => {
            let Some(d) = landscape::data(o) else { return };
            for hole in &d.holes {
                let ring: Vec<egui::Pos2> = curve::flatten(hole, true, (0.25 / view.zoom).max(1e-3)).iter().map(|p| s(*p)).collect();
                painter.add(egui::epaint::PathShape { points: ring, closed: true, fill: Color32::from_white_alpha(200), stroke: egui::epaint::PathStroke::new(1.0, st.pen) });
            }
            let e = extras(doc, o);
            let stroke = Stroke::new(1.0, st.pen.gamma_multiply(0.8));
            for (at, r, fill, zone) in e.plants.iter().take(landscape::MAX_SCATTER) {
                let fill = fill.map_or(Color32::from_rgba_unmultiplied(80, 150, 70, 90), |c| color32(c).gamma_multiply(0.5));
                painter.circle(s(*at), (*r * view.zoom) as f32, fill, stroke);
                if let Some(z) = zone {
                    painter.circle_stroke(s(*at), (*z * view.zoom) as f32, Stroke::new(1.0, st.pen.gamma_multiply(0.5)));
                }
            }
        }
        _ => {}
    }
}
