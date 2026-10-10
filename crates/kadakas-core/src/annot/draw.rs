//! Drawing of annotations as primitives. Every renderer (SVG, the print sheet, the app canvas and
//! the DXF export) turns these primitives into its own output, so they all agree.

use glam::DVec2;

use crate::model::{Color, Document};
use crate::sheets::Viewport;

use super::dim;
use super::style::{DimensionStyle, Justify, TagFrame, TagStyle, Terminator, TextStyle};
use super::{Annot, Callout, DataTag, Legend, NorthArrow, RevisionCloud, ScaleBar, StyledText};

/// How a closed outline is filled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    None,
    /// With the pen color of the object.
    Ink,
    Color(Color),
}

/// One drawing primitive, in the coordinates of the layer (or paper mm in a viewport).
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Poly {
        pts: Vec<DVec2>,
        closed: bool,
        fill: Fill,
        color: Option<Color>,
    },
    /// `at` is the start of the baseline, or the middle of it when `center` is set. `height` is the font size.
    Text {
        at: DVec2,
        text: String,
        height: f64,
        center: bool,
        bold: bool,
        color: Option<Color>,
    },
}

impl Prim {
    pub fn line(a: DVec2, b: DVec2) -> Prim {
        Prim::Poly {
            pts: vec![a, b],
            closed: false,
            fill: Fill::None,
            color: None,
        }
    }

    pub fn poly(pts: Vec<DVec2>, closed: bool, fill: Fill) -> Prim {
        Prim::Poly {
            pts,
            closed,
            fill,
            color: None,
        }
    }

    pub fn text(at: DVec2, text: &str, height: f64, center: bool, color: Option<Color>) -> Prim {
        Prim::Text {
            at,
            text: text.to_string(),
            height,
            center,
            bold: false,
            color,
        }
    }

    /// Box around the primitive. Text counts with the width estimate `text_width`.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        match self {
            Prim::Poly { pts, .. } => pts.iter().fold(
                (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            ),
            Prim::Text {
                at,
                text,
                height,
                center,
                ..
            } => {
                let w = text_width(text, *height);
                let x0 = if *center { at.x - w / 2.0 } else { at.x };
                (
                    DVec2::new(x0, at.y - height * 0.2),
                    DVec2::new(x0 + w, at.y + height * 0.8),
                )
            }
        }
    }
}

/// Box around primitives, or `None` for none.
pub fn prims_bounds(prims: &[Prim]) -> Option<(DVec2, DVec2)> {
    prims
        .iter()
        .map(Prim::bounds)
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
}

/// A rough width of a text, the same estimate that the sheet output uses.
pub fn text_width(text: &str, height: f64) -> f64 {
    text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * height * 0.52
}

/// A regular polygon that approximates a circle.
pub fn circle_points(center: DVec2, r: f64, n: usize) -> Vec<DVec2> {
    (0..n)
        .map(|i| center + DVec2::from_angle(std::f64::consts::TAU * i as f64 / n as f64) * r)
        .collect()
}

/// Where an annotation is drawn: the factors between paper and the coordinates of the drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ctx {
    /// Drawing units per paper mm. The scale denominator on a design layer, 1 on a sheet and in a viewport.
    pub paper: f64,
    /// Drawing units per real mm. 1 on a layer, 1 over the scale in a viewport.
    pub real: f64,
    /// The mapping of model positions into a viewport. `None` on a layer.
    pub view: Option<ViewMap>,
}

/// Maps model positions to the annotation space of a viewport: paper mm from the lower left corner of the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewMap {
    pub center: DVec2,
    pub frame_half: DVec2,
    pub scale: f64,
}

impl Ctx {
    /// For an object on a layer. A sheet layer has the scale 1, so its sizes are paper mm.
    pub fn layer(doc: &Document, layer: crate::model::LayerId) -> Ctx {
        Ctx {
            paper: doc.layer(layer).map_or(1.0, |l| l.scale),
            real: 1.0,
            view: None,
        }
    }

    /// For the annotation space of a viewport.
    pub fn viewport(vp: &Viewport) -> Ctx {
        Ctx {
            paper: 1.0,
            real: 1.0 / vp.scale,
            view: Some(ViewMap {
                center: vp.center,
                frame_half: (vp.max - vp.min) / 2.0,
                scale: vp.scale,
            }),
        }
    }

    /// The position of a model point in the coordinates that the annotation uses.
    pub fn local(&self, model: DVec2) -> DVec2 {
        match self.view {
            Some(v) => (model - v.center) / v.scale + v.frame_half,
            None => model,
        }
    }
}

/// One line of a legend.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendRow {
    pub label: String,
    pub count: usize,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
}

/// The text that needs worksheet expressions. `kadakas-annot` implements it.
pub trait TextSource {
    /// The lines of a data tag (the fields of the tag style, calculated for the tagged object).
    fn tag_lines(&self, doc: &Document, tag: &DataTag, style: &TagStyle) -> Vec<String>;
    /// The rows of a legend, in display order.
    fn legend_rows(&self, doc: &Document, legend: &Legend) -> Vec<LegendRow>;
}

/// A source without expressions: tags show their template lines, legends are empty.
pub struct PlainSource;

impl TextSource for PlainSource {
    fn tag_lines(&self, _: &Document, _: &DataTag, style: &TagStyle) -> Vec<String> {
        style.lines.clone()
    }

    fn legend_rows(&self, _: &Document, _: &Legend) -> Vec<LegendRow> {
        Vec::new()
    }
}

/// The primitives of an annotation.
pub fn prims(doc: &Document, a: &Annot, ctx: &Ctx, src: &dyn TextSource) -> Vec<Prim> {
    match a {
        Annot::Dimension(d) => {
            let style = d
                .style
                .and_then(|s| doc.resources.dimension_style(s))
                .cloned()
                .unwrap_or_default();
            dim::prims(d, &style, ctx)
        }
        Annot::Text(t) => styled_text(doc, t, ctx),
        Annot::Callout(c) => callout(doc, c, ctx),
        Annot::DataTag(t) => data_tag(doc, t, ctx, src),
        Annot::Legend(l) => legend(doc, l, ctx, src),
        Annot::NorthArrow(n) => north_arrow(doc, n, ctx),
        Annot::ScaleBar(s) => scale_bar(doc, s, ctx),
        Annot::RevisionCloud(r) => revision_cloud(r, ctx),
    }
}

fn text_style(doc: &Document, id: Option<crate::resource::ResourceId>) -> TextStyle {
    id.and_then(|s| doc.resources.text_style(s))
        .cloned()
        .unwrap_or_default()
}

/// Lines of text from `at` (the baseline of the first line) downward.
fn text_lines(out: &mut Vec<Prim>, text: &str, at: DVec2, st: &TextStyle, ctx: &Ctx) {
    let h = st.size_mm * ctx.paper;
    for (i, line) in text.split('\n').enumerate() {
        let y = at.y - i as f64 * h * st.line_spacing;
        let w = text_width(line, h);
        let (x, center) = match st.justify {
            Justify::Left => (at.x, false),
            Justify::Center => (at.x, true),
            Justify::Right => (at.x - w, false),
        };
        out.push(Prim::Text {
            at: DVec2::new(x, y),
            text: line.to_string(),
            height: h,
            center,
            bold: st.bold,
            color: st.color,
        });
    }
}

fn styled_text(doc: &Document, t: &StyledText, ctx: &Ctx) -> Vec<Prim> {
    let st = text_style(doc, t.style);
    let mut out = Vec::new();
    text_lines(&mut out, &t.text, t.at, &st, ctx);
    out
}

/// The text of a callout: the note of its notes database if the key is found, else its own text.
pub fn callout_text(doc: &Document, c: &Callout) -> String {
    c.note
        .as_deref()
        .zip(c.notes_db)
        .and_then(|(key, db)| doc.resources.notes_db(db)?.get(key))
        .map_or_else(|| c.text.clone(), |n| n.text.clone())
}

fn callout(doc: &Document, c: &Callout, ctx: &Ctx) -> Vec<Prim> {
    let st = text_style(doc, c.style);
    let h = st.size_mm * ctx.paper;
    let mut out = Vec::new();
    let size = 2.5 * ctx.paper;
    for l in &c.leaders {
        let mut pts = l.points.clone();
        pts.push(c.at);
        out.push(Prim::poly(pts.clone(), false, Fill::None));
        if pts.len() >= 2 {
            let dir = (pts[0] - pts[1]).normalize_or_zero();
            terminator_prims(&mut out, c.terminator, size, pts[0], dir);
        }
    }
    // The text sits on the side that the first leader does not come from.
    let right = c
        .leaders
        .first()
        .and_then(|l| l.points.first())
        .is_none_or(|tip| tip.x <= c.at.x);
    let side = if right { 1.0 } else { -1.0 };
    let shoulder = c.shoulder_mm * ctx.paper;
    let end = c.at + DVec2::X * side * shoulder;
    if shoulder > 0.0 {
        out.push(Prim::line(c.at, end));
    }
    let gap = ctx.paper;
    let text = callout_text(doc, c);
    let lines = text.split('\n').count().max(1) as f64;
    // The first line is centered on the shoulder line when there is one line.
    let first = DVec2::new(
        0.0,
        end.y - h * 0.35 + (lines - 1.0) * h * st.line_spacing / 2.0,
    );
    let x = if right { end.x + gap } else { end.x - gap };
    let mut st2 = st.clone();
    st2.justify = if right { Justify::Left } else { Justify::Right };
    text_lines(&mut out, &text, DVec2::new(x, first.y), &st2, ctx);
    out
}

fn terminator_prims(out: &mut Vec<Prim>, kind: Terminator, size: f64, at: DVec2, dir: DVec2) {
    dim::terminator(out, kind, size, at, dir);
}

/// The live position of the reference point of a tagged object, in annotation coordinates.
pub fn tag_anchor(doc: &Document, t: &DataTag, ctx: &Ctx) -> DVec2 {
    doc.object(t.object)
        .and_then(|o| super::ref_point(&o.shape))
        .map_or(t.anchor, |p| ctx.local(p))
}

fn data_tag(doc: &Document, t: &DataTag, ctx: &Ctx, src: &dyn TextSource) -> Vec<Prim> {
    let Some(style) = doc.resources.tag_style(t.style) else {
        return Vec::new();
    };
    let alive = doc.object(t.object).is_some();
    let lines = if alive {
        src.tag_lines(doc, t, style)
    } else {
        vec!["?".to_string()]
    };
    let anchor = tag_anchor(doc, t, ctx);
    let at = anchor + t.offset * ctx.paper;
    let h = style.size_mm * ctx.paper;
    let pad = style.pad_mm * ctx.paper;
    let lh = h * 1.3;
    let w = lines.iter().map(|l| text_width(l, h)).fold(0.0, f64::max);
    let (bw, bh) = (
        w + 2.0 * pad,
        lines.len() as f64 * lh + 2.0 * pad - (lh - h),
    );
    let (lo, hi) = (
        at - DVec2::new(bw / 2.0, bh / 2.0),
        at + DVec2::new(bw / 2.0, bh / 2.0),
    );
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let y = hi.y - pad - h * 0.8 - i as f64 * lh;
        out.push(Prim::Text {
            at: DVec2::new(at.x, y),
            text: line.clone(),
            height: h,
            center: true,
            bold: false,
            color: style.color,
        });
    }
    let edge = match style.frame {
        TagFrame::Box => {
            out.push(Prim::poly(
                vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)],
                true,
                Fill::None,
            ));
            let q = anchor.clamp(lo, hi);
            (q != anchor).then_some(q)
        }
        TagFrame::Circle => {
            let r = (bw * bw + bh * bh).sqrt() / 2.0;
            out.push(Prim::poly(circle_points(at, r, 32), true, Fill::None));
            let d = anchor - at;
            (d.length() > r).then(|| at + d.normalize() * r)
        }
        TagFrame::Underline => {
            out.push(Prim::line(
                DVec2::new(lo.x, lo.y + pad * 0.5),
                DVec2::new(hi.x, lo.y + pad * 0.5),
            ));
            Some(DVec2::new(anchor.x.clamp(lo.x, hi.x), lo.y + pad * 0.5)).filter(|q| *q != anchor)
        }
        TagFrame::None => {
            let q = anchor.clamp(lo - DVec2::splat(pad * 0.5), hi + DVec2::splat(pad * 0.5));
            (q != anchor).then_some(q)
        }
    };
    if let (true, Some(q)) = (style.leader, edge) {
        out.push(Prim::line(q, anchor));
        terminator_prims(
            &mut out,
            style.terminator,
            2.5 * ctx.paper,
            anchor,
            (anchor - q).normalize_or_zero(),
        );
    }
    out
}

fn legend(doc: &Document, l: &Legend, ctx: &Ctx, src: &dyn TextSource) -> Vec<Prim> {
    let st = text_style(doc, l.style);
    let h = st.size_mm * ctx.paper;
    let sw = l.swatch_mm * ctx.paper;
    let rows = src.legend_rows(doc, l);
    let gap = ctx.paper * 2.0;
    let row_h = sw.max(h * 1.4) + gap * 0.5;
    let mut out = Vec::new();
    let mut y = l.at.y;
    if !l.title.is_empty() {
        y -= h * 1.2;
        out.push(Prim::Text {
            at: DVec2::new(l.at.x, y),
            text: l.title.clone(),
            height: h * 1.2,
            center: false,
            bold: true,
            color: st.color,
        });
        y -= h * 0.6;
    }
    let label_w = rows
        .iter()
        .map(|r| text_width(&r.label, h))
        .fold(0.0, f64::max);
    for r in &rows {
        let top = y;
        let bottom = top - sw;
        let x0 = l.at.x;
        out.push(Prim::Poly {
            pts: vec![
                DVec2::new(x0, bottom),
                DVec2::new(x0 + sw, bottom),
                DVec2::new(x0 + sw, top),
                DVec2::new(x0, top),
            ],
            closed: true,
            fill: r.fill.map_or(Fill::None, Fill::Color),
            color: r.stroke,
        });
        let base = (top + bottom) / 2.0 - h * 0.35;
        out.push(Prim::Text {
            at: DVec2::new(x0 + sw + gap, base),
            text: r.label.clone(),
            height: h,
            center: false,
            bold: false,
            color: st.color,
        });
        if l.show_count {
            out.push(Prim::Text {
                at: DVec2::new(x0 + sw + gap * 2.0 + label_w, base),
                text: r.count.to_string(),
                height: h,
                center: false,
                bold: false,
                color: st.color,
            });
        }
        y -= row_h;
    }
    out
}

fn rotate(p: DVec2, deg: f64) -> DVec2 {
    DVec2::from_angle(deg.to_radians()).rotate(p)
}

/// The counterclockwise angle of the north arrow on the page. The georeference maps the model to
/// the map by a turn of `rotation`, so true north lies at minus that angle in the model.
pub fn north_rotation_deg(doc: &Document, n: &NorthArrow) -> f64 {
    n.rotation_deg
        - if n.use_georef {
            doc.georef.as_ref().map_or(0.0, |g| g.rotation.to_degrees())
        } else {
            0.0
        }
}

fn north_arrow(doc: &Document, n: &NorthArrow, ctx: &Ctx) -> Vec<Prim> {
    let s = n.size_mm * ctx.paper;
    let rot = north_rotation_deg(doc, n);
    let at = |x: f64, y: f64| n.at + rotate(DVec2::new(x, y) * s, rot);
    let mut out = Vec::new();
    match n.style {
        super::NorthStyle::Arrow => {
            out.push(Prim::poly(
                vec![at(0.0, 0.4), at(-0.15, -0.4), at(0.0, -0.2)],
                true,
                Fill::None,
            ));
            out.push(Prim::poly(
                vec![at(0.0, 0.4), at(0.0, -0.2), at(0.15, -0.4)],
                true,
                Fill::Ink,
            ));
        }
        super::NorthStyle::Simple => {
            out.push(Prim::line(at(0.0, -0.4), at(0.0, 0.4)));
            out.push(Prim::poly(
                vec![at(-0.12, 0.2), at(0.0, 0.4), at(0.12, 0.2)],
                false,
                Fill::None,
            ));
        }
        super::NorthStyle::Compass => {
            out.push(Prim::poly(
                (0..48)
                    .map(|i| {
                        n.at + DVec2::from_angle(std::f64::consts::TAU * i as f64 / 48.0)
                            * (0.3 * s)
                    })
                    .collect(),
                true,
                Fill::None,
            ));
            out.push(Prim::line(at(0.0, -0.4), at(0.0, 0.4)));
            out.push(Prim::line(at(-0.4, 0.0), at(0.4, 0.0)));
            out.push(Prim::poly(
                vec![at(0.0, 0.4), at(-0.08, 0.1), at(0.08, 0.1)],
                true,
                Fill::Ink,
            ));
        }
    }
    let th = 0.28 * s;
    let label = at(0.0, 0.5) + rotate(DVec2::new(0.0, th * 0.5), rot);
    out.push(Prim::Text {
        at: label - DVec2::new(0.0, th * 0.35),
        text: "N".into(),
        height: th,
        center: true,
        bold: true,
        color: None,
    });
    out
}

/// The length of a scale bar in drawing units.
pub fn scale_bar_length(b: &ScaleBar, ctx: &Ctx) -> f64 {
    match b.scale {
        Some(s) => b.real_length_mm() / s * ctx.paper,
        None => b.real_length_mm() * ctx.real,
    }
}

fn number_text(x: f64) -> String {
    let s = format!("{:.3}", x);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn scale_bar(doc: &Document, b: &ScaleBar, ctx: &Ctx) -> Vec<Prim> {
    let st = text_style(doc, b.style);
    let total = scale_bar_length(b, ctx);
    let seg = total / b.segments as f64;
    let h = b.height_mm * ctx.paper;
    let th = st.size_mm.min(3.0) * ctx.paper;
    let mut out = Vec::new();
    for i in 0..b.segments {
        let x0 = b.at.x + seg * i as f64;
        out.push(Prim::poly(
            vec![
                DVec2::new(x0, b.at.y),
                DVec2::new(x0 + seg, b.at.y),
                DVec2::new(x0 + seg, b.at.y + h),
                DVec2::new(x0, b.at.y + h),
            ],
            true,
            if i % 2 == 0 { Fill::Ink } else { Fill::None },
        ));
    }
    for i in 0..=b.segments {
        let x = b.at.x + seg * i as f64;
        let mut label = number_text(b.segment * i as f64);
        if i == b.segments {
            label.push(' ');
            label.push_str(b.unit.label());
        }
        out.push(Prim::Text {
            at: DVec2::new(x, b.at.y + h + th * 0.5),
            text: label,
            height: th,
            center: true,
            bold: false,
            color: st.color,
        });
    }
    out
}

/// The scallops of a revision cloud around the points.
pub fn cloud_outline(points: &[DVec2], bump: f64) -> Vec<DVec2> {
    let mut pts: Vec<DVec2> = points.to_vec();
    if crate::geom::signed_area(&pts) < 0.0 {
        pts.reverse();
    }
    let mut out = Vec::new();
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let len = a.distance(b);
        if len < 1e-9 {
            continue;
        }
        let n = (len / bump).round().max(1.0) as usize;
        let step = (b - a) / n as f64;
        let theta = step.to_angle();
        let r = step.length() / 2.0;
        for k in 0..n {
            let m = a + step * (k as f64 + 0.5);
            for j in 0..8 {
                let t = std::f64::consts::PI * j as f64 / 8.0;
                out.push(m + DVec2::from_angle(theta + std::f64::consts::PI + t) * r);
            }
        }
    }
    out
}

fn revision_cloud(r: &RevisionCloud, ctx: &Ctx) -> Vec<Prim> {
    let mut out = vec![Prim::poly(
        cloud_outline(&r.points, r.bump_mm * ctx.paper),
        true,
        Fill::None,
    )];
    if r.mark {
        let hi = r
            .points
            .iter()
            .fold(DVec2::splat(f64::NEG_INFINITY), |m, p| m.max(*p));
        let c = hi + DVec2::splat(4.0 * ctx.paper);
        let s = 5.0 * ctx.paper;
        out.push(Prim::poly(
            vec![
                c + DVec2::new(0.0, s),
                c + DVec2::new(-s * 0.87, -s * 0.5),
                c + DVec2::new(s * 0.87, -s * 0.5),
            ],
            true,
            Fill::None,
        ));
        out.push(Prim::Text {
            at: c - DVec2::new(0.0, s * 0.25),
            text: r.revision.clone(),
            height: s * 0.7,
            center: true,
            bold: true,
            color: None,
        });
    }
    out
}

/// The dimension style that a dimension uses.
pub fn dimension_style(doc: &Document, d: &super::Dimension) -> DimensionStyle {
    d.style
        .and_then(|s| doc.resources.dimension_style(s))
        .cloned()
        .unwrap_or_default()
}
