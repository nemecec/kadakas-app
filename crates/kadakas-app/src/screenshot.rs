//! `view.screenshot`: a PNG picture of the live window for agents (API-14).
//!
//! The agent server asks the window for a screenshot (`ViewportCommand::Screenshot`). The window
//! sends the picture in a later frame as an event. `Pending` keeps the reply until then. The
//! picture is cut to the drawing area (default) or the whole window, scaled down if it is wider
//! than `max_width`, and sent as base64 PNG.

use std::sync::mpsc::Sender;

use kadakas_render::raster::Image;
use serde_json::{Value, json};

/// How long to wait for the picture, in seconds.
pub const TIMEOUT: f64 = 10.0;
pub const DEFAULT_MAX_WIDTH: u32 = 2048;
const MIN_WIDTH: u32 = 64;
const MAX_WIDTH: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Drawing,
    Window,
}

impl Region {
    fn id(self) -> &'static str {
        match self {
            Region::Drawing => "drawing",
            Region::Window => "window",
        }
    }
}

/// A valid request: what to take and how big.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub region: Region,
    pub max_width: u32,
}

/// A request that waits for its picture.
pub struct Pending {
    pub id: Value,
    pub reply: Sender<Option<String>>,
    pub request: Request,
    /// Time of the request (seconds, from egui).
    pub since: f64,
    /// The key that comes back with the picture.
    pub key: u64,
}

fn error_line(id: &Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// Reads a line. `None`: not a screenshot request. `Some(Err(line))`: a request with bad
/// parameters, with the error line to send back. `Some(Ok((id, request)))`: a good request.
pub fn parse(line: &str) -> Option<Result<(Value, Request), String>> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("method")?.as_str()? != "view.screenshot" {
        return None;
    }
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    let params = if params.is_null() { json!({}) } else { params };
    let bad = |m: &str| Some(Err(error_line(&id, -32602, m)));
    let Some(obj) = params.as_object() else {
        return bad("params must be an object");
    };
    let region = match obj.get("region").map(Value::as_str) {
        None | Some(Some("drawing")) => Region::Drawing,
        Some(Some("window")) => Region::Window,
        _ => return bad("region must be drawing or window"),
    };
    let max_width = match obj.get("max_width") {
        None | Some(Value::Null) => DEFAULT_MAX_WIDTH,
        Some(v) => match v.as_u64() {
            Some(n) if (MIN_WIDTH as u64..=MAX_WIDTH as u64).contains(&n) => n as u32,
            _ => return bad("max_width must be a whole number from 64 to 4096"),
        },
    };
    Some(Ok((id, Request { region, max_width })))
}

/// The picture cut to `crop` (in points; the whole image if `None`), scaled down to `max_width`,
/// as an RGBA image.
pub fn prepare(
    image: &egui::ColorImage,
    crop: Option<egui::Rect>,
    pixels_per_point: f32,
    max_width: u32,
) -> Result<Image, String> {
    let [w, h] = image.size;
    if w == 0 || h == 0 {
        return Err("the window picture is empty".into());
    }
    let (x0, y0, x1, y1) = match crop {
        Some(r) => {
            let ppp = pixels_per_point.max(0.01);
            (
                ((r.min.x * ppp).floor().max(0.0) as usize).min(w),
                ((r.min.y * ppp).floor().max(0.0) as usize).min(h),
                ((r.max.x * ppp).ceil().max(0.0) as usize).min(w),
                ((r.max.y * ppp).ceil().max(0.0) as usize).min(h),
            )
        }
        None => (0, 0, w, h),
    };
    if x1 <= x0 || y1 <= y0 {
        return Err("the drawing area is not visible".into());
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    // Scale down by whole steps of a box filter, so lines stay clean.
    let scale = (cw as f64 / max_width as f64).max(1.0);
    let (ow, oh) = (
        ((cw as f64 / scale).round() as usize).max(1),
        ((ch as f64 / scale).round() as usize).max(1),
    );
    let mut rgba = Vec::with_capacity(ow * oh * 4);
    for oy in 0..oh {
        let (sy0, sy1) = (
            y0 + (oy as f64 * scale) as usize,
            (y0 + ((oy + 1) as f64 * scale).ceil() as usize).clamp(y0 + 1, y1),
        );
        for ox in 0..ow {
            let (sx0, sx1) = (
                x0 + (ox as f64 * scale) as usize,
                (x0 + ((ox + 1) as f64 * scale).ceil() as usize).clamp(x0 + 1, x1),
            );
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in sy0..sy1.min(y1) {
                for x in sx0..sx1.min(x1) {
                    let c = image.pixels[y * w + x];
                    sum[0] += c.r() as u32;
                    sum[1] += c.g() as u32;
                    sum[2] += c.b() as u32;
                    sum[3] += c.a() as u32;
                    n += 1;
                }
            }
            let n = n.max(1);
            rgba.extend_from_slice(&[
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                (sum[3] / n) as u8,
            ]);
        }
    }
    Ok(Image {
        width: ow as u32,
        height: oh as u32,
        rgba,
    })
}

/// The answer line for a picture.
pub fn answer(id: &Value, img: &Image, req: &Request, dark: bool, view: &str) -> String {
    use base64::Engine;
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "png_base64": base64::engine::general_purpose::STANDARD.encode(img.to_png()),
            "width": img.width,
            "height": img.height,
            "region": req.region.id(),
            "theme": if dark { "dark" } else { "light" },
            "view": view,
        }
    })
    .to_string()
}

pub fn timeout_line(id: &Value) -> String {
    error_line(
        id,
        1,
        "the window did not give a picture in time. Is it hidden or minimized?",
    )
}

pub fn failure_line(id: &Value, message: &str) -> String {
    error_line(id, 1, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Color32, ColorImage};

    #[test]
    fn requests_are_read_and_checked() {
        assert!(parse("{not json").is_none());
        assert!(parse(r#"{"id":1,"method":"undo"}"#).is_none());
        let Some(Ok((id, r))) = parse(r#"{"id":7,"method":"view.screenshot"}"#) else {
            panic!()
        };
        assert_eq!(
            (id, r),
            (
                json!(7),
                Request {
                    region: Region::Drawing,
                    max_width: DEFAULT_MAX_WIDTH
                }
            )
        );
        let Some(Ok((_, r))) = parse(
            r#"{"id":7,"method":"view.screenshot","params":{"region":"window","max_width":300}}"#,
        ) else {
            panic!()
        };
        assert_eq!(
            r,
            Request {
                region: Region::Window,
                max_width: 300
            }
        );
        for bad in [
            r#"{"id":1,"method":"view.screenshot","params":{"region":"all"}}"#,
            r#"{"id":1,"method":"view.screenshot","params":{"max_width":10}}"#,
            r#"{"id":1,"method":"view.screenshot","params":{"max_width":"big"}}"#,
            r#"{"id":1,"method":"view.screenshot","params":[1]}"#,
        ] {
            let Some(Err(line)) = parse(bad) else {
                panic!("{bad}")
            };
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["error"]["code"], -32602, "{bad}");
        }
    }

    fn image(w: usize, h: usize) -> ColorImage {
        let mut img = ColorImage::filled([w, h], Color32::WHITE);
        // A red square in the middle.
        for y in h / 4..h * 3 / 4 {
            for x in w / 4..w * 3 / 4 {
                img.pixels[y * w + x] = Color32::from_rgb(200, 0, 0);
            }
        }
        img
    }

    #[test]
    fn the_picture_is_cut_and_scaled() {
        let img = image(400, 200);
        let whole = prepare(&img, None, 1.0, 2048).unwrap();
        assert_eq!((whole.width, whole.height), (400, 200));
        assert_eq!(whole.pixel(200, 100), [200, 0, 0, 255]);
        // Cut to a part, with 2 pixels per point.
        let part = prepare(
            &img,
            Some(egui::Rect::from_min_max(
                egui::pos2(50.0, 25.0),
                egui::pos2(150.0, 75.0),
            )),
            2.0,
            2048,
        )
        .unwrap();
        assert_eq!((part.width, part.height), (200, 100));
        // Scaled down to a width.
        let small = prepare(&img, None, 1.0, 100).unwrap();
        assert_eq!((small.width, small.height), (100, 50));
        assert_eq!(small.pixel(50, 25), [200, 0, 0, 255]);
        assert_eq!(small.pixel(0, 0), [255, 255, 255, 255]);
        // Errors.
        assert!(prepare(&ColorImage::filled([0, 0], Color32::WHITE), None, 1.0, 100).is_err());
        let outside = egui::Rect::from_min_max(egui::pos2(900.0, 900.0), egui::pos2(950.0, 950.0));
        assert!(prepare(&img, Some(outside), 1.0, 100).is_err());
    }

    #[test]
    fn the_answer_holds_a_png() {
        use base64::Engine;
        let img = prepare(&image(64, 64), None, 1.0, 2048).unwrap();
        let line = answer(
            &json!(3),
            &img,
            &Request {
                region: Region::Drawing,
                max_width: 64,
            },
            true,
            "plan",
        );
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 3);
        assert_eq!(v["result"]["theme"], "dark");
        assert_eq!(v["result"]["region"], "drawing");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(v["result"]["png_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        for l in [timeout_line(&json!(1)), failure_line(&json!(1), "x")] {
            let e: Value = serde_json::from_str(&l).unwrap();
            assert_eq!(e["error"]["code"], 1);
        }
    }
}
