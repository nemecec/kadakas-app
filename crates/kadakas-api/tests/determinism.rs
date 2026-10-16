//! The output of the renderers depends on the content of the drawing, not on the IDs that its
//! objects happen to have (3D-13). Two sessions that run the same script get different object IDs
//! (a session with sequential IDs, a session with random ones). Their sheets and pictures must be
//! the same, byte for byte.

use kadakas_api::Session;
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
}

/// The same script in any session: plants of every form, existing trees, an area with a scatter
/// of plants, a building, and a sheet with a plan and two 3D viewports.
fn script(s: &mut Session) -> (String, Vec<u8>) {
    let forms = [
        "tree",
        "columnar",
        "conical",
        "spreading",
        "shrub_mound",
        "weeping",
        "palm",
        "columnar_conifer",
        "multi_stem",
        "hedge",
    ];
    let mut styles = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        let st = call(
            s,
            "plant_style.create",
            json!({"name": format!("P{i}"), "form": form, "spread": 4000, "height": 7000}),
        );
        styles.push(st["id"].clone());
        call(
            s,
            "plants.place",
            json!({"mode": "grid", "min": [i * 9000, 0], "max": [i * 9000 + 8000, 8000], "spacing": 4000, "style": st["id"]}),
        );
    }
    let area_style = call(
        s,
        "landscape_area_style.create",
        json!({"name": "Meadow", "mix": [{"plant": styles[4], "percent": 80, "spacing": 1500}, {"plant": styles[0], "percent": 20, "spacing": 3000}]}),
    );
    call(
        s,
        "landscape_area.add",
        json!({"boundary": [[0, 12000], [30000, 12000], [30000, 30000], [0, 30000]], "style": area_style["id"]}),
    );
    call(
        s,
        "tree.import_survey",
        json!({"text": "ID,E,N,Species,DBH (cm),Height (m),Spread (m)\nT1,40000,5000,Quercus robur,55,16,9\nT2,45000,12000,Tilia cordata,40,14,8\nT3,50000,3000,Betula pendula,30,12,6\n"}),
    );
    call(
        s,
        "massing.add",
        json!({"footprint": [[60000, 0], [75000, 0], [75000, 9000], [60000, 9000]], "height": 6000, "roof": "gable"}),
    );
    let sheet = call(s, "sheet.create", json!({"name": "S"}))["layer"].clone();
    call(
        s,
        "viewport.create",
        json!({"sheet": sheet, "min": [10, 10], "max": [200, 280], "scale": 500, "view": "plan"}),
    );
    for (min, max, view, mode) in [
        ([210, 10], [400, 140], "right_isometric", "hidden_line"),
        (
            [210, 150],
            [400, 280],
            "left_isometric",
            "dashed_hidden_line",
        ),
    ] {
        call(
            s,
            "viewport.create",
            json!({"sheet": sheet, "min": min, "max": max, "scale": "fit", "view": view, "mode": mode}),
        );
    }
    let svg = call(s, "sheet.render", json!({"sheet": sheet, "format": "svg"}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    let png = call(
        s,
        "render.png",
        json!({"view": "right_isometric", "width": 400, "height": 300, "mode": "shaded", "samples": 2}),
    );
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png["png_base64"].as_str().unwrap())
        .unwrap();
    (strip_ids(&svg), bytes)
}

/// Removes the IDs from a text: the drawing is the same, the IDs are not.
fn strip_ids(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let id = i + 36 <= b.len()
            && text.is_char_boundary(i)
            && text.is_char_boundary(i + 36)
            && text[i..i + 36].chars().enumerate().all(|(k, c)| {
                matches!(k, 8 | 13 | 18 | 23) == (c == '-') && (c == '-' || c.is_ascii_hexdigit())
            });
        if id {
            out.push_str("<id>");
            i += 36;
        } else {
            let c = text[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

#[test]
fn the_same_script_gives_the_same_sheet_and_picture_whatever_the_ids() {
    let (svg_a, png_a) = script(&mut Session::with_sequential_ids());
    let (svg_b, png_b) = script(&mut Session::default());
    let (svg_c, png_c) = script(&mut Session::default());
    assert!(svg_a.len() > 10_000, "the sheet has content");
    assert!(
        svg_a == svg_b,
        "sequential and random ids: the sheet differs"
    );
    assert!(svg_b == svg_c, "two random id sets: the sheet differs");
    assert!(png_a == png_b && png_b == png_c, "the picture differs");
}
