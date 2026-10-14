//! The pictures of the compare: the SVG of the plan, the PNG of the 3D model, and sheets side by
//! side. They draw the new side of the comparison (`compare.to`) with the tints of the overlay,
//! or, side by side, both sides with the same framing.

use std::collections::HashSet;

use glam::{DVec2, DVec3};
use kadakas_core::mesh::{self, Bounds3, Scene};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::svg_overlay::{SvgGhost, SvgLook, SvgOverlay};
use kadakas_render::look::Look;
use kadakas_render::shade::{self, Light, ShadeOptions};
use kadakas_render::{BACKGROUND, Camera, Image, ViewSettings};
use kadakas_review::overlay::{Status, SvgCompare, tint_scene};

use super::*;

/// The gap between two pictures side by side, in pixels.
const GAP_PX: u32 = 4;
/// The gap between two sheets side by side, in mm.
const GAP_MM: f64 = 10.0;
/// The background of a picture for the dark theme (the paper of the app).
const DARK_BACKGROUND: [u8; 4] = [30, 32, 36, 255];

/// An overlay that only fixes the part of the model to show, so that two pictures line up.
struct ViewOnly((DVec2, DVec2));

impl SvgOverlay for ViewOnly {
    fn look(&self, _: ObjectId) -> SvgLook {
        SvgLook::Keep
    }

    fn ghosts(&self) -> &[SvgGhost] {
        &[]
    }

    fn view(&self) -> Option<(DVec2, DVec2)> {
        Some(self.0)
    }
}

/// What a PNG request asks for, after the checks of `render.png`.
pub(crate) struct PngRequest {
    pub settings: ViewSettings,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The width and height in the first `<svg ...>` tag.
fn svg_size(svg: &str) -> (f64, f64) {
    let tag = svg.split('>').next().unwrap_or("");
    let num = |key: &str| {
        tag.split(&format!("{key}=\""))
            .nth(1)
            .and_then(|r| r.split('"').next())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (num("width"), num("height"))
}

/// Two SVG pictures side by side, with a label above each. The pictures keep their own sizes.
fn side_by_side_svg(a: &str, b: &str, labels: [&str; 2]) -> String {
    const STRIP: f64 = 24.0;
    let (wa, ha) = svg_size(a);
    let (wb, hb) = svg_size(b);
    let (gap, width, height) = (
        f64::from(GAP_PX),
        wa + f64::from(GAP_PX) + wb,
        ha.max(hb) + STRIP,
    );
    let place =
        |svg: &str, x: f64| svg.replacen("<svg ", &format!("<svg x=\"{x}\" y=\"{STRIP}\" "), 1);
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" data-compare-mode=\"side_by_side\">\n"
    );
    s += &format!(
        "<rect width=\"{width}\" height=\"{height}\" fill=\"white\"/>\n<text x=\"6\" y=\"16\" font-family=\"sans-serif\" font-size=\"14\">{}</text>\n<text x=\"{}\" y=\"16\" font-family=\"sans-serif\" font-size=\"14\">{}</text>\n",
        esc(labels[0]),
        wa + gap + 6.0,
        esc(labels[1])
    );
    s += &place(a, 0.0);
    s += &place(b, wa + gap);
    s += &format!(
        "<line x1=\"{x}\" y1=\"0\" x2=\"{x}\" y2=\"{height}\" stroke=\"gray\"/>\n</svg>\n",
        x = wa + gap / 2.0
    );
    s
}

/// Two pictures of the same height side by side, with a gray line between them.
fn join(a: &Image, b: &Image) -> Image {
    let height = a.height.max(b.height);
    let width = a.width + GAP_PX + b.width;
    let mut out = Image::new(width, height, [160, 160, 160, 255]);
    for (img, x0) in [(a, 0usize), (b, (a.width + GAP_PX) as usize)] {
        for y in 0..img.height as usize {
            let from = y * img.width as usize * 4;
            let to = (y * width as usize + x0) * 4;
            let row = img.width as usize * 4;
            out.rgba[to..to + row].copy_from_slice(&img.rgba[from..from + row]);
        }
    }
    out
}

/// The box around two boxes.
fn union(a: Option<Bounds3>, b: Option<Bounds3>) -> Option<Bounds3> {
    match (a, b) {
        (Some((al, ah)), Some((bl, bh))) => Some((al.min(bl), ah.max(bh))),
        (a, b) => a.or(b),
    }
}

impl Session {
    /// Runs `f` on the document of a side with the caches that go with it: the caches of the
    /// session for the open drawing, new ones (that keep nothing) for a built version.
    fn with_shown<R>(
        &self,
        shown: &ShownDoc,
        f: impl FnOnce(&Document, u64, &kadakas_site::SiteCache, &kadakas_annot::AnnotCache) -> R,
    ) -> R {
        match shown {
            ShownDoc::Live => f(
                self.editor.doc(),
                self.editor.revision(),
                &self.site_cache,
                &self.annot_cache,
            ),
            ShownDoc::Built(d) => f(
                d,
                0,
                &kadakas_site::SiteCache::default(),
                &kadakas_annot::AnnotCache::default(),
            ),
        }
    }

    /// The 3D scene of a side. With an `origin`, the scene is built around it (two scenes that
    /// are drawn together need the same one).
    pub fn compare_scene(
        &mut self,
        shown: &ShownDoc,
        origin: Option<DVec3>,
    ) -> std::sync::Arc<Scene> {
        if let ShownDoc::Live = shown {
            let scene = self.scene();
            if origin.is_none_or(|o| o == scene.origin) {
                return scene;
            }
        }
        self.with_shown(shown, |doc, revision, site, _| {
            let ext = kadakas_site::draw::scene_ext(site, revision);
            let origin = origin.unwrap_or_else(|| mesh::scene_origin(doc));
            std::sync::Arc::new(mesh::build_with_detail(doc, origin, Some(&ext), None))
        })
    }

    /// The name of a side for a label.
    fn side_label(&mut self, side: Side) -> String {
        let h = review_api::read(self);
        match side {
            Side::Current => "current drawing".to_string(),
            Side::Version(id) => h
                .meta(id)
                .map_or_else(|| format!("version {id}"), |m| m.name.clone()),
        }
    }

    /// `render.svg` with a `compare` option.
    pub(crate) fn render_svg_compare(
        &mut self,
        width: u32,
        spec: &CompareSpec,
    ) -> Result<Value, RpcError> {
        if spec.mode == CompareMode::Overlay {
            let overlay = self.compare_overlay(spec.from, spec.to)?;
            let shown = self.compare_doc(spec.to)?;
            let svg = self.with_shown(&shown, |doc, revision, site, annot| {
                let ov = SvgCompare::new(doc, &overlay, spec.style);
                crate::svg_of(doc, width, revision, site, annot, Some(&ov))
            });
            let info = self.compare_info(spec, Some(&overlay));
            return Ok(json!({"svg": svg, "compare": info}));
        }
        let (a, b) = (self.compare_doc(spec.from)?, self.compare_doc(spec.to)?);
        // The same part of the model in both pictures.
        let bounds = |s: &Session, shown: &ShownDoc| {
            s.with_shown(shown, |d, _, _, _| kadakas_core::svg::drawing_bounds(d))
        };
        let (lo, hi) = match (bounds(self, &a), bounds(self, &b)) {
            (Some((al, ah)), Some((bl, bh))) => (al.min(bl), ah.max(bh)),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => (DVec2::ZERO, DVec2::splat(1000.0)),
        };
        let margin = ((hi - lo).max_element() * 0.05).max(1.0);
        let view = ViewOnly((lo - margin, hi + margin));
        let half = (width.clamp(16, 8192).saturating_sub(GAP_PX) / 2).max(16);
        let svg_of = |s: &Session, shown: &ShownDoc| {
            s.with_shown(shown, |doc, revision, site, annot| {
                crate::svg_of(doc, half, revision, site, annot, Some(&view))
            })
        };
        let (sa, sb) = (svg_of(self, &a), svg_of(self, &b));
        let labels = [self.side_label(spec.from), self.side_label(spec.to)];
        let svg = side_by_side_svg(&sa, &sb, [&labels[0], &labels[1]]);
        let info = self.compare_info(spec, None);
        Ok(json!({"svg": svg, "compare": info}))
    }

    /// `render.png` with a `compare` option.
    pub(crate) fn render_png_compare(
        &mut self,
        req: &PngRequest,
        spec: &CompareSpec,
    ) -> Result<Value, RpcError> {
        let dark = spec.style.dark;
        let background = if dark { DARK_BACKGROUND } else { BACKGROUND };
        let draw = |scene: &Scene, look: &Look, camera: &Camera, w: u32, h: u32| {
            shade::render_scene(
                scene,
                look,
                camera,
                &ShadeOptions {
                    width: w,
                    height: h,
                    mode: req.settings.mode,
                    edges: true,
                    shadows: false,
                    light: Light::Fixed,
                    background,
                    line_px: 1.2,
                    samples: req.samples,
                },
            )
        };
        let camera_for = |bounds: Option<Bounds3>, w: u32, h: u32| {
            let mut c = Camera::standard(req.settings.view, req.settings.projection);
            c.fit(bounds, w as f32 / h as f32);
            c
        };
        let (img, info) = if spec.mode == CompareMode::Overlay {
            let overlay = self.compare_overlay(spec.from, spec.to)?;
            let shown = self.compare_doc(spec.to)?;
            let scene = self.compare_scene(&shown, None);
            let (tinted, ghosts) = self.with_shown(&shown, |doc, _, _, _| {
                tint_scene(&scene, doc, &overlay, &spec.style)
            });
            let ghosts: HashSet<ObjectId> = ghosts.into_iter().collect();
            let [r, g, b] = spec.style.color(Status::Removed);
            let look = Look::ghosted(
                &tinted,
                &|id| ghosts.contains(&id),
                [r, g, b],
                spec.style.ghost,
            );
            let camera = camera_for(tinted.bounds, req.width, req.height);
            let img = draw(&tinted, &look, &camera, req.width, req.height);
            (img, self.compare_info(spec, Some(&overlay)))
        } else {
            let b = self.compare_doc(spec.to)?;
            let a = self.compare_doc(spec.from)?;
            let scene_b = self.compare_scene(&b, None);
            let scene_a = self.compare_scene(&a, Some(scene_b.origin));
            let half = (req.width.saturating_sub(GAP_PX) / 2).max(16);
            let camera = camera_for(union(scene_a.bounds, scene_b.bounds), half, req.height);
            let left = draw(&scene_a, &Look::plain(&scene_a), &camera, half, req.height);
            let right = draw(&scene_b, &Look::plain(&scene_b), &camera, half, req.height);
            (join(&left, &right), self.compare_info(spec, None))
        };
        use base64::Engine;
        Ok(json!({
            "png_base64": base64::engine::general_purpose::STANDARD.encode(img.to_png()),
            "width": img.width,
            "height": img.height,
            "view": req.settings.view.id(),
            "projection": req.settings.projection.id(),
            "mode": req.settings.mode.id(),
            "compare": info,
        }))
    }

    /// The sheet `layer` of two sides on one page, side by side, each with its label (the old
    /// side at the left). The sheet must exist in both. The app shows the page in its Compare
    /// window; `sheet.render` with `compare` writes it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compare_sheet_page(
        &mut self,
        layer: kadakas_core::model::LayerId,
        from: Side,
        to: Side,
    ) -> Result<kadakas_print::Page, RpcError> {
        let labels = [self.side_label(from), self.side_label(to)];
        let mut pages = Vec::new();
        for (side, label) in [from, to].into_iter().zip(&labels) {
            let shown = self.compare_doc(side)?;
            let page = match &shown {
                ShownDoc::Live => self.sheet_page(layer)?.as_ref().clone(),
                ShownDoc::Built(d) => kadakas_print::compose_page(d, layer)
                    .map_err(|e| bad(format!("the sheet cannot be drawn for {label}: {e}")))?,
            };
            pages.push(page);
        }
        Ok(pages[0].beside(&pages[1], GAP_MM, [&labels[0], &labels[1]]))
    }

    /// `sheet.render` with a `compare` option: the sheet of both sides on one page.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn sheet_compare(
        &mut self,
        sheet: &Value,
        spec: &CompareSpec,
        format: &str,
        dpi: f64,
    ) -> Result<Value, RpcError> {
        if spec.mode_given && spec.mode == CompareMode::Overlay {
            return Err(bad(
                "sheets compare side by side: use compare.mode side_by_side, or leave mode out",
            ));
        }
        let spec = &CompareSpec {
            mode: CompareMode::SideBySide,
            ..*spec
        };
        let layer = crate::sheets_api::sheet_ref(self.editor.doc(), sheet)?;
        let page = self.compare_sheet_page(layer, spec.from, spec.to)?;
        let info = self.compare_info(spec, None);
        match format {
            "svg" => Ok(json!({
                "svg": kadakas_print::svg::page_svg(&page),
                "width_mm": page.size.x,
                "height_mm": page.size.y,
                "compare": info,
            })),
            "png" => {
                let bytes = kadakas_print::png::page_png(&page, dpi).map_err(crate::errors::app)?;
                use base64::Engine;
                let (w, h) = crate::sheets_api::png_size(&bytes);
                Ok(json!({
                    "png_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "width": w,
                    "height": h,
                    "compare": info,
                }))
            }
            _ => Err(bad("format is svg or png")),
        }
    }
}
