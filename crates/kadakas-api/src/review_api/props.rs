//! Property tests of the versions on rich documents: several layers, a real georeference, groups,
//! resources, pictures (an underlay and the picture of a plant style, which become blobs), a site
//! model with more points than one chunk, a plug-in object and a texture bed.
//!
//! The rules: a restore gives back the content of the version, its automatic version gives back
//! the work, one undo undoes the restore exactly, the diff of a version with itself is empty, and
//! every version reads as a document.

use proptest::prelude::*;
use serde_json::{Value, json};

use kadakas_core::model::Document;
use kadakas_review::restore::same_content;

use super::*;

const PLANTER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../kadakas-plugin/tests/fixtures/raised_planter.wasm"
);

#[derive(Clone, Debug)]
enum Act {
    AddRect { x: i32, layer: u8 },
    AddLayer { n: u8 },
    AddClass { n: u8 },
    Delete { i: u8 },
    Move { i: u8 },
    Group { i: u8, j: u8 },
    Ungroup { i: u8 },
    ToLayer { i: u8, l: u8 },
    Arrange { i: u8, how: u8 },
    Resource { n: u8 },
    Underlay { x: i32, v: u8 },
    PlantPicture { v: u8 },
    Site { n: u16 },
    EditSite { i: u8, k: u16 },
    Georef { e: u8 },
    Bed { x: i32 },
    Place { x: i32 },
    Params { i: u8, w: u16 },
    Data { i: u8, v: u8 },
    Version,
    Restore { pick: u8 },
}

fn act() -> impl Strategy<Value = Act> {
    prop_oneof![
        5 => (-500i32..500, any::<u8>()).prop_map(|(x, layer)| Act::AddRect { x, layer }),
        2 => (0u8..4).prop_map(|n| Act::AddLayer { n }),
        1 => (0u8..3).prop_map(|n| Act::AddClass { n }),
        2 => any::<u8>().prop_map(|i| Act::Delete { i }),
        2 => any::<u8>().prop_map(|i| Act::Move { i }),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(i, j)| Act::Group { i, j }),
        1 => any::<u8>().prop_map(|i| Act::Ungroup { i }),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(i, l)| Act::ToLayer { i, l }),
        2 => (any::<u8>(), 0u8..4).prop_map(|(i, how)| Act::Arrange { i, how }),
        1 => (0u8..2).prop_map(|n| Act::Resource { n }),
        1 => (-500i32..500, 0u8..3).prop_map(|(x, v)| Act::Underlay { x, v }),
        1 => (0u8..3).prop_map(|v| Act::PlantPicture { v }),
        1 => (1u16..600).prop_map(|n| Act::Site { n }),
        1 => (any::<u8>(), 0u16..600).prop_map(|(i, k)| Act::EditSite { i, k }),
        1 => (0u8..3).prop_map(|e| Act::Georef { e }),
        1 => (-500i32..500).prop_map(|x| Act::Bed { x }),
        1 => (-500i32..500).prop_map(|x| Act::Place { x }),
        1 => (any::<u8>(), 50u16..200).prop_map(|(i, w)| Act::Params { i, w }),
        1 => (any::<u8>(), 0u8..3).prop_map(|(i, v)| Act::Data { i, v }),
        3 => Just(Act::Version),
        3 => any::<u8>().prop_map(|pick| Act::Restore { pick }),
    ]
}

/// A PNG of noise, big enough to become a blob.
fn noisy_png_base64(side: u32, variant: u32) -> String {
    let mut seed = 0x1234_5678u32 ^ variant;
    let mut data = Vec::new();
    for _ in 0..side * side * 4 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        data.push((seed >> 24) as u8);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, side, side);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }
    kadakas_core::underlay::base64_encode(&out)
}

fn ids(s: &Session) -> Vec<String> {
    s.editor
        .doc()
        .objects
        .keys()
        .map(|k| k.0.to_string())
        .collect()
}

fn layers(s: &Session) -> Vec<String> {
    s.editor
        .doc()
        .layers
        .iter()
        .map(|l| l.id.0.to_string())
        .collect()
}

fn pick(v: &[String], i: u8) -> Option<String> {
    (!v.is_empty()).then(|| v[i as usize % v.len()].clone())
}

fn exec(s: &mut Session, cmd: Value) {
    let _ = s.handle("execute", json!({"command": cmd}));
}

/// Runs one action. `saved` holds the document at the time of each version.
fn run(s: &mut Session, saved: &mut Vec<(u64, Document)>, a: &Act, now: u64) -> Result<(), String> {
    match a {
        Act::AddRect { x, layer } => {
            let layer = pick(&layers(s), *layer);
            exec(
                s,
                json!({"op": "add_object", "layer": layer,
                    "shape": {"kind": "rect", "min": [x, 0], "max": [x + 100, 50]}}),
            );
        }
        Act::AddLayer { n } => exec(s, json!({"op": "add_layer", "name": format!("L{n}")})),
        Act::AddClass { n } => exec(s, json!({"op": "add_class", "name": format!("C{n}")})),
        Act::Delete { i } => {
            if let Some(id) = pick(&ids(s), *i) {
                exec(s, json!({"op": "delete_objects", "ids": [id]}));
            }
        }
        Act::Move { i } => {
            if let Some(id) = pick(&ids(s), *i) {
                exec(
                    s,
                    json!({"op": "move_objects", "ids": [id], "delta": [10, 5]}),
                );
            }
        }
        Act::Group { i, j } => {
            let all = ids(s);
            if let (Some(p), Some(q)) = (pick(&all, *i), pick(&all, *j)) {
                exec(s, json!({"op": "group", "ids": [p, q]}));
            }
        }
        Act::Ungroup { i } => {
            if let Some(id) = pick(&ids(s), *i) {
                exec(s, json!({"op": "ungroup", "ids": [id]}));
            }
        }
        Act::ToLayer { i, l } => {
            if let (Some(id), Some(layer)) = (pick(&ids(s), *i), pick(&layers(s), *l)) {
                exec(s, json!({"op": "modify_object", "id": id, "layer": layer}));
            }
        }
        Act::Arrange { i, how } => {
            if let Some(id) = pick(&ids(s), *i) {
                let how = ["front", "back", "forward", "backward"][usize::from(*how)];
                exec(s, json!({"op": "arrange", "ids": [id], "how": how}));
            }
        }
        Act::Resource { n } => exec(
            s,
            json!({"op": "add_resource", "name": format!("W{n}"), "body": {"type": "worksheet"}}),
        ),
        Act::Underlay { x, v } => exec(
            s,
            json!({"op": "add_object", "shape": {"kind": "underlay",
                "min": [x, 0], "max": [x + 6400, 6400], "width": 64, "height": 64,
                "png": noisy_png_base64(64, u32::from(*v)), "opacity": 1.0}}),
        ),
        Act::PlantPicture { v } => {
            if let Ok(style) = s.handle(
                "plant_style.create",
                json!({"name": format!("Plant {v}"), "category": "tree"}),
            ) {
                let _ = s.handle(
                    "plant_style.set_picture",
                    json!({"id": style["id"], "base64": noisy_png_base64(48, u32::from(*v))}),
                );
            }
        }
        Act::Site { n } => {
            let points: Vec<Value> = (0..*n)
                .map(|i| json!([f64::from(i) * 10.0, f64::from(i % 9) * 5.0, 0.0]))
                .collect();
            exec(
                s,
                json!({"op": "add_object", "shape": {"kind": "site_model", "points": points}}),
            );
        }
        Act::EditSite { i, k } => {
            let sites: Vec<(String, Vec<Value>)> = s
                .editor
                .doc()
                .objects
                .values()
                .filter_map(|o| match &o.shape {
                    kadakas_core::model::Shape::SiteModel { points, .. } => Some((
                        o.id.0.to_string(),
                        points.iter().map(|p| json!([p.x, p.y, p.z])).collect(),
                    )),
                    _ => None,
                })
                .collect();
            if !sites.is_empty() {
                let (id, mut points) = sites[usize::from(*i) % sites.len()].clone();
                if !points.is_empty() {
                    let at = usize::from(*k) % points.len();
                    points[at] = json!([1.0, 1.0, 77.0]);
                    exec(
                        s,
                        json!({"op": "modify_object", "id": id,
                            "shape": {"kind": "site_model", "points": points}}),
                    );
                }
            }
        }
        Act::Georef { e } => {
            let _ = s.handle(
                "gis.georef.set",
                json!({"crs": "EPSG:3067", "origin": [385_000 + i32::from(*e) * 100, 6_672_000]}),
            );
        }
        Act::Bed { x } => {
            let _ = s.handle(
                "bed.add",
                json!({"boundary": [[x, 0], [x + 400, 0], [x + 400, 300], [x, 300]]}),
            );
        }
        Act::Place { x } => {
            let _ = s.handle("plugins.install", json!({"path": PLANTER}));
            let _ = s.handle(
                "plugins.place",
                json!({"plugin": "org.kadakas.raised-planter", "min": [x, 0], "max": [x + 2000, 800]}),
            );
        }
        Act::Params { i, w } => {
            let planters: Vec<String> = s
                .editor
                .doc()
                .objects
                .values()
                .filter(|o| o.data.values().any(|v| v.get("plugin").is_some()))
                .map(|o| o.id.0.to_string())
                .collect();
            if let Some(id) = pick(&planters, *i) {
                let _ = s.handle(
                    "plugins.set_params",
                    json!({"id": id, "params": {"wall": f64::from(*w)}}),
                );
            }
        }
        Act::Data { i, v } => {
            if let Some(id) = pick(&ids(s), *i) {
                exec(
                    s,
                    json!({"op": "set_data", "id": id, "set": {format!("k{v}"): v}}),
                );
            }
        }
        Act::Version => {
            let r = s
                .handle("version.create", json!({"name": "v", "now_ms": now}))
                .map_err(|e| e.message)?;
            let id = r["version"]["id"].as_u64().ok_or("no id")?;
            // The version may push older ones out; the caps are the defaults here.
            saved.push((id, s.editor.doc().clone()));
        }
        Act::Restore { pick: p } => {
            let listed = s.handle("version.list", json!({})).map_err(|e| e.message)?;
            let live: Vec<u64> = listed["versions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v["id"].as_u64())
                .collect();
            let candidates: Vec<&(u64, Document)> =
                saved.iter().filter(|(id, _)| live.contains(id)).collect();
            if candidates.is_empty() {
                return Ok(());
            }
            let (id, want) = candidates[usize::from(*p) % candidates.len()].clone();
            let before = s.editor.doc().clone();
            let depth = s.editor.undo_depth();
            let r = s
                .handle("version.restore", json!({"id": id, "now_ms": now}))
                .map_err(|e| e.message)?;
            if !r["notes"].as_array().is_none_or(Vec::is_empty) {
                return Err(format!("the restore left things out: {}", r["notes"]));
            }
            if !same_content(s.editor.doc(), &want) {
                return Err(format!("version {id} was not restored"));
            }
            // The diff of the version with the drawing is empty.
            let d = s
                .handle("version.diff", json!({"from": id, "to": "current"}))
                .map_err(|e| e.message)?;
            if d["totals"] != json!({"added": 0, "removed": 0, "changed": 0}) {
                return Err(format!("the diff after the restore is {}", d["totals"]));
            }
            // One undo step takes it all back, the automatic version too.
            if s.editor.undo_depth() > depth + 1 {
                return Err("a restore is more than one undo step".into());
            }
            if s.editor.undo_depth() == depth + 1 {
                assert!(s.editor.undo());
                if s.editor.doc() != &before {
                    return Err("undo did not give back the document".into());
                }
                assert!(s.editor.redo());
                if !same_content(s.editor.doc(), &want) {
                    return Err("redo did not restore the version".into());
                }
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn restore_round_trips_on_rich_documents(acts in prop::collection::vec(act(), 4..30)) {
        let mut s = Session::with_sequential_ids();
        s.scripting.allow_agent_scripts = true;
        s.wasm_plugins.registry.set_limits(kadakas_plugin::Limits {
            max_millis: 60_000,
            ..kadakas_plugin::Limits::default()
        });
        let mut saved = Vec::new();
        let mut now = 1000;
        for a in &acts {
            now += 1;
            if let Err(e) = run(&mut s, &mut saved, a, now) {
                return Err(TestCaseError::fail(format!("{a:?}: {e}")));
            }
        }
        // Every version that is left reads as a document, and compares equal with itself.
        let listed = s.handle("version.list", json!({})).unwrap();
        for v in listed["versions"].as_array().unwrap() {
            let id = v["id"].as_u64().unwrap();
            let got = s
                .handle("version.get", json!({"id": id, "document": true}))
                .unwrap();
            prop_assert!(got.get("notes").is_none(), "{}", got["notes"]);
            prop_assert!(Document::from_value(got["document"].clone()).is_ok());
            let d = s
                .handle("version.diff", json!({"from": id, "to": id}))
                .unwrap();
            prop_assert_eq!(&d["totals"], &json!({"added": 0, "removed": 0, "changed": 0}));
        }
        // The file form of the document keeps the versions.
        let file = serde_json::to_value(s.editor.doc()).unwrap();
        let mut t = Session::with_sequential_ids();
        let loaded = t.handle("document.load", json!({"document": file})).unwrap();
        prop_assert!(loaded.get("notes").is_none(), "{loaded}");
        prop_assert_eq!(
            t.handle("version.list", json!({})).unwrap()["count"].clone(),
            listed["count"].clone()
        );
    }
}
