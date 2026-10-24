//! Regression tests for the findings of the review: small files that built a wrong result.

use std::path::PathBuf;

use kadakas_meshio::Part;
use kadakas_step::{StepImport, StepOptions, read};

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n";
const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

fn load(name: &str) -> StepImport {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step")
        .join(name);
    read(&std::fs::read(path).unwrap(), &StepOptions::default()).unwrap()
}

fn run(body: &str) -> StepImport {
    read(
        format!("{HEAD}{body}{TAIL}").as_bytes(),
        &StepOptions::default(),
    )
    .unwrap()
}

fn bounds(p: &Part) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in &p.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    (lo, hi)
}

/// The sum of the Z components of the triangle normals, as twice the area seen from above.
fn up_area(p: &Part) -> f64 {
    p.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| p.positions[i as usize]);
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        })
        .sum::<f64>()
        / 2.0
}

const SQUARE: &str =
    "#1=COORDINATES_LIST('',4,((0.,0.,0.),(10.,0.,0.),(10.,10.,0.),(0.,10.,0.)));\n";

#[test]
fn a_complex_set_with_empty_strips_reads_its_fan_and_its_point_index() {
    // The strips are an empty list: the fan is the second list, not the first.
    let r = run(&format!(
        "{SQUARE}#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,(),(),(),((1,2,3,4)));\n"
    ));
    let p = &r.set.parts[0];
    assert_eq!(p.triangles.len(), 2);
    assert!((up_area(p) - 100.0).abs() < 1e-9, "{}", up_area(p));
    // The point index turns the order of the points: the face looks down.
    let r = run(&format!(
        "{SQUARE}#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,(),(4,3,2,1),(),((1,2,3,4)));\n"
    ));
    assert!((up_area(&r.set.parts[0]) + 100.0).abs() < 1e-9);
    // A strip with no fan, and the face form with its link to the B-rep face.
    let r = run(&format!(
        "{SQUARE}#2=COMPLEX_TRIANGULATED_FACE('',#1,4,(),$,(),((1,2,4,3)),());\n"
    ));
    assert_eq!(r.set.parts[0].triangles.len(), 2);
}

/// A box (faceted B-rep) from entity 100 on, with the number of its solid.
fn box_text() -> (String, usize) {
    let mut w = String::new();
    let c = [
        (0., 0., 0.),
        (10., 0., 0.),
        (0., 10., 0.),
        (10., 10., 0.),
        (0., 0., 10.),
        (10., 0., 10.),
        (0., 10., 10.),
        (10., 10., 10.),
    ];
    for (i, p) in c.iter().enumerate() {
        w.push_str(&format!(
            "#{}=CARTESIAN_POINT('',({:?},{:?},{:?}));\n",
            100 + i,
            p.0,
            p.1,
            p.2
        ));
    }
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let mut faces = Vec::new();
    for (k, q) in quads.iter().enumerate() {
        let b = 200 + k * 3;
        let pts: Vec<String> = q.iter().map(|i| format!("#{}", 100 + i)).collect();
        w.push_str(&format!(
            "#{b}=POLY_LOOP('',({}));\n#{}=FACE_OUTER_BOUND('',#{b},.T.);\n#{}=FACE('',(#{}));\n",
            pts.join(","),
            b + 1,
            b + 2,
            b + 1
        ));
        faces.push(format!("#{}", b + 2));
    }
    w.push_str(&format!(
        "#300=CLOSED_SHELL('',({}));\n#301=FACETED_BREP('b',#300);\n",
        faces.join(",")
    ));
    (w, 301)
}

/// The text of a style: a surface style of the color `rgb` and a blue curve style, in the given
/// order, from entity number `id`. The assignment is `id + 12`.
fn style(first_surface: bool, id: usize, rgb: &str) -> String {
    let surface = format!(
        "#{a}=COLOUR_RGB('',{rgb});\n#{b}=FILL_AREA_STYLE_COLOUR('',#{a});\n#{c}=FILL_AREA_STYLE('',(#{b}));\n\
         #{d}=SURFACE_STYLE_FILL_AREA(#{c});\n#{e}=SURFACE_SIDE_STYLE('',(#{d}));\n#{f}=SURFACE_STYLE_USAGE(.BOTH.,#{e});\n",
        a = id,
        b = id + 1,
        c = id + 2,
        d = id + 3,
        e = id + 4,
        f = id + 5
    );
    let curve = format!(
        "#{a}=COLOUR_RGB('',0.,0.,1.);\n#{b}=CURVE_STYLE('',$,$,#{a});\n",
        a = id + 10,
        b = id + 11
    );
    let list = if first_surface {
        format!("#{},#{}", id + 5, id + 11)
    } else {
        format!("#{},#{}", id + 11, id + 5)
    };
    format!(
        "{surface}{curve}#{}=PRESENTATION_STYLE_ASSIGNMENT(({list}));\n",
        id + 12
    )
}

#[test]
fn the_color_of_a_surface_wins_over_the_color_of_a_curve_in_any_order() {
    for first_surface in [true, false] {
        let (b, solid) = box_text();
        let r = run(&format!(
            "{b}{}#1100=STYLED_ITEM('',(#1012),#{solid});\n",
            style(first_surface, 1000, "1.,0.,0.")
        ));
        assert_eq!(
            r.set.parts[0].color,
            Some([255, 0, 0, 255]),
            "{first_surface}"
        );
    }
}

#[test]
fn an_overriding_style_wins_whatever_the_order_of_the_file() {
    let (b, solid) = box_text();
    let base = style(true, 1000, "1.,0.,0.");
    let over = style(true, 2000, "0.,1.,0.");
    let styled = format!("#1100=STYLED_ITEM('',(#1012),#{solid});\n");
    let overriding = format!("#2100=OVER_RIDING_STYLED_ITEM('',(#2012),#{solid},#1100);\n");
    for text in [
        format!("{b}{base}{over}{styled}{overriding}"),
        format!("{b}{base}{over}{overriding}{styled}"),
    ] {
        let r = run(&text);
        assert_eq!(r.set.parts[0].color, Some([0, 255, 0, 255]));
    }
    // A style for one instance only does not color the item for all of them, and is counted.
    let one =
        format!("#2100=CONTEXT_DEPENDENT_OVER_RIDING_STYLED_ITEM('',(#2012),#{solid},#1100,());\n");
    let r = run(&format!("{b}{base}{over}{styled}{one}"));
    assert_eq!(r.set.parts[0].color, Some([255, 0, 0, 255]));
    assert!(
        r.report
            .skipped
            .iter()
            .any(|s| s.1.contains("one instance"))
    );
}

#[test]
fn a_unit_given_as_a_complex_measure_is_read() {
    let (b, solid) = box_text();
    let r = run(&format!(
        "{b}#10=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #11=(LENGTH_MEASURE_WITH_UNIT() MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#10));\n\
         #12=(CONVERSION_BASED_UNIT('INCH',#11) LENGTH_UNIT() NAMED_UNIT(*));\n\
         #13=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#12)) REPRESENTATION_CONTEXT('',''));\n\
         #14=AXIS2_PLACEMENT_3D('',#100,$,$);\n\
         #15=SHAPE_REPRESENTATION('',(#14,#{solid}),#13);\n"
    ));
    assert!(r.report.unit_from_file);
    let (lo, hi) = bounds(&r.set.parts[0]);
    assert!((hi[0] - lo[0] - 254.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

#[test]
fn a_shell_with_a_skipped_face_far_from_the_origin_keeps_its_outside() {
    // The box has one face that cannot be read. It sits 10 m from the origin. The triangles
    // of the other five faces must still look outward.
    let r = load("partial_box.step");
    let p = &r.set.parts[0];
    let (lo, hi) = bounds(p);
    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    assert!(!p.triangles.is_empty());
    for t in &p.triangles {
        let [a, b, d] = t.map(|i| p.positions[i as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let out = (0..3).map(|k| n[k] * (a[k] - c[k])).sum::<f64>();
        assert!(out > 0.0, "a triangle looks inward: {t:?}");
    }
    assert!(r.report.skipped.iter().any(|s| s.2 >= 1));
}

#[test]
fn child_frames_that_are_moved_and_both_directions_of_the_relationship() {
    let r = load("assembly_frames.step");
    assert_eq!(r.set.parts.len(), 2, "{:?}", r.set.notes);
    let mut boxes: Vec<_> = r.set.parts.iter().map(bounds).collect();
    boxes.sort_by(|a, b| a.0[0].total_cmp(&b.0[0]));
    // The frame at (2, 0, 0) with x along the world y goes to the frame in the parent. The
    // cube [0,10]^3 ends up with x in [100,110] and y in [-8,2], and the same at x = 200.
    for (b, x) in boxes.iter().zip([100.0, 200.0]) {
        let (lo, hi) = b;
        assert!(
            (lo[0] - x).abs() < 1e-6 && (hi[0] - x - 10.0).abs() < 1e-6,
            "{lo:?} {hi:?}"
        );
        assert!(
            (lo[1] + 8.0).abs() < 1e-6 && (hi[1] - 2.0).abs() < 1e-6,
            "{lo:?} {hi:?}"
        );
        assert!(lo[2].abs() < 1e-6 && (hi[2] - 10.0).abs() < 1e-6);
    }
}

#[test]
fn a_usage_without_a_placement_is_counted_and_not_hidden() {
    let r = load("assembly_unplaced.step");
    assert!(
        r.set.notes.iter().any(|n| n.contains("no placement")),
        "{:?}",
        r.set.notes
    );
    // The placed leg is where the file puts it. The other one is at the origin, as said.
    assert_eq!(r.set.parts.len(), 2);
}

#[test]
fn mapped_item_assemblies_do_not_show_the_mapped_part_twice() {
    let r = load("mapped.step");
    let triangles: usize = r.set.parts.iter().map(|p| p.triangles.len()).sum();
    // Two plates of 12 triangles: not three.
    assert_eq!(triangles, 24);
}

#[test]
fn entity_numbers_that_repeat_use_the_first_definition_and_are_reported() {
    let (b, solid) = box_text();
    let r = run(&format!(
        "{b}#400=CARTESIAN_POINT('',(1.,1.,1.));\n#400=CARTESIAN_POINT('',(2.,2.,2.));\n#401=GEOMETRIC_CURVE_SET('c',(#{solid}));\n"
    ));
    assert!(
        r.set.notes.iter().any(|n| n.contains("more than once")),
        "{:?}",
        r.set.notes
    );
}

#[test]
fn a_file_with_a_named_data_section_is_read() {
    let (b, _) = box_text();
    let text = format!("{HEAD}{b}{TAIL}").replace("DATA;", "DATA('part',('AUTOMOTIVE_DESIGN'));");
    let r = read(text.as_bytes(), &StepOptions::default()).unwrap();
    assert!(r.report.entities_read > 0);
}

#[test]
fn a_file_with_more_entities_than_the_limit_stops_in_the_parser() {
    let mut w = String::from(HEAD);
    for i in 1..=50_000 {
        w.push_str(&format!("#{i}=CARTESIAN_POINT('',(0.,0.,0.));\n"));
    }
    w.push_str(TAIL);
    let opts = StepOptions {
        step: kadakas_step::StepLimits {
            max_entities: 1000,
            ..Default::default()
        },
        ..Default::default()
    };
    let e = read(w.as_bytes(), &opts).err().unwrap();
    assert!(
        matches!(
            e,
            kadakas_meshio::MeshError::TooManyParts { limit: 1000, .. }
        ),
        "{e}"
    );
    // The size of a STEP file is limited lower than the general limit.
    let opts = StepOptions {
        step: kadakas_step::StepLimits {
            max_bytes: 100,
            ..Default::default()
        },
        ..Default::default()
    };
    let e = read(w.as_bytes(), &opts).err().unwrap();
    assert!(
        matches!(
            e,
            kadakas_meshio::MeshError::FileTooLarge { limit: 100, .. }
        ),
        "{e}"
    );
}
