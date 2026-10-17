use super::*;
use crate::PathPolicy;
use base64::Engine;
use kadakas_core::uid::AsUid;

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn fail(s: &mut Session, method: &str, params: Value) -> String {
    s.handle(method, params).unwrap_err().message
}

fn slab(
    s: &mut Session,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    thickness: i64,
    fill: Value,
) -> kadakas_core::uid::Uid {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object",
            "shape": {"kind": "hardscape", "boundary": [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
                      "material": "m", "thickness": thickness, "elevation": thickness},
            "attrs": {"fill": fill}}}),
    )["created"][0]
        .as_uid()
        .unwrap()
}

fn plant(s: &mut Session) -> kadakas_core::uid::Uid {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object",
            "shape": {"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 4000, "height": 8000}}}),
    )["created"][0]
        .as_uid()
        .unwrap()
}

fn png_pixels(r: &Value) -> (u32, u32, Vec<u8>) {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(r["png_base64"].as_str().unwrap())
        .unwrap();
    let img = kadakas_render::texture::decode_png_bytes(&bytes).unwrap();
    (img.width, img.height, img.rgba)
}

fn lum(px: &[u8], w: u32, x: u32, y: u32) -> f32 {
    let i = ((y * w + x) * 4) as usize;
    0.3 * px[i] as f32 + 0.59 * px[i + 1] as f32 + 0.11 * px[i + 2] as f32
}

#[test]
fn material_crud_and_validation() {
    let mut s = Session::with_sequential_ids();
    let id = ok(
        &mut s,
        "material.create",
        json!({"name": "Teak", "base_color": [150, 100, 60], "texture": {"procedural": "timber", "seed": 7},
               "mapping": {"kind": "planar", "scale": [1200, 300], "rotation": 15, "offset": [10, 20]},
               "roughness": 0.6, "metallic": 0.1, "opacity": 0.9}),
    )["id"]
        .as_uid()
        .unwrap();
    let m = ok(&mut s, "material.get", json!({"id": "teak"}));
    assert_eq!(m["id"], id);
    assert_eq!(m["base_color"], json!([150, 100, 60, 255]));
    assert_eq!(
        m["texture"],
        json!({"source": "procedural", "kind": "timber", "seed": 7})
    );
    assert_eq!(m["mapping"]["kind"], "planar");
    assert_eq!(m["mapping"]["scale"], json!([1200.0, 300.0]));
    assert_eq!(m["mapping"]["rotation"], 15.0);
    assert!((m["roughness"].as_f64().unwrap() - 0.6).abs() < 1e-6);
    assert_eq!(
        ok(&mut s, "material.list", json!({}))["materials"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Update: renames, replaces some fields and keeps the others. A number scales both sides.
    let m = ok(
        &mut s,
        "material.update",
        json!({"id": id, "name": "Teak deck", "mapping": {"scale": 500}, "texture": "brick", "opacity": 1}),
    );
    assert_eq!(m["name"], "Teak deck");
    assert_eq!(m["mapping"]["scale"], json!([500.0, 500.0]));
    assert_eq!(m["mapping"]["rotation"], 15.0, "other mapping fields stay");
    assert_eq!(m["texture"]["kind"], "brick");
    ok(
        &mut s,
        "material.update",
        json!({"id": id, "texture": null}),
    );
    assert_eq!(
        ok(&mut s, "material.get", json!({"id": id}))["texture"],
        Value::Null
    );

    // Duplicate, name clash, delete.
    let copy = ok(&mut s, "material.duplicate", json!({"id": id}))["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "material.get", json!({"id": copy}))["name"],
        "Teak deck copy"
    );
    let copy2 = ok(&mut s, "material.duplicate", json!({"id": id}))["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "material.get", json!({"id": copy2}))["name"],
        "Teak deck copy 2"
    );
    assert!(
        fail(&mut s, "material.create", json!({"name": "teak DECK"})).contains("exists already")
    );
    ok(&mut s, "material.delete", json!({"id": copy}));
    assert!(fail(&mut s, "material.get", json!({"id": copy})).contains("no material"));

    // Undo brings the material back.
    assert!(s.editor.undo());
    assert!(s.editor.doc().resources.get(ResourceId(copy)).is_some());

    // Bad input is refused with a message.
    for (params, text) in [
        (json!({"name": "x", "roughness": 2}), "roughness"),
        (json!({"name": "x", "opacity": "no"}), "number"),
        (json!({"name": "x", "base_color": [1, 2]}), "base_color"),
        (json!({"name": "x", "base_color": [1, 2, 300]}), "0 to 255"),
        (
            json!({"name": "x", "color": [1, 2, 3]}),
            "unknown parameter",
        ),
        (json!({"name": "x", "texture": "marble"}), "unknown pattern"),
        (json!({"name": "x", "texture": 5}), "texture"),
        (
            json!({"name": "x", "texture": {"bogus": 1}}),
            "unknown texture key",
        ),
        (json!({"name": "x", "texture": {}}), "give procedural"),
        (
            json!({"name": "x", "texture": {"png_base64": "!!"}}),
            "base64",
        ),
        (
            json!({"name": "x", "mapping": {"kind": "sphere"}}),
            "mapping kind",
        ),
        (json!({"name": "x", "mapping": {"scale": 0}}), "scale"),
        (
            json!({"name": "x", "mapping": {"scale": [1, 2, 3]}}),
            "number",
        ),
        (json!({"name": "x", "mapping": {"offset": [1]}}), "offset"),
        (
            json!({"name": "x", "mapping": {"zoom": 2}}),
            "unknown mapping key",
        ),
        (json!({"name": "x", "mapping": 4}), "object"),
        (json!({}), "name"),
        (json!({"name": 5}), "string"),
    ] {
        let e = fail(&mut s, "material.create", params.clone());
        assert!(e.contains(text), "{params}: {e}");
    }
    assert!(fail(&mut s, "material.create", json!([1])).contains("object"));
    assert!(fail(&mut s, "material.get", json!({})).contains("missing"));
}

#[test]
fn default_library_is_added_once() {
    let mut s = Session::with_sequential_ids();
    let r = ok(&mut s, "material.add_defaults", json!({}));
    assert_eq!(r["added"].as_array().unwrap().len(), 7);
    let names: Vec<_> = r["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_string())
        .collect();
    for n in [
        "Grass",
        "Gravel",
        "Concrete pavers",
        "Bark mulch",
        "Water",
        "Brick",
        "Timber",
    ] {
        assert!(names.contains(&n.to_string()), "{n}");
    }
    // The ids are real.
    let first = r["added"][0]["id"].as_uid().unwrap();
    assert_eq!(
        ok(&mut s, "material.get", json!({"id": first}))["name"],
        "Grass"
    );
    let again = ok(&mut s, "material.add_defaults", json!({}));
    assert!(again["added"].as_array().unwrap().is_empty());
    assert_eq!(again["existing"].as_array().unwrap().len(), 7);
    // One undo step removes all seven.
    assert!(s.editor.undo());
    assert_eq!(
        ok(&mut s, "material.list", json!({}))["materials"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    // Water is see-through.
    ok(&mut s, "material.add_defaults", json!({}));
    assert!(
        ok(&mut s, "material.get", json!({"id": "water"}))["opacity"]
            .as_f64()
            .unwrap()
            < 0.9
    );
}

#[test]
fn textures_come_from_png_and_jpeg_images() {
    let mut s = Session::with_sequential_ids();
    let id = ok(&mut s, "material.create", json!({"name": "Photo"}))["id"]
        .as_uid()
        .unwrap();
    let png = kadakas_render::texture::encode_png(4, 2, &[200; 32]).unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    let t = ok(
        &mut s,
        "material.import_texture",
        json!({"id": id, "png_base64": b64}),
    );
    assert_eq!(
        (t["width"].as_u64(), t["height"].as_u64()),
        (Some(4), Some(2))
    );
    assert_eq!(t["source"], "image");
    // The image is stored in the document and the material keeps working after save and load.
    let text = serde_json::to_string(s.editor.doc()).unwrap();
    assert_eq!(&Document::from_json(&text).unwrap(), s.editor.doc());

    // JPEG, through the texture parameter, scaled down to max_side.
    let jpg = include_bytes!("../../../kadakas-render/testdata/red_blue.jpg");
    let jb64 = base64::engine::general_purpose::STANDARD.encode(jpg);
    let t = ok(
        &mut s,
        "material.import_texture",
        json!({"id": "Photo", "jpeg_base64": jb64, "max_side": 8}),
    );
    assert_eq!(
        (t["width"].as_u64(), t["height"].as_u64()),
        (Some(8), Some(8))
    );
    let t = ok(
        &mut s,
        "material.create",
        json!({"name": "Photo 2", "texture": {"jpeg_base64": jb64}}),
    );
    assert_eq!(
        ok(&mut s, "material.get", json!({"id": t["id"]}))["texture"]["width"],
        16
    );

    // From a file: a PNG on disk.
    let path = std::env::temp_dir().join(format!("kadakas_tex_{}.png", std::process::id()));
    std::fs::write(&path, &png).unwrap();
    let t = ok(
        &mut s,
        "material.import_texture",
        json!({"id": id, "path": path.to_str().unwrap()}),
    );
    assert_eq!(t["width"], 4);
    // A restricted session refuses files outside its folders and files of the wrong type.
    s.paths = PathPolicy::restricted([]);
    let e = fail(
        &mut s,
        "material.import_texture",
        json!({"id": id, "path": path.to_str().unwrap()}),
    );
    assert!(e.contains("outside"), "{e}");
    let e = fail(
        &mut s,
        "material.import_texture",
        json!({"id": id, "path": "/etc/hosts"}),
    );
    assert!(e.contains(".png"), "{e}");
    s.paths = PathPolicy::unrestricted();
    std::fs::remove_file(&path).unwrap();
    assert!(
        fail(
            &mut s,
            "material.import_texture",
            json!({"id": id, "path": path.to_str().unwrap()})
        )
        .contains("cannot read")
    );

    // Bad images.
    assert!(fail(&mut s, "material.import_texture", json!({"id": id})).contains("path"));
    let e = fail(
        &mut s,
        "material.import_texture",
        json!({"id": id, "png_base64": base64::engine::general_purpose::STANDARD.encode(b"GIF89a")}),
    );
    assert!(e.contains("PNG or JPEG"), "{e}");
    assert!(
        fail(
            &mut s,
            "material.import_texture",
            json!({"id": id, "png_base64": b64, "max_side": 0})
        )
        .contains("max_side")
    );
    assert!(
        fail(
            &mut s,
            "material.import_texture",
            json!({"id": id, "png_base64": b64, "max_side": 5000})
        )
        .contains("max_side")
    );
}

#[test]
fn assigning_materials_and_resolution_order_in_the_api() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "material.add_defaults", json!({}));
    let tree = plant(&mut s);
    let style = ok(
        &mut s,
        "plant_style.create",
        json!({"name": "Oak", "spread": 4000}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(
        &mut s,
        "plants.set_style",
        json!({"ids": [tree], "style": style}),
    );

    // Class, style part, object, object part.
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Grass", "class": "00000000-0000-0000-0000-000000000000"}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Bark mulch", "style": "Oak", "slot": "trunk"}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Brick", "object": tree, "slot": "canopy"}),
    );
    let slots = ok(&mut s, "material.slots", json!({}));
    assert_eq!(slots["slots"].as_array().unwrap().len(), 3);
    assert_eq!(slots["slots"][2]["material_name"], "Brick");
    assert_eq!(slots["slots"][1]["target"]["kind"], "style");
    assert_eq!(
        ok(&mut s, "material.get", json!({"id": "Brick"}))["used_by"],
        1
    );

    let doc = s.editor.doc();
    let ix = kadakas_core::present::SlotIndex::new(doc);
    let name_of = |id: Option<ResourceId>| {
        id.and_then(|i| doc.resources.get(i))
            .map(|r| r.name.clone())
    };
    let obj = doc.object(ObjectId(tree)).unwrap();
    assert_eq!(
        name_of(ix.resolve(obj, kadakas_core::present::PART_TRUNK)).as_deref(),
        Some("Bark mulch")
    );
    assert_eq!(
        name_of(ix.resolve(obj, kadakas_core::present::PART_CANOPY)).as_deref(),
        Some("Brick")
    );

    // Several objects in one undo step, and clearing.
    let a = slab(&mut s, 5000, 0, 6000, 1000, 100, json!([10, 10, 10, 255]));
    let b = slab(&mut s, 7000, 0, 8000, 1000, 100, json!([10, 10, 10, 255]));
    let r = ok(
        &mut s,
        "material.assign",
        json!({"material": "Timber", "objects": [a, b]}),
    );
    assert_eq!(r["assigned"], 2);
    assert!(s.editor.undo());
    assert_eq!(
        ok(&mut s, "material.slots", json!({}))["slots"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": null, "class": "00000000-0000-0000-0000-000000000000"}),
    );
    assert_eq!(
        ok(&mut s, "material.slots", json!({}))["slots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Deleting a material clears its slots.
    ok(&mut s, "material.delete", json!({"id": "Brick"}));
    assert_eq!(
        ok(&mut s, "material.slots", json!({}))["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Errors.
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"class": "00000000-0000-0000-0000-000000000000"})
        )
        .contains("missing 'material'")
    );
    assert!(
        fail(&mut s, "material.assign", json!({"material": "Grass"}))
            .contains("exactly one target")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "class": "00000000-0000-0000-0000-000000000000", "object": a})
        )
        .contains("exactly one")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "style": "Oak"})
        )
        .contains("slot")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "style": "Nothing", "slot": "x"})
        )
        .contains("no plant style")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Nothing", "class": "00000000-0000-0000-0000-000000000000"})
        )
        .contains("no material")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "object": "00000000-0000-0000-0000-00000000270f"})
        )
        .contains("does not exist")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "class": "00000000-0000-0000-0000-00000000004d"})
        )
        .contains("does not exist")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "object": "a"})
        )
        .contains("object ID")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "objects": 5})
        )
        .contains("array")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "objects": ["x"]})
        )
        .contains("list")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "class": "x"})
        )
        .contains("no class is named \"x\"")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "object": a, "slot": 4})
        )
        .contains("string")
    );
    assert!(
        fail(
            &mut s,
            "material.assign",
            json!({"material": "Grass", "object": a, "colour": 4})
        )
        .contains("unknown parameter")
    );
}

#[test]
fn render_styles_crud_and_names() {
    let mut s = Session::with_sequential_ids();
    let id = ok(
        &mut s,
        "render_style.create",
        json!({"name": "Afternoon", "mode": "shaded", "sun": true, "shadows": true, "background": [250, 250, 250], "line_weight": 0.35}),
    )["id"]
        .as_uid()
        .unwrap();
    let r = ok(&mut s, "render_style.get", json!({"id": "afternoon"}));
    assert_eq!(r["sun"], true);
    assert_eq!(r["shadows"], true);
    assert_eq!(r["edges"], true);
    assert_eq!(r["background"], json!([250, 250, 250, 255]));
    assert_eq!(r["active"], false);
    let r = ok(
        &mut s,
        "render_style.update",
        json!({"id": id, "name": "Plan", "mode": "hidden-line", "edges": false, "textures": false}),
    );
    assert_eq!(r["mode"], "hidden_line");
    assert_eq!(r["name"], "Plan");
    assert_eq!(r["textures"], false);
    let list = ok(&mut s, "render_style.list", json!({}));
    assert_eq!(list["styles"].as_array().unwrap().len(), 1);
    assert_eq!(list["builtin"].as_array().unwrap().len(), 5);
    assert_eq!(list["active"], Value::Null);
    assert_eq!(list["modes"].as_array().unwrap().len(), 4);

    ok(&mut s, "render_style.set_active", json!({"id": "Plan"}));
    assert_eq!(
        ok(&mut s, "render_style.get", json!({"id": id}))["active"],
        true
    );
    let copy = ok(
        &mut s,
        "render_style.duplicate",
        json!({"id": id, "name": "Plan B"}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(&mut s, "render_style.delete", json!({"id": id}));
    assert_eq!(
        ok(&mut s, "render_style.list", json!({}))["active"],
        Value::Null,
        "deleting clears the active style"
    );
    ok(&mut s, "render_style.set_active", json!({"id": copy}));
    ok(&mut s, "render_style.set_active", json!({"id": null}));

    for (params, text) in [
        (json!({"name": "x", "mode": "sketch"}), "one of shaded"),
        (json!({"name": "x", "edges": "yes"}), "boolean"),
        (json!({"name": "x", "line_weight": 99}), "line weight"),
        (json!({"name": "x", "background": "white"}), "background"),
        (json!({"name": "x", "fog": true}), "unknown parameter"),
        (json!({}), "name"),
    ] {
        assert!(
            fail(&mut s, "render_style.create", params.clone()).contains(text),
            "{params}"
        );
    }
    assert!(
        fail(
            &mut s,
            "render_style.set_active",
            json!({"id": "00000000-0000-0000-0000-000000067932"})
        )
        .contains("no render style")
    );
    assert!(fail(&mut s, "render_style.set_active", json!({})).contains("missing"));
    // A material is not a render style.
    let m = ok(&mut s, "material.create", json!({"name": "M"}))["id"]
        .as_uid()
        .unwrap();
    assert!(fail(&mut s, "render_style.get", json!({"id": m})).contains("no render style"));
    assert!(fail(&mut s, "material.get", json!({"id": copy})).contains("no material"));
}

#[test]
fn sun_set_and_get_give_published_positions() {
    let mut s = Session::with_sequential_ids();
    assert_eq!(ok(&mut s, "sun.get", json!({}))["sun"], Value::Null);
    let r = ok(
        &mut s,
        "sun.set",
        json!({"latitude": 60.17, "longitude": 24.94, "date": "2026-06-21", "time": "13:22", "tz_offset": 3}),
    );
    // Around solar noon in Helsinki at the June solstice: the sun is due south, 53 degrees up.
    let az = r["position"]["azimuth"].as_f64().unwrap();
    let el = r["position"]["elevation"].as_f64().unwrap();
    assert!((az - 180.0).abs() < 3.0, "{az}");
    assert!((el - 53.3).abs() < 0.3, "{el}");
    assert_eq!(r["sun"]["date"], "2026-06-21");
    assert_eq!(r["sun"]["time"], "13:22");
    assert_eq!(r["position"]["above_horizon"], true);
    assert_eq!(r["day"]["solar_noon"], "13:22");
    assert!(
        r["day"]["sunrise"].as_str().unwrap().starts_with("03:5"),
        "{}",
        r["day"]
    );
    // A change replaces only the fields it gives.
    let r = ok(&mut s, "sun.set", json!({"time": "23:30"}));
    assert_eq!(r["sun"]["latitude"], 60.17);
    assert_eq!(r["position"]["above_horizon"], false);
    // A longitude without a time zone picks the nominal zone.
    let r = ok(
        &mut s,
        "sun.set",
        json!({"longitude": -74.0, "latitude": 40.7}),
    );
    assert_eq!(r["sun"]["tz_offset"], -5.0);
    // The sun path.
    let g = ok(
        &mut s,
        "sun.get",
        json!({"times": ["06:00", "12:00", "18:00"]}),
    );
    let path = g["path"].as_array().unwrap();
    assert_eq!(path.len(), 3);
    assert!(
        path[0]["azimuth"].as_f64().unwrap() < 180.0
            && path[2]["azimuth"].as_f64().unwrap() > 180.0
    );
    assert!(path[1]["elevation"].as_f64().unwrap() > path[0]["elevation"].as_f64().unwrap());
    // Undo and off.
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "sun.get", json!({}))["sun"]["longitude"], 24.94);
    assert_eq!(
        ok(&mut s, "sun.set", json!({"off": true}))["sun"],
        Value::Null
    );
    assert_eq!(ok(&mut s, "sun.get", json!({}))["sun"], Value::Null);

    for (params, text) in [
        (json!({"latitude": 100}), "latitude"),
        (json!({"date": "2026-02-30"}), "date"),
        (json!({"date": "tomorrow"}), "date"),
        (json!({"date": 5}), "text"),
        (json!({"time": "25:00"}), "time"),
        (json!({"time": "noon"}), "time"),
        (json!({"time": "12:75"}), "time"),
        (json!({"tz_offset": 30}), "time zone"),
        (json!({"north_angle": "up"}), "number"),
        (json!({"colour": 1}), "unknown parameter"),
        (json!({"from_georef": true}), "georeference"),
        (json!({"off": 1}), "boolean"),
    ] {
        assert!(
            fail(&mut s, "sun.set", params.clone()).contains(text),
            "{params}"
        );
    }
    ok(&mut s, "sun.set", json!({"latitude": 50.0}));
    assert!(fail(&mut s, "sun.get", json!({"times": 5})).contains("list"));
    assert!(fail(&mut s, "sun.get", json!({"times": [5]})).contains("text"));
}

#[test]
fn sun_can_take_its_place_from_the_georeference() {
    let mut s = Session::with_sequential_ids();
    // ETRS-TM35FIN coordinates in Helsinki.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000.0, 6672000.0], "rotation": 0.5}}}),
    );
    let r = ok(
        &mut s,
        "sun.set",
        json!({"from_georef": true, "date": "2026-06-21", "time": "12:00"}),
    );
    let lat = r["sun"]["latitude"].as_f64().unwrap();
    let lon = r["sun"]["longitude"].as_f64().unwrap();
    assert!((60.0..60.4).contains(&lat), "{lat}");
    assert!((24.5..25.5).contains(&lon), "{lon}");
    assert_eq!(r["sun"]["from_georef"], true);
    assert_eq!(r["sun"]["tz_offset"], 2.0, "nominal zone of the longitude");
    // The rotation of the model plus the meridian convergence (about 1.8 degrees here).
    let north = r["sun"]["north_angle"].as_f64().unwrap();
    assert!((north - (0.5f64.to_degrees() + 1.8)).abs() < 0.1, "{north}");
    // Giving a latitude makes it manual again.
    let r = ok(&mut s, "sun.set", json!({"latitude": 55.0}));
    assert_eq!(r["sun"]["from_georef"], false);
    // A georeference without a coordinate system is not enough.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_georeference", "georef": {"crs": "", "origin": [0.0, 0.0]}}}),
    );
    assert!(fail(&mut s, "sun.set", json!({"from_georef": true})).contains("coordinate system"));
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_georeference", "georef": null}}),
    );
    assert!(fail(&mut s, "sun.set", json!({"from_georef": true})).contains("no georeference"));
}

#[test]
fn render_png_uses_materials_styles_the_sun_and_shadows() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "material.add_defaults", json!({}));
    slab(
        &mut s,
        -4000,
        -4000,
        4000,
        4000,
        10,
        json!([200, 200, 200, 255]),
    );
    let cube = slab(&mut s, 0, 0, 1000, 1000, 1000, json!([180, 180, 180, 255]));
    let _ = cube;
    // Without materials and styles the render is the plain one.
    let plain = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 160, "height": 160}),
    );
    assert!(plain.get("render_style").is_none());
    let (w, _, px) = png_pixels(&plain);
    let flat = lum(&px, w, 20, 100);
    assert!((lum(&px, w, 30, 100) - flat).abs() < 1.0);

    // A material on the class gives texture detail on the ground.
    ok(
        &mut s,
        "material.assign",
        json!({"material": "Gravel", "class": "00000000-0000-0000-0000-000000000000"}),
    );
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 160, "height": 160}),
    );
    let (_, _, px) = png_pixels(&r);
    let mut vals = Vec::new();
    for x in 10..40 {
        vals.push(lum(&px, w, x, 100));
    }
    let mean = vals.iter().sum::<f32>() / vals.len() as f32;
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / vals.len() as f32;
    assert!(var > 20.0, "gravel has texture: {var}");
    assert_eq!(r["render_style"]["mode"], "shaded");

    // The sun and shadows: with shadows on, the ground west of the cube is darker.
    ok(
        &mut s,
        "sun.set",
        json!({"latitude": 52.0, "longitude": 5.0, "tz_offset": 0, "date": "2026-03-20", "time": "15:00"}),
    );
    ok(
        &mut s,
        "material.assign",
        json!({"material": null, "class": "00000000-0000-0000-0000-000000000000"}),
    );
    let shadowed = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 160, "height": 160, "render_style": "Realistic"}),
    );
    assert_eq!(shadowed["render_style"]["shadows"], true);
    let (_, _, px_shadow) = png_pixels(&shadowed);
    ok(
        &mut s,
        "render_style.create",
        json!({"name": "Sunny", "sun": true, "shadows": false}),
    );
    let lit = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 160, "height": 160, "render_style": "Sunny"}),
    );
    let (_, _, px_lit) = png_pixels(&lit);
    let darker = (0..160u32)
        .flat_map(|y| (0..160u32).map(move |x| (x, y)))
        .filter(|(x, y)| lum(&px_shadow, 160, *x, *y) < lum(&px_lit, 160, *x, *y) - 15.0)
        .count();
    assert!(
        darker > 100,
        "a shadow falls on the ground: {darker} darker pixels"
    );
    // The sun in the afternoon is in the west: the shadow goes east of the cube (right of it in
    // the top view).
    let (mut left, mut right) = (0, 0);
    for y in 0..160u32 {
        for x in 0..160u32 {
            if lum(&px_shadow, 160, x, y) < lum(&px_lit, 160, x, y) - 15.0 {
                if x < 80 { left += 1 } else { right += 1 }
            }
        }
    }
    assert!(right > left, "shadow right {right} left {left}");

    // The active style applies without naming it; an explicit mode wins.
    let hl = ok(
        &mut s,
        "render_style.create",
        json!({"name": "Plan", "mode": "hidden_line", "background": [255, 255, 255]}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(&mut s, "render_style.set_active", json!({"id": hl}));
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "right_isometric", "width": 200, "height": 200}),
    );
    assert_eq!(r["mode"], "hidden_line");
    let (_, _, px) = png_pixels(&r);
    let white = px
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] == 255 && p[1] == 255)
        .count();
    assert!(white > 200 * 200 * 8 / 10, "{white}");
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "right_isometric", "width": 64, "height": 64, "mode": "wireframe"}),
    );
    assert_eq!(r["mode"], "wireframe");
    // Errors.
    assert!(
        fail(&mut s, "render.png", json!({"render_style": "Nothing"})).contains("no render style")
    );
    assert!(fail(&mut s, "render.png", json!({"mode": "sketch"})).contains("hidden_line"));
    // Samples give a smoother picture of the same size.
    let r = ok(
        &mut s,
        "render.png",
        json!({"view": "top", "width": 64, "height": 64, "render_style": "Shaded", "samples": 2}),
    );
    assert_eq!(r["width"], 64);
}

#[test]
fn hidden_line_output_has_nine_edges_for_a_cube() {
    let mut s = Session::with_sequential_ids();
    slab(&mut s, 0, 0, 1000, 1000, 1000, json!([180, 180, 180, 255]));
    let r = ok(
        &mut s,
        "render.hidden_line",
        json!({"view": "right_isometric", "width": 400, "height": 400}),
    );
    assert_eq!(r["visible_edges"], 9);
    assert!(r["svg"].as_str().unwrap().starts_with("<svg"));
    assert_eq!(r["svg"].as_str().unwrap().matches("<line").count(), 9);
    assert_eq!(r["segments"], Value::Null);
    let d = ok(
        &mut s,
        "render.hidden_line",
        json!({"view": "right_isometric", "dashed": true, "segments": true, "mesh_edges": false}),
    );
    let segs = d["segments"].as_array().unwrap();
    assert_eq!(segs.iter().filter(|g| g["hidden"] == true).count(), 3);
    assert_eq!(segs.iter().filter(|g| g["hidden"] == false).count(), 9);
    assert!(d["svg"].as_str().unwrap().contains("stroke-dasharray"));
    assert!(fail(&mut s, "render.hidden_line", json!({"colour": 1})).contains("unknown parameter"));
    assert!(
        fail(&mut s, "render.hidden_line", json!({"view": "sideways"})).contains("unknown view")
    );
}

#[test]
fn methods_are_documented() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    assert!(d["presentation"]["examples"].as_array().unwrap().len() >= 8);
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for m in KNOWN {
        assert!(names.contains(m), "{m} is in describe");
        assert!(knows(m));
    }
    assert!(!knows("material.paint"));
    assert!(handle(&mut s, "material.paint", json!({})).is_none());
    // The view list names all four modes.
    let v = ok(&mut s, "view.list", json!({}));
    assert_eq!(v["modes"].as_array().unwrap().len(), 4);
    // Every example method is real.
    for e in d["presentation"]["examples"].as_array().unwrap() {
        assert!(names.contains(&e["method"].as_str().unwrap()), "{e}");
    }
}

#[test]
fn summer_time_and_true_north_in_sun_set() {
    let mut s = Session::with_sequential_ids();
    let winter = ok(
        &mut s,
        "sun.set",
        json!({"latitude": 60.17, "longitude": 24.94, "date": "2026-06-21", "time": "12:22", "tz_offset": 2}),
    );
    let summer = ok(&mut s, "sun.set", json!({"dst": true, "time": "13:22"}));
    assert_eq!(summer["sun"]["dst"], true);
    assert_eq!(summer["sun"]["tz_offset"], 2.0, "the standard offset stays");
    let (a, b) = (
        winter["position"]["azimuth"].as_f64().unwrap(),
        summer["position"]["azimuth"].as_f64().unwrap(),
    );
    assert!((a - b).abs() < 1e-6, "{a} {b}");
    assert!(fail(&mut s, "sun.set", json!({"dst": "yes"})).contains("boolean"));
    // From the georeference the north angle includes the convergence of the CRS (about 1.8 degrees).
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [385000.0, 6672000.0]}}}),
    );
    let g = ok(&mut s, "sun.set", json!({"from_georef": true}));
    let north = g["sun"]["north_angle"].as_f64().unwrap();
    assert!((north - 1.8).abs() < 0.1, "{north}");
}
