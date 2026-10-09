//! PNG output of one sheet: a CPU raster of the display list with `tiny-skia`.
//! Text uses the Ubuntu font that ships with the egui crates.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use glam::DVec2;
use tiny_skia::{
    BlendMode, FillRule, LineCap, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform,
};

use crate::display::*;
use crate::PrintError;

/// Largest side of a sheet PNG in pixels.
pub const MAX_SIDE: u32 = 12_000;

fn font() -> FontRef<'static> {
    FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("the bundled font is valid")
}

/// The width of a text in mm in the PNG font.
pub fn measure(text: &str, height: f64) -> f64 {
    let f = font();
    let scaled = f.as_scaled(PxScale::from(height as f32));
    text.chars().map(|c| scaled.h_advance(f.glyph_id(c))).sum::<f32>() as f64
}

fn color(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

/// Draws a page at `dpi` dots per inch and returns the PNG file.
pub fn page_png(page: &Page, dpi: f64) -> Result<Vec<u8>, PrintError> {
    if !(dpi.is_finite() && (10.0..=1200.0).contains(&dpi)) {
        return Err(PrintError::Invalid("dpi must be 10 to 1200".into()));
    }
    let k = dpi / 25.4;
    let (w, h) = ((page.size.x * k).round() as u32, (page.size.y * k).round() as u32);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(PrintError::Invalid(format!("the image would be {w} by {h} px. Use a lower dpi")));
    }
    let mut pix = Pixmap::new(w, h).ok_or_else(|| PrintError::Invalid("image too large".into()))?;
    pix.fill(tiny_skia::Color::WHITE);
    // Paper (mm, Y up) to pixels.
    let tf = Transform::from_row(k as f32, 0.0, 0.0, -(k as f32), 0.0, h as f32);
    let px = |p: DVec2| ((p.x * k) as f32, (h as f64 - p.y * k) as f32);
    let mut masks: Vec<Mask> = Vec::new();
    for prim in &page.prims {
        match prim {
            Prim::PushClip(poly) => {
                let mut m = Mask::new(w, h).expect("size checked");
                let mut pb = PathBuilder::new();
                for (i, p) in poly.iter().enumerate() {
                    let (x, y) = px(*p);
                    if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
                }
                pb.close();
                if let Some(path) = pb.finish() {
                    m.fill_path(&path, FillRule::Winding, true, Transform::identity());
                    if let Some(prev) = masks.last() {
                        // Keep what both masks allow.
                        for (a, b) in m.data_mut().iter_mut().zip(prev.data()) {
                            *a = ((*a as u16 * *b as u16) / 255) as u8;
                        }
                    }
                }
                masks.push(m);
            }
            Prim::PopClip => {
                masks.pop();
            }
            Prim::Path(p) => draw_path(&mut pix, p, tf, masks.last(), k),
            Prim::Text(t) => draw_text(&mut pix, t, k, h, masks.last()),
            Prim::Image(i) => draw_image(&mut pix, i, k, h, masks.last()),
        }
    }
    pix.encode_png().map_err(|e| PrintError::Invalid(e.to_string()))
}

fn draw_path(pix: &mut Pixmap, p: &PathPrim, tf: Transform, mask: Option<&Mask>, k: f64) {
    let mut pb = PathBuilder::new();
    for s in &p.segs {
        match *s {
            PSeg::Move(a) => pb.move_to(a.x as f32, a.y as f32),
            PSeg::Line(a) => pb.line_to(a.x as f32, a.y as f32),
            PSeg::Cubic(a, b, c) => pb.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, c.x as f32, c.y as f32),
        }
    }
    if p.closed {
        pb.close();
    }
    let Some(path) = pb.finish() else { return };
    if let Some(f) = p.fill {
        pix.fill_path(&path, &color(f), FillRule::Winding, tf, mask);
    }
    if let Some(s) = p.stroke {
        // Work in pixels, so a thin line is never thinner than one pixel.
        let width = ((p.width * k) as f32).max(1.0);
        let mut stroke = Stroke { width, line_cap: LineCap::Butt, ..Stroke::default() };
        if let Some([d, g]) = p.dash {
            stroke.dash = StrokeDash::new(vec![(d * k) as f32, (g * k) as f32], 0.0);
        }
        if let Some(px_path) = path.transform(tf) {
            pix.stroke_path(&px_path, &color(s), &stroke, Transform::identity(), mask);
        }
    }
}

fn draw_text(pix: &mut Pixmap, t: &TextPrim, k: f64, h: u32, mask: Option<&Mask>) {
    let f = font();
    let size = (t.height * k) as f32;
    if size < 1.0 {
        return;
    }
    let scaled = f.as_scaled(PxScale::from(size));
    let mut x = (t.at.x * k) as f32;
    if t.align == Align::Center {
        x -= t.text.chars().map(|c| scaled.h_advance(f.glyph_id(c))).sum::<f32>() / 2.0;
    }
    let y = (h as f64 - t.at.y * k) as f32;
    let (w, ph) = (pix.width() as i32, pix.height() as i32);
    for ch in t.text.chars() {
        let id = f.glyph_id(ch);
        let glyph = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(x, y));
        x += scaled.h_advance(id);
        let Some(out) = f.outline_glyph(glyph) else { continue };
        let b = out.px_bounds();
        out.draw(|gx, gy, cov| {
            let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
            if px < 0 || py < 0 || px >= w || py >= ph {
                return;
            }
            let idx = (py * w + px) as usize;
            let m = mask.map_or(255, |m| m.data()[idx]) as f32 / 255.0;
            let a = cov * m * (t.color[3] as f32 / 255.0);
            let data = pix.data_mut();
            for c in 0..3 {
                let old = data[idx * 4 + c] as f32;
                data[idx * 4 + c] = (old + (t.color[c] as f32 - old) * a).round() as u8;
            }
        });
    }
}

fn draw_image(pix: &mut Pixmap, i: &ImagePrim, k: f64, h: u32, mask: Option<&Mask>) {
    let mut src = Pixmap::new(i.width, i.height).expect("image size is valid");
    for (d, s) in src.data_mut().chunks_exact_mut(4).zip(i.rgb.chunks_exact(3)) {
        d.copy_from_slice(&[s[0], s[1], s[2], 255]);
    }
    let sx = (i.max.x - i.min.x) * k / i.width as f64;
    let sy = (i.max.y - i.min.y) * k / i.height as f64;
    let tf = Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, (i.min.x * k) as f32, (h as f64 - i.max.y * k) as f32);
    let paint = PixmapPaint { blend_mode: BlendMode::SourceOver, quality: tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() };
    pix.draw_pixmap(0, 0, src.as_ref(), &paint, tf, mask);
}
