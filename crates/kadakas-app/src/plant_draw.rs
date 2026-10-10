//! Drawing of plants, existing trees and plant tags in the plan view.

use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Stroke};
use glam::DVec2;
use kadakas_core::model::Shape;
use kadakas_core::plants::graphic::{self, GraphicPath};
use kadakas_core::plants::link::Look;
use kadakas_core::plants::style::PlantGraphic;
use kadakas_core::plants::tree::{self, TreeAction};

use crate::canvas::Style;
use crate::view::View;

/// The circles of an existing tree in the plan, in model units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeParts {
    pub rpz_radius: f64,
    pub canopy_radius: f64,
    pub trunk_radius: f64,
    /// A tree to remove is drawn dashed, with a cross.
    pub removed: bool,
}

/// The parts of an existing tree, or `None` for another shape.
pub fn tree_parts(shape: &Shape) -> Option<TreeParts> {
    match shape {
        Shape::ExistingTree {
            dbh,
            spread,
            rpz_factor,
            rpz_cap,
            action,
            ..
        } => Some(TreeParts {
            rpz_radius: tree::rpz_radius(*dbh, *rpz_factor, *rpz_cap),
            canopy_radius: spread / 2.0,
            trunk_radius: dbh / 2.0,
            removed: *action == TreeAction::Remove,
        }),
        _ => None,
    }
}

fn screen_path(view: &View, p: &GraphicPath, offset: DVec2) -> Vec<Pos2> {
    p.points
        .iter()
        .map(|q| view.to_screen(*q + offset))
        .collect()
}

/// Draws a plant with a generated outline, turned by its rotation. A circle graphic keeps the plain
/// circle with a center mark. A tree with a protection zone also gets the dashed zone circle.
pub fn draw_plant(
    painter: &Painter,
    view: &View,
    look: &Look,
    at: DVec2,
    spread: f64,
    st: Style,
    offset: DVec2,
) {
    let graphic = look.graphic;
    if let Some(zone) = look.zone {
        let thin = Stroke::new((st.width * 0.7).max(1.0), st.pen.gamma_multiply(0.7));
        dashed_circle(
            painter,
            view.to_screen(at + offset),
            (zone * view.zoom) as f32,
            thin,
        );
    }
    let stroke = Stroke::new(st.width, st.pen);
    let c = view.to_screen(at + offset);
    let r = (spread / 2.0 * view.zoom) as f32;
    if graphic == PlantGraphic::Circle {
        painter.circle(c, r, st.fill, stroke);
        let k = (r * 0.25).clamp(3.0, 8.0);
        painter.line_segment([c - egui::vec2(k, 0.0), c + egui::vec2(k, 0.0)], stroke);
        painter.line_segment([c - egui::vec2(0.0, k), c + egui::vec2(0.0, k)], stroke);
        return;
    }
    for (i, p) in graphic::paths_rotated(graphic, at, spread / 2.0, look.rotation)
        .iter()
        .enumerate()
    {
        let points = screen_path(view, p, offset);
        let fill = if i == 0 {
            st.fill
        } else {
            Color32::TRANSPARENT
        };
        painter.add(PathShape {
            points,
            closed: p.closed,
            fill,
            stroke: PathStroke::new(st.width, st.pen),
        });
    }
}

fn dashed_circle(painter: &Painter, c: Pos2, r: f32, stroke: Stroke) {
    let n = ((r * 0.6) as usize).clamp(24, 200);
    let pts: Vec<Pos2> = (0..=n)
        .map(|k| c + egui::Vec2::angled(std::f32::consts::TAU * k as f32 / n as f32) * r)
        .collect();
    painter.extend(egui::Shape::dashed_line(&pts, stroke, 6.0, 4.0));
}

/// Draws an existing tree: the root protection zone (dashed), the canopy and the trunk.
/// A tree to remove has a dashed canopy and a cross.
pub fn draw_tree(painter: &Painter, view: &View, shape: &Shape, st: Style, offset: DVec2) {
    let (Some(parts), Shape::ExistingTree { at, .. }) = (tree_parts(shape), shape) else {
        return;
    };
    let stroke = Stroke::new(st.width, st.pen);
    let thin = Stroke::new((st.width * 0.7).max(1.0), st.pen.gamma_multiply(0.7));
    let c = view.to_screen(*at + offset);
    let z = view.zoom as f32;
    dashed_circle(painter, c, parts.rpz_radius as f32 * z, thin);
    let canopy = parts.canopy_radius as f32 * z;
    if parts.removed {
        dashed_circle(painter, c, canopy, stroke);
        let k = canopy * std::f32::consts::FRAC_1_SQRT_2;
        for (a, b) in [
            (egui::vec2(-k, -k), egui::vec2(k, k)),
            (egui::vec2(-k, k), egui::vec2(k, -k)),
        ] {
            painter.extend(egui::Shape::dashed_line(&[c + a, c + b], stroke, 6.0, 4.0));
        }
    } else {
        painter.circle(c, canopy, st.fill, stroke);
    }
    painter.circle_filled(c, (parts.trunk_radius as f32 * z).max(1.5), st.pen);
}

#[allow(clippy::too_many_arguments)]
/// Draws a plant tag: a leader line from `anchor` to the label at `at`.
pub fn draw_tag(
    painter: &Painter,
    view: &View,
    at: DVec2,
    anchor: DVec2,
    height: f64,
    layout: Option<&kadakas_core::plants::tag::TagLayout>,
    st: Style,
    offset: DVec2,
) {
    let stroke = Stroke::new(st.width, st.pen);
    let (a, l) = (view.to_screen(anchor + offset), view.to_screen(at + offset));
    painter.line_segment([a, l], stroke);
    painter.circle_filled(a, 2.5, st.pen);
    let size = (height * view.zoom) as f32;
    let font = egui::FontId::proportional(size.clamp(0.0, 400.0));
    let Some(layout) = layout else {
        if size >= 3.0 {
            painter.text(l, egui::Align2::LEFT_BOTTOM, "?", font, st.pen);
        }
        return;
    };
    if let Some((c, r)) = layout.bubble {
        painter.circle_stroke(
            view.to_screen(at + offset + c),
            (r * view.zoom) as f32,
            stroke,
        );
    }
    if size >= 3.0 {
        for t in &layout.texts {
            let anchor = if t.centered {
                egui::Align2::CENTER_CENTER
            } else {
                egui::Align2::LEFT_BOTTOM
            };
            painter.text(
                view.to_screen(at + offset + t.at),
                anchor,
                &t.text,
                font.clone(),
                st.pen,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::plants::tree::TreeCondition;

    #[test]
    fn tree_parts_follow_the_shape() {
        let t = |action, factor| Shape::ExistingTree {
            at: DVec2::ZERO,
            species: String::new(),
            dbh: 400.0,
            spread: 6000.0,
            height: 9000.0,
            condition: TreeCondition::Good,
            action,
            rpz_factor: factor,
            rpz_cap: None,
        };
        let p = tree_parts(&t(TreeAction::Retain, None)).unwrap();
        assert_eq!(
            (p.rpz_radius, p.canopy_radius, p.trunk_radius, p.removed),
            (4800.0, 3000.0, 200.0, false)
        );
        let p = tree_parts(&t(TreeAction::Remove, Some(6.0))).unwrap();
        assert_eq!((p.rpz_radius, p.removed), (2400.0, true));
        assert!(!tree_parts(&t(TreeAction::Relocate, None)).unwrap().removed);
        assert!(
            tree_parts(&Shape::Locus3d {
                p: glam::DVec3::ZERO
            })
            .is_none()
        );
    }

    #[test]
    fn drawing_runs_for_every_graphic_and_tree_state() {
        // Paint into a context without a window: the shapes only need to be built.
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| {
            let painter = ui.painter().clone();
            let view = View::default();
            let st = Style {
                pen: Color32::BLACK,
                fill: Color32::GREEN,
                width: 1.0,
            };
            for g in PlantGraphic::ALL {
                let look = Look {
                    graphic: g,
                    rotation: 0.7,
                    zone: Some(5000.0),
                    ..Look::default()
                };
                draw_plant(
                    &painter,
                    &view,
                    &look,
                    DVec2::ZERO,
                    2000.0,
                    st,
                    DVec2::new(5.0, 5.0),
                );
            }
            for action in TreeAction::ALL {
                let shape = Shape::ExistingTree {
                    at: DVec2::ZERO,
                    species: String::new(),
                    dbh: 400.0,
                    spread: 6000.0,
                    height: 9000.0,
                    condition: TreeCondition::Poor,
                    action,
                    rpz_factor: None,
                    rpz_cap: None,
                };
                draw_tree(&painter, &view, &shape, st, DVec2::ZERO);
            }
            draw_tree(
                &painter,
                &view,
                &Shape::Locus3d {
                    p: glam::DVec3::ZERO,
                },
                st,
                DVec2::ZERO,
            );
            let info = kadakas_core::plants::tag::TagInfo {
                code: "AC".into(),
                count: 3,
                name: "Maple".into(),
                details: "C10".into(),
                format: kadakas_core::plants::tag::TagFormat {
                    levels: 3,
                    bubble: true,
                },
            };
            draw_tag(
                &painter,
                &view,
                DVec2::new(10.0, 10.0),
                DVec2::ZERO,
                400.0,
                Some(&info.layout(400.0)),
                st,
                DVec2::ZERO,
            );
            draw_tag(
                &painter,
                &view,
                DVec2::new(10.0, 10.0),
                DVec2::ZERO,
                0.001,
                None,
                st,
                DVec2::ZERO,
            );
        });
    }
}
