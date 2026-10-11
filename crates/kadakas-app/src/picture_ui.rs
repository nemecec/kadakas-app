//! Pictures of plant styles (PLT-16): the textures of small pictures, the picture row of the plant
//! style editor (Choose and Remove) and the small picture in Object Info.
//!
//! The pictures come from the thumbnail that the document keeps with the file (at most 160 px), so
//! no frame decodes a big picture. A chosen file is read with a size limit and goes through
//! `kadakas_render::picture::import_picture`, which checks the size, the format and the pixels.
//! The web build asks the browser for the file; the bytes arrive later in an inbox.

use std::collections::HashMap;

use egui::{Color32, Sense, TextureHandle, TextureOptions, Ui, Vec2};
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::model::Document;
use kadakas_core::resource::ResourceId;
use kadakas_l10n::tr;
use kadakas_render::picture::PictureLimits;

/// Most textures kept. When there are more, the cache starts again.
const CACHE_LIMIT: usize = 256;

type Cache = HashMap<String, (TextureHandle, [u32; 2])>;

fn cache_id() -> egui::Id {
    egui::Id::new("picture-ui-textures")
}

/// The texture for pixels, kept by the hash of the picture file.
pub fn texture(
    ctx: &egui::Context,
    sha256: &str,
    size: [u32; 2],
    rgba: impl FnOnce() -> Vec<u8>,
) -> TextureHandle {
    let mut cache: Cache = ctx.data_mut(|d| d.get_temp(cache_id()).unwrap_or_default());
    if let Some((t, _)) = cache.get(sha256) {
        return t.clone();
    }
    if cache.len() >= CACHE_LIMIT {
        cache.clear();
    }
    let image =
        egui::ColorImage::from_rgba_unmultiplied([size[0] as usize, size[1] as usize], &rgba());
    let t = ctx.load_texture(
        format!("picture-{}", &sha256[..sha256.len().min(12)]),
        image,
        TextureOptions::LINEAR,
    );
    cache.insert(sha256.to_string(), (t.clone(), size));
    ctx.data_mut(|d| d.insert_temp(cache_id(), cache));
    t
}

/// The texture of the picture that a plant style has, and its size in pixels.
pub fn style_texture(
    ctx: &egui::Context,
    doc: &Document,
    style: ResourceId,
) -> Option<(TextureHandle, [u32; 2])> {
    let img = doc.resources.plant_style(style)?.image?;
    let res = doc.resources.image(img)?;
    let known: Cache = ctx.data_mut(|d| d.get_temp(cache_id()).unwrap_or_default());
    if let Some((t, s)) = known.get(&res.sha256) {
        return Some((t.clone(), *s));
    }
    let pixels = kadakas_render::picture::thumb_image(res)?;
    let size = [pixels.width, pixels.height];
    Some((texture(ctx, &res.sha256, size, || pixels.rgba), size))
}

/// Draws a texture in a square of `side` points, with its proportions, on a light background.
pub fn paint_square(ui: &mut Ui, tex: &TextureHandle, size: [u32; 2], side: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, Color32::WHITE);
    let k = side / size[0].max(size[1]).max(1) as f32;
    let (w, h) = (size[0] as f32 * k, size[1] as f32 * k);
    let r = egui::Rect::from_center_size(rect.center(), Vec2::new(w, h));
    p.image(
        tex.id(),
        r,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    p.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, Color32::from_gray(200)),
        egui::StrokeKind::Inside,
    );
}

/// The small picture of a plant style, for Object Info. Draws nothing if the style has none.
pub fn show_small(ui: &mut Ui, doc: &Document, style: ResourceId, side: f32) {
    if let Some((tex, size)) = style_texture(ui.ctx(), doc, style) {
        paint_square(ui, &tex, size, side);
    }
}

/// A picture that the person chose and that still has to be put in the document.
#[derive(Debug)]
pub struct Picked {
    pub style: ResourceId,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The state of the picture row: where the web build leaves the files it got from the browser.
#[derive(Default)]
pub struct PictureUi {
    #[cfg(target_arch = "wasm32")]
    inbox: std::rc::Rc<std::cell::RefCell<Vec<Picked>>>,
}

impl PictureUi {
    /// The files that arrived since the last frame. Native builds read the file at once, so this is
    /// empty there.
    pub fn take(&self) -> Vec<Picked> {
        #[cfg(target_arch = "wasm32")]
        {
            self.inbox.borrow_mut().drain(..).collect()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Vec::new()
        }
    }

    /// Asks for a picture file for the style. Native: a file dialog, and the file comes back at
    /// once. Web: the browser file picker; the file comes later through [`PictureUi::take`].
    pub fn choose(&self, ctx: &egui::Context, style: ResourceId, limit: usize) -> Option<Picked> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = ctx;
            let path = rfd::FileDialog::new()
                .add_filter(tr!("picture-ui-filter"), &["png", "jpg", "jpeg"])
                .pick_file()?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // One byte more than the limit, so a file that is too large is noticed without reading it all.
            let bytes = read_limited(&path, limit).unwrap_or_default();
            Some(Picked { style, name, bytes })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = limit;
            let (inbox, ctx) = (self.inbox.clone(), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(f) = rfd::AsyncFileDialog::new()
                    .add_filter(tr!("picture-ui-filter"), &["png", "jpg", "jpeg"])
                    .pick_file()
                    .await
                {
                    let bytes = f.read().await;
                    inbox.borrow_mut().push(Picked {
                        style,
                        name: f.file_name(),
                        bytes,
                    });
                    ctx.request_repaint();
                }
            });
            None
        }
    }
}

/// Reads at most `limit + 1` bytes of a file.
#[cfg(not(target_arch = "wasm32"))]
fn read_limited(path: &std::path::Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Puts a chosen picture in the style. The checks of the import apply (size, format, pixels).
/// Returns the text for the status line.
pub fn apply(session: &mut Session, picked: Picked) -> String {
    let limits: PictureLimits = session.limits().picture_limits();
    let res = match kadakas_render::picture::import_picture(&picked.bytes, &picked.name, limits) {
        Ok(r) => r,
        Err(e) => return tr!("picture-ui-error", message = e),
    };
    let cmd = Command::SetPlantPicture {
        style: picked.style,
        picture: Some(Box::new(res)),
    };
    let mut status = String::new();
    match crate::panels::run_command(session, &cmd, &mut status) {
        Some(_) => tr!("picture-ui-set", name = picked.name),
        None => status,
    }
}

/// What the picture row asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum RowAction {
    None,
    Choose,
    Remove,
}

/// The picture row of the plant style editor: the picture, its facts, Choose and Remove.
pub fn picture_row(ui: &mut Ui, doc: &Document, style: ResourceId) -> RowAction {
    let mut action = RowAction::None;
    let image = doc
        .resources
        .plant_style(style)
        .and_then(|s| s.image)
        .and_then(|i| doc.resources.image(i));
    ui.strong(tr!("picture-ui-picture"));
    ui.horizontal(|ui| {
        match style_texture(ui.ctx(), doc, style) {
            Some((tex, size)) => paint_square(ui, &tex, size, 72.0),
            None => {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
                ui.painter_at(rect).rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.0, Color32::from_gray(160)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        ui.vertical(|ui| {
            match image {
                Some(i) => {
                    ui.label(tr!(
                        "picture-ui-facts",
                        format = i.format.id().to_uppercase(),
                        width = i.width,
                        height = i.height,
                        size =
                            kadakas_units::current().number_text(i.byte_len() as f64 / 1024.0, 0)
                    ));
                    if !i.source.is_empty() {
                        ui.weak(&i.source);
                    }
                }
                None => {
                    ui.weak(tr!("picture-ui-none"));
                }
            }
            ui.horizontal(|ui| {
                if ui
                    .button(tr!("picture-ui-choose"))
                    .on_hover_text(tr!("picture-ui-choose-hint"))
                    .clicked()
                {
                    action = RowAction::Choose;
                }
                if ui
                    .add_enabled(image.is_some(), egui::Button::new(tr!("picture-ui-remove")))
                    .clicked()
                {
                    action = RowAction::Remove;
                }
            });
        });
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_render::picture::test_png;
    use serde_json::json;

    fn frame(mut f: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| f(ui));
    }

    fn styled() -> (Session, ResourceId) {
        let mut s = Session::default();
        let id = s
            .handle("plant_style.create", json!({"name": "Maple"}))
            .unwrap()["id"]
            .clone();
        let id = ResourceId(kadakas_core::uid::Uid::parse(id.as_str().unwrap()).unwrap());
        (s, id)
    }

    #[test]
    fn a_picked_file_becomes_the_picture_and_errors_go_to_the_status() {
        let (mut s, id) = styled();
        let ok = apply(
            &mut s,
            Picked {
                style: id,
                name: "maple.png".into(),
                bytes: test_png(40, 20),
            },
        );
        assert!(ok.contains("maple.png"), "{ok}");
        let doc = s.editor.doc();
        assert!(doc.resources.plant_style(id).unwrap().image.is_some());
        for bytes in [Vec::new(), b"not a picture".to_vec()] {
            let msg = apply(
                &mut s,
                Picked {
                    style: id,
                    name: "x.png".into(),
                    bytes,
                },
            );
            assert!(msg.contains("picture") || msg.contains("Cannot"), "{msg}");
        }
        // Too large for the limit of the session.
        s.set_limits(kadakas_api::SessionLimits {
            picture_bytes: 10,
            ..Default::default()
        });
        let msg = apply(
            &mut s,
            Picked {
                style: id,
                name: "big.png".into(),
                bytes: test_png(40, 20),
            },
        );
        assert!(!msg.is_empty());
        // A style that does not exist.
        let msg = apply(
            &mut Session::default(),
            Picked {
                style: id,
                name: "a.png".into(),
                bytes: test_png(4, 4),
            },
        );
        assert!(!msg.is_empty());
    }

    #[test]
    fn the_row_and_the_small_picture_draw_with_and_without_a_picture() {
        let (mut s, id) = styled();
        frame(|ui| {
            assert_eq!(picture_row(ui, s.editor.doc(), id), RowAction::None);
            show_small(ui, s.editor.doc(), id, 40.0);
        });
        apply(
            &mut s,
            Picked {
                style: id,
                name: "maple.png".into(),
                bytes: test_png(40, 20),
            },
        );
        frame(|ui| {
            picture_row(ui, s.editor.doc(), id);
            show_small(ui, s.editor.doc(), id, 40.0);
        });
        let ctx = egui::Context::default();
        let (t1, size) = style_texture(&ctx, s.editor.doc(), id).unwrap();
        let (t2, _) = style_texture(&ctx, s.editor.doc(), id).unwrap();
        assert_eq!(t1.id(), t2.id(), "the texture is made once");
        assert_eq!(size, [40, 20]);
        assert!(
            style_texture(
                &ctx,
                s.editor.doc(),
                ResourceId(kadakas_core::uid::Uid::NIL)
            )
            .is_none()
        );
    }

    #[test]
    fn the_cache_starts_again_when_it_is_full() {
        let ctx = egui::Context::default();
        for i in 0..CACHE_LIMIT + 3 {
            let _ = texture(&ctx, &format!("{i:064}"), [1, 1], || vec![0, 0, 0, 255]);
        }
        let cache: Cache = ctx.data_mut(|d| d.get_temp(cache_id()).unwrap());
        assert!(cache.len() <= 4);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_file_is_read_up_to_the_limit_plus_one() {
        let dir = std::env::temp_dir().join(format!("kad-pic-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.bin");
        std::fs::write(&f, vec![1u8; 100]).unwrap();
        assert_eq!(read_limited(&f, 10).unwrap().len(), 11);
        assert_eq!(read_limited(&f, 1000).unwrap().len(), 100);
        assert!(read_limited(&dir.join("none"), 10).is_err());
        let ui = PictureUi::default();
        assert!(ui.take().is_empty());
    }
}
