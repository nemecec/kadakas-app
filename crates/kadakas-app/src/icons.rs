//! Our own icon set. The SVG files are in `assets/icons/` (see `docs/icons.md`). They are built into
//! the program, drawn with resvg to textures at the size that the screen needs (HiDPI aware), tinted
//! with the theme text color and cached. A missing icon shows the text label instead.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Context, TextureHandle, Ui, Vec2};

use crate::icon_size::{self, IconRole, IconSize};
use crate::panels::MenuAction;
use kadakas_ui_core::tools::Tool;

/// All icons: the name and the SVG text. Tabler Icons (MIT) are in `assets/icons/tabler/`, ours in
/// `assets/icons/kadakas/`.
pub const ICONS: &[(&str, &[u8])] = &[
    (
        "arc",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/arc.svg"
        )),
    ),
    (
        "array",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/array.svg"
        )),
    ),
    (
        "callout",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/callout.svg"
        )),
    ),
    (
        "chamfer",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/chamfer.svg"
        )),
    ),
    (
        "circle",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/circle.svg"
        )),
    ),
    (
        "class",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/class.svg"
        )),
    ),
    (
        "versions",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/versions.svg"
        )),
    ),
    (
        "compose",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/compose.svg"
        )),
    ),
    (
        "data-tag",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/data-tag.svg"
        )),
    ),
    (
        "design-layer",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/design-layer.svg"
        )),
    ),
    (
        "dimension",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/dimension.svg"
        )),
    ),
    (
        "dxf",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/dxf.svg"
        )),
    ),
    (
        "existing-tree",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/existing-tree.svg"
        )),
    ),
    (
        "extend",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/extend.svg"
        )),
    ),
    (
        "extrude",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/extrude.svg"
        )),
    ),
    (
        "fillet",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/fillet.svg"
        )),
    ),
    (
        "flyover",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/flyover.svg"
        )),
    ),
    (
        "grade",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/grade.svg"
        )),
    ),
    (
        "group",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/group.svg"
        )),
    ),
    (
        "hardscape",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/hardscape.svg"
        )),
    ),
    (
        "hatch",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/hatch.svg"
        )),
    ),
    (
        "ifc",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/ifc.svg"
        )),
    ),
    (
        "irrigation-line",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/irrigation-line.svg"
        )),
    ),
    (
        "irrigation-outlet",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/irrigation-outlet.svg"
        )),
    ),
    (
        "landscape-area",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/landscape-area.svg"
        )),
    ),
    (
        "landscape-wall",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/landscape-wall.svg"
        )),
    ),
    (
        "lasso",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/lasso.svg"
        )),
    ),
    (
        "line",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/line.svg"
        )),
    ),
    (
        "massing",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/massing.svg"
        )),
    ),
    (
        "mirror",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/mirror.svg"
        )),
    ),
    (
        "north-arrow",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/north-arrow.svg"
        )),
    ),
    (
        "offset",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/offset.svg"
        )),
    ),
    (
        "plant",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/plant.svg"
        )),
    ),
    (
        "polygon",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/polygon.svg"
        )),
    ),
    (
        "polyline",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/polyline.svg"
        )),
    ),
    (
        "record",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/record.svg"
        )),
    ),
    (
        "rectangle",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/rectangle.svg"
        )),
    ),
    (
        "render-dashed",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/render-dashed.svg"
        )),
    ),
    (
        "render-hidden-line",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/render-hidden-line.svg"
        )),
    ),
    (
        "render-shaded",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/render-shaded.svg"
        )),
    ),
    (
        "render-wireframe",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/render-wireframe.svg"
        )),
    ),
    (
        "revision-cloud",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/revision-cloud.svg"
        )),
    ),
    (
        "rotate",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/rotate.svg"
        )),
    ),
    (
        "scale",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/scale.svg"
        )),
    ),
    (
        "scale-bar",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/scale-bar.svg"
        )),
    ),
    (
        "section-line",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/section-line.svg"
        )),
    ),
    (
        "select",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/select.svg"
        )),
    ),
    (
        "sheet-layer",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/sheet-layer.svg"
        )),
    ),
    (
        "site-model",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/site-model.svg"
        )),
    ),
    (
        "site-modifier",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/site-modifier.svg"
        )),
    ),
    (
        "smart-cursor",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/smart-cursor.svg"
        )),
    ),
    (
        "polygon3d",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/polygon3d.svg"
        )),
    ),
    (
        "split",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/split.svg"
        )),
    ),
    (
        "spot-elevation",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/spot-elevation.svg"
        )),
    ),
    (
        "surface-add",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/surface-add.svg"
        )),
    ),
    (
        "surface-clip",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/surface-clip.svg"
        )),
    ),
    (
        "surface-intersect",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/surface-intersect.svg"
        )),
    ),
    (
        "surface-subtract",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/surface-subtract.svg"
        )),
    ),
    (
        "sweep",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/sweep.svg"
        )),
    ),
    (
        "symbol",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/symbol.svg"
        )),
    ),
    (
        "text",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/text.svg"
        )),
    ),
    (
        "top-plan",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/top-plan.svg"
        )),
    ),
    (
        "trim",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/trim.svg"
        )),
    ),
    (
        "view-3d",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/view-3d.svg"
        )),
    ),
    (
        "viewport",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/viewport.svg"
        )),
    ),
    (
        "worksheet",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/kadakas/worksheet.svg"
        )),
    ),
    (
        "arrow-back-up",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/arrow-back-up.svg"
        )),
    ),
    (
        "arrow-forward-up",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/arrow-forward-up.svg"
        )),
    ),
    (
        "arrow-narrow-left",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/arrow-narrow-left.svg"
        )),
    ),
    (
        "arrow-narrow-right",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/arrow-narrow-right.svg"
        )),
    ),
    (
        "code",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/code.svg"
        )),
    ),
    (
        "device-floppy",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/device-floppy.svg"
        )),
    ),
    (
        "file-export",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/file-export.svg"
        )),
    ),
    (
        "file-import",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/file-import.svg"
        )),
    ),
    (
        "file-plus",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/file-plus.svg"
        )),
    ),
    (
        "file-type-pdf",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/file-type-pdf.svg"
        )),
    ),
    (
        "file-type-svg",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/file-type-svg.svg"
        )),
    ),
    (
        "folder-open",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/folder-open.svg"
        )),
    ),
    (
        "grid-dots",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/grid-dots.svg"
        )),
    ),
    (
        "magnet",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/magnet.svg"
        )),
    ),
    (
        "map",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/map.svg"
        )),
    ),
    (
        "plug",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/plug.svg"
        )),
    ),
    (
        "search",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/search.svg"
        )),
    ),
    (
        "select-all",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/select-all.svg"
        )),
    ),
    (
        "settings",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/settings.svg"
        )),
    ),
    (
        "terminal-2",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/terminal-2.svg"
        )),
    ),
    (
        "trash",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/trash.svg"
        )),
    ),
    (
        "world-latitude",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/world-latitude.svg"
        )),
    ),
    (
        "world-www",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/world-www.svg"
        )),
    ),
    (
        "zoom-scan",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/tabler/zoom-scan.svg"
        )),
    ),
];

/// The SVG source of an icon.
pub fn svg(name: &str) -> Option<&'static str> {
    ICONS
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, b)| std::str::from_utf8(b).ok())
}

/// True if the icon exists (for the tests).
#[cfg(test)]
pub fn has(name: &str) -> bool {
    svg(name).is_some()
}

/// The icon of a tool.
pub fn tool_icon(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "select",
        Tool::Line => "line",
        Tool::Rect => "rectangle",
        Tool::Circle => "circle",
        Tool::Polyline => "polyline",
        Tool::Text => "text",
        Tool::Plant => "plant",
        Tool::LandscapeArea => "landscape-area",
        Tool::Hardscape => "hardscape",
        Tool::Massing => "massing",
        Tool::Polygon3d => "polygon3d",
        Tool::SiteModifier => "site-modifier",
        Tool::Irrigation => "irrigation-outlet",
        Tool::Grading => "grade",
        Tool::Dimension => "dimension",
        Tool::Callout => "callout",
        Tool::DataTag => "data-tag",
        Tool::NorthArrow => "north-arrow",
        Tool::ScaleBar => "scale-bar",
        Tool::RevisionCloud => "revision-cloud",
        Tool::Rotate => "rotate",
        Tool::Mirror => "mirror",
        Tool::Scale => "scale",
        Tool::Offset => "offset",
        Tool::Fillet => "fillet",
        Tool::Chamfer => "chamfer",
        Tool::Trim => "trim",
        Tool::Extend => "extend",
        Tool::Split => "split",
    }
}

/// The icon of a menu item.
pub fn menu_icon(a: MenuAction) -> &'static str {
    match a {
        MenuAction::New => "file-plus",
        MenuAction::Open => "folder-open",
        MenuAction::Save | MenuAction::SaveAs => "device-floppy",
        MenuAction::ExportSvg => "file-type-svg",
        MenuAction::ExportPdf => "file-type-pdf",
        MenuAction::Import => "file-import",
        MenuAction::Export => "file-export",
        MenuAction::MapSources => "map",
        MenuAction::ChangeCrs => "world-latitude",
        MenuAction::Scripts => "code",
        MenuAction::PlugIns => "plug",
        MenuAction::Versions => "versions",
        MenuAction::Undo => "arrow-back-up",
        MenuAction::Redo => "arrow-forward-up",
        MenuAction::Delete => "trash",
        MenuAction::SelectAll => "select-all",
        MenuAction::ZoomFit => "zoom-scan",
        MenuAction::CreateSiteModel | MenuAction::UpdateSiteModel => "site-model",
        MenuAction::CreateTextureBed => "site-modifier",
        MenuAction::Preferences => "settings",
        MenuAction::DuplicateArray => "array",
        MenuAction::AddSurface => "surface-add",
        MenuAction::SubtractSurface => "surface-subtract",
        MenuAction::ClipSurface => "surface-clip",
        MenuAction::IntersectSurface => "surface-intersect",
        MenuAction::Compose => "compose",
        MenuAction::Extrude => "extrude",
        MenuAction::Sweep => "sweep",
        MenuAction::RotateLeft90 | MenuAction::RotateRight90 => "rotate",
        MenuAction::FlipHorizontal | MenuAction::FlipVertical => "mirror",
        MenuAction::PreviousView => "arrow-narrow-left",
        MenuAction::NextView => "arrow-narrow-right",
        MenuAction::QuickSearch => "search",
        MenuAction::CollabConnect | MenuAction::CollabDisconnect => "plug",
        MenuAction::Tool(t) => tool_icon(t),
    }
}

/// The icon of a standard view: Top/Plan is the square seen from above, the others the 3D cube.
pub fn view_icon(v: kadakas_render::StandardView) -> &'static str {
    if v.is_plan() { "top-plan" } else { "view-3d" }
}

/// The icon of a render mode.
pub fn render_icon(m: kadakas_render::RenderMode) -> &'static str {
    use kadakas_render::RenderMode::*;
    match m {
        Wireframe => "render-wireframe",
        Shaded => "render-shaded",
        HiddenLine => "render-hidden-line",
        DashedHiddenLine => "render-dashed",
    }
}

/// The icon of a resource type (the names that `library::list` uses). Empty means all types.
pub fn resource_icon(ty: &str) -> &'static str {
    match ty {
        "plant_style" => "plant",
        "hardscape_style" => "hardscape",
        "landscape_area_style" => "landscape-area",
        "record_format" => "record",
        "data_map" => "data-tag",
        "worksheet" => "worksheet",
        "symbol" => "symbol",
        "hatch" => "hatch",
        "tile" => "hardscape",
        "line_type" => "line",
        "dimension_style" => "dimension",
        "text_style" => "text",
        "tag_style" => "data-tag",
        "notes_database" => "callout",
        "class" => "class",
        "design_layer" => "design-layer",
        "sheet_layer" => "sheet-layer",
        "viewport" => "viewport",
        _ => "design-layer",
    }
}

/// Draws an icon to RGBA pixels (straight alpha) at `px` by `px` in `color`.
pub fn render_rgba(name: &str, px: u32, color: [u8; 3]) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let src = svg(name)?;
    let hex = format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]);
    let src = src.replace("currentColor", &hex);
    let tree = usvg::Tree::from_str(&src, &usvg::Options::default()).ok()?;
    let mut pm = tiny_skia::Pixmap::new(px.max(1), px.max(1))?;
    let k = px.max(1) as f32 / 24.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(k, k),
        &mut pm.as_mut(),
    );
    let mut data = pm.take();
    // Pixmap pixels are premultiplied. Make them straight.
    for p in data.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some(data)
}

type Key = (&'static str, u32);

/// The textures made so far. They are white: the widget tints them with the text color, so the
/// theme, hover and selection need no new texture. It lives in the egui context and is emptied
/// when the pixel ratio changes.
#[derive(Clone, Default)]
struct Cache(Arc<Mutex<Textures>>);

/// The pixel ratio and the icon size of the textures, and the textures.
type Textures = ((f32, IconSize), HashMap<Key, Option<TextureHandle>>);

/// The white texture of an icon at `size` points. `None` if the icon is missing.
pub fn texture(ctx: &Context, name: &str, size: f32) -> Option<TextureHandle> {
    let (name, _) = ICONS.iter().find(|(n, _)| *n == name)?;
    let ppp = ctx.pixels_per_point();
    let px = (size * ppp).ceil().max(1.0) as u32;
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::NULL).clone());
    let mut guard = cache.0.lock().ok()?;
    let state = (ppp, icon_size::current(ctx));
    if guard.0 != state {
        // The scale or the icon size changed: free the old textures.
        guard.1.clear();
        guard.0 = state;
    }
    guard
        .1
        .entry((name, px))
        .or_insert_with(|| {
            let data = render_rgba(name, px, [255, 255, 255])?;
            let img = egui::ColorImage::from_rgba_unmultiplied([px as usize; 2], &data);
            Some(ctx.load_texture(
                format!("icon-{name}-{px}"),
                img,
                egui::TextureOptions::LINEAR,
            ))
        })
        .clone()
}

/// The cache keys (name and pixel size) of the icon textures made so far. For tests.
#[cfg(test)]
pub fn cached_keys(ctx: &Context) -> Vec<Key> {
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::NULL).clone());
    let guard = cache.0.lock().unwrap();
    let mut keys: Vec<Key> = guard.1.keys().copied().collect();
    keys.sort();
    keys
}

/// The icon as an image widget, tinted with the text color of `ui`. `None` if the icon is missing.
pub fn image(ui: &Ui, name: &str, size: f32) -> Option<egui::Image<'static>> {
    let color = if ui.is_enabled() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let t = texture(ui.ctx(), name, size)?;
    Some(egui::Image::new(egui::load::SizedTexture::new(t.id(), Vec2::splat(size))).tint(color))
}

/// A button with an icon and a label, with the icon size of menus and lists (16 points).
/// Without the icon, it shows the label only.
pub fn button(ui: &Ui, name: &str, label: &str) -> egui::Button<'static> {
    button_for(ui, IconRole::Menu, name, label)
}

/// A button with an icon and a label. The icon size comes from the user's choice and `role`.
pub fn button_for(ui: &Ui, role: IconRole, name: &str, label: &str) -> egui::Button<'static> {
    let size = icon_size::points(ui.ctx(), role);
    match image(ui, name, size) {
        Some(img) => {
            egui::Button::image_and_text(img, label.to_string()).image_tint_follows_text_color(true)
        }
        None => egui::Button::new(label.to_string()),
    }
}

/// A button for a list: the icon and the label, marked when `selected`.
pub fn select_button(ui: &mut Ui, name: &str, label: &str, selected: bool) -> egui::Response {
    select_button_for(ui, IconRole::Menu, name, label, selected)
}

/// A button for a palette or a bar, sized by `role`, marked when `selected`.
pub fn select_button_for(
    ui: &mut Ui,
    role: IconRole,
    name: &str,
    label: &str,
    selected: bool,
) -> egui::Response {
    ui.add(button_for(ui, role, name, label).selected(selected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Action;
    use resvg::usvg;
    use std::path::PathBuf;

    fn asset_dir(sub: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/icons")
            .join(sub)
    }

    fn files(sub: &str) -> Vec<(String, String)> {
        let mut v: Vec<_> = std::fs::read_dir(asset_dir(sub))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "svg"))
            .map(|e| {
                (
                    e.path().file_stem().unwrap().to_string_lossy().to_string(),
                    std::fs::read_to_string(e.path()).unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn every_file_is_in_the_table_and_the_other_way() {
        let mut on_disk: Vec<String> = files("kadakas")
            .into_iter()
            .chain(files("tabler"))
            .map(|(n, _)| n)
            .collect();
        on_disk.sort();
        let mut in_table: Vec<String> = ICONS.iter().map(|(n, _)| n.to_string()).collect();
        in_table.sort();
        assert_eq!(on_disk, in_table);
        let mut dedup = in_table.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), in_table.len(), "duplicate icon names");
    }

    #[test]
    fn every_svg_parses_with_usvg() {
        for (name, bytes) in ICONS {
            let text = std::str::from_utf8(bytes).unwrap();
            usvg::Tree::from_str(text, &usvg::Options::default())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn our_icons_follow_the_style() {
        let list = files("kadakas");
        let mut outside: Vec<String> = Vec::new();
        assert!(list.len() >= 60);
        for (name, s) in list {
            assert!(!name.to_lowercase().contains("vectorworks"), "{name}");
            assert!(s.contains("viewBox=\"0 0 24 24\""), "{name}: viewBox");
            assert!(s.contains("stroke=\"currentColor\""), "{name}: stroke");
            assert!(s.contains("stroke-width=\"2\""), "{name}: stroke width");
            assert!(s.contains("stroke-linecap=\"round\""), "{name}: caps");
            assert!(s.contains("stroke-linejoin=\"round\""), "{name}: joins");
            assert!(
                !s.contains("<image") && !s.contains("data:"),
                "{name}: raster"
            );
            assert!(s.len() < 4096, "{name}: {} bytes", s.len());
            // Colors only through currentColor.
            assert!(!s.contains('#'), "{name}: fixed color");
            // Light areas are accents. They must be clear at 16 px.
            for part in s.split("fill-opacity=\"").skip(1) {
                let v: f32 = part.split('"').next().unwrap().parse().unwrap();
                assert!(v <= 0.8, "{name}: fill-opacity {v}");
                if name.starts_with("surface-") {
                    assert!(
                        v >= 0.5,
                        "{name}: the result area needs fill-opacity >= 0.5"
                    );
                }
            }
            // The drawing stays inside the 3 to 21 range (the stroke needs room at 16 px).
            let tree = usvg::Tree::from_str(&s, &usvg::Options::default()).unwrap();
            let b = tree.root().abs_bounding_box();
            if !(b.left() >= 2.9 && b.top() >= 2.9 && b.right() <= 21.1 && b.bottom() <= 21.1) {
                outside.push(format!("{name}: outside 3..21: {b:?}"));
            }
        }
        assert!(outside.is_empty(), "{}", outside.join("\n"));
    }

    #[test]
    fn tabler_icons_are_small_and_in_the_grid() {
        for (name, s) in files("tabler") {
            assert!(s.contains("viewBox=\"0 0 24 24\""), "{name}");
            assert!(s.contains("currentColor"), "{name}");
            assert!(s.len() < 4096, "{name}");
        }
        assert!(asset_dir("tabler").join("LICENSE").exists());
    }

    #[test]
    fn every_tool_has_an_icon() {
        use Tool::*;
        for t in Tool::BASIC
            .iter()
            .chain(&Tool::LANDSCAPE)
            .chain(&Tool::SITE)
            .chain(&Tool::ANNOTATION)
            .chain(&Tool::IRRIGATION)
            .chain(&Tool::MODIFY)
            .chain(&[Select, Grading])
        {
            assert!(has(tool_icon(*t)), "tool {t:?} has no icon");
        }
        // Every tool that a key selects.
        for a in Action::ALL {
            if let Some(t) = a.tool() {
                assert!(has(tool_icon(t)), "action {a:?}: tool {t:?} has no icon");
            }
        }
    }

    #[test]
    fn menus_views_and_resources_have_icons() {
        use MenuAction::*;
        for a in [
            New,
            Open,
            Save,
            SaveAs,
            ExportSvg,
            ExportPdf,
            Import,
            Export,
            MapSources,
            ChangeCrs,
            Scripts,
            PlugIns,
            Versions,
            Undo,
            Redo,
            Delete,
            SelectAll,
            ZoomFit,
            CreateSiteModel,
            UpdateSiteModel,
            CreateTextureBed,
            Preferences,
            DuplicateArray,
            AddSurface,
            SubtractSurface,
            ClipSurface,
            IntersectSurface,
            Compose,
            Extrude,
            Sweep,
            RotateLeft90,
            RotateRight90,
            FlipHorizontal,
            FlipVertical,
            PreviousView,
            NextView,
            QuickSearch,
        ] {
            assert!(has(menu_icon(a)), "{a:?}");
        }
        for v in kadakas_render::StandardView::ALL {
            assert!(has(view_icon(v)));
        }
        for ty in [
            "",
            "plant_style",
            "hardscape_style",
            "landscape_area_style",
            "record_format",
            "data_map",
            "worksheet",
            "symbol",
            "hatch",
            "tile",
            "line_type",
        ] {
            assert!(has(resource_icon(ty)), "{ty}");
        }
    }

    #[test]
    fn missing_icon_gives_none() {
        assert!(!has("no-such-icon"));
        assert!(render_rgba("no-such-icon", 24, [0; 3]).is_none());
    }

    #[test]
    fn rendering_uses_the_color_and_scales() {
        let a = render_rgba("line", 48, [255, 0, 0]).unwrap();
        assert_eq!(a.len(), 48 * 48 * 4);
        let hit = a
            .as_chunks::<4>()
            .0
            .iter()
            .find(|p| p[3] > 200)
            .expect("pixels");
        assert_eq!(&hit[..3], &[255, 0, 0]);
        assert!(render_rgba("line", 0, [0; 3]).is_some());
    }

    /// Draws all icons at `px` pixels, enlarged `zoom` times (nearest pixel), on a white sheet.
    fn sheet(px: u32, zoom: u32) -> (u32, u32, Vec<u8>) {
        let icon = px * zoom;
        let cell = icon + 16;
        let cols = 12u32;
        let rows = (ICONS.len() as u32).div_ceil(cols);
        let (w, h) = (cols * cell, rows * cell);
        let mut sheet = vec![255u8; (w * h * 4) as usize];
        for (i, (name, _)) in ICONS.iter().enumerate() {
            let src = render_rgba(name, px, [20, 20, 20]).unwrap();
            assert!(
                src.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "icon {name} renders blank"
            );
            let (cx, cy) = ((i as u32 % cols) * cell + 8, (i as u32 / cols) * cell + 8);
            for y in 0..icon {
                for x in 0..icon {
                    let s = (((y / zoom) * px + x / zoom) * 4) as usize;
                    let d = (((cy + y) * w + cx + x) * 4) as usize;
                    let a = src[s + 3] as u32;
                    for c in 0..3 {
                        sheet[d + c] = ((src[s + c] as u32 * a + 255 * (255 - a)) / 255) as u8;
                    }
                }
            }
        }
        (w, h, sheet)
    }

    /// Draws all icons on sheets (16 px, 24 px and 48 px) and checks that they are not blank. The
    /// PNG files go to the folder in `ICON_SHEET_DIR` or the temp folder. They are not committed.
    #[test]
    fn golden_icon_sheet_is_not_blank() {
        let dir = std::env::var_os("ICON_SHEET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (px, zoom) in [(16, 4), (24, 3), (48, 1)] {
            let (w, h, data) = sheet(px, zoom);
            let dark = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] < 128)
                .count();
            assert!(
                dark > 3000,
                "the {px} px icon sheet is blank ({dark} dark pixels)"
            );
            let mut pm = resvg::tiny_skia::Pixmap::new(w, h).unwrap();
            pm.data_mut().copy_from_slice(&data);
            pm.save_png(dir.join(format!("kadakas-icon-sheet-{px}.png")))
                .unwrap();
        }
    }
}
