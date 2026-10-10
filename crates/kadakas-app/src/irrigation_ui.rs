//! Irrigation in the app: drawing, picking, snapping, the Tool bar options, the fields in
//! Object Info and the Calculate command with its warnings.

use std::cell::RefCell;

use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Color32, ComboBox, DragValue, Painter, Pos2, Stroke, Ui};
use glam::DVec2;
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::irrigation::{IrrigationShape, LineType, sector_points, zone_color};
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_irrigation::report::{self, WarningRow, ZonePressure};
use kadakas_irrigation::{Catalog, DEFAULT_TOLERANCE, analyze, zone_commands};

use crate::canvas::Style;
use crate::panels::{Fields, run_command};
use crate::view::{View, ViewExt as _};
use kadakas_ui_core::tools::{IrrMode, ToolSettings};

fn color(c: [u8; 3], alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], alpha)
}

/// Draws an irrigation object. An outlet draws its coverage as a translucent fan in the color of
/// its zone.
pub fn draw(painter: &Painter, view: &View, i: &IrrigationShape, st: Style, offset: DVec2) {
    let s = |p: DVec2| view.to_screen(p + offset);
    let w = st.width.max(1.0);
    match i {
        IrrigationShape::Outlet {
            at,
            arc,
            direction,
            radius,
            zone,
            ..
        } => {
            let c = zone_color(zone.unwrap_or(0));
            let centre = s(*at);
            if let Some(r) = radius {
                let fan = sector_points(*at + offset, *r, *arc, *direction, 64);
                let pts: Vec<Pos2> = fan.iter().map(|p| view.to_screen(*p)).collect();
                if *arc >= 360.0 {
                    painter.circle(
                        centre,
                        (*r * view.zoom) as f32,
                        color(c, 60),
                        Stroke::new(1.0, color(c, 200)),
                    );
                } else {
                    let mut mesh = Mesh::default();
                    for p in &pts {
                        mesh.colored_vertex(*p, color(c, 60));
                    }
                    for k in 1..pts.len() - 1 {
                        mesh.add_triangle(0, k as u32, k as u32 + 1);
                    }
                    painter.add(mesh);
                    painter.add(PathShape {
                        points: pts,
                        closed: true,
                        fill: Color32::TRANSPARENT,
                        stroke: PathStroke::new(1.0, color(c, 200)),
                    });
                }
            }
            painter.circle(centre, 3.5, color(c, 255), Stroke::new(1.0, Color32::BLACK));
        }
        IrrigationShape::Line {
            points, line_type, ..
        } => {
            let pts: Vec<Pos2> = points.iter().map(|p| s(*p)).collect();
            let (c, width) = match line_type {
                LineType::Mainline => (Color32::from_rgb(20, 50, 140), w * 2.0),
                LineType::Lateral => (Color32::from_rgb(40, 110, 200), w * 1.4),
                LineType::Drip => (Color32::from_rgb(30, 140, 70), w),
            };
            let stroke = Stroke::new(width, c);
            if *line_type == LineType::Drip {
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 6.0, 3.0));
            } else {
                painter.add(PathShape {
                    points: pts,
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: PathStroke::new(width, c),
                });
            }
        }
        IrrigationShape::Poc { at, .. } => {
            let c = s(*at);
            let blue = Color32::from_rgb(20, 50, 140);
            painter.circle(c, 7.0, color([20, 50, 140], 70), Stroke::new(w, blue));
            painter.line_segment(
                [c - egui::vec2(7.0, 0.0), c + egui::vec2(7.0, 0.0)],
                Stroke::new(w, blue),
            );
            painter.line_segment(
                [c - egui::vec2(0.0, 7.0), c + egui::vec2(0.0, 7.0)],
                Stroke::new(w, blue),
            );
        }
        IrrigationShape::Valve { at, zone, .. } => {
            let c = s(*at);
            let zc = zone_color(*zone);
            let r = 6.0;
            let pts = vec![
                c + egui::vec2(-r, -r),
                c + egui::vec2(r, r),
                c + egui::vec2(r, -r),
                c + egui::vec2(-r, r),
            ];
            painter.add(PathShape {
                points: pts,
                closed: true,
                fill: color(zc, 140),
                stroke: PathStroke::new(w, color(zc, 255)),
            });
        }
        IrrigationShape::Controller { at, name, .. } => {
            let c = s(*at);
            let r = egui::Rect::from_center_size(c, egui::vec2(14.0, 14.0));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(w, Color32::from_rgb(20, 50, 140)),
                egui::StrokeKind::Middle,
            );
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                name.chars().next().unwrap_or('C').to_string(),
                egui::FontId::proportional(10.0),
                Color32::BLACK,
            );
        }
    }
}

/// The options of the Irrigation tool in the Tool bar. Returns true when the mode changed.
pub fn tool_options(ui: &mut Ui, settings: &mut ToolSettings, doc: &Document) -> bool {
    let s = &mut settings.irrigation;
    let before = s.mode;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for m in IrrMode::ALL {
            if ui.selectable_label(s.mode == m, m.label()).clicked() {
                s.mode = m;
            }
        }
    });
    ui.small(s.mode.help());
    let cat = Catalog::of_document(doc);
    let num = |ui: &mut Ui,
               label: &str,
               v: &mut f64,
               speed: f64,
               range: std::ops::RangeInclusive<f64>,
               unit: &str| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(DragValue::new(v).speed(speed).range(range).suffix(unit));
        });
    };
    match s.mode {
        IrrMode::Outlet => {
            ui.horizontal(|ui| {
                ui.label("Model");
                ComboBox::from_id_salt("irr_model")
                    .selected_text(&s.model)
                    .show_ui(ui, |ui| {
                        for h in &cat.heads {
                            ui.selectable_value(&mut s.model, h.name.clone(), &h.name);
                        }
                    });
            });
            num(ui, "Arc", &mut s.arc, 1.0, 1.0..=360.0, "\u{b0}");
            num(
                ui,
                "Direction",
                &mut s.direction,
                1.0,
                -360.0..=360.0,
                "\u{b0}",
            );
        }
        IrrMode::Line => {
            ui.horizontal(|ui| {
                ui.label("Type");
                for t in LineType::ALL {
                    ui.selectable_value(&mut s.line_type, t, t.id());
                }
            });
            ui.horizontal(|ui| {
                ui.label("Material");
                ComboBox::from_id_salt("irr_material")
                    .selected_text(&s.material)
                    .show_ui(ui, |ui| {
                        for m in &cat.materials {
                            ui.selectable_value(&mut s.material, m.name.clone(), &m.name);
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Size");
                ComboBox::from_id_salt("irr_size")
                    .selected_text(&s.size)
                    .show_ui(ui, |ui| {
                        for p in cat
                            .pipes
                            .iter()
                            .filter(|p| p.material.eq_ignore_ascii_case(&s.material))
                        {
                            ui.selectable_value(&mut s.size, p.size.clone(), &p.size);
                        }
                    });
            });
        }
        IrrMode::Poc => {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut s.name);
            });
            num(
                ui,
                "Static pressure",
                &mut s.static_pressure,
                5.0,
                1.0..=5000.0,
                " kPa",
            );
            num(
                ui,
                "Available flow",
                &mut s.available_flow,
                1.0,
                1.0..=100000.0,
                " l/min",
            );
        }
        IrrMode::Valve => {
            let mut zone = f64::from(s.zone);
            num(ui, "Zone (0 = next)", &mut zone, 1.0, 0.0..=9999.0, "");
            s.zone = zone as u32;
            num(
                ui,
                "Flow factor kv",
                &mut s.kv,
                0.1,
                0.01..=1000.0,
                " m\u{b3}/h",
            );
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut s.name);
            });
        }
        IrrMode::Controller => {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut s.name);
            });
            let mut n = f64::from(s.stations);
            num(ui, "Stations", &mut n, 1.0, 1.0..=999.0, "");
            s.stations = n as u32;
        }
    }
    ui.small("Select an irrigation object and use Calculate in Object Info.");
    s.mode != before
}

/// The fields of an irrigation object in Object Info. Lists of catalog parts are in
/// [`object_info`].
pub fn edit(ui: &mut Ui, f: &mut Fields, i: &mut IrrigationShape) {
    match i {
        IrrigationShape::Outlet {
            at,
            model,
            arc,
            direction,
            radius,
            zone,
        } => {
            f.xy(ui, "Position", at);
            f.text(ui, "Model", model);
            f.scalar(ui, "Arc (deg)", arc, 1.0);
            f.scalar(ui, "Direction (deg)", direction, 1.0);
            let mut r = radius.unwrap_or(0.0);
            f.scalar(ui, "Radius", &mut r, 10.0);
            *radius = (r > 0.0).then_some(r);
            ui.label("Zone");
            ui.label(zone.map_or("-".to_string(), |z| z.to_string()));
            ui.end_row();
        }
        IrrigationShape::Line {
            points,
            line_type,
            size,
            material,
            ..
        } => {
            ui.label("Type");
            ui.horizontal(|ui| {
                for t in LineType::ALL {
                    if ui.selectable_label(*line_type == t, t.id()).clicked() && *line_type != t {
                        *line_type = t;
                        f.commit = true;
                    }
                }
            });
            ui.end_row();
            f.text(ui, "Size", size);
            f.text(ui, "Material", material);
            f.count(ui, points.len());
            ui.label("Length");
            let len: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
            ui.label(format!("{:.2} m", len / 1000.0));
            ui.end_row();
        }
        IrrigationShape::Poc {
            at,
            name,
            static_pressure,
            available_flow,
            supply_loss,
            meter_loss,
        } => {
            f.xy(ui, "Position", at);
            f.text(ui, "Name", name);
            f.scalar(ui, "Static pressure (kPa)", static_pressure, 5.0);
            f.scalar(ui, "Available flow (l/min)", available_flow, 1.0);
            for (label, v) in [
                ("Supply loss (kPa)", supply_loss),
                ("Meter loss (kPa)", meter_loss),
            ] {
                let mut x = v.unwrap_or(0.0);
                f.scalar(ui, label, &mut x, 1.0);
                *v = (x > 0.0).then_some(x);
            }
        }
        IrrigationShape::Valve {
            at,
            zone,
            name,
            kv,
            controller,
            station,
        } => {
            f.xy(ui, "Position", at);
            let mut z = f64::from(*zone);
            f.scalar(ui, "Zone", &mut z, 1.0);
            *zone = z.round().max(1.0) as u32;
            f.text(ui, "Name", name);
            f.scalar(ui, "Flow factor kv", kv, 0.1);
            let mut c = controller.clone().unwrap_or_default();
            f.text(ui, "Controller", &mut c);
            *controller = (!c.trim().is_empty()).then_some(c);
            let mut st = f64::from(station.unwrap_or(0));
            f.scalar(ui, "Station (0 = none)", &mut st, 1.0);
            *station = (st >= 1.0).then_some(st as u32);
        }
        IrrigationShape::Controller { at, name, stations } => {
            f.xy(ui, "Position", at);
            f.text(ui, "Name", name);
            let mut n = f64::from(*stations);
            f.scalar(ui, "Stations", &mut n, 1.0);
            *stations = n.round().max(1.0) as u32;
        }
    }
}

/// The result of the last calculation.
#[derive(Clone, Debug)]
pub struct Calc {
    /// The revision of the document after the calculation (zones stored).
    pub revision: u64,
    pub zones: Vec<ZonePressure>,
    pub warnings: Vec<WarningRow>,
    pub assigned: usize,
}

thread_local! {
    static CALC: RefCell<Option<Calc>> = const { RefCell::new(None) };
}

/// The last calculation, if any.
pub fn last_calculation() -> Option<Calc> {
    CALC.with(|c| c.borrow().clone())
}

/// Calculates the network, stores the zone in the outlets (one undo step) and keeps the result
/// for Object Info. Returns the number of warnings.
pub fn calculate(session: &mut Session, status: &mut String) -> usize {
    let a = analyze(session.editor.doc(), DEFAULT_TOLERANCE);
    let cmds = zone_commands(session.editor.doc(), &a);
    let assigned = cmds.len();
    if !cmds.is_empty() {
        let _ = run_command(session, &Command::Batch { commands: cmds }, status);
    }
    let calc = Calc {
        revision: session.editor.revision(),
        zones: report::pressure_analysis(&a),
        warnings: a.warnings().into_iter().map(WarningRow::from).collect(),
        assigned,
    };
    let n = calc.warnings.len();
    *status = match (a.solution.zones.len(), n) {
        (0, _) => "Calculated: no zone has water. See the warnings in Object Info.".to_string(),
        (z, 0) => format!("Calculated {z} zone(s): no warnings"),
        (z, n) => format!("Calculated {z} zone(s): {n} warning(s). See Object Info."),
    };
    CALC.with(|c| *c.borrow_mut() = Some(calc));
    n
}

fn kpa(x: f64) -> String {
    format!("{x:.1} kPa")
}

/// Calculate, the result for the selected object, and the warnings of the calculation.
pub fn object_info(ui: &mut Ui, session: &mut Session, id: ObjectId, status: &mut String) {
    ui.separator();
    ui.strong("Irrigation");
    // Pick catalog parts from lists.
    catalog_pickers(ui, session, id, status);
    if ui
        .button("Calculate")
        .on_hover_text("Flow, pressure and zones of the whole network")
        .clicked()
    {
        calculate(session, status);
    }
    let Some(calc) = last_calculation() else {
        ui.label("Not calculated yet.");
        return;
    };
    if calc.revision != session.editor.revision() {
        ui.colored_label(
            Color32::from_rgb(200, 120, 0),
            "Out of date: calculate again.",
        );
    }
    if calc.assigned > 0 {
        ui.small(format!(
            "The last calculation stored the zone in {} outlet(s).",
            calc.assigned
        ));
    }
    // The result for this object.
    for z in &calc.zones {
        if let Some(o) = z.outlets.iter().find(|o| o.id == Some(id.0)) {
            ui.label(format!(
                "Zone {}: {:.2} l/min, pressure {} (minimum {}, margin {})",
                z.zone,
                o.flow_lpm,
                kpa(o.pressure_kpa),
                kpa(o.min_kpa),
                kpa(o.margin_kpa)
            ));
        }
    }
    let zone_of_object = match session.editor.doc().object(id).map(|o| &o.shape) {
        Some(Shape::Irrigation(i)) => match i.as_ref() {
            IrrigationShape::Valve { zone, .. } => Some(*zone),
            IrrigationShape::Outlet { zone, .. } => *zone,
            _ => None,
        },
        _ => None,
    };
    if let Some(z) = zone_of_object.and_then(|z| calc.zones.iter().find(|r| r.zone == z)) {
        ui.label(format!(
            "Zone {} total: {:.2} l/min of {:.1} available, highest velocity {:.2} m/s",
            z.zone, z.flow_lpm, z.available_lpm, z.max_velocity_ms
        ));
        if let Some(c) = &z.critical {
            ui.label(format!(
                "Critical outlet: {} at {} (margin {})",
                c.label,
                kpa(c.pressure_kpa),
                kpa(c.margin_kpa)
            ));
        }
    }
    let mine: Vec<&WarningRow> = calc
        .warnings
        .iter()
        .filter(|w| w.object == Some(id.0))
        .collect();
    for w in &mine {
        ui.colored_label(Color32::from_rgb(200, 40, 40), &w.message);
    }
    egui::CollapsingHeader::new(format!("All warnings ({})", calc.warnings.len()))
        .id_salt("irr_warnings")
        .show(ui, |ui| {
            if calc.warnings.is_empty() {
                ui.label("None");
            }
            for w in &calc.warnings {
                ui.label(format!("{}: {}", w.kind, w.message));
            }
        });
}

/// Lists of catalog parts: change the model of an outlet, or the size and material of a line.
fn catalog_pickers(ui: &mut Ui, session: &mut Session, id: ObjectId, status: &mut String) {
    let doc = session.editor.doc();
    let Some(Shape::Irrigation(i)) = doc.object(id).map(|o| o.shape.clone()) else {
        return;
    };
    let cat = Catalog::of_document(doc);
    let mut changed: Option<IrrigationShape> = None;
    match i.as_ref() {
        IrrigationShape::Outlet { model, .. } => {
            ui.horizontal(|ui| {
                ui.label("Catalog model");
                ComboBox::from_id_salt(("irr_pick_model", id))
                    .selected_text(model)
                    .show_ui(ui, |ui| {
                        for h in &cat.heads {
                            if ui
                                .selectable_label(h.name.eq_ignore_ascii_case(model), &h.name)
                                .clicked()
                            {
                                changed = Some(with_head(&i, h));
                            }
                        }
                    });
            });
        }
        IrrigationShape::Line { size, material, .. } => {
            ui.horizontal(|ui| {
                ui.label("Catalog pipe");
                ComboBox::from_id_salt(("irr_pick_pipe", id))
                    .selected_text(format!("{material} {size}"))
                    .show_ui(ui, |ui| {
                        for p in &cat.pipes {
                            let name = format!("{} {}", p.material, p.size);
                            if ui.selectable_label(false, &name).clicked() {
                                changed = Some(with_pipe(&i, p));
                            }
                        }
                    });
            });
        }
        _ => {}
    }
    if let Some(n) = changed {
        replace_shape(session, id, n, status);
    }
}

/// The outlet with the model and the radius of a catalog head. Other objects stay as they are.
fn with_head(i: &IrrigationShape, h: &kadakas_irrigation::HeadModel) -> IrrigationShape {
    let mut n = i.clone();
    if let IrrigationShape::Outlet { model, radius, .. } = &mut n {
        *model = h.name.clone();
        *radius = Some(h.radius);
    }
    n
}

/// The line with the size and material of a catalog pipe. Other objects stay as they are.
fn with_pipe(i: &IrrigationShape, p: &kadakas_irrigation::PipeSize) -> IrrigationShape {
    let mut n = i.clone();
    if let IrrigationShape::Line { size, material, .. } = &mut n {
        *size = p.size.clone();
        *material = p.material.clone();
    }
    n
}

fn replace_shape(session: &mut Session, id: ObjectId, n: IrrigationShape, status: &mut String) {
    let _ = run_command(
        session,
        &Command::ModifyObject {
            id,
            shape: Some(Shape::Irrigation(Box::new(n))),
            layer: None,
            class: None,
            attrs: None,
        },
        status,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Editor;
    use kadakas_core::model::Attributes;

    fn add(ed: &mut Editor, s: IrrigationShape) -> ObjectId {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Irrigation(Box::new(s)),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    fn zone() -> Session {
        let mut s = Session::default();
        let v = DVec2::new;
        add(
            &mut s.editor,
            IrrigationShape::Poc {
                at: v(0.0, 0.0),
                name: String::new(),
                static_pressure: 400.0,
                available_flow: 100.0,
                supply_loss: None,
                meter_loss: None,
            },
        );
        add(
            &mut s.editor,
            IrrigationShape::Line {
                points: vec![v(0.0, 0.0), v(20_000.0, 0.0)],
                line_type: LineType::Mainline,
                size: "50".into(),
                material: "PVC".into(),
                emitter_spacing: None,
                emitter_flow: None,
            },
        );
        add(
            &mut s.editor,
            IrrigationShape::Valve {
                at: v(20_000.0, 0.0),
                zone: 1,
                name: String::new(),
                kv: 6.0,
                controller: None,
                station: None,
            },
        );
        add(
            &mut s.editor,
            IrrigationShape::Line {
                points: vec![v(20_000.0, 0.0), v(40_000.0, 0.0)],
                line_type: LineType::Lateral,
                size: "32".into(),
                material: "PE".into(),
                emitter_spacing: None,
                emitter_flow: None,
            },
        );
        add(
            &mut s.editor,
            IrrigationShape::Outlet {
                at: v(40_000.0, 0.0),
                model: "Spray-4.0".into(),
                arc: 90.0,
                direction: 0.0,
                radius: Some(4000.0),
                zone: None,
            },
        );
        s
    }

    #[test]
    fn calculate_stores_zones_and_keeps_the_result() {
        let mut s = zone();
        let mut status = String::new();
        let n = calculate(&mut s, &mut status);
        assert_eq!(n, 0, "{status}");
        assert!(status.contains("1 zone"), "{status}");
        let c = last_calculation().unwrap();
        assert_eq!(c.assigned, 1);
        assert_eq!(c.revision, s.editor.revision());
        assert_eq!(c.zones.len(), 1);
        // The outlet stores its zone.
        let stored = s
            .editor
            .doc()
            .objects
            .values()
            .find_map(|o| match &o.shape {
                Shape::Irrigation(i) => match i.as_ref() {
                    IrrigationShape::Outlet { zone, .. } => Some(*zone),
                    _ => None,
                },
                _ => None,
            });
        assert_eq!(stored, Some(Some(1)));
        // Nothing to assign the second time.
        calculate(&mut s, &mut status);
        assert_eq!(last_calculation().unwrap().assigned, 0);
        // A document without a POC reports it.
        let mut empty = Session::default();
        add(
            &mut empty.editor,
            IrrigationShape::Outlet {
                at: DVec2::ZERO,
                model: "Spray-4.0".into(),
                arc: 360.0,
                direction: 0.0,
                radius: None,
                zone: None,
            },
        );
        assert!(calculate(&mut empty, &mut status) > 0);
        assert!(status.contains("no zone"), "{status}");
        // A weak supply gives warnings.
        let mut weak = zone();
        if let Some(o) = weak.editor.doc().objects.values().next().map(|o| o.id) {
            let mut shape = IrrigationShape::Poc {
                at: DVec2::ZERO,
                name: String::new(),
                static_pressure: 120.0,
                available_flow: 100.0,
                supply_loss: None,
                meter_loss: None,
            };
            if let IrrigationShape::Poc { at, .. } = &mut shape {
                *at = DVec2::ZERO;
            }
            weak.editor
                .execute(&Command::ModifyObject {
                    id: o,
                    shape: Some(Shape::Irrigation(Box::new(shape))),
                    layer: None,
                    class: None,
                    attrs: None,
                })
                .unwrap();
        }
        assert!(calculate(&mut weak, &mut status) >= 1);
        assert!(status.contains("warning"), "{status}");
    }

    fn sample_shapes() -> Vec<IrrigationShape> {
        let v = DVec2::new;
        let line = |t| IrrigationShape::Line {
            points: vec![v(0.0, 0.0), v(2000.0, 0.0), v(2000.0, 1000.0)],
            line_type: t,
            size: "32".into(),
            material: "PE".into(),
            emitter_spacing: None,
            emitter_flow: None,
        };
        let outlet = |arc, radius| IrrigationShape::Outlet {
            at: v(100.0, 100.0),
            model: "Spray-4.0".into(),
            arc,
            direction: 30.0,
            radius,
            zone: Some(2),
        };
        vec![
            outlet(360.0, Some(4000.0)),
            outlet(120.0, Some(4000.0)),
            outlet(90.0, None),
            line(LineType::Mainline),
            line(LineType::Lateral),
            line(LineType::Drip),
            IrrigationShape::Poc {
                at: v(0.0, 0.0),
                name: "M".into(),
                static_pressure: 400.0,
                available_flow: 90.0,
                supply_loss: None,
                meter_loss: None,
            },
            IrrigationShape::Valve {
                at: v(0.0, 0.0),
                zone: 3,
                name: "V".into(),
                kv: 6.0,
                controller: Some("A".into()),
                station: Some(2),
            },
            IrrigationShape::Controller {
                at: v(0.0, 0.0),
                name: "Timer".into(),
                stations: 4,
            },
            IrrigationShape::Controller {
                at: v(0.0, 0.0),
                name: String::new(),
                stations: 4,
            },
        ]
    }

    #[test]
    fn every_object_draws_and_edits_in_a_frame() {
        let ctx = egui::Context::default();
        let view = View::default();
        let st = Style {
            pen: Color32::BLACK,
            fill: Color32::TRANSPARENT,
            width: 1.0,
        };
        let mut settings = ToolSettings::default();
        let doc = Document::default();
        let mut changed_modes = 0;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            for mut shape in sample_shapes() {
                draw(ui.painter(), &view, &shape, st, DVec2::new(5.0, 5.0));
                let before = shape.clone();
                let mut f = Fields { commit: false };
                egui::Grid::new(("edit", shape.kind(), format!("{before:?}"))).show(ui, |ui| {
                    edit(ui, &mut f, &mut shape);
                });
                assert_eq!(
                    shape, before,
                    "showing the fields does not change the object"
                );
            }
            for mode in IrrMode::ALL {
                settings.irrigation.mode = mode;
                if tool_options(ui, &mut settings, &doc) {
                    changed_modes += 1;
                }
            }
        });
        assert_eq!(changed_modes, 0);
    }

    #[test]
    fn object_info_shows_results_and_picks_catalog_parts() {
        let ctx = egui::Context::default();
        let mut s = zone();
        let mut status = String::new();
        let ids: Vec<ObjectId> = s.editor.doc().objects.keys().copied().collect();
        // Before any calculation... then after.
        for round in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                for id in &ids {
                    object_info(ui, &mut s, *id, &mut status);
                }
            });
            if round == 0 {
                calculate(&mut s, &mut status);
                // Make the result out of date for the second pass.
                s.editor
                    .execute(&Command::MoveObjects {
                        ids: vec![ids[0]],
                        delta: DVec2::X,
                    })
                    .unwrap();
            }
        }
        // The pickers do not change anything unless clicked.
        assert_eq!(s.editor.doc().objects.len(), ids.len());
    }

    #[test]
    fn catalog_parts_replace_model_and_pipe() {
        let mut s = zone();
        let mut status = String::new();
        let cat = Catalog::builtin();
        let objs: Vec<(ObjectId, IrrigationShape)> = s
            .editor
            .doc()
            .objects
            .values()
            .filter_map(|o| match &o.shape {
                Shape::Irrigation(i) => Some((o.id, i.as_ref().clone())),
                _ => None,
            })
            .collect();
        let (outlet_id, outlet) = objs
            .iter()
            .find(|(_, i)| matches!(i, IrrigationShape::Outlet { .. }))
            .unwrap()
            .clone();
        let (line_id, line) = objs
            .iter()
            .find(|(_, i)| matches!(i, IrrigationShape::Line { .. }))
            .unwrap()
            .clone();
        let rotor = cat.head("Rotor-8").unwrap();
        let n = with_head(&outlet, rotor);
        assert!(
            matches!(&n, IrrigationShape::Outlet { model, radius: Some(r), .. } if model == "Rotor-8" && *r == 8000.0)
        );
        assert_eq!(with_head(&line, rotor), line);
        let pipe = cat.pipe("40", "PE").unwrap();
        let n2 = with_pipe(&line, pipe);
        assert!(
            matches!(&n2, IrrigationShape::Line { size, material, .. } if size == "40" && material == "PE")
        );
        assert_eq!(with_pipe(&outlet, pipe), outlet);
        replace_shape(&mut s, outlet_id, n, &mut status);
        replace_shape(&mut s, line_id, n2, &mut status);
        assert!(status.is_empty(), "{status}");
        // An invalid object is refused with a message.
        let bad = IrrigationShape::Outlet {
            at: DVec2::ZERO,
            model: String::new(),
            arc: 90.0,
            direction: 0.0,
            radius: None,
            zone: None,
        };
        replace_shape(&mut s, outlet_id, bad, &mut status);
        assert!(status.contains("model"), "{status}");
    }
}
