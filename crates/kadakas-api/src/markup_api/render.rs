//! The pictures of a markup (REV-11): the model at the stored viewpoint, with and without marks.
//!
//! The clean picture is `render.png` with the camera of the viewpoint. A plan viewpoint is a
//! parallel camera that looks straight down and shows the same part of the drawing as the canvas
//! did, as the snapshots of the issues do. The picture with the marks is the clean picture with the
//! marks painted over it (`kadakas_print::markup`, the same geometry that the app paints on the
//! canvas, `kadakas_core::markup::draw`).

use base64::Engine;
use serde_json::{Value, json};

use kadakas_core::markup::{Markup, MarkupViewpoint};

use super::pick::{plan_camera, view_camera};
use crate::{RpcError, Session};

/// The width of a picture when the caller gives none, in pixels.
pub const DEFAULT_WIDTH: u32 = 640;

/// A PNG with its size.
pub(super) struct Picture {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Picture {
    pub fn json(&self) -> Value {
        json!({
            "png_base64": base64::engine::general_purpose::STANDARD.encode(&self.png),
            "width": self.width,
            "height": self.height,
        })
    }
}

impl Session {
    /// The size of a picture of a canvas, about `width` pixels wide: the shape of the canvas is
    /// kept, and the size fits the limits of the session (the longest side and the pixels). A
    /// canvas so long and thin that it cannot keep its shape within the limits (the short side
    /// would be under 16 pixels) is refused with a message.
    pub(super) fn fit_picture(&self, canvas: [f64; 2], width: u32) -> Result<(u32, u32), RpcError> {
        let limits = self.limits();
        let side = f64::from(limits.render_side.max(16));
        let pixels = limits.render_pixels as f64;
        let (mut w, mut h) = (
            f64::from(width.max(1)),
            f64::from(width.max(1)) * canvas[1] / canvas[0],
        );
        let k = (side / w.max(h)).min((pixels / (w * h)).sqrt()).min(1.0);
        (w, h) = (w * k, h * k);
        // The short side is at least 16 pixels: scale up if the shape allows it.
        let up = 16.0 / w.min(h);
        if up > 1.0 {
            if w.max(h) * up > side || w * up * h * up > pixels {
                return Err(RpcError::limit(format!(
                    "a canvas of {} by {} points is too long and thin for a picture of at most {side} pixels on a side: use a canvas with a shape nearer to a square",
                    canvas[0], canvas[1]
                )));
            }
            (w, h) = (w * up, h * up);
        }
        Ok((w.floor().max(16.0) as u32, h.floor().max(16.0) as u32))
    }

    /// The model at the viewpoint, without marks, `width` pixels wide (the height follows the
    /// shape of the canvas). The size is bounded by the limits of the session.
    pub(super) fn markup_clean(
        &mut self,
        vp: &MarkupViewpoint,
        width: u32,
    ) -> Result<Picture, RpcError> {
        let camera = view_camera(vp)
            .or_else(|| plan_camera(vp))
            .ok_or_else(|| RpcError::validation("the viewpoint has no view"))?;
        let (width, height) = self.fit_picture(vp.canvas, width)?;
        let r = self.dispatch(
            "render.png",
            json!({"camera": camera, "width": width, "height": height}),
        )?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(r["png_base64"].as_str().unwrap_or_default())
            .map_err(|e| RpcError::app(format!("the render gave no picture: {e}")))?;
        Ok(Picture {
            png,
            width: r["width"].as_u64().unwrap_or(0) as u32,
            height: r["height"].as_u64().unwrap_or(0) as u32,
        })
    }

    /// The marks of a markup painted over a clean picture.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn markup_marked(
        &self,
        markup: &Markup,
        clean: &Picture,
    ) -> Result<Picture, RpcError> {
        let max = self.limits().picture_limits().max_pixels;
        let mut img = kadakas_render::texture::decode_image_limited(&clean.png, max)
            .map_err(|e| RpcError::app(format!("the render cannot be read: {e}")))?;
        let prims = kadakas_core::markup::draw::markup_prims(
            markup,
            [f64::from(img.width), f64::from(img.height)],
        );
        kadakas_print::markup::paint_marks(&mut img.rgba, img.width, img.height, &prims, 1.0)
            .map_err(|e| RpcError::app(e.to_string()))?;
        let png = kadakas_render::texture::encode_png(img.width, img.height, &img.rgba)
            .map_err(RpcError::app)?;
        Ok(Picture {
            png,
            width: img.width,
            height: img.height,
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn markup_marked(
        &self,
        _markup: &Markup,
        _clean: &Picture,
    ) -> Result<Picture, RpcError> {
        Err(RpcError::policy(
            "a browser cannot paint the marks into a picture: read the marks as JSON",
        ))
    }
}
