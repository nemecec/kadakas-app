//! Native DXF entities: DIMENSION, LEADER and MTEXT for annotations, POLYFACE meshes and 3DFACE
//! for meshes, and the GEODATA object for the georeference.
//!
//! A DIMENSION refers to an anonymous block (`*D1`, ...) that holds its drawing, as in files from CAD
//! programs. The block is made from the same primitives that draw the annotation everywhere else, so
//! the dimension looks the same. The text of the dimension is written as a text override, so the
//! number is the one that Cadektor shows (its units and its decimals).

use cadektor_core::annot::draw::{Prim, text_width};
use cadektor_core::annot::objects::{Axis, DimMode};
use cadektor_core::annot::style::{Justify, Terminator};
use cadektor_core::annot::{Annot, Callout, dim};
use cadektor_core::georef::Georeference;
use cadektor_core::mesh3d::Mesh3d;
use cadektor_core::{DVec2, DVec3};

use super::{Ctx, H_MODEL_RECORD, W, common, escape_unicode, mtext_escape};
use cadektor_core::model::Object;

/// An anonymous block with the drawing of a dimension.
pub(super) struct AnonBlock {
    pub name: String,
    pub prims: Vec<Prim>,
}

/// The most faces of one polyface mesh (the file format counts them with 16 bits).
const MAX_FACES: usize = 30_000;

/// Writes the primitives of a dimension block: lines, closed outlines and text. The pen is BYBLOCK,
/// so the entities take the color of the dimension.
pub(super) fn write_block_prims(w: &mut W, unit: f64, prims: &[Prim]) {
    for p in prims {
        match p {
            Prim::Poly { pts, closed, .. } => {
                if pts.len() < 2 {
                    continue;
                }
                if pts.len() == 2 && !closed {
                    w.start("LINE");
                    w.pair(100, "AcDbEntity");
                    w.pair(8, "0");
                    w.pair(62, 0);
                    w.pair(100, "AcDbLine");
                    for (base, q) in [(10, pts[0]), (11, pts[1])] {
                        w.num(base, q.x / unit);
                        w.num(base + 10, q.y / unit);
                        w.num(base + 20, 0.0);
                    }
                } else {
                    w.start("LWPOLYLINE");
                    w.pair(100, "AcDbEntity");
                    w.pair(8, "0");
                    w.pair(62, 0);
                    w.pair(100, "AcDbPolyline");
                    w.pair(90, pts.len());
                    w.pair(70, i32::from(*closed));
                    for q in pts {
                        w.num(10, q.x / unit);
                        w.num(20, q.y / unit);
                    }
                }
            }
            Prim::Text {
                at,
                text,
                height,
                center,
                ..
            } => {
                let x = if *center {
                    at.x - text_width(text, *height) / 2.0
                } else {
                    at.x
                };
                w.start("TEXT");
                w.pair(100, "AcDbEntity");
                w.pair(8, "0");
                w.pair(62, 0);
                w.pair(100, "AcDbText");
                w.num(10, x / unit);
                w.num(20, at.y / unit);
                w.num(30, 0.0);
                w.num(40, height / unit);
                w.pair(1, escape_unicode(text).replace('\r', " "));
                w.pair(100, "AcDbText");
            }
        }
    }
}

/// Writes an annotation as native entities. False if this kind of annotation (or dimension mode)
/// has no native form here. The caller then writes lines and text.
pub(super) fn annotation(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool) -> bool {
    let cadektor_core::model::Shape::Annotation(a) = &o.shape else {
        return false;
    };
    match &**a {
        Annot::Text(t) => {
            let doc = cx.doc;
            let st = t
                .style
                .and_then(|s| doc.resources.text_style(s))
                .cloned()
                .unwrap_or_default();
            let paper = cadektor_core::annot::draw::Ctx::layer(doc, o.layer).paper;
            let h = st.size_mm * paper;
            let attach = match st.justify {
                Justify::Left => 1,
                Justify::Center => 2,
                Justify::Right => 3,
            };
            mtext(
                w,
                cx,
                o,
                layer,
                hidden,
                t.at + DVec2::Y * h,
                h,
                attach,
                &t.text,
            );
            true
        }
        Annot::Callout(c) => {
            callout(w, cx, o, layer, hidden, c);
            true
        }
        Annot::Dimension(_) => dimension(w, cx, o, layer, hidden),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn mtext(
    w: &mut W,
    cx: &Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    at: DVec2,
    height: f64,
    attach: i32,
    text: &str,
) {
    common(w, "MTEXT", layer, o, hidden);
    w.pair(100, "AcDbMText");
    cx.pt3(w, 10, at.x, at.y, 0.0);
    w.num(40, height / cx.unit);
    w.pair(71, attach);
    w.pair(72, 1);
    let esc = mtext_escape(text);
    let chars: Vec<char> = esc.chars().collect();
    let chunks: Vec<String> = chars
        .chunks(super::TEXT_LIMIT)
        .map(|c| c.iter().collect())
        .collect();
    if chunks.is_empty() {
        w.pair(1, "");
    }
    for (i, chunk) in chunks.iter().enumerate() {
        w.pair(if i + 1 == chunks.len() { 1 } else { 3 }, chunk);
    }
}

fn callout(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool, c: &Callout) {
    let doc = cx.doc;
    let paper = cadektor_core::annot::draw::Ctx::layer(doc, o.layer).paper;
    let st = c
        .style
        .and_then(|s| doc.resources.text_style(s))
        .cloned()
        .unwrap_or_default();
    let h = st.size_mm * paper;
    let size = 2.5 * paper;
    for l in &c.leaders {
        let mut pts = l.points.clone();
        pts.push(c.at);
        if pts.len() < 2 {
            continue;
        }
        let arrow = c.terminator == Terminator::Arrow;
        common(w, "LEADER", layer, o, hidden);
        w.pair(100, "AcDbLeader");
        w.pair(3, "Standard");
        w.pair(71, i32::from(arrow));
        w.pair(72, 0);
        w.pair(73, 3);
        w.pair(76, pts.len());
        for p in &pts {
            cx.pt3(w, 10, p.x, p.y, 0.0);
        }
        if arrow {
            // The size of the arrow head for this leader only (DIMASZ, group 41 of the dimension style).
            w.pair(1001, "ACAD");
            w.pair(1000, "DSTYLE");
            w.pair(1002, "{");
            w.pair(1070, 41);
            w.num(1040, size / cx.unit);
            w.pair(1002, "}");
        } else {
            // Other end marks are drawn as outlines next to the leader.
            let dir = (pts[0] - pts[1]).normalize_or_zero();
            let mut prims = Vec::new();
            dim::terminator(&mut prims, c.terminator, size, pts[0], dir);
            for p in prims {
                if let Prim::Poly { pts, closed, .. } = p {
                    let verts: Vec<(DVec2, f64)> = pts.iter().map(|q| (*q, 0.0)).collect();
                    super::lwpolyline(w, cx, o, layer, hidden, &verts, closed);
                }
            }
        }
    }
    // The text sits on the side that the first leader does not come from.
    let right = c
        .leaders
        .first()
        .and_then(|l| l.points.first())
        .is_none_or(|tip| tip.x <= c.at.x);
    let side = if right { 1.0 } else { -1.0 };
    let shoulder = c.shoulder_mm * paper;
    let end = c.at + DVec2::X * side * shoulder;
    if shoulder > 0.0 {
        common(w, "LINE", layer, o, hidden);
        w.pair(100, "AcDbLine");
        cx.pt3(w, 10, c.at.x, c.at.y, 0.0);
        cx.pt3(w, 11, end.x, end.y, 0.0);
    }
    let text = cadektor_core::annot::draw::callout_text(doc, c);
    if text.trim().is_empty() {
        return;
    }
    let x = if right { end.x + paper } else { end.x - paper };
    // Middle left (4) or middle right (6): the block of lines is centered on the shoulder line.
    mtext(
        w,
        cx,
        o,
        layer,
        hidden,
        DVec2::new(x, end.y),
        h,
        if right { 4 } else { 6 },
        &text,
    );
}

fn dimension(w: &mut W, cx: &mut Ctx, o: &Object, layer: &str, hidden: bool) -> bool {
    let cadektor_core::model::Shape::Annotation(a) = &o.shape else {
        return false;
    };
    let Annot::Dimension(d) = &**a else {
        return false;
    };
    let doc = cx.doc;
    let p = &d.points;
    // Linear, radial and diameter dimensions have a native form. The others stay lines and text.
    let supported = match d.mode {
        DimMode::Linear { .. } => p.len() >= 2,
        DimMode::Radial | DimMode::Diameter => p.len() >= 2,
        _ => false,
    };
    if !supported {
        return false;
    }
    let style = d
        .style
        .and_then(|s| doc.resources.dimension_style(s))
        .cloned()
        .unwrap_or_default();
    let ctx = cadektor_core::annot::draw::Ctx::layer(doc, o.layer);
    let prims = cadektor_annot::prims(doc, o);
    let texts = dim::texts(d, &style, &ctx);
    let Some(text) = texts.first() else {
        return false;
    };
    // The middle of the text, from the first text primitive.
    let mid = prims
        .iter()
        .find_map(|q| match q {
            Prim::Text {
                at,
                text,
                height,
                center,
                ..
            } => {
                let cx_ = if *center {
                    at.x
                } else {
                    at.x + text_width(text, *height) / 2.0
                };
                Some(DVec2::new(cx_, at.y + height * 0.35))
            }
            _ => None,
        })
        .unwrap_or((p[0] + p[1]) / 2.0);
    let name = format!("*D{}", cx.anon_blocks.len() + 1);
    cx.anon_blocks.push(AnonBlock {
        name: name.clone(),
        prims,
    });
    let kind = match d.mode {
        DimMode::Linear { .. } => 32,
        DimMode::Radial => 4 | 32,
        _ => 3 | 32,
    };
    // A dimension of a linear kind gets the point on its dimension line as definition point.
    let off = d.offset * ctx.paper;
    let u = match d.mode {
        DimMode::Linear {
            axis: Axis::Horizontal,
        } => DVec2::X,
        DimMode::Linear {
            axis: Axis::Vertical,
        } => DVec2::Y,
        _ => (p[p.len() - 1] - p[0]).normalize_or(DVec2::X),
    };
    let def = match d.mode {
        DimMode::Linear { .. } => {
            let n = u.perp();
            let c0 = p[0].dot(n) + off;
            p[1] - n * (p[1].dot(n) - c0)
        }
        DimMode::Radial => p[0],
        _ => p[0] - (p[1] - p[0]),
    };
    common(w, "DIMENSION", layer, o, hidden);
    w.pair(100, "AcDbDimension");
    w.pair(2, &name);
    w.pair(3, "Standard");
    cx.pt3(w, 10, def.x, def.y, 0.0);
    cx.pt3(w, 11, mid.x, mid.y, 0.0);
    w.pair(70, kind);
    w.pair(71, 5);
    w.pair(1, mtext_escape(text));
    match d.mode {
        DimMode::Linear { .. } => {
            w.pair(100, "AcDbAlignedDimension");
            cx.pt3(w, 13, p[0].x, p[0].y, 0.0);
            cx.pt3(w, 14, p[1].x, p[1].y, 0.0);
            w.num(50, u.to_angle().to_degrees());
            w.pair(100, "AcDbRotatedDimension");
        }
        DimMode::Radial => {
            w.pair(100, "AcDbRadialDimension");
            cx.pt3(w, 15, p[1].x, p[1].y, 0.0);
            w.num(40, 0.0);
        }
        _ => {
            w.pair(100, "AcDbDiameterDimension");
            cx.pt3(w, 15, p[1].x, p[1].y, 0.0);
            w.num(40, 0.0);
        }
    }
    true
}

/// A triangle mesh as one or more polyface meshes, or as one 3DFACE per triangle.
pub(super) fn mesh(
    w: &mut W,
    cx: &Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    vertices: &[DVec3],
    triangles: &[[u32; 3]],
) {
    if cx.mesh_entity == crate::options::MeshEntity::Face3d {
        for t in triangles {
            let p = |i: u32| vertices[i as usize];
            common(w, "3DFACE", layer, o, hidden);
            w.pair(100, "AcDbFace");
            for (k, c) in [t[0], t[1], t[2], t[2]].into_iter().enumerate() {
                let q = p(c);
                cx.pt3(w, 10 + k as i32, q.x, q.y, q.z);
            }
        }
        return;
    }
    for chunk in triangles.chunks(MAX_FACES) {
        // The vertices of this piece, numbered again from 1.
        let mut map = std::collections::HashMap::new();
        let mut used: Vec<u32> = Vec::new();
        let faces: Vec<[i32; 3]> = chunk
            .iter()
            .map(|t| {
                t.map(|i| {
                    *map.entry(i).or_insert_with(|| {
                        used.push(i);
                        used.len() as i32
                    })
                })
            })
            .collect();
        common(w, "POLYLINE", layer, o, hidden);
        w.pair(100, "AcDbPolyFaceMesh");
        w.pair(66, 1);
        cx.pt3(w, 10, 0.0, 0.0, 0.0);
        w.pair(70, 64);
        w.pair(71, used.len());
        w.pair(72, faces.len());
        for i in &used {
            let q = vertices[*i as usize];
            w.start("VERTEX");
            w.pair(100, "AcDbEntity");
            w.pair(8, layer);
            w.pair(100, "AcDbVertex");
            w.pair(100, "AcDbPolyFaceMeshVertex");
            cx.pt3(w, 10, q.x, q.y, q.z);
            w.pair(70, 192);
        }
        for f in &faces {
            w.start("VERTEX");
            w.pair(100, "AcDbEntity");
            w.pair(8, layer);
            w.pair(100, "AcDbVertex");
            w.pair(100, "AcDbFaceRecord");
            cx.pt3(w, 10, 0.0, 0.0, 0.0);
            w.pair(70, 128);
            for (k, v) in f.iter().enumerate() {
                w.pair(71 + k as i32, v);
            }
        }
        w.start("SEQEND");
        w.pair(100, "AcDbEntity");
        w.pair(8, layer);
    }
}

/// The mesh of a mesh object.
pub(super) fn mesh_object(w: &mut W, cx: &Ctx, o: &Object, layer: &str, hidden: bool, m: &Mesh3d) {
    mesh(w, cx, o, layer, hidden, &m.vertices, &m.triangles);
}

/// The CRS alias that a GEODATA object names: `EPSG:3067` for an EPSG code. `None` for other CRSs.
pub(super) fn epsg_of(g: &Georeference) -> Option<String> {
    let c = g.crs.trim();
    let digits = c
        .strip_prefix("EPSG:")
        .or_else(|| c.strip_prefix("epsg:"))?;
    (!digits.is_empty() && digits.chars().all(|d| d.is_ascii_digit()))
        .then(|| format!("EPSG:{digits}"))
}

/// The GEODATA object for the georeference. `design` is the point in drawing coordinates that has the
/// map position `g.origin`-based `reference` (meters). `unit_m` is meters per drawing unit.
pub(super) fn geodata(
    objects: &mut W,
    epsg: &str,
    design: DVec2,
    reference: DVec2,
    north: DVec2,
    insunits: i64,
    unit_m: f64,
) {
    objects.pair(0, "GEODATA");
    let h = objects.handle();
    objects.pair(5, h);
    objects.pair(102, "{ACAD_REACTORS");
    objects.pair(330, H_MODEL_RECORD);
    objects.pair(102, "}");
    objects.pair(330, H_MODEL_RECORD);
    objects.pair(100, "AcDbGeoData");
    objects.pair(90, 2);
    objects.pair(330, H_MODEL_RECORD);
    // 2: projected grid coordinates.
    objects.pair(70, 2);
    objects.num(10, design.x);
    objects.num(20, design.y);
    objects.num(30, 0.0);
    objects.num(11, reference.x);
    objects.num(21, reference.y);
    objects.num(31, 0.0);
    objects.num(40, unit_m);
    objects.pair(91, insunits);
    objects.num(41, unit_m);
    objects.pair(92, insunits);
    objects.num(210, 0.0);
    objects.num(220, 0.0);
    objects.num(230, 1.0);
    objects.num(12, north.x);
    objects.num(22, north.y);
    objects.pair(95, 1);
    objects.num(141, 1.0);
    objects.pair(294, 0);
    objects.num(142, 0.0);
    objects.num(143, 0.0);
    objects.pair(
        301,
        format!(
            "<?xml version=\"1.0\"?><Dictionary version=\"1.0\"><Alias id=\"{epsg}\" type=\"CoordinateSystem\"><ObjectId>{epsg}</ObjectId><Namespace>EPSG Code</Namespace></Alias></Dictionary>"
        ),
    );
    objects.pair(93, 0);
    objects.pair(96, 0);
}
