//! Annotations as SVG elements, for `render.svg` (model Y up, so the output flips Y).

use std::fmt::Write;

use cadektor_core::annot::draw::{Fill, Prim};
use cadektor_core::model::{Color, Document, Object, Shape};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The value of `name="..."` in an attribute string.
fn attr<'a>(style: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = style.find(&key)? + key.len();
    let len = style[start..].find('"')?;
    Some(&style[start..start + len])
}

/// The SVG of primitives. `stroke` is the pen of the object (a CSS color) and `weight` the
/// stroke width.
pub fn prims_svg(prims: &[Prim], stroke: &str, weight: f64, group_attrs: &str) -> String {
    let mut s = String::new();
    let _ = write!(s, "<g {group_attrs}>");
    for p in prims {
        match p {
            Prim::Poly { pts, closed, fill, color } => {
                let pen = color.map_or_else(|| stroke.to_string(), Color::to_css);
                let fill = match fill {
                    Fill::None => "none".to_string(),
                    Fill::Ink => pen.clone(),
                    Fill::Color(c) => c.to_css(),
                };
                let points: Vec<String> = pts.iter().map(|q| format!("{},{}", q.x, -q.y)).collect();
                let tag = if *closed { "polygon" } else { "polyline" };
                let _ = write!(s, r#"<{tag} points="{}" stroke="{pen}" fill="{fill}" stroke-width="{weight}"/>"#, points.join(" "));
            }
            Prim::Text { at, text, height, center, bold, color } => {
                let fill = color.map_or_else(|| stroke.to_string(), Color::to_css);
                let anchor = if *center { r#" text-anchor="middle""# } else { "" };
                let weight = if *bold { r#" font-weight="bold""# } else { "" };
                let _ = write!(
                    s,
                    r#"<text x="{}" y="{}" font-size="{height}" font-family="sans-serif" fill="{fill}"{anchor}{weight}>{}</text>"#,
                    at.x,
                    -at.y,
                    esc(text)
                );
            }
        }
    }
    s.push_str("</g>\n");
    s
}

/// The SVG of an annotation object, as `cadektor_core::svg::render_with` wants it from an
/// extension: it gets the document, the object, the attribute string of the object and the output.
pub fn object_svg(doc: &Document, obj: &Object, style: &str, out: &mut String) {
    let Shape::Annotation(a) = &obj.shape else { return };
    let stroke = attr(style, "stroke").unwrap_or("rgba(0,0,0,1.000)");
    let weight: f64 = attr(style, "stroke-width").and_then(|w| w.parse().ok()).unwrap_or(1.0);
    let prims = crate::prims(doc, obj);
    let group = format!(r#"data-kind="annotation" data-type="{}" data-id="{}""#, a.type_name(), obj.id);
    out.push_str(&prims_svg(&prims, stroke, weight, &group));
}
