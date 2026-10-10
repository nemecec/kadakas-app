//! Annotations on the sheet: the primitives of `kadakas_annot` become display list primitives.

use glam::DVec2;
use kadakas_core::annot::Annot;
use kadakas_core::annot::draw::{Ctx, Fill, Prim as AnnotPrim};
use kadakas_core::model::Document;

use crate::display::*;
use crate::plan::{Style, Xf};

/// Adds the primitives of an annotation. `ctx` says how sizes and real lengths relate to the
/// coordinates of the annotation, `xf` maps those coordinates to the page.
pub fn push(doc: &Document, a: &Annot, ctx: &Ctx, st: &Style, xf: &Xf, out: &mut Vec<Prim>) {
    let ink = st.stroke.unwrap_or([0, 0, 0, 255]);
    let color = |c: Option<kadakas_core::model::Color>| c.map_or(ink, |c| c.0);
    for p in kadakas_annot::prims_in(doc, a, ctx) {
        match p {
            AnnotPrim::Poly {
                pts,
                closed,
                fill,
                color: c,
            } => {
                let pen = color(c);
                let fill = match fill {
                    Fill::None => None,
                    Fill::Ink => Some(pen),
                    Fill::Color(f) => Some(f.0),
                };
                let mapped: Vec<DVec2> = pts.iter().map(|q| xf.p(*q)).collect();
                out.push(Prim::Path(PathPrim {
                    segs: PathPrim::polyline(&mapped),
                    closed,
                    stroke: Some(pen),
                    fill: fill.filter(|_| closed),
                    width: st.width,
                    dash: None,
                }));
            }
            AnnotPrim::Text {
                at,
                text,
                height,
                center,
                color: c,
                ..
            } => {
                out.push(Prim::Text(TextPrim {
                    at: xf.p(at),
                    text,
                    height: xf.len(height),
                    color: color(c),
                    align: if center { Align::Center } else { Align::Left },
                }));
            }
        }
    }
}

/// The context for an annotation object on a layer, seen through `xf` (paper space has `k` = 1).
pub fn layer_ctx(xf: &Xf) -> Ctx {
    Ctx {
        paper: 1.0 / xf.k,
        real: 1.0,
        view: None,
    }
}
