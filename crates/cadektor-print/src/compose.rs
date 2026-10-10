//! Builds the display list of a sheet layer: paper-space shapes, viewports and title blocks.

use std::collections::HashMap;
use std::sync::Arc;

use cadektor_core::hardscape::{NoTerrain, TerrainSource};
use cadektor_core::mesh;
use cadektor_core::model::{Class, Document, Layer, LayerId, LayerKind, Object, Shape};
use cadektor_core::plants::schedule;
use cadektor_core::present::RenderStyle;
use cadektor_core::sheets::{self, Fields, TitleBlock, Viewport, title_block_geometry};
use cadektor_render::{Camera, FOV_Y, Projection, RenderMode, StandardView};
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
    /// The section drawings of the pages. Share them with the API so that a section is computed once.
    pub sections: cadektor_x3d::section::SharedSections,
    /// Another limit for the pixels of 3D viewport images (`None` is `MAX_RASTER_PIXELS`).
    /// Tests set a small one, so they need no big rasters.
    pub raster_limit: Option<u64>,
}

impl SheetCache {
    /// The limit for the pixels of 3D viewport images in force for this cache.
    pub fn raster_pixels(&self) -> u64 {
        self.raster_limit.unwrap_or(MAX_RASTER_PIXELS)
    }

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
        let page = Arc::new(compose_page_limited(
            doc,
            layer,
            ground,
            Some((self.sections.clone(), revision)),
            self.raster_pixels(),
        )?);
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
    compose_page_shared(doc, layer, ground, None)
}

/// Like `compose_page_on`, with a shared cache of section drawings and the revision of the document.
pub fn compose_page_shared(
    doc: &Document,
    layer: LayerId,
    ground: &dyn TerrainSource,
    sections: Option<(cadektor_x3d::section::SharedSections, u64)>,
) -> Result<Page, PrintError> {
    compose_page_limited(doc, layer, ground, sections, MAX_RASTER_PIXELS)
}

/// Like `compose_page_shared`, with another limit for the pixels of 3D viewport images on the page.
/// Tests use a small limit, so they need no big rasters.
pub fn compose_page_limited(
    doc: &Document,
    layer: LayerId,
    ground: &dyn TerrainSource,
    sections: Option<(cadektor_x3d::section::SharedSections, u64)>,
    max_pixels: u64,
) -> Result<Page, PrintError> {
    let l = doc.layer(layer).ok_or(PrintError::NoSheet(layer))?;
    if l.kind != LayerKind::Sheet {
        return Err(PrintError::NoSheet(layer));
    }
    let sheet = doc.sheet(layer).ok_or(PrintError::NoSheet(layer))?;
    let size = sheet.size_mm();
    let counts = schedule::tag_counts(doc);
    let mut tables = Tables::build(doc, ground);
    if let Some((store, revision)) = sections {
        tables = tables.with_sections(store, revision);
    }
    let mut prims = Vec::new();
    let mut budget = max_pixels;
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
        if let Some(sv) = &vp.section {
            crate::section::section_prims(doc, vp, sv, tables, out);
        } else if vp.is_plan() {
            plan_view(doc, vp, counts, tables, out);
        } else if let Some(style) = viewport_style(doc, vp).filter(|s| s.mode.is_hidden_line()) {
            hidden_line_prims(doc, vp, &style, MAX_HL_SEGMENTS, out);
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
    for obj in shown().filter(under).chain(shown().filter(|o| !under(o))) {
        let (a, b) = doc.object_bounds(obj);
        if !(a.is_finite() && b.is_finite()) || b.x < lo.x || a.x > hi.x || b.y < lo.y || a.y > hi.y
        {
            continue;
        }
        if cadektor_core::viewrules::hidden_by_phase(doc, obj) {
            continue;
        }
        let attrs = cadektor_core::viewrules::display_attrs(doc, obj, vp.resolve(doc, obj));
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
    d.drawing.presentation = doc.drawing.presentation.clone();
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

/// The render style of a 3D viewport: its render style resource, else its `mode`. `None` if the
/// view name is not known.
fn viewport_style(doc: &Document, vp: &Viewport) -> Option<RenderStyle> {
    StandardView::from_name(&vp.view)?;
    let from_resource = vp
        .render_style
        .and_then(|id| cadektor_core::present::render_style(doc, id))
        .cloned();
    Some(from_resource.unwrap_or_else(|| RenderStyle {
        mode: RenderMode::from_name(&vp.mode).unwrap_or_default(),
        ..RenderStyle::default()
    }))
}

/// The camera of a 3D viewport: the viewport scale and center, from its standard view.
fn viewport_camera(vp: &Viewport, scene: &cadektor_core::mesh::Scene) -> Option<Camera> {
    let view = StandardView::from_name(&vp.view)?;
    let size = vp.max - vp.min;
    let mut cam = Camera::standard(view, Projection::Orthographic);
    let c = vp.center - scene.origin.truncate();
    let z = scene.bounds.map_or(0.0, |(lo, hi)| (lo.z + hi.z) / 2.0);
    cam.target = glam::Vec3::new(c.x as f32, c.y as f32, z);
    // The frame is `size.y` mm tall on paper, so it shows `size.y * scale` mm of the model.
    let half = size.y * vp.scale / 2.0;
    cam.distance = (half / (FOV_Y as f64 / 2.0).tan()) as f32;
    Some(cam)
}

/// Renders a 3D viewport to an image, with the viewport scale and center, from its standard view.
/// Returns `None` for a view name that is not known, or when no pixels are left.
fn raster_view(doc: &Document, vp: &Viewport, budget: &mut u64) -> Option<ImagePrim> {
    let style = viewport_style(doc, vp)?;
    let (w, h) = raster_size(vp, budget)?;
    let d = viewport_document(doc, vp);
    let scene = mesh::build(&d);
    let cam = viewport_camera(vp, &scene)?;
    // Sheets are white. A style with its own background keeps it.
    let style = RenderStyle {
        background: if style.background == RenderStyle::default().background {
            cadektor_core::model::Color([255, 255, 255, 255])
        } else {
            style.background
        },
        ..style
    };
    let img = cadektor_render::styled::render_styled(&d, &scene, &cam, &style, w, h, 2);
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

/// Pixels per mm of paper for the hidden-line computation. Only the unit of the coordinates.
const HL_PX_PER_MM: f64 = 20.0;

/// Longest side of the hidden-line coordinate grid.
const HL_MAX_SIDE: f64 = 20_000.0;

/// Most line segments of one hidden-line viewport. More would make huge SVG and PDF files.
pub const MAX_HL_SEGMENTS: usize = 100_000;

/// The hidden-line drawing of a 3D viewport as vector paths: black lines for visible edges,
/// and gray dashes for covered edges in the dashed mode.
pub fn hidden_line_prims(
    doc: &Document,
    vp: &Viewport,
    style: &RenderStyle,
    max_segments: usize,
    out: &mut Vec<Prim>,
) {
    let size = vp.max - vp.min;
    // One scale for both sides, so a large viewport keeps its proportions.
    let k = HL_PX_PER_MM
        .min(HL_MAX_SIDE / size.max_element())
        .max(8.0 / size.min_element());
    let (w, h) = (
        (size.x * k).round().max(1.0) as u32,
        (size.y * k).round().max(1.0) as u32,
    );
    let d = viewport_document(doc, vp);
    let scene = mesh::build(&d);
    let Some(cam) = viewport_camera(vp, &scene) else {
        return;
    };
    let Some(lines) = cadektor_render::styled::vector_lines(&scene, &cam, style, w, h) else {
        return;
    };
    // Image pixels (y down) to paper mm (y up).
    let to_paper = |p: [f32; 2]| {
        DVec2::new(
            vp.min.x + p[0] as f64 / w as f64 * size.x,
            vp.max.y - p[1] as f64 / h as f64 * size.y,
        )
    };
    let weight = style.line_weight.max(0.05);
    // Very large drawings are cut at a number of segments, visible ones first, and say so.
    let total = lines.segments.len();
    let mut budget = max_segments;
    if total > max_segments {
        out.push(Prim::Text(TextPrim {
            at: vp.min + DVec2::new(1.0, 1.0),
            text: format!(
                "Warning: hidden-line drawing cut at {max_segments} of {total} lines. Use a smaller viewport or fewer layers."
            ),
            height: 2.5,
            color: [200, 0, 0, 255],
            align: Align::Left,
        }));
    }
    // Covered edges first, so visible lines are on top.
    for hidden in [true, false] {
        let share = if hidden { budget / 4 } else { budget };
        let mut used = 0;
        for s in lines.segments.iter().filter(|s| s.hidden == hidden) {
            if used >= share {
                break;
            }
            used += 1;
            let (a, b) = (to_paper(s.a), to_paper(s.b));
            if a.distance(b) < 0.02 {
                continue;
            }
            let color = if hidden {
                [120, 120, 120, 255]
            } else {
                s.color
            };
            let mut path = PathPrim::stroked(PathPrim::polyline(&[a, b]), false, color, weight);
            if hidden {
                path.width = weight * 0.7;
                path.dash = Some([1.5, 1.0]);
            }
            out.push(Prim::Path(path));
        }
        budget = budget.saturating_sub(used);
    }
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
