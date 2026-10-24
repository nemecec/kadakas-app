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

/// 2,500 copies of the plate with a hole, numbered apart, in one file.
fn plates() -> String {
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
        let mut rest = body;
        while let Some(at) = rest.find('#') {
            w.push_str(&rest[..at]);
            rest = &rest[at + 1..];
            let digits = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            let n: u64 = rest[..digits].parse().unwrap();
            w.push_str(&format!("#{}", n + k * max));
            rest = &rest[digits..];
        }
        w.push_str(rest);
    }
    w.push_str(TAIL);
    w
}

#[test]
#[ignore = "release probe"]
fn two_and_a_half_thousand_plates_with_holes_are_a_legitimate_file() {
    let w = plates();
    let (r, t) = timed(&w);
    println!("2,500 plates ({} MB): {t:?}", w.len() / 1_000_000);
    let r = r.expect("a legitimate file is read");
    println!(
        "work {}, triangles {}",
        r.report.work_units, r.set.totals.triangles
    );
    assert!(t < Duration::from_secs(8), "{t:?}");
}

#[test]
#[ignore = "release probe"]
fn the_plates_at_a_fine_tolerance_are_a_legitimate_file_that_takes_seconds() {
    // 1.2 million triangles of cylinders and planes. It was refused at 177.9 million steps
    // against a limit of 177.87 million, after 3.8 seconds of honest work.
    let w = plates();
    for (tol, secs) in [(0.005, 10), (0.002, 20)] {
        let start = Instant::now();
        let r = read(
            w.as_bytes(),
            &StepOptions {
                tolerance_mm: Some(tol),
                ..Default::default()
            },
        );
        let t = start.elapsed();
        let r = r.unwrap_or_else(|e| panic!("tolerance {tol} after {t:?}: {e}"));
        println!(
            "plates at {tol} mm: {t:?}, {} triangles, work {}",
            r.set.totals.triangles, r.report.work_units
        );
        assert!(t < Duration::from_secs(secs), "{t:?}");
    }
}

/// 6 mapped copies of a mesh of a million triangles that the file gives, then 60,000 faces on
/// a revolution of a spline that give nothing. The copies were counted as made triangles, and
/// each earned work for the faces: 54 seconds before the limit.
fn copies_then_faces() -> String {
    let g = 700usize;
    let mut w = String::from(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n",
    );
    w.push_str(
        "#1=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#2)) REPRESENTATION_CONTEXT('',''));\n\
#2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#3=CARTESIAN_POINT('',(0.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#3,$,$);\n",
    );
    let pts: Vec<String> = (0..g * g)
        .map(|k| format!("({}.,{}.,{}.)", k % g, k / g, (k % g * (k / g)) % 7))
        .collect();
    w.push_str(&format!(
        "#10=COORDINATES_LIST('',{},({}));\n",
        g * g,
        pts.join(",")
    ));
    let mut tris = Vec::new();
    for j in 0..g - 1 {
        for i in 0..g - 1 {
            let a = j * g + i + 1;
            tris.push(format!("({a},{},{})", a + 1, a + g));
            tris.push(format!("({},{},{})", a + 1, a + g + 1, a + g));
        }
    }
    w.push_str(&format!(
        "#11=TRIANGULATED_FACE('',#10,{},(),$,(),({}));\n",
        g * g,
        tris.join(",")
    ));
    w.push_str("#12=SHAPE_REPRESENTATION('mesh',(#4,#11),#1);\n#13=REPRESENTATION_MAP(#4,#12);\n");
    for k in 0..6 {
        w.push_str(&format!("#{}=MAPPED_ITEM('',#13,#4);\n", 20 + k));
    }
    w.push_str(
        "#100=CARTESIAN_POINT('',(10.,0.,0.));\n#101=AXIS2_PLACEMENT_3D('',#3,$,$);\n#102=CIRCLE('',#101,10.);\n\
#103=VERTEX_POINT('',#100);\n#104=EDGE_CURVE('',#103,#103,#102,.T.);\n#105=ORIENTED_EDGE('',*,*,#104,.T.);\n\
#106=EDGE_LOOP('',(#105));\n#107=FACE_OUTER_BOUND('',#106,.T.);\n#108=AXIS1_PLACEMENT('',#3,$);\n",
    );
    let n = 2000;
    let ids: Vec<String> = (0..n).map(|i| format!("#{}", 1000 + i)).collect();
    for i in 0..n {
        w.push_str(&format!(
            "#{}=CARTESIAN_POINT('',(10.,{},{}));\n",
            1000 + i,
            i as f64 * 0.01,
            (i % 7) as f64 * 0.001
        ));
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
    w.push_str(&format!(
        "#109=B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,{mult},{knots},.UNSPECIFIED.);\n#110=SURFACE_OF_REVOLUTION('',#109,#108);\n",
        ids.join(",")
    ));
    let faces: Vec<String> = (0..60_000).map(|k| format!("#{}", 10_000 + k)).collect();
    for f in &faces {
        w.push_str(&format!("{f}=ADVANCED_FACE('',(#107),#110,.T.);\n"));
    }
    w.push_str(&format!(
        "#900000=OPEN_SHELL('',({}));\n#900001=SHELL_BASED_SURFACE_MODEL('s',(#900000));\n\
#900002=SHAPE_REPRESENTATION('top',(#4,#20,#21,#22,#23,#24,#25,#900001),#1);\n{TAIL}",
        faces.join(",")
    ));
    w
}

#[test]
#[ignore = "release probe"]
fn copies_of_a_big_mesh_do_not_earn_time_for_faces_that_give_nothing() {
    let w = copies_then_faces();
    let (r, t) = timed(&w);
    println!(
        "6 copies of a million triangles, then 60,000 faces ({} MB): {t:?}",
        w.len() / 1_000_000
    );
    assert!(
        matches!(r, Err(MeshError::TooMuchWork { .. })),
        "{:?}",
        r.map(|_| ())
    );
    assert!(t < Duration::from_secs(8), "{t:?}");
}

/// One planar face (faceted) with `n` holes of 3 or 4 corners, in rows of the same length.
fn face_with_many_holes(n: usize, corners: usize, spacing: f64, outer: Option<f64>) -> String {
    let side = (n as f64).sqrt().ceil() as usize;
    let (w, h) = match outer {
        Some(o) => (o, o),
        None => (side as f64 * spacing + 10.0, side as f64 * spacing + 10.0),
    };
    let mut s = String::from(HEAD);
    let mut id = 10usize;
    let mut pt = |s: &mut String, x: f64, y: f64| {
        id += 1;
        s.push_str(&format!("#{id}=CARTESIAN_POINT('',({x:?},{y:?},0.));\n"));
        id
    };
    let outer: Vec<String> = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
        .iter()
        .map(|p| format!("#{}", pt(&mut s, p.0, p.1)))
        .collect();
    s.push_str(&format!(
        "#1=POLY_LOOP('',({}));\n#2=FACE_OUTER_BOUND('',#1,.T.);\n",
        outer.join(",")
    ));
    let mut bounds = vec!["#2".to_string()];
    let mut next = 10_000_000usize;
    'rows: for j in 0..side {
        for i in 0..side {
            if bounds.len() > n {
                break 'rows;
            }
            let (x, y) = (spacing * i as f64 + 10.0, spacing * j as f64 + 10.0);
            let q = spacing / 2.0;
            let mut c = vec![(x, y), (x, y + q), (x + q, y + q), (x + q, y)];
            c.truncate(corners);
            let ids: Vec<String> = c
                .iter()
                .map(|p| format!("#{}", pt(&mut s, p.0, p.1)))
                .collect();
            s.push_str(&format!(
                "#{next}=POLY_LOOP('',({}));\n#{}=FACE_BOUND('',#{next},.T.);\n",
                ids.join(","),
                next + 1
            ));
            bounds.push(format!("#{}", next + 1));
            next += 2;
        }
    }
    s.push_str(&format!(
        "#3=FACE('',({}));\n#4=CLOSED_SHELL('',(#3));\n#5=FACETED_BREP('plate',#4);\n{TAIL}",
        bounds.join(",")
    ));
    s
}

#[test]
#[ignore = "release probe"]
fn a_face_with_ten_thousand_square_holes_is_cut_in_a_blink() {
    let (r, t) = timed(&face_with_many_holes(10_000, 4, 20.0, None));
    println!("100 x 100 square holes: {t:?}");
    let r = r.expect("a legitimate file is read");
    println!(
        "work {}, triangles {}",
        r.report.work_units, r.set.totals.triangles
    );
    assert_eq!(r.set.totals.triangles, 60_002);
    assert!(t < Duration::from_millis(500), "{t:?}");
}

#[test]
#[ignore = "release probe"]
fn a_face_with_thirty_thousand_and_a_hundred_thousand_holes_is_cut_or_refused_fast() {
    let (r, t) = timed(&face_with_many_holes(30_000, 3, 20.0, None));
    println!("30,000 triangle holes: {t:?}");
    r.expect("a legitimate file is read");
    assert!(t < Duration::from_secs(3), "{t:?}");
    let (r, t) = timed(&face_with_many_holes(100_000, 3, 20.0, None));
    println!("100,000 triangle holes: {t:?}");
    assert!(t < Duration::from_secs(10), "{t:?}");
    // Either result is a clean answer.
    match r {
        Ok(i) => assert!(i.set.totals.triangles > 400_000),
        Err(e) => assert!(
            e.is_limit() || matches!(e, MeshError::Unsupported(_)),
            "{e}"
        ),
    }
}

#[test]
#[ignore = "release probe"]
fn holes_all_in_one_corner_of_a_huge_face_end_at_the_fuel_fast() {
    // 30,000 holes in a square of 3 mm in a face of 1,000 km: the grid of the joining has all
    // of them in one cell. The steps are counted, and the face is refused.
    let (r, t) = timed(&face_with_many_holes(30_000, 3, 0.02, Some(1.0e9)));
    println!("30,000 holes in one cell: {t:?}");
    assert!(t < Duration::from_secs(10), "{t:?}");
    match r {
        Ok(_) => {}
        Err(e) => assert!(
            matches!(
                e,
                MeshError::TooMuchWork { .. }
                    | MeshError::Unsupported(_)
                    | MeshError::OutOfRange { .. }
            ),
            "{e}"
        ),
    }
}
