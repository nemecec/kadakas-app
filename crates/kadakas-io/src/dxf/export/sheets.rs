//! Sheet layers as layouts of paper space (IOP-16).
//!
//! A sheet layer becomes a layout with its own paper space block, a LAYOUT object that holds the
//! paper, and the entities of the sheet in the order of the document, so what lies under a
//! viewport stays under it. A viewport becomes a VIEWPORT entity with the scale, the view and the
//! frozen layers, and a crop becomes the clipping outline. The first layout is written in the
//! section ENTITIES with the flag of paper space (as CAD programs do), the others in their blocks.
//!
//! Paper space is in millimeters. The coordinates of the sheet are not shifted by the
//! georeference and not divided by the unit of the model.

use std::collections::BTreeMap;

use kadakas_core::DVec2;
use kadakas_core::model::{Document, Object, ObjectId, Shape};
use kadakas_core::sheets::{AnnotationItem, Crop, Fields, Viewport, title_block_geometry};

use super::{
    Ctx, H_PAPER_BLOCK, H_PAPER_ENDBLK, H_PAPER_LAYOUT, H_PAPER_RECORD, W, common, lwpolyline,
    safe_name, write_object,
};
use crate::dxf::views::direction_of;
use crate::options::LayerMode;
use crate::{ExportOptions, Report};

/// One sheet layer written as a layout.
pub(super) struct SheetOut {
    /// The name of the layout.
    pub name: String,
    /// The name of the paper space block: `*Paper_Space`, `*Paper_Space0`, `*Paper_Space1`, ...
    pub block_name: String,
    pub record: String,
    pub layout: String,
    pub block: String,
    pub endblk: String,
    /// The paper in mm, with the orientation applied.
    pub page: DVec2,
    /// The entities of the layout. Empty for the first layout, whose entities are in ENTITIES.
    pub text: String,
}

/// The sheet layers of the document that an export writes, in the order of the document.
pub(super) fn sheet_layers(doc: &Document) -> Vec<kadakas_core::model::LayerId> {
    doc.sheet_layers().map(|l| l.id).collect()
}

#[allow(clippy::too_many_arguments)]
fn text_entity(
    w: &mut W,
    cx: &Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    at: DVec2,
    height: f64,
    text: &str,
) {
    common(w, "TEXT", layer, o, hidden);
    w.pair(100, "AcDbText");
    cx.pt3(w, 10, at.x, at.y, 0.0);
    w.num(40, height / cx.unit);
    w.pair(1, super::escape_unicode(text).replace('\r', " "));
    w.pair(100, "AcDbText");
}

fn line_entity(w: &mut W, cx: &Ctx, o: &Object, layer: &str, hidden: bool, a: DVec2, b: DVec2) {
    common(w, "LINE", layer, o, hidden);
    w.pair(100, "AcDbLine");
    cx.pt3(w, 10, a.x, a.y, 0.0);
    cx.pt3(w, 11, b.x, b.y, 0.0);
}

fn rect_entity(w: &mut W, cx: &Ctx, o: &Object, layer: &str, hidden: bool, lo: DVec2, hi: DVec2) {
    let pts = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    let verts: Vec<(DVec2, f64)> = pts.iter().map(|p| (*p, 0.0)).collect();
    lwpolyline(w, cx, o, layer, hidden, &verts, true);
}

/// The border, the grid reference and the title block as lines and text.
fn title_block(w: &mut W, cx: &Ctx, o: &Object, layer: &str, hidden: bool, page: DVec2) {
    let Shape::TitleBlock(tb) = &o.shape else {
        return;
    };
    let g = title_block_geometry(tb, &Fields::of(cx.doc, o.layer, tb), page);
    rect_entity(w, cx, o, layer, hidden, g.border.0, g.border.1);
    for (a, b) in &g.grid_lines {
        line_entity(w, cx, o, layer, hidden, *a, *b);
    }
    let h = (tb.margin * 0.4).clamp(1.5, 3.5);
    for (at, label) in &g.grid_labels {
        let width = kadakas_core::annot::draw::text_width(label, h);
        text_entity(
            w,
            cx,
            o,
            layer,
            hidden,
            *at - DVec2::new(width / 2.0, h * 0.35),
            h,
            label,
        );
    }
    if g.block.1.x > g.block.0.x {
        rect_entity(w, cx, o, layer, hidden, g.block.0, g.block.1);
    }
    for (lo, hi, label, value, size) in &g.cells {
        rect_entity(w, cx, o, layer, hidden, *lo, *hi);
        if !label.is_empty() {
            text_entity(
                w,
                cx,
                o,
                layer,
                hidden,
                DVec2::new(lo.x + 1.0, hi.y - 2.6),
                1.8,
                label,
            );
        }
        if !value.is_empty() {
            let room = (hi.x - lo.x - 2.0).max(1.0);
            let fit = room / (value.chars().count() as f64 * 0.52);
            text_entity(
                w,
                cx,
                o,
                layer,
                hidden,
                DVec2::new(lo.x + 1.0, lo.y + 2.0),
                size.min(fit),
                value,
            );
        }
    }
}

/// What the writing of a layout needs from the export.
pub(super) struct Env<'a> {
    pub full: &'a Document,
    pub opts: &'a ExportOptions,
    /// The unit of the model in mm, and the shift (mm) of the georeference.
    pub model_unit: f64,
    pub shift: DVec2,
}

/// The frozen layers of a viewport: handles are reserved here and used by the LAYER table.
pub(super) type Frozen = BTreeMap<String, String>;

/// Writes the VIEWPORT entity of a viewport object (and the clipping outline).
#[allow(clippy::too_many_arguments)]
fn viewport(
    w: &mut W,
    cx: &mut Ctx,
    env: &Env,
    o: &Object,
    v: &Viewport,
    id: i32,
    hidden: bool,
    frozen: &mut Frozen,
    report: &mut Report,
) -> bool {
    if v.section.is_some() {
        report.skip(
            "viewport",
            "section viewport: a DXF viewport shows the model, not a section",
        );
        return false;
    }
    let center = v.frame_center();
    let size = v.max - v.min;
    // A layer that the viewport hides is frozen in it. The DXF layers are classes (or design
    // layers in the mode `layers`).
    let mut names: Vec<String> = Vec::new();
    match cx.layer_mode {
        LayerMode::Classes => {
            for (class, ov) in &v.class_overrides {
                if ov.visible == Some(false)
                    && let Some(c) = env.full.class(*class)
                {
                    names.push(if c.name == "None" {
                        "0".into()
                    } else {
                        safe_name(&c.name)
                    });
                }
            }
            if v.layers.len()
                < env
                    .full
                    .layers
                    .iter()
                    .filter(|l| l.kind == kadakas_core::model::LayerKind::Design)
                    .count()
            {
                report.note("a viewport shows some design layers only: the DXF layers are classes, so every layer shows in it");
            }
        }
        LayerMode::Layers => {
            for l in env
                .full
                .layers
                .iter()
                .filter(|l| l.kind == kadakas_core::model::LayerKind::Design)
            {
                let off = v.layer_overrides.get(&l.id) == Some(&false) || !v.layers.contains(&l.id);
                if off {
                    names.push(safe_name(&l.name));
                }
            }
        }
    }
    names.sort();
    names.dedup();
    let model_center = (v.center + env.shift) / env.model_unit;
    let view_height = size.y * v.scale / env.model_unit;
    let dir = direction_of(&v.view);
    let has_clip = v.crop.is_some();
    let layer = if v.show_frame {
        "VIEWPORTS"
    } else {
        "Defpoints"
    };
    // A viewport is an entity of the sheet, on a layer of its own.
    w.start("VIEWPORT");
    w.pair(100, "AcDbEntity");
    w.pair(8, layer);
    if hidden {
        w.pair(60, 1);
    }
    w.pair(100, "AcDbViewport");
    w.pair(10, center.x);
    w.pair(20, center.y);
    w.pair(30, 0.0);
    w.num(40, size.x);
    w.num(41, size.y);
    w.pair(68, 1);
    w.pair(69, id);
    // The view center is relative to the target, which is the center of the view.
    for (c, x) in [
        (12, 0.0),
        (22, 0.0),
        (13, 0.0),
        (23, 0.0),
        (14, 0.5),
        (24, 0.5),
        (15, 0.5),
        (25, 0.5),
    ] {
        w.num(c, x);
    }
    w.num(16, dir[0]);
    w.num(26, dir[1]);
    w.num(36, dir[2]);
    w.num(17, model_center.x);
    w.num(27, model_center.y);
    w.num(37, 0.0);
    w.num(42, 50.0);
    w.num(43, 0.0);
    w.num(44, 0.0);
    w.num(45, view_height);
    w.num(50, 0.0);
    w.num(51, 0.0);
    w.pair(72, 1000);
    // 32768 is always on in files of AutoCAD. 65536 says that a clipping outline follows.
    w.pair(90, 32768 + if has_clip { 65536 } else { 0 });
    for n in &names {
        let h = frozen
            .entry(n.to_uppercase())
            .or_insert_with(|| w.handle())
            .clone();
        w.pair(331, h);
    }
    if has_clip {
        // The outline is written next, and gets the next handle.
        w.pair(340, format!("{:X}", w.next));
    }
    w.pair(
        281,
        match v.mode.as_str() {
            _ if v.is_plan() => 0,
            "wireframe" => 1,
            "hidden_line" | "dashed_hidden_line" => 2,
            _ => 3,
        },
    );
    // The outline that crops the viewport.
    if let Some(crop) = &v.crop {
        let pts: Vec<DVec2> = match crop {
            Crop::Polygon { points } => points.clone(),
            Crop::Rect { min, max } => vec![
                *min,
                DVec2::new(max.x, min.y),
                *max,
                DVec2::new(min.x, max.y),
            ],
        };
        let clip = Object::new(
            o.id,
            o.layer,
            o.class,
            Shape::Polyline {
                points: kadakas_core::curve::corners(&pts),
                closed: true,
            },
        );
        let verts: Vec<(DVec2, f64)> = pts.iter().map(|p| (*p, 0.0)).collect();
        lwpolyline(w, cx, &clip, "Defpoints", hidden, &verts, true);
    }
    // The annotations of the viewport are entities of the sheet, in paper millimeters from the
    // lower left corner of the frame.
    for a in &v.annotations {
        annotation(w, cx, o, v, a, hidden, report);
    }
    true
}

/// One annotation of a viewport as entities of the sheet.
#[allow(clippy::too_many_arguments)]
fn annotation(
    w: &mut W,
    cx: &mut Ctx,
    o: &Object,
    v: &Viewport,
    a: &kadakas_core::sheets::Annotation,
    hidden: bool,
    report: &mut Report,
) {
    let origin = v.min;
    let obj = |shape: Shape| {
        let mut x = Object::new(ObjectId(o.id.0), o.layer, a.class, shape);
        x.attrs = a.attrs.clone();
        x
    };
    let at = |p: DVec2| p + origin;
    match &a.item {
        AnnotationItem::Line { a: p, b: q } => {
            let x = obj(Shape::Line {
                a: at(*p),
                b: at(*q),
            });
            let name = cx.layer_name(&x);
            write_object(w, cx, &x, &name, hidden);
        }
        AnnotationItem::Text {
            at: p,
            text,
            height,
        } => {
            let x = obj(Shape::Text {
                at: at(*p),
                text: text.clone(),
                height: *height,
            });
            let name = cx.layer_name(&x);
            write_object(w, cx, &x, &name, hidden);
        }
        AnnotationItem::Shape { shape } => {
            let mut s = (**shape).clone();
            s.translate(origin);
            let x = obj(s);
            let name = cx.layer_name(&x);
            if !write_object(w, cx, &x, &name, hidden) {
                report.skip("viewport annotation", "shape that a sheet cannot write");
            }
        }
        AnnotationItem::Dimension {
            a: p,
            b: q,
            offset,
            text,
        } => {
            // A linear dimension as lines and text: the dimension line, two extension lines and the text.
            let (p, q) = (at(*p), at(*q));
            let dir = (q - p).normalize_or(DVec2::X);
            let n = dir.perp() * *offset;
            let x = obj(Shape::Line { a: p + n, b: q + n });
            let name = cx.layer_name(&x);
            write_object(w, cx, &x, &name, hidden);
            for end in [p, q] {
                let e = obj(Shape::Line { a: end, b: end + n });
                write_object(w, cx, &e, &name, hidden);
            }
            let label = text
                .clone()
                .unwrap_or_else(|| format!("{:.0}", (q - p).length() * v.scale));
            let height = 2.5;
            let width = kadakas_core::annot::draw::text_width(&label, height);
            let t = obj(Shape::Text {
                at: (p + q) / 2.0 + n - dir * (width / 2.0) + dir.perp() * 1.0,
                text: label,
                height,
            });
            write_object(w, cx, &t, &name, hidden);
        }
    }
}

/// Writes one sheet layer: the viewport of the paper itself, then every object of the sheet in
/// the order of the document.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_sheet(
    w: &mut W,
    cx: &mut Ctx,
    env: &Env,
    layer: kadakas_core::model::LayerId,
    page: DVec2,
    frozen: &mut Frozen,
    report: &mut Report,
) {
    // The viewport of the paper itself: every layout has one, with the id 1.
    w.start("VIEWPORT");
    w.pair(100, "AcDbEntity");
    w.pair(8, "Defpoints");
    w.pair(100, "AcDbViewport");
    w.num(10, page.x / 2.0);
    w.num(20, page.y / 2.0);
    w.num(30, 0.0);
    w.num(40, page.x);
    w.num(41, page.y);
    w.pair(68, 2);
    w.pair(69, 1);
    for (c, x) in [
        (12, page.x / 2.0),
        (22, page.y / 2.0),
        (13, 0.0),
        (23, 0.0),
        (14, 0.5),
        (24, 0.5),
        (15, 0.5),
        (25, 0.5),
        (16, 0.0),
        (26, 0.0),
        (36, 1.0),
        (17, 0.0),
        (27, 0.0),
        (37, 0.0),
        (42, 50.0),
        (43, 0.0),
        (44, 0.0),
        (45, page.y),
        (50, 0.0),
        (51, 0.0),
    ] {
        w.num(c, x);
    }
    w.pair(72, 1000);
    w.pair(90, 32768);
    let mut id = 1;
    for o in env.full.objects.values().filter(|o| o.layer == layer) {
        // `is_shown` is the visibility on any kind of layer (`is_visible` is the model's).
        if env.opts.visible_only && !env.full.is_shown(o) {
            continue;
        }
        // A hand-made document can hold objects on a sheet that a sheet does not take.
        if !kadakas_core::sheets::allowed_on_sheet(&o.shape) {
            report.skip(
                o.shape.kind(),
                "an object that a sheet layer does not hold (paper space)",
            );
            continue;
        }
        let hidden = env.full.layer(o.layer).is_some_and(|l| !l.visible);
        let name = cx.layer_name(o);
        let ok = match &o.shape {
            Shape::Viewport(v) => {
                id += 1;
                viewport(w, cx, env, o, v, id, hidden, frozen, report)
            }
            Shape::TitleBlock(_) => {
                title_block(w, cx, o, &name, hidden, page);
                true
            }
            _ => write_object(w, cx, o, &name, hidden),
        };
        if ok {
            report.count(o.shape.kind());
        } else if !matches!(o.shape, Shape::Viewport(_)) {
            report.skip(o.shape.kind(), "unsupported kind on a sheet");
        }
    }
}

/// The names for the layouts: the title of the sheet, made unique and safe.
pub(super) fn layout_names(doc: &Document, layers: &[kadakas_core::model::LayerId]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in layers {
        let sheet = doc.sheet(*id);
        let raw = sheet
            .as_ref()
            .map(|s| s.title.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| doc.layer(*id).map(|l| l.name.clone()))
            .unwrap_or_else(|| "Layout".into());
        let base = safe_name(&raw);
        let mut name = base.clone();
        let mut n = 2;
        while name.eq_ignore_ascii_case("Model")
            || out.iter().any(|x| x.eq_ignore_ascii_case(&name))
        {
            name = format!("{base} {n}");
            n += 1;
        }
        out.push(name);
    }
    out
}

/// The handles of the fixed records for the first layout, and the names of the blocks.
pub(super) fn first_handles() -> (String, String, String, String) {
    (
        H_PAPER_RECORD.into(),
        H_PAPER_LAYOUT.into(),
        H_PAPER_BLOCK.into(),
        H_PAPER_ENDBLK.into(),
    )
}
