//! `dxf.compare` (IOP-24): two DXF files compared without importing them, as a JSON diff and as an
//! overlay picture (SVG, PNG, PDF) with optional revision clouds. The files are in a private folder.

use base64::Engine;
use kadakas_api::{PathPolicy, RpcError, Session};
use kadakas_review::overlay::{CompareStyle, Palette, Status};
use serde_json::{Value, json};

fn file(ents: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(handle: u32, layer: &str, y: i64) -> String {
    format!("0\nLINE\n5\n{handle:X}\n8\n{layer}\n10\n0\n20\n{y}\n11\n4000\n21\n{y}\n")
}

/// Four lines far apart (so the tints are easy to find), and a far cluster for the clouds.
fn old_text() -> String {
    file(
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 1000),
            line(0x12, "A", 2000),
            line(0x13, "A", 3000),
        ]
        .concat(),
    )
}

/// 0x11 moved, 0x12 removed, one line added.
fn new_text() -> String {
    file(
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 1300),
            line(0x13, "A", 3000),
            line(0x14, "A", 4000),
        ]
        .concat(),
    )
}

fn temp(name: &str) -> kadakas_sys::tmp::PrivateTempDir {
    kadakas_sys::tmp::PrivateTempDir::new(name).unwrap()
}

fn write(dir: &std::path::Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn ok(s: &mut Session, p: Value) -> Value {
    s.handle("dxf.compare", p)
        .unwrap_or_else(|e| panic!("dxf.compare: {} {}", e.code, e.message))
}

fn fail(s: &mut Session, p: Value) -> RpcError {
    s.handle("dxf.compare", p)
        .expect_err("dxf.compare should fail")
}

fn files(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        write(dir, "base.dxf", &old_text()),
        write(dir, "new.dxf", &new_text()),
    )
}

fn decode(v: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(v.as_str().unwrap())
        .unwrap()
}

/// Width, height and RGBA of a PNG.
fn pixels(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width as usize, info.height as usize, buf)
}

/// The smallest distance between `rgb` and any pixel color.
fn nearest(rgba: &[u8], rgb: [u8; 3]) -> f64 {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            (0..3)
                .map(|i| (f64::from(p[i]) - f64::from(rgb[i])).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

fn tint(status: Status) -> [u8; 3] {
    CompareStyle::default().color(status)
}

const CLOUD: [u8; 3] = [150, 40, 190];

#[test]
fn the_diff_is_json_and_the_document_is_not_touched() {
    let dir = temp("kadakas-compare-json");
    let (base, new) = files(dir.path());
    let mut s = Session::with_sequential_ids();
    let before = s.editor.doc().clone();
    let r = ok(&mut s, json!({"base": base, "new": new}));
    assert_eq!(r["base"]["name"], "base.dxf");
    assert_eq!(r["new"]["name"], "new.dxf");
    assert_eq!(r["counts"]["unchanged"], 2);
    assert_eq!(r["counts"]["changed"], 1);
    assert_eq!(r["counts"]["added"], 1);
    assert_eq!(r["counts"]["removed"], 1);
    assert_eq!(r["total_changes"], 3);
    assert!(r.get("image").is_none());
    let changed = r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["status"] == "changed")
        .unwrap();
    assert_eq!(changed["handle"], "11");
    assert_eq!(changed["what"], json!(["geometry"]));
    assert_eq!(changed["via"], "handle");
    assert!(changed["id"].as_str().unwrap().starts_with('c'));
    // Nothing was imported and nothing is pending.
    assert_eq!(s.editor.doc(), &before);
    assert!(s.update_pending().is_none());
    // The same answer twice, with the same change IDs.
    let again = ok(&mut s, json!({"base": base, "new": new}));
    assert_eq!(r["changes"], again["changes"]);
    // Layers and the limit of the list.
    let r = ok(
        &mut s,
        json!({"base": base, "new": new, "options": {"limit": 1}}),
    );
    assert_eq!(r["listed"], 1);
    assert_eq!(r["total_changes"], 3);
    let r = ok(
        &mut s,
        json!({"base": base, "new": new, "options": {"layers": ["zzz"]}}),
    );
    assert_eq!(r["total_changes"], 0);
}

#[test]
fn bad_requests_are_told() {
    let dir = temp("kadakas-compare-bad");
    let (base, new) = files(dir.path());
    let mut s = Session::with_sequential_ids();
    let junk = write(dir.path(), "junk.dxf", "not a drawing");
    assert!(matches!(
        fail(&mut s, json!({"base": junk, "new": new})).kind,
        "validation" | "error"
    ));
    assert_eq!(
        fail(
            &mut s,
            json!({"base": base, "new": dir.path().join("none.dxf")})
        )
        .kind,
        "error"
    );
    for options in [
        json!({"limit": 0}),
        json!({"limit": 5000}),
        json!({"layers": "A"}),
        json!({"tolerance": "x"}),
        json!(5),
    ] {
        let e = fail(
            &mut s,
            json!({"base": base, "new": new, "options": options}),
        );
        assert_eq!(e.kind, "invalid_params", "{options}");
    }
    for render in [
        json!({}),
        json!({"format": "gif"}),
        json!({"format": "png", "revision_clouds": 3}),
        json!({"format": "png", "width": -1}),
        json!({"format": "png", "view": 3}),
        json!({"format": "png", "color": "red"}),
        json!({"format": "png", "palette": "neon"}),
        json!("png"),
    ] {
        let e = fail(&mut s, json!({"base": base, "new": new, "render": render}));
        assert_eq!(e.kind, "invalid_params", "{render}");
    }
    // The tolerance must be a number of at least 0.
    let e = fail(
        &mut s,
        json!({"base": base, "new": new, "options": {"tolerance": -2}}),
    );
    assert_eq!(e.kind, "validation");
    // A restricted session reads only inside its folders, and only DXF or DWG files.
    let outside = temp("kadakas-compare-outside");
    let elsewhere = write(outside.path(), "other.dxf", &new_text());
    s.paths = PathPolicy::restricted([dir.path().to_path_buf()]);
    assert_eq!(
        fail(&mut s, json!({"base": base, "new": elsewhere})).kind,
        "policy"
    );
    let txt = write(dir.path(), "x.txt", &new_text());
    assert_eq!(
        fail(&mut s, json!({"base": base, "new": txt})).kind,
        "policy"
    );
    // The user's own dialog is not limited.
    let r = s
        .dxf_compare_trusted(json!({"base": base, "new": elsewhere}))
        .unwrap();
    assert_eq!(r["counts"]["changed"], 1);
}

#[test]
fn the_svg_overlay_has_the_tints_the_marks_and_the_legend() {
    let dir = temp("kadakas-compare-svg");
    let (base, new) = files(dir.path());
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "svg", "width": 800}}),
    );
    assert_eq!(r["image"]["format"], "svg");
    let svg = r["image"]["svg"].as_str().unwrap();
    // The width of the picture is bounded like the size of a PNG.
    let big = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "svg", "width": 4_000_000}}),
    );
    let head = &big["image"]["svg"].as_str().unwrap()[..400];
    assert!(head.contains("width=\"2048\""), "{head}");
    for status in ["added", "changed"] {
        assert!(
            svg.contains(&format!("data-compare=\"{status}\"")),
            "{status}"
        );
    }
    assert!(svg.contains("data-ghost=\"removed\""));
    assert!(svg.contains("data-mark=\"added\">+</text>"));
    assert!(svg.contains("data-mark=\"removed\">-</text>"));
    assert!(svg.contains("data-mark=\"changed\">~</text>"));
    assert!(svg.contains(">added 1</text>") && svg.contains(">removed 1</text>"));
    assert!(!svg.contains("rgba(150,40,190"), "no clouds were asked for");
    let c = &r["compare"];
    assert_eq!(c["counts"], json!({"added": 1, "removed": 1, "changed": 1}));
    assert_eq!(c["legend"].as_array().unwrap().len(), 3);
    assert_eq!(c["ghosts"]["drawn"], 1);

    // Pixels: the three tints are in the picture, and the unchanged lines are faint.
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).unwrap();
    let size = tree.size();
    let mut pm = resvg::tiny_skia::Pixmap::new(size.width() as u32, size.height() as u32).unwrap();
    pm.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pm.as_mut(),
    );
    for status in [Status::Added, Status::Changed, Status::Removed] {
        let d = nearest(pm.data(), tint(status));
        assert!(d < 45.0, "{status:?} tint is missing: {d}");
    }

    // The color blind palette and the dark theme change the tints.
    let r = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "svg", "palette": "color_blind", "dark": true, "unchanged": "hide"}}),
    );
    let svg = r["image"]["svg"].as_str().unwrap();
    assert!(svg.contains("rgba(86,180,233,1.000)"));
    assert!(!svg.contains("data-compare=\"unchanged\""));
    assert_eq!(r["compare"]["style"]["palette"], "color_blind");
    let _ = Palette::ColorBlind;
}

#[test]
fn the_png_overlay_tints_added_changed_and_removed_objects() {
    let dir = temp("kadakas-compare-png");
    let (base, new) = files(dir.path());
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "png", "width": 900, "height": 700, "view": "top"}}),
    );
    assert_eq!(r["image"]["format"], "png");
    assert_eq!(r["image"]["width"], 900);
    assert_eq!(r["image"]["height"], 700);
    let (w, h, rgba) = pixels(&decode(&r["image"]["png_base64"]));
    assert_eq!((w, h), (900, 700));
    for status in [Status::Added, Status::Changed, Status::Removed] {
        let d = nearest(&rgba, tint(status));
        assert!(d < 60.0, "{status:?} tint is missing: {d}");
    }
    // No clouds without the option.
    assert!(nearest(&rgba, CLOUD) > 60.0);
    assert!(r["image"].get("clouds").is_none());
    // The unchanged lines are dimmed toward the paper; with `show` they are the pen color.
    let shown = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "png", "unchanged": "show"}}),
    );
    let (_, _, rgba) = pixels(&decode(&shown["image"]["png_base64"]));
    assert!(nearest(&rgba, [0, 0, 0]) < 90.0);
    // The color blind palette has blue where the standard one has green.
    let cb = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "png", "palette": "color_blind"}}),
    );
    let (_, _, rgba) = pixels(&decode(&cb["image"]["png_base64"]));
    let blue = CompareStyle {
        palette: Palette::ColorBlind,
        ..CompareStyle::default()
    }
    .color(Status::Added);
    assert!(nearest(&rgba, blue) < 60.0);
}

#[test]
fn revision_clouds_surround_the_clusters_of_changes() {
    let dir = temp("kadakas-compare-clouds");
    let (base, new) = files(dir.path());
    let mut s = Session::with_sequential_ids();
    for format in ["png", "svg"] {
        let r = ok(
            &mut s,
            json!({"base": base, "new": new, "render": {"format": format, "revision_clouds": true}}),
        );
        let clouds = &r["image"]["clouds"];
        assert!(clouds["count"].as_u64().unwrap() >= 1, "{format}: {clouds}");
        assert_eq!(clouds["dropped"], 0);
        if format == "png" {
            let (_, _, rgba) = pixels(&decode(&r["image"]["png_base64"]));
            assert!(nearest(&rgba, CLOUD) < 60.0, "no cloud in the png");
            // The tints are still there.
            assert!(nearest(&rgba, tint(Status::Added)) < 60.0);
        } else {
            let svg = r["image"]["svg"].as_str().unwrap();
            assert!(svg.contains("rgba(150,40,190"), "no cloud in the svg");
        }
    }
    // The document is not changed by the clouds.
    assert!(s.editor.doc().objects.is_empty());
    // No changes, no clouds.
    let r = ok(
        &mut s,
        json!({"base": base, "new": base, "render": {"format": "png", "revision_clouds": true}}),
    );
    assert_eq!(r["image"]["clouds"]["count"], 0);
    let (_, _, rgba) = pixels(&decode(&r["image"]["png_base64"]));
    assert!(nearest(&rgba, CLOUD) > 60.0);
}

#[test]
fn the_pdf_holds_the_picture() {
    let dir = temp("kadakas-compare-pdf");
    let (base, new) = files(dir.path());
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "pdf", "width": 400, "height": 300, "revision_clouds": true}}),
    );
    assert_eq!(r["image"]["format"], "pdf");
    let pdf = decode(&r["image"]["pdf_base64"]);
    assert!(pdf.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("/Image") && text.contains("/Count 1"),
        "{text}"
    );
    assert!(pdf.ends_with(b"%%EOF\n") || String::from_utf8_lossy(&pdf).contains("%%EOF"));
    assert_eq!(r["image"]["width"], 400);
    assert_eq!(r["compare"]["counts"]["added"], 1);
}

#[test]
fn the_picture_size_follows_the_limits_of_the_session() {
    let dir = temp("kadakas-compare-size");
    let (base, new) = files(dir.path());
    let mut s = Session::with_sequential_ids();
    s.set_limits(kadakas_api::SessionLimits {
        render_pixels: 10_000,
        ..kadakas_api::SessionLimits::default()
    });
    let r = ok(
        &mut s,
        json!({"base": base, "new": new, "render": {"format": "png", "width": 2000, "height": 2000}}),
    );
    let (w, h) = (
        r["image"]["width"].as_u64().unwrap(),
        r["image"]["height"].as_u64().unwrap(),
    );
    assert!(w * h <= 10_000, "{w}x{h}");
}
