//! STEP import (IOP-11) through the API: the report, the exact data that each mesh keeps,
//! the methods `mesh3d.exact_info` and `mesh3d.retessellate`, and the option checks.

use std::f64::consts::PI;
use std::path::PathBuf;

use kadakas_api::{RpcError, Session};
use kadakas_core::command::Command;
use kadakas_core::exact::State;
use kadakas_core::model::{Document, ObjectId, Shape};
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {:?}", e.message, e.details))
}

fn err(s: &mut Session, method: &str, params: Value) -> RpcError {
    match s.handle(method, params) {
        Ok(v) => panic!("{method} should fail but gave {v}"),
        Err(e) => e,
    }
}

fn data(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step")
        .join(name)
        .display()
        .to_string()
}

fn first_mesh(s: &Session) -> (ObjectId, kadakas_core::mesh3d::Mesh3d) {
    for (id, o) in s.editor.doc().objects.iter() {
        if let Shape::Mesh3d(m) = &o.shape {
            return (*id, (**m).clone());
        }
    }
    panic!("no mesh");
}

fn import(s: &mut Session, name: &str, options: Value) -> Value {
    ok(
        s,
        "import.file",
        json!({"path": data(name), "options": options}),
    )
}

#[test]
fn a_step_file_becomes_a_mesh_with_a_report_and_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "box.step", json!({}));
    assert_eq!(r["imported"], 1);
    let rep = &r["report"];
    assert_eq!(rep["format"], "step");
    assert_eq!(rep["unit"], "mm");
    assert_eq!(rep["triangles"], 12);
    assert_eq!(rep["bounds_mm"]["size"], json!([20.0, 30.0, 40.0]));
    assert_eq!(rep["objects"][0]["name"], "Box");
    assert_eq!(rep["objects"][0]["color"], json!([255, 0, 0, 255]));
    let st = &rep["step"];
    // Every entity is accounted for.
    assert_eq!(
        st["entities_read"].as_u64().unwrap(),
        st["entities_used"].as_u64().unwrap() + st["entities_skipped"].as_u64().unwrap()
    );
    assert!(
        st["used"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["entity"] == "ADVANCED_FACE" && u["count"] == 6)
    );
    assert!(
        st["tessellated"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["surface"] == "PLANE" && u["faces"] == 6)
    );
    assert_eq!(st["products"], 1);
    assert_eq!(st["length_unit_mm"], json!([1.0]));
    assert_eq!(rep["exact"]["kept_objects"], 1);
    assert!(rep["exact"]["approx_bytes"].as_u64().unwrap() > 500);
    // The exact data is on the object.
    let (id, m) = first_mesh(&s);
    assert_eq!(m.exact_state(), Some(State::Fresh));
    let info = ok(&mut s, "mesh3d.exact_info", json!({"id": id}));
    assert_eq!(info["state"], "fresh");
    assert_eq!(info["faces"], 6);
    assert_eq!(info["summary"], "Exact data kept: 6 faces (plane x 6)");
    assert_eq!(info["list"][0]["kind"], "plane");
    assert_eq!(info["list"][0]["color"], json!([255, 0, 0, 255]));
    // One undo step.
    ok(&mut s, "undo", json!({}));
    assert!(s.editor.doc().objects.is_empty());
}

#[test]
fn curves_assemblies_and_tessellated_files() {
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "curves.step", json!({}));
    assert_eq!(r["imported"], 3);
    assert_eq!(r["report"]["curves"], 3);
    let kinds: Vec<String> = s
        .editor
        .doc()
        .objects
        .values()
        .map(|o| o.shape.kind().to_string())
        .collect();
    assert!(kinds.iter().all(|k| k == "polyline3d"));
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "assembly.step", json!({}));
    assert_eq!(r["imported"], 2);
    assert_eq!(r["report"]["step"]["instances"], 2);
    assert_eq!(r["report"]["exact"]["kept_objects"], 2);
    // join makes one mesh and joins the exact data.
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "assembly.step", json!({"join": true}));
    assert_eq!(r["imported"], 1);
    let (id, m) = first_mesh(&s);
    assert_eq!(m.exact.as_ref().unwrap().faces.len(), 12);
    let info = ok(&mut s, "mesh3d.exact_info", json!({"id": id, "limit": 3}));
    assert_eq!(info["listed"], 3);
    // Triangles that were given have no exact data, and the report says why.
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "tessellated.step", json!({}));
    assert_eq!(r["report"]["exact"]["kept_objects"], 0);
    assert!(
        r["report"]["exact"]["without"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("given")
    );
    let (id, _) = first_mesh(&s);
    assert_eq!(
        err(&mut s, "mesh3d.exact_info", json!({"id": id})).reason,
        Some("no_exact_data")
    );
    // The option keep_exact off keeps the triangles only.
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "box.step", json!({"keep_exact": false}));
    assert_eq!(r["report"]["exact"]["enabled"], false);
    assert_eq!(r["report"]["exact"]["kept_objects"], 0);
    assert!(first_mesh(&s).1.exact.is_none());
    // A surface model has no payload: the report says so.
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "box.step", json!({"to_surface": true}));
    assert_eq!(r["report"]["exact"]["kept_objects"], 0);
    assert_eq!(r["report"]["exact"]["lost_to_surface"], 1);
    assert!(
        r["report"]["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("exact data is not kept"))
    );
}

#[test]
fn units_tolerance_and_option_checks() {
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "box_inch.step", json!({}));
    assert_eq!(r["report"]["bounds_mm"]["size"][0], 25.4);
    assert_eq!(r["report"]["step"]["unit_from_file"], true);
    let mut s = Session::with_sequential_ids();
    let r = import(&mut s, "box.step", json!({"unit": "cm"}));
    assert_eq!(
        r["report"]["bounds_mm"]["size"],
        json!([200.0, 300.0, 400.0])
    );
    assert_eq!(r["report"]["step"]["unit_from_file"], false);
    // A finer tolerance gives more triangles.
    let mut s = Session::with_sequential_ids();
    let coarse = import(&mut s, "cylinder.step", json!({"tolerance": 1.0}));
    let mut t = Session::with_sequential_ids();
    let fine = import(&mut t, "cylinder.step", json!({"tolerance": 0.01}));
    assert!(fine["report"]["triangles"].as_u64() > coarse["report"]["triangles"].as_u64());
    assert_eq!(fine["report"]["step"]["tolerance_mm"], 0.01);
    // Checks.
    let bad = |opts: Value| {
        err(
            &mut Session::with_sequential_ids(),
            "import.file",
            json!({"path": data("box.step"), "options": opts}),
        )
    };
    assert!(bad(json!({"up_axis": "y"})).message.contains("up_axis"));
    assert!(bad(json!({"tolerance": 0.0})).message.contains("tolerance"));
    assert!(bad(json!({"angle": 0.5})).message.contains("angle"));
    assert!(bad(json!({"unit": "furlong"})).message.contains("unit"));
    let r = err(
        &mut Session::with_sequential_ids(),
        "import.file",
        json!({"path": data("make_step_files.py")}),
    );
    assert!(
        r.message.contains("format")
            || r.message.contains("extension")
            || r.message.contains("Unsupported")
            || !r.message.is_empty()
    );
}

#[test]
fn retessellate_makes_the_triangles_finer_in_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    import(&mut s, "cylinder.step", json!({"tolerance": 1.0}));
    let (id, m) = first_mesh(&s);
    let want = PI * 100.0 * 25.0;
    let coarse = m.volume();
    assert!((want - coarse).abs() / want > 1e-3, "{coarse}");
    let r = ok(
        &mut s,
        "mesh3d.retessellate",
        json!({"id": id, "tolerance": 0.005}),
    );
    assert_eq!(r["tolerance_mm"], 0.005);
    assert!(r["triangles"]["after"].as_u64() > r["triangles"]["before"].as_u64());
    assert!(
        r["faces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["surface"] == "CYLINDRICAL_SURFACE")
    );
    let (_, m2) = first_mesh(&s);
    let fine = m2.volume();
    assert!(
        (want - fine).abs() < (want - coarse).abs() / 5.0,
        "{coarse} {fine}"
    );
    assert_eq!(m2.exact_state(), Some(State::Fresh));
    assert_eq!(m2.name, m.name);
    assert!((m2.exact.as_ref().unwrap().tolerance_mm - 0.005).abs() < 1e-12);
    // Back to the coarse one with one undo.
    ok(&mut s, "undo", json!({}));
    assert!((first_mesh(&s).1.volume() - coarse).abs() < 1e-9);
    // Bad parameters.
    assert!(
        err(
            &mut s,
            "mesh3d.retessellate",
            json!({"id": id, "tolerance": 0.0})
        )
        .message
        .contains("tolerance")
    );
    assert!(
        err(
            &mut s,
            "mesh3d.retessellate",
            json!({"id": id, "tolerance": 0.1, "angle": 200.0})
        )
        .message
        .contains("angle")
    );
    assert!(
        err(&mut s, "mesh3d.retessellate", json!({"id": id}))
            .message
            .contains("tolerance")
    );
    // An object that is no mesh.
    let c = s
        .editor
        .execute(&Command::AddObject {
            shape: Shape::Circle {
                center: glam::DVec2::ZERO,
                radius: 100.0,
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
    let circle = ObjectId(c.created[0]);
    assert_eq!(
        err(&mut s, "mesh3d.exact_info", json!({"id": circle})).reason,
        Some("not_a_mesh")
    );
    assert_eq!(
        err(
            &mut s,
            "mesh3d.retessellate",
            json!({"id": circle, "tolerance": 1.0})
        )
        .reason,
        Some("not_a_mesh")
    );
    let missing = "00000000-0000-0000-0000-0000000000ff";
    assert_eq!(
        err(&mut s, "mesh3d.exact_info", json!({"id": missing})).reason,
        Some("no_such_object")
    );
}

#[test]
fn a_move_and_a_turn_keep_the_exact_data_in_step() {
    let mut s = Session::with_sequential_ids();
    import(&mut s, "cylinder.step", json!({}));
    let (id, before) = first_mesh(&s);
    let c0 = before.exact.as_ref().unwrap().clone();
    s.editor
        .execute(&Command::MoveObjects {
            ids: vec![id],
            delta: glam::DVec2::new(1000.0, -500.0),
        })
        .unwrap();
    s.editor
        .execute(&Command::RotateObjects {
            ids: vec![id],
            center: glam::DVec2::new(50.0, 20.0),
            angle: 0.9,
        })
        .unwrap();
    let (_, after) = first_mesh(&s);
    assert_eq!(after.exact_state(), Some(State::Fresh));
    // The cylinder axis moved with the object: its point is where the triangles are.
    let info = ok(&mut s, "mesh3d.exact_info", json!({"id": id}));
    let cyl = info["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "cylinder")
        .unwrap();
    assert_eq!(cyl["radius"], 10.0);
    let origin: Vec<f64> = cyl["origin"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let c = glam::DVec2::new(50.0, 20.0);
    let want = glam::DVec2::from_angle(0.9).rotate(glam::DVec2::new(1000.0, -500.0) - c) + c;
    assert!(
        (origin[0] - want.x).abs() < 1e-6 && (origin[1] - want.y).abs() < 1e-6,
        "{origin:?} {want:?}"
    );
    // The triangles cut again from the payload fit the old ones.
    let (lo, hi) = after.bounds3();
    ok(
        &mut s,
        "mesh3d.retessellate",
        json!({"id": id, "tolerance": 0.05}),
    );
    let (_, again) = first_mesh(&s);
    let (lo2, hi2) = again.bounds3();
    assert!(
        (lo - lo2).length() < 0.2 && (hi - hi2).length() < 0.2,
        "{lo} {lo2}"
    );
    assert_eq!(c0.faces.len(), again.exact.as_ref().unwrap().faces.len());
}

#[test]
fn edited_triangles_make_the_payload_stale_and_it_is_never_used() {
    let mut s = Session::with_sequential_ids();
    import(&mut s, "box.step", json!({}));
    let (id, mut m) = first_mesh(&s);
    // A tool that edits the triangles does not know the payload.
    m.vertices[0].z += 3.0;
    s.editor
        .execute(&Command::ModifyObject {
            id,
            shape: Some(Shape::Mesh3d(Box::new(m))),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
    assert_eq!(first_mesh(&s).1.exact_state(), Some(State::Stale));
    let info = ok(&mut s, "mesh3d.exact_info", json!({"id": id}));
    assert_eq!(info["state"], "stale");
    assert!(info["note"].as_str().unwrap().contains("edited"));
    let e = err(
        &mut s,
        "mesh3d.retessellate",
        json!({"id": id, "tolerance": 0.1}),
    );
    assert_eq!(e.reason, Some("exact_stale"));
    // A move keeps it stale: the payload moves, the mismatch stays.
    s.editor
        .execute(&Command::MoveObjects {
            ids: vec![id],
            delta: glam::DVec2::new(5.0, 5.0),
        })
        .unwrap();
    assert_eq!(first_mesh(&s).1.exact_state(), Some(State::Stale));
}

#[test]
fn the_payload_survives_a_save_and_a_load() {
    let mut s = Session::with_sequential_ids();
    import(&mut s, "plate_hole.step", json!({}));
    let (_, m) = first_mesh(&s);
    let text = serde_json::to_string(s.editor.doc()).unwrap();
    assert!(text.contains("\"exact\""));
    let doc: Document = serde_json::from_str(&text).unwrap();
    let (_, back) = doc
        .objects
        .iter()
        .find_map(|(id, o)| match &o.shape {
            Shape::Mesh3d(m) => Some((*id, (**m).clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(back.exact, m.exact);
    assert_eq!(back.exact_state(), Some(State::Fresh));
    // A later version added a field inside the payload: it is written back unchanged.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    let objects = v["objects"].as_array_mut().or(None);
    let _ = objects;
    let mut found = false;
    fn walk(v: &mut Value, found: &mut bool) {
        match v {
            Value::Object(m) => {
                if m.contains_key("mesh_hash") {
                    m.insert("from_the_future".into(), json!({"x": 1}));
                    *found = true;
                }
                for x in m.values_mut() {
                    walk(x, found);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(x, found)),
            _ => {}
        }
    }
    walk(&mut v, &mut found);
    assert!(found);
    let doc: Document = serde_json::from_value(v).unwrap();
    let again = serde_json::to_string(&doc).unwrap();
    assert!(again.contains("from_the_future"));
}

/// The entities of a flat bicubic patch with n x n control points, from entity number `base`
/// on: the exact data is big.
fn patch_body(n: usize, base_id: usize, name: &str) -> String {
    let mut out = String::new();
    let mut id = base_id;
    let mut rows = Vec::new();
    for i in 0..n {
        let mut row = Vec::new();
        for j in 0..n {
            id += 1;
            out.push_str(&format!(
                "#{id}=CARTESIAN_POINT('',({},{},0.));\n",
                i as f64 * 0.1,
                j as f64 * 0.1
            ));
            row.push(format!("#{id}"));
        }
        rows.push(format!("({})", row.join(",")));
    }
    let grid = rows.join(",");
    let mult = format!("(4,{},4)", "1,".repeat(n - 4).trim_end_matches(','));
    let kn = format!(
        "(0.,{},{}.)",
        (1..n - 3)
            .map(|k| format!("{}.", k))
            .collect::<Vec<_>>()
            .join(","),
        n - 3
    );
    let surf = id + 1;
    out.push_str(&format!(
        "#{surf}=B_SPLINE_SURFACE_WITH_KNOTS('',3,3,({grid}),.UNSPECIFIED.,.F.,.F.,.F.,{mult},{mult},{kn},{kn},.UNSPECIFIED.);\n"
    ));
    // Four corners and lines.
    let m = (n - 1) as f64 * 0.1;
    let pts = [(0.0, 0.0), (m, 0.0), (m, m), (0.0, m)];
    let mut v = Vec::new();
    for (k, p) in pts.iter().enumerate() {
        let pid = surf + 1 + 2 * k;
        out.push_str(&format!(
            "#{pid}=CARTESIAN_POINT('',({},{},0.));\n#{}=VERTEX_POINT('',#{pid});\n",
            p.0,
            p.1,
            pid + 1
        ));
        v.push(pid + 1);
    }
    let base = surf + 9;
    out.push_str(&format!("#{base}=DIRECTION('',(1.,0.,0.));\n#{}=DIRECTION('',(0.,1.,0.));\n#{}=DIRECTION('',(-1.,0.,0.));\n#{}=DIRECTION('',(0.,-1.,0.));\n", base + 1, base + 2, base + 3));
    let mut oe = Vec::new();
    for k in 0..4 {
        let (a, b) = (pts[k], pts[(k + 1) % 4]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let vec_id = base + 4 + 4 * k;
        out.push_str(&format!(
            "#{vec_id}=VECTOR('',#{},{len});\n#{}=LINE('',#{},#{vec_id});\n#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
            base + k, vec_id + 1, surf + 1 + 2 * k, vec_id + 2, v[k], v[(k + 1) % 4], vec_id + 1, vec_id + 3, vec_id + 2
        ));
        oe.push(format!("#{}", vec_id + 3));
    }
    let l = base + 20;
    out.push_str(&format!(
        "#{l}=EDGE_LOOP('',({}));\n#{}=FACE_OUTER_BOUND('',#{l},.T.);\n#{}=ADVANCED_FACE('',(#{}),#{surf},.T.);\n#{}=OPEN_SHELL('',(#{}));\n#{}=SHELL_BASED_SURFACE_MODEL('{name}',(#{}));\n",
        oe.join(","), l + 1, l + 2, l + 1, l + 3, l + 2, l + 4, l + 3
    ));
    out
}

/// A whole file with one patch.
fn big_patch(n: usize) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
        patch_body(n, 0, "patch")
    )
}

#[test]
fn a_payload_over_the_cap_of_one_object_is_dropped_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    // 400 x 400 control points are about 10 MB of exact data: over the 8 MiB of one object.
    let big = dir.path().join("big.step");
    std::fs::write(&big, big_patch(400)).unwrap();
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": big.display().to_string()}),
    );
    assert_eq!(r["imported"], 1, "{r}");
    assert_eq!(r["report"]["exact"]["kept_objects"], 0);
    let why = r["report"]["exact"]["without"][0]["reason"]
        .as_str()
        .unwrap();
    assert!(why.contains("over the limit"), "{why}");
    assert!(first_mesh(&s).1.exact.is_none());
    // A small patch fits.
    let small = dir.path().join("small.step");
    std::fs::write(&small, big_patch(8)).unwrap();
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": small.display().to_string()}),
    );
    assert_eq!(r["report"]["exact"]["kept_objects"], 1);
}

#[test]
fn hostile_files_are_refused_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let run = |name: &str, text: String| -> RpcError {
        let p = dir.path().join(name);
        std::fs::write(&p, text).unwrap();
        err(
            &mut Session::with_sequential_ids(),
            "import.file",
            json!({"path": p.display().to_string()}),
        )
    };
    let head = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n";
    let e = run(
        "trunc.step",
        format!("{head}#1=CARTESIAN_POINT('',(0.,0.,0."),
    );
    assert!(e.reason.is_some(), "{e:?}");
    let e = run("none.step", "hello".into());
    assert_eq!(e.reason, Some("mesh_malformed"));
    let e = run("empty.step", format!("{head}ENDSEC;\nEND-ISO-10303-21;\n"));
    assert_eq!(e.reason, Some("mesh_empty"));
    let deep = format!(
        "{head}#1=A({}1{});\nENDSEC;\n",
        "(".repeat(50_000),
        ")".repeat(50_000)
    );
    let e = run("deep.step", deep);
    assert_eq!(e.reason, Some("mesh_malformed"));
    assert!(e.message.contains("nested"));
}

#[test]
fn join_over_the_cap_keeps_the_triangles_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    // Three patches of 250 x 250 control points: each fits the cap of an object (3.7 MB of
    // estimate), the three together do not.
    let mut text = String::from(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n",
    );
    for k in 0..3 {
        text.push_str(&patch_body(250, k * 100_000, &format!("patch{k}")));
    }
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    let p = dir.path().join("three.step");
    std::fs::write(&p, text).unwrap();
    let path = p.display().to_string();
    let mut s = Session::with_sequential_ids();
    let r = ok(&mut s, "import.file", json!({"path": path}));
    assert_eq!(r["imported"], 3, "{r}");
    assert_eq!(r["report"]["exact"]["kept_objects"], 3);
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "import.file",
        json!({"path": path, "options": {"join": true}}),
    );
    assert_eq!(r["imported"], 1, "the import is not refused: {r}");
    assert_eq!(r["report"]["exact"]["kept_objects"], 0);
    let why = r["report"]["exact"]["without"][0]["reason"]
        .as_str()
        .unwrap();
    assert!(
        why.contains("joined parts") && why.contains("only the triangles"),
        "{why}"
    );
    assert!(first_mesh(&s).1.exact.is_none());
}

#[test]
fn a_payload_of_a_later_version_is_kept_reported_and_never_stops_the_load() {
    let mut s = Session::with_sequential_ids();
    import(&mut s, "plate_hole.step", json!({}));
    let text = serde_json::to_string(s.editor.doc()).unwrap();
    // A later program wrote version 2, with a kind of surface that this one does not know.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    fn later(v: &mut Value) {
        match v {
            Value::Object(m) => {
                if m.contains_key("mesh_hash") {
                    m.insert("version".into(), json!(2));
                    m["faces"][0]["surface"] = json!({"kind": "helicoid", "pitch": 3});
                }
                m.values_mut().for_each(later);
            }
            Value::Array(a) => a.iter_mut().for_each(later),
            _ => {}
        }
    }
    later(&mut v);
    let doc: Document = serde_json::from_value(v).unwrap();
    let mut s = Session::with_sequential_ids();
    s.replace_document(doc);
    let (id, m) = first_mesh(&s);
    assert!(m.exact.is_none() && m.exact_unusable.is_some());
    let r = ok(&mut s, "mesh3d.exact_info", json!({"id": id.0}));
    assert_eq!(r["state"], "unusable");
    assert!(r["reason"].as_str().unwrap().contains("version 2"));
    let e = err(
        &mut s,
        "mesh3d.retessellate",
        json!({"id": id.0, "tolerance": 0.1}),
    );
    assert!(
        format!("{:?}", e.details).contains("exact_unusable"),
        "{:?}",
        e.details
    );
    // The document still writes it as it was read.
    let again = serde_json::to_string(s.editor.doc()).unwrap();
    assert!(again.contains("helicoid"));
}
