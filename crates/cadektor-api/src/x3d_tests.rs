//! `export.file` with gltf, glb and obj, and the `section_viewport.*` methods.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::{RpcError, Session};

pub(crate) fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cadektor-api-x3d-{}-{name}", std::process::id()))
}

pub(crate) fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

pub(crate) fn fail(s: &mut Session, method: &str, p: Value) -> RpcError {
    s.handle(method, p).unwrap_err()
}

pub(crate) fn slab(
    s: &mut Session,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    thickness: i64,
    top: i64,
) -> u64 {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], "material": "Stone", "thickness": thickness, "elevation": top}}}),
    )["created"][0]
        .as_u64()
        .unwrap()
}

fn plant(s: &mut Session, x: i64, y: i64) {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "plant", "at": [x, y], "species": "Acer", "spread": 3000, "height": 6000}}}),
    );
}

pub(crate) fn glb_json(bytes: &[u8]) -> Value {
    assert_eq!(&bytes[0..4], b"glTF");
    let jl = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + jl]).unwrap()
}

#[test]
fn export_writes_gltf_glb_and_obj() {
    let dir = tmp("formats");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    slab(&mut s, 0, 0, 4000, 3000, 150, 0);
    for i in 0..4 {
        plant(&mut s, 6000 + i * 4000, 1000);
    }
    let before = s.editor.doc().clone();
    // GLB by format name; instancing shares the plant mesh.
    let glb = dir.join("site.glb");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": glb, "format": "glb", "options": {"units": "m"}}),
    );
    assert_eq!(r["format"], "glb");
    assert_eq!(r["exported"], 5);
    assert_eq!(r["report"]["instances"], 3);
    let j = glb_json(&std::fs::read(&glb).unwrap());
    assert_eq!(j["asset"]["version"], "2.0");
    assert_eq!(j["nodes"].as_array().unwrap().len(), 5);
    // GLTF by extension, grouped by layer.
    let gltf = dir.join("site.gltf");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": gltf, "options": {"grouping": "layer"}}),
    );
    assert_eq!(r["report"]["nodes"], 1);
    let text: Value = serde_json::from_slice(&std::fs::read(&gltf).unwrap()).unwrap();
    assert!(
        text["buffers"][0]["uri"]
            .as_str()
            .unwrap()
            .starts_with("data:")
    );
    // OBJ writes the MTL next to it.
    let obj = dir.join("site.obj");
    let r = ok(
        &mut s,
        "export.file",
        json!({"path": obj, "options": {"units": "mm", "y_up": false}}),
    );
    assert_eq!(r["files"].as_array().unwrap().len(), 2);
    let text = std::fs::read_to_string(&obj).unwrap();
    assert!(text.contains("mtllib site.mtl") && text.contains("\nf "));
    assert!(
        std::fs::read_to_string(dir.join("site.mtl"))
            .unwrap()
            .contains("newmtl ")
    );
    // Exports never change the document.
    assert_eq!(&before, s.editor.doc());
    // Bad options and an empty document.
    assert_eq!(
        fail(
            &mut s,
            "export.file",
            json!({"path": dir.join("x.glb"), "options": {"bogus": 1}})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "export.file",
            json!({"path": dir.join("x.glb"), "options": {"units": "yards"}})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "export.file",
            json!({"path": dir.join("x.glb"), "options": {"instance_tolerance": 0}})
        )
        .code,
        -32602
    );
    let mut empty = Session::default();
    let e = fail(
        &mut empty,
        "export.file",
        json!({"path": dir.join("e.glb")}),
    );
    assert_eq!(e.code, 1);
    assert!(e.message.contains("no 3D objects"));
    assert!(!dir.join("e.glb").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_follows_the_path_policy() {
    let base = tmp("policy");
    let _ = std::fs::remove_dir_all(&base);
    let (inside, outside) = (base.join("inside"), base.join("outside"));
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let mut s = Session {
        paths: crate::PathPolicy::restricted([inside.clone()]),
        ..Session::default()
    };
    slab(&mut s, 0, 0, 4000, 3000, 150, 0);
    assert_eq!(
        fail(
            &mut s,
            "export.file",
            json!({"path": outside.join("a.glb")})
        )
        .code,
        1
    );
    assert_eq!(
        fail(
            &mut s,
            "export.file",
            json!({"path": inside.join("a.txt"), "format": "glb"})
        )
        .code,
        1
    );
    assert!(!outside.join("a.glb").exists() && !inside.join("a.txt").exists());
    let out = inside.join("a.obj");
    ok(&mut s, "export.file", json!({"path": out}));
    // A second export needs overwrite, and so does the MTL file.
    assert_eq!(fail(&mut s, "export.file", json!({"path": out})).code, 1);
    ok(
        &mut s,
        "export.file",
        json!({"path": out, "overwrite": true}),
    );
    std::fs::remove_file(&out).unwrap();
    assert_eq!(
        fail(&mut s, "export.file", json!({"path": out})).code,
        1,
        "the MTL exists"
    );
    // The trusted call (the file dialog of the app) ignores the policy.
    let free = outside.join("free.glb");
    s.export_file_trusted(json!({"path": free})).unwrap();
    assert!(free.exists());
    let _ = std::fs::remove_dir_all(&base);
}

fn sheet(s: &mut Session) -> u64 {
    ok(
        s,
        "sheet.create",
        json!({"name": "Sections", "paper": "a3"}),
    )["layer"]
        .as_u64()
        .unwrap()
}

#[test]
fn section_viewports_follow_the_model() {
    let mut s = Session::default();
    // A slab 4000 by 1000, 300 thick, top at 0; the line crosses it at y = 500.
    slab(&mut s, 0, 0, 4000, 1000, 300, 0);
    let line = ok(
        &mut s,
        "section_viewport.line_add",
        json!({"a": [-1000, 500], "b": [5000, 500], "name": "A"}),
    )["id"]
        .as_u64()
        .unwrap();
    // One undo step makes the line and its settings.
    let n = s.editor.doc().objects.len();
    assert!(s.editor.undo());
    assert_eq!(s.editor.doc().objects.len(), n - 1);
    assert!(s.editor.redo());
    let obj = ok(&mut s, "objects.get", json!({"id": line}));
    assert_eq!(obj["data"]["section_line"]["name"], "A");
    assert_eq!(obj["data"]["section_line"]["look"], "left");

    let got = ok(&mut s, "section_viewport.get", json!({"id": line}));
    assert_eq!(got["length_mm"], 6000.0);
    assert_eq!(got["cut_faces"].as_array().unwrap().len(), 1);
    assert!((got["cut_faces"][0]["area_m2"].as_f64().unwrap() - 4.0 * 0.3).abs() < 1e-6);
    let builds = s.section_cache.builds;
    ok(&mut s, "section_viewport.get", json!({"id": line}));
    assert_eq!(s.section_cache.builds, builds, "cached by revision");

    let sh = sheet(&mut s);
    let vp = ok(
        &mut s,
        "section_viewport.create",
        json!({"sheet": sh, "line": line, "scale": "fit", "min": [10, 10], "max": [400, 280]}),
    );
    let vid = vp["id"].as_u64().unwrap();
    assert_eq!(vp["view"], "section");
    assert_eq!(vp["section"]["line"], line);
    assert_eq!(vp["name"], "A");
    // The content is the cut: 4000 wide, 300 high. The center is in its middle: u = 3000, height -150.
    assert!(
        (vp["center"][0].as_f64().unwrap() - 3000.0).abs() < 1.0,
        "{}",
        vp["center"]
    );
    assert!((vp["center"][1].as_f64().unwrap() + 150.0).abs() < 1.0);
    // The viewport has the same drawing.
    let again = ok(
        &mut s,
        "section_viewport.get",
        json!({"id": vid, "geometry": true}),
    );
    assert!((again["cut_faces"][0]["area_m2"].as_f64().unwrap() - 1.2).abs() < 1e-6);
    assert_eq!(
        again["cut_faces"][0]["rings"][0].as_array().unwrap().len(),
        4
    );

    // The sheet draws it.
    let svg = ok(&mut s, "sheet.render", json!({"sheet": sh}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(svg.matches("<path").count() >= 2);

    // Move the slab's neighbor: a new revision gives a new drawing.
    slab(&mut s, 0, 600, 4000, 900, 600, 0);
    let third = ok(&mut s, "section_viewport.get", json!({"id": vid}));
    assert_eq!(
        third["cut_faces"].as_array().unwrap().len(),
        1,
        "the new slab is behind the line"
    );
    assert!(third["edge_count"].as_u64().unwrap() > 0);
    // Change the line: it follows.
    ok(
        &mut s,
        "section_viewport.line_set",
        json!({"id": line, "a": [-1000, 800], "b": [5000, 800]}),
    );
    let moved = ok(&mut s, "section_viewport.get", json!({"id": vid}));
    // At y = 800 the line cuts both slabs: 4 x 0.3 and 4 x 0.6 square metres.
    let total: f64 = moved["cut_faces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["area_m2"].as_f64().unwrap())
        .sum();
    assert_eq!(moved["cut_faces"].as_array().unwrap().len(), 2);
    assert!((total - 3.6).abs() < 1e-6, "{total}");
    // Look right: only the first slab is cut, and there is nothing beyond but its back half.
    ok(
        &mut s,
        "section_viewport.line_set",
        json!({"id": line, "look": "right", "depth": 100000}),
    );
    let right = ok(&mut s, "section_viewport.get", json!({"id": vid}));
    assert_eq!(right["look"], "right");
    assert_eq!(right["depth"], 100000.0);
    ok(
        &mut s,
        "section_viewport.line_set",
        json!({"id": line, "depth": null, "look": "left", "mode": "elevation"}),
    );
    let elev = ok(&mut s, "section_viewport.get", json!({"id": vid}));
    assert_eq!(elev["mode"], "elevation");
    assert!(elev["cut_faces"].as_array().unwrap().is_empty());
    assert!(elev["edge_count"].as_u64().unwrap() > 0);

    // Update the viewport.
    let u = ok(
        &mut s,
        "section_viewport.update",
        json!({"id": vid, "scale": 25, "name": "Wall section", "cut_weight": 0.7, "beyond_weight": 0.1, "show_frame": true}),
    );
    assert_eq!(u["scale"], 25.0);
    assert_eq!(u["name"], "Wall section");
    assert_eq!(u["section"]["cut_weight"], 0.7);
    let fit = ok(
        &mut s,
        "section_viewport.update",
        json!({"id": vid, "scale": "fit"}),
    );
    assert!(fit["scale"].as_f64().unwrap() >= 1.0);
    // The generic viewport methods keep working on it.
    let g = ok(&mut s, "viewport.get", json!({"id": vid}));
    assert_eq!(g["section"]["line"], line);
}

#[test]
fn terrain_sections_match_the_site_model() {
    let mut s = Session::default();
    let mut ids = Vec::new();
    for i in 0..11 {
        for j in 0..11 {
            let out = ok(
                &mut s,
                "execute",
                json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100 + (i % 3) * 40]}}}),
            );
            ids.push(out["created"][0].as_u64().unwrap());
        }
    }
    let site = ok(&mut s, "site.create", json!({"source_ids": ids}))["id"]
        .as_u64()
        .unwrap();
    let line = ok(
        &mut s,
        "section_viewport.line_add",
        json!({"a": [0, 5200], "b": [10000, 5200], "name": "T"}),
    )["id"]
        .as_u64()
        .unwrap();
    let got = ok(
        &mut s,
        "section_viewport.get",
        json!({"id": line, "geometry": true}),
    );
    let profiles = got["profiles"].as_array().unwrap();
    assert!(!profiles.is_empty());
    let mut checked = 0;
    for p in profiles {
        for pt in p["points"].as_array().unwrap() {
            let (u, z) = (pt[0].as_f64().unwrap(), pt[1].as_f64().unwrap());
            let e = ok(
                &mut s,
                "site.elevation",
                json!({"id": site, "x": u, "y": 5200}),
            );
            let ground = e["z"].as_f64().unwrap();
            // The profile follows one of the surfaces of the site model (existing or proposed).
            assert!(
                (ground - z).abs() < 0.5,
                "u {u}: profile {z}, site {ground}"
            );
            checked += 1;
        }
    }
    // The profile is simplified: it keeps the points where the ground changes slope.
    assert!(checked >= 8, "{checked} points");
    // It also equals the site section of the grading tools.
    let sec = ok(
        &mut s,
        "section.add",
        json!({"a": [0, 5200], "b": [10000, 5200]}),
    )["id"]
        .as_u64()
        .unwrap();
    let site_profile = ok(&mut s, "section.profile", json!({"id": sec}));
    let run = site_profile["existing"][0].as_array().unwrap();
    for pt in run {
        let (x, z) = (pt[0].as_f64().unwrap(), pt[1].as_f64().unwrap());
        let mine = profiles
            .iter()
            .flat_map(|p| p["points"].as_array().unwrap().iter())
            .map(|q| (q[0].as_f64().unwrap(), q[1].as_f64().unwrap()))
            .collect::<Vec<_>>();
        // Find the segment of my profile that holds x, and interpolate.
        let mut best = f64::INFINITY;
        for w in mine.windows(2) {
            if x >= w[0].0.min(w[1].0) - 1e-6
                && x <= w[0].0.max(w[1].0) + 1e-6
                && (w[1].0 - w[0].0).abs() > 1e-9
            {
                let t = (x - w[0].0) / (w[1].0 - w[0].0);
                best = best.min((w[0].1 + (w[1].1 - w[0].1) * t - z).abs());
            }
        }
        assert!(best < 0.5, "x {x}: site section {z}, difference {best}");
    }
}

#[test]
fn bad_section_requests_are_refused() {
    let mut s = Session::default();
    slab(&mut s, 0, 0, 4000, 1000, 300, 0);
    assert_eq!(
        fail(&mut s, "section_viewport.line_add", json!({"a": [0, 0]})).code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_add",
            json!({"a": [0, 0], "b": [0.5, 0]})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_add",
            json!({"a": [0, 0], "b": [1000, 0], "look": "up"})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_add",
            json!({"a": [0, 0], "b": [1000, 0], "depth": -5})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_add",
            json!({"a": [0, 0], "b": [1000, 0], "bogus": 1})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_add",
            json!({"a": [0, 0], "b": [1000, 0], "layer": "Nope"})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(&mut s, "section_viewport.line_add", json!(5)).code,
        -32602
    );
    let sh = sheet(&mut s);
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_add",
            json!({"a": [0, 0], "b": [1000, 0], "layer": sh})
        )
        .code,
        -32602
    );
    let line = ok(
        &mut s,
        "section_viewport.line_add",
        json!({"a": [0, 500], "b": [4000, 500]}),
    )["id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_set",
            json!({"id": line, "a": [0, 500], "b": [0, 500]})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_set",
            json!({"id": 9999, "name": "x"})
        )
        .code,
        1
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_set",
            json!({"id": line, "look": "diagonal"})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(&mut s, "section_viewport.line_set", json!({})).code,
        -32602
    );
    assert_eq!(
        fail(&mut s, "section_viewport.get", json!({"id": 9999})).code,
        1
    );
    assert_eq!(fail(&mut s, "section_viewport.get", json!({})).code, -32602);
    // A plain object is not a section line.
    let slab_id = s.editor.doc().objects.keys().next().unwrap().0;
    assert_eq!(
        fail(&mut s, "section_viewport.get", json!({"id": slab_id})).code,
        1
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.line_set",
            json!({"id": slab_id, "name": "x"})
        )
        .code,
        1
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.create",
            json!({"sheet": sh, "line": slab_id})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.create",
            json!({"sheet": 1, "line": line})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(&mut s, "section_viewport.create", json!({"sheet": sh})).code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.create",
            json!({"sheet": sh, "line": line, "min": [0, 0]})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.create",
            json!({"sheet": sh, "line": line, "scale": "big"})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.create",
            json!({"sheet": sh, "line": line, "cut_weight": -1})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.create",
            json!({"sheet": sh, "line": line, "scale": 0})
        )
        .code,
        -32602
    );
    let vid = ok(
        &mut s,
        "section_viewport.create",
        json!({"sheet": sh, "line": line, "at": [10, 10], "size": [200, 100], "scale": 50, "center": [1000, 0], "layers": ["Layer 1"], "show_frame": true}),
    );
    assert_eq!(vid["scale"], 50.0);
    assert_eq!(vid["center"][0], 1000.0);
    let vid = vid["id"].as_u64().unwrap();
    assert_eq!(
        fail(&mut s, "section_viewport.update", json!({"id": slab_id})).code,
        1
    );
    assert_eq!(
        fail(&mut s, "section_viewport.update", json!({"id": line})).code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.update",
            json!({"id": vid, "line": slab_id})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.update",
            json!({"id": vid, "scale": "tiny"})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.update",
            json!({"id": vid, "beyond_weight": "x"})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section_viewport.update",
            json!({"id": vid, "min": [500, 500]})
        )
        .code,
        -32602
    );
    let other = ok(
        &mut s,
        "section_viewport.line_add",
        json!({"a": [0, 300], "b": [4000, 300]}),
    )["id"]
        .as_u64()
        .unwrap();
    let u = ok(
        &mut s,
        "section_viewport.update",
        json!({"id": vid, "line": other, "fit": true}),
    );
    assert_eq!(u["section"]["line"], other);
    // A plain viewport is not a section viewport.
    let plain = ok(&mut s, "viewport.create", json!({"sheet": sh}))["id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        fail(&mut s, "section_viewport.get", json!({"id": plain})).code,
        1
    );
    assert_eq!(
        fail(&mut s, "section_viewport.update", json!({"id": plain})).code,
        1
    );
    // A line deleted under a viewport: the viewport reports it, the sheet still renders.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [other]}}),
    );
    assert_eq!(
        fail(&mut s, "section_viewport.get", json!({"id": vid})).code,
        1
    );
    ok(&mut s, "sheet.render", json!({"sheet": sh}));
    // The methods are in the list, and describe has the examples.
    let d = ok(&mut s, "describe", json!({}));
    for m in [
        "section_viewport.line_add",
        "section_viewport.create",
        "section_viewport.get",
    ] {
        assert!(
            d["methods"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["name"] == m),
            "{m}"
        );
    }
    assert!(
        d["io"]["section_viewport"]["examples"]
            .as_array()
            .unwrap()
            .len()
            >= 3
    );
    assert!(d["io"]["x3d"]["examples"][0]["params"]["format"] == "glb");
    assert_eq!(
        fail(&mut s, "section_viewport.nope", json!({})).code,
        -32601
    );
}

#[test]
fn plants_and_objects_beyond_show_in_a_section() {
    let mut s = Session::default();
    plant(&mut s, 2000, 500);
    plant(&mut s, 2000, 6000);
    slab(&mut s, 0, 3000, 4000, 4000, 300, 500);
    let line = ok(
        &mut s,
        "section_viewport.line_add",
        json!({"a": [0, 500], "b": [4000, 500]}),
    )["id"]
        .as_u64()
        .unwrap();
    let got = ok(&mut s, "section_viewport.get", json!({"id": line}));
    assert_eq!(got["silhouettes"].as_array().unwrap().len(), 1);
    assert!(got["cut_faces"].as_array().unwrap().is_empty());
    assert!(got["edge_count"].as_u64().unwrap() > 0);
    // The slab beyond the line starts at the first end of the line.
    assert_eq!(got["bounds"]["min"][0].as_f64().unwrap().round(), 0.0);
    // Without the objects beyond there are no edges.
    ok(
        &mut s,
        "section_viewport.line_set",
        json!({"id": line, "beyond": false}),
    );
    assert_eq!(
        ok(&mut s, "section_viewport.get", json!({"id": line}))["edge_count"],
        0
    );
}
