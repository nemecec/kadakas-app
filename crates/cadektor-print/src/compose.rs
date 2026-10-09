//! Builds the display list of a sheet layer: paper-space shapes, viewports and title blocks.

use std::collections::HashMap;
use std::sync::Arc;

use cadektor_core::mesh;
use cadektor_core::model::{Document, LayerId, LayerKind, Object, Shape};
use cadektor_core::plants::schedule;
use cadektor_core::sheets::{self, title_block_geometry, Fields, TitleBlock, Viewport};
use cadektor_render::{render, Camera, Projection, RenderMode, RenderOptions, StandardView, FOV_Y};
use glam::DVec2;

use crate::display::*;
use crate::plan::{annotation_prims, shape_prims, Xf};
use crate::PrintError;

const INK: Rgba = [0, 0, 0, 255];
/// Largest side of a 3D viewport image in pixels.
const MAX_IMAGE_SIDE: f64 = 3000.0;

/// Display lists by sheet layer, valid for one document revision. A new revision clears them,
/// so a sheet is built again only after the document changed. Rasters of 3D viewports are kept
/// in the same way.
#[derive(Default)]
pub struct SheetCache {
    revision: Option<u64>,
    pages: HashMap<LayerId, Arc<Page>>,
    /// Number of pages built so far. Tests use it to see that the cache works.
    pub builds: u64,
}

impl SheetCache {
    /// The page of a sheet layer for this revision of the document.
    pub fn page(&mut self, doc: &Document, revision: u64, layer: LayerId) -> Result<Arc<Page>, PrintError> {
        if self.revision != Some(revision) {
            self.pages.clear();
            self.revision = Some(revision);
        }
        if let Some(p) = self.pages.get(&layer) {
            return Ok(p.clone());
        }
        let page = Arc::new(compose_page(doc, layer)?);
        self.builds += 1;
        self.pages.insert(layer, page.clone());
        Ok(page)
    }
}

/// Builds the page of a sheet layer.
pub fn compose_page(doc: &Document, layer: LayerId) -> Result<Page, PrintError> {
    let l = doc.layer(layer).ok_or(PrintError::NoSheet(layer))?;
    if l.kind != LayerKind::Sheet {
        return Err(PrintError::NoSheet(layer));
    }
    let sheet = doc.sheet(layer).ok_or(PrintError::NoSheet(layer))?;
    let size = sheet.size_mm();
    let counts = schedule::tag_counts(doc);
    let mut prims = Vec::new();
    for obj in doc.objects.values().filter(|o| o.layer == layer && doc.is_shown(o)) {
        match &obj.shape {
            Shape::Viewport(vp) => viewport_prims(doc, vp, &counts, &mut prims),
            Shape::TitleBlock(tb) => title_block_prims(doc, layer, tb, size, &mut prims),
            _ => {
                let attrs = doc.resolved_attrs(obj);
                shape_prims(doc, obj, &attrs, &Xf::IDENTITY, &counts, &mut prims);
            }
        }
    }
    let title = format!("{} {}", sheet.number, sheet.title).trim().to_string();
    Ok(Page { size, title: if title.is_empty() { l.name.clone() } else { title }, prims })
}

fn rect_prim(lo: DVec2, hi: DVec2, width: f64) -> Prim {
    Prim::Path(PathPrim::stroked(PathPrim::polyline(&sheets::rect_polygon(lo, hi)), true, INK, width))
}

/// The primitives of a viewport: the frame (if shown), the clipped view and the annotations.
pub fn viewport_prims(doc: &Document, vp: &Viewport, counts: &schedule::TagCounts, out: &mut Vec<Prim>) {
    let clip = vp.clip_polygon();
    if clip.len() >= 3 {
        out.push(Prim::PushClip(clip.clone()));
        if vp.is_plan() {
            plan_view(doc, vp, counts, out);
        } else if let Some(img) = raster_view(doc, vp) {
            out.push(Prim::Image(img));
        }
        out.push(Prim::PopClip);
        if vp.show_frame {
            out.push(Prim::Path(PathPrim::stroked(PathPrim::polyline(&clip), true, INK, 0.35)));
        }
    }
    for a in &vp.annotations {
        annotation_prims(doc, vp, a, out);
    }
}

fn plan_view(doc: &Document, vp: &Viewport, counts: &schedule::TagCounts, out: &mut Vec<Prim>) {
    let xf = Xf::of(vp);
    let (lo, hi) = vp.model_window();
    for obj in doc.objects.values().filter(|o| vp.shows(doc, o)) {
        let (a, b) = obj.shape.bounds();
        if !(a.is_finite() && b.is_finite()) || b.x < lo.x || a.x > hi.x || b.y < lo.y || a.y > hi.y {
            continue;
        }
        let attrs = vp.resolve(doc, obj);
        shape_prims(doc, obj, &attrs, &xf, counts, out);
    }
}

/// The document as the viewport sees it: layers and classes shown or hidden, and class
/// overrides applied to the objects. Used for 3D views, which read the document.
pub fn viewport_document(doc: &Document, vp: &Viewport) -> Document {
    let mut d = doc.clone();
    let shown: Vec<(LayerId, bool)> = d.layers.iter().map(|l| (l.id, vp.layer_visible(doc, l.id))).collect();
    for (id, vis) in shown {
        if let Some(l) = d.layers.iter_mut().find(|l| l.id == id) {
            l.visible = vis;
        }
    }
    for c in &mut d.classes {
        c.visible = vp.class_visible(doc, c.id);
    }
    for o in d.objects.values_mut() {
        if let Some(ov) = vp.class_overrides.get(&o.class) {
            o.attrs.pen = ov.pen.or(o.attrs.pen);
            o.attrs.fill = ov.fill.or(o.attrs.fill);
            o.attrs.line_weight = ov.line_weight.or(o.attrs.line_weight);
        }
    }
    d
}

/// Renders a 3D viewport to an image, with the viewport scale and center, from its standard view.
/// Returns `None` for a view name that is not known.
fn raster_view(doc: &Document, vp: &Viewport) -> Option<ImagePrim> {
    let view = StandardView::from_name(&vp.view)?;
    let d = viewport_document(doc, vp);
    let origin = mesh::build(&d).origin;
    let scene = mesh::build_at(&d, origin);
    let size = vp.max - vp.min;
    let px = |mm: f64| (mm / 25.4 * vp.dpi).round().clamp(16.0, MAX_IMAGE_SIDE) as u32;
    let (w, h) = (px(size.x), px(size.y));
    let mut cam = Camera::standard(view, Projection::Orthographic);
    let c = vp.center - origin.truncate();
    let z = scene.bounds.map_or(0.0, |(lo, hi)| (lo.z + hi.z) / 2.0);
    cam.target = glam::Vec3::new(c.x as f32, c.y as f32, z);
    // The frame is `size.y` mm tall on paper, so it shows `size.y * scale` mm of the model.
    let half = size.y * vp.scale / 2.0;
    cam.distance = (half / (FOV_Y as f64 / 2.0).tan()) as f32;
    let mode = if vp.mode == "wireframe" { RenderMode::Wireframe } else { RenderMode::Shaded };
    let img = render(&scene, &cam, &RenderOptions { width: w, height: h, mode, background: [255, 255, 255, 255] });
    let rgb: Vec<u8> = img.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    Some(ImagePrim { min: vp.min, max: vp.max, width: w, height: h, rgb: Arc::new(rgb) })
}

/// Border, grid reference and title block.
pub fn title_block_prims(doc: &Document, layer: LayerId, tb: &TitleBlock, page: DVec2, out: &mut Vec<Prim>) {
    let fields = Fields::of(doc, layer, tb);
    let g = title_block_geometry(tb, &fields, page);
    out.push(rect_prim(g.border.0, g.border.1, 0.7));
    for (a, b) in &g.grid_lines {
        out.push(Prim::Path(PathPrim::stroked(PathPrim::polyline(&[*a, *b]), false, INK, 0.25)));
    }
    let h = (tb.margin * 0.4).clamp(1.5, 3.5);
    for (at, label) in &g.grid_labels {
        out.push(Prim::Text(TextPrim { at: *at - DVec2::new(0.0, h * 0.35), text: label.clone(), height: h, color: INK, align: Align::Center }));
    }
    if g.block.1.x > g.block.0.x {
        out.push(rect_prim(g.block.0, g.block.1, 0.5));
    }
    for (lo, hi, label, value, size) in &g.cells {
        out.push(rect_prim(*lo, *hi, 0.25));
        if !label.is_empty() {
            out.push(Prim::Text(TextPrim { at: DVec2::new(lo.x + 1.0, hi.y - 2.6), text: label.clone(), height: 1.8, color: [90, 90, 90, 255], align: Align::Left }));
        }
        if !value.is_empty() {
            // Shrink the text until it fits the cell.
            let room = (hi.x - lo.x - 2.0).max(1.0);
            let fit = room / (value.chars().count() as f64 * 0.52);
            out.push(Prim::Text(TextPrim { at: DVec2::new(lo.x + 1.0, lo.y + 2.0), text: value.clone(), height: size.min(fit), color: INK, align: Align::Left }));
        }
    }
}

/// Finds an object on a sheet layer by ID.
pub fn find_viewport(doc: &Document, id: cadektor_core::model::ObjectId) -> Option<(&Object, &Viewport)> {
    let o = doc.object(id)?;
    match &o.shape {
        Shape::Viewport(v) => Some((o, &**v)),
        _ => None,
    }
}
