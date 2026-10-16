//! The drawing of what an agent shows (AGR-02, AGR-03): the pointers in the plan and in 3D, the
//! small panel with Back, the list of pointers, and the row of the agent in the People palette.
//!
//! The pointers are never part of the document. A color always comes with a mark (a diamond, a
//! triangle, a square, a circle or a plus) and a line style (solid, dashed or dotted), and the
//! panel names both, so the color is not the only sign (as the pins of the issues).

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use glam::{DVec3, Mat4, Vec3, Vec4};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::uid::Uid;
use kadakas_l10n::tr;
use kadakas_ui_core::agent_view::{Aim, HColor, Highlight, HighlightSet, Line, Mark};
use kadakas_ui_core::collab::Peer;

use crate::agent_view::{AgentView, color32};
use crate::view::{View, ViewExt as _};

/// What the person did in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAction {
    Back,
    Next,
    Zoom(u32),
    Dismiss(u32),
    ClearAll,
}

/// Black or white text, whichever reads better on the color.
fn text_on(c: Color32) -> Color32 {
    let lum = 0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b());
    if lum > 140.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// The shapes of a mark at `center`, `r` points big.
pub fn mark_shapes(center: Pos2, r: f32, mark: Mark, fill: Color32, edge: Color32) -> Vec<Shape> {
    let stroke = Stroke::new(1.0, edge);
    match mark {
        Mark::Diamond => vec![Shape::convex_polygon(
            vec![
                center + vec2(0.0, -r),
                center + vec2(r, 0.0),
                center + vec2(0.0, r),
                center + vec2(-r, 0.0),
            ],
            fill,
            stroke,
        )],
        Mark::Triangle => vec![Shape::convex_polygon(
            vec![
                center + vec2(0.0, -r),
                center + vec2(r, r * 0.8),
                center + vec2(-r, r * 0.8),
            ],
            fill,
            stroke,
        )],
        Mark::Square => vec![Shape::Rect(egui::epaint::RectShape::new(
            Rect::from_center_size(center, vec2(r * 1.6, r * 1.6)),
            0.0,
            fill,
            stroke,
            egui::StrokeKind::Middle,
        ))],
        Mark::Circle => vec![Shape::circle_filled(center, r * 0.9, fill), {
            Shape::circle_stroke(center, r * 0.9, stroke)
        }],
        Mark::Plus => {
            let t = r * 0.38;
            vec![
                Shape::rect_filled(
                    Rect::from_center_size(center, vec2(r * 2.0, t * 2.0)),
                    0.0,
                    fill,
                ),
                Shape::rect_filled(
                    Rect::from_center_size(center, vec2(t * 2.0, r * 2.0)),
                    0.0,
                    fill,
                ),
                Shape::rect_stroke(
                    Rect::from_center_size(center, vec2(r * 2.0, t * 2.0)),
                    0.0,
                    stroke,
                    egui::StrokeKind::Outside,
                ),
            ]
        }
    }
}

/// A closed outline of the rectangle, solid, dashed or dotted.
fn outline(painter: &Painter, r: Rect, stroke: Stroke, line: Line) {
    let pts = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    match line {
        Line::Solid => {
            painter.add(Shape::line(pts.to_vec(), stroke));
        }
        Line::Dashed => {
            painter.extend(Shape::dashed_line(&pts, stroke, 9.0, 5.0));
        }
        Line::Dotted => {
            painter.extend(Shape::dashed_line(&pts, stroke, 2.0, 4.0));
        }
    }
}

/// The label of a pointer: the mark, the number and the text, on the color.
fn badge(painter: &Painter, clip: Rect, anchor: Pos2, number: usize, h: &Highlight) {
    let fill = color32(h.color, 235);
    let ink = text_on(fill);
    let text = if h.text.is_empty() {
        number.to_string()
    } else {
        format!("{number}  {}", h.text)
    };
    let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), ink);
    let pad = vec2(5.0, 3.0);
    let glyph = 14.0;
    let size = vec2(glyph + 4.0 + galley.size().x, galley.size().y.max(glyph)) + pad * 2.0;
    // The badge stays inside the drawing, so a pointer outside the view shows at the edge.
    let inner = clip.shrink(2.0);
    let min = pos2(
        anchor
            .x
            .clamp(inner.min.x, (inner.max.x - size.x).max(inner.min.x)),
        anchor
            .y
            .clamp(inner.min.y, (inner.max.y - size.y).max(inner.min.y)),
    );
    let r = Rect::from_min_size(min, size);
    painter.rect_filled(r, 3.0, fill);
    painter.rect_stroke(r, 3.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    let c = pos2(r.min.x + pad.x + glyph / 2.0, r.center().y);
    painter.extend(mark_shapes(c, 5.5, h.color.mark(), ink, fill));
    painter.galley(
        pos2(
            r.min.x + pad.x + glyph + 4.0,
            r.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
}

/// Where a model point is on the screen, or `None`.
pub type Project<'a> = dyn Fn(DVec3) -> Option<Pos2> + 'a;
/// The box of an object in model coordinates.
pub type Bounds<'a> = dyn Fn(Uid) -> Option<(DVec3, DVec3)> + 'a;

fn screen_box(project: &Project, lo: DVec3, hi: DVec3) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for i in 0..8 {
        let p = project(DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        ))?;
        let r = Rect::from_min_max(p, p);
        out = Some(out.map_or(r, |o| o.union(r)));
    }
    out
}

/// Draws all pointers. `project` turns a model point into a screen point; `bounds` gives the box
/// of an object.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    set: &HighlightSet,
    project: &Project,
    bounds: &Bounds,
) {
    let painter = painter.with_clip_rect(rect);
    for (i, h) in set.items().iter().enumerate() {
        let number = i + 1;
        let main = color32(h.color, 255);
        let line = h.color.line();
        let glow = |r: Rect| {
            painter.rect_stroke(
                r.expand(6.0),
                5.0,
                Stroke::new(7.0, color32(h.color, 34)),
                egui::StrokeKind::Middle,
            );
            painter.rect_stroke(
                r.expand(3.5),
                4.0,
                Stroke::new(3.0, color32(h.color, 90)),
                egui::StrokeKind::Middle,
            );
        };
        let mut anchor: Option<Pos2> = None;
        match &h.aim {
            Aim::Objects(ids) => {
                for u in ids {
                    let Some((lo, hi)) = bounds(*u) else { continue };
                    let Some(r) = screen_box(project, lo, hi) else {
                        continue;
                    };
                    let r = r.expand(1.5);
                    glow(r);
                    outline(&painter, r, Stroke::new(2.0, main), line);
                    let top = r.min + vec2(0.0, -24.0);
                    anchor = Some(match anchor {
                        Some(a) => pos2(a.x.min(top.x), a.y.min(top.y)),
                        None => top,
                    });
                }
            }
            Aim::Box { lo, hi } => {
                let z = 0.0;
                if let Some(r) = screen_box(
                    project,
                    DVec3::new(lo.x, lo.y, z),
                    DVec3::new(hi.x, hi.y, z),
                ) {
                    painter.rect_filled(r, 2.0, color32(h.color, 22));
                    glow(r);
                    outline(&painter, r, Stroke::new(2.0, main), line);
                    anchor = Some(r.min + vec2(0.0, -24.0));
                }
            }
            Aim::At(p) => {
                if let Some(at) = project(*p) {
                    painter.circle_stroke(at, 21.0, Stroke::new(6.0, color32(h.color, 50)));
                    painter.circle_stroke(at, 14.0, Stroke::new(2.5, main));
                    painter.extend(mark_shapes(at, 6.0, h.color.mark(), main, Color32::WHITE));
                    anchor = Some(at + vec2(16.0, -34.0));
                }
            }
        }
        if let Some(a) = anchor {
            badge(&painter, rect, a, number, h);
        }
    }
}

/// The pointers in the plan.
pub fn paint_plan(painter: &Painter, rect: Rect, view: &View, doc: &Document, set: &HighlightSet) {
    if set.is_empty() {
        return;
    }
    let view = *view;
    let project = move |p: DVec3| Some(view.to_screen(glam::DVec2::new(p.x, p.y)));
    let bounds = |u: Uid| {
        let o = doc.object(ObjectId(u))?;
        let (lo, hi) = doc.object_bounds(o);
        (lo.is_finite() && hi.is_finite()).then(|| (lo.extend(0.0), hi.extend(0.0)))
    };
    paint(painter, rect, set, &project, &bounds);
}

/// A model point on the screen through a view-projection matrix, or `None` behind the camera.
pub fn project_3d(vp: Mat4, rect: Rect, p: Vec3) -> Option<Pos2> {
    let c = vp * Vec4::new(p.x, p.y, p.z, 1.0);
    if c.w <= 1e-6 {
        return None;
    }
    let (x, y) = (c.x / c.w, c.y / c.w);
    (x.is_finite() && y.is_finite()).then(|| {
        pos2(
            rect.min.x + (x * 0.5 + 0.5) * rect.width(),
            rect.min.y + (0.5 - y * 0.5) * rect.height(),
        )
    })
}

/// The pointers in 3D. `vp` is the view-projection matrix of the canvas, which works in scene
/// coordinates (relative to the scene origin). The pointers use drawing coordinates (mm), so the
/// origin goes off the points first (in `f64`, then `f32`).
pub fn paint_3d(
    painter: &Painter,
    rect: Rect,
    vp: Mat4,
    scene: &kadakas_core::mesh::Scene,
    doc: &Document,
    set: &HighlightSet,
) {
    if set.is_empty() {
        return;
    }
    let origin = scene.origin;
    let project = move |p: DVec3| project_3d(vp, rect, (p - origin).as_vec3());
    let bounds = |u: Uid| {
        let id = ObjectId(u);
        scene
            .object_bounds(id)
            .filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
            .map(|(lo, hi)| (scene.to_world(lo), scene.to_world(hi)))
            .or_else(|| {
                let o = doc.object(id)?;
                let (lo, hi) = doc.object_bounds(o);
                (lo.is_finite() && hi.is_finite()).then(|| (lo.extend(0.0), hi.extend(0.0)))
            })
    };
    paint(painter, rect, set, &project, &bounds);
}

/// The cursor of the agent in 3D: the same pointer as the cursors of the others in the plan.
pub fn paint_agent_cursor_3d(
    painter: &Painter,
    rect: Rect,
    vp: Mat4,
    origin: DVec3,
    ground: f32,
    peer: &Peer,
) {
    let Some(c) = peer.cursor else { return };
    let p = DVec3::new(c[0], c[1], f64::from(ground)) - origin;
    let Some(at) = project_3d(vp, rect, p.as_vec3()) else {
        return;
    };
    if rect.expand(40.0).contains(at) {
        let [r, g, b] = peer.rgb();
        crate::collab_ui::paint_pointer(
            &painter.with_clip_rect(rect),
            at,
            Color32::from_rgb(r, g, b),
            &peer.short_label(),
        );
    }
}

/// Draws the small panel in the corner of the drawing: Back, the next pointer and the list. Gives
/// what the person chose.
pub fn panel(
    ctx: &egui::Context,
    canvas: Rect,
    state: &mut AgentView,
    back_key: &str,
    next_key: &str,
) -> Option<PanelAction> {
    if state.highlights.is_empty() && !state.can_go_back() {
        return None;
    }
    let mut action = None;
    egui::Area::new(egui::Id::new("agent-view-panel"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(canvas.right() - 8.0, canvas.top() + 8.0))
        .pivot(Align2::RIGHT_TOP)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(280.0);
                ui.horizontal(|ui| {
                    if state.can_go_back() {
                        let b = ui
                            .button(tr!("agent-view-back-count", count = state.back_len()))
                            .on_hover_text(tr!("agent-view-back-hint", key = back_key));
                        if b.clicked() {
                            action = Some(PanelAction::Back);
                        }
                    }
                    if !state.highlights.is_empty() {
                        let b = ui
                            .button(tr!("agent-view-next"))
                            .on_hover_text(tr!("agent-view-next-hint", key = next_key));
                        if b.clicked() {
                            action = Some(PanelAction::Next);
                        }
                    }
                });
                if state.highlights.is_empty() {
                    return;
                }
                egui::CollapsingHeader::new(tr!(
                    "agent-view-pointers",
                    count = state.highlights.len()
                ))
                .id_salt("agent-view-list")
                .default_open(true)
                .show(ui, |ui| {
                    for (i, h) in state.highlights.items().iter().enumerate() {
                        ui.horizontal(|ui| {
                            let (swatch, _) =
                                ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                            ui.painter().extend(mark_shapes(
                                swatch.center(),
                                6.0,
                                h.color.mark(),
                                color32(h.color, 255),
                                ui.visuals().text_color(),
                            ));
                            let row = row_text(i + 1, h);
                            let b = ui
                                .selectable_label(state.cycle == Some(h.id), row)
                                .on_hover_text(tr!("agent-view-zoom-hint"));
                            if b.clicked() {
                                action = Some(PanelAction::Zoom(h.id));
                            }
                            let x = ui
                                .small_button(tr!("agent-view-dismiss-mark"))
                                .on_hover_text(tr!("agent-view-dismiss-hint"));
                            x.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    tr!("agent-view-dismiss"),
                                )
                            });
                            if x.clicked() {
                                action = Some(PanelAction::Dismiss(h.id));
                            }
                        });
                    }
                    if ui.button(tr!("agent-view-clear-all")).clicked() {
                        action = Some(PanelAction::ClearAll);
                    }
                });
            });
        });
    action
}

/// The name of a color with its mark, in words.
pub fn color_words(c: HColor) -> String {
    match c {
        HColor::Agent => tr!("agent-view-color-agent"),
        HColor::Red => tr!("agent-view-color-red"),
        HColor::Blue => tr!("agent-view-color-blue"),
        HColor::Green => tr!("agent-view-color-green"),
        HColor::Orange => tr!("agent-view-color-orange"),
    }
}

/// The row of a pointer in the list: number, text or kind, and the color in words.
pub fn row_text(number: usize, h: &Highlight) -> String {
    let color = color_words(h.color);
    if !h.text.is_empty() {
        return tr!(
            "agent-view-row-text",
            number = number,
            text = h.text.clone(),
            color = color
        );
    }
    let what = match &h.aim {
        Aim::Objects(ids) => tr!("agent-view-what-objects", count = ids.len()),
        Aim::Box { .. } => tr!("agent-view-what-box"),
        Aim::At(_) => tr!("agent-view-what-place"),
    };
    tr!(
        "agent-view-row",
        number = number,
        what = what,
        color = color
    )
}

/// The agent in the People palette: with a mark next to the color, and the count of pointers.
pub fn people_row(ui: &mut Ui, state: &AgentView, now_ms: f64) {
    let Some(peer) = state.presence.peer(now_ms, tr!("agent-view-name")) else {
        return;
    };
    ui.horizontal(|ui| {
        let [r, g, b] = peer.rgb();
        let (swatch, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().extend(mark_shapes(
            swatch.center(),
            6.0,
            Mark::Diamond,
            Color32::from_rgb(r, g, b),
            ui.visuals().text_color(),
        ));
        let mut line = peer.label();
        if kadakas_ui_core::collab::PeerBook::is_idle(&peer, now_ms as u64) {
            line = tr!("collab-ui-quiet", name = line);
        }
        ui.label(line);
        if !state.highlights.is_empty() {
            ui.weak(tr!(
                "agent-view-people-pointing",
                count = state.highlights.len()
            ));
        }
    })
    .response
    .on_hover_text(tr!("agent-view-people-hint"));
}
