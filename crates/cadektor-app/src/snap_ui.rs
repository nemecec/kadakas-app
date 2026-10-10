//! The Snapping palette (a tab of the dock) and the drawing of snap marks, cue labels and
//! alignment guides.

use cadektor_core::snapping::{Kind, Settings, Smart};
use egui::{Color32, FontId, Painter, Pos2, Rect, Stroke, Ui, pos2, vec2};

use crate::snap::SnapResult;
use crate::view::View;

/// What each category of the palette is called and what it does.
pub const CATEGORIES: [(&str, &str, &str); 11] = [
    ("grid", "Grid", "Snap to the points of the grid."),
    (
        "object_points",
        "Object points",
        "Endpoints, corners, centers and circle quadrants.",
    ),
    (
        "midpoints",
        "Midpoints",
        "The middle of a segment or an arc.",
    ),
    ("intersections", "Intersections", "Where two edges cross."),
    ("edges", "Edge", "The nearest point on an edge."),
    (
        "perpendicular",
        "Perpendicular",
        "The foot of the perpendicular from the last point.",
    ),
    (
        "tangent",
        "Tangent",
        "The tangent point of a circle seen from the last point.",
    ),
    (
        "angle",
        "Angle",
        "Directions at steps of the angle step from the last point.",
    ),
    (
        "distance",
        "Distance",
        "Lengths at steps of the distance step from the last point.",
    ),
    (
        "smart_points",
        "Smart points",
        "Alignment lines through points you acquired (T).",
    ),
    (
        "smart_edges",
        "Smart edges",
        "Lines along and across edges you acquired (T).",
    ),
];

/// The palette. Returns true if something changed.
pub fn palette(ui: &mut Ui, settings: &mut Settings, smart: &mut Smart, cues: &mut bool) -> bool {
    let before = (*settings, *cues);
    ui.heading("Snapping");
    ui.small("Switch the kinds of snaps. The back-quote key suspends all snaps while held.");
    ui.separator();
    for (name, label, tip) in CATEGORIES {
        let mut on = settings.set.get(name).unwrap_or(false);
        if ui.checkbox(&mut on, label).on_hover_text(tip).changed() {
            settings.set.set(name, on);
        }
        match name {
            "angle" if on => {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label("Step");
                    let mut deg = settings.angle_step.to_degrees();
                    if ui
                        .add(
                            egui::DragValue::new(&mut deg)
                                .speed(0.5)
                                .range(0.1..=180.0)
                                .suffix("\u{b0}"),
                        )
                        .changed()
                    {
                        settings.angle_step = deg.to_radians();
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.checkbox(
                        &mut settings.relative_angles,
                        "Relative to the prior segment",
                    )
                    .on_hover_text("The / key switches this.");
                });
            }
            "distance" if on => {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label("Step");
                    ui.add(
                        egui::DragValue::new(&mut settings.distance_step)
                            .speed(10.0)
                            .range(0.0..=1.0e6)
                            .suffix(" mm"),
                    )
                    .on_hover_text("0 uses the step of the grid");
                });
            }
            _ => {}
        }
    }
    ui.separator();
    ui.checkbox(&mut settings.other_layers, "Snap to other layers")
        .on_hover_text("Off: only objects of the active layer.");
    ui.checkbox(&mut settings.grey, "Snap to grey layers and classes")
        .on_hover_text("Objects of grey layers and classes cannot be picked. This lets the pointer snap to them.");
    ui.checkbox(cues, "SmartCursor cues")
        .on_hover_text("Show the name of the snap at the pointer (Y).");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(format!(
            "Smart points {}/{}  edges {}/{}",
            smart.points.len(),
            Smart::MAX_POINTS,
            smart.edges.len(),
            Smart::MAX_EDGES
        ));
        if ui
            .add_enabled(!smart.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            smart.clear();
        }
    });
    (*settings, *cues) != before
}

fn mark_color(kind: Kind, dark: bool) -> Color32 {
    match kind {
        Kind::Grid => Color32::from_rgb(150, 150, 150),
        Kind::SmartPoint | Kind::SmartEdge => Color32::from_rgb(40, 170, 90),
        Kind::Angle | Kind::Distance => Color32::from_rgb(40, 130, 220),
        _ if dark => Color32::from_rgb(255, 90, 90),
        _ => Color32::from_rgb(220, 40, 40),
    }
}

/// The mark of a snap at the screen point `c`.
pub fn draw_mark(painter: &Painter, c: Pos2, kind: Kind, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let r = 5.0;
    match kind {
        Kind::Grid => {
            painter.line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, 4.0), c + vec2(0.0, 4.0)], stroke);
        }
        Kind::Endpoint => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(9.0, 9.0)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Kind::Midpoint => {
            let pts = vec![
                c + vec2(0.0, -r),
                c + vec2(r, r * 0.8),
                c + vec2(-r, r * 0.8),
            ];
            painter.add(egui::epaint::PathShape::closed_line(pts, stroke));
        }
        Kind::Center => {
            painter.circle_stroke(c, r, stroke);
        }
        Kind::Quadrant => {
            let pts = vec![
                c + vec2(0.0, -r),
                c + vec2(r, 0.0),
                c + vec2(0.0, r),
                c + vec2(-r, 0.0),
            ];
            painter.add(egui::epaint::PathShape::closed_line(pts, stroke));
        }
        Kind::Intersection => {
            painter.line_segment([c + vec2(-r, -r), c + vec2(r, r)], stroke);
            painter.line_segment([c + vec2(-r, r), c + vec2(r, -r)], stroke);
        }
        Kind::Edge => {
            painter.circle_stroke(c, 3.5, stroke);
            painter.line_segment([c + vec2(-r - 2.0, 0.0), c + vec2(-3.5, 0.0)], stroke);
            painter.line_segment([c + vec2(3.5, 0.0), c + vec2(r + 2.0, 0.0)], stroke);
        }
        Kind::Perpendicular => {
            painter.line_segment([c + vec2(-r, r), c + vec2(r, r)], stroke);
            painter.line_segment([c + vec2(0.0, r), c + vec2(0.0, -r)], stroke);
            painter.line_segment([c + vec2(0.0, 1.0), c + vec2(3.0, 1.0)], stroke);
            painter.line_segment([c + vec2(3.0, 1.0), c + vec2(3.0, r)], stroke);
        }
        Kind::Tangent => {
            painter.circle_stroke(c + vec2(0.0, -3.0), 3.5, stroke);
            painter.line_segment([c + vec2(-r - 1.0, 1.0), c + vec2(r + 1.0, 1.0)], stroke);
        }
        Kind::Angle | Kind::Distance | Kind::SmartPoint | Kind::SmartEdge => {
            painter.circle_stroke(c, 3.0, stroke);
            painter.line_segment([c - vec2(r + 2.0, 0.0), c + vec2(r + 2.0, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, r + 2.0), c + vec2(0.0, r + 2.0)], stroke);
        }
    }
}

/// A small label next to the pointer.
pub fn draw_cue(painter: &Painter, at: Pos2, text: &str, dark: bool, clip: Rect) {
    let font = FontId::proportional(12.0);
    let galley = painter.layout_no_wrap(
        text.to_string(),
        font,
        if dark { Color32::WHITE } else { Color32::BLACK },
    );
    let size = galley.size() + vec2(8.0, 4.0);
    // Below and right of the pointer, or on the other side near the edge of the canvas.
    let mut min = at + vec2(14.0, 12.0);
    if min.x + size.x > clip.right() {
        min.x = at.x - 14.0 - size.x;
    }
    if min.y + size.y > clip.bottom() {
        min.y = at.y - 12.0 - size.y;
    }
    let rect = Rect::from_min_size(min, size);
    let bg = if dark {
        Color32::from_rgba_unmultiplied(40, 44, 52, 235)
    } else {
        Color32::from_rgba_unmultiplied(255, 252, 220, 240)
    };
    painter.rect(
        rect,
        3.0,
        bg,
        Stroke::new(1.0, Color32::from_gray(if dark { 110 } else { 150 })),
        egui::StrokeKind::Middle,
    );
    painter.galley(rect.min + vec2(4.0, 2.0), galley, Color32::BLACK);
}

/// The snap: guides, mark and (with `cues`) the label.
pub fn draw_snap(
    painter: &Painter,
    view: &View,
    snap: &SnapResult,
    cues: bool,
    dark: bool,
    clip: Rect,
) {
    let color = mark_color(snap.kind, dark);
    for g in &snap.guides {
        let (a, b) = (view.to_screen(g.a), view.to_screen(g.b));
        let c = mark_color(g.kind, dark).gamma_multiply(0.8);
        painter.extend(egui::Shape::dashed_line(
            &[a, b],
            Stroke::new(1.0, c),
            6.0,
            4.0,
        ));
    }
    let c = view.to_screen(snap.point);
    draw_mark(painter, c, snap.kind, color);
    if cues && !snap.cue.is_empty() {
        draw_cue(painter, c, &snap.cue, dark, clip);
    }
}

/// The smart points and edges you acquired: a small x and a short line.
pub fn draw_smart(painter: &Painter, view: &View, smart: &Smart, dark: bool) {
    let color = mark_color(Kind::SmartPoint, dark);
    for p in &smart.points {
        let c = view.to_screen(*p);
        painter.line_segment(
            [c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)],
            Stroke::new(1.5, color),
        );
        painter.line_segment(
            [c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)],
            Stroke::new(1.5, color),
        );
        painter.circle_stroke(c, 6.0, Stroke::new(1.0, color));
    }
    for (a, b) in &smart.edges {
        let (sa, sb) = (view.to_screen(*a), view.to_screen(*b));
        let d = (sb - sa).normalized();
        let mid = pos2((sa.x + sb.x) / 2.0, (sa.y + sb.y) / 2.0);
        painter.line_segment([mid - d * 14.0, mid + d * 14.0], Stroke::new(2.5, color));
    }
}

/// The text for the status line after the pointer snapped.
pub fn status_text(snap: Option<&SnapResult>, on: bool) -> String {
    match (snap, on) {
        (Some(r), _) => format!("Snap: {}", r.cue),
        (None, true) => "Snap: none".to_string(),
        (None, false) => "Snap: off".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;

    fn result(kind: Kind) -> SnapResult {
        SnapResult {
            point: DVec2::new(10.0, 10.0),
            kind,
            cue: kind.label().to_string(),
            guides: vec![cadektor_core::snapping::Guide {
                a: DVec2::new(0.0, 10.0),
                b: DVec2::new(100.0, 10.0),
                kind,
            }],
        }
    }

    #[test]
    fn every_kind_draws_and_the_palette_changes_settings() {
        let ctx = egui::Context::default();
        let kinds = [
            Kind::Grid,
            Kind::Endpoint,
            Kind::Midpoint,
            Kind::Center,
            Kind::Quadrant,
            Kind::Intersection,
            Kind::Edge,
            Kind::Perpendicular,
            Kind::Tangent,
            Kind::Angle,
            Kind::Distance,
            Kind::SmartPoint,
            Kind::SmartEdge,
        ];
        let mut settings = Settings::default();
        let mut smart = Smart::default();
        smart.acquire_point(DVec2::new(5.0, 5.0));
        smart.acquire_edge(DVec2::ZERO, DVec2::new(10.0, 10.0));
        let mut cues = true;
        let mut changed = false;
        let _ = ctx.run_ui(Default::default(), |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter().clone();
            let view = View::default();
            for dark in [false, true] {
                for k in kinds {
                    draw_snap(&painter, &view, &result(k), true, dark, rect);
                }
                draw_smart(&painter, &view, &smart, dark);
            }
            changed = palette(ui, &mut settings, &mut smart, &mut cues);
        });
        assert!(!changed, "drawing does not change settings");
        assert_eq!(
            CATEGORIES.len(),
            cadektor_core::snapping::SnapSet::NAMES.len()
        );
        for (name, ..) in CATEGORIES {
            assert!(settings.set.get(name).is_some(), "{name}");
        }
    }

    #[test]
    fn status_text_cases() {
        let r = result(Kind::Midpoint);
        assert_eq!(status_text(Some(&r), true), "Snap: Midpoint");
        assert_eq!(status_text(None, true), "Snap: none");
        assert_eq!(status_text(None, false), "Snap: off");
    }
}
