//! Section and elevation viewports: the cut through the 3D model as lines and fills.
//!
//! The cut comes from `cadektor-x3d`. Cut faces are filled with the fill and the hatch of the
//! class of their object. Plants show as outlines. The objects beyond are thin lines. The
//! viewport `center` is a point (distance along the section line, height) in mm.

use cadektor_core::mesh;
use cadektor_core::model::{Document, Object, ObjectId};
use cadektor_core::pattern;
use cadektor_core::section::{DEFAULT_BEYOND_WEIGHT, DEFAULT_CUT_WEIGHT, SectionView};
use cadektor_core::sheets::Viewport;
use cadektor_x3d::section::{self, CutFace, SectionDrawing, SectionSpec};
use std::sync::Arc;

use glam::{DVec2, DVec3};

use crate::compose::viewport_document;
use crate::display::*;
use crate::plan::Xf;
use crate::table::Tables;

/// The revision of the document of a page, for the cache of its site models.
const SITE_REVISION: u64 = 1;
/// Fill of cut faces when the class has no fill.
const POCHE: Rgba = [214, 214, 210, 255];
const INK: Rgba = [0, 0, 0, 255];
const BEYOND_INK: Rgba = [60, 60, 60, 255];

/// The drawing of a section viewport, or `None` if its section line is missing or not valid.
/// `site` holds the site models of the document at `revision`; the terrain is cut with the rest.
pub fn drawing(
    doc: &Document,
    vp: &Viewport,
    sv: &SectionView,
    site: &cadektor_site::SiteCache,
    revision: u64,
) -> Option<SectionDrawing> {
    let line = doc.object(sv.line)?;
    let spec = SectionSpec::from_line(line)?;
    spec.problem().is_none().then_some(())?;
    let shown = viewport_document(doc, vp);
    let mid = (spec.a + spec.b) / 2.0;
    let ext = cadektor_site::draw::scene_ext(site, revision);
    let scene = mesh::build_with(&shown, DVec3::new(mid.x, mid.y, 0.0), Some(&ext));
    Some(section::compute(&shown, &scene, &spec))
}

fn mapped(points: &[DVec2], xf: &Xf) -> Vec<PSeg> {
    PathPrim::polyline(&points.iter().map(|p| xf.p(*p)).collect::<Vec<_>>())
}

fn attrs_of(
    doc: &Document,
    vp: &Viewport,
    id: ObjectId,
) -> Option<(Object, cadektor_core::model::Attributes)> {
    let obj = doc.object(id)?;
    Some((obj.clone(), vp.resolve(doc, obj)))
}

fn face_prims(
    doc: &Document,
    vp: &Viewport,
    f: &CutFace,
    cut_width: f64,
    xf: &Xf,
    silhouette: bool,
    out: &mut Vec<Prim>,
) {
    let Some((_, attrs)) = attrs_of(doc, vp, f.object) else {
        return;
    };
    let rings: Vec<&Vec<DVec2>> = f.rings.iter().filter(|r| r.len() >= 3).collect();
    if rings.is_empty() {
        return;
    }
    let fill = attrs
        .fill
        .filter(|c| c.0[3] > 0)
        .map(|c| c.0)
        .unwrap_or(if silhouette {
            [96, 150, 84, 110]
        } else {
            POCHE
        });
    let pen = attrs.pen.map(|c| c.0).unwrap_or(INK);
    let width = if silhouette {
        cut_width * 0.5
    } else {
        cut_width
    };
    // The rings run in opposite directions for holes, so the nonzero rule leaves them empty.
    let segs: Vec<PSeg> = rings.iter().flat_map(|r| mapped(r, xf)).collect();
    out.push(Prim::Path(PathPrim {
        segs,
        closed: true,
        stroke: None,
        fill: Some(fill),
        width,
        dash: None,
    }));
    if !silhouette && let Some(pat) = attrs.fill_pattern {
        let owned: Vec<Vec<DVec2>> = rings.iter().map(|r| (*r).clone()).collect();
        for seg in pattern::fill_segments(&doc.resources, pat, &owned, vp.scale) {
            out.push(Prim::Path(PathPrim::stroked(
                mapped(&[seg.a, seg.b], xf),
                false,
                seg.color.map_or(pen, |c| c.0),
                seg.weight.unwrap_or(width * 0.4),
            )));
        }
    }
    for r in rings {
        out.push(Prim::Path(PathPrim::stroked(
            mapped(r, xf),
            true,
            pen,
            width,
        )));
    }
}

/// The key of the drawing of a section line seen through a viewport: the line and the layers and
/// visibility that the viewport shows. Callers that share a `SectionCache` use the same key.
pub fn drawing_key(vp: &Viewport, line: ObjectId) -> u64 {
    // FNV-1a over what decides the objects of the drawing. The frame and the scale are left
    // out: the drawing does not depend on them.
    let text = format!(
        "{:?}{:?}{:?}",
        vp.layers, vp.layer_overrides, vp.class_overrides
    );
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ line.0.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for b in text.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The primitives of a section viewport, inside the clip that the caller has set.
pub fn section_prims(
    doc: &Document,
    vp: &Viewport,
    sv: &SectionView,
    tables: &Tables,
    out: &mut Vec<Prim>,
) {
    let d = match tables.sections() {
        Some((store, revision)) => {
            let key = drawing_key(vp, sv.line);
            let mut cache = store.lock().unwrap_or_else(|e| e.into_inner());
            let mut missing = false;
            let d = cache.get(revision, key, || {
                match drawing(doc, vp, sv, tables.site(), SITE_REVISION) {
                    Some(d) => d,
                    None => {
                        missing = true;
                        SectionDrawing::default()
                    }
                }
            });
            if missing {
                return;
            }
            d
        }
        None => {
            let Some(d) = drawing(doc, vp, sv, tables.site(), SITE_REVISION) else {
                return;
            };
            Arc::new(d)
        }
    };
    prims_of(doc, vp, sv, &d, out);
}

/// The primitives of a computed drawing.
pub fn prims_of(
    doc: &Document,
    vp: &Viewport,
    sv: &SectionView,
    d: &SectionDrawing,
    out: &mut Vec<Prim>,
) {
    let xf = Xf::of(vp);
    let cut = sv.cut_weight.unwrap_or(DEFAULT_CUT_WEIGHT);
    let beyond = sv.beyond_weight.unwrap_or(DEFAULT_BEYOND_WEIGHT);
    // Far lines first, so that nearer ones draw over them.
    let mut edges: Vec<&section::Edge> = d.edges.iter().collect();
    edges.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    for e in edges {
        out.push(Prim::Path(PathPrim::stroked(
            mapped(&[e.a, e.b], &xf),
            false,
            BEYOND_INK,
            beyond,
        )));
    }
    for f in &d.silhouettes {
        face_prims(doc, vp, f, cut, &xf, true, out);
    }
    for f in &d.faces {
        face_prims(doc, vp, f, cut, &xf, false, out);
    }
    for p in &d.profiles {
        let pen = attrs_of(doc, vp, p.object)
            .and_then(|(_, a)| a.pen)
            .map_or(INK, |c| c.0);
        out.push(Prim::Path(PathPrim::stroked(
            mapped(&p.points, &xf),
            false,
            pen,
            cut,
        )));
    }
}
