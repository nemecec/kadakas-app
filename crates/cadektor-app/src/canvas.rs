//! Painting the drawing area. Pure drawing: no document changes here.

use cadektor_core::curve;
use cadektor_core::model::{Color, Document, Object, ObjectId, Shape, Vertex, arc_sweep};
use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke};
use glam::DVec2;

use cadektor_core::plants::schedule::{TagCounts, tag_counts};

use crate::plant_draw;
use crate::snap::SnapResult;
use crate::tess;
use crate::tools::Preview;
use crate::view::{View, grid_step};

pub const SELECT_COLOR: Color32 = Color32::from_rgb(30, 120, 255);
const PREVIEW_COLOR: Color32 = Color32::from_rgb(230, 120, 20);

pub fn color32(c: Color) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

#[derive(Clone, Copy)]
pub struct Style {
    pub pen: Color32,
    pub fill: Color32,
    pub width: f32,
}

fn object_style(doc: &Document, o: &Object, zoom: f64) -> Style {
    let a = doc.resolved_attrs(o);
    let scale = doc.layer(o.layer).map_or(100.0, |l| l.scale);
    let pen = color32(a.pen.unwrap_or(Color::BLACK));
    let mut fill = a.fill.map_or(Color32::TRANSPARENT, color32);
    let tinted = |c: [u8; 4]| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    // Planting and paving get a default tint when the class has no fill.
    if fill.a() == 0 {
        fill = match o.shape {
            Shape::LandscapeArea { .. } => tinted([80, 170, 80, 90]),
            Shape::Hardscape { .. } => tinted([175, 150, 115, 130]),
            Shape::Plant { .. } => tinted([70, 150, 70, 70]),
            Shape::ExistingTree { .. } => tinted([110, 130, 110, 50]),
            Shape::SiteModifier { modifier: cadektor_core::model::ModifierKind::Pad { .. }, .. } => tinted([200, 170, 120, 70]),
            _ => fill,
        };
    }
    let width = (a.line_weight.unwrap_or(0.25) * scale * zoom).clamp(1.0, 24.0) as f32;
    Style { pen, fill, width }
}

fn fill_polygon(painter: &Painter, model: &[DVec2], screen: &[Pos2], color: Color32) {
    if color.a() == 0 {
        return;
    }
    let mut mesh = Mesh::default();
    for p in screen {
        mesh.colored_vertex(*p, color);
    }
    for t in tess::triangulate(model) {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    painter.add(mesh);
}

fn arc_points(center: DVec2, r: f64, start: f64, end: f64) -> Vec<DVec2> {
    let sweep = arc_sweep(start, end);
    let n = ((sweep / std::f64::consts::TAU * 96.0).ceil() as usize).max(4);
    (0..=n).map(|i| center + DVec2::from_angle(start + sweep * i as f64 / n as f64) * r).collect()
}

/// Chord tolerance for drawing curves: a quarter of a screen pixel, in model units.
fn curve_tolerance(zoom: f64) -> f64 {
    (0.25 / zoom).max(1e-3)
}

/// Draws one shape. `offset` is added to every model point (used for drag ghosts).
pub fn draw_shape(painter: &Painter, view: &View, shape: &Shape, st: Style, offset: DVec2) {
    let s = |p: DVec2| view.to_screen(p + offset);
    let stroke = Stroke::new(st.width, st.pen);
    let path = |pts: &[DVec2], closed: bool| {
        painter.add(PathShape { points: pts.iter().map(|p| s(*p)).collect(), closed, fill: Color32::TRANSPARENT, stroke: PathStroke::new(st.width, st.pen) });
    };
    match shape {
        Shape::Line { a, b } => {
            painter.line_segment([s(*a), s(*b)], stroke);
        }
        Shape::Rect { min, max } => {
            let r = Rect::from_two_pos(s(*min), s(*max));
            painter.rect_filled(r, 0.0, st.fill);
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
        }
        Shape::Circle { center, radius } => {
            painter.circle(s(*center), (*radius * view.zoom) as f32, st.fill, stroke);
        }
        Shape::Arc { center, radius, start, end } => path(&arc_points(*center, *radius, *start, *end), false),
        Shape::Polyline { points, closed } => {
            let flat = curve::flatten(points, *closed, curve_tolerance(view.zoom));
            if *closed {
                let sp: Vec<Pos2> = flat.iter().map(|p| s(*p)).collect();
                fill_polygon(painter, &flat, &sp, st.fill);
            }
            path(&flat, *closed);
        }
        Shape::Locus3d { p } => {
            let c = s(p.truncate());
            painter.line_segment([c - egui::vec2(4.0, 0.0), c + egui::vec2(4.0, 0.0)], stroke);
            painter.line_segment([c - egui::vec2(0.0, 4.0), c + egui::vec2(0.0, 4.0)], stroke);
        }
        Shape::Polyline3d { points, closed } => {
            let flat: Vec<DVec2> = points.iter().map(|p| p.truncate()).collect();
            if *closed {
                let sp: Vec<Pos2> = flat.iter().map(|p| s(*p)).collect();
                fill_polygon(painter, &flat, &sp, st.fill);
            }
            path(&flat, *closed);
        }
        Shape::Text { at, text, height } => {
            let size = (*height * view.zoom) as f32;
            if size >= 3.0 {
                painter.text(s(*at), egui::Align2::LEFT_BOTTOM, text, egui::FontId::proportional(size.min(400.0)), st.pen);
            } else {
                painter.circle_filled(s(*at), 1.5, st.pen);
            }
        }
        Shape::Plant { at, spread, .. } => {
            let c = s(*at);
            let r = (*spread / 2.0 * view.zoom) as f32;
            painter.circle(c, r, st.fill, stroke);
            let k = (r * 0.25).clamp(3.0, 8.0);
            painter.line_segment([c - egui::vec2(k, 0.0), c + egui::vec2(k, 0.0)], stroke);
            painter.line_segment([c - egui::vec2(0.0, k), c + egui::vec2(0.0, k)], stroke);
        }
        Shape::SiteModel { .. } => {
            let outline = cadektor_core::svg::site_outline(shape);
            path(&outline, true);
        }
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } | Shape::SiteModifier { boundary, .. } => {
            let flat = curve::flatten(boundary, true, curve_tolerance(view.zoom));
            let sp: Vec<Pos2> = flat.iter().map(|p| s(*p)).collect();
            fill_polygon(painter, &flat, &sp, st.fill);
            path(&flat, true);
        }
        Shape::ExistingTree { .. } => plant_draw::draw_tree(painter, view, shape, st, offset),
        Shape::PlantTag { at, anchor, height, .. } => plant_draw::draw_tag(painter, view, *at, *anchor, *height, "?", st, offset),
        // Sheet layer objects are drawn from the display list of the sheet (see `sheets_ui`).
        Shape::Viewport(_) | Shape::TitleBlock(_) => {}
    }
}

/// Draws an object. Plants and tags need the document: a plant draws the outline and colors
/// of its style, and a tag shows the live ID code and count.
pub fn draw_object(painter: &Painter, view: &View, doc: &Document, counts: &TagCounts, o: &Object, st: Style, offset: DVec2) {
    match &o.shape {
        Shape::Plant { at, spread, .. } => {
            let look = cadektor_core::plants::link::look(doc, o);
            // The colors of the style apply when the object has none of its own.
            let soft = |c: Color| {
                let [r, g, b, a] = c.0;
                Color32::from_rgba_unmultiplied(r, g, b, a.min(110))
            };
            let mut st = st;
            if o.attrs.fill.is_none()
                && let Some(c) = look.fill
            {
                st.fill = soft(c);
            }
            if o.attrs.pen.is_none()
                && let Some(c) = look.pen
            {
                st.pen = color32(c);
            }
            plant_draw::draw_plant(painter, view, &look, *at, *spread, st, offset);
        }
        Shape::PlantTag { at, anchor, height, .. } => {
            let text = cadektor_core::plants::schedule::tag_info_with(doc, o.id, counts).map_or_else(|| "?".to_string(), |t| t.text());
            plant_draw::draw_tag(painter, view, *at, *anchor, *height, &text, st, offset);
        }
        shape => draw_shape(painter, view, shape, st, offset),
    }
}

/// Small squares at the vertices of a selected path. Filled: on the curve (corner, point on arc).
/// Hollow: the curve is shaped by the point but may not pass through it (arc, Bezier, cubic).
fn draw_vertex_markers(painter: &Painter, view: &View, vertices: &[Vertex]) {
    for v in vertices.iter().take(1000) {
        let r = Rect::from_center_size(view.to_screen(v.p), egui::vec2(6.0, 6.0));
        if matches!(v.kind, cadektor_core::model::VertexKind::Corner | cadektor_core::model::VertexKind::PointOnArc) {
            painter.rect_filled(r, 0.0, SELECT_COLOR);
        } else {
            painter.rect(r, 0.0, Color32::WHITE, Stroke::new(1.0, SELECT_COLOR), egui::StrokeKind::Middle);
        }
    }
}

fn draw_grid(painter: &Painter, view: &View, rect: Rect) {
    let step = grid_step(view.zoom, 12.0);
    let (lo, hi) = view.visible_model(rect);
    let minor = Stroke::new(1.0, Color32::from_gray(228));
    let major = Stroke::new(1.0, Color32::from_gray(205));
    let mut x = (lo.x / step).floor() * step;
    while x <= hi.x {
        let is_major = ((x / step).round() as i64) % 10 == 0;
        let sx = view.to_screen(DVec2::new(x, 0.0)).x;
        painter.vline(sx, rect.y_range(), if is_major { major } else { minor });
        x += step;
    }
    let mut y = (lo.y / step).floor() * step;
    while y <= hi.y {
        let is_major = ((y / step).round() as i64) % 10 == 0;
        let sy = view.to_screen(DVec2::new(0.0, y)).y;
        painter.hline(rect.x_range(), sy, if is_major { major } else { minor });
        y += step;
    }
    // Origin axes.
    let o = view.to_screen(DVec2::ZERO);
    painter.vline(o.x, rect.y_range(), Stroke::new(1.0, Color32::from_rgb(220, 120, 120)));
    painter.hline(rect.x_range(), o.y, Stroke::new(1.0, Color32::from_rgb(120, 170, 120)));
}

pub struct Frame<'a> {
    pub doc: &'a Document,
    pub selection: &'a [ObjectId],
    pub preview: &'a Preview,
    pub snap: Option<SnapResult>,
    pub show_grid: bool,
    /// Cache of site model results and the document revision, to draw contours.
    pub site: Option<(&'a cadektor_site::SiteCache, u64)>,
    /// The page of the active sheet layer. The canvas shows the sheet instead of the model.
    pub sheet: Option<(&'a crate::sheets_ui::SheetUi, &'a cadektor_print::Page)>,
}

pub fn paint(painter: &Painter, rect: Rect, view: &View, f: &Frame) {
    let painter = painter.with_clip_rect(rect);
    if let Some((sheets, page)) = f.sheet {
        painter.rect_filled(rect, 0.0, Color32::from_gray(205));
        sheets.paint(&painter, view, page);
        for o in f.doc.objects.values().filter(|o| f.selection.contains(&o.id)) {
            let (lo, hi) = o.shape.bounds();
            painter.rect_stroke(Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi)).expand(2.0), 0.0, Stroke::new(1.5, SELECT_COLOR), egui::StrokeKind::Middle);
        }
        draw_preview(&painter, view, f);
        return;
    }
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    if f.show_grid {
        draw_grid(&painter, view, rect);
    }
    // Tag counts are made once per frame, not once per tag.
    let counts = tag_counts(f.doc);
    for o in f.doc.objects.values().filter(|o| f.doc.is_visible(o)) {
        let st = object_style(f.doc, o, view.zoom);
        let selected = f.selection.contains(&o.id);
        let locked = f.doc.layer(o.layer).is_some_and(|l| l.locked);
        let st = if locked { Style { pen: st.pen.gamma_multiply(0.6), ..st } } else { st };
        draw_object(&painter, view, f.doc, &counts, o, st, DVec2::ZERO);
        if let (Shape::SiteModel { .. }, Some((cache, revision))) = (&o.shape, f.site) {
            crate::site_ui::draw_contours(&painter, view, f.doc, o, cache, revision, st.width);
        }
        if selected {
            let hl = Style { pen: SELECT_COLOR, fill: Color32::TRANSPARENT, width: st.width + 1.5 };
            draw_shape(&painter, view, &o.shape, hl, DVec2::ZERO);
            if let Some(vs) = o.shape.vertices() {
                draw_vertex_markers(&painter, view, vs);
            }
            let (lo, hi) = o.shape.bounds();
            painter.rect_stroke(
                Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi)).expand(3.0),
                0.0,
                Stroke::new(1.0, SELECT_COLOR.gamma_multiply(0.6)),
                egui::StrokeKind::Middle,
            );
        }
    }
    draw_preview(&painter, view, f);
    if let Some(s) = f.snap {
        let c = view.to_screen(s.point);
        let stroke = Stroke::new(1.5, Color32::from_rgb(220, 40, 40));
        match s.kind {
            crate::snap::SnapKind::Grid => {
                painter.line_segment([c - egui::vec2(4.0, 0.0), c + egui::vec2(4.0, 0.0)], stroke);
                painter.line_segment([c - egui::vec2(0.0, 4.0), c + egui::vec2(0.0, 4.0)], stroke);
            }
            crate::snap::SnapKind::Vertex => {
                painter.rect_stroke(Rect::from_center_size(c, egui::vec2(9.0, 9.0)), 0.0, stroke, egui::StrokeKind::Middle);
            }
            crate::snap::SnapKind::Center => {
                painter.circle_stroke(c, 5.0, stroke);
            }
        }
    }
}

fn draw_preview(painter: &Painter, view: &View, f: &Frame) {
    let st = Style { pen: PREVIEW_COLOR, fill: PREVIEW_COLOR.gamma_multiply(0.12), width: 1.5 };
    match f.preview {
        Preview::None => {}
        Preview::Shape(shape) => draw_shape(painter, view, shape, st, DVec2::ZERO),
        Preview::Path { vertices, closed } => {
            let points = curve::flatten(vertices, *closed, curve_tolerance(view.zoom));
            let sp: Vec<Pos2> = points.iter().map(|p| view.to_screen(*p)).collect();
            if *closed && points.len() >= 3 {
                fill_polygon(painter, &points, &sp, st.fill);
            }
            painter.add(PathShape { points: sp, closed: *closed, fill: Color32::TRANSPARENT, stroke: PathStroke::new(1.5, PREVIEW_COLOR) });
        }
        Preview::Box { min, max, crossing } => {
            let r = Rect::from_two_pos(view.to_screen(*min), view.to_screen(*max));
            let c = if *crossing { Color32::from_rgb(40, 160, 70) } else { SELECT_COLOR };
            painter.rect(r, 0.0, c.gamma_multiply(0.1), Stroke::new(1.0, c), egui::StrokeKind::Middle);
        }
        Preview::Move { ids, delta } => {
            let counts = tag_counts(f.doc);
            for o in f.doc.objects.values().filter(|o| ids.contains(&o.id)) {
                draw_object(painter, view, f.doc, &counts, o, st, *delta);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_and_multi_turn_arcs_use_core_sweep() {
        use std::f64::consts::{PI, TAU};
        let full = arc_points(DVec2::ZERO, 10.0, 0.5, 0.5 + TAU);
        assert!(full.len() > 50, "a full circle is not a dot");
        let three_pi = arc_points(DVec2::ZERO, 10.0, 0.0, 3.0 * PI);
        assert!(three_pi.len() > 50 && three_pi.last().unwrap().distance(DVec2::new(10.0, 0.0)) < 1e-9);
    }

    #[test]
    fn arc_sampling_hits_endpoints() {
        let pts = arc_points(DVec2::ZERO, 10.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert!(pts[0].distance(DVec2::new(10.0, 0.0)) < 1e-9);
        assert!(pts.last().unwrap().distance(DVec2::new(0.0, 10.0)) < 1e-9);
    }

    #[test]
    fn color_conversion() {
        assert_eq!(color32(Color([255, 0, 0, 255])), Color32::from_rgb(255, 0, 0));
        assert_eq!(color32(Color::NONE).a(), 0);
    }
}
