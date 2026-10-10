//! Builds the display list of a sheet layer: paper-space shapes, viewports and title blocks.

use std::collections::HashMap;
use std::sync::Arc;

use cadektor_core::hardscape::{NoTerrain, TerrainSource};
use cadektor_core::mesh;
use cadektor_core::model::{Class, Document, Layer, LayerId, LayerKind, Object, Shape};
use cadektor_core::plants::schedule;
use cadektor_core::sheets::{self, Fields, TitleBlock, Viewport, title_block_geometry};
use cadektor_render::{Camera, FOV_Y, Projection, RenderMode, RenderOptions, StandardView, render};
use glam::DVec2;

use crate::PrintError;
use crate::display::*;
use crate::plan::{Xf, annotation_prims, shape_prims};
use crate::table::Tables;

const INK: Rgba = [0, 0, 0, 255];
/// Largest side of a 3D viewport image in pixels.
const MAX_IMAGE_SIDE: f64 = 3000.0;
/// Most pixels of 3D viewport images on one page, and in one export (about 64 megapixels).
pub const MAX_RASTER_PIXELS: u64 = 64_000_000;
/// Pages that the cache keeps.
const MAX_CACHED_PAGES: usize = 8;

/// Display lists by sheet layer, valid for one document revision. A new revision clears them,
/// so a sheet is built again only after the document changed. Rasters of 3D viewports are kept
/// in the same way.
#[derive(Default)]
pub struct SheetCache {
    revision: Option<u64>,
    pages: HashMap<LayerId, Arc<Page>>,
    /// Layers in the order of use, the oldest first.
    used: std::collections::VecDeque<LayerId>,
    /// Number of pages built so far. Tests use it to see that the cache works.
    pub builds: u64,
}

impl SheetCache {
    /// The page of a sheet layer for this revision of the document.
    pub fn page(
        &mut self,
        doc: &Document,
        revision: u64,
        layer: LayerId,
    ) -> Result<Arc<Page>, PrintError> {
        self.page_on(doc, revision, layer, &NoTerrain)
    }

    /// Like `page`, with the ground of the site models for the worksheet tables. The ground must
    /// belong to this revision.
    pub fn page_on(
        &mut self,
        doc: &Document,
        revision: u64,
        layer: LayerId,
        ground: &dyn TerrainSource,
    ) -> Result<Arc<Page>, PrintError> {
        if self.revision != Some(revision) {
            self.pages.clear();
            self.used.clear();
            self.revision = Some(revision);
        }
        if let Some(p) = self.pages.get(&layer) {
            self.used.retain(|l| *l != layer);
            self.used.push_back(layer);
            return Ok(p.clone());
        }
        let page = Arc::new(compose_page_on(doc, layer, ground)?);
        self.builds += 1;
        // Pages hold images, so the cache keeps a few of them only.
        while self.pages.len() >= MAX_CACHED_PAGES {
            match self.used.pop_front() {
                Some(old) => self.pages.remove(&old),
                None => break,
            };
        }
        self.pages.insert(layer, page.clone());
        self.used.push_back(layer);
        Ok(page)
    }
}

/// Builds the page of a sheet layer.
pub fn compose_page(doc: &Document, layer: LayerId) -> Result<Page, PrintError> {
    compose_page_on(doc, layer, &NoTerrain)
}

/// Builds the page of a sheet layer. `ground` gives the heights of the site models, for the
/// worksheet tables on the page.
pub fn compose_page_on(
    doc: &Document,
    layer: LayerId,
    ground: &dyn TerrainSource,
) -> Result<Page, PrintError> {
    let l = doc.layer(layer).ok_or(PrintError::NoSheet(layer))?;
    if l.kind != LayerKind::Sheet {
        return Err(PrintError::NoSheet(layer));
    }
    let sheet = doc.sheet(layer).ok_or(PrintError::NoSheet(layer))?;
    let size = sheet.size_mm();
    let counts = schedule::tag_counts(doc);
    let tables = Tables::build(doc, ground);
    let mut prims = Vec::new();
    let mut budget = MAX_RASTER_PIXELS;
    let mut rasters = 0;
    for obj in doc
        .objects
        .values()
        .filter(|o| o.layer == layer && doc.is_shown(o))
    {
        match &obj.shape {
            Shape::Viewport(vp) => {
                // A sheet layer holds a few 3D viewports at most (the commands enforce it too).
                if !vp.is_plan() {
                    rasters += 1;
                }
                let mut left = if rasters > cadektor_core::sheets::MAX_3D_VIEWPORTS {
                    0
                } else {
                    budget
                };
                viewport_prims(doc, vp, &counts, &tables, &mut left, &mut prims);
                if rasters <= cadektor_core::sheets::MAX_3D_VIEWPORTS {
                    budget = left;
                }
            }
            Shape::TitleBlock(tb) => title_block_prims(doc, layer, tb, size, &mut prims),
            _ => {
                let attrs = doc.resolved_attrs(obj);
                shape_prims(
                    doc,
                    obj,
                    &attrs,
                    &Xf::IDENTITY,
                    &counts,
                    &tables,
                    &mut prims,
                );
            }
        }
    }
    let title = format!("{} {}", sheet.number, sheet.title)
        .trim()
        .to_string();
    Ok(Page {
        size,
        title: if title.is_empty() {
            l.name.clone()
        } else {
            title
        },
        prims,
    })
}

fn rect_prim(lo: DVec2, hi: DVec2, width: f64) -> Prim {
    Prim::Path(PathPrim::stroked(
        PathPrim::polyline(&sheets::rect_polygon(lo, hi)),
        true,
        INK,
        width,
    ))
}

/// The primitives of a viewport: the frame (if shown), the clipped view and the annotations.
pub fn viewport_prims(
    doc: &Document,
    vp: &Viewport,
    counts: &schedule::TagCounts,
    tables: &Tables,
    budget: &mut u64,
    out: &mut Vec<Prim>,
) {
    let clip = vp.clip_polygon();
    if clip.len() >= 3 {
        out.push(Prim::PushClip(clip.clone()));
        if vp.is_plan() {
            plan_view(doc, vp, counts, tables, out);
        } else if let Some(img) = raster_view(doc, vp, budget) {
            out.push(Prim::Image(img));
        }
        out.push(Prim::PopClip);
        if vp.show_frame {
            out.push(Prim::Path(PathPrim::stroked(
                PathPrim::polyline(&clip),
                true,
                INK,
                0.35,
            )));
        }
    }
    for a in &vp.annotations {
        annotation_prims(doc, vp, a, out);
    }
}

fn plan_view(
    doc: &Document,
    vp: &Viewport,
    counts: &schedule::TagCounts,
    tables: &Tables,
    out: &mut Vec<Prim>,
) {
    let xf = Xf::of(vp);
    let (lo, hi) = vp.model_window();
    // Map underlays go below the vector objects.
    let under = |o: &&cadektor_core::model::Object| matches!(o.shape, Shape::Underlay(_));
    let shown = || doc.objects.values().filter(|o| vp.shows(doc, o));
    for obj in shown().filter(under).chain(shown().filter(|o| !under(&o))) {
        let (a, b) = doc.object_bounds(obj);
        if !(a.is_finite() && b.is_finite()) || b.x < lo.x || a.x > hi.x || b.y < lo.y || a.y > hi.y
        {
            continue;
        }
        let attrs = vp.resolve(doc, obj);
        shape_prims(doc, obj, &attrs, &xf, counts, tables, out);
    }
}

/// The part of the document that a viewport shows, for the 3D renderer: only the objects that the
/// viewport shows, with class overrides filled into the attributes that the object leaves to its class.
/// The rest of the document is not copied.
pub fn viewport_document(doc: &Document, vp: &Viewport) -> Document {
    let mut d = Document::default();
    d.layers = doc
        .layers
        .iter()
        .cloned()
        .map(|l| Layer { visible: true, ..l })
        .collect();
    d.classes = doc
        .classes
        .iter()
        .cloned()
        .map(|c| Class { visible: true, ..c })
        .collect();
    d.resources = doc.resources.clone();
    d.active_layer = doc.active_layer;
    d.active_class = doc.active_class;
    d.objects = doc
        .objects
        .values()
        .filter(|o| vp.shows(doc, o))
        .map(|o| {
            let mut o = o.clone();
            if let Some(ov) = vp.class_overrides.get(&o.class) {
                o.attrs.pen = o.attrs.pen.or(ov.pen);
                o.attrs.fill = o.attrs.fill.or(ov.fill);
                o.attrs.line_weight = o.attrs.line_weight.or(ov.line_weight);
            }
            (o.id, o)
        })
        .collect();
    d
}

/// The pixel size of a 3D viewport image. Both sides scale by one factor, so the picture keeps the
/// proportions of the frame. `budget` is what is left of the pixels of the page. `None` if the image
/// would be too small to see.
fn raster_size(vp: &Viewport, budget: &mut u64) -> Option<(u32, u32)> {
    let size = (vp.max - vp.min) / 25.4 * vp.dpi;
    let mut k = (MAX_IMAGE_SIDE / size.max_element()).min(1.0);
    let pixels = size.x * size.y * k * k;
    if pixels > *budget as f64 {
        k *= (*budget as f64 / pixels).sqrt();
    }
    let (w, h) = ((size.x * k).floor(), (size.y * k).floor());
    if w < 8.0 || h < 8.0 {
        return None;
    }
    *budget = budget.saturating_sub((w * h) as u64);
    Some((w as u32, h as u32))
}

/// Renders a 3D viewport to an image, with the viewport scale and center, from its standard view.
/// Returns `None` for a view name that is not known, or when no pixels are left.
fn raster_view(doc: &Document, vp: &Viewport, budget: &mut u64) -> Option<ImagePrim> {
    let view = StandardView::from_name(&vp.view)?;
    let (w, h) = raster_size(vp, budget)?;
    let d = viewport_document(doc, vp);
    let scene = mesh::build(&d);
    let origin = scene.origin;
    let size = vp.max - vp.min;
    let mut cam = Camera::standard(view, Projection::Orthographic);
    let c = vp.center - origin.truncate();
    let z = scene.bounds.map_or(0.0, |(lo, hi)| (lo.z + hi.z) / 2.0);
    cam.target = glam::Vec3::new(c.x as f32, c.y as f32, z);
    // The frame is `size.y` mm tall on paper, so it shows `size.y * scale` mm of the model.
    let half = size.y * vp.scale / 2.0;
    cam.distance = (half / (FOV_Y as f64 / 2.0).tan()) as f32;
    let mode = if vp.mode == "wireframe" {
        RenderMode::Wireframe
    } else {
        RenderMode::Shaded
    };
    let img = render(
        &scene,
        &cam,
        &RenderOptions {
            width: w,
            height: h,
            mode,
            background: [255, 255, 255, 255],
        },
    );
    let rgb: Vec<u8> = img
        .rgba
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    Some(ImagePrim {
        min: vp.min,
        max: vp.max,
        width: w,
        height: h,
        rgb: Arc::new(rgb),
    })
}

/// Border, grid reference and title block.
pub fn title_block_prims(
    doc: &Document,
    layer: LayerId,
    tb: &TitleBlock,
    page: DVec2,
    out: &mut Vec<Prim>,
) {
    let fields = Fields::of(doc, layer, tb);
    let g = title_block_geometry(tb, &fields, page);
    out.push(rect_prim(g.border.0, g.border.1, 0.7));
    for (a, b) in &g.grid_lines {
        out.push(Prim::Path(PathPrim::stroked(
            PathPrim::polyline(&[*a, *b]),
            false,
            INK,
            0.25,
        )));
    }
    let h = (tb.margin * 0.4).clamp(1.5, 3.5);
    for (at, label) in &g.grid_labels {
        out.push(Prim::Text(TextPrim {
            at: *at - DVec2::new(0.0, h * 0.35),
            text: label.clone(),
            height: h,
            color: INK,
            align: Align::Center,
        }));
    }
    if g.block.1.x > g.block.0.x {
        out.push(rect_prim(g.block.0, g.block.1, 0.5));
    }
    for (lo, hi, label, value, size) in &g.cells {
        out.push(rect_prim(*lo, *hi, 0.25));
        if !label.is_empty() {
            out.push(Prim::Text(TextPrim {
                at: DVec2::new(lo.x + 1.0, hi.y - 2.6),
                text: label.clone(),
                height: 1.8,
                color: [90, 90, 90, 255],
                align: Align::Left,
            }));
        }
        if !value.is_empty() {
            // Shrink the text until it fits the cell.
            let room = (hi.x - lo.x - 2.0).max(1.0);
            let fit = room / (value.chars().count() as f64 * 0.52);
            out.push(Prim::Text(TextPrim {
                at: DVec2::new(lo.x + 1.0, lo.y + 2.0),
                text: value.clone(),
                height: size.min(fit),
                color: INK,
                align: Align::Left,
            }));
        }
    }
}

/// Finds an object on a sheet layer by ID.
pub fn find_viewport(
    doc: &Document,
    id: cadektor_core::model::ObjectId,
) -> Option<(&Object, &Viewport)> {
    let o = doc.object(id)?;
    match &o.shape {
        Shape::Viewport(v) => Some((o, &**v)),
        _ => None,
    }
}
