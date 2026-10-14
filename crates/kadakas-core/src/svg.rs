//! SVG rendering of the visible objects. Agents use it to "see" a drawing.

use std::fmt::Write;

use glam::DVec2;

use crate::curve;
use crate::model::*;
use crate::plants::graphic::{self, GraphicPath};
use crate::plants::style::PlantGraphic;
use crate::plants::tag as schedule;
use crate::plants::tree::{self, TreeAction};
use crate::svg_overlay::{SvgLegendRow, SvgLook, SvgOverlay, SvgPattern, SvgTint};

/// Bounds of all visible objects, or `None` for an empty drawing.
pub fn drawing_bounds(doc: &Document) -> Option<(DVec2, DVec2)> {
    doc.objects
        .values()
        .filter(|o| doc.is_visible(o))
        .map(|o| doc.object_bounds(o))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
}

/// The outline that a site model or modifier draws in 2D: the convex hull of a site model's
/// source data, or the flattened boundary of a modifier. Empty for other shapes.
pub fn site_outline(shape: &Shape) -> Vec<DVec2> {
    match shape {
        Shape::SiteModel { points, lines, .. } => {
            let all: Vec<DVec2> = points
                .iter()
                .chain(lines.iter().flat_map(|l| &l.points))
                .map(|p| p.truncate())
                .collect();
            convex_hull(&all)
        }
        Shape::SiteModifier { boundary, .. } => crate::site::boundary_points(boundary),
        _ => Vec::new(),
    }
}

/// Convex hull, counterclockwise (monotone chain).
pub fn convex_hull(points: &[DVec2]) -> Vec<DVec2> {
    let mut p: Vec<DVec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let mut hull: Vec<DVec2> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &DVec2>> = if pass == 0 {
            Box::new(p.iter())
        } else {
            Box::new(p.iter().rev())
        };
        for &q in iter {
            while hull.len() >= start + 2
                && (hull[hull.len() - 1] - hull[hull.len() - 2]).perp_dot(q - hull[hull.len() - 2])
                    <= 0.0
            {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn points_attr(pts: &[DVec2]) -> String {
    pts.iter()
        .map(|p| format!("{},{}", p.x, -p.y))
        .collect::<Vec<_>>()
        .join(" ")
}

/// SVG path data for a stroke, with the Y axis flipped.
fn path_d(p: &GraphicPath) -> String {
    let mut d = String::new();
    for (i, q) in p.points.iter().enumerate() {
        let _ = write!(d, "{}{} {}", if i == 0 { "M " } else { " L " }, q.x, -q.y);
    }
    if p.closed {
        d.push_str(" Z");
    }
    d
}

/// Renders the drawing with the model Y axis pointing up.
/// `width_px` sets the pixel width. The height follows the aspect ratio.
pub fn render(doc: &Document, width_px: u32) -> String {
    render_with(doc, width_px, None)
}

/// Adds the SVG of one site model or site modifier to the output. It gets the document, the
/// object and a style string for the attributes of the elements.
pub type SvgExt<'a> = &'a dyn Fn(&Document, &Object, &str, &mut String);

/// Like `render`, with an extension that draws site models (contours) after their outline.
pub fn render_with(doc: &Document, width_px: u32, ext: Option<SvgExt<'_>>) -> String {
    render_overlay(doc, width_px, ext, None)
}

/// The box around the visible objects and the ghosts of an overlay.
fn overlay_bounds(doc: &Document, overlay: Option<&dyn SvgOverlay>) -> Option<(DVec2, DVec2)> {
    let ghosts = overlay.map(|o| o.ghosts()).unwrap_or_default();
    ghosts
        .iter()
        .map(|g| doc.object_bounds(&g.object))
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .chain(drawing_bounds(doc))
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
}

/// The dash pattern of a tinted outline, for a line weight of `sw`.
fn dash_attr(pattern: SvgPattern, sw: f64) -> String {
    match pattern {
        SvgPattern::Solid => String::new(),
        SvgPattern::Dashed => format!(r#" stroke-dasharray="{} {}""#, 6.0 * sw, 4.0 * sw),
        SvgPattern::Dotted => format!(r#" stroke-dasharray="{} {}""#, 1.5 * sw, 3.0 * sw),
    }
}

/// Like `render_with`, with a change of the look of objects, and extra objects (the compare
/// overlay, REV-03). Each tinted object is in a group with `data-compare` set to its status.
pub fn render_overlay(
    doc: &Document,
    width_px: u32,
    ext: Option<SvgExt<'_>>,
    overlay: Option<&dyn SvgOverlay>,
) -> String {
    // A view that the overlay gives is the final box. Else the box is the drawing and the ghosts,
    // with a margin.
    let (lo, hi) = overlay.and_then(|o| o.view()).unwrap_or_else(|| {
        let (lo, hi) = overlay_bounds(doc, overlay).unwrap_or((DVec2::ZERO, DVec2::splat(1000.0)));
        let margin = ((hi - lo).max_element() * 0.05).max(1.0);
        (lo - margin, hi + margin)
    });
    let size = hi - lo;
    let height_px = (width_px as f64 * size.y / size.x).round().max(1.0);
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width_px}" height="{height_px}" viewBox="{} {} {} {}">"#,
        lo.x, -hi.y, size.x, size.y
    );
    if let (Some(g), Some(grid)) = (&doc.georef, &doc.drawing.view_rules.ne_grid) {
        let [r, gr, b, _] = grid.color.0;
        let text_h = if grid.label_height > 0.0 {
            grid.label_height
        } else {
            (hi - lo).max_element() * 0.012
        };
        let _ = writeln!(
            s,
            r#"<g data-kind="ne_grid" stroke="rgb({r},{gr},{b})" fill="rgb({r},{gr},{b})" stroke-width="{}" font-size="{text_h}">"#,
            (hi - lo).max_element() * 0.0008
        );
        for l in crate::viewrules::grid_lines(g, grid, lo, hi) {
            let _ = writeln!(
                s,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
                l.a.x, -l.a.y, l.b.x, -l.b.y
            );
            if grid.labels {
                // The label sits where the line enters the drawing box.
                let at = l.a.clamp(lo, hi);
                let _ = writeln!(
                    s,
                    r#"<text x="{}" y="{}" stroke="none">{}</text>"#,
                    at.x, -at.y, l.label
                );
            }
        }
        s.push_str(
            "</g>
",
        );
    }
    let tag_counts = schedule::tag_counts(doc);
    // 1 mm of line weight on paper is `scale` mm in the model.
    // Underlays (map backgrounds) go below everything else.
    let look_of = |o: &Object| overlay.map_or(SvgLook::Keep, |ov| ov.look(o.id));
    let visible = || {
        doc.objects
            .values()
            .filter(|o| doc.is_displayed(o))
            .filter(|o| look_of(o) != SvgLook::Hide)
    };
    // The size of the mark of a tinted object, and its distance from the corner.
    let mark_h = (hi - lo).max_element() * 0.02;
    // An object of a grey layer or class is drawn faint and without colors, in a group.
    let one = |o: &Object, s: &mut String| {
        let grey = doc.is_grey(o);
        if grey {
            s.push_str(GREY_OPEN);
        }
        match look_of(o) {
            SvgLook::Tint(t) => {
                tinted_object(doc, o, &t, "", s, &tag_counts, ext, mark_h);
            }
            SvgLook::Dim(a) => {
                let _ = writeln!(s, r#"<g data-compare="unchanged" opacity="{a}">"#);
                draw_object(doc, o, s, &tag_counts, ext, 0, None);
                s.push_str("</g>\n");
            }
            SvgLook::Keep | SvgLook::Hide => draw_object(doc, o, s, &tag_counts, ext, 0, None),
        }
        if grey {
            s.push_str("</g>\n");
        }
    };
    for o in visible().filter(|o| matches!(o.shape, Shape::Underlay(_))) {
        one(o, &mut s);
    }
    for o in visible().filter(|o| !matches!(o.shape, Shape::Underlay(_))) {
        one(o, &mut s);
    }
    for g in overlay.map(|o| o.ghosts()).unwrap_or_default() {
        tinted_object(
            doc,
            &g.object,
            &g.tint,
            g.kind,
            &mut s,
            &tag_counts,
            ext,
            mark_h,
        );
    }
    if let Some(ov) = overlay {
        legend_svg(ov.legend(), lo, hi, &mut s);
    }
    s.push_str("</svg>\n");
    s
}

/// The legend of an overlay: a swatch with the pattern and the mark of each tint, and its text.
fn legend_svg(rows: &[SvgLegendRow], lo: DVec2, hi: DVec2, s: &mut String) {
    if rows.is_empty() {
        return;
    }
    let size = (hi - lo).max_element();
    let (h, m) = (size * 0.025, size * 0.015);
    let _ = writeln!(
        s,
        r#"<g data-compare-legend="true" font-family="sans-serif" font-size="{h}">"#
    );
    let width = rows
        .iter()
        .map(|r| r.text.chars().count() as f64)
        .fold(0.0, f64::max)
        * h
        * 0.6
        + h * 3.5;
    let height = rows.len() as f64 * h * 1.5 + h * 0.5;
    let _ = writeln!(
        s,
        r#"<rect x="{}" y="{}" width="{width}" height="{height}" fill="white" fill-opacity="0.85" stroke="gray" stroke-width="{}"/>"#,
        lo.x + m,
        -hi.y + m,
        h * 0.05
    );
    for (i, r) in rows.iter().enumerate() {
        let y = -hi.y + m + h * 1.5 * (i as f64 + 1.0);
        let mut x = lo.x + m + h * 0.5;
        if let Some(t) = &r.tint {
            let [cr, cg, cb] = t.rgb;
            let _ = writeln!(
                s,
                r#"<line x1="{x}" y1="{}" x2="{}" y2="{}" stroke="rgb({cr},{cg},{cb})" stroke-width="{}"{} data-status="{}"/>"#,
                y - h * 0.3,
                x + h * 1.2,
                y - h * 0.3,
                h * 0.2,
                dash_attr(t.pattern, h * 0.2),
                t.status
            );
            if let Some(c) = t.symbol {
                let _ = writeln!(
                    s,
                    r#"<text x="{}" y="{y}" font-weight="bold" fill="rgb({cr},{cg},{cb})">{c}</text>"#,
                    x + h * 1.4
                );
            }
        }
        x += h * 2.4;
        let _ = writeln!(
            s,
            r#"<text x="{x}" y="{y}" fill="black">{}</text>"#,
            esc(&r.text)
        );
    }
    s.push_str("</g>\n");
}

/// Draws an object in a tint, in a group with `data-compare` (and `data-ghost` for a ghost), and
/// its mark at the top right corner of its box.
#[allow(clippy::too_many_arguments)]
fn tinted_object(
    doc: &Document,
    o: &Object,
    t: &SvgTint,
    ghost: &str,
    s: &mut String,
    tag_counts: &schedule::TagCounts,
    ext: Option<SvgExt<'_>>,
    mark_h: f64,
) {
    let ghost_attr = if ghost.is_empty() {
        String::new()
    } else {
        format!(r#" data-ghost="{ghost}""#)
    };
    let _ = writeln!(
        s,
        r#"<g data-compare="{}"{ghost_attr} data-id="{}" opacity="{}">"#,
        t.status, o.id, t.opacity
    );
    draw_object(doc, o, s, tag_counts, ext, 0, Some(t));
    if let Some(c) = t.symbol {
        let (_, hi) = doc.object_bounds(o);
        if hi.is_finite() {
            let [r, g, b] = t.rgb;
            let _ = writeln!(
                s,
                r#"<text x="{}" y="{}" font-size="{mark_h}" font-family="sans-serif" font-weight="bold" fill="rgb({r},{g},{b})" stroke="none" data-mark="{}">{c}</text>"#,
                hi.x + mark_h * 0.2,
                -hi.y - mark_h * 0.2,
                t.status
            );
        }
    }
    s.push_str("</g>\n");
}

/// The start of the group around an object of a grey layer or class (see `crate::visibility`).
const GREY_OPEN: &str = r#"<g data-visibility="grey" opacity="0.4" style="filter:grayscale(1)">"#;

/// Draws one object (and, for a symbol, the shapes of its definition). Symbols can contain symbols, so
/// this calls itself, up to a depth that stops a loop.
fn draw_object(
    doc: &Document,
    o: &Object,
    s: &mut String,
    tag_counts: &schedule::TagCounts,
    ext: Option<SvgExt<'_>>,
    depth: usize,
    tint: Option<&SvgTint>,
) {
    if depth > crate::symbols::MAX_DEPTH + 2 {
        return;
    }
    let mut a = crate::viewrules::display_attrs(
        doc,
        o,
        crate::hardscape::styled_attrs(doc, o, doc.resolved_attrs(o)),
    );
    if let Some(t) = tint {
        // The tint replaces the pen and the fill of the object, whatever its class and rules say.
        let [r, g, b] = t.rgb;
        a.pen = Some(Color([r, g, b, 255]));
        if a.fill.is_some_and(|c| c.0[3] > 0) {
            a.fill = Some(Color([
                r,
                g,
                b,
                (t.fill_alpha.clamp(0.0, 1.0) * 255.0) as u8,
            ]));
        }
        a.line_weight = Some(a.line_weight.unwrap_or(0.25) * t.weight_factor);
    }
    let scale = doc.layer(o.layer).map_or(100.0, |l| l.scale);
    let stroke = a.pen.unwrap_or(Color::BLACK).to_css();
    let fill = a.fill.unwrap_or(Color::NONE).to_css();
    let sw = a.line_weight.unwrap_or(0.25) * scale;
    // A tinted object draws its plain outline: patterns and line types would hide the tint.
    let decor = if tint.is_some() {
        None
    } else {
        crate::pattern::decorate(doc, o, &a, scale)
    };
    // A line type draws the outline itself, in dashes.
    let line_stroke = if decor.as_ref().is_some_and(|d| d.replaces_outline) {
        "none".to_string()
    } else {
        stroke.clone()
    };
    let dash = tint.map_or(String::new(), |t| dash_attr(t.pattern, sw));
    let style = format!(
        r#"stroke="{line_stroke}" fill="{fill}" stroke-width="{sw}" data-id="{}"{dash}"#,
        o.id
    );
    let extra_style = (ext.is_some() && crate::site::carries_extra(o)).then(|| style.clone());
    let _ = match &o.shape {
        Shape::Group { .. } | Shape::Unknown { .. } => Ok(()),
        Shape::Underlay(u) => writeln!(s, "{}", u.svg_element()),
        Shape::Mesh3d(m) => {
            // Plan view: the box around the mesh.
            let (lo, hi) = m.bounds3();
            writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" {style} data-kind="mesh3d"/>"#,
                lo.x,
                -hi.y,
                hi.x - lo.x,
                hi.y - lo.y
            )
        }
        Shape::Extrude(e) => {
            // The profile and the holes in one path: the holes are cut out (even-odd).
            let d: Vec<String> = std::iter::once(&e.profile)
                .chain(e.holes.iter())
                .map(|r| curve::path(r, true).svg_d())
                .collect();
            writeln!(
                s,
                r#"<path d="{}" {style} fill-rule="evenodd" data-kind="extrude"/>"#,
                d.join(" ")
            )
        }
        Shape::Sweep(sw) => {
            // Plan view: the box around the solid.
            let (lo, hi) = sw.bounds3();
            writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" {style} data-kind="sweep"/>"#,
                lo.x,
                -hi.y,
                hi.x - lo.x,
                hi.y - lo.y
            )
        }
        Shape::Massing(m) => {
            let res = if curve::all_straight(&m.footprint) {
                writeln!(
                    s,
                    r#"<polygon points="{}" {style} data-kind="massing"/>"#,
                    points_attr(&curve::positions(&m.footprint)),
                )
            } else {
                writeln!(
                    s,
                    r#"<path d="{}" {style} data-kind="massing"/>"#,
                    curve::path(&m.footprint, true).svg_d(),
                )
            };
            // The roof lines: ridge and hips.
            let lines = crate::massing::roof_lines(m);
            if !lines.is_empty() {
                let d: String = lines
                    .iter()
                    .map(|(p, q)| format!("M {} {} L {} {} ", p.x, -p.y, q.x, -q.y))
                    .collect();
                let _ = writeln!(
                    s,
                    r#"<path d="{}" {style} fill="none" data-kind="massing_roof"/>"#,
                    d.trim_end()
                );
            }
            res
        }
        Shape::Irrigation(i) => {
            s.push_str(&crate::irrigation::svg(o.id.0, i, scale));
            Ok(())
        }
        Shape::Symbol { .. } => {
            for v in crate::symbols::instance_objects(doc, o) {
                draw_object(doc, &v, s, tag_counts, ext, depth + 1, tint);
            }
            Ok(())
        }
        Shape::Line { a, b } => writeln!(
            s,
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}" {style}/>"#,
            a.x, -a.y, b.x, -b.y
        ),
        Shape::Rect { min, max } => {
            let (p, q) = (min.min(*max), min.max(*max));
            writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" {style}/>"#,
                p.x,
                -q.y,
                q.x - p.x,
                q.y - p.y
            )
        }
        Shape::Circle { center, radius } => {
            writeln!(
                s,
                r#"<circle cx="{}" cy="{}" r="{radius}" {style}/>"#,
                center.x, -center.y
            )
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = arc_sweep(*start, *end);
            let p0 = *center + DVec2::from_angle(*start) * *radius;
            if sweep >= std::f64::consts::TAU {
                // One SVG arc cannot draw a full circle, so draw two halves.
                let p1 = *center - DVec2::from_angle(*start) * *radius;
                writeln!(
                    s,
                    r#"<path d="M {} {} A {radius} {radius} 0 1 0 {} {} A {radius} {radius} 0 1 0 {} {}" {style}/>"#,
                    p0.x, -p0.y, p1.x, -p1.y, p0.x, -p0.y
                )
            } else {
                let p1 = *center + DVec2::from_angle(*start + sweep) * *radius;
                let large = (sweep > std::f64::consts::PI) as u8;
                // Y is flipped, so a counterclockwise arc has sweep-flag 0.
                writeln!(
                    s,
                    r#"<path d="M {} {} A {radius} {radius} 0 {large} 0 {} {}" {style}/>"#,
                    p0.x, -p0.y, p1.x, -p1.y
                )
            }
        }
        Shape::Polyline { points, closed } if !curve::all_straight(points) => {
            // Curved: exact arcs and Bezier curves in one path.
            writeln!(
                s,
                r#"<path d="{}" {style}/>"#,
                curve::path(points, *closed).svg_d()
            )
        }
        Shape::Polyline { points, closed } => {
            let points = &curve::positions(points);
            let tag = if *closed { "polygon" } else { "polyline" };
            writeln!(s, r#"<{tag} points="{}" {style}/>"#, points_attr(points))
        }
        Shape::Locus3d { p } => {
            let h = crate::model::LOCUS_MARKER_HALF;
            writeln!(
                s,
                r#"<path d="M {} {} L {} {} M {} {} L {} {}" {style} data-kind="locus3d"/>"#,
                p.x - h,
                -p.y,
                p.x + h,
                -p.y,
                p.x,
                -p.y - h,
                p.x,
                -p.y + h
            )
        }
        Shape::Polyline3d { points, closed } => {
            let points = points.iter().map(|p| p.truncate()).collect::<Vec<_>>();
            let tag = if *closed { "polygon" } else { "polyline" };
            writeln!(
                s,
                r#"<{tag} points="{}" {style} data-kind="polyline3d"/>"#,
                points_attr(&points)
            )
        }
        Shape::Viewport(_) | Shape::TitleBlock(_) => Ok(()),
        // Annotations come from the extension, which has the worksheet code for tags and legends.
        Shape::Annotation(_) => {
            if let Some(ext) = ext {
                ext(doc, o, &style, s);
            }
            Ok(())
        }
        Shape::SiteModel { .. } | Shape::SiteModifier { .. } => {
            let outline = site_outline(&o.shape);
            let res = writeln!(
                s,
                r#"<polygon points="{}" {style} data-kind="{}"/>"#,
                points_attr(&outline),
                o.shape.kind()
            );
            if let Some(ext) = ext {
                ext(doc, o, &style, s);
            }
            res
        }
        Shape::Text { at, text, height } => writeln!(
            s,
            r#"<text x="{}" y="{}" font-size="{height}" font-family="sans-serif" fill="{stroke}" data-id="{}">{}</text>"#,
            at.x,
            -at.y,
            o.id,
            esc(text)
        ),
        Shape::Plant {
            at,
            spread,
            species,
            ..
        } => {
            // A plant with a style draws the outline of its style, in the style colors unless the object sets its own.
            let look = crate::viewrules::plant_look(doc, o);
            let style = if tint.is_none() && o.attrs.fill.is_none() && o.attrs.pen.is_none() {
                let stroke = look.pen.map_or(stroke.clone(), |c| c.to_css());
                let fill = look.fill.map_or(fill.clone(), |c| c.to_css());
                format!(
                    r#"stroke="{stroke}" fill="{fill}" stroke-width="{sw}" data-id="{}""#,
                    o.id
                )
            } else {
                style
            };
            if let Some(zone) = look.zone {
                let dash = format!("{} {}", 4.0 * scale, 2.0 * scale);
                let _ = writeln!(
                    s,
                    r#"<circle cx="{}" cy="{}" r="{zone}" stroke="{stroke}" fill="none" stroke-width="{sw}" stroke-dasharray="{dash}" data-kind="tree_protection_zone" data-id="{}"/>"#,
                    at.x, -at.y, o.id
                );
            }
            let parts = look
                .symbol
                .map(|sym| {
                    crate::symbols::plant_symbol_objects(doc, o, sym, *at, *spread, look.rotation)
                })
                .unwrap_or_default();
            if !parts.is_empty() {
                for v in &parts {
                    draw_object(doc, v, s, tag_counts, ext, depth + 1, tint);
                }
                writeln!(
                    s,
                    r#"<g data-kind="plant" data-graphic="symbol"><title>{}</title></g>"#,
                    esc(species)
                )
            } else if look.graphic == PlantGraphic::Circle {
                writeln!(
                    s,
                    r#"<g data-kind="plant"><circle cx="{}" cy="{}" r="{}" {style} fill-opacity="0.3"/><title>{}</title></g>"#,
                    at.x,
                    -at.y,
                    spread / 2.0,
                    esc(species)
                )
            } else {
                let d = graphic::paths_rotated(look.graphic, *at, spread / 2.0, look.rotation)
                    .iter()
                    .map(path_d)
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(
                    s,
                    r#"<g data-kind="plant" data-graphic="{}"><path d="{d}" {style} fill-opacity="0.3"/><title>{}</title></g>"#,
                    look.graphic.id(),
                    esc(species)
                )
            }
        }
        Shape::ExistingTree {
            at,
            species,
            dbh,
            spread,
            condition,
            action,
            rpz_factor,
            rpz_cap,
            ..
        } => {
            let rpz = tree::rpz_radius(*dbh, *rpz_factor, *rpz_cap);
            let fill_style = style.clone();
            let style = format!(
                r#"stroke="{stroke}" fill="none" stroke-width="{sw}" data-id="{}""#,
                o.id
            );
            let removed = *action == TreeAction::Remove;
            let dash = format!(r#"stroke-dasharray="{} {}""#, 4.0 * scale, 2.0 * scale);
            let canopy_dash = if removed { dash.clone() } else { String::new() };
            let cross = if removed {
                let r = spread / 2.0;
                format!(
                    r#"<path d="M {} {} L {} {} M {} {} L {} {}" {style} {dash}/>"#,
                    at.x - r,
                    -at.y - r,
                    at.x + r,
                    -at.y + r,
                    at.x - r,
                    -at.y + r,
                    at.x + r,
                    -at.y - r
                )
            } else {
                String::new()
            };
            writeln!(
                s,
                r#"<g data-kind="existing_tree" data-action="{}" data-condition="{}"><circle cx="{x}" cy="{y}" r="{rpz}" {style} {dash} data-part="rpz"/><circle cx="{x}" cy="{y}" r="{}" {fill_style} fill-opacity="0.2" {canopy_dash} data-part="canopy"/><circle cx="{x}" cy="{y}" r="{}" {style} data-part="trunk"/>{cross}<title>{}</title></g>"#,
                action.id(),
                condition.id(),
                spread / 2.0,
                dbh / 2.0,
                esc(species),
                x = at.x,
                y = -at.y
            )
        }
        Shape::PlantTag {
            at, anchor, height, ..
        } => {
            let layout = schedule::tag_info_with(doc, o.id, tag_counts).map(|t| t.layout(*height));
            match layout {
                None => writeln!(
                    s,
                    r#"<g data-kind="plant_tag"><line x1="{}" y1="{}" x2="{}" y2="{}" {style}/><text x="{}" y="{}" font-size="{height}" font-family="sans-serif" fill="{stroke}">?</text></g>"#,
                    anchor.x, -anchor.y, at.x, -at.y, at.x, -at.y
                ),
                Some(l) => {
                    let mut g = format!(
                        r#"<g data-kind="plant_tag"><line x1="{}" y1="{}" x2="{}" y2="{}" {style}/>"#,
                        anchor.x, -anchor.y, at.x, -at.y
                    );
                    if let Some((c, r)) = l.bubble {
                        g += &format!(
                            r#"<circle cx="{}" cy="{}" r="{r}" fill="none" stroke="{stroke}" stroke-width="{sw}" data-kind="plant_tag_bubble"/>"#,
                            at.x + c.x,
                            -(at.y + c.y)
                        );
                    }
                    for t in &l.texts {
                        let (x, y) = (at.x + t.at.x, -(at.y + t.at.y));
                        let anchor_attr = if t.centered {
                            r#" text-anchor="middle" dominant-baseline="central""#
                        } else {
                            ""
                        };
                        g += &format!(
                            r#"<text x="{x}" y="{y}" font-size="{height}" font-family="sans-serif" fill="{stroke}"{anchor_attr}>{}</text>"#,
                            esc(&t.text)
                        );
                    }
                    g += "</g>";
                    writeln!(s, "{g}")
                }
            }
        }
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            let res = if curve::all_straight(boundary) {
                writeln!(
                    s,
                    r#"<polygon points="{}" {style} data-kind="{}"/>"#,
                    points_attr(&curve::positions(boundary)),
                    o.shape.kind()
                )
            } else {
                writeln!(
                    s,
                    r#"<path d="{}" {style} data-kind="{}"/>"#,
                    curve::path(boundary, true).svg_d(),
                    o.shape.kind()
                )
            };
            s.push_str(&crate::hardscape::object_svg_extras(doc, o, scale, &stroke));
            res
        }
    };
    if let Some(d) = &decor {
        decor_svg(doc, d, &a, o, scale, s, tag_counts, ext, depth);
    }
    if let (Some(ext), Some(style)) = (ext, extra_style) {
        ext(doc, o, &style, s);
    }
}

/// The fill pattern, dashes and marks of an object.
#[allow(clippy::too_many_arguments)]
fn decor_svg(
    doc: &Document,
    d: &crate::pattern::Decor,
    a: &Attributes,
    o: &Object,
    scale: f64,
    s: &mut String,
    tag_counts: &schedule::TagCounts,
    ext: Option<SvgExt<'_>>,
    depth: usize,
) {
    let pen = a.pen.unwrap_or(Color::BLACK);
    let sw = a.line_weight.unwrap_or(0.25) * scale;
    // Pattern lines with the same color and weight share one path.
    let mut groups: Vec<((Color, f64), String)> = Vec::new();
    for seg in &d.fill {
        let key = (
            crate::pattern::pen_of(seg, pen),
            seg.weight.map_or(sw, |w| w * scale),
        );
        let piece = format!("M {} {} L {} {} ", seg.a.x, -seg.a.y, seg.b.x, -seg.b.y);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, path)) => path.push_str(&piece),
            None => groups.push((key, piece)),
        }
    }
    for ((color, width), path) in groups {
        let _ = writeln!(
            s,
            r#"<path d="{}" fill="none" stroke="{}" stroke-width="{width}" data-pattern="fill" data-id="{}"/>"#,
            path.trim_end(),
            color.to_css(),
            o.id
        );
    }
    if !d.strokes.is_empty() {
        let path: String = d
            .strokes
            .iter()
            .map(|st| {
                st.iter()
                    .enumerate()
                    .map(|(i, q)| format!("{}{} {}", if i == 0 { "M " } else { " L " }, q.x, -q.y))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            s,
            r#"<path d="{path}" fill="none" stroke="{}" stroke-width="{sw}" data-pattern="line_type" data-id="{}"/>"#,
            pen.to_css(),
            o.id
        );
    }
    for m in &d.marks {
        let inst = crate::symbols::mark_object(
            o,
            Some(pen),
            a.line_weight,
            m.symbol,
            m.at,
            m.angle,
            m.scale,
        );
        draw_object(doc, &inst, s, tag_counts, ext, depth + 1, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};

    #[test]
    fn renders_visible_objects_only() {
        let mut ed = Editor::default();
        let add = |shape| Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        };
        ed.execute(&add(Shape::Circle {
            center: DVec2::ZERO,
            radius: 50.0,
        }))
        .unwrap();
        ed.execute(&add(Shape::Text {
            at: DVec2::ZERO,
            text: "a<b".into(),
            height: 10.0,
        }))
        .unwrap();
        let svg = render(ed.doc(), 400);
        assert!(svg.contains("<circle"));
        assert!(svg.contains("a&lt;b"));
        ed.execute(&Command::ModifyClass {
            id: NONE_CLASS,
            name: None,
            visible: Some(false),
            attrs: None,
        })
        .unwrap();
        assert!(!render(ed.doc(), 400).contains("<circle"));
    }

    fn add(ed: &mut Editor, shape: Shape) -> crate::uid::Uid {
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0]
    }

    fn one(shape: Shape) -> String {
        let mut ed = Editor::default();
        add(&mut ed, shape);
        render(ed.doc(), 400)
    }

    #[test]
    fn line_and_circle_elements() {
        let svg = one(Shape::Line {
            a: DVec2::new(1.0, 2.0),
            b: DVec2::new(3.0, 4.0),
        });
        assert!(
            svg.contains(r#"<line x1="1" y1="-2" x2="3" y2="-4""#),
            "{svg}"
        );
        let svg = one(Shape::Circle {
            center: DVec2::new(1.0, 2.0),
            radius: 3.0,
        });
        assert!(svg.contains(r#"<circle cx="1" cy="-2" r="3""#), "{svg}");
    }

    #[test]
    fn rect_with_swapped_corners_is_normalized() {
        let svg = one(Shape::Rect {
            min: DVec2::new(10.0, 8.0),
            max: DVec2::new(2.0, 3.0),
        });
        assert!(
            svg.contains(r#"<rect x="2" y="-8" width="8" height="5""#),
            "{svg}"
        );
    }

    #[test]
    fn arc_large_arc_flag_depends_on_sweep() {
        let arc = |start: f64, end: f64| Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start,
            end,
        };
        // Sweep of a quarter turn: small arc.
        let svg = one(arc(0.0, std::f64::consts::FRAC_PI_2));
        assert!(svg.contains("A 10 10 0 0 0"), "{svg}");
        // Sweep of three quarter turns: large arc.
        let svg = one(arc(0.0, 1.5 * std::f64::consts::PI));
        assert!(svg.contains("A 10 10 0 1 0"), "{svg}");
        // End below start wraps around: from 270 degrees to 0 is a quarter turn.
        let svg = one(arc(1.5 * std::f64::consts::PI, 0.0));
        assert!(svg.contains("A 10 10 0 0 0"), "{svg}");
    }

    #[test]
    fn locus3d_is_a_cross_and_polyline3d_uses_xy() {
        let svg = one(Shape::Locus3d {
            p: glam::DVec3::new(10.0, 20.0, 5.0),
        });
        assert!(
            svg.contains(r#"<path d="M -90 -20 L 110 -20 M 10 -120 L 10 80""#)
                && svg.contains("locus3d"),
            "{svg}"
        );
        let pts = vec![
            glam::DVec3::new(1.0, 2.0, 9.0),
            glam::DVec3::new(3.0, -4.0, 8.0),
        ];
        let svg = one(Shape::Polyline3d {
            points: pts.clone(),
            closed: false,
        });
        assert!(svg.contains(r#"<polyline points="1,-2 3,4""#), "{svg}");
        let svg = one(Shape::Polyline3d {
            points: pts,
            closed: true,
        });
        assert!(svg.contains(r#"<polygon points="1,-2 3,4""#), "{svg}");
    }

    #[test]
    fn polyline_and_polygon_elements() {
        let pts = vec![DVec2::new(1.0, 2.0), DVec2::new(3.0, -4.0)];
        let svg = one(Shape::Polyline {
            points: crate::curve::corners(&pts.clone()),
            closed: false,
        });
        assert!(svg.contains(r#"<polyline points="1,-2 3,4""#), "{svg}");
        let svg = one(Shape::Polyline {
            points: crate::curve::corners(&pts),
            closed: true,
        });
        assert!(svg.contains(r#"<polygon points="1,-2 3,4""#), "{svg}");
    }

    #[test]
    fn text_is_escaped_and_positioned() {
        let svg = one(Shape::Text {
            at: DVec2::new(1.0, 2.0),
            text: r#"R&D "x" <y>"#.into(),
            height: 10.0,
        });
        assert!(
            svg.contains(r#"<text x="1" y="-2" font-size="10""#),
            "{svg}"
        );
        assert!(
            svg.contains(">R&amp;D &quot;x&quot; &lt;y&gt;</text>"),
            "{svg}"
        );
    }

    #[test]
    fn plant_is_a_faded_circle_with_escaped_title() {
        let svg = one(Shape::Plant {
            at: DVec2::new(1.0, 2.0),
            species: "A&B".into(),
            spread: 4.0,
            height: 8.0,
        });
        assert!(
            svg.contains(r#"<g data-kind="plant"><circle cx="1" cy="-2" r="2""#),
            "{svg}"
        );
        assert!(
            svg.contains(r#"fill-opacity="0.3"/><title>A&amp;B</title></g>"#),
            "{svg}"
        );
    }

    #[test]
    fn landscape_and_hardscape_are_polygons_with_kind() {
        let boundary = vec![
            DVec2::new(1.0, 1.0),
            DVec2::new(3.0, 1.0),
            DVec2::new(3.0, 4.0),
        ];
        let svg = one(Shape::LandscapeArea {
            boundary: crate::curve::corners(&boundary.clone()),
            name: "Meadow".into(),
        });
        assert!(svg.contains(r#"<polygon points="1,-1 3,-1 3,-4""#), "{svg}");
        assert!(svg.contains(r#"data-kind="landscape_area""#), "{svg}");
        let svg = one(Shape::Hardscape {
            boundary: crate::curve::corners(&boundary),
            material: "Setts".into(),
            thickness: None,
            elevation: None,
        });
        assert!(svg.contains(r#"data-kind="hardscape""#), "{svg}");
    }

    #[test]
    fn stroke_width_uses_layer_scale_and_attributes_resolve() {
        let mut ed = Editor::default();
        // Class defaults: pen black, no fill, weight 0.25 mm at 1:100 gives 25.
        let c = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        let svg = render(ed.doc(), 400);
        let expected =
            format!(r#"stroke="rgba(0,0,0,1.000)" fill="none" stroke-width="25" data-id="{c}""#);
        assert!(svg.contains(&expected), "{svg}");

        // Object overrides beat the class.
        ed.execute(&Command::ModifyObject {
            id: ObjectId(c),
            shape: None,
            layer: None,
            class: None,
            attrs: Some(Attributes {
                pen: Some(Color([255, 0, 0, 255])),
                fill: Some(Color([0, 0, 255, 128])),
                line_weight: None,
                ..Default::default()
            }),
        })
        .unwrap();
        let svg = render(ed.doc(), 400);
        assert!(
            svg.contains(r#"stroke="rgba(255,0,0,1.000)" fill="rgba(0,0,255,0.502)""#),
            "{svg}"
        );

        // A layer at 1:50 halves the stroke width.
        let lid = ed
            .execute(&Command::AddLayer {
                name: "Detail".into(),
                elevation: 0.0,
                scale: Some(50.0),
                kind: None,
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::SetActive {
            layer: Some(LayerId(lid)),
            class: None,
        })
        .unwrap();
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(100.0, 0.0),
                radius: 5.0,
            },
        );
        let svg = render(ed.doc(), 400);
        assert!(svg.contains(r#"stroke-width="12.5""#), "{svg}");
    }

    #[test]
    fn class_without_attributes_uses_fallback_colors() {
        let mut ed = Editor::default();
        let cid = ClassId(
            ed.execute(&Command::AddClass {
                name: "Bare".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::SetActive {
            layer: None,
            class: Some(cid),
        })
        .unwrap();
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        let svg = render(ed.doc(), 400);
        assert!(
            svg.contains(r#"stroke="rgba(0,0,0,1.000)" fill="none" stroke-width="25""#),
            "{svg}"
        );
    }

    #[test]
    fn hidden_layer_objects_are_not_drawn() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        ed.execute(&Command::ModifyLayer {
            id: FIRST_LAYER,
            name: None,
            visible: Some(false),
            locked: None,
            elevation: None,
            scale: None,
        })
        .unwrap();
        let svg = render(ed.doc(), 400);
        assert!(!svg.contains("<circle"));
        // With nothing visible the empty-drawing defaults apply.
        assert!(svg.contains(r#"viewBox="-50 -1050 1100 1100""#), "{svg}");
    }

    #[test]
    fn empty_drawing_uses_default_frame() {
        let svg = render(&Document::default(), 400);
        assert!(
            svg.contains(r#"width="400" height="400" viewBox="-50 -1050 1100 1100""#),
            "{svg}"
        );
    }

    #[test]
    fn height_follows_aspect_ratio() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(1000.0, 500.0),
            },
        );
        // Model frame is 1100 x 600 with the 5% margin.
        let svg = render(ed.doc(), 550);
        assert!(
            svg.contains(r#"width="550" height="300" viewBox="-50 -550 1100 600""#),
            "{svg}"
        );
        // A tiny width still gives at least one pixel of height.
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(1000.0, 500.0),
            },
        );
        assert!(render(ed.doc(), 1).contains(r#"width="1" height="1""#));
    }

    #[test]
    fn drawing_bounds_ignores_hidden_objects() {
        let mut ed = Editor::default();
        assert_eq!(drawing_bounds(ed.doc()), None);
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(10.0, 10.0),
                radius: 1.0,
            },
        );
        let hidden = ed
            .execute(&Command::AddLayer {
                name: "Hidden2".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::ModifyLayer {
            id: LayerId(hidden),
            name: None,
            visible: Some(false),
            locked: None,
            elevation: None,
            scale: None,
        })
        .unwrap();
        ed.execute(&Command::SetActive {
            layer: Some(LayerId(hidden)),
            class: None,
        })
        .unwrap();
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(-500.0, 40.0),
                radius: 1.0,
            },
        );
        assert_eq!(
            drawing_bounds(ed.doc()),
            Some((DVec2::new(9.0, 9.0), DVec2::new(11.0, 11.0)))
        );
    }

    #[test]
    fn full_circle_arc_is_drawn_as_two_half_arcs() {
        let svg = one(Shape::Arc {
            center: DVec2::ZERO,
            radius: 10.0,
            start: 0.0,
            end: std::f64::consts::TAU,
        });
        assert_eq!(svg.matches(" A 10 10 0 1 0 ").count(), 2, "{svg}");
        assert!(svg.contains("M 10 -0 A 10 10 0 1 0 -10 -0 A"), "{svg}");
    }

    #[test]
    fn curved_shapes_use_exact_path_commands() {
        let v = |x: f64, y: f64, kind| Vertex::new(DVec2::new(x, y), kind);
        let corner = VertexKind::Corner;
        let rounded = vec![
            v(0.0, 0.0, corner),
            v(10.0, 0.0, VertexKind::Arc { radius: 2.0 }),
            v(10.0, 10.0, VertexKind::Arc { radius: 2.0 }),
            v(0.0, 10.0, corner),
        ];
        let svg = one(Shape::Polyline {
            points: rounded.clone(),
            closed: true,
        });
        assert!(
            svg.contains(
                r#"<path d="M 0 0 L 8 0 A 2 2 0 0 0 10 -2 L 10 -8 A 2 2 0 0 0 8 -10 L 0 -10 Z""#
            ),
            "{svg}"
        );
        let svg = one(Shape::Hardscape {
            boundary: rounded.clone(),
            material: "m".into(),
            thickness: None,
            elevation: None,
        });
        assert!(
            svg.contains(r#"data-kind="hardscape""#) && svg.contains("<path d="),
            "{svg}"
        );
        let svg = one(Shape::LandscapeArea {
            boundary: rounded,
            name: "n".into(),
        });
        assert!(
            svg.contains(r#"data-kind="landscape_area""#) && !svg.contains("<polygon"),
            "{svg}"
        );
        let curves = vec![
            v(0.0, 0.0, corner),
            v(5.0, 5.0, VertexKind::Bezier),
            v(10.0, 0.0, corner),
            v(15.0, -5.0, VertexKind::Cubic),
            v(20.0, -5.0, VertexKind::Cubic),
            v(25.0, 0.0, corner),
        ];
        let svg = one(Shape::Polyline {
            points: curves,
            closed: false,
        });
        assert!(
            svg.contains(" Q ") && svg.contains(" C ") && !svg.contains(" Z"),
            "{svg}"
        );
    }

    fn style_with(
        graphic: crate::plants::style::PlantGraphic,
    ) -> (Editor, crate::resource::ResourceId) {
        use crate::plants::style::PlantStyle;
        use crate::resource::{ResourceBody, ResourceId};
        let mut ed = Editor::default();
        let style = PlantStyle {
            graphic,
            spread: 2000.0,
            fill: Some(Color([1, 2, 3, 255])),
            pen: Some(Color([4, 5, 6, 255])),
            id_code: "ZZ".into(),
            ..PlantStyle::default()
        };
        let id = ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: ResourceBody::plant_style(style),
            })
            .unwrap()
            .created[0],
        );
        (ed, id)
    }

    #[test]
    fn styled_plants_draw_their_graphic_and_colors() {
        use crate::plants::style::PlantGraphic;
        let (mut ed, id) = style_with(PlantGraphic::Star);
        ed.execute(&Command::AddPlants {
            style: Some(id),
            overrides: Default::default(),
            positions: vec![DVec2::new(5.0, 5.0)],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        let svg = render(ed.doc(), 400);
        assert!(
            svg.contains(r#"data-graphic="star"><path d="M "#) && svg.contains(" Z"),
            "{svg}"
        );
        assert!(
            svg.contains("rgba(1,2,3,1.000)") && svg.contains("rgba(4,5,6,1.000)"),
            "{svg}"
        );
        // A circle graphic keeps the plain circle, with the style colors.
        let (mut ed, id) = style_with(PlantGraphic::Circle);
        ed.execute(&Command::AddPlants {
            style: Some(id),
            overrides: Default::default(),
            positions: vec![DVec2::new(0.0, 5.0)],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        assert!(
            render(ed.doc(), 400)
                .contains(r#"<circle cx="0" cy="-5" r="1000" stroke="rgba(4,5,6,1.000)""#)
        );
        // Own colors on the object win.
        let pid = *ed.doc().objects.keys().next().unwrap();
        ed.execute(&Command::ModifyObject {
            id: pid,
            shape: None,
            layer: None,
            class: None,
            attrs: Some(Attributes {
                fill: Some(Color([9, 9, 9, 255])),
                ..Default::default()
            }),
        })
        .unwrap();
        let svg = render(ed.doc(), 400);
        assert!(
            svg.contains("rgba(9,9,9,1.000)") && !svg.contains("rgba(1,2,3"),
            "{svg}"
        );
    }

    #[test]
    fn plants_show_rotation_and_a_protection_zone() {
        use crate::plants::style::{PlantCategory, PlantGraphic};
        let (mut ed, id) = style_with(PlantGraphic::Star);
        let mut style = ed.doc().resources.plant_style(id).unwrap().clone();
        style.category = PlantCategory::Tree;
        style.trunk_diameter = 300.0;
        style.protection_zone = true;
        ed.execute(&Command::ModifyResource {
            id,
            name: None,
            body: Some(crate::resource::ResourceBody::plant_style(style)),
        })
        .unwrap();
        ed.execute(&Command::AddPlants {
            style: Some(id),
            overrides: Default::default(),
            positions: vec![DVec2::new(0.0, 5.0)],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        let plain = render(ed.doc(), 400);
        assert!(
            plain.contains(r#"data-kind="tree_protection_zone""#) && plain.contains(r#"r="3600""#),
            "{plain}"
        );
        let pid = *ed.doc().objects.keys().next().unwrap();
        ed.execute(&Command::SetData {
            id: pid,
            set: [("rotation".to_string(), serde_json::json!(1.0))].into(),
            remove: vec![],
        })
        .unwrap();
        let turned = render(ed.doc(), 400);
        assert_ne!(plain, turned);
        assert!(turned.contains(r#"data-graphic="star""#));
    }

    #[test]
    fn existing_tree_shows_its_zone_and_dashes_removed_trees() {
        use crate::plants::tree::TreeAction;
        let tree = |action| Shape::ExistingTree {
            at: DVec2::new(10.0, 20.0),
            species: "Oak <old>".into(),
            dbh: 500.0,
            spread: 8000.0,
            height: 12000.0,
            condition: Default::default(),
            action,
            rpz_factor: None,
            rpz_cap: None,
        };
        let svg = one(tree(TreeAction::Retain));
        assert!(
            svg.contains(r#"data-kind="existing_tree" data-action="retain""#),
            "{svg}"
        );
        assert!(
            svg.contains(r#"r="6000""#) && svg.contains(r#"data-part="rpz""#),
            "{svg}"
        );
        assert!(svg.contains("stroke-dasharray") && svg.contains("Oak &lt;old&gt;"));
        let kept = svg.matches("stroke-dasharray").count();
        let removed = one(tree(TreeAction::Remove));
        assert!(
            removed.matches("stroke-dasharray").count() > kept,
            "{removed}"
        );
        assert!(
            removed.contains(r#"data-action="remove""#) && removed.contains("M -3990"),
            "{removed}"
        );
    }

    #[test]
    fn the_root_protection_zone_stops_at_15_m_unless_the_cap_is_changed() {
        let tree = |cap| Shape::ExistingTree {
            at: DVec2::ZERO,
            species: String::new(),
            dbh: 1500.0,
            spread: 8000.0,
            height: 12000.0,
            condition: Default::default(),
            action: Default::default(),
            rpz_factor: None,
            rpz_cap: cap,
        };
        assert!(one(tree(None)).contains(r#"r="15000""#));
        assert!(one(tree(Some(20_000.0))).contains(r#"r="18000""#));
        assert!(one(tree(Some(6_000.0))).contains(r#"r="6000""#));
    }

    #[test]
    fn plant_tag_shows_code_count_and_leader() {
        use crate::resource::ResourceId;
        let (mut ed, id) = style_with(crate::plants::style::PlantGraphic::Circle);
        ed.execute(&Command::AddPlants {
            style: Some(id),
            overrides: Default::default(),
            positions: vec![DVec2::ZERO; 3],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        add(
            &mut ed,
            Shape::PlantTag {
                at: DVec2::new(100.0, 200.0),
                anchor: DVec2::new(0.0, 50.0),
                style: id,
                height: 300.0,
            },
        );
        let svg = render(ed.doc(), 400);
        assert!(
            svg.contains(r#"<g data-kind="plant_tag"><line x1="0" y1="-50" x2="100" y2="-200""#),
            "{svg}"
        );
        assert!(svg.contains(">ZZ (3)</text>"), "{svg}");
        // A tag whose style is gone shows a question mark, if a file was edited by hand.
        let mut doc = ed.doc().clone();
        doc.resources.remove(ResourceId(id.0));
        assert!(render(&doc, 400).contains(">?</text>"));
    }

    #[test]
    fn grey_layers_are_drawn_in_a_faint_group() {
        use crate::command::{Command, Editor};
        use crate::model::{Attributes, FIRST_LAYER};
        use crate::visibility::VisMode;
        let mut ed = Editor::default();
        ed.execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(10.0, 10.0),
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
            layer: Some(crate::model::LayerId(work)),
            class: None,
        })
        .unwrap();
        let plain = render(ed.doc(), 200);
        assert!(!plain.contains("data-visibility"));
        ed.execute(&Command::SetVisibility {
            layers: vec![FIRST_LAYER],
            classes: vec![],
            mode: VisMode::Grey,
        })
        .unwrap();
        let grey = render(ed.doc(), 200);
        assert!(grey.contains(GREY_OPEN) && grey.contains("</g>"));
        assert!(grey.contains("<rect"), "the object is still drawn");
        ed.execute(&Command::SetVisibility {
            layers: vec![FIRST_LAYER],
            classes: vec![],
            mode: VisMode::Invisible,
        })
        .unwrap();
        assert!(!render(ed.doc(), 200).contains("<rect"));
    }

    /// An overlay for the tests: by object ID.
    struct Fixed {
        looks: Vec<(ObjectId, SvgLook)>,
        ghosts: Vec<crate::svg_overlay::SvgGhost>,
        view: Option<(DVec2, DVec2)>,
        legend: Vec<SvgLegendRow>,
    }

    impl SvgOverlay for Fixed {
        fn look(&self, id: ObjectId) -> SvgLook {
            self.looks
                .iter()
                .find(|(i, _)| *i == id)
                .map_or(SvgLook::Keep, |(_, l)| l.clone())
        }
        fn ghosts(&self) -> &[crate::svg_overlay::SvgGhost] {
            &self.ghosts
        }
        fn view(&self) -> Option<(DVec2, DVec2)> {
            self.view
        }
        fn legend(&self) -> &[SvgLegendRow] {
            &self.legend
        }
    }

    fn tint(rgb: [u8; 3], pattern: SvgPattern, status: &'static str) -> SvgTint {
        SvgTint {
            rgb,
            pattern,
            weight_factor: 2.0,
            fill_alpha: 0.5,
            opacity: 1.0,
            symbol: Some('+'),
            status,
        }
    }

    fn only(look: SvgLook, id: ObjectId) -> Fixed {
        Fixed {
            looks: vec![(id, look)],
            ghosts: vec![],
            view: None,
            legend: vec![],
        }
    }

    #[test]
    fn an_overlay_tints_dims_hides_and_adds_ghosts() {
        let mut ed = Editor::default();
        let rect = |x: f64| Shape::Rect {
            min: DVec2::new(x, 0.0),
            max: DVec2::new(x + 10.0, 10.0),
        };
        let a = ObjectId(add(&mut ed, rect(0.0)));
        let b = ObjectId(add(&mut ed, rect(20.0)));
        let c = ObjectId(add(&mut ed, rect(40.0)));
        let d = ObjectId(add(
            &mut ed,
            Shape::Plant {
                at: DVec2::new(60.0, 5.0),
                species: "Oak".into(),
                spread: 8.0,
                height: 10.0,
            },
        ));
        let ghost_object = Object::new(
            ObjectId(crate::uid::Uid::from_num(900)),
            FIRST_LAYER,
            NONE_CLASS,
            Shape::Circle {
                center: DVec2::new(-30.0, 5.0),
                radius: 4.0,
            },
        );
        let overlay = Fixed {
            looks: vec![
                (
                    a,
                    SvgLook::Tint(tint([0, 128, 48], SvgPattern::Solid, "added")),
                ),
                (b, SvgLook::Dim(0.25)),
                (c, SvgLook::Hide),
                (
                    d,
                    SvgLook::Tint(tint([205, 102, 0], SvgPattern::Dotted, "changed")),
                ),
            ],
            ghosts: vec![crate::svg_overlay::SvgGhost {
                object: ghost_object,
                tint: SvgTint {
                    opacity: 0.5,
                    ..tint([200, 30, 45], SvgPattern::Dashed, "removed")
                },
                kind: "removed",
            }],
            view: None,
            legend: vec![
                SvgLegendRow {
                    tint: Some(tint([0, 128, 48], SvgPattern::Solid, "added")),
                    text: "added 1 <b>".into(),
                },
                SvgLegendRow {
                    tint: Some(SvgTint {
                        symbol: None,
                        ..tint([200, 30, 45], SvgPattern::Dashed, "removed")
                    }),
                    text: "removed 0".into(),
                },
                SvgLegendRow {
                    tint: None,
                    text: "a note".into(),
                },
            ],
        };
        let svg = render_overlay(ed.doc(), 400, None, Some(&overlay));
        // The legend has a swatch, a mark and an escaped text for each row.
        assert!(svg.contains(r#"<g data-compare-legend="true""#), "{svg}");
        assert!(svg.contains("added 1 &lt;b&gt;") && svg.contains(">a note</text>"));
        assert!(svg.contains(r#"data-status="removed""#) && svg.contains("stroke-dasharray"));
        // The tinted object is in a group of its status, with its pen and a mark.
        assert!(svg.contains(r#"<g data-compare="added""#), "{svg}");
        assert!(svg.contains("rgba(0,128,48,1.000)"), "{svg}");
        assert!(svg.contains(r#"data-mark="added">+</text>"#), "{svg}");
        assert!(svg.contains(r#"<g data-compare="changed""#));
        // A tinted plant draws in the tint, not in the colors of its style.
        let plant = svg.split(r#"data-kind="plant""#).nth(1).unwrap();
        assert!(plant.contains("rgba(205,102,0,1.000)"), "{plant}");
        // The dimmed object is faint, the hidden one is gone, the ghost is drawn on top.
        assert!(svg.contains(r#"<g data-compare="unchanged" opacity="0.25">"#));
        assert!(!svg.contains(&format!(r#"data-id="{c}""#)));
        assert!(svg.contains(r#"data-ghost="removed""#) && svg.contains(r#"opacity="0.5""#));
        assert!(svg.contains(r#"<circle cx="-30""#));
        // The view box covers the ghost, which is left of the drawing.
        assert!(svg.contains(r#"viewBox="-"#), "{svg}");
        // A given view wins over the bounds of the drawing.
        let fixed = Fixed {
            looks: vec![],
            ghosts: vec![],
            view: Some((DVec2::new(-100.0, -100.0), DVec2::new(100.0, 100.0))),
            legend: vec![],
        };
        let svg = render_overlay(ed.doc(), 400, None, Some(&fixed));
        assert!(svg.contains(r#"viewBox="-100 -100 200 200""#), "{svg}");
        // No overlay: the same as `render_with`.
        assert_eq!(
            render_overlay(ed.doc(), 400, None, None),
            render_with(ed.doc(), 400, None)
        );
    }

    #[test]
    fn the_line_pattern_of_a_tint_is_dashed_dotted_or_solid() {
        let mut ed = Editor::default();
        let line = ObjectId(add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(50.0, 0.0),
            },
        ));
        let with = |p| {
            let o = only(SvgLook::Tint(tint([1, 2, 3], p, "added")), line);
            render_overlay(ed.doc(), 400, None, Some(&o))
        };
        assert!(with(SvgPattern::Dashed).contains("stroke-dasharray=\"300 200"));
        assert!(with(SvgPattern::Dotted).contains("stroke-dasharray=\"75 150"));
        assert!(!with(SvgPattern::Solid).contains("dasharray"));
    }
}
