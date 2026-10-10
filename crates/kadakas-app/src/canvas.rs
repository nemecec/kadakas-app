//! Painting the drawing area. Pure drawing: no document changes here.

use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke};
use glam::DVec2;
use kadakas_core::curve;
use kadakas_core::model::{Color, Document, Object, ObjectId, Shape, Vertex, arc_sweep};

use kadakas_core::plants::tag::{TagCounts, tag_counts};

use crate::plant_draw;
use crate::tess;
use crate::view::{View, grid_step};
use kadakas_ui_core::snap::SnapResult;
use kadakas_ui_core::tools::Preview;

pub const SELECT_COLOR: Color32 = Color32::from_rgb(30, 120, 255);
const PREVIEW_COLOR: Color32 = Color32::from_rgb(230, 120, 20);

pub fn color32(c: Color) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

thread_local! {
    /// The canvas is drawn in the dark theme: nearly black pens are drawn light.
    static DARK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The object that is drawn now is on a grey layer or class.
    static GREY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sets the theme of the canvas for the drawing code of this thread.
pub fn set_dark(dark: bool) {
    DARK.with(|d| d.set(dark));
}

fn set_grey(grey: bool) {
    GREY.with(|g| g.set(grey));
}

/// The color to draw for a color of the document: the theme and the grey mode applied.
pub fn draw_color(c: Color) -> Color32 {
    let dark = DARK.with(std::cell::Cell::get);
    let rgba = if GREY.with(std::cell::Cell::get) {
        crate::theme::grey_color(c.0, dark)
    } else {
        crate::theme::canvas_color(c.0, dark)
    };
    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
}

#[derive(Clone, Copy)]
pub struct Style {
    pub pen: Color32,
    pub fill: Color32,
    pub width: f32,
}

fn object_style(doc: &Document, o: &Object, zoom: f64) -> Style {
    let a = kadakas_core::viewrules::display_attrs(
        doc,
        o,
        kadakas_core::hardscape::styled_attrs(doc, o, doc.resolved_attrs(o)),
    );
    let scale = doc.layer(o.layer).map_or(100.0, |l| l.scale);
    let pen = draw_color(a.pen.unwrap_or(Color::BLACK));
    let mut fill = a.fill.map_or(Color32::TRANSPARENT, draw_color);
    let tinted = |c: [u8; 4]| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    // Planting and paving get a default tint when the class has no fill.
    if fill.a() == 0 {
        fill = match o.shape {
            Shape::LandscapeArea { .. } => tinted([80, 170, 80, 90]),
            Shape::Hardscape { .. } => tinted([175, 150, 115, 130]),
            Shape::Plant { .. } => tinted([70, 150, 70, 70]),
            Shape::ExistingTree { .. } => tinted([110, 130, 110, 50]),
            Shape::SiteModifier {
                modifier: kadakas_core::model::ModifierKind::Pad { .. },
                ..
            } => tinted([200, 170, 120, 70]),
            _ => fill,
        };
    }
    let width = (a.line_weight.unwrap_or(0.25) * scale * zoom).clamp(1.0, 24.0) as f32;
    Style { pen, fill, width }
}

pub(crate) fn fill_polygon(painter: &Painter, model: &[DVec2], screen: &[Pos2], color: Color32) {
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
    (0..=n)
        .map(|i| center + DVec2::from_angle(start + sweep * i as f64 / n as f64) * r)
        .collect()
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
        painter.add(PathShape {
            points: pts.iter().map(|p| s(*p)).collect(),
            closed,
            fill: Color32::TRANSPARENT,
            stroke: PathStroke::new(st.width, st.pen),
        });
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
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => path(&arc_points(*center, *radius, *start, *end), false),
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
                painter.text(
                    s(*at),
                    egui::Align2::LEFT_BOTTOM,
                    text,
                    egui::FontId::proportional(size.min(400.0)),
                    st.pen,
                );
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
            let outline = kadakas_core::svg::site_outline(shape);
            path(&outline, true);
        }
        Shape::LandscapeArea { boundary, .. }
        | Shape::Hardscape { boundary, .. }
        | Shape::SiteModifier { boundary, .. } => {
            let flat = curve::flatten(boundary, true, curve_tolerance(view.zoom));
            let sp: Vec<Pos2> = flat.iter().map(|p| s(*p)).collect();
            fill_polygon(painter, &flat, &sp, st.fill);
            path(&flat, true);
        }
        Shape::Massing(m) => {
            let flat = curve::flatten(&m.footprint, true, curve_tolerance(view.zoom));
            let sp: Vec<Pos2> = flat.iter().map(|p| s(*p)).collect();
            fill_polygon(painter, &flat, &sp, st.fill);
            path(&flat, true);
            for seg in crate::gis_ui::roof_lines(m, view, offset) {
                painter.line_segment(seg, stroke);
            }
        }
        Shape::Mesh3d(m) => {
            // Plan view: the box around the mesh. The 3D view shows the triangles.
            let (lo, hi) = m.bounds3();
            let flat = [
                lo.truncate(),
                DVec2::new(hi.x, lo.y),
                hi.truncate(),
                DVec2::new(lo.x, hi.y),
            ];
            let sp: Vec<Pos2> = flat.iter().map(|p| s(*p)).collect();
            fill_polygon(painter, &flat, &sp, st.fill);
            path(&flat, true);
        }
        Shape::Underlay(u) => crate::gis_ui::draw_underlay(painter, view, u, offset),
        Shape::ExistingTree { .. } => plant_draw::draw_tree(painter, view, shape, st, offset),
        Shape::PlantTag {
            at, anchor, height, ..
        } => plant_draw::draw_tag(painter, view, *at, *anchor, *height, None, st, offset),
        // Annotations need the document (see `draw_object`).
        Shape::Annotation(_) => {}
        // Sheet layer objects are drawn from the display list of the sheet (see `sheets_ui`).
        Shape::Viewport(_)
        | Shape::TitleBlock(_)
        | Shape::Group { .. }
        | Shape::Unknown { .. }
        | Shape::Symbol { .. } => {}
        Shape::Irrigation(i) => crate::irrigation_ui::draw(painter, view, i, st, offset),
    }
}

/// Draws an object. Plants and tags need the document: a plant draws the outline and colors
/// of its style, and a tag shows the live ID code and count.
pub fn draw_object(
    painter: &Painter,
    view: &View,
    doc: &Document,
    counts: &TagCounts,
    o: &Object,
    st: Style,
    offset: DVec2,
) {
    draw_object_deep(painter, view, doc, counts, o, st, offset, 0);
}

type DecorCache = (
    u64,
    std::collections::HashMap<(ObjectId, u64), std::rc::Rc<Option<kadakas_core::pattern::Decor>>>,
);

thread_local! {
    static DECOR: std::cell::RefCell<DecorCache> = std::cell::RefCell::new((0, Default::default()));
}

/// The pattern geometry of an object. It is kept for one revision of the document, so a hatch is not
/// computed again for every frame. Parts of a symbol (depth > 0) share an object ID and are not kept.
fn decor_cached(
    doc: &Document,
    o: &Object,
    attrs: &kadakas_core::model::Attributes,
    scale: f64,
    depth: usize,
) -> std::rc::Rc<Option<kadakas_core::pattern::Decor>> {
    let revision = if depth == 0 {
        crate::area_ui::current_revision()
    } else {
        None
    };
    let Some(rev) = revision else {
        return std::rc::Rc::new(kadakas_core::pattern::decorate(doc, o, attrs, scale));
    };
    DECOR.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != rev {
            *c = (rev, Default::default());
        }
        let key = (o.id, scale.to_bits());
        c.1.entry(key)
            .or_insert_with(|| {
                std::rc::Rc::new(kadakas_core::pattern::decorate(doc, o, attrs, scale))
            })
            .clone()
    })
}

/// Like `draw_object`, for a symbol also through the shapes of its definition. `depth` stops loops.
#[allow(clippy::too_many_arguments)]
fn draw_object_deep(
    painter: &Painter,
    view: &View,
    doc: &Document,
    counts: &TagCounts,
    o: &Object,
    st: Style,
    offset: DVec2,
    depth: usize,
) {
    if depth > kadakas_core::symbols::MAX_DEPTH + 2 {
        return;
    }
    let attrs = kadakas_core::viewrules::display_attrs(
        doc,
        o,
        kadakas_core::hardscape::styled_attrs(doc, o, doc.resolved_attrs(o)),
    );
    let scale = doc.layer(o.layer).map_or(100.0, |l| l.scale);
    let decor_rc = decor_cached(doc, o, &attrs, scale, depth);
    let decor = (*decor_rc).as_ref();
    // A line type draws the outline in dashes, so the plain outline is left out.
    let outline_st = if decor.is_some_and(|d| d.replaces_outline) {
        Style { width: 0.0, ..st }
    } else {
        st
    };
    draw_shape_or_object(painter, view, doc, counts, o, outline_st, offset, depth);
    if let Some(d) = decor {
        let s = |p: DVec2| view.to_screen(p + offset);
        for seg in &d.fill {
            let pen = seg.color.map_or(st.pen, draw_color);
            let w = seg.weight.map_or(st.width, |w| {
                (w * scale * view.zoom).clamp(1.0, 24.0) as f32
            });
            painter.line_segment(
                [s(seg.a), s(seg.b)],
                Stroke::new(w.min(st.width.max(1.0)), pen),
            );
        }
        for stroke in &d.strokes {
            painter.add(PathShape {
                points: stroke.iter().map(|p| s(*p)).collect(),
                closed: false,
                fill: Color32::TRANSPARENT,
                stroke: PathStroke::new(st.width, st.pen),
            });
        }
        for m in &d.marks {
            let inst = kadakas_core::symbols::mark_object(
                o,
                attrs.pen,
                attrs.line_weight,
                m.symbol,
                m.at,
                m.angle,
                m.scale,
            );
            draw_object_deep(painter, view, doc, counts, &inst, st, offset, depth + 1);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_shape_or_object(
    painter: &Painter,
    view: &View,
    doc: &Document,
    counts: &TagCounts,
    o: &Object,
    st: Style,
    offset: DVec2,
    depth: usize,
) {
    match &o.shape {
        Shape::Symbol { .. } => {
            for v in kadakas_core::symbols::instance_objects(doc, o) {
                let vst = object_style(doc, &v, view.zoom);
                draw_object_deep(painter, view, doc, counts, &v, vst, offset, depth + 1);
            }
        }
        Shape::Group { .. } | Shape::Unknown { .. } => {}
        Shape::Plant { at, spread, .. } => {
            let look = kadakas_core::viewrules::plant_look(doc, o);
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
                st.pen = draw_color(c);
            }
            let parts = look
                .symbol
                .map(|sym| {
                    kadakas_core::symbols::plant_symbol_objects(
                        doc,
                        o,
                        sym,
                        *at,
                        *spread,
                        look.rotation,
                    )
                })
                .unwrap_or_default();
            if parts.is_empty() {
                plant_draw::draw_plant(painter, view, &look, *at, *spread, st, offset);
            } else {
                for v in &parts {
                    let vst = Style {
                        pen: if o.attrs.pen.is_none() && v.attrs.pen.is_none() {
                            st.pen
                        } else {
                            object_style(doc, v, view.zoom).pen
                        },
                        ..object_style(doc, v, view.zoom)
                    };
                    draw_object(painter, view, doc, counts, v, vst, offset);
                }
            }
        }
        Shape::PlantTag {
            at, anchor, height, ..
        } => {
            let layout = kadakas_core::plants::tag::tag_info_with(doc, o.id, counts)
                .map(|t| t.layout(*height));
            plant_draw::draw_tag(
                painter,
                view,
                *at,
                *anchor,
                *height,
                layout.as_ref(),
                st,
                offset,
            );
        }
        Shape::Annotation(_) => crate::annot_ui::draw_object(painter, view, doc, o, st, offset),
        Shape::Hardscape { .. } | Shape::LandscapeArea { .. } => {
            draw_shape(painter, view, &o.shape, st, offset);
            crate::area_ui::draw_extras(painter, view, doc, o, st, offset);
        }
        shape => draw_shape(painter, view, shape, st, offset),
    }
}

/// Small squares at the vertices of a selected path. Filled: on the curve (corner, point on arc).
/// Hollow: the curve is shaped by the point but may not pass through it (arc, Bezier, cubic).
fn draw_vertex_markers(painter: &Painter, view: &View, vertices: &[Vertex]) {
    for v in vertices.iter().take(1000) {
        let r = Rect::from_center_size(view.to_screen(v.p), egui::vec2(6.0, 6.0));
        if matches!(
            v.kind,
            kadakas_core::model::VertexKind::Corner | kadakas_core::model::VertexKind::PointOnArc
        ) {
            painter.rect_filled(r, 0.0, SELECT_COLOR);
        } else {
            painter.rect(
                r,
                0.0,
                Color32::WHITE,
                Stroke::new(1.0, SELECT_COLOR),
                egui::StrokeKind::Middle,
            );
        }
    }
}

fn draw_grid(painter: &Painter, view: &View, rect: Rect, dark: bool) {
    let colors = crate::theme::canvas_colors(dark);
    let step = grid_step(view.zoom, 12.0);
    let (lo, hi) = view.visible_model(rect);
    let minor = Stroke::new(1.0, Color32::from_gray(colors.grid_minor));
    let major = Stroke::new(1.0, Color32::from_gray(colors.grid_major));
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
    painter.vline(
        o.x,
        rect.y_range(),
        Stroke::new(
            1.0,
            Color32::from_rgb(colors.axis_x[0], colors.axis_x[1], colors.axis_x[2]),
        ),
    );
    painter.hline(
        rect.x_range(),
        o.y,
        Stroke::new(
            1.0,
            Color32::from_rgb(colors.axis_y[0], colors.axis_y[1], colors.axis_y[2]),
        ),
    );
}

/// Clips the screen segment `a..b` to the rect. `None` if it lies outside.
fn clip_segment(a: egui::Pos2, b: egui::Pos2, r: Rect) -> Option<(egui::Pos2, egui::Pos2)> {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let d = b - a;
    for (p, q) in [
        (-d.x, a.x - r.min.x),
        (d.x, r.max.x - a.x),
        (-d.y, a.y - r.min.y),
        (d.y, r.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then(|| (a + d * t0, a + d * t1))
}

/// The northing/easting grid of the georeference (see `kadakas_core::viewrules`).
fn draw_ne_grid(painter: &Painter, view: &View, rect: Rect, doc: &Document) {
    let (Some(g), Some(grid)) = (&doc.georef, &doc.drawing.view_rules.ne_grid) else {
        return;
    };
    let (lo, hi) = view.visible_model(rect);
    let [r, gr, b, a] = grid.color.0;
    let color = Color32::from_rgba_unmultiplied(r, gr, b, a);
    for l in kadakas_core::viewrules::grid_lines(g, grid, lo, hi) {
        let Some((p, q)) = clip_segment(view.to_screen(l.a), view.to_screen(l.b), rect) else {
            continue;
        };
        painter.line_segment([p, q], Stroke::new(1.0, color));
        if grid.labels {
            // Easting labels sit at the top of the view, northing labels at the left.
            let (at, anchor) = if l.easting {
                let top = if p.y < q.y { p } else { q };
                (top + egui::vec2(3.0, 2.0), egui::Align2::LEFT_TOP)
            } else {
                let left = if p.x < q.x { p } else { q };
                (left + egui::vec2(3.0, -2.0), egui::Align2::LEFT_BOTTOM)
            };
            painter.text(at, anchor, l.label, egui::FontId::proportional(11.0), color);
        }
    }
}

pub struct Frame<'a> {
    pub doc: &'a Document,
    pub selection: &'a [ObjectId],
    pub preview: &'a Preview,
    pub snap: Option<&'a SnapResult>,
    /// Show the cue label of the snap at the pointer.
    pub cues: bool,
    /// The canvas uses the dark theme.
    pub dark: bool,
    /// The smart points and edges you acquired.
    pub smart: &'a kadakas_core::snapping::Smart,
    pub show_grid: bool,
    /// Cache of site model results and the document revision, to draw contours.
    pub site: Option<(&'a kadakas_site::SiteCache, u64)>,
    /// The page of the active sheet layer. The canvas shows the sheet instead of the model.
    pub sheet: Option<(&'a crate::sheets_ui::SheetUi, &'a kadakas_print::Page)>,
}

pub fn paint(painter: &Painter, rect: Rect, view: &View, f: &Frame) {
    let painter = painter.with_clip_rect(rect);
    let colors = crate::theme::canvas_colors(f.dark);
    set_dark(f.dark);
    set_grey(false);
    if let Some((sheets, page)) = f.sheet {
        painter.rect_filled(rect, 0.0, Color32::from_gray(colors.desk));
        sheets.paint(&painter, view, page);
        for o in f
            .doc
            .objects
            .values()
            .filter(|o| f.selection.contains(&o.id))
        {
            let (lo, hi) = f.doc.object_bounds(o);
            painter.rect_stroke(
                Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi)).expand(2.0),
                0.0,
                Stroke::new(1.5, SELECT_COLOR),
                egui::StrokeKind::Middle,
            );
        }
        draw_preview(&painter, view, f);
        return;
    }
    painter.rect_filled(
        rect,
        0.0,
        Color32::from_rgb(colors.paper[0], colors.paper[1], colors.paper[2]),
    );
    if f.show_grid {
        draw_grid(&painter, view, rect, f.dark);
    }
    draw_ne_grid(&painter, view, rect, f.doc);
    crate::area_ui::set_revision(f.site.map(|(_, revision)| revision));
    kadakas_ui_core::annot_pick::set_revision(f.site.map_or(0, |(_, revision)| revision));
    // Tag counts are made once per frame, not once per tag.
    let counts = tag_counts(f.doc);
    // Map underlays are drawn first, below the vector objects.
    let visible = || f.doc.objects.values().filter(|o| f.doc.is_displayed(o));
    let is_under = |o: &&Object| matches!(o.shape, Shape::Underlay(_));
    for o in visible()
        .filter(is_under)
        .chain(visible().filter(|o| !is_under(o)))
    {
        let grey = f.doc.is_grey(o);
        set_grey(grey);
        let st = object_style(f.doc, o, view.zoom);
        let direct = f.selection.contains(&o.id);
        let via_group = !direct
            && o.parent.is_some()
            && f.doc
                .ancestors(o.id)
                .iter()
                .any(|g| f.selection.contains(g));
        let selected = direct || via_group;
        let locked = f.doc.layer(o.layer).is_some_and(|l| l.locked);
        let st = if locked {
            Style {
                pen: st.pen.gamma_multiply(0.6),
                ..st
            }
        } else {
            st
        };
        {
            // A grey object is drawn in grey and faint.
            let mut p = painter.clone();
            if grey {
                p.multiply_opacity(0.6);
            }
            draw_object(&p, view, f.doc, &counts, o, st, DVec2::ZERO);
        }
        set_grey(false);
        #[cfg(not(target_arch = "wasm32"))]
        crate::script_ui::draw_badge(&painter, view, f.doc, o);
        if let Some(t) = kadakas_core::worksheet_table::data(o) {
            crate::data_ui::draw_table(&painter, view, f.doc, o, &t, st.pen, f.site);
        }
        if let (Shape::SiteModel { .. }, Some((cache, revision))) = (&o.shape, f.site) {
            crate::grading_ui::draw_analysis(&painter, view, f.doc, o, cache, revision);
        }
        if let (true, Some((cache, revision))) = (kadakas_core::site::carries_extra(o), f.site) {
            crate::grading_ui::draw_extras(&painter, view, f.doc, o, cache, revision);
        }
        if let (Shape::SiteModel { .. }, Some((cache, revision))) = (&o.shape, f.site) {
            crate::site_ui::draw_contours(&painter, view, f.doc, o, cache, revision, st.width);
        }
        if selected {
            let hl = Style {
                pen: SELECT_COLOR,
                fill: Color32::TRANSPARENT,
                width: st.width + 1.5,
            };
            if let Shape::Symbol { .. } = o.shape {
                for v in kadakas_core::symbols::instance_objects(f.doc, o) {
                    draw_shape(&painter, view, &v.shape, hl, DVec2::ZERO);
                }
            } else {
                draw_shape(&painter, view, &o.shape, hl, DVec2::ZERO);
            }
            if let Some(vs) = o.shape.vertices() {
                draw_vertex_markers(&painter, view, vs);
            }
            let (lo, hi) = f.doc.object_bounds(o);
            if direct && lo.is_finite() && hi.is_finite() {
                painter.rect_stroke(
                    Rect::from_two_pos(view.to_screen(lo), view.to_screen(hi)).expand(3.0),
                    0.0,
                    Stroke::new(1.0, SELECT_COLOR.gamma_multiply(0.6)),
                    egui::StrokeKind::Middle,
                );
            }
        }
    }
    draw_preview(&painter, view, f);
    crate::snap_ui::draw_smart(&painter, view, f.smart, f.dark);
    if let Some(s) = f.snap {
        crate::snap_ui::draw_snap(&painter, view, s, f.cues, f.dark, rect);
    }
}

fn draw_preview(painter: &Painter, view: &View, f: &Frame) {
    let st = Style {
        pen: PREVIEW_COLOR,
        fill: PREVIEW_COLOR.gamma_multiply(0.12),
        width: 1.5,
    };
    match f.preview {
        Preview::None => {}
        Preview::Shape(Shape::Annotation(a)) => {
            crate::annot_ui::draw_preview(painter, view, f.doc, a, st)
        }
        Preview::Shape(shape) => draw_shape(painter, view, shape, st, DVec2::ZERO),
        Preview::Path { vertices, closed } => {
            let points = curve::flatten(vertices, *closed, curve_tolerance(view.zoom));
            let sp: Vec<Pos2> = points.iter().map(|p| view.to_screen(*p)).collect();
            if *closed && points.len() >= 3 {
                fill_polygon(painter, &points, &sp, st.fill);
            }
            painter.add(PathShape {
                points: sp,
                closed: *closed,
                fill: Color32::TRANSPARENT,
                stroke: PathStroke::new(1.5, PREVIEW_COLOR),
            });
        }
        Preview::Box { min, max, crossing } => {
            let r = Rect::from_two_pos(view.to_screen(*min), view.to_screen(*max));
            let c = if *crossing {
                Color32::from_rgb(40, 160, 70)
            } else {
                SELECT_COLOR
            };
            painter.rect(
                r,
                0.0,
                c.gamma_multiply(0.1),
                Stroke::new(1.0, c),
                egui::StrokeKind::Middle,
            );
        }
        Preview::Lasso { points, crossing } => {
            let c = if *crossing {
                Color32::from_rgb(40, 160, 70)
            } else {
                SELECT_COLOR
            };
            let sp: Vec<Pos2> = points.iter().map(|p| view.to_screen(*p)).collect();
            if sp.len() >= 3 {
                painter.add(PathShape {
                    points: sp.clone(),
                    closed: true,
                    fill: c.gamma_multiply(0.1),
                    stroke: PathStroke::new(1.0, c),
                });
            } else {
                painter.add(PathShape {
                    points: sp,
                    closed: false,
                    fill: Color32::TRANSPARENT,
                    stroke: PathStroke::new(1.0, c),
                });
            }
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
        assert!(
            three_pi.len() > 50 && three_pi.last().unwrap().distance(DVec2::new(10.0, 0.0)) < 1e-9
        );
    }

    #[test]
    fn arc_sampling_hits_endpoints() {
        let pts = arc_points(DVec2::ZERO, 10.0, 0.0, std::f64::consts::FRAC_PI_2);
        assert!(pts[0].distance(DVec2::new(10.0, 0.0)) < 1e-9);
        assert!(pts.last().unwrap().distance(DVec2::new(0.0, 10.0)) < 1e-9);
    }

    #[test]
    fn color_conversion() {
        assert_eq!(
            color32(Color([255, 0, 0, 255])),
            Color32::from_rgb(255, 0, 0)
        );
        assert_eq!(color32(Color::NONE).a(), 0);
    }

    #[test]
    fn the_dark_theme_and_grey_mode_change_the_drawn_colors() {
        set_dark(false);
        set_grey(false);
        assert_eq!(draw_color(Color([0, 0, 0, 255])), Color32::BLACK);
        set_dark(true);
        assert_eq!(
            draw_color(Color([0, 0, 0, 255])),
            Color32::from_rgb(230, 230, 230)
        );
        assert_eq!(
            draw_color(Color([200, 30, 30, 255])),
            Color32::from_rgb(200, 30, 30)
        );
        set_grey(true);
        let g = draw_color(Color([200, 30, 30, 255]));
        assert!(g.r() == g.g() && g.g() == g.b() && g.a() < 255);
        set_grey(false);
        set_dark(false);
    }

    #[test]
    fn a_frame_paints_grey_objects_snaps_and_smart_marks_in_both_themes() {
        use kadakas_core::command::{Command, Editor};
        use kadakas_core::model::{Attributes, FIRST_LAYER};
        use kadakas_core::snapping::{Kind, Smart};
        use kadakas_core::visibility::VisMode;
        let mut ed = Editor::default();
        ed.execute(&Command::AddObject {
            shape: Shape::Circle {
                center: DVec2::new(100.0, 100.0),
                radius: 50.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let work = ed
            .execute(&Command::AddLayer {
                name: "Work".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::SetActive {
            layer: Some(kadakas_core::model::LayerId(work)),
            class: None,
        })
        .unwrap();
        ed.execute(&Command::SetVisibility {
            layers: vec![FIRST_LAYER],
            classes: vec![],
            mode: VisMode::Grey,
        })
        .unwrap();
        let mut smart = Smart::default();
        smart.acquire_point(DVec2::new(10.0, 10.0));
        let snap = SnapResult {
            point: DVec2::new(100.0, 100.0),
            kind: Kind::Center,
            cue: "Center".into(),
            guides: vec![],
        };
        let ctx = egui::Context::default();
        for dark in [false, true] {
            let _ = ctx.run_ui(Default::default(), |ui| {
                let rect = ui.max_rect();
                let view = View::default();
                let preview = Preview::Lasso {
                    points: vec![DVec2::ZERO, DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0)],
                    crossing: dark,
                };
                let frame = Frame {
                    doc: ed.doc(),
                    selection: &[],
                    preview: &preview,
                    snap: Some(&snap),
                    cues: true,
                    dark,
                    smart: &smart,
                    show_grid: true,
                    site: None,
                    sheet: None,
                };
                paint(&ui.painter_at(rect), rect, &view, &frame);
            });
        }
        // Every frame sets the theme itself, so a light frame after a dark one is light.
        let _ = ctx.run_ui(Default::default(), |ui| {
            let rect = ui.max_rect();
            let frame = Frame {
                doc: ed.doc(),
                selection: &[],
                preview: &Preview::None,
                snap: None,
                cues: false,
                dark: false,
                smart: &smart,
                show_grid: false,
                site: None,
                sheet: None,
            };
            paint(&ui.painter_at(rect), rect, &View::default(), &frame);
        });
        assert_eq!(draw_color(Color::BLACK), Color32::BLACK);
    }
}
