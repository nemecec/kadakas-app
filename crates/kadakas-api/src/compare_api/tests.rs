use base64::Engine;
use serde_json::{Value, json};

use super::*;

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> RpcError {
    s.handle(method, p)
        .expect_err(&format!("{method} should fail"))
}

fn rect(s: &mut Session, x: i32, y: i32, fill: [u8; 4]) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object",
            "shape": {"kind": "rect", "min": [x, y], "max": [x + 3000, y + 3000]},
            "attrs": {"fill": fill, "pen": [0, 0, 0, 255]}}}),
    )["created"][0]
        .clone()
}

fn save(s: &mut Session, name: &str) -> u64 {
    ok(s, "version.create", json!({"name": name, "now_ms": 1}))["version"]["id"]
        .as_u64()
        .unwrap()
}

const GRAY: [u8; 4] = [120, 120, 120, 255];

/// A removed rect at x 0, a changed one at x 5000, an unchanged one at x 15000 and an added one at
/// x 10000 (3 m squares). Version 1 is the drawing before the edits. Returns the session and the
/// IDs.
fn fixture() -> (Session, [Value; 4]) {
    let mut s = Session::with_sequential_ids();
    let removed = rect(&mut s, 0, 0, GRAY);
    let changed = rect(&mut s, 5000, 0, GRAY);
    let same = rect(&mut s, 15000, 0, GRAY);
    save(&mut s, "Before");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [removed]}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [changed], "delta": [0, 8000]}}),
    );
    let added = rect(&mut s, 10000, 0, GRAY);
    (s, [removed, changed, same, added])
}

fn compare(from: Value, to: Value) -> Value {
    json!({"from": from, "to": to})
}

/// The pixels of a PNG answer: width, height and RGBA.
fn pixels(r: &Value) -> (usize, usize, Vec<u8>) {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(r["png_base64"].as_str().unwrap())
        .unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width as usize, info.height as usize, buf)
}

/// The mean color and the mean column of the pixels that `pick` selects, or `None`.
fn mean_of(
    (w, _, rgba): &(usize, usize, Vec<u8>),
    pick: impl Fn([u8; 3]) -> bool,
) -> Option<([f64; 3], f64, usize)> {
    let (mut sum, mut col, mut n) = ([0f64; 3], 0f64, 0usize);
    for (i, p) in rgba.chunks(4).enumerate() {
        if pick([p[0], p[1], p[2]]) {
            for k in 0..3 {
                sum[k] += f64::from(p[k]);
            }
            col += (i % w) as f64;
            n += 1;
        }
    }
    (n > 0).then(|| {
        (
            [sum[0] / n as f64, sum[1] / n as f64, sum[2] / n as f64],
            col / n as f64,
            n,
        )
    })
}

/// `a - b` as a signed number.
fn d(a: u8, b: u8) -> i32 {
    i32::from(a) - i32::from(b)
}

fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
}

#[test]
fn the_compare_option_is_checked() {
    let spec = |v: Value| CompareSpec::parse(Some(&v));
    assert_eq!(CompareSpec::parse(None).unwrap(), None);
    assert_eq!(CompareSpec::parse(Some(&Value::Null)).unwrap(), None);
    let s = spec(compare(json!(1), json!("current"))).unwrap().unwrap();
    assert_eq!((s.from, s.to), (Side::Version(1), Side::Current));
    assert_eq!(s.mode, CompareMode::Overlay);
    assert!(!s.mode_given);
    assert_eq!(s.style, CompareStyle::default());
    let s = spec(
        json!({"from": "CURRENT", "to": 2, "mode": "side_by_side", "palette":
        "color_blind", "dark": true, "ghosts": false, "previous": false, "unchanged": "show"}),
    )
    .unwrap()
    .unwrap();
    assert!(s.mode_given && s.mode == CompareMode::SideBySide);
    assert_eq!(s.mode.name(), "side_by_side");
    assert_eq!(s.style.palette, Palette::ColorBlind);
    assert!(s.style.dark && !s.style.show_ghosts && !s.style.show_previous);
    assert_eq!(s.style.unchanged, UnchangedMode::Show);
    // hide_unchanged is the short form of unchanged: hide, and false takes back a hide.
    let hide = |extra: Value| {
        let mut v = compare(json!(1), json!(2));
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        spec(v).unwrap().unwrap().style.unchanged
    };
    assert_eq!(hide(json!({"hide_unchanged": true})), UnchangedMode::Hide);
    assert_eq!(hide(json!({"hide_unchanged": false})), UnchangedMode::Dim);
    assert_eq!(
        hide(json!({"unchanged": "hide", "hide_unchanged": false})),
        UnchangedMode::Dim
    );
    assert_eq!(
        hide(json!({"unchanged": "show", "hide_unchanged": false})),
        UnchangedMode::Show
    );
    for (bad, what) in [
        (json!("x"), "must be an object"),
        (json!({"from": 1}), "compare.to is missing"),
        (json!({"to": 1}), "compare.from is missing"),
        (json!({"from": 1, "to": "x"}), "number of a version"),
        (
            json!({"from": 1, "to": 2, "mode": "both"}),
            "overlay or side_by_side",
        ),
        (
            json!({"from": 1, "to": 2, "palette": "pink"}),
            "standard or color_blind",
        ),
        (
            json!({"from": 1, "to": 2, "unchanged": "x"}),
            "show, dim or hide",
        ),
        (json!({"from": 1, "to": 2, "dark": "yes"}), "true or false"),
        (
            json!({"from": 1, "to": 2, "colour": 1}),
            "unknown compare option 'colour'",
        ),
    ] {
        let e = spec(bad.clone()).unwrap_err();
        assert!(e.message.contains(what), "{bad}: {}", e.message);
        assert_eq!(e.code, -32602, "bad parameters");
    }
}

#[test]
fn render_svg_tints_added_removed_and_changed_and_the_pixels_agree() {
    let (mut s, [removed, changed, same, added]) = fixture();
    let r = ok(
        &mut s,
        "render.svg",
        json!({"width": 600, "compare": compare(json!(1), json!("current"))}),
    );
    let svg = r["svg"].as_str().unwrap();
    let counts = &r["compare"]["counts"];
    assert_eq!(
        (
            counts["added"].as_u64(),
            counts["removed"].as_u64(),
            counts["changed"].as_u64()
        ),
        (Some(1), Some(1), Some(1))
    );
    // Each status has its group; the unchanged object is dimmed; the removed one is a ghost.
    let group = |status: &str, id: &Value| {
        svg.contains(&format!(
            "<g data-compare=\"{status}\" data-id=\"{}\"",
            id.as_str().unwrap()
        ))
    };
    assert!(group("added", &added), "{svg}");
    assert!(group("changed", &changed));
    assert!(svg.contains("data-ghost=\"removed\""));
    assert!(svg.contains(&format!("data-id=\"{}\"", removed.as_str().unwrap())));
    assert!(svg.contains("<g data-compare=\"unchanged\" opacity=\"0.25\">"));
    assert!(svg.contains(same.as_str().unwrap()));
    // The old place of the changed object is a faint ghost too.
    assert!(svg.contains("data-ghost=\"previous\""));
    // Marks and a legend with the counts, so the colors are not the only sign.
    assert!(svg.contains("data-mark=\"added\">+</text>"));
    assert!(svg.contains("data-mark=\"removed\">-</text>"));
    assert!(svg.contains("data-mark=\"changed\">~</text>"));
    assert!(svg.contains("data-compare-legend") && svg.contains(">added 1</text>"));
    // The answer carries the legend with color, pattern and mark.
    let legend = r["compare"]["legend"].as_array().unwrap();
    assert_eq!(legend.len(), 3);
    assert_eq!(legend[0]["status"], "added");
    assert_eq!(legend[1]["pattern"], "dashed");
    assert_eq!(legend[2]["symbol"], "~");
    assert_eq!(r["compare"]["from"]["name"], "Before");
    assert_eq!(r["compare"]["to"]["current"], true);

    // Pixels: draw the SVG and look at the middle of each rectangle.
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).unwrap();
    let size = tree.size();
    let mut pm = resvg::tiny_skia::Pixmap::new(size.width() as u32, size.height() as u32).unwrap();
    pm.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pm.as_mut(),
    );
    let view_box = svg
        .split("viewBox=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .split(' ')
        .map(|v| v.parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    let at = |x: f64, y: f64| {
        let px = ((x - view_box[0]) / view_box[2] * f64::from(pm.width())) as u32;
        let py = ((-y - view_box[1]) / view_box[3] * f64::from(pm.height())) as u32;
        let p = pm.pixel(px, py).unwrap();
        [p.red(), p.green(), p.blue()]
    };
    // Added (x 100): green. Removed ghost (x 0): red. Changed (x 50, y 80 after the move):
    // orange. Unchanged (x 150): the gray, dimmed toward white.
    let green = at(11500.0, 1500.0);
    assert!(
        d(green[1], green[0]) > 40 && d(green[1], green[2]) > 20,
        "added {green:?}"
    );
    let red = at(1500.0, 1500.0);
    assert!(
        d(red[0], red[1]) > 15 && d(red[0], red[2]) > 15,
        "removed {red:?}"
    );
    let orange = at(6500.0, 9500.0);
    assert!(
        d(orange[0], orange[1]) > 25 && d(orange[1], orange[2]) > 25,
        "changed {orange:?}"
    );
    let dim = at(16500.0, 1500.0);
    assert!(
        dim[0].abs_diff(dim[1]) < 6 && dim[0] > 190,
        "unchanged is a faint gray {dim:?}"
    );
    // Where the changed object was before: only a faint trace, not a fill.
    let old_place = at(6500.0, 1500.0);
    assert!(old_place.iter().all(|v| *v > 235), "{old_place:?} {svg}");
}

#[test]
fn the_unchanged_objects_can_be_shown_or_hidden_and_a_palette_chosen() {
    let (mut s, [_, _, same, _]) = fixture();
    let mut svg = |extra: Value| {
        let mut c = compare(json!(1), json!("current"));
        for (k, x) in extra.as_object().unwrap() {
            c[k] = x.clone();
        }
        ok(&mut s, "render.svg", json!({"compare": c}))["svg"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let hidden = svg(json!({"hide_unchanged": true}));
    assert!(!hidden.contains(same.as_str().unwrap()));
    assert!(!hidden.contains("data-compare=\"unchanged\""));
    let shown = svg(json!({"unchanged": "show"}));
    assert!(shown.contains(same.as_str().unwrap()));
    assert!(!shown.contains("data-compare=\"unchanged\""));
    // The color blind palette uses blue for added objects, in the dark theme a lighter blue.
    assert!(svg(json!({"palette": "color_blind"})).contains("rgba(0,114,178,1.000)"));
    assert!(
        svg(json!({"palette": "color_blind", "dark": true})).contains("rgba(86,180,233,1.000)")
    );
    // Ghosts can be switched off.
    let none = svg(json!({"ghosts": false, "previous": false}));
    assert!(!none.contains("data-ghost"));
}

#[test]
fn a_comparison_of_two_versions_draws_the_newer_one() {
    let (mut s, _) = fixture();
    let two = save(&mut s, "After");
    // Version 1 against version 2: the second version has the edits, the drawing is the same.
    let r = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!(two))}),
    );
    assert_eq!(r["compare"]["counts"]["added"], 1);
    assert_eq!(r["compare"]["to"]["name"], "After");
    // Edit the drawing: the picture of the two versions does not change (it is built from the
    // versions, not from the open drawing).
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [900, 900], "radius": 20}}}),
    );
    let again = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!(two))}),
    );
    assert_eq!(r["svg"], again["svg"]);
    let live = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(two), json!("current"))}),
    );
    assert_eq!(live["compare"]["counts"]["added"], 1, "the circle");
    // The same version on both sides: no change, nothing to tint.
    let none = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!(1))}),
    );
    assert_eq!(none["compare"]["summary"]["total"], 0);
    assert!(!none["svg"].as_str().unwrap().contains("data-ghost"));
}

#[test]
fn render_png_tints_added_removed_and_changed_in_the_cpu_renderer() {
    let (mut s, _) = fixture();
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "projection": "ortho", "width": 600, "height": 200,
            "compare": compare(json!(1), json!("current"))}),
    );
    assert_eq!(
        (r["width"].as_u64(), r["height"].as_u64()),
        (Some(600), Some(200))
    );
    assert_eq!(r["compare"]["mode"], "overlay");
    assert_eq!(r["compare"]["counts"]["removed"], 1);
    let img = pixels(&r);
    // The colors after the flat shading of a top face (about 0.88 of the tint): added green,
    // changed orange, and the ghost of the removed object: red, mixed with the background.
    let green = mean_of(&img, |p| d(p[1], p[0]) > 60 && d(p[1], p[2]) > 40).unwrap();
    assert!(
        near(green.0, [0.0, 112.0, 42.0], 10.0),
        "added {:?}",
        green.0
    );
    let orange = mean_of(&img, |p| {
        p[0] > 150 && d(p[0], p[1]) > 50 && d(p[1], p[2]) > 50
    })
    .unwrap();
    assert!(
        near(orange.0, [180.0, 90.0, 0.0], 14.0),
        "changed {:?}",
        orange.0
    );
    let red = mean_of(&img, |p| {
        d(p[0], p[1]) > 30 && d(p[0], p[2]) > 30 && p[1].abs_diff(p[2]) < 25 && p[1] > 90
    })
    .unwrap();
    assert!(
        red.0[0] > 200.0 && red.0[1] > 90.0,
        "the ghost is see-through: {:?}",
        red.0
    );
    // From left to right: the ghost of the removed object (x 0), the changed one (x 50), the added one (x 100).
    assert!(
        red.1 < orange.1 && orange.1 < green.1,
        "{} {} {}",
        red.1,
        orange.1,
        green.1
    );
    // The unchanged object is faded to a light gray, with no hue.
    let faint = mean_of(&img, |p| {
        p[0].abs_diff(p[1]) < 4 && p[1].abs_diff(p[2]) < 6 && p[0] < 235 && p[0] > 160
    });
    assert!(
        faint.is_some_and(|f| f.1 > green.1),
        "an unchanged object at the right"
    );
    // The dark theme has a dark background.
    let dark = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 64, "height": 64,
            "compare": {"from": 1, "to": "current", "dark": true}}),
    );
    assert_eq!(pixels(&dark).2[..3], [30, 32, 36]);
}

#[test]
fn render_png_hides_unchanged_objects_and_the_ghosts() {
    let (mut s, _) = fixture();
    let count = |s: &mut Session, extra: Value| {
        let mut c = compare(json!(1), json!("current"));
        for (k, x) in extra.as_object().unwrap() {
            c[k] = x.clone();
        }
        let r = ok(
            s,
            "render.png",
            json!({"view": "top", "width": 400, "height": 100, "compare": c}),
        );
        let img = pixels(&r);
        img.2
            .chunks(4)
            .filter(|p| p[..3] != [246, 247, 249])
            .count()
    };
    let all = count(&mut s, json!({}));
    let hidden = count(&mut s, json!({"hide_unchanged": true}));
    let no_ghosts = count(&mut s, json!({"ghosts": false}));
    assert!(hidden < all, "{hidden} < {all}");
    assert!(no_ghosts < all, "{no_ghosts} < {all}");
}

#[test]
fn side_by_side_pictures_show_both_sides() {
    let (mut s, [removed, ..]) = fixture();
    let c = json!({"from": 1, "to": "current", "mode": "side_by_side"});
    let r = ok(&mut s, "render.svg", json!({"width": 800, "compare": c}));
    let svg = r["svg"].as_str().unwrap();
    assert!(svg.contains("data-compare-mode=\"side_by_side\""));
    assert_eq!(svg.matches("<svg ").count(), 3, "the page and two pictures");
    assert!(svg.contains(">Before</text>") && svg.contains(">current drawing</text>"));
    // The removed object is in the left picture only.
    assert_eq!(svg.matches(removed.as_str().unwrap()).count(), 1);
    assert!(
        !svg.contains("data-compare=\"added\""),
        "no tints side by side"
    );
    assert!(r["compare"].get("counts").is_none());
    // PNG: two pictures of the same size with a line between.
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 404, "height": 100, "compare": c}),
    );
    let (w, h, rgba) = pixels(&r);
    assert_eq!((w, h), (404, 100));
    let ink = |x0: usize, x1: usize| {
        (0..h)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|(x, y)| rgba[(y * w + x) * 4..][..3] != [246, 247, 249])
            .count()
    };
    assert!(ink(0, 200) > 0 && ink(202, 404) > 0);
    assert_eq!(rgba[(50 * w + 201) * 4..][..3], [160, 160, 160]);
    // The left picture has one more object than the right one: the removed rectangle.
    assert!(ink(0, 200) != ink(202, 404));
}

#[test]
fn the_cache_keeps_the_states_so_a_slider_replays_each_version_once() {
    let mut s = Session::with_sequential_ids();
    let mut ids = Vec::new();
    for i in 0..5 {
        rect(&mut s, i * 40, 0, GRAY);
        ids.push(save(&mut s, &format!("V{i}")));
    }
    // Step from the oldest pair to the newest: five states, four overlays.
    for w in ids.windows(2) {
        s.compare_overlay(Side::Version(w[0]), Side::Version(w[1]))
            .unwrap();
    }
    assert_eq!(s.compare.state_builds, 5);
    assert_eq!(
        s.compare.state_hits, 3,
        "the shared neighbour comes from the cache"
    );
    assert_eq!(s.compare.overlay_builds, 4);
    // Step back: every overlay is cached.
    for w in ids.windows(2).rev() {
        let o = s
            .compare_overlay(Side::Version(w[0]), Side::Version(w[1]))
            .unwrap();
        assert_eq!(o.counts().added, 1);
    }
    assert_eq!(s.compare.overlay_builds, 4);
    assert_eq!(s.compare.overlay_hits, 4);
    assert_eq!(s.compare.state_builds, 5);
    // The memory is counted, and dropped with the caches.
    assert!(s.compare.approx_bytes() > 0);
    assert!(s.estimated_bytes() >= s.compare.approx_bytes());
    s.drop_caches();
    assert_eq!(s.compare.approx_bytes(), 0);
    assert_eq!(s.compare.state_count(), 0);
}

#[test]
fn the_cache_is_bounded_and_follows_the_history() {
    let mut s = Session::with_sequential_ids();
    let mut ids = Vec::new();
    for i in 0..(STATE_CACHE_ENTRIES + OVERLAY_CACHE_ENTRIES + 3) as i32 {
        rect(&mut s, i * 40, 0, GRAY);
        ids.push(save(&mut s, &format!("V{i}")));
    }
    let first = ids[0];
    for id in &ids[1..] {
        s.compare_overlay(Side::Version(first), Side::Version(*id))
            .unwrap();
    }
    assert!(s.compare.state_count() <= STATE_CACHE_ENTRIES);
    assert!(s.compare.overlays.len() <= OVERLAY_CACHE_ENTRIES);
    // A new version changes the history: the cache does not give old answers for it.
    let before = s.compare.overlay_builds;
    let id = ids[1];
    s.compare_overlay(Side::Version(first), Side::Version(id))
        .unwrap();
    save(&mut s, "Later");
    s.compare_overlay(Side::Version(first), Side::Version(id))
        .unwrap();
    assert!(s.compare.overlay_builds > before);
    // The drawing: an edit makes a new state of the current side.
    let a = s
        .compare_overlay(Side::Version(first), Side::Current)
        .unwrap();
    rect(&mut s, 9000, 0, GRAY);
    let b = s
        .compare_overlay(Side::Version(first), Side::Current)
        .unwrap();
    assert_eq!(b.counts().added, a.counts().added + 1);
}

#[test]
fn unknown_versions_and_options_are_errors() {
    let (mut s, _) = fixture();
    let e = fail(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(99), json!("current"))}),
    );
    assert_eq!(e.kind, "not_found", "{}", e.message);
    let e = fail(
        &mut s,
        "render.png",
        json!({"render_style": "Realistic", "compare": compare(json!(1), json!("current"))}),
    );
    assert!(e.message.contains("render_style"), "{}", e.message);
    let e = fail(&mut s, "render.png", json!({"compare": {"from": 1}}));
    assert!(e.message.contains("compare.to"));
}

#[test]
fn a_session_without_versions_cannot_be_compared_with_a_version() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0, 0, GRAY);
    let e = fail(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!("current"))}),
    );
    assert_eq!(e.kind, "not_found");
    // Current with current is fine.
    let r = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!("current"), json!("current"))}),
    );
    assert_eq!(r["compare"]["summary"]["total"], 0);
}

#[test]
fn sheets_compare_side_by_side() {
    let (mut s, _) = fixture();
    ok(&mut s, "sheet.create", json!({"name": "S"}));
    ok(&mut s, "viewport.create", json!({"sheet": "S"}));
    // The sheet is part of the drawing, so version 1 (made before it) has no such layer: make
    // versions that have it.
    let before = save(&mut s, "With sheet");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "text", "at": [1500, 1500],
            "text": "LIVE-MARK", "height": 800}}}),
    );
    let c = json!({"from": before, "to": "current"});
    let r = ok(&mut s, "sheet.render", json!({"sheet": "S", "compare": c}));
    let svg = r["svg"].as_str().unwrap();
    assert!(svg.contains("With sheet") && svg.contains("current drawing"));
    assert_eq!(
        svg.matches("LIVE-MARK").count(),
        1,
        "only the right page has it"
    );
    assert!(r["width_mm"].as_f64().unwrap() > 400.0);
    let png = ok(
        &mut s,
        "sheet.render",
        json!({"sheet": "S", "format": "png", "dpi": 12, "compare": c}),
    );
    assert!(png["width"].as_u64().unwrap() > 100 && png["compare"]["mode"] == "side_by_side");
    // Overlay is not for sheets; side_by_side is allowed when it is named.
    let e = fail(
        &mut s,
        "sheet.render",
        json!({"sheet": "S", "compare": {"from": before, "to": "current", "mode": "overlay"}}),
    );
    assert!(e.message.contains("side by side"));
    ok(
        &mut s,
        "sheet.render",
        json!({"sheet": "S", "compare": {"from": before, "to": "current", "mode": "side_by_side"}}),
    );
    // A version made before the sheet existed has no such sheet: its side says so.
    let r = ok(
        &mut s,
        "sheet.render",
        json!({"sheet": "S", "compare": compare(json!(1), json!("current"))}),
    );
    let svg = r["svg"].as_str().unwrap();
    assert!(svg.contains("this sheet is not in this version"));
    assert_eq!(svg.matches("LIVE-MARK").count(), 1);
    let e = fail(
        &mut s,
        "sheet.render",
        json!({"sheet": "S", "format": "pdf", "compare": c}),
    );
    assert!(e.message.contains("svg, png"));
}

#[test]
fn ghosts_are_drawn_against_the_old_side_and_reported() {
    let mut s = Session::with_sequential_ids();
    let layer = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Fine", "scale": 50}}),
    )["created"][0]
        .clone();
    let gone = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "layer": layer,
            "shape": {"kind": "rect", "min": [0, 0], "max": [3000, 3000]}}}),
    )["created"][0]
        .clone();
    save(&mut s, "With layer");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [gone]}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_layer", "id": layer}}),
    );
    let r = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!("current"))}),
    );
    let svg = r["svg"].as_str().unwrap();
    // The layer of the old side gives the scale of the line: 0.25 mm * 1.5 * 50.
    let ghost = svg.split("data-ghost=\"removed\"").nth(1).unwrap();
    assert!(ghost.contains("stroke-width=\"18.75\""), "{ghost}");
    assert_eq!(
        r["compare"]["ghosts"],
        json!({"drawn": 1, "hidden": 0, "not_drawable": 0, "capped": 0})
    );
    assert_eq!(r["compare"]["summary"]["ghosts_kept"], 1);
    // The same in 3D: the ghost has geometry.
    let png = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 64, "height": 64,
            "compare": compare(json!(1), json!("current"))}),
    );
    assert_eq!(png["compare"]["ghosts"]["drawn"], 1);
    // A ghost on a hidden layer is reported as hidden, not as drawn.
    let (mut s, _) = fixture();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": "00000000-0000-0000-0000-000000000001",
            "visible": false}}),
    );
    let r = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!("current"))}),
    );
    assert_eq!(r["compare"]["ghosts"]["hidden"], 1);
    assert_eq!(r["compare"]["ghosts"]["drawn"], 0);
}

#[test]
fn a_dark_svg_has_a_dark_paper_and_legend_and_previous_ghosts_go_below() {
    let (mut s, _) = fixture();
    let light = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!("current"))}),
    )["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!light.contains("data-compare-paper"));
    let dark = ok(
        &mut s,
        "render.svg",
        json!({"compare": {"from": 1, "to": "current", "dark": true}}),
    )["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(dark.contains("data-compare-paper=\"dark\""));
    assert!(
        dark.contains("fill=\"rgb(230,230,230)\""),
        "a light text on the dark legend"
    );
    // The old place of the changed object comes before the live objects; the ghost of a removed
    // object after them. The id of the object is in data-ghost-of.
    let before = dark.find("data-ghost=\"previous\"").unwrap();
    let live = dark.find("data-compare=\"added\"").unwrap();
    let removed = dark.find("data-ghost=\"removed\"").unwrap();
    assert!(before < live && live < removed);
    assert!(dark.contains("data-ghost-of=\""));
}

#[test]
fn the_state_cache_keeps_to_a_quarter_of_a_memory_limit_and_two_built_documents() {
    let mut s = Session::with_sequential_ids();
    for i in 0..3 {
        rect(&mut s, i * 5000, 0, GRAY);
        save(&mut s, &format!("V{i}"));
    }
    s.limits.max_memory_bytes = 400;
    assert_eq!(s.state_cache_cap(), 100);
    s.compare_overlay(Side::Version(1), Side::Version(2))
        .unwrap();
    assert_eq!(
        s.compare.state_count(),
        0,
        "a state over the cap is not kept"
    );
    s.limits.max_memory_bytes = u64::MAX;
    assert_eq!(s.state_cache_cap(), STATE_CACHE_BYTES);
    // Built documents: the last two are kept.
    for id in [1, 2, 3, 1] {
        s.compare_doc(Side::Version(id)).unwrap();
    }
    assert_eq!(s.compare.built.len(), 2);
    assert_eq!(s.compare.doc_builds, 4, "version 1 was dropped when 3 came");
    // A copy of the open drawing for ghosts is made once for a revision.
    let o = s.compare_overlay(Side::Current, Side::Version(1)).unwrap();
    let a = s.compare_old_doc(&o, Side::Current).unwrap().unwrap();
    let b = s.compare_old_doc(&o, Side::Current).unwrap().unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    // No ghost, no old document.
    let none = s
        .compare_overlay(Side::Version(1), Side::Version(3))
        .unwrap();
    assert!(
        s.compare_old_doc(&none, Side::Version(1))
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_sheet_compare_checks_the_format_and_dpi_first_and_finds_the_sheet_on_either_side() {
    let (mut s, _) = fixture();
    let c = compare(json!(1), json!("current"));
    let e = fail(
        &mut s,
        "sheet.render",
        json!({"sheet": "Nope", "format": "pdf", "compare": c}),
    );
    assert!(e.message.contains("svg, png"), "{}", e.message);
    let e = fail(
        &mut s,
        "sheet.render",
        json!({"sheet": "Nope", "format": "png", "dpi": 5, "compare": c}),
    );
    assert!(e.message.contains("dpi"), "{}", e.message);
    // A sheet that has another name now: the name of the old side finds it.
    let layer = ok(&mut s, "sheet.create", json!({"name": "S"}))["layer"].clone();
    let kept = save(&mut s, "With sheet");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_layer", "id": layer, "name": "T"}}),
    );
    let r = ok(
        &mut s,
        "sheet.render",
        json!({"sheet": "S", "compare": {"from": kept, "to": "current"}}),
    );
    assert!(r["svg"].as_str().unwrap().contains("With sheet"));
    let e = fail(
        &mut s,
        "sheet.render",
        json!({"sheet": "Nobody", "compare": {"from": kept, "to": "current"}}),
    );
    assert!(e.message.contains("no layer"), "{}", e.message);
}

/// A site model from nine 3D loci (they move into it), and a north arrow.
fn site_and_arrow(s: &mut Session) -> (Value, Value) {
    let ids: Vec<Value> = (0..9)
        .map(|i| {
            ok(
                s,
                "execute",
                json!({"command": {"op": "add_object", "shape": {"kind": "locus3d",
                    "p": [(i % 3) * 4000, (i / 3) * 4000, i * 100]}}}),
            )["created"][0]
                .clone()
        })
        .collect();
    let site = ok(s, "site.create", json!({"source_ids": ids}))["id"].clone();
    let arrow = ok(s, "north_arrow.add", json!({"at": [20000, 0]}))["id"].clone();
    (site, arrow)
}

/// The text of the ghost group of `kind` for the object `id` (up to the next `</g>`).
fn ghost_group<'a>(svg: &'a str, kind: &str, id: &Value) -> &'a str {
    let head = format!("data-ghost=\"{kind}\"");
    let id = id.as_str().unwrap();
    for (i, _) in svg.match_indices(&head) {
        let end = svg[i..].find('>').map_or(svg.len(), |e| i + e);
        // The id is in the head of the group: `data-ghost-of` for an old place.
        let tag = &svg[i..end];
        if tag.contains(&format!("data-id=\"{id}\""))
            || tag.contains(&format!("data-ghost-of=\"{id}\""))
        {
            let rest = &svg[i..];
            return &rest[..rest.find("</g>").unwrap_or(rest.len())];
        }
    }
    panic!("no {kind} ghost of {id} in {svg}");
}

#[test]
fn removed_annotations_and_site_models_have_ghosts_in_the_svg() {
    let mut s = Session::with_sequential_ids();
    let (site, arrow) = site_and_arrow(&mut s);
    save(&mut s, "Before");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [site, arrow]}}),
    );
    let r = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!("current"))}),
    );
    let svg = r["svg"].as_str().unwrap();
    let a = ghost_group(svg, "removed", &arrow);
    assert!(a.contains("data-kind=\"annotation\""), "{a}");
    let m = ghost_group(svg, "removed", &site);
    assert!(m.contains("data-kind=\"site_model\""), "{m}");
    assert!(m.contains("data-kind=\"contour"), "{m}");
    assert_eq!(
        r["compare"]["ghosts"],
        json!({"drawn": 2, "hidden": 0, "not_drawable": 0, "capped": 0})
    );
}

#[test]
fn changed_annotations_and_site_models_have_an_old_place_in_the_svg() {
    let mut s = Session::with_sequential_ids();
    let (site, arrow) = site_and_arrow(&mut s);
    save(&mut s, "Before");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [site, arrow], "delta": [0, 30000]}}),
    );
    let r = ok(
        &mut s,
        "render.svg",
        json!({"compare": {"from": 1, "to": "current", "previous": true}}),
    );
    let svg = r["svg"].as_str().unwrap();
    assert!(
        r["compare"]["counts"]["changed"].as_u64().unwrap() >= 2,
        "{r}"
    );
    let a = ghost_group(svg, "previous", &arrow);
    assert!(a.contains("data-kind=\"annotation\""), "{a}");
    let m = ghost_group(svg, "previous", &site);
    assert!(m.contains("data-kind=\"contour"), "{m}");
}

#[test]
fn a_sheet_compare_shows_a_removed_annotation_and_site_model_on_the_old_page() {
    let mut s = Session::with_sequential_ids();
    let (site, arrow) = site_and_arrow(&mut s);
    ok(&mut s, "sheet.create", json!({"name": "S"}));
    ok(&mut s, "viewport.create", json!({"sheet": "S"}));
    let before = save(&mut s, "With both");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [site, arrow]}}),
    );
    let page = |s: &mut Session, from: u64| -> String {
        let r = ok(
            s,
            "sheet.render",
            json!({"sheet": "S", "compare": {"from": from, "to": "current"}}),
        );
        r["svg"].as_str().unwrap().to_string()
    };
    // Sheets compare side by side, so there are no ghosts: the page of the old side draws what
    // the current one has lost, the annotation and the site model among it.
    let with = page(&mut s, before);
    let current = save(&mut s, "Without both");
    let without = page(&mut s, current);
    assert!(
        with.len() > without.len() + 500,
        "the old page draws the annotation and the site model: {} against {}",
        with.len(),
        without.len()
    );
    // The old page draws more paths (contours, arrow) than the new one.
    assert!(
        with.matches("<path").count() > without.matches("<path").count(),
        "the old page has more paths"
    );
}

/// A legend of the rects (one row with their count) is drawn from the document that it is in. A
/// ghost of it is drawn against the old side, so it shows the count of the old side.
#[test]
fn a_legend_ghost_shows_the_rows_of_the_old_side() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0, 0, GRAY);
    rect(&mut s, 5000, 0, GRAY);
    let legend = ok(
        &mut s,
        "legend.add",
        json!({"at": [0, 20000], "criteria": "Kind = \"rect\"", "group_by": "Kind"}),
    )["id"]
        .clone();
    save(&mut s, "Two rects");
    // The legend is removed, and three more rects make the count of the live drawing 5.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [legend]}}),
    );
    for i in 0..3 {
        rect(&mut s, 10000 + i * 5000, 0, GRAY);
    }
    let legend2 = ok(
        &mut s,
        "legend.add",
        json!({"at": [0, 40000], "criteria": "Kind = \"rect\"", "group_by": "Kind"}),
    )["id"]
        .clone();
    let r = ok(
        &mut s,
        "render.svg",
        json!({"compare": compare(json!(1), json!("current"))}),
    );
    let svg = r["svg"].as_str().unwrap();
    let ghost = ghost_group(svg, "removed", &legend);
    assert!(ghost.contains("data-kind=\"annotation\""), "{ghost}");
    // The row of the ghost has the count of the old side (2), the live legend that of the new (5).
    assert!(
        ghost.contains(">rect</text>") && ghost.contains(">2</text>"),
        "{ghost}"
    );
    assert!(!ghost.contains(">5</text>"), "{ghost}");
    let live = text_of(svg, &legend2);
    assert!(live.contains(">5</text>"), "{live}");
}

/// The text of the live object `id` in the SVG: from its group to the next `</g>`.
fn text_of<'a>(svg: &'a str, id: &Value) -> &'a str {
    let key = format!("data-id=\"{}\"", id.as_str().unwrap());
    let from = svg.rfind(&key).expect("the live object is drawn");
    let rest = &svg[from..];
    &rest[..rest.find("</g>").unwrap_or(rest.len())]
}

#[test]
fn the_ghost_caches_follow_the_old_document_and_are_dropped_with_the_caches() {
    let caches = crate::GhostCaches::default();
    let (a, b) = (Arc::new(Document::default()), Arc::new(Document::default()));
    let ra = caches.for_doc(&a).2;
    assert!(ra > 0, "revision 0 would keep nothing");
    assert_eq!(
        caches.for_doc(&a).2,
        ra,
        "the same old document: the same caches"
    );
    let rb = caches.for_doc(&b).2;
    assert_ne!(rb, ra, "another old document: new caches");
    // A session keeps them across renders of one comparison, and drop_caches frees them.
    let (mut s, _) = fixture();
    for _ in 0..2 {
        ok(
            &mut s,
            "render.svg",
            json!({"compare": compare(json!(1), json!("current"))}),
        );
    }
    assert!(s.ghost_caches.held_bytes() > 0, "the old document is kept");
    s.drop_caches();
    assert_eq!(s.ghost_caches.held_bytes(), 0);
}
