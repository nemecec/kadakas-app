//! SVG output of one sheet. The size is in mm, so it prints at the right size.

use std::fmt::Write;

use base64::Engine;
use glam::DVec2;

use crate::display::*;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn css(c: Rgba) -> String {
    format!("rgb({},{},{})", c[0], c[1], c[2])
}

fn opacity(c: Rgba) -> f64 {
    c[3] as f64 / 255.0
}

/// PNG bytes of an RGB image, for a data URI.
pub fn rgb_png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("png header");
    w.write_image_data(rgb).expect("png data");
    w.finish().expect("png end");
    out
}

/// The SVG text of a page.
pub fn page_svg(page: &Page) -> String {
    let h = page.size.y;
    let y = |v: f64| h - v;
    let pt = |p: DVec2| format!("{:.3} {:.3}", p.x, y(p.y));
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="0 0 {} {}">"#,
        page.size.x, page.size.y, page.size.x, page.size.y
    );
    let _ = writeln!(s, "<title>{}</title>", esc(&page.title));
    if !page.notes.is_empty() {
        let _ = writeln!(s, "<desc>{}</desc>", esc(&page.notes.join("\n")));
    }
    let _ = writeln!(
        s,
        r#"<rect width="{}" height="{}" fill="white"/>"#,
        page.size.x, page.size.y
    );
    let mut clips = 0;
    let mut open = 0;
    for prim in &page.prims {
        match prim {
            Prim::PushClip(poly) => {
                clips += 1;
                open += 1;
                let d = poly
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("{}{}", if i == 0 { "M " } else { "L " }, pt(*p)))
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = writeln!(
                    s,
                    r#"<clipPath id="c{clips}"><path d="{d} Z"/></clipPath><g clip-path="url(#c{clips})">"#
                );
            }
            Prim::PopClip => {
                if open > 0 {
                    open -= 1;
                    s.push_str("</g>\n");
                }
            }
            Prim::Path(p) => {
                if p.segs.is_empty() {
                    continue;
                }
                let mut d = String::new();
                for seg in &p.segs {
                    let _ = match *seg {
                        PSeg::Move(a) => write!(d, "M {} ", pt(a)),
                        PSeg::Line(a) => write!(d, "L {} ", pt(a)),
                        PSeg::Cubic(a, b, c) => write!(d, "C {}, {}, {} ", pt(a), pt(b), pt(c)),
                    };
                }
                if p.closed {
                    d.push('Z');
                }
                let fill = p.fill.map_or("none".to_string(), css);
                let (stroke, so) = p
                    .stroke
                    .map_or(("none".to_string(), 1.0), |c| (css(c), opacity(c)));
                let fo = p.fill.map_or(1.0, opacity);
                let dash = p.dash.map_or(String::new(), |[a, b]| {
                    format!(r#" stroke-dasharray="{a} {b}""#)
                });
                let _ = writeln!(
                    s,
                    r#"<path d="{}" fill="{fill}" fill-opacity="{fo:.3}" stroke="{stroke}" stroke-opacity="{so:.3}" stroke-width="{}"{dash}/>"#,
                    d.trim_end(),
                    p.width
                );
            }
            Prim::Text(t) => {
                let anchor = if t.align == Align::Center {
                    "middle"
                } else {
                    "start"
                };
                let _ = writeln!(
                    s,
                    r#"<text x="{:.3}" y="{:.3}" font-size="{}" font-family="Helvetica, Arial, sans-serif" text-anchor="{anchor}" fill="{}">{}</text>"#,
                    t.at.x,
                    y(t.at.y),
                    t.height,
                    css(t.color),
                    esc(&t.text)
                );
            }
            Prim::Image(i) => {
                let data = base64::engine::general_purpose::STANDARD
                    .encode(rgb_png(i.width, i.height, &i.rgb));
                let _ = writeln!(
                    s,
                    r#"<image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none" href="data:image/png;base64,{data}"/>"#,
                    i.min.x,
                    y(i.max.y),
                    i.max.x - i.min.x,
                    i.max.y - i.min.y
                );
            }
        }
    }
    for _ in 0..open {
        s.push_str("</g>\n");
    }
    s.push_str("</svg>\n");
    s
}
