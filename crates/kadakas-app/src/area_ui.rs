//! The window part of hardscapes and landscape areas: the style pickers of their tools, the edit
//! modes in Object Info, and the 2D extras (joint pattern, border, holes and plants). All changes
//! are commands. The geometry comes from `kadakas_core::hardscape` and `landscape`.

use crate::units_ui::DragValueExt as _;
use kadakas_l10n::tr;
use kadakas_view3d::numfmt::UiNumber;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use egui::{Color32, DragValue, Painter, Stroke, Ui};
use glam::DVec2;
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::curve;
use kadakas_core::hardscape::{self, HardscapePatch, Join, Mode, Slope};
use kadakas_core::landscape::{self, LandscapePatch};
use kadakas_core::model::{Document, Object, ObjectId, Shape};
use kadakas_core::resource::ResourceId;
use kadakas_site::SiteTerrain;

use crate::canvas::{Style, color32};
use crate::panels::run_command;
use crate::view::{View, ViewExt as _};
use kadakas_ui_core::tools::{Tool, ToolSettings};

fn style_combo(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    current: Option<ResourceId>,
    styles: &[(ResourceId, String)],
    none_label: &str,
) -> Option<Option<ResourceId>> {
    let mut chosen = current;
    let text = current
        .and_then(|c| styles.iter().find(|(i, _)| *i == c))
        .map_or(none_label.to_string(), |(_, n)| n.clone());
    egui::ComboBox::from_id_salt(salt)
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, none_label);
            for (id, name) in styles {
                ui.selectable_value(&mut chosen, Some(*id), name);
            }
        });
    (chosen != current).then_some(chosen)
}

fn hardscape_styles(doc: &Document) -> Vec<(ResourceId, String)> {
    doc.resources
        .hardscape_styles()
        .map(|(i, n, _)| (i, n.to_string()))
        .collect()
}

fn landscape_styles(doc: &Document) -> Vec<(ResourceId, String)> {
    doc.resources
        .landscape_area_styles()
        .map(|(i, n, _)| (i, n.to_string()))
        .collect()
}

/// The style pickers in the Tool bar for the Hardscape and Landscape Area tools.
pub fn tool_options(ui: &mut Ui, tool: Tool, settings: &mut ToolSettings, doc: &Document) {
    ui.add_space(6.0);
    let s = &mut settings.areas;
    match tool {
        Tool::Hardscape => {
            ui.label(tr!("area-ui-hardscape-style"));
            if let Some(c) = style_combo(
                ui,
                "hardscape_tool_style",
                s.hardscape_style,
                &hardscape_styles(doc),
                &tr!("area-ui-none-plain-slab"),
            ) {
                s.hardscape_style = c;
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.path, false, tr!("area-ui-slab"));
                ui.selectable_value(&mut s.path, true, tr!("area-ui-path"));
            });
            if s.path {
                ui.horizontal(|ui| {
                    ui.label(tr!("area-ui-width"));
                    ui.add(
                        DragValue::new(&mut s.width)
                            .speed(10.0)
                            .range(1.0..=1.0e6)
                            .length(),
                    );
                });
                egui::ComboBox::from_id_salt("hardscape_tool_join")
                    .selected_text(tr!(
                        "area-ui-join",
                        join = crate::enum_names::name("join", s.join.id())
                    ))
                    .show_ui(ui, |ui| {
                        for j in Join::ALL {
                            ui.selectable_value(
                                &mut s.join,
                                j,
                                crate::enum_names::name("join", j.id()),
                            );
                        }
                    });
            }
        }
        Tool::LandscapeArea => {
            ui.label(tr!("area-ui-landscape-area-style"));
            if let Some(c) = style_combo(
                ui,
                "landscape_tool_style",
                s.landscape_style,
                &landscape_styles(doc),
                "(none)",
            ) {
                s.landscape_style = c;
            }
        }
        _ => {}
    }
}

/// The 3D-aware quantity readout and edit modes of a hardscape or landscape area in Object Info.
pub fn object_info(
    ui: &mut Ui,
    session: &mut Session,
    id: ObjectId,
    paving: &mut crate::paving_ui::PavingUi,
    status: &mut String,
) {
    let Some(obj) = session.editor.doc().object(id).cloned() else {
        return;
    };
    match obj.shape {
        Shape::Hardscape { .. } => hardscape_info(ui, session, &obj, paving, status),
        Shape::LandscapeArea { .. } => landscape_info(ui, session, &obj, status),
        _ => {}
    }
}

fn apply(session: &mut Session, id: ObjectId, patch: HardscapePatch, status: &mut String) {
    run_command(
        session,
        &Command::SetHardscape {
            ids: vec![id],
            patch,
        },
        status,
    );
}

fn hardscape_info(
    ui: &mut Ui,
    session: &mut Session,
    obj: &Object,
    paving: &mut crate::paving_ui::PavingUi,
    status: &mut String,
) {
    let id = obj.id;
    let doc = session.editor.doc();
    let d = hardscape::data(obj).unwrap_or_default();
    let styles = hardscape_styles(doc);
    ui.separator();
    ui.strong(tr!("area-ui-hardscape"));
    let mut patch = HardscapePatch::default();
    ui.horizontal(|ui| {
        ui.label(tr!("area-ui-style"));
        if let Some(c) = style_combo(ui, ("hardscape_style_of", id), d.style, &styles, "(none)") {
            match c {
                Some(s) => patch.style = Some(s),
                None => patch.unstyle = true,
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr!("area-ui-mode"));
        let can_path = !d.center.is_empty();
        if ui
            .selectable_label(d.mode == Mode::Slab, tr!("area-ui-slab"))
            .clicked()
            && d.mode != Mode::Slab
        {
            patch.mode = Some(Mode::Slab);
        }
        if ui
            .add_enabled(
                can_path,
                egui::Button::selectable(d.mode == Mode::Path, tr!("area-ui-path")),
            )
            .on_disabled_hover_text(tr!("area-ui-draw-a-path-with-the-hardscape-tool"))
            .clicked()
            && d.mode != Mode::Path
        {
            patch.mode = Some(Mode::Path);
        }
    });
    if d.mode == Mode::Path {
        let mut width = d.width;
        let r = ui.horizontal(|ui| {
            ui.label(tr!("area-ui-width"));
            ui.add(
                DragValue::new(&mut width)
                    .speed(10.0)
                    .range(1.0..=1.0e6)
                    .length(),
            )
        });
        if (r.inner.drag_stopped() || r.inner.lost_focus()) && width != d.width {
            patch.width = Some(width);
        }
        let mut join = d.join;
        egui::ComboBox::from_id_salt(("hardscape_join", id))
            .selected_text(tr!(
                "area-ui-join",
                join = crate::enum_names::name("join", join.id())
            ))
            .show_ui(ui, |ui| {
                for j in Join::ALL {
                    ui.selectable_value(&mut join, j, crate::enum_names::name("join", j.id()));
                }
            });
        if join != d.join {
            patch.join = Some(join);
        }
    }
    // Slope.
    let mut kind = d.slope.id();
    ui.horizontal(|ui| {
        ui.label(tr!("area-ui-slope"));
        egui::ComboBox::from_id_salt(("hardscape_slope", id))
            .selected_text(crate::enum_names::name("slope", kind))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut kind, "none", crate::enum_names::name("slope", "none"));
                ui.selectable_value(
                    &mut kind,
                    "cross",
                    crate::enum_names::name("slope", "cross"),
                );
                if matches!(d.slope, Slope::Edges { .. }) {
                    ui.label(tr!("area-ui-edges-set-by-the-api"));
                }
            });
    });
    if kind != d.slope.id() {
        patch.slope = Some(if kind == "cross" {
            Slope::Cross {
                percent: 2.0,
                angle_deg: 0.0,
            }
        } else {
            Slope::None
        });
    }
    if let Slope::Cross { percent, angle_deg } = &d.slope {
        let (mut p, mut a) = (*percent, *angle_deg);
        let r = ui.horizontal(|ui| {
            let r1 = ui.add(
                DragValue::new(&mut p)
                    .speed(0.1)
                    .range(-100.0..=100.0)
                    .suffix(" %")
                    .ui_number(),
            );
            let r2 = if d.mode == Mode::Slab {
                Some(ui.add(DragValue::new(&mut a).speed(1.0).degrees()))
            } else {
                None
            };
            (r1, r2)
        });
        let done = r.inner.0.drag_stopped()
            || r.inner.0.lost_focus()
            || r.inner
                .1
                .is_some_and(|r| r.drag_stopped() || r.lost_focus());
        if done && (p != *percent || a != *angle_deg) {
            patch.slope = Some(Slope::Cross {
                percent: p,
                angle_deg: a,
            });
        }
    }
    let elevation = match &obj.shape {
        Shape::Hardscape { elevation, .. } => elevation.unwrap_or(0.0),
        _ => 0.0,
    };
    let mut e = elevation;
    let r = ui.horizontal(|ui| {
        ui.label(if d.drape {
            tr!("area-ui-height-above-ground")
        } else {
            tr!("area-ui-elevation")
        });
        ui.add(
            DragValue::new(&mut e)
                .speed(5.0)
                .range(-1.0e8..=1.0e8)
                .length(),
        )
    });
    if (r.inner.drag_stopped() || r.inner.lost_focus()) && e != elevation {
        patch.elevation = Some(e);
    }
    let mut drape = d.drape;
    if ui
        .checkbox(&mut drape, tr!("area-ui-drape-on-site-model"))
        .changed()
    {
        patch.drape = Some(drape);
    }
    let mut pad = d.site_modifier;
    if ui
        .checkbox(&mut pad, tr!("area-ui-site-modifier-acts-as-a-pad"))
        .on_hover_text(tr!("area-ui-the-formation-level-under-the-build-up"))
        .changed()
    {
        patch.site_modifier = Some(pad);
    }
    pattern_settings(ui, obj, &d, doc, &mut patch);
    // Quantities.
    let terrain = SiteTerrain::new(session.site_cache(), session.editor.revision(), doc);
    use kadakas_core::ground::GroundSource;
    let at = |p: DVec2| terrain.elevation(obj, p);
    let rev = session.editor.revision();
    let cached = QUANTITIES.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(r, i, h, _)| *r == rev && *i == id && h.is_some())
            .and_then(|(_, _, h, _)| h.clone())
    });
    let computed = cached.or_else(|| {
        let q = hardscape::report::quantities(doc, obj, Some(&at));
        QUANTITIES.with(|c| *c.borrow_mut() = Some((rev, id, q.clone(), None)));
        q
    });
    if let Some(q) = computed {
        egui::Grid::new(("hardscape_qty", id))
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(tr!("area-ui-plan-area"));
                ui.label(kadakas_units::current().area_text(q.plan_area_m2 * 1e6));
                ui.end_row();
                if (q.surface_area_m2 - q.plan_area_m2).abs() > 1e-6 {
                    ui.label(if q.draped {
                        tr!("area-ui-draped-area")
                    } else {
                        tr!("area-ui-surface-area")
                    });
                    ui.label(kadakas_units::current().area_text(q.surface_area_m2 * 1e6));
                    ui.end_row();
                }
                ui.label(tr!("area-ui-perimeter"));
                ui.label(kadakas_units::current().length_text_d(q.perimeter_m * 1000.0, 1));
                ui.end_row();
                for l in &q.layers {
                    ui.label(format!("{} {}", l.kind, l.material));
                    ui.label(kadakas_units::current().volume_text(l.volume_m3 * 1e9));
                    ui.end_row();
                }
                ui.label(tr!("area-ui-volume"));
                ui.label(kadakas_units::current().volume_text(q.volume_m3 * 1e9));
                ui.end_row();
            });
    }
    if !patch.is_empty() {
        apply(session, id, patch, status);
    }
    // Zones, courses and counts of the unit pattern (PAT-07, PAT-08, PAT-04).
    crate::paving_ui::hardscape_section(ui, session, id, paving, status);
}

/// Holes of a hardscape, and the origin and the angle of its joint pattern. The origin and the
/// angle replace the ones of the style; the buttons give the style's back.
fn pattern_settings(
    ui: &mut Ui,
    obj: &Object,
    d: &hardscape::HardscapeData,
    doc: &kadakas_core::model::Document,
    patch: &mut HardscapePatch,
) {
    if !d.holes.is_empty() {
        ui.horizontal(|ui| {
            ui.label(tr!("area-ui-hole-s", count = d.holes.len()));
            if ui.button(tr!("area-ui-clear-holes")).clicked() {
                patch.holes = Some(Vec::new());
            }
        });
    }
    let Some(joints) = hardscape::data::style_of(doc, obj).and_then(|(_, s)| s.joints) else {
        return;
    };
    // A view setting, not part of the document: the findings of unit_pattern.check on the plan.
    let mut check = crate::plan_draw::show_check();
    if ui
        .checkbox(&mut check, tr!("area-ui-show-pattern-check"))
        .on_hover_text(tr!(
            "area-ui-the-check-marks-overlaps-small-pieces-wrong-joints-and-holes"
        ))
        .changed()
    {
        crate::plan_draw::set_show_check(check);
    }
    let units = kadakas_units::current();
    ui.horizontal(|ui| {
        match d.origin {
            Some(o) => {
                ui.label(tr!(
                    "area-ui-pattern-origin",
                    x = units.length_text_d(o.x, 1),
                    y = units.length_text_d(o.y, 1)
                ));
                if ui.button(tr!("area-ui-clear-origin")).clicked() {
                    patch.clear_origin = true;
                }
            }
            None => {
                ui.label(tr!("area-ui-pattern-origin-middle"));
            }
        };
    });
    ui.horizontal(|ui| {
        ui.label(tr!("area-ui-pattern-angle"));
        match d.angle {
            Some(a) => {
                let mut v = a;
                let r = ui.add(DragValue::new(&mut v).speed(1.0).degrees());
                if (r.drag_stopped() || r.lost_focus()) && v != a {
                    patch.angle = Some(v);
                }
                if ui.button(tr!("area-ui-clear-angle")).clicked() {
                    patch.clear_angle = true;
                }
            }
            None => {
                ui.label(format!("{}°", joints.angle));
                if ui.button(tr!("area-ui-set-angle")).clicked() {
                    patch.angle = Some(joints.angle);
                }
            }
        }
    });
}

fn landscape_info(ui: &mut Ui, session: &mut Session, obj: &Object, status: &mut String) {
    let id = obj.id;
    let doc = session.editor.doc();
    let d = landscape::data(obj).unwrap_or_default();
    let styles = landscape_styles(doc);
    ui.separator();
    ui.strong(tr!("area-ui-landscape-area"));
    let mut patch = LandscapePatch::default();
    ui.horizontal(|ui| {
        ui.label(tr!("area-ui-style"));
        if let Some(c) = style_combo(ui, ("landscape_style_of", id), d.style, &styles, "(none)") {
            match c {
                Some(s) => patch.style = Some(s),
                None => patch.unstyle = true,
            }
        }
    });
    let mut show = landscape::shows_plants(doc, obj);
    if ui.checkbox(&mut show, tr!("area-ui-show-plants")).changed() {
        patch.show_plants = Some(show);
    }
    let mut drape = d.drape;
    if ui
        .checkbox(&mut drape, tr!("area-ui-drape-on-site-model"))
        .changed()
    {
        patch.drape = Some(drape);
    }
    if !d.holes.is_empty() {
        ui.horizontal(|ui| {
            ui.label(tr!("area-ui-hole-s", count = d.holes.len()));
            if ui.button(tr!("area-ui-clear-holes")).clicked() {
                patch.holes = Some(Vec::new());
            }
        });
    }
    let rev = session.editor.revision();
    let cached = QUANTITIES.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(r, i, _, l)| *r == rev && *i == id && l.is_some())
            .and_then(|(_, _, _, l)| l.clone())
    });
    let computed = cached.or_else(|| {
        let q = landscape::quantities(doc, obj);
        QUANTITIES.with(|c| *c.borrow_mut() = Some((rev, id, None, q.clone())));
        q
    });
    if let Some(q) = computed {
        egui::Grid::new(("landscape_qty", id))
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(tr!("area-ui-net-area"));
                ui.label(tr!(
                    "area-ui-net-area-holes",
                    area = kadakas_units::current().area_text(q.net_area_m2 * 1e6),
                    holes = kadakas_units::current().area_text(q.holes_area_m2 * 1e6)
                ));
                ui.end_row();
                for r in &q.rows {
                    ui.label(&r.name);
                    ui.label(tr!(
                        "area-ui-plants-percent-density",
                        count = r.count,
                        percent = kadakas_units::current().number_text(r.percent, 1),
                        density = crate::units_ui::format_per_area(r.density_per_m2)
                    ));
                    ui.end_row();
                }
            });
    }
    if patch != LandscapePatch::default() {
        run_command(
            session,
            &Command::SetLandscapeArea {
                ids: vec![id],
                patch,
            },
            status,
        );
    }
}

// ---------------------------------------------------------------------------
// Drawing in 2D

/// What the plan view draws on top of a hardscape or a landscape area. It is computed when the
/// object or a style changes, not at every frame.
#[derive(Default)]
struct Extras {
    plants: Vec<(DVec2, f64, Option<kadakas_core::model::Color>, Option<f64>)>,
}

type QuantityCache = Option<(
    u64,
    ObjectId,
    Option<hardscape::report::Quantities>,
    Option<landscape::AreaQuantities>,
)>;

thread_local! {
    static CACHE: RefCell<HashMap<ObjectId, (u64, Rc<Extras>)>> = RefCell::new(HashMap::new());
    /// The revision of the document that is drawn now, set by the canvas for each frame.
    /// A revision names one state of one document: the editor takes it from a counter that all
    /// editors share, so two documents never have the same revision, and this key is also the
    /// identity of the document.
    static REVISION: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// The quantities that Object Info shows, for one object and one document revision.
    static QUANTITIES: RefCell<QuantityCache> = const { RefCell::new(None) };
}

/// Tells the drawing which document revision it draws. With it, the extras are computed again
/// only after the document changed. Without it (`None`), the object and the styles are compared.
pub fn current_revision() -> Option<u64> {
    REVISION.with(|r| r.get())
}

pub fn set_revision(revision: Option<u64>) {
    REVISION.with(|r| r.set(revision));
}

fn extras(doc: &Document, o: &Object) -> Rc<Extras> {
    let key = match REVISION.with(|r| r.get()) {
        Some(rev) => rev,
        None => {
            let mut h = DefaultHasher::new();
            format!("{:?}|{:?}", o, doc.resources).hash(&mut h);
            h.finish()
        }
    };
    if let Some(found) = CACHE.with(|c| {
        c.borrow()
            .get(&o.id)
            .filter(|(k, _)| *k == key)
            .map(|(_, e)| e.clone())
    }) {
        return found;
    }
    let mut e = Extras::default();
    match &o.shape {
        Shape::LandscapeArea { .. } if landscape::shows_plants(doc, o) => {
            for p in landscape::scatter(doc, o) {
                if let Some(s) = doc.resources.plant_style(p.plant) {
                    e.plants
                        .push((p.at, s.spread / 2.0, s.fill, s.protection_radius()));
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
pub fn draw_extras(
    painter: &Painter,
    view: &View,
    doc: &Document,
    o: &Object,
    st: Style,
    offset: DVec2,
) {
    let s = |p: DVec2| view.to_screen(p + offset);
    match &o.shape {
        Shape::Hardscape { boundary, .. } => {
            // Holes are open: white over the fill, with an outline.
            for hole in &hardscape::data(o).unwrap_or_default().holes {
                let ring: Vec<egui::Pos2> =
                    curve::flatten(hole, true, (0.25 / view.zoom).max(1e-3))
                        .iter()
                        .map(|p| s(*p))
                        .collect();
                painter.add(egui::epaint::PathShape {
                    points: ring,
                    closed: true,
                    fill: Color32::from_white_alpha(230),
                    stroke: egui::epaint::PathStroke::new(1.0, st.pen),
                });
            }
            let Some((_, style)) = hardscape::data::style_of(doc, o) else {
                return;
            };
            if let Some(j) = &style.joints {
                // Groups with units under 3 px are hidden by the drawing, with a label.
                let color = j.color.map_or(st.pen.gamma_multiply(0.5), color32);
                crate::plan_draw::draw_plan(
                    painter,
                    view,
                    doc,
                    o,
                    current_revision(),
                    color,
                    st.pen,
                    offset,
                );
            }
            if let Some(b) = &style.border {
                let color = b.color.map_or(st.pen, color32);
                let w = (b.width * view.zoom * 0.5).clamp(1.5, 8.0) as f32;
                let stroke = egui::epaint::PathStroke::new(w, color.gamma_multiply(0.6));
                let d = hardscape::data(o).unwrap_or_default();
                let line = |pts: Vec<DVec2>, closed: bool| {
                    painter.add(egui::epaint::PathShape {
                        points: pts.iter().map(|p| s(*p)).collect(),
                        closed,
                        fill: Color32::TRANSPARENT,
                        stroke: stroke.clone(),
                    });
                };
                if d.mode == Mode::Path && !d.closed {
                    // A path has the border along the chosen edges only.
                    let (left, right) = hardscape::offset::sides(&d.axis(), d.width);
                    match b.sides {
                        hardscape::Sides::Both => {
                            line(left, false);
                            line(right, false);
                        }
                        hardscape::Sides::Left => line(left, false),
                        hardscape::Sides::Right => line(right, false),
                    }
                } else {
                    line(
                        curve::flatten(boundary, true, (0.25 / view.zoom).max(1e-3)),
                        true,
                    );
                }
            }
        }
        Shape::LandscapeArea { .. } => {
            let Some(d) = landscape::data(o) else { return };
            for hole in &d.holes {
                let ring: Vec<egui::Pos2> =
                    curve::flatten(hole, true, (0.25 / view.zoom).max(1e-3))
                        .iter()
                        .map(|p| s(*p))
                        .collect();
                painter.add(egui::epaint::PathShape {
                    points: ring,
                    closed: true,
                    fill: Color32::from_white_alpha(200),
                    stroke: egui::epaint::PathStroke::new(1.0, st.pen),
                });
            }
            let e = extras(doc, o);
            let stroke = Stroke::new(1.0, st.pen.gamma_multiply(0.8));
            for (at, r, fill, zone) in e.plants.iter().take(landscape::MAX_SCATTER) {
                let fill = fill.map_or(Color32::from_rgba_unmultiplied(80, 150, 70, 90), |c| {
                    color32(c).gamma_multiply(0.5)
                });
                painter.circle(s(*at), (*r * view.zoom) as f32, fill, stroke);
                if let Some(z) = zone {
                    painter.circle_stroke(
                        s(*at),
                        (*z * view.zoom) as f32,
                        Stroke::new(1.0, st.pen.gamma_multiply(0.5)),
                    );
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod pattern_tests {
    use super::*;
    use serde_json::json;

    fn session_with_lot(set: serde_json::Value) -> (Session, ObjectId) {
        let mut s = Session::with_sequential_ids();
        s.handle(
            "hardscape_style.create",
            json!({"name": "P", "joints": {"kind": "stack", "unit_length": 200, "unit_width": 100, "spacing": 5, "angle": 30}}),
        )
        .unwrap();
        let id = s
            .handle(
                "hardscape.add",
                json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "style": "P"}),
            )
            .unwrap()["id"]
            .clone();
        let id: ObjectId = serde_json::from_value(id).unwrap();
        if !set.is_null() {
            let mut p = set;
            p["id"] = json!(id);
            s.handle("hardscape.set", p).unwrap();
        }
        (s, id)
    }

    fn run(s: &mut Session, id: ObjectId) -> HardscapePatch {
        let ctx = egui::Context::default();
        let obj = s.editor.doc().object(id).unwrap().clone();
        let d = hardscape::data(&obj).unwrap_or_default();
        let mut patch = HardscapePatch::default();
        let doc = s.editor.doc().clone();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            pattern_settings(ui, &obj, &d, &doc, &mut patch);
        });
        patch
    }

    #[test]
    fn the_pattern_settings_show_holes_origin_and_angle_without_changing_them() {
        for set in [
            json!(null),
            json!({"holes": [[[1000, 1000], [1500, 1000], [1500, 1500]]]}),
            json!({"origin": [100, 200], "angle": 45, "holes": [[[1000, 1000], [1500, 1000], [1500, 1500]]]}),
        ] {
            let (mut s, id) = session_with_lot(set);
            assert_eq!(run(&mut s, id), HardscapePatch::default());
            // The whole panel runs too.
            let ctx = egui::Context::default();
            let mut status = String::new();
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                object_info(
                    ui,
                    &mut s,
                    id,
                    &mut crate::paving_ui::PavingUi::default(),
                    &mut status,
                );
            });
        }
        // A hardscape without a style has holes but no pattern settings.
        let mut s = Session::with_sequential_ids();
        let id: ObjectId = serde_json::from_value(
            s.handle(
                "hardscape.add",
                json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]],
                    "holes": [[[1000, 1000], [1500, 1000], [1500, 1500]]]}),
            )
            .unwrap()["id"]
                .clone(),
        )
        .unwrap();
        assert_eq!(run(&mut s, id), HardscapePatch::default());
    }

    #[test]
    fn the_strings_exist_in_both_languages() {
        for key in [
            "area-ui-pattern-origin",
            "area-ui-pattern-origin-middle",
            "area-ui-clear-origin",
            "area-ui-pattern-angle",
            "area-ui-set-angle",
            "area-ui-clear-angle",
            "io-dialog-patterns",
        ] {
            let en = include_str!("../../kadakas-l10n/locales/en-US/area-ui.ftl").to_string()
                + include_str!("../../kadakas-l10n/locales/en-US/io-dialog.ftl");
            let et = include_str!("../../kadakas-l10n/locales/et/area-ui.ftl").to_string()
                + include_str!("../../kadakas-l10n/locales/et/io-dialog.ftl");
            assert!(en.contains(&format!("{key} =")), "{key} in en-US");
            assert!(et.contains(&format!("{key} =")), "{key} in et");
        }
    }
}
