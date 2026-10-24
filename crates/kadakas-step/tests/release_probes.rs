//! Probes of the budgets with files that take seconds, not milliseconds. They run in a release
//! build, and they are left out of the normal run (a debug build is some 20 times slower):
//!
//! `cargo test --release -p kadakas-step --test release_probes -- --ignored --nocapture`
//!
//! The numbers in `docs/interop.md` come from them (and from the `probe` example with
//! `/usr/bin/time -l` for the memory).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kadakas_meshio::MeshError;
use kadakas_step::{StepOptions, read};

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n";
const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

fn timed(text: &str) -> (Result<kadakas_step::StepImport, MeshError>, Duration) {
    let start = Instant::now();
    let r = read(text.as_bytes(), &StepOptions::default());
    (r, start.elapsed())
}

/// A revolution of `curve` about the Z axis, and `n` faces that share one loop on it. None
/// gives a triangle, and each point of the loop is searched on the surface.
fn faces_on_a_revolution(curve: &str, n: usize, extra: &str) -> String {
    let mut w = String::from(HEAD);
    w.push_str(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(10.,0.,0.));\n#3=AXIS2_PLACEMENT_3D('',#1,$,$);\n#4=CIRCLE('',#3,10.);\n\
#5=VERTEX_POINT('',#2);\n#6=EDGE_CURVE('',#5,#5,#4,.T.);\n#7=ORIENTED_EDGE('',*,*,#6,.T.);\n#8=EDGE_LOOP('',(#7));\n#9=FACE_OUTER_BOUND('',#8,.T.);\n\
#13=AXIS1_PLACEMENT('',#1,$);\n",
    );
    w.push_str(curve);
    w.push_str(extra);
    let mut faces = Vec::new();
    for k in 0..n {
        w.push_str(&format!(
            "#{}=ADVANCED_FACE('',(#9),#14,.T.);\n",
            10_000 + k
        ));
        faces.push(format!("#{}", 10_000 + k));
    }
    w.push_str(&format!(
        "#900000=OPEN_SHELL('',({}));\n#900001=SHELL_BASED_SURFACE_MODEL('s',(#900000));\n",
        faces.join(",")
    ));
    w.push_str(TAIL);
    w
}

#[test]
#[ignore = "release probe"]
fn thirty_thousand_faces_on_a_revolution_end_at_the_work_limit_in_a_second() {
    let line = "#10=DIRECTION('',(1.,0.,0.));\n#11=VECTOR('',#10,1.);\n#12=LINE('',#2,#11);\n#14=SURFACE_OF_REVOLUTION('',#12,#13);\n";
    let (r, t) = timed(&faces_on_a_revolution(line, 30_000, ""));
    println!("30,000 faces on a revolution of a line: {t:?}");
    assert!(
        matches!(r, Err(MeshError::TooMuchWork { .. })),
        "{:?}",
        r.map(|_| ())
    );
    assert!(t < Duration::from_secs(3), "{t:?}");
}

#[test]
#[ignore = "release probe"]
fn faces_on_a_revolution_of_a_spline_end_at_the_work_limit_in_seconds() {
    // 2,000 control points on the curve: each search along it is long.
    let n = 2000;
    let mut pts = String::new();
    let mut ids = Vec::new();
    for i in 0..n {
        pts.push_str(&format!(
            "#{}=CARTESIAN_POINT('',(10.,{},{}));\n",
            1000 + i,
            i as f64 * 0.01,
            (i % 7) as f64 * 0.001
        ));
        ids.push(format!("#{}", 1000 + i));
    }
    let mult = format!("(4,{},4)", vec!["1"; n - 4].join(","));
    let knots = format!(
        "(0.,{},{}.)",
        (1..n - 3)
            .map(|k| format!("{k}."))
            .collect::<Vec<_>>()
            .join(","),
        n - 3
    );
    let curve = format!(
        "{pts}#100=B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,{mult},{knots},.UNSPECIFIED.);\n#14=SURFACE_OF_REVOLUTION('',#100,#13);\n",
        ids.join(",")
    );
    let (r, t) = timed(&faces_on_a_revolution(&curve, 3000, ""));
    println!("3,000 faces on a revolution of a spline: {t:?}");
    assert!(
        matches!(r, Err(MeshError::TooMuchWork { .. })),
        "{:?}",
        r.map(|_| ())
    );
    assert!(t < Duration::from_secs(10), "{t:?}");
}

#[test]
#[ignore = "release probe"]
fn two_hundred_faces_of_4800_points_are_a_legitimate_file() {
    // 960,000 triangles of a polygon that many faces share: they weld to 9,596.
    let n = 4800;
    let mut w = String::from(HEAD);
    for i in 0..n {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        w.push_str(&format!(
            "#{}=CARTESIAN_POINT('',({:?},{:?},0.));\n",
            10 + i,
            100.0 * a.cos(),
            100.0 * a.sin()
        ));
    }
    let pts: Vec<String> = (0..n).map(|i| format!("#{}", 10 + i)).collect();
    w.push_str(&format!(
        "#5=POLY_LOOP('',({}));\n#6=FACE_OUTER_BOUND('',#5,.T.);\n",
        pts.join(",")
    ));
    let faces: Vec<String> = (0..200).map(|k| format!("#{}", 100_000 + k)).collect();
    for f in &faces {
        w.push_str(&format!("{f}=FACE('',(#6));\n"));
    }
    w.push_str(&format!(
        "#900000=CLOSED_SHELL('',({}));\n#900001=FACETED_BREP('b',#900000);\n",
        faces.join(",")
    ));
    w.push_str(TAIL);
    let (r, t) = timed(&w);
    println!("200 faces of 4,800 points: {t:?}");
    let r = r.expect("a legitimate file is read");
    println!("work {}", r.report.work_units);
    assert!(t < Duration::from_secs(5), "{t:?}");
}

#[test]
#[ignore = "release probe"]
fn two_and_a_half_thousand_plates_with_holes_are_a_legitimate_file() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/step/plate_hole.step");
    let src = std::fs::read_to_string(path).unwrap();
    let body = &src[src.find("DATA;\n").unwrap() + 6..src.rfind("ENDSEC;").unwrap()];
    // The biggest entity number, to shift the numbers of each copy past it.
    let max = body
        .split('#')
        .skip(1)
        .filter_map(|s| {
            s.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u64>()
                .ok()
        })
        .max()
        .unwrap()
        + 1;
    let mut w = String::from(HEAD);
    for k in 0..2500u64 {
        let mut out = String::with_capacity(body.len());
        let mut rest = body;
        while let Some(at) = rest.find('#') {
            out.push_str(&rest[..at]);
            rest = &rest[at + 1..];
            let digits = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            let n: u64 = rest[..digits].parse().unwrap();
            out.push_str(&format!("#{}", n + k * max));
            rest = &rest[digits..];
        }
        out.push_str(rest);
        w.push_str(&out);
    }
    w.push_str(TAIL);
    let (r, t) = timed(&w);
    println!("2,500 plates ({} MB): {t:?}", w.len() / 1_000_000);
    let r = r.expect("a legitimate file is read");
    println!(
        "work {}, triangles {}",
        r.report.work_units, r.set.totals.triangles
    );
    assert!(t < Duration::from_secs(8), "{t:?}");
}
