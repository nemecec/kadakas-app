//! Images of plant styles (PLT-16): the textures of small pictures, the image row of the plant
//! style editor (Choose and Remove) and the small image in Object Info.
//!
//! What is drawn is a thumbnail that this program derives from the file itself, once for each
//! hash (`kadakas_render::picture::derive_thumb`). The thumbnail that a document stores comes from
//! the sender, and a peer could store a different picture there. On a desktop the derivation runs
//! on a background thread (at most two at a time), and the image appears when it is ready; the
//! browser build derives it at once. A picture that this program imported itself is known, so it
//! shows at once. A failed derivation is kept too, so it is not tried in every frame.
//!
//! A chosen file is read with a size limit and imported on a background thread
//! (`kadakas_render::picture::import_picture` checks the size, the format and the pixels). The web
//! build asks the browser for the file.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use egui::{Color32, Sense, TextureHandle, TextureOptions, Ui, Vec2};
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::model::Document;
use kadakas_core::picture::ImageResource;
use kadakas_core::resource::ResourceId;
use kadakas_l10n::tr;
use kadakas_render::picture::PictureLimits;
use kadakas_render::texture::TextureImage;
use kadakas_render::thumbnail::ThumbPicture;

/// Most textures kept. When there are more, the cache starts again.
const CACHE_LIMIT: usize = 256;
/// Most derivations that run at the same time.
const MAX_RUNNING: usize = 2;

type Ready = Option<(TextureHandle, [u32; 2])>;

/// The textures that are ready, by the hash of the picture file. `None`: the derivation failed.
#[derive(Default)]
struct Shared {
    ready: HashMap<String, Ready>,
    pending: HashSet<String>,
}

type SharedRef = Arc<Mutex<Shared>>;

fn shared(ctx: &egui::Context) -> SharedRef {
    let id = egui::Id::new("picture-ui-textures");
    ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(id, SharedRef::default)
            .clone()
    })
}

fn lock(s: &SharedRef) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

fn make_texture(ctx: &egui::Context, sha: &str, img: &TextureImage) -> (TextureHandle, [u32; 2]) {
    let size = [img.width, img.height];
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [img.width as usize, img.height as usize],
        &img.rgba,
    );
    let name = format!("picture-{}", &sha[..sha.len().min(12)]);
    (ctx.load_texture(name, image, TextureOptions::LINEAR), size)
}

fn store(shared: &SharedRef, sha: &str, value: Ready) {
    let mut s = lock(shared);
    if s.ready.len() >= CACHE_LIMIT {
        s.ready.clear();
    }
    s.pending.remove(sha);
    s.ready.insert(sha.to_string(), value);
}

/// The texture for a picture that is ready, without starting any work.
pub fn cached(ctx: &egui::Context, sha256: &str) -> Ready {
    lock(&shared(ctx)).ready.get(sha256).cloned().flatten()
}

/// Makes a picture known that this program imported itself, so it shows at once.
pub fn seed(ctx: &egui::Context, res: &ImageResource) {
    if let Some(img) = kadakas_render::picture::thumb_image(res) {
        let t = make_texture(ctx, &res.sha256, &img);
        store(&shared(ctx), &res.sha256, Some(t));
    }
}

/// The texture of a picture, derived from the file. Returns `None` while the derivation runs (the
/// window is painted again when it is ready) and when the file cannot be read.
pub fn resolve(ctx: &egui::Context, res: &ImageResource) -> Ready {
    let sh = shared(ctx);
    {
        let mut s = lock(&sh);
        if let Some(r) = s.ready.get(&res.sha256) {
            return r.clone();
        }
        if s.pending.contains(&res.sha256) || s.pending.len() >= MAX_RUNNING {
            return None;
        }
        s.pending.insert(res.sha256.clone());
    }
    let (ctx2, res2, sh2) = (ctx.clone(), res.clone(), sh.clone());
    let work = move || {
        let made = kadakas_render::picture::derive_thumb(&res2, PictureLimits::default())
            .map(|img| make_texture(&ctx2, &res2.sha256, &img));
        store(&sh2, &res2.sha256, made);
        ctx2.request_repaint();
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(work);
        None
    }
    #[cfg(target_arch = "wasm32")]
    {
        work();
        cached(ctx, &res.sha256)
    }
}

/// The texture of the picture that a plant style has, and its size in pixels.
pub fn style_texture(ctx: &egui::Context, doc: &Document, style: ResourceId) -> Ready {
    let img = doc.resources.plant_style(style)?.image?;
    resolve(ctx, doc.resources.image(img)?)
}

/// The small picture for `kadakas_render::thumbnail::thumbnail_with`: only a picture that is
/// ready. The pixels stay in the texture, so `rgba` is empty.
pub fn thumb_for(ctx: &egui::Context, res: &ImageResource) -> Option<ThumbPicture> {
    let (_, size) = resolve(ctx, res)?;
    Some(ThumbPicture {
        sha256: res.sha256.clone(),
        width: size[0],
        height: size[1],
        rgba: Arc::new(Vec::new()),
        png_base64: String::new(),
    })
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

/// The small image of a plant style, for Object Info. Draws nothing if the style has none.
pub fn show_small(ui: &mut Ui, doc: &Document, style: ResourceId, side: f32) {
    if let Some((tex, size)) = style_texture(ui.ctx(), doc, style) {
        paint_square(ui, &tex, size, side);
    }
}

/// A picture that was chosen and imported (or refused) off the window thread.
#[derive(Debug)]
pub struct Imported {
    pub style: ResourceId,
    pub name: String,
    pub result: Result<ImageResource, String>,
}

/// Imports the bytes of a file. The same steps as a chosen file takes on its thread.
pub fn import_bytes(
    style: ResourceId,
    name: &str,
    bytes: &[u8],
    limits: PictureLimits,
) -> Imported {
    Imported {
        style,
        name: name.to_string(),
        result: kadakas_render::picture::import_picture(bytes, name, limits)
            .map_err(|e| tr!("picture-ui-error", message = e)),
    }
}

/// The state of the picture row: where the imported files arrive from their threads.
#[derive(Default)]
pub struct PictureUi {
    inbox: Arc<Mutex<Vec<Imported>>>,
}

impl PictureUi {
    /// The files that arrived since the last frame.
    pub fn take(&self) -> Vec<Imported> {
        std::mem::take(&mut *self.inbox.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Asks for an image file for the style. Native: a file dialog, then a thread reads and imports
    /// the file. Web: the browser file picker; the size of the file is checked before it is read.
    /// The result comes through [`PictureUi::take`].
    pub fn choose(&self, ctx: &egui::Context, style: ResourceId, limits: PictureLimits) {
        let limits = limits.capped();
        let (inbox, ctx) = (self.inbox.clone(), ctx.clone());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(path) = rfd::FileDialog::new()
                .add_filter(tr!("picture-ui-filter"), &["png", "jpg", "jpeg"])
                .pick_file()
            else {
                return;
            };
            std::thread::spawn(move || {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let got = match read_limited(&path, limits.max_bytes) {
                    Ok(bytes) => import_bytes(style, &name, &bytes, limits),
                    Err(e) => Imported {
                        style,
                        name: name.clone(),
                        result: Err(tr!("picture-ui-io-error", name = name, error = e)),
                    },
                };
                inbox.lock().unwrap_or_else(|e| e.into_inner()).push(got);
                ctx.request_repaint();
            });
        }
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            let Some(f) = rfd::AsyncFileDialog::new()
                .add_filter(tr!("picture-ui-filter"), &["png", "jpg", "jpeg"])
                .pick_file()
                .await
            else {
                return;
            };
            let name = f.file_name();
            // The browser knows the size before the file is read into memory.
            let size = f.inner().size();
            let got = if size > limits.max_bytes as f64 {
                Imported {
                    style,
                    name,
                    result: Err(tr!(
                        "picture-ui-error",
                        message = tr!(
                            "picture-ui-too-large",
                            limit = kadakas_units::int(limits.max_bytes as i64)
                        )
                    )),
                }
            } else {
                import_bytes(style, &name, &f.read().await, limits)
            };
            inbox.lock().unwrap_or_else(|e| e.into_inner()).push(got);
            ctx.request_repaint();
        });
    }
}

/// Reads at most `limit + 1` bytes of a file. An error is an I/O error, not a bad picture.
#[cfg(not(target_arch = "wasm32"))]
fn read_limited(path: &std::path::Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Puts an imported image in the style and returns the text for the status line.
pub fn apply(ctx: &egui::Context, session: &mut Session, got: Imported) -> String {
    let res = match got.result {
        Ok(r) => r,
        Err(e) => return e,
    };
    let cmd = Command::SetPlantPicture {
        style: got.style,
        picture: Some(Box::new(res.clone())),
    };
    let mut status = String::new();
    match crate::panels::run_command(session, &cmd, &mut status) {
        Some(_) => {
            seed(ctx, &res);
            tr!("picture-ui-set", name = got.name)
        }
        None => status,
    }
}

/// What the image row asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum RowAction {
    None,
    Choose,
    Remove,
}

/// The image row of the plant style editor: the image, its facts, Choose and Remove.
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

    fn imported(style: ResourceId, w: u32, h: u32) -> Imported {
        import_bytes(
            style,
            "maple.png",
            &test_png(w, h),
            PictureLimits::default(),
        )
    }

    /// Waits until a derivation has finished (a background thread on a desktop). Gives the
    /// result that was stored, `None` when the derivation failed. The bound is only a guard
    /// against a derivation that never reports.
    fn wait_for(ctx: &egui::Context, res: &ImageResource) -> Ready {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            // Start the work, if nobody has. The answer is read from the store below, because
            // `resolve` can answer `None` just before the thread stores a finished picture.
            let _ = resolve(ctx, res);
            if let Some(r) = lock(&shared(ctx)).ready.get(&res.sha256) {
                return r.clone();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the derivation did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn an_imported_file_becomes_the_image_and_errors_go_to_the_status() {
        let (mut s, id) = styled();
        let ctx = egui::Context::default();
        let ok = apply(&ctx, &mut s, imported(id, 40, 20));
        assert!(ok.contains("maple.png"), "{ok}");
        assert!(
            s.editor
                .doc()
                .resources
                .plant_style(id)
                .unwrap()
                .image
                .is_some()
        );
        for bytes in [&b""[..], b"not a picture"] {
            let bad = import_bytes(id, "x.png", bytes, PictureLimits::default());
            assert!(bad.result.is_err());
            assert!(!apply(&ctx, &mut s, bad).is_empty());
        }
        // Too large for the limit of the session.
        let small = PictureLimits {
            max_bytes: 10,
            ..Default::default()
        };
        assert!(
            import_bytes(id, "big.png", &test_png(40, 20), small)
                .result
                .is_err()
        );
        // A style that does not exist.
        let msg = apply(&ctx, &mut Session::default(), imported(id, 4, 4));
        assert!(!msg.is_empty() && !msg.contains("maple.png"));
    }

    #[test]
    fn the_row_and_the_small_image_draw_with_and_without_an_image() {
        let (mut s, id) = styled();
        frame(|ui| {
            assert_eq!(picture_row(ui, s.editor.doc(), id), RowAction::None);
            show_small(ui, s.editor.doc(), id, 40.0);
        });
        let ctx = egui::Context::default();
        apply(&ctx, &mut s, imported(id, 40, 20));
        // An image that was imported here is known at once.
        let (t1, size) = style_texture(&ctx, s.editor.doc(), id).unwrap();
        assert_eq!(size, [40, 20]);
        let (t2, _) = style_texture(&ctx, s.editor.doc(), id).unwrap();
        assert_eq!(t1.id(), t2.id(), "the texture is made once");
        frame(|ui| {
            picture_row(ui, s.editor.doc(), id);
            show_small(ui, s.editor.doc(), id, 40.0);
        });
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
    fn a_picture_from_outside_gets_its_own_thumbnail_derived_from_the_file() {
        let (mut s, id) = styled();
        let mine = egui::Context::default();
        apply(&mine, &mut s, imported(id, 40, 20));
        let img = s
            .editor
            .doc()
            .resources
            .plant_style(id)
            .unwrap()
            .image
            .unwrap();
        let mut res = s.editor.doc().resources.image(img).unwrap().clone();
        // The sender stored the thumbnail of another picture.
        let other = import_bytes(id, "o.png", &test_png(16, 16), PictureLimits::default())
            .result
            .unwrap();
        res.thumb = other.thumb.clone();
        // A context that does not know this picture derives the thumbnail from the file.
        let ctx = egui::Context::default();
        let (_, size) = wait_for(&ctx, &res).expect("derived");
        assert_eq!(
            size,
            [40, 20],
            "the size of the file, not of the sent thumbnail"
        );
        // The result is kept: cached() answers, and a second call starts no work.
        assert!(cached(&ctx, &res.sha256).is_some());
        assert!(thumb_for(&ctx, &res).is_some());
        // A picture that cannot be derived is remembered as failed.
        let mut broken = res.clone();
        broken.sha256 = "1".repeat(64);
        broken.data = "AAAA".into();
        assert!(wait_for(&ctx, &broken).is_none());
        assert!(resolve(&ctx, &broken).is_none());
        assert!(lock(&shared(&ctx)).ready.contains_key(&broken.sha256));
    }

    #[test]
    fn the_cache_starts_again_when_it_is_full() {
        let sh = SharedRef::default();
        for i in 0..CACHE_LIMIT + 3 {
            store(&sh, &format!("{i:064}"), None);
        }
        assert!(lock(&sh).ready.len() <= 4);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_file_is_read_up_to_the_limit_plus_one_and_io_errors_are_not_picture_errors() {
        let dir = std::env::temp_dir().join(format!("kad-pic-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.bin");
        std::fs::write(&f, vec![1u8; 100]).unwrap();
        assert_eq!(read_limited(&f, 10).unwrap().len(), 11);
        assert_eq!(read_limited(&f, 1000).unwrap().len(), 100);
        assert!(read_limited(&dir.join("none"), 10).is_err());
        let ui = PictureUi::default();
        assert!(ui.take().is_empty());
        ui.inbox
            .lock()
            .unwrap()
            .push(imported(ResourceId(kadakas_core::uid::Uid::NIL), 4, 4));
        assert_eq!(ui.take().len(), 1);
        assert!(ui.take().is_empty());
    }
}
