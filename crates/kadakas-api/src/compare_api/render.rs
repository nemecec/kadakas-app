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
use kadakas_review::overlay::{
    CompareOverlay, CompareStyle, Status, SvgCompare, Tinted, tint_scene,
};

use super::*;

/// The gap between two pictures side by side, in pixels.
const GAP_PX: u32 = 4;
/// The gap between two sheets side by side, in mm.
#[cfg(not(target_arch = "wasm32"))]
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
    pub(crate) fn with_shown<R>(
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

    /// The 3D scene of a comparison with the overlay (see `kadakas_review::overlay::tint_scene`).
    /// The ghosts are made against the document of the old side, with its site models. The app
    /// draws the same scene.
    pub fn compare_tinted(
        &mut self,
        overlay: &CompareOverlay,
        from: Side,
        to: Side,
        style: &CompareStyle,
    ) -> Result<Tinted, RpcError> {
        let shown = self.compare_doc(to)?;
        let old = if style.show_ghosts {
            self.compare_old_doc(overlay, from)?
        } else {
            None
        };
        Ok(self.tint_shown(&shown, old, overlay, style))
    }

    /// The tinted scene of a shown document, with ghosts made against `old` (the document of the
    /// old side; None: the shown document itself). Joins an overlay that a caller built from its
    /// own list of changes (IOP-23, IOP-24) with the renderer.
    pub(crate) fn tint_shown(
        &mut self,
        shown: &ShownDoc,
        old: Option<Arc<Document>>,
        overlay: &CompareOverlay,
        style: &CompareStyle,
    ) -> Tinted {
        let scene = self.compare_scene(shown, None);
        self.with_shown(shown, |doc, _, _, _| {
            let site = kadakas_site::SiteCache::default();
            let ext = kadakas_site::draw::scene_ext(&site, 0);
            let old_doc: &Document = old.as_deref().unwrap_or(doc);
            tint_scene(&scene, doc, old_doc, overlay, style, Some(&ext))
        })
    }

    /// The SVG of a shown document with an overlay: the changes tinted, the ghosts of removed
    /// objects drawn against `old`. Returns the SVG and what became of the ghosts.
    pub(crate) fn overlay_svg(
        &self,
        shown: &ShownDoc,
        old: Option<&Arc<Document>>,
        overlay: &CompareOverlay,
        style: CompareStyle,
        width: u32,
    ) -> (String, GhostReport) {
        self.with_shown(shown, |doc, revision, site, annot| {
            let ov = SvgCompare::new(doc, old, overlay, style);
            let ghost = old.map(|o| self.ghost_caches.for_doc(o));
            let svg = crate::svg_of(doc, width, revision, site, annot, Some(&ov), ghost);
            (svg, ov.report())
        })
    }

    /// The picture of a tinted scene (see `tint_shown`) for a PNG request.
    pub(crate) fn overlay_image(
        &self,
        req: &PngRequest,
        t: Tinted,
        style: &CompareStyle,
    ) -> (Image, GhostReport) {
        let dark = style.dark;
        let background = if dark { DARK_BACKGROUND } else { BACKGROUND };
        let (tinted, report) = (t.scene, t.report);
        let ghosts: HashSet<ObjectId> = t.ghosts.into_iter().collect();
        let [r, g, b] = style.color(Status::Removed);
        let look = Look::ghosted(&tinted, &|id| ghosts.contains(&id), [r, g, b], style.ghost);
        let mut camera = Camera::standard(req.settings.view, req.settings.projection);
        camera.fit(tinted.bounds, req.width as f32 / req.height as f32);
        let img = shade::render_scene(
            &tinted,
            &look,
            &camera,
            &ShadeOptions {
                width: req.width,
                height: req.height,
                mode: req.settings.mode,
                edges: true,
                shadows: false,
                light: Light::Fixed,
                background,
                line_px: 1.2,
                samples: req.samples,
            },
        );
        (img, report)
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
            let old = if spec.style.show_ghosts || spec.style.show_previous {
                self.compare_old_doc(&overlay, spec.from)?
            } else {
                None
            };
            let (svg, report) = self.overlay_svg(&shown, old.as_ref(), &overlay, spec.style, width);
            let info = self.compare_info(spec, Some(&overlay), Some(report));
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
                crate::svg_of(doc, half, revision, site, annot, Some(&view), None)
            })
        };
        let (sa, sb) = (svg_of(self, &a), svg_of(self, &b));
        let labels = [self.side_label(spec.from), self.side_label(spec.to)];
        let svg = side_by_side_svg(&sa, &sb, [&labels[0], &labels[1]]);
        let info = self.compare_info(spec, None, None);
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
            let t = self.compare_tinted(&overlay, spec.from, spec.to, &spec.style)?;
            let (img, report) = self.overlay_image(req, t, &spec.style);
            (img, self.compare_info(spec, Some(&overlay), Some(report)))
        } else {
            let b = self.compare_doc(spec.to)?;
            let a = self.compare_doc(spec.from)?;
            let scene_b = self.compare_scene(&b, None);
            let scene_a = self.compare_scene(&a, Some(scene_b.origin));
            let half = (req.width.saturating_sub(GAP_PX) / 2).max(16);
            let camera = camera_for(union(scene_a.bounds, scene_b.bounds), half, req.height);
            let left = draw(&scene_a, &Look::plain(&scene_a), &camera, half, req.height);
            let right = draw(&scene_b, &Look::plain(&scene_b), &camera, half, req.height);
            (join(&left, &right), self.compare_info(spec, None, None))
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

    /// The sheet that a name or an ID means: in the new side, else in the old one.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compare_sheet_ref(
        &mut self,
        sheet: &Value,
        spec: &CompareSpec,
    ) -> Result<kadakas_core::model::LayerId, RpcError> {
        let mut first = None;
        for side in [spec.to, spec.from] {
            let shown = self.compare_doc(side)?;
            let found =
                self.with_shown(&shown, |d, _, _, _| crate::sheets_api::sheet_ref(d, sheet));
            match found {
                Ok(l) => return Ok(l),
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        Err(first.unwrap_or_else(|| bad("no such sheet")))
    }

    /// The sheet `layer` of two sides on one page, side by side, each with its label (the old
    /// side at the left). A side that does not have the sheet (it was added or deleted between the
    /// two) shows an empty page of the other size with a note. The app shows the page in its
    /// Compare window; `sheet.render` with `compare` writes it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compare_sheet_page(
        &mut self,
        layer: kadakas_core::model::LayerId,
        from: Side,
        to: Side,
    ) -> Result<kadakas_print::Page, RpcError> {
        use kadakas_core::model::LayerKind;
        let labels = [self.side_label(from), self.side_label(to)];
        let mut pages: Vec<Option<kadakas_print::Page>> = Vec::new();
        for (side, label) in [from, to].into_iter().zip(&labels) {
            let shown = self.compare_doc(side)?;
            let has = self.with_shown(&shown, |d, _, _, _| {
                d.layer(layer).is_some_and(|l| l.kind == LayerKind::Sheet)
            });
            pages.push(match (&shown, has) {
                (_, false) => None,
                (ShownDoc::Live, true) => Some(self.sheet_page(layer)?.as_ref().clone()),
                (ShownDoc::Built(d), true) => Some(
                    kadakas_print::compose_page(d, layer)
                        .map_err(|e| bad(format!("the sheet cannot be drawn for {label}: {e}")))?,
                ),
            });
        }
        let size = pages.iter().flatten().next().map(|p| p.size);
        let Some(size) = size else {
            return Err(bad("the sheet is in neither side"));
        };
        let [a, b] = [0, 1].map(|i| {
            pages[i].take().unwrap_or_else(|| {
                use kadakas_print::display::{Align, TextPrim};
                kadakas_print::Page {
                    size,
                    title: labels[i].clone(),
                    prims: vec![kadakas_print::Prim::Text(TextPrim {
                        at: size / 2.0,
                        text: "(this sheet is not in this version)".into(),
                        height: 4.0,
                        color: [120, 120, 120, 255],
                        align: Align::Center,
                    })],
                }
            })
        });
        Ok(a.beside(&b, GAP_MM, [&labels[0], &labels[1]]))
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
        if !matches!(format, "svg" | "png") {
            return Err(bad("format is svg or png"));
        }
        if format == "png" && !(10.0..=1200.0).contains(&dpi) {
            return Err(bad("dpi must be 10 to 1200"));
        }
        if spec.mode_given && spec.mode == CompareMode::Overlay {
            return Err(bad(
                "sheets compare side by side: use compare.mode side_by_side, or leave mode out",
            ));
        }
        let spec = &CompareSpec {
            mode: CompareMode::SideBySide,
            ..*spec
        };
        let layer = self.compare_sheet_ref(sheet, spec)?;
        let page = self.compare_sheet_page(layer, spec.from, spec.to)?;
        let info = self.compare_info(spec, None, None);
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
