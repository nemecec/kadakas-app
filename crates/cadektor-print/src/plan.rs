//! Shapes to display list primitives. The same code draws the paper-space shapes of a sheet
//! (scale 1) and the model shapes inside a plan viewport (scale of the viewport).

use cadektor_core::curve::{self, Seg};
use cadektor_core::model::{Attributes, Color, Document, Object, Shape};
use cadektor_core::plants::graphic;
use cadektor_core::plants::schedule::{self, TagCounts};
use cadektor_core::plants::style::PlantGraphic;
use cadektor_core::plants::tree::{self, TreeAction};
use cadektor_core::sheets::{Annotation, AnnotationItem, Viewport};
use cadektor_core::svg::site_outline;
use cadektor_site::grading::draw::Drawn;
use cadektor_site::grading::hook;
use glam::DVec2;

use crate::display::*;

/// Maps model coordinates to paper: `paper = paper_center + (model - model_center) * k`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub model_center: DVec2,
    pub paper_center: DVec2,
    pub k: f64,
}

impl Xf {
    /// Paper space: nothing changes.
    pub const IDENTITY: Xf = Xf {
        model_center: DVec2::ZERO,
        paper_center: DVec2::ZERO,
        k: 1.0,
    };

    pub fn of(vp: &Viewport) -> Xf {
        Xf {
            model_center: vp.center,
            paper_center: vp.frame_center(),
            k: 1.0 / vp.scale,
        }
    }

    pub fn p(&self, m: DVec2) -> DVec2 {
        self.paper_center + (m - self.model_center) * self.k
    }

    pub fn len(&self, l: f64) -> f64 {
        l * self.k
    }
}

/// Pen and fill of an object as colors that can be drawn. A color with alpha 0 means "none".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub stroke: Option<Rgba>,
    pub fill: Option<Rgba>,
    pub width: f64,
}

impl Style {
    pub fn of(a: &Attributes) -> Style {
        let some = |c: Option<Color>| c.filter(|c| c.0[3] > 0).map(|c| c.0);
        Style {
            stroke: some(a.pen.or(Some(Color::BLACK))),
            fill: some(a.fill),
            width: a.line_weight.unwrap_or(0.25).max(0.0),
        }
    }
}

fn with_alpha(c: Option<Rgba>, factor: f64) -> Option<Rgba> {
    c.map(|[r, g, b, a]| [r, g, b, (a as f64 * factor).round() as u8])
}

fn path(segs: Vec<PSeg>, closed: bool, s: &Style) -> Prim {
    Prim::Path(PathPrim {
        segs,
        closed,
        stroke: s.stroke,
        fill: if closed { s.fill } else { None },
        width: s.width,
        dash: None,
    })
}

/// The pieces of an exact curve path, mapped to paper.
pub fn curve_segs(p: &curve::Path, xf: &Xf) -> Vec<PSeg> {
    let mut out = vec![PSeg::Move(xf.p(p.start))];
    for s in &p.segs {
        match *s {
            Seg::Line { to, .. } => out.push(PSeg::Line(xf.p(to))),
            Seg::Arc {
                center,
                radius,
                start,
                sweep,
                ..
            } => out.extend(arc_segs(xf.p(center), xf.len(radius), start, sweep, false)),
            Seg::Quad { from, c, to } => {
                let (c1, c2) = (from + (c - from) * (2.0 / 3.0), to + (c - to) * (2.0 / 3.0));
                out.push(PSeg::Cubic(xf.p(c1), xf.p(c2), xf.p(to)));
            }
            Seg::Cubic { c1, c2, to, .. } => out.push(PSeg::Cubic(xf.p(c1), xf.p(c2), xf.p(to))),
        }
    }
    out
}

fn mapped(points: &[DVec2], xf: &Xf) -> Vec<PSeg> {
    PathPrim::polyline(&points.iter().map(|p| xf.p(*p)).collect::<Vec<_>>())
}

fn dashed(mut p: Prim, dash: [f64; 2]) -> Prim {
    if let Prim::Path(pp) = &mut p {
        pp.dash = Some(dash);
    }
    p
}

fn text(at: DVec2, t: &str, height: f64, color: Rgba) -> Prim {
    Prim::Text(TextPrim {
        at,
        text: t.to_string(),
        height,
        color,
        align: Align::Left,
    })
}

/// Draws one shape. `attrs` are the final attributes (with viewport overrides applied).
pub fn shape_prims(
    doc: &Document,
    obj: &Object,
    attrs: &Attributes,
    xf: &Xf,
    counts: &TagCounts,
    tables: &crate::table::Tables,
    out: &mut Vec<Prim>,
) {
    shape_prims_deep(doc, obj, attrs, xf, counts, tables, out, 0);
}

/// Fill pattern, line type and symbol marks of an object, as primitives on paper.
#[allow(clippy::too_many_arguments)]
fn decor_prims(
    doc: &Document,
    obj: &Object,
    attrs: &Attributes,
    xf: &Xf,
    counts: &TagCounts,
    tables: &crate::table::Tables,
    out: &mut Vec<Prim>,
    start: usize,
    depth: usize,
) {
    // Model mm per paper mm.
    let scale = if xf.k > 0.0 { 1.0 / xf.k } else { 1.0 };
    let Some(d) = cadektor_core::pattern::decorate(doc, obj, attrs, scale) else {
        return;
    };
    let st = Style::of(attrs);
    if d.replaces_outline {
        for p in &mut out[start..] {
            if let Prim::Path(pp) = p {
                pp.stroke = None;
            }
        }
    }
    let pen = st.stroke.unwrap_or([0, 0, 0, 255]);
    for seg in &d.fill {
        let color = seg.color.map_or(pen, |c| c.0);
        out.push(Prim::Path(PathPrim::stroked(
            mapped(&[seg.a, seg.b], xf),
            false,
            color,
            seg.weight.unwrap_or(st.width),
        )));
    }
    for stroke in &d.strokes {
        out.push(Prim::Path(PathPrim::stroked(
            mapped(stroke, xf),
            false,
            pen,
            st.width,
        )));
    }
    for m in &d.marks {
        let inst =
            cadektor_core::symbols::mark_object(obj, None, None, m.symbol, m.at, m.angle, m.scale);
        let a = Attributes {
            fill_pattern: None,
            line_type: None,
            ..attrs.clone()
        };
        shape_prims_deep(doc, &inst, &a, xf, counts, tables, out, depth + 1);
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_prims_deep(
    doc: &Document,
    obj: &Object,
    attrs: &Attributes,
    xf: &Xf,
    counts: &TagCounts,
    tables: &crate::table::Tables,
    out: &mut Vec<Prim>,
    depth: usize,
) {
    if depth > cadektor_core::symbols::MAX_DEPTH + 2 {
        return;
    }
    let start = out.len();
    // The colors of an analysis go under the outline of the site model. Walls, grades, spot
    // elevations and the like go over their shape.
    let is_site = matches!(obj.shape, Shape::SiteModel { .. });
    if is_site {
        site_prims(doc, obj, xf, tables, out);
    }
    let start = if is_site { out.len() } else { start };
    shape_core(doc, obj, attrs, xf, counts, tables, out, depth);
    decor_prims(doc, obj, attrs, xf, counts, tables, out, start, depth);
    if cadektor_core::site::carries_extra(obj) {
        let scale = if xf.k > 0.0 { 1.0 / xf.k } else { 1.0 };
        let drawn = hook::extras_at(doc, tables.site(), SITE_REVISION, obj, scale);
        drawn_prims(&drawn, xf, out);
    }
}

/// The revision of the document of a page, for the cache of its site models.
const SITE_REVISION: u64 = 1;

/// The contours and the analysis colors of a site model.
fn site_prims(
    doc: &Document,
    obj: &Object,
    xf: &Xf,
    tables: &crate::table::Tables,
    out: &mut Vec<Prim>,
) {
    let scale = if xf.k > 0.0 { 1.0 / xf.k } else { 1.0 };
    let drawn = hook::site_layers(doc, tables.site(), SITE_REVISION, obj, scale, true);
    drawn_prims(&drawn, xf, out);
}

/// The shapes and texts of the grading objects as primitives on paper. The same `Drawn` is drawn
/// by the plan SVG and by the app canvas. Text is not turned: a turned label stays horizontal.
pub fn drawn_prims(d: &Drawn, xf: &Xf, out: &mut Vec<Prim>) {
    for poly in &d.polys {
        let rings: Vec<&Vec<DVec2>> = poly.rings.iter().filter(|r| !r.is_empty()).collect();
        let width = poly.weight * xf.k;
        if let Some(fill) = poly.fill {
            // The rings of a shape with holes run in opposite directions, so the nonzero rule leaves the holes empty.
            let segs: Vec<PSeg> = rings.iter().flat_map(|r| mapped(r, xf)).collect();
            let stroke = if rings.len() == 1 { poly.stroke } else { None };
            out.push(Prim::Path(PathPrim {
                segs,
                closed: poly.closed,
                stroke,
                fill: Some(fill),
                width,
                dash: None,
            }));
            if rings.len() == 1 {
                continue;
            }
        }
        if let Some(stroke) = poly.stroke {
            for r in rings {
                out.push(Prim::Path(PathPrim::stroked(
                    mapped(r, xf),
                    poly.closed,
                    stroke,
                    width,
                )));
            }
        }
    }
    for l in &d.labels {
        out.push(Prim::Text(TextPrim {
            at: xf.p(l.at),
            text: l.text.clone(),
            height: l.height * xf.k,
            color: [0, 0, 0, 255],
            align: if l.center { Align::Center } else { Align::Left },
        }));
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_core(
    doc: &Document,
    obj: &Object,
    attrs: &Attributes,
    xf: &Xf,
    counts: &TagCounts,
    tables: &crate::table::Tables,
    out: &mut Vec<Prim>,
    depth: usize,
) {
    let st = Style::of(attrs);
    let ink = st.stroke.unwrap_or([0, 0, 0, 255]);
    match &obj.shape {
        Shape::Line { a, b } => out.push(path(mapped(&[*a, *b], xf), false, &st)),
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            out.push(path(
                mapped(
                    &[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)],
                    xf,
                ),
                true,
                &st,
            ));
            if let Some(t) = cadektor_core::worksheet_table::data(obj) {
                crate::table::table_prims(tables, lo, hi, &t, xf, ink, out);
            }
        }
        Shape::Circle { center, radius } => {
            out.push(path(circle_segs(xf.p(*center), xf.len(*radius)), true, &st))
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = cadektor_core::model::arc_sweep(*start, *end);
            out.push(path(
                arc_segs(xf.p(*center), xf.len(*radius), *start, sweep, true),
                false,
                &st,
            ));
        }
        Shape::Polyline { points, closed } => {
            let segs = if curve::all_straight(points) {
                mapped(&curve::positions(points), xf)
            } else {
                curve_segs(&curve::path(points, *closed), xf)
            };
            out.push(path(segs, *closed, &st));
        }
        Shape::Text {
            at,
            text: t,
            height,
        } => out.push(text(xf.p(*at), t, xf.len(*height), ink)),
        Shape::Plant { at, spread, .. } => {
            let look = cadektor_core::viewrules::plant_look(doc, obj);
            let own = attrs_set(obj);
            let (pen, fill) = if own {
                (st.stroke, st.fill)
            } else {
                (
                    look.pen.map(|c| c.0).or(st.stroke),
                    look.fill.map(|c| c.0).or(st.fill),
                )
            };
            let s = Style {
                stroke: pen,
                fill: with_alpha(fill, 0.3),
                width: st.width,
            };
            if let Some(zone) = look.zone {
                out.push(dashed(
                    path(
                        circle_segs(xf.p(*at), xf.len(zone)),
                        true,
                        &Style { fill: None, ..st },
                    ),
                    [2.0, 1.0],
                ));
            }
            let parts = look
                .symbol
                .map(|sym| {
                    cadektor_core::symbols::plant_symbol_objects(
                        doc,
                        obj,
                        sym,
                        *at,
                        *spread,
                        look.rotation,
                    )
                })
                .unwrap_or_default();
            if !parts.is_empty() {
                for v in &parts {
                    let a = Attributes {
                        pen: v.attrs.pen.or(attrs.pen),
                        fill: v.attrs.fill.or(attrs.fill),
                        line_weight: v.attrs.line_weight.or(attrs.line_weight),
                        ..Attributes::default()
                    };
                    shape_prims_deep(doc, v, &a, xf, counts, tables, out, depth + 1);
                }
            } else if look.graphic == PlantGraphic::Circle {
                out.push(path(circle_segs(xf.p(*at), xf.len(spread / 2.0)), true, &s));
            } else {
                for (i, gp) in
                    graphic::paths_rotated(look.graphic, *at, spread / 2.0, look.rotation)
                        .iter()
                        .enumerate()
                {
                    let sty = if i == 0 { s } else { Style { fill: None, ..s } };
                    out.push(path(mapped(&gp.points, xf), gp.closed, &sty));
                }
            }
        }
        Shape::ExistingTree {
            at,
            dbh,
            spread,
            rpz_factor,
            rpz_cap,
            action,
            ..
        } => {
            let rpz = tree::rpz_radius(*dbh, *rpz_factor, *rpz_cap);
            let outline = Style { fill: None, ..st };
            out.push(dashed(
                path(circle_segs(xf.p(*at), xf.len(rpz)), true, &outline),
                [2.0, 1.0],
            ));
            let removed = *action == TreeAction::Remove;
            let canopy = path(
                circle_segs(xf.p(*at), xf.len(spread / 2.0)),
                true,
                &Style {
                    fill: with_alpha(st.fill, 0.2),
                    ..st
                },
            );
            out.push(if removed {
                dashed(canopy, [2.0, 1.0])
            } else {
                canopy
            });
            out.push(path(
                circle_segs(xf.p(*at), xf.len(dbh / 2.0)),
                true,
                &outline,
            ));
            if removed {
                let r = spread / 2.0;
                let cross = [
                    [*at + DVec2::new(-r, -r), *at + DVec2::new(r, r)],
                    [*at + DVec2::new(-r, r), *at + DVec2::new(r, -r)],
                ];
                for c in cross {
                    out.push(dashed(path(mapped(&c, xf), false, &outline), [2.0, 1.0]));
                }
            }
        }
        Shape::PlantTag {
            at, anchor, height, ..
        } => {
            let layout = schedule::tag_info_with(doc, obj.id, counts).map(|t| t.layout(*height));
            out.push(path(mapped(&[*anchor, *at], xf), false, &st));
            match layout {
                None => out.push(text(xf.p(*at), "?", xf.len(*height), ink)),
                Some(l) => {
                    if let Some((c, r)) = l.bubble {
                        out.push(path(circle_segs(xf.p(*at + c), xf.len(r)), true, &st));
                    }
                    for t in &l.texts {
                        let w = xf.len(*height) * 0.55 * t.text.chars().count() as f64;
                        let mut p = xf.p(*at + t.at);
                        if t.centered {
                            p.x -= w / 2.0;
                        }
                        out.push(text(p, &t.text, xf.len(*height), ink));
                    }
                }
            }
        }
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            // The style gives pen and fill where the object has none of its own. The attributes
            // arrive with the by-class overrides of the viewport applied, and objects have no
            // overrides of their own in a viewport.
            // A field that the viewport changed stays as the viewport made it.
            let base = doc.resolved_attrs(obj);
            let styled = cadektor_core::hardscape::styled_attrs(doc, obj, base.clone());
            let mut a = attrs.clone();
            if a.fill == base.fill {
                a.fill = styled.fill;
            }
            if a.pen == base.pen {
                a.pen = styled.pen;
            }
            if a.line_weight == base.line_weight {
                a.line_weight = styled.line_weight;
            }
            let st = Style::of(&a);
            let segs = if curve::all_straight(boundary) {
                mapped(&curve::positions(boundary), xf)
            } else {
                curve_segs(&curve::path(boundary, true), xf)
            };
            out.push(path(segs, true, &st));
            crate::areas::extras(doc, obj, &st, xf, out);
        }
        Shape::Locus3d { p } => {
            let h = cadektor_core::model::LOCUS_MARKER_HALF;
            let q = p.truncate();
            out.push(path(
                mapped(&[q - DVec2::new(h, 0.0), q + DVec2::new(h, 0.0)], xf),
                false,
                &st,
            ));
            out.push(path(
                mapped(&[q - DVec2::new(0.0, h), q + DVec2::new(0.0, h)], xf),
                false,
                &st,
            ));
        }
        Shape::Polyline3d { points, closed } => {
            let pts: Vec<DVec2> = points.iter().map(|p| p.truncate()).collect();
            out.push(path(mapped(&pts, xf), *closed, &st));
        }
        Shape::SiteModel { .. } | Shape::SiteModifier { .. } => {
            let outline = site_outline(&obj.shape);
            if outline.len() >= 3 {
                out.push(path(mapped(&outline, xf), true, &st));
            }
        }
        Shape::Annotation(a) => {
            crate::annot::push(doc, a, &crate::annot::layer_ctx(xf), &st, xf, out)
        }
        // Paper-space objects that other code composes.
        Shape::Viewport(_) | Shape::TitleBlock(_) => {}
        Shape::Group { .. } | Shape::Unknown { .. } => {}
        Shape::Underlay(u) => {
            if let Some(img) = crate::underlay::image_prim(u, xf) {
                out.push(Prim::Image(img));
            }
        }
        Shape::Mesh3d(m) => {
            // Plan view: the box around the mesh.
            let (lo, hi) = m.bounds3();
            let (lo, hi) = (lo.truncate(), hi.truncate());
            out.push(path(
                mapped(
                    &[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)],
                    xf,
                ),
                true,
                &st,
            ));
        }
        Shape::Massing(m) => {
            let segs = if curve::all_straight(&m.footprint) {
                mapped(&curve::positions(&m.footprint), xf)
            } else {
                curve_segs(&curve::path(&m.footprint, true), xf)
            };
            out.push(path(segs, true, &st));
            for (p, q) in cadektor_core::massing::roof_lines(m) {
                out.push(path(mapped(&[p, q], xf), false, &st));
            }
        }
        Shape::Irrigation(i) => irrigation_prims(i, &st, xf, out),
        Shape::Symbol { .. } => {
            // A symbol in paper units keeps its paper size at the scale of the drawing that shows it.
            let paper_scale = if xf.k > 0.0 { 1.0 / xf.k } else { 1.0 };
            for v in cadektor_core::symbols::instance_objects_at(doc, obj, paper_scale) {
                // A component with a class of its own looks like that class. Otherwise it follows the instance.
                let a = if v.class == obj.class {
                    v.attrs.over(attrs)
                } else {
                    doc.resolved_attrs(&v)
                };
                shape_prims_deep(doc, &v, &a, xf, counts, tables, out, depth + 1);
            }
        }
    }
}

/// Irrigation objects: a translucent fan per outlet (in the color of its zone), lines, and small
/// markers for points of connection, valves and controllers.
fn irrigation_prims(
    i: &cadektor_core::irrigation::IrrigationShape,
    st: &Style,
    xf: &Xf,
    out: &mut Vec<Prim>,
) {
    use cadektor_core::irrigation::{IrrigationShape as I, sector_points, zone_color};
    let rgba = |c: [u8; 3], a: u8| Some([c[0], c[1], c[2], a]);
    match i {
        I::Outlet {
            at,
            arc,
            direction,
            radius,
            zone,
            ..
        } => {
            let c = zone_color(zone.unwrap_or(0));
            if let Some(r) = radius {
                let pts = sector_points(*at, *r, *arc, *direction, 48);
                let s = Style {
                    stroke: rgba(c, 255),
                    fill: rgba(c, 64),
                    width: st.width,
                };
                out.push(path(mapped(&pts, xf), true, &s));
            }
            let s = Style {
                stroke: Some([0, 0, 0, 255]),
                fill: rgba(c, 255),
                width: st.width,
            };
            out.push(path(circle_segs(xf.p(*at), xf.len(300.0)), true, &s));
        }
        I::Line { points, .. } => out.push(path(
            mapped(points, xf),
            false,
            &Style {
                stroke: rgba([30, 80, 170], 255),
                fill: None,
                width: st.width,
            },
        )),
        I::Poc { at, .. } | I::Controller { at, .. } | I::Valve { at, .. } => {
            let c = match i {
                I::Valve { zone, .. } => zone_color(*zone),
                _ => [20, 50, 140],
            };
            let s = Style {
                stroke: rgba(c, 255),
                fill: rgba(c, 96),
                width: st.width,
            };
            out.push(path(circle_segs(xf.p(*at), xf.len(500.0)), true, &s));
        }
    }
}

fn attrs_set(obj: &Object) -> bool {
    obj.attrs.fill.is_some() || obj.attrs.pen.is_some()
}

/// The primitives of one annotation, on the sheet.
pub fn annotation_prims(doc: &Document, vp: &Viewport, a: &Annotation, out: &mut Vec<Prim>) {
    let attrs = vp.resolve_annotation(doc, a);
    let st = Style::of(&attrs);
    let ink = st.stroke.unwrap_or([0, 0, 0, 255]);
    let to = |p: DVec2| vp.annotation_to_paper(p);
    match &a.item {
        AnnotationItem::Line { a, b } => {
            out.push(path(PathPrim::polyline(&[to(*a), to(*b)]), false, &st))
        }
        AnnotationItem::Text {
            at,
            text: t,
            height,
        } => out.push(text(to(*at), t, *height, ink)),
        AnnotationItem::Dimension {
            a,
            b,
            offset,
            text: label,
        } => {
            let (p, q) = (to(*a), to(*b));
            let d = q - p;
            if d.length() > 0.0 {
                let n = DVec2::new(-d.y, d.x).normalize();
                let (p2, q2) = (p + n * *offset, q + n * *offset);
                let ext = 1.5 * offset.signum();
                out.push(path(PathPrim::polyline(&[p, p2 + n * ext]), false, &st));
                out.push(path(PathPrim::polyline(&[q, q2 + n * ext]), false, &st));
                out.push(path(PathPrim::polyline(&[p2, q2]), false, &st));
                let u = d.normalize();
                for e in [p2, q2] {
                    out.push(path(
                        PathPrim::polyline(&[e - (u + n) * 1.0, e + (u + n) * 1.0]),
                        false,
                        &st,
                    ));
                }
                let label = label
                    .clone()
                    .unwrap_or_else(|| format!("{}", (d.length() * vp.scale).round()));
                let h = 2.5;
                let mid = (p2 + q2) / 2.0 + n * 1.0;
                let at = mid - u * (text_width(&label, h) / 2.0);
                out.push(text(at, &label, h, ink));
            }
        }
        AnnotationItem::Shape { shape } if matches!(**shape, Shape::Annotation(_)) => {
            // Positions are paper mm from the frame. Real lengths follow the scale of the viewport.
            if let Shape::Annotation(an) = &**shape {
                let xf = Xf {
                    model_center: DVec2::ZERO,
                    paper_center: vp.min,
                    k: 1.0,
                };
                crate::annot::push(
                    doc,
                    an,
                    &cadektor_core::annot::draw::Ctx::viewport(vp),
                    &st,
                    &xf,
                    out,
                );
            }
        }
        AnnotationItem::Shape { shape } => {
            let obj = Object::new(
                cadektor_core::model::ObjectId(0),
                cadektor_core::model::FIRST_LAYER,
                a.class,
                (**shape).clone(),
            );
            let xf = Xf {
                model_center: DVec2::ZERO,
                paper_center: vp.min,
                k: 1.0,
            };
            shape_prims(
                doc,
                &obj,
                &attrs,
                &xf,
                &TagCounts::new(),
                &crate::table::Tables::default(),
                out,
            );
        }
    }
}
