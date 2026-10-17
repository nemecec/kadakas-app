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

/// The label of a pointer: the mark, the number (the ID that the agent got back) and the text,
/// on the color. Labels do not lie on top of each other, and a label never covers its own target
/// (`target`) or the target of an earlier pointer: it goes above, below, right or left of it.
fn badge(
    painter: &Painter,
    clip: Rect,
    anchor: Pos2,
    h: &Highlight,
    placed: &mut Vec<Rect>,
    target: Option<Rect>,
) -> Rect {
    let fill = color32(h.color, 235);
    let ink = text_on(fill);
    let text = if h.text.is_empty() {
        h.id.to_string()
    } else {
        format!("{}  {}", h.id, h.text)
    };
    let galley = painter.layout_no_wrap(text, FontId::proportional(12.0), ink);
    let pad = vec2(5.0, 3.0);
    let glyph = 14.0;
    let size = vec2(glyph + 4.0 + galley.size().x, galley.size().y.max(glyph)) + pad * 2.0;
    // The badge stays inside the drawing, so a pointer outside the view shows at the edge.
    let inner = clip.shrink(2.0);
    let mut min = pos2(
        anchor
            .x
            .clamp(inner.min.x, (inner.max.x - size.x).max(inner.min.x)),
        anchor
            .y
            .clamp(inner.min.y, (inner.max.y - size.y).max(inner.min.y)),
    );
    let clamp = |p: Pos2| {
        pos2(
            p.x.clamp(inner.min.x, (inner.max.x - size.x).max(inner.min.x)),
            p.y.clamp(inner.min.y, (inner.max.y - size.y).max(inner.min.y)),
        )
    };
    let free = |min: Pos2| {
        let r = Rect::from_min_size(min, size);
        !placed.iter().any(|p| p.intersects(r)) && !target.is_some_and(|t| t.intersects(r))
    };
    // The label of a pointer sits where it was asked to, unless that covers the target: then it
    // goes to the side of the target where there is room.
    if let Some(t) = target
        && !free(min)
    {
        let gap = 4.0;
        let candidates = [
            pos2(min.x, t.min.y - size.y - gap),
            pos2(min.x, t.max.y + gap),
            pos2(t.max.x + gap, min.y),
            pos2(t.min.x - size.x - gap, min.y),
        ];
        if let Some(c) = candidates.iter().map(|c| clamp(*c)).find(|c| free(*c)) {
            min = c;
        }
    }
    for _ in 0..40 {
        let r = Rect::from_min_size(min, size);
        if free(min) {
            break;
        }
        min.y += size.y + 2.0;
        if min.y + size.y > inner.max.y {
            min.y = inner.min.y;
            min.x = (min.x + size.x + 2.0).min((inner.max.x - size.x).max(inner.min.x));
        }
    }
    let r = Rect::from_min_size(min, size);
    placed.push(r);
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
    r
}

/// Where a model point is on the screen, or `None`.
pub type Project<'a> = dyn Fn(DVec3) -> Option<Pos2> + 'a;
/// The box of an object in model coordinates; `None` for an object that is not shown (its layer
/// or class is off): nothing is drawn around what the person cannot see.
pub type Bounds<'a> = dyn Fn(Uid) -> Option<(DVec3, DVec3)> + 'a;

/// The corners of a box on the screen: the rectangle around them, or `Err(())` when a corner is
/// behind the camera.
fn screen_box(project: &Project, lo: DVec3, hi: DVec3) -> Result<Rect, ()> {
    let mut out: Option<Rect> = None;
    for i in 0..8 {
        let p = project(DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        ))
        .ok_or(())?;
        let r = Rect::from_min_max(p, p);
        out = Some(out.map_or(r, |o| o.union(r)));
    }
    out.ok_or(())
}

/// An arrow at the edge of the drawing that points toward something outside it.
fn edge_arrow(painter: &Painter, rect: Rect, toward: Pos2, color: Color32) -> Pos2 {
    let c = rect.center();
    let d = toward - c;
    let d = if d.length() < 1.0 {
        vec2(0.0, 1.0)
    } else {
        d.normalized()
    };
    let inner = rect.shrink(14.0);
    // The point where the ray from the middle meets the inner rectangle.
    let tx = if d.x.abs() > 1e-6 {
        ((if d.x > 0.0 { inner.max.x } else { inner.min.x }) - c.x) / d.x
    } else {
        f32::INFINITY
    };
    let ty = if d.y.abs() > 1e-6 {
        ((if d.y > 0.0 { inner.max.y } else { inner.min.y }) - c.y) / d.y
    } else {
        f32::INFINITY
    };
    let at = c + d * tx.min(ty);
    let side = vec2(-d.y, d.x);
    painter.add(Shape::convex_polygon(
        vec![
            at + d * 9.0,
            at - d * 5.0 + side * 8.0,
            at - d * 5.0 - side * 8.0,
        ],
        color,
        Stroke::new(1.0, Color32::WHITE),
    ));
    at
}

/// Draws all pointers. `project` turns a model point into a screen point; `bounds` gives the box
/// of an object. A pointer outside the drawing (or behind the camera) shows as an arrow at the
/// edge that points toward it, with its label.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    set: &HighlightSet,
    project: &Project,
    bounds: &Bounds,
) {
    let painter = painter.with_clip_rect(rect);
    let mut placed: Vec<Rect> = Vec::new();
    // The drawn areas of the pointers, for the panel to keep clear of.
    let mut targets: Vec<Rect> = Vec::new();
    for h in set.items() {
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
        // The drawn area of the pointer, and whether a part of it is behind the camera.
        let mut area: Option<Rect> = None;
        let mut behind = false;
        let mut union = |r: Rect| area = Some(area.map_or(r, |a| a.union(r)));
        match &h.aim {
            Aim::Objects(ids) => {
                for u in ids {
                    let Some((lo, hi)) = bounds(*u) else { continue };
                    match screen_box(project, lo, hi) {
                        Ok(r) => {
                            let r = r.expand(1.5);
                            if r.intersects(rect) {
                                glow(r);
                                outline(&painter, r, Stroke::new(2.0, main), line);
                            }
                            union(r);
                        }
                        Err(()) => behind = true,
                    }
                }
            }
            Aim::Box { lo, hi } => {
                let z = h.ground;
                match screen_box(
                    project,
                    DVec3::new(lo.x, lo.y, z),
                    DVec3::new(hi.x, hi.y, z),
                ) {
                    Ok(r) => {
                        if r.intersects(rect) {
                            painter.rect_filled(r, 2.0, color32(h.color, 22));
                            glow(r);
                            outline(&painter, r, Stroke::new(2.0, main), line);
                        }
                        union(r);
                    }
                    Err(()) => behind = true,
                }
            }
            Aim::At(p) => match project(*p) {
                Some(at) => {
                    if rect.expand(30.0).contains(at) {
                        painter.circle_stroke(at, 21.0, Stroke::new(6.0, color32(h.color, 50)));
                        painter.circle_stroke(at, 14.0, Stroke::new(2.5, main));
                        painter.extend(mark_shapes(at, 6.0, h.color.mark(), main, Color32::WHITE));
                    }
                    union(Rect::from_center_size(at, vec2(2.0, 2.0)));
                }
                None => behind = true,
            },
        }
        // A part behind the camera cannot be outlined: an arrow at the bottom edge says so.
        if behind && area.is_some() {
            let toward = rect.center() + vec2(0.0, rect.height());
            edge_arrow(&painter, rect, toward, main);
        }
        match area {
            Some(a) if a.intersects(rect) => {
                let (top, covered) = match &h.aim {
                    Aim::At(_) => (a.center() + vec2(16.0, -34.0), a.expand(24.0)),
                    _ => (a.min + vec2(0.0, -24.0), a.expand(7.0)),
                };
                badge(&painter, rect, top, h, &mut placed, Some(covered));
                // Later labels keep clear of this target too.
                placed.push(covered);
                targets.push(covered);
            }
            Some(a) => {
                // Outside the drawing: an arrow at the edge, and the label beside it.
                let at = edge_arrow(&painter, rect, a.center(), main);
                badge(&painter, rect, at + vec2(-8.0, -8.0), h, &mut placed, None);
            }
            None if behind => {
                // Behind the camera: an arrow at the bottom edge.
                let toward = rect.center() + vec2(0.0, rect.height());
                let at = edge_arrow(&painter, rect, toward, main);
                badge(&painter, rect, at + vec2(-8.0, -30.0), h, &mut placed, None);
            }
            None => {}
        }
    }
    remember_targets(painter.ctx(), targets);
}

fn targets_id() -> egui::Id {
    egui::Id::new("agent-view-targets")
}

/// The areas that the pointers cover on the screen in this frame; the panel reads them in the
/// next frame (and the tests read them).
fn remember_targets(ctx: &egui::Context, targets: Vec<Rect>) {
    ctx.data_mut(|d| d.insert_temp(targets_id(), targets));
}

/// The targets of the last frame.
pub fn targets_of(ctx: &egui::Context) -> Vec<Rect> {
    ctx.data(|d| d.get_temp::<Vec<Rect>>(targets_id()))
        .unwrap_or_default()
}

/// The pointers in the plan.
pub fn paint_plan(painter: &Painter, rect: Rect, view: &View, doc: &Document, set: &HighlightSet) {
    if set.is_empty() {
        remember_targets(painter.ctx(), Vec::new());
        return;
    }
    let view = *view;
    let project = move |p: DVec3| Some(view.to_screen(glam::DVec2::new(p.x, p.y)));
    let bounds = |u: Uid| {
        let o = doc.object(ObjectId(u))?;
        if !doc.is_displayed(o) {
            return None;
        }
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
        remember_targets(painter.ctx(), Vec::new());
        return;
    }
    let origin = scene.origin;
    let project = move |p: DVec3| project_3d(vp, rect, (p - origin).as_vec3());
    let bounds = |u: Uid| {
        let id = ObjectId(u);
        if !doc.object(id).is_some_and(|o| doc.is_displayed(o)) {
            return None;
        }
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
    if state.panel_hidden || (state.highlights.is_empty() && !state.can_go_back()) {
        return None;
    }
    let mut action = None;
    let mut allow = !state.locked;
    let mut hide = false;
    // The panel keeps clear of what the pointers mark: the first corner of the drawing where it
    // covers none of them (the top right, then the top left and the bottom corners).
    let size_id = egui::Id::new("agent-view-panel-size");
    let size = ctx
        .data(|d| d.get_temp::<egui::Vec2>(size_id))
        .unwrap_or(vec2(280.0, 120.0));
    let targets = targets_of(ctx);
    let corners = [
        (
            Align2::RIGHT_TOP,
            pos2(canvas.right() - 8.0, canvas.top() + 8.0),
        ),
        (
            Align2::LEFT_TOP,
            pos2(canvas.left() + 8.0, canvas.top() + 8.0),
        ),
        (
            Align2::RIGHT_BOTTOM,
            pos2(canvas.right() - 8.0, canvas.bottom() - 8.0),
        ),
        (
            Align2::LEFT_BOTTOM,
            pos2(canvas.left() + 8.0, canvas.bottom() - 8.0),
        ),
    ];
    let (pivot, pos) = corners
        .iter()
        .find(|(pivot, pos)| {
            let r = pivot.anchor_size(*pos, size);
            !targets.iter().any(|t| t.intersects(r))
        })
        .copied()
        .unwrap_or(corners[0]);
    let area = egui::Area::new(egui::Id::new("agent-view-panel"))
        // Under the dialogs and the menus of the window, not over them.
        .order(egui::Order::Middle)
        .fixed_pos(pos)
        .pivot(pivot)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(280.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut allow, tr!("agent-view-allow-move"))
                        .on_hover_text(tr!("agent-view-allow-move-hint"));
                    if ui
                        .small_button(tr!("agent-view-close"))
                        .on_hover_text(tr!("agent-view-close-hint"))
                        .clicked()
                    {
                        hide = true;
                    }
                });
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
                    for h in state.highlights.items() {
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
                            let row = row_text(h.id as usize, h);
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
    ctx.data_mut(|d| d.insert_temp(size_id, area.response.rect.size()));
    state.locked = !allow;
    state.panel_hidden |= hide;
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
pub fn people_row(ui: &mut Ui, state: &mut AgentView, now_ms: f64) {
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
    // The person decides whether the agent may move the view.
    let mut allow = !state.locked;
    ui.checkbox(&mut allow, tr!("agent-view-allow-move"))
        .on_hover_text(tr!("agent-view-allow-move-hint"));
    state.locked = !allow;
}
