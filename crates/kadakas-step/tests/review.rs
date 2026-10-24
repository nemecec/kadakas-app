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
            kadakas_meshio::MeshError::TooManyEntities { limit: 1000, .. }
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

#[test]
fn mapped_items_with_a_usage_and_a_plain_relationship_are_not_doubled() {
    // The frame maps the plate twice and also uses the plate as a product, with a relationship
    // that has no transformation. The mapped items place the plates: 24 triangles, no third
    // plate at the origin, and no note about a missing placement.
    let r = load("mapped_nauo.step");
    let triangles: usize = r.set.parts.iter().map(|p| p.triangles.len()).sum();
    assert_eq!(
        triangles,
        24,
        "{:?}",
        r.set.parts.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    assert_eq!(r.set.parts.len(), 1);
    assert!(
        !r.set
            .notes
            .iter()
            .any(|n| n.contains("no placement") || n.contains("stand at the origin")),
        "{:?}",
        r.set.notes
    );
}

#[test]
fn a_file_with_many_mapped_items_of_an_empty_representation_is_read_in_a_blink() {
    // 20,000 mapped items of a representation without geometry, and 100,000 points in the file.
    // The size of the file was found again at every mapped item: 40 seconds.
    let mut w = String::from(HEAD);
    w.push_str(
        "#1=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#2)) REPRESENTATION_CONTEXT('',''));\n\
         #2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #3=CARTESIAN_POINT('',(0.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#3,$,$);\n\
         #5=SHAPE_REPRESENTATION('empty',(#4),#1);\n#6=REPRESENTATION_MAP(#4,#5);\n",
    );
    let items: Vec<String> = (0..20_000).map(|i| format!("#{}", 100 + i)).collect();
    for i in 0..20_000 {
        w.push_str(&format!("#{}=MAPPED_ITEM('',#6,#4);\n", 100 + i));
    }
    w.push_str(&format!(
        "#50000=SHAPE_REPRESENTATION('top',({}),#1);\n",
        items.join(",")
    ));
    for i in 0..100_000 {
        w.push_str(&format!(
            "#{}=CARTESIAN_POINT('',({i}.,0.,0.));\n",
            200_000 + i
        ));
    }
    w.push_str(TAIL);
    let start = std::time::Instant::now();
    let r = read(w.as_bytes(), &StepOptions::default());
    // Debug builds are slow: a second is 100 times what a release build needs.
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    assert!(
        matches!(r, Err(kadakas_meshio::MeshError::Empty)),
        "{:?}",
        r.map(|i| i.set.parts.len())
    );
}

#[test]
fn each_limit_has_its_own_error_and_words() {
    use kadakas_meshio::MeshError;
    use kadakas_step::StepLimits;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/step");
    let bytes = |n: &str| std::fs::read(path.join(n)).unwrap();
    let with = |step: StepLimits| StepOptions {
        step,
        ..Default::default()
    };
    // The memory of the parser.
    let e = read(
        &bytes("plate_hole.step"),
        &with(StepLimits {
            max_parse_memory: 2000,
            ..Default::default()
        }),
    )
    .err()
    .unwrap();
    assert!(matches!(e, MeshError::TooMuchMemory { .. }), "{e}");
    assert_eq!(e.kind(), "mesh_too_much_memory");
    // The number of entities.
    let e = read(
        &bytes("plate_hole.step"),
        &with(StepLimits {
            max_entities: 10,
            ..Default::default()
        }),
    )
    .err()
    .unwrap();
    assert_eq!(e.kind(), "mesh_too_many_entities");
    assert!(
        e.to_string().contains("entities") && !e.to_string().contains("parts"),
        "{e}"
    );
    // The uses of products in an assembly, and the copies of mapped items.
    let e = read(
        &bytes("assembly.step"),
        &with(StepLimits {
            max_instances: 1,
            ..Default::default()
        }),
    )
    .err()
    .unwrap();
    assert_eq!(e.kind(), "mesh_too_many_instances");
    assert!(e.to_string().contains("uses products"), "{e}");
    let e = read(
        &bytes("mapped5.step"),
        &with(StepLimits {
            max_instances: 1,
            ..Default::default()
        }),
    )
    .err()
    .unwrap();
    assert!(
        matches!(e, MeshError::TooManyCopies { .. }) && e.to_string().contains("copy"),
        "{e}"
    );
    // The clock: a limit of no time at all.
    let e = read(
        &bytes("sphere.step"),
        &StepOptions {
            tolerance_mm: Some(0.002),
            step: StepLimits {
                max_millis: 0,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .err()
    .unwrap();
    assert_eq!(e.kind(), "mesh_too_much_time", "{e}");
    assert!(e.is_limit());
}

#[test]
fn parts_that_are_joined_by_color_lose_their_exact_data_and_the_report_says_so() {
    // More parts than `max_parts` are joined by color. The payloads of the parts do not fit the
    // joined parts, so none is kept, and the report names the reason.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/step/assembly.step");
    let opts = StepOptions {
        limits: kadakas_meshio::Limits {
            max_parts: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let r = read(&std::fs::read(path).unwrap(), &opts).unwrap();
    assert_eq!(r.set.parts.len(), 1);
    assert!(r.exact.iter().all(Option::is_none));
    assert_eq!(r.report.exact_kept, 0);
    assert!(
        r.report
            .exact_dropped
            .iter()
            .any(|(_, why)| why.contains("joined by color")),
        "{:?}",
        r.report.exact_dropped
    );
    assert!(
        r.report.to_json()["exact"]["without"][0]["reason"]
            .as_str()
            .is_some()
    );
}

/// One planar face (faceted) of 10 mm squares with a grid of square holes.
fn face_with_holes(nx: usize, ny: usize) -> (String, f64) {
    let (w, h) = (nx as f64 * 20.0 + 10.0, ny as f64 * 20.0 + 10.0);
    let mut s = String::new();
    let mut id = 10usize;
    let mut pt = |s: &mut String, x: f64, y: f64| {
        id += 1;
        s.push_str(&format!("#{id}=CARTESIAN_POINT('',({x:?},{y:?},0.));\n"));
        id
    };
    let outer: Vec<usize> = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
        .iter()
        .map(|p| pt(&mut s, p.0, p.1))
        .collect();
    let list = |v: &[usize]| {
        v.iter()
            .map(|i| format!("#{i}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    s.push_str(&format!(
        "#1=POLY_LOOP('',({}));\n#2=FACE_OUTER_BOUND('',#1,.T.);\n",
        list(&outer)
    ));
    let mut bounds = vec!["#2".to_string()];
    let mut next = 100_000usize;
    for j in 0..ny {
        for i in 0..nx {
            let (x, y) = (20.0 * i as f64 + 10.0, 20.0 * j as f64 + 10.0);
            let c: Vec<usize> = [(x, y), (x, y + 10.0), (x + 10.0, y + 10.0), (x + 10.0, y)]
                .iter()
                .map(|p| pt(&mut s, p.0, p.1))
                .collect();
            s.push_str(&format!(
                "#{next}=POLY_LOOP('',({}));\n#{}=FACE_BOUND('',#{next},.T.);\n",
                list(&c),
                next + 1
            ));
            bounds.push(format!("#{}", next + 1));
            next += 2;
        }
    }
    s.push_str(&format!(
        "#3=FACE('',({}));\n#4=CLOSED_SHELL('',(#3));\n#5=FACETED_BREP('plate',#4);\n",
        bounds.join(",")
    ));
    (s, w * h - 100.0 * (nx * ny) as f64)
}

#[test]
fn a_planar_face_with_holes_in_a_column_a_row_or_a_block_is_cut() {
    for (nx, ny) in [(1, 3), (1, 4), (3, 1), (2, 3), (3, 3), (10, 10)] {
        let (body, want) = face_with_holes(nx, ny);
        let r = run(&body);
        assert!(
            r.report
                .skipped
                .iter()
                .all(|s| !s.1.contains("cannot be cut")),
            "{nx}x{ny}: {:?}",
            r.report.skipped
        );
        let p = &r.set.parts[0];
        let area: f64 = p
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| p.positions[i as usize]);
                (((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2.0).abs()
            })
            .sum();
        assert!(
            (area - want).abs() < 1e-6 * want,
            "{nx}x{ny}: {area} instead of {want}"
        );
    }
}

#[test]
fn a_face_that_is_left_out_is_always_in_a_note() {
    // Two faces of a solid: a triangle, and a face of three points on a line, which has no
    // plane and cannot be cut.
    let r = run(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(10.,0.,0.));\n#3=CARTESIAN_POINT('',(0.,10.,0.));\n\
         #4=CARTESIAN_POINT('',(5.,0.,0.));\n#5=CARTESIAN_POINT('',(7.,0.,0.));\n\
         #6=POLY_LOOP('',(#1,#2,#3));\n#7=FACE_OUTER_BOUND('',#6,.T.);\n#8=FACE('',(#7));\n\
         #9=POLY_LOOP('',(#1,#4,#5));\n#10=FACE_OUTER_BOUND('',#9,.T.);\n#11=FACE('',(#10));\n\
         #12=CLOSED_SHELL('',(#8,#11));\n#13=FACETED_BREP('b',#12);\n",
    );
    assert_eq!(r.set.parts[0].triangles.len(), 1);
    assert!(
        r.set
            .notes
            .iter()
            .any(|n| n.contains("1 faces were not cut") || n.contains("not cut")),
        "{:?}",
        r.set.notes
    );
}

#[test]
fn a_solid_far_from_the_origin_keeps_all_its_faces() {
    // The plate with a hole 10 km from the origin and turned, with the locations of its planes
    // at the feet of the perpendiculars from the origin: the parameters of its faces are far
    // from zero. The area of a loop lost all its digits there, and two planar faces were left
    // out (166 triangles of 190 and an open solid).
    let near = load("plate_hole.step");
    let far = load("plate_far.step");
    let measure = |i: &StepImport| {
        let p = &i.set.parts[0];
        let o = glam::DVec3::from(p.positions[0]);
        let (mut vol, mut area) = (0.0, 0.0);
        for t in &p.triangles {
            let q = |k: u32| glam::DVec3::from(p.positions[k as usize]) - o;
            vol += q(t[0]).dot(q(t[1]).cross(q(t[2]))) / 6.0;
            area += (q(t[1]) - q(t[0])).cross(q(t[2]) - q(t[0])).length() / 2.0;
        }
        (vol, area)
    };
    let (v0, a0) = measure(&near);
    let (v1, a1) = measure(&far);
    assert!((v1 - v0).abs() < 0.01 * v0, "{v1} {v0}");
    assert!((a1 - a0).abs() < 0.01 * a0, "{a1} {a0}");
    assert!(
        far.report
            .tessellated
            .iter()
            .any(|t| t.0.contains("PLANE") && t.1 == 6)
    );
    assert!(
        far.set.notes.iter().all(|n| !n.contains("not cut")),
        "{:?}",
        far.set.notes
    );
}

#[test]
fn seven_mapped_copies_of_a_mesh_count_seven_times_not_eight() {
    // The mesh is read once and each mapped item makes a copy of it. The mesh itself is not in
    // the result, so the vertices of the result are seven times the vertices of the mesh.
    let g = 142usize;
    let mut w = String::from(HEAD);
    w.push_str(
        "#1=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#2)) REPRESENTATION_CONTEXT('',''));\n\
         #2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #3=CARTESIAN_POINT('',(0.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#3,$,$);\n",
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
    let mut items = Vec::new();
    for k in 0..7 {
        let target = 100 + 2 * k;
        w.push_str(&format!(
            "#{}=CARTESIAN_POINT('',({}.,0.,0.));\n#{}=AXIS2_PLACEMENT_3D('',#{},$,$);\n#{}=MAPPED_ITEM('',#13,#{});\n",
            target,
            k * 1000,
            target + 1,
            target,
            200 + k,
            target + 1
        ));
        items.push(format!("#{}", 200 + k));
    }
    w.push_str(&format!(
        "#300=SHAPE_REPRESENTATION('top',({}),#1);\n#301=APPLICATION_CONTEXT('x');\n#302=PRODUCT_CONTEXT('',#301,'m');\n\
         #303=PRODUCT('top','top','',(#302));\n#304=PRODUCT_DEFINITION_FORMATION('','',#303);\n\
         #305=PRODUCT_DEFINITION_CONTEXT('p',#301,'d');\n#306=PRODUCT_DEFINITION('d','',#304,#305);\n\
         #307=PRODUCT_DEFINITION_SHAPE('','',#306);\n#308=SHAPE_DEFINITION_REPRESENTATION(#307,#300);\n{TAIL}",
        items.join(",")
    ));
    let opts = StepOptions {
        limits: kadakas_meshio::Limits {
            max_vertices: 7 * (g * g) as u64 + 100,
            ..Default::default()
        },
        ..Default::default()
    };
    let r = read(w.as_bytes(), &opts).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.set.totals.vertices, 7 * (g * g) as u64);
}

#[test]
fn a_product_that_one_frame_maps_and_another_assembly_places_is_in_both_places() {
    // Loss A: the plate is mapped twice by the frame and placed once by a usage with a
    // transformation: three plates of 12 triangles.
    let r = load("mapped_and_placed.step");
    let triangles: usize = r.set.parts.iter().map(|p| p.triangles.len()).sum();
    assert_eq!(
        triangles,
        36,
        "{:?}",
        r.set
            .parts
            .iter()
            .map(|p| (&p.name, p.triangles.len()))
            .collect::<Vec<_>>()
    );
    let far = r
        .set
        .parts
        .iter()
        .any(|p| p.positions.iter().any(|v| v[0] >= 500.0));
    assert!(far, "the plate that the usage places at x = 500 is missing");
    assert!(
        !r.set.notes.iter().any(|n| n.contains("no placement")),
        "{:?}",
        r.set.notes
    );
}

#[test]
fn a_mapped_representation_without_geometry_follows_its_relationship() {
    // Loss B: the plate's representation is empty and a plain relationship ties it to the one
    // with the box. The frame maps the empty one twice: two plates.
    let r = load("mapped_related.step");
    let triangles: usize = r.set.parts.iter().map(|p| p.triangles.len()).sum();
    assert_eq!(triangles, 24, "{:?}", r.set.notes);
    let xs: Vec<f64> = r
        .set
        .parts
        .iter()
        .flat_map(|p| p.positions.iter().map(|v| v[0]))
        .collect();
    assert!(xs.iter().cloned().fold(f64::INFINITY, f64::min) > 40.0);
}
