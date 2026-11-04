//! Marks painted into a picture (REV-11): the render with marks that `markup.get` gives and the
//! snapshot of a markup in a BCF file.
//!
//! The geometry is made by `kadakas_core::markup::draw` (the app paints the same primitives with
//! egui). This module draws them with `tiny-skia`, antialiased, over a picture of opaque RGBA
//! pixels. A highlighter is half transparent.

use ab_glyph::{Font, PxScale, ScaleFont};
use kadakas_core::markup::Rgb;
use kadakas_core::markup::draw::{NOTE_BACKGROUND_ALPHA, Prim};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::PrintError;

fn paint(color: Rgb, alpha: f64) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(
        color.0[0],
        color.0[1],
        color.0[2],
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    p.anti_alias = true;
    p
}

fn polygon(points: &[[f64; 2]], k: f64, closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for (i, p) in points.iter().enumerate() {
        let (x, y) = ((p[0] * k) as f32, (p[1] * k) as f32);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

/// Paints `prims` into the picture `rgba` of `width` by `height` pixels (straight, opaque
/// pixels). `k` is the number of pixels for one point of the canvas the primitives were made for.
pub fn paint_marks(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    prims: &[Prim],
    k: f64,
) -> Result<(), PrintError> {
    if rgba.len() != width as usize * height as usize * 4 || !(k.is_finite() && k > 0.0) {
        return Err(PrintError::Invalid(
            "the picture does not fit its size".into(),
        ));
    }
    let mut pix =
        Pixmap::new(width, height).ok_or_else(|| PrintError::Invalid("empty picture".into()))?;
    pix.data_mut().copy_from_slice(rgba);
    for prim in prims {
        match prim {
            Prim::Line {
                points,
                closed,
                width,
                color,
                alpha,
                round,
            } => {
                let Some(path) = polygon(points, k, *closed) else {
                    continue;
                };
                let stroke = Stroke {
                    width: ((*width * k) as f32).max(1.0),
                    line_cap: if *round {
                        LineCap::Round
                    } else {
                        LineCap::Butt
                    },
                    line_join: if *round {
                        LineJoin::Round
                    } else {
                        LineJoin::Miter
                    },
                    ..Stroke::default()
                };
                pix.stroke_path(
                    &path,
                    &paint(*color, *alpha),
                    &stroke,
                    Transform::identity(),
                    None,
                );
            }
            Prim::Fill {
                points,
                color,
                alpha,
            } => {
                if let Some(path) = polygon(points, k, true) {
                    pix.fill_path(
                        &path,
                        &paint(*color, *alpha),
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            Prim::Dot {
                at,
                radius,
                color,
                alpha,
            } => dot(&mut pix, *at, *radius, k, *color, *alpha),
            Prim::Note {
                at,
                text,
                size,
                color,
            } => note(&mut pix, *at, text, *size, k, *color),
        }
    }
    rgba.copy_from_slice(pix.data());
    Ok(())
}

fn dot(pix: &mut Pixmap, at: [f64; 2], radius: f64, k: f64, color: Rgb, alpha: f64) {
    let mut pb = PathBuilder::new();
    pb.push_circle(
        (at[0] * k) as f32,
        (at[1] * k) as f32,
        ((radius * k) as f32).max(0.5),
    );
    if let Some(path) = pb.finish() {
        pix.fill_path(
            &path,
            &paint(color, alpha),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// A note: a dot at the place and the text in a box to the right of it.
fn note(pix: &mut Pixmap, at: [f64; 2], text: &str, size: f64, k: f64, color: Rgb) {
    dot(pix, at, 4.0 * size / 15.0, k, color, 1.0);
    let font = crate::png::font();
    let px = ((size * k) as f32).max(6.0);
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return;
    }
    let width = |l: &str| {
        l.chars()
            .map(|c| scaled.h_advance(font.glyph_id(c)))
            .sum::<f32>()
    };
    let text_w = lines.iter().map(|l| width(l)).fold(0.0f32, f32::max);
    let line_h = scaled.height() + scaled.line_gap();
    let pad = px * 0.4;
    let (box_w, box_h) = (text_w + 2.0 * pad, line_h * lines.len() as f32 + 2.0 * pad);
    let (img_w, img_h) = (pix.width() as f32, pix.height() as f32);
    let mut x = (at[0] * k) as f32 + px * 0.6;
    if x + box_w > img_w {
        x = ((at[0] * k) as f32 - px * 0.6 - box_w).max(0.0);
    }
    let y = ((at[1] * k) as f32 - box_h / 2.0).clamp(0.0, (img_h - box_h).max(0.0));
    if let Some(rect) = Rect::from_xywh(x, y, box_w, box_h) {
        let path = PathBuilder::from_rect(rect);
        pix.fill_path(
            &path,
            &paint(Rgb([255, 255, 255]), NOTE_BACKGROUND_ALPHA),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        let stroke = Stroke {
            width: (px * 0.1).max(1.0),
            ..Stroke::default()
        };
        pix.stroke_path(
            &path,
            &paint(color, 1.0),
            &stroke,
            Transform::identity(),
            None,
        );
    }
    let (w, h) = (pix.width() as i32, pix.height() as i32);
    for (n, line) in lines.iter().enumerate() {
        let mut gx = x + pad;
        let base = y + pad + scaled.ascent() + line_h * n as f32;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(gx, base));
            gx += scaled.h_advance(id);
            let Some(out) = font.outline_glyph(glyph) else {
                continue;
            };
            let b = out.px_bounds();
            out.draw(|ox, oy, cov| {
                let (qx, qy) = (b.min.x as i32 + ox as i32, b.min.y as i32 + oy as i32);
                if qx < 0 || qy < 0 || qx >= w || qy >= h {
                    return;
                }
                let idx = (qy * w + qx) as usize * 4;
                let data = pix.data_mut();
                for c in 0..3 {
                    let old = f32::from(data[idx + c]);
                    data[idx + c] = (old - old * cov).round() as u8;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::markup::draw::markup_prims;
    use kadakas_core::markup::*;

    fn picture(w: u32, h: u32) -> Vec<u8> {
        vec![255; w as usize * h as usize * 4]
    }

    fn markup(marks: Vec<Mark>) -> Markup {
        let mut m = Markup::new(MarkupViewpoint {
            view: kadakas_core::issue::ViewSpec::Plan {
                center: [0.0, 0.0],
                zoom: 0.1,
            },
            fov: None,
            canvas: [200.0, 100.0],
            layers: Default::default(),
            classes: Default::default(),
        });
        m.marks = marks;
        m
    }

    fn at(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn a_stroke_is_painted_and_the_rest_stays() {
        let mut m = Mark::new(MarkKind::Pen, vec![[0.1, 0.5], [0.9, 0.5]]);
        m.color = Rgb([255, 0, 0]);
        m.width = 6.0;
        let m = markup(vec![m]);
        let (w, h) = (400u32, 200u32);
        let mut img = picture(w, h);
        paint_marks(&mut img, w, h, &markup_prims(&m, [200.0, 100.0]), 2.0).unwrap();
        assert_eq!(at(&img, w, 200, 100), [255, 0, 0, 255]);
        assert_eq!(at(&img, w, 200, 10), [255, 255, 255, 255]);
        assert_eq!(
            at(&img, w, 5, 100),
            [255, 255, 255, 255],
            "before the start"
        );
    }

    #[test]
    fn a_highlighter_is_half_transparent() {
        let mut m = Mark::new(MarkKind::Highlighter, vec![[0.1, 0.5], [0.9, 0.5]]);
        m.color = Rgb([255, 0, 0]);
        let m = markup(vec![m]);
        let (w, h) = (400u32, 200u32);
        let mut img = picture(w, h);
        paint_marks(&mut img, w, h, &markup_prims(&m, [200.0, 100.0]), 2.0).unwrap();
        let p = at(&img, w, 200, 100);
        assert_eq!(p[0], 255);
        assert!(p[1] > 100 && p[1] < 200, "{p:?}: the white shows through");
    }

    #[test]
    fn a_note_draws_its_text_and_every_kind_paints_something() {
        let mut n = Mark::new(MarkKind::Note, vec![[0.2, 0.5]]);
        n.text = "tree here".into();
        n.color = Rgb([0, 0, 255]);
        let marks = vec![
            n,
            Mark::new(MarkKind::Arrow, vec![[0.5, 0.2], [0.9, 0.2]]),
            Mark::new(MarkKind::Rectangle, vec![[0.5, 0.4], [0.9, 0.9]]),
            Mark::new(MarkKind::Ellipse, vec![[0.55, 0.45], [0.85, 0.85]]),
            Mark::new(MarkKind::Pen, vec![[0.05, 0.05]]),
        ];
        let m = markup(marks);
        let (w, h) = (400u32, 200u32);
        let mut img = picture(w, h);
        paint_marks(&mut img, w, h, &markup_prims(&m, [200.0, 100.0]), 2.0).unwrap();
        let changed = img.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
        assert!(changed > 800, "{changed}");
        assert!(paint_marks(&mut img, w, h + 1, &[], 2.0).is_err());
    }
}
