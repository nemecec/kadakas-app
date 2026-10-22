//! The icons of the tool bar. Each is our own drawing from a written idea (a pencil, a box with
//! an arrow on its top face, an open hand), in `assets/icons/modeler/`, in the style of the
//! Kadakas icon set (`docs/icons.md`): 24 by 24, stroke 2, round caps, no colors. Nothing is copied
//! from another program.
//!
//! An icon is drawn with resvg to a white texture at the size the screen needs, and the widget
//! tints it with the text color, like the icons of `kadakas-app`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Context, TextureHandle, Ui, Vec2};
use kadakas_ui_core::modeler::ModelerTool;

macro_rules! icon {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/icons/modeler/",
                $name,
                ".svg"
            )) as &[u8],
        )
    };
}

/// All icons: the name and the SVG text.
pub const ICONS: &[(&str, &[u8])] = &[
    icon!("select"),
    icon!("eraser"),
    icon!("line"),
    icon!("rectangle"),
    icon!("circle"),
    icon!("polygon"),
    icon!("arc"),
    icon!("push-pull"),
    icon!("follow-me"),
    icon!("offset"),
    icon!("move"),
    icon!("rotate"),
    icon!("scale"),
    icon!("tape-measure"),
    icon!("paint-bucket"),
    icon!("orbit"),
    icon!("pan"),
    icon!("zoom"),
    icon!("zoom-extents"),
];

/// The icon of a tool. The names are the ids of the tools.
pub fn tool_icon(tool: ModelerTool) -> &'static str {
    tool.id()
}

/// The SVG source of an icon.
pub fn svg(name: &str) -> Option<&'static str> {
    ICONS
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, b)| std::str::from_utf8(b).ok())
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
        let a = u32::from(p[3]);
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some(data)
}

/// The white textures made so far, by name and pixel size. They live in the egui context and are
/// emptied when the pixel ratio changes.
#[derive(Clone, Default)]
struct Cache(Arc<Mutex<Textures>>);

type Textures = (f32, HashMap<(&'static str, u32), Option<TextureHandle>>);

/// The white texture of an icon at `size` points. `None` if the icon is missing.
pub fn texture(ctx: &Context, name: &str, size: f32) -> Option<TextureHandle> {
    let (name, _) = ICONS.iter().find(|(n, _)| *n == name)?;
    let ppp = ctx.pixels_per_point();
    let px = (size * ppp).ceil().max(1.0) as u32;
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::NULL).clone());
    let mut guard = cache.0.lock().ok()?;
    if guard.0 != ppp {
        guard.1.clear();
        guard.0 = ppp;
    }
    guard
        .1
        .entry((name, px))
        .or_insert_with(|| {
            let data = render_rgba(name, px, [255, 255, 255])?;
            let img = egui::ColorImage::from_rgba_unmultiplied([px as usize; 2], &data);
            Some(ctx.load_texture(
                format!("modeler-icon-{name}-{px}"),
                img,
                egui::TextureOptions::LINEAR,
            ))
        })
        .clone()
}

/// The icon as an image widget, tinted with the text color of `ui`.
pub fn image(ui: &Ui, name: &str, size: f32) -> Option<egui::Image<'static>> {
    let color = if ui.is_enabled() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let t = texture(ui.ctx(), name, size)?;
    Some(egui::Image::new(egui::load::SizedTexture::new(t.id(), Vec2::splat(size))).tint(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_an_icon_and_every_icon_a_tool() {
        for t in ModelerTool::ALL {
            assert!(svg(tool_icon(t)).is_some(), "{t:?} has no icon");
        }
        assert_eq!(ICONS.len(), ModelerTool::ALL.len());
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/modeler");
        let on_disk = std::fs::read_dir(dir).unwrap().count();
        assert_eq!(on_disk, ICONS.len(), "a file is not in the table");
    }

    #[test]
    fn the_icons_follow_the_style_of_the_set() {
        use resvg::usvg;
        for (name, bytes) in ICONS {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(bytes.len() < 4096, "{name} is too big");
            assert!(text.contains("viewBox=\"0 0 24 24\""), "{name}");
            assert!(text.contains("stroke=\"currentColor\""), "{name}");
            assert!(text.contains("stroke-width=\"2\""), "{name}");
            assert!(
                !text.contains("<text") && !text.contains("<image"),
                "{name}"
            );
            // The only colors are currentColor (no hex colors).
            assert!(!text.contains('#'), "{name} has a color");
            let tree = usvg::Tree::from_str(text, &usvg::Options::default())
                .unwrap_or_else(|e| panic!("{name} does not parse: {e}"));
            let b = tree.root().abs_bounding_box();
            assert!(
                b.left() >= 2.0 && b.top() >= 2.0 && b.right() <= 22.0 && b.bottom() <= 22.0,
                "{name} leaves the grid: {b:?}"
            );
        }
    }

    #[test]
    fn nothing_in_the_set_names_another_program() {
        for (name, bytes) in ICONS {
            let all = format!("{name} {}", std::str::from_utf8(bytes).unwrap()).to_lowercase();
            assert!(
                !all.contains("sketchup") && !all.contains("trimble"),
                "{name}"
            );
        }
    }

    #[test]
    fn an_icon_draws_in_the_color_and_scales() {
        for (name, _) in ICONS {
            let small = render_rgba(name, 16, [255, 0, 0]).unwrap();
            let big = render_rgba(name, 48, [255, 0, 0]).unwrap();
            assert_eq!(small.len(), 16 * 16 * 4);
            assert_eq!(big.len(), 48 * 48 * 4);
            // Not blank, and every visible pixel is red.
            let visible = |d: &[u8]| d.chunks(4).filter(|p| p[3] > 128).count();
            assert!(visible(&small) > 10, "{name} is blank at 16 px");
            assert!(visible(&big) > visible(&small), "{name}");
            assert!(
                small
                    .chunks(4)
                    .filter(|p| p[3] > 200)
                    .all(|p| p[0] > 200 && p[1] < 60)
            );
        }
        assert!(render_rgba("nope", 16, [0, 0, 0]).is_none());
    }

    /// Writes a sheet of the icons to `ICON_SHEET_DIR` (default: the temp folder) to look at.
    #[test]
    fn the_icon_sheet_is_not_blank() {
        let px = 64u32;
        let rows = (ICONS.len() as u32).div_ceil(8);
        let (w, h) = (px * 8, px * rows);
        let mut sheet = vec![255u8; (w * h * 4) as usize];
        for (i, (name, _)) in ICONS.iter().enumerate() {
            let data = render_rgba(name, px, [30, 30, 30]).unwrap();
            let (ox, oy) = ((i as u32 % 8) * px, (i as u32 / 8) * px);
            for y in 0..px {
                for x in 0..px {
                    let s = ((y * px + x) * 4) as usize;
                    let d = (((oy + y) * w + ox + x) * 4) as usize;
                    let a = u32::from(data[s + 3]);
                    for c in 0..3 {
                        let v = (u32::from(data[s + c]) * a + 255 * (255 - a)) / 255;
                        sheet[d + c] = v as u8;
                    }
                }
            }
        }
        let dark = sheet.chunks(4).filter(|p| p[0] < 128).count();
        assert!(dark > 1500, "the sheet is nearly blank: {dark}");
        let dir = std::env::var_os("ICON_SHEET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join("kadakas-modeler-icon-sheet.ppm");
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        ppm.extend(sheet.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
        let _ = std::fs::write(path, ppm);
    }
}
