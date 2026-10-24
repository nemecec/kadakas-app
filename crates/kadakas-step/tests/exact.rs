//! The exact payload of a STEP import: what it keeps, how the triangles are cut again from it,
//! and that moves, scales and mirrors keep it in step.

use std::f64::consts::PI;
use std::path::PathBuf;

use glam::{DAffine3, DMat3, DVec3};
use kadakas_core::exact::ExactSolid;
use kadakas_meshio::{Limits, MeshError};
use kadakas_step::{StepImport, StepOptions, read, retessellate};

fn load(name: &str, opts: &StepOptions) -> StepImport {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step")
        .join(name);
    read(&std::fs::read(path).unwrap(), opts).unwrap()
}

fn volume(positions: &[[f64; 3]], tris: &[[u32; 3]]) -> f64 {
    tris.iter()
        .map(|t| {
            let p = |i: u32| DVec3::from(positions[i as usize]);
            p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0
        })
        .sum()
}

fn area(positions: &[[f64; 3]], tris: &[[u32; 3]]) -> f64 {
    tris.iter()
        .map(|t| {
            let p = |i: u32| DVec3::from(positions[i as usize]);
            (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
        })
        .sum()
}

fn exact_of(imp: &StepImport) -> &ExactSolid {
    imp.exact[0]
        .as_ref()
        .expect("the part keeps its exact data")
}

#[test]
fn the_payload_has_the_true_sizes() {
    let imp = load("cylinder.step", &StepOptions::default());
    let e = exact_of(&imp);
    assert!(e.validate().is_ok());
    assert_eq!(e.counts()["cylinder"], 1);
    assert_eq!(e.counts()["plane"], 2);
    let info = e.info(10);
    let cyl = info["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "cylinder")
        .unwrap();
    assert_eq!(cyl["radius"], 10.0);
    assert!(e.summary().contains("cylinder x 1"));
    assert_eq!(imp.report.exact_kept, 1);
    assert!(imp.report.exact_bytes > 500);
    // The payload survives JSON.
    let text = serde_json::to_string(e).unwrap();
    assert_eq!(&serde_json::from_str::<ExactSolid>(&text).unwrap(), e);
    // Units: the inch box has the true sizes in millimeters.
    let inch = load("box_inch.step", &StepOptions::default());
    let b = exact_of(&inch);
    assert!((b.unit_mm - 25.4).abs() < 1e-12);
    let planes = b.info(10);
    assert_eq!(planes["by_kind"]["plane"], 6);
}

#[test]
fn finer_tolerance_is_closer_to_the_analytic_values() {
    let limits = Limits::default();
    // A cylinder: the volume of the inscribed polygon approaches pi r2 h from below.
    let imp = load(
        "cylinder.step",
        &StepOptions {
            tolerance_mm: Some(0.5),
            ..Default::default()
        },
    );
    let e = exact_of(&imp);
    let want = PI * 100.0 * 25.0;
    let coarse = retessellate(e, 0.5, 15.0, &limits).unwrap();
    let fine = retessellate(e, 0.002, 15.0, &limits).unwrap();
    let (vc, vf) = (
        volume(&coarse.positions, &coarse.triangles),
        volume(&fine.positions, &fine.triangles),
    );
    assert!(
        (want - vf).abs() < (want - vc).abs() / 4.0,
        "{vc} {vf} {want}"
    );
    assert!((want - vf).abs() / want < 5e-4);
    assert!(
        fine.triangles.len() > coarse.triangles.len(),
        "{} {}",
        fine.triangles.len(),
        coarse.triangles.len()
    );
    // A sphere (poles and a seam): the same.
    let imp = load(
        "sphere.step",
        &StepOptions {
            tolerance_mm: Some(0.5),
            ..Default::default()
        },
    );
    let e = exact_of(&imp);
    let want = 4.0 / 3.0 * PI * 12f64.powi(3);
    let coarse = retessellate(e, 0.5, 15.0, &limits).unwrap();
    let fine = retessellate(e, 0.01, 15.0, &limits).unwrap();
    let (vc, vf) = (
        volume(&coarse.positions, &coarse.triangles),
        volume(&fine.positions, &fine.triangles),
    );
    assert!(
        (want - vf).abs() < (want - vc).abs() / 4.0,
        "{vc} {vf} {want}"
    );
    let (ac, af) = (
        area(&coarse.positions, &coarse.triangles),
        area(&fine.positions, &fine.triangles),
    );
    let want_a = 4.0 * PI * 144.0;
    assert!((want_a - af).abs() < (want_a - ac).abs(), "{ac} {af}");
    assert!(fine.kinds.iter().any(|k| k.0 == "SPHERICAL_SURFACE"));
}

#[test]
fn a_uniform_scale_and_a_mirror_keep_the_payload_right() {
    let limits = Limits::default();
    let imp = load("cylinder.step", &StepOptions::default());
    let e = exact_of(&imp);
    let scaled = e
        .transformed(&DAffine3 {
            matrix3: DMat3::from_rotation_z(0.7) * 3.0,
            translation: DVec3::new(10.0, 20.0, 30.0),
        })
        .unwrap();
    let info = scaled.info(10);
    let cyl = info["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "cylinder")
        .unwrap();
    assert!((cyl["radius"].as_f64().unwrap() - 30.0).abs() < 1e-9);
    let r = retessellate(&scaled, 0.01, 15.0, &limits).unwrap();
    let want = PI * 100.0 * 25.0 * 27.0;
    let v = volume(&r.positions, &r.triangles);
    assert!(v > 0.0 && (want - v).abs() / want < 2e-3, "{v} {want}");
    // The cut is where the map put it.
    let (mut lo, mut hi) = (DVec3::splat(1e9), DVec3::splat(-1e9));
    for p in &r.positions {
        lo = lo.min(DVec3::from(*p));
        hi = hi.max(DVec3::from(*p));
    }
    assert!((hi.z - 105.0).abs() < 1e-6 && (lo.z - 30.0).abs() < 1e-6);
    // A mirror: a closed solid with a positive volume.
    let mirror = e
        .transformed(&DAffine3 {
            matrix3: DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0)),
            translation: DVec3::ZERO,
        })
        .unwrap();
    let r = retessellate(&mirror, 0.05, 15.0, &limits).unwrap();
    assert!(volume(&r.positions, &r.triangles) > 0.0);
    // A stretch cannot follow.
    assert!(
        e.transformed(&DAffine3 {
            matrix3: DMat3::from_diagonal(DVec3::new(2.0, 1.0, 1.0)),
            translation: DVec3::ZERO,
        })
        .is_err()
    );
    // The same for a solid with a hole and a faceted solid.
    for f in [
        "plate_hole.step",
        "faceted.step",
        "torus.step",
        "revolved.step",
        "extrusion.step",
    ] {
        let imp = load(f, &StepOptions::default());
        let e = exact_of(&imp);
        let m = e
            .transformed(&DAffine3 {
                matrix3: DMat3::from_diagonal(DVec3::new(1.0, -1.0, 1.0)) * 2.0,
                translation: DVec3::new(1.0, 2.0, 3.0),
            })
            .unwrap();
        let r = retessellate(&m, 0.05, 15.0, &limits).unwrap();
        let orig = imp.set.parts[0].clone();
        let v0 = volume(&orig.positions, &orig.triangles);
        let v = volume(&r.positions, &r.triangles);
        assert!(
            v > 0.0 && (v - 8.0 * v0).abs() / (8.0 * v0) < 0.03,
            "{f}: {v} {v0}"
        );
    }
}

#[test]
fn the_payload_follows_the_placement_of_an_assembly() {
    let imp = load("assembly.step", &StepOptions::default());
    assert_eq!(imp.exact.len(), 2);
    for (part, e) in imp.set.parts.iter().zip(&imp.exact) {
        let e = e.as_ref().unwrap();
        let r = retessellate(e, 0.05, 15.0, &Limits::default()).unwrap();
        let bounds = |pts: &[[f64; 3]]| {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in pts {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            (lo, hi)
        };
        let (a, b) = (bounds(&part.positions), bounds(&r.positions));
        for k in 0..3 {
            assert!((a.0[k] - b.0[k]).abs() < 1e-9 && (a.1[k] - b.1[k]).abs() < 1e-9);
        }
    }
}

#[test]
fn what_has_no_payload_says_why() {
    let off = load(
        "box.step",
        &StepOptions {
            keep_exact: false,
            ..Default::default()
        },
    );
    assert!(off.exact.iter().all(Option::is_none));
    assert_eq!(off.report.exact_kept, 0);
    assert!(off.report.exact_dropped.is_empty());
    // Triangles that were given have no surfaces.
    let t = load("tessellated.step", &StepOptions::default());
    assert!(t.exact[0].is_none());
    assert_eq!(t.report.exact_dropped.len(), 1);
    assert!(t.report.exact_dropped[0].1.contains("given"));
    let json = t.report.to_json();
    assert_eq!(json["exact"]["kept_parts"], 0);
    assert_eq!(json["exact"]["without"][0]["part"], "Tessellated");
}

#[test]
fn retessellate_refuses_bad_input() {
    let imp = load("box.step", &StepOptions::default());
    let e = exact_of(&imp).clone();
    let limits = Limits::default();
    assert!(matches!(
        retessellate(&e, 0.0, 15.0, &limits),
        Err(MeshError::Malformed(_))
    ));
    assert!(matches!(
        retessellate(&e, 0.1, f64::NAN, &limits),
        Err(MeshError::Malformed(_))
    ));
    let mut bad = e.clone();
    bad.edges[0].start = 999;
    assert!(matches!(
        retessellate(&bad, 0.1, 15.0, &limits),
        Err(MeshError::Malformed(_))
    ));
    let small = Limits {
        max_triangles: 5,
        ..Limits::default()
    };
    assert!(matches!(
        retessellate(&e, 0.1, 15.0, &small),
        Err(MeshError::TooManyTriangles { .. })
    ));
    let few = Limits {
        max_vertices: 3,
        ..Limits::default()
    };
    assert!(matches!(
        retessellate(&e, 0.1, 15.0, &few),
        Err(MeshError::TooManyVertices { .. })
    ));
    let mem = Limits {
        max_memory: 10,
        ..Limits::default()
    };
    assert!(matches!(
        retessellate(&e, 0.1, 15.0, &mem),
        Err(MeshError::TooMuchMemory { .. })
    ));
    let mut empty = e;
    empty.faces.clear();
    empty.shells.clear();
    assert!(matches!(
        retessellate(&empty, 0.1, 15.0, &limits),
        Err(MeshError::Empty)
    ));
}

/// Planes must look away from the middle of the solid, bores must say that they are concave.
fn check_info(e: &ExactSolid) -> (usize, usize) {
    let lo = e
        .vertices
        .iter()
        .fold(DVec3::splat(f64::INFINITY), |a, v| a.min(*v));
    let hi = e
        .vertices
        .iter()
        .fold(DVec3::splat(f64::NEG_INFINITY), |a, v| a.max(*v));
    let mid = (lo + hi) / 2.0;
    let info = e.info(100);
    let (mut planes, mut concave) = (0, 0);
    for f in info["list"].as_array().unwrap() {
        if f["kind"] == "plane" {
            let v = |k: &str| {
                let a = f[k].as_array().unwrap();
                DVec3::new(
                    a[0].as_f64().unwrap(),
                    a[1].as_f64().unwrap(),
                    a[2].as_f64().unwrap(),
                )
            };
            assert!(v("normal").dot(v("point") - mid) > 0.0, "{f}");
            planes += 1;
        }
        if f["concave"] == true {
            concave += 1;
        }
    }
    (planes, concave)
}

#[test]
fn info_after_a_mirror_names_the_same_normals_and_bores() {
    let mirror = DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    let mirror_z = DAffine3::from_cols(DVec3::X, DVec3::Y, -DVec3::Z, DVec3::new(0.0, 0.0, 5.0));
    for (file, planes, concave) in [
        ("box.step", 6, 0),
        ("cylinder.step", 2, 0),
        ("plate_hole.step", 6, 1),
    ] {
        let imp = load(file, &StepOptions::default());
        let e = exact_of(&imp);
        assert_eq!(check_info(e), (planes, concave), "{file}");
        for m in [mirror, mirror_z] {
            let moved = e.transformed(&m).unwrap();
            assert_eq!(check_info(&moved), (planes, concave), "{file} mirrored");
        }
    }
}

#[test]
fn a_face_that_cannot_be_cut_any_more_does_not_open_a_closed_solid() {
    let imp = load("box.step", &StepOptions::default());
    let mut e = exact_of(&imp).clone();
    // A loop of two edges bounds nothing: the face gives no triangles.
    e.faces[0].loops[0].items.truncate(2);
    match retessellate(&e, 0.1, 15.0, &Limits::default()) {
        Err(MeshError::Unsupported(why)) => assert!(why.contains("old mesh is kept"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// The kinds of surface of the fixtures, to mirror one by one.
const MIRROR_FILES: [&str; 16] = [
    "box.step",
    "cylinder.step",
    "sphere.step",
    "hemisphere.step",
    "torus.step",
    "frustum.step",
    "cone.step",
    "revolved.step",
    "extrusion.step",
    "plate_hole.step",
    "hollow.step",
    "patch_rational.step",
    "patch_bump.step",
    "no_seam.step",
    "bead.step",
    "inverted.step",
];

fn concave_faces(e: &ExactSolid) -> Vec<(u64, bool)> {
    let info = e.info(1000);
    info["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["face"].as_u64().unwrap(), f["concave"] == true))
        .collect()
}

#[test]
fn a_mirror_of_every_kind_of_surface_cuts_to_the_same_solid_and_says_the_same_about_bores() {
    let mirrors = [
        DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)),
        DAffine3::from_scale(DVec3::new(1.0, -1.0, 1.0)),
        DAffine3::from_cols(DVec3::X, DVec3::Y, -DVec3::Z, DVec3::new(3.0, 4.0, 5.0)),
        // A turn and a mirror together, and a uniform scale with a mirror.
        DAffine3::from_mat3_translation(
            DMat3::from_rotation_z(0.7) * DMat3::from_diagonal(DVec3::new(-2.0, 2.0, 2.0)),
            DVec3::new(10.0, -3.0, 1.0),
        ),
    ];
    let limits = Limits::default();
    for file in MIRROR_FILES {
        let imp = load(file, &StepOptions::default());
        let e = exact_of(&imp);
        let base = retessellate(e, 0.02, 15.0, &limits).unwrap();
        let v0 = volume(&base.positions, &base.triangles);
        let a0 = area(&base.positions, &base.triangles);
        assert!(base.skipped.is_empty(), "{file}: {:?}", base.skipped);
        for (k, m) in mirrors.iter().enumerate() {
            let scale = m.matrix3.determinant().abs().cbrt();
            let moved = e.transformed(m).unwrap();
            assert!(moved.validate().is_ok(), "{file} {k}");
            let r = retessellate(&moved, 0.02 * scale, 15.0, &limits)
                .unwrap_or_else(|err| panic!("{file} mirror {k}: {err}"));
            assert!(r.skipped.is_empty(), "{file} mirror {k}: {:?}", r.skipped);
            let v = volume(&r.positions, &r.triangles);
            let a = area(&r.positions, &r.triangles);
            // The volume keeps its sign for a turn and flips for a mirror in the triangles
            // (they are turned inside out by the mirror only if the payload is wrong).
            let want_v = v0 * scale.powi(3);
            let want_a = a0 * scale.powi(2);
            // The volume of a surface that is not closed depends on where the origin is.
            if e.shells.iter().all(|s| s.closed) {
                assert!(
                    (v - want_v).abs() <= 0.02 * want_v.abs().max(1.0),
                    "{file} mirror {k}: volume {v} instead of {want_v}"
                );
            }
            assert!(
                (a - want_a).abs() <= 0.02 * want_a,
                "{file} mirror {k}: area {a} instead of {want_a}"
            );
            assert_eq!(
                concave_faces(&moved),
                concave_faces(e),
                "{file} mirror {k}: the bores differ"
            );
        }
    }
}
