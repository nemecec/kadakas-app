//! Files that nobody should trust: truncated, damaged, cyclic, huge, not finite. A read ends
//! with a clean refusal or a result. It never panics and never makes more than the limits allow.

use std::path::PathBuf;

use kadakas_meshio::{Limits, MeshError};
use kadakas_step::{StepLimits, StepOptions, read};

fn text(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn head(schema: &str) -> String {
    format!("ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n")
}

const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

fn run(body: &str) -> Result<kadakas_step::StepImport, MeshError> {
    read(
        format!("{}{body}{TAIL}", head("AUTOMOTIVE_DESIGN")).as_bytes(),
        &StepOptions::default(),
    )
}

/// A tiny xorshift for repeatable damage.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn a_file_cut_anywhere_never_panics() {
    let t = text("cylinder.step");
    let mut cut = 0;
    while cut < t.len() {
        if t.is_char_boundary(cut) {
            let r = read(&t.as_bytes()[..cut], &StepOptions::default());
            if cut < t.len() - 40 {
                assert!(r.is_err(), "cut at {cut}");
            }
        }
        cut += 37;
    }
    // A file that stops inside the data section is a truncation.
    let half = &t[..t.find("ENDSEC;\nEND").unwrap()];
    assert!(matches!(
        read(half.as_bytes(), &StepOptions::default()),
        Err(MeshError::Truncated { .. })
    ));
}

#[test]
fn damaged_bytes_never_panic() {
    for name in [
        "box.step",
        "sphere.step",
        "plate_hole.step",
        "patch_rational.step",
        "assembly.step",
        "tessellated.step",
        "torus.step",
    ] {
        let original = text(name).into_bytes();
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ name.len() as u64);
        for round in 0..60 {
            let mut b = original.clone();
            for _ in 0..1 + round % 5 {
                let at = (rng.next() as usize) % b.len();
                match rng.next() % 4 {
                    0 => b[at] = b"0123456789.#,()'$*=-E"[(rng.next() % 21) as usize],
                    1 => {
                        b.remove(at);
                    }
                    2 => b.insert(at, b"#9(),."[(rng.next() % 6) as usize]),
                    _ => b[at] ^= 1 << (rng.next() % 7),
                }
            }
            // Either a result or a refusal: no panic and no endless loop.
            let _ = read(&b, &StepOptions::default());
        }
    }
}

#[test]
fn numbers_that_are_not_finite_or_far_too_big_are_not_used() {
    for bad in ["1.E999", "-1.E999", "9.E307", "1.E308"] {
        let t = text("box.step").replace("20.0", bad);
        // Either nothing is read, or the box is refused for its size. No NaN gets in.
        match read(t.as_bytes(), &StepOptions::default()) {
            Ok(imp) => {
                for p in &imp.set.parts {
                    assert!(
                        p.positions.iter().all(|v| v.iter().all(|x| x.is_finite())),
                        "{bad}"
                    );
                }
            }
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
    let t = text("cylinder.step").replace("10.0", "1.E-320");
    let _ = read(t.as_bytes(), &StepOptions::default());
}

#[test]
fn cycles_in_references_end() {
    // A trimmed curve whose basis is itself, a composite curve that holds itself, a surface
    // of revolution whose curve is itself.
    let r = run(
        "#1=TRIMMED_CURVE('',#1,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);\n\
         #2=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#3);\n\
         #3=COMPOSITE_CURVE('',(#2),.F.);\n\
         #4=GEOMETRIC_CURVE_SET('loops',(#1,#3));\n",
    );
    assert!(r.is_err());
    // A shell that holds itself, a face set inside a face set.
    let r = run("#1=CLOSED_SHELL('',(#1));\n#2=MANIFOLD_SOLID_BREP('s',#1);\n");
    assert!(r.is_err());
    let r = run("#1=ORIENTED_CLOSED_SHELL('',*,#1,.T.);\n#2=MANIFOLD_SOLID_BREP('s',#1);\n");
    assert!(r.is_err());
    // An assembly that contains itself, and a part that is its own child.
    let t = text("assembly.step");
    let mut selfref = String::new();
    for line in t.lines() {
        if line.contains("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
            // parent = child
            let (before, after) = line.split_once("'','','',").unwrap();
            let parent = after.split(',').next().unwrap();
            let rest = after.split_once(',').unwrap().1;
            let rest = rest.split_once(',').unwrap().1;
            selfref.push_str(&format!("{before}'','','',{parent},{parent},{rest}\n"));
        } else {
            selfref.push_str(line);
            selfref.push('\n');
        }
    }
    let r = read(selfref.as_bytes(), &StepOptions::default());
    assert!(r.is_ok() || r.is_err());
    // A mapped item that maps its own representation.
    let r = run(
        "#1=AXIS2_PLACEMENT_3D('',#2,$,$);\n#2=CARTESIAN_POINT('',(0.,0.,0.));\n\
         #3=REPRESENTATION_MAP(#1,#5);\n#4=MAPPED_ITEM('',#3,#1);\n\
         #5=SHAPE_REPRESENTATION('',(#4),#6);\n#6=REPRESENTATION_CONTEXT('','');\n",
    );
    assert!(r.is_err());
}

#[test]
fn huge_degrees_counts_and_references_are_refused() {
    let points = "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(1.,0.,0.));\n";
    // A degree of 9999 and a million knots that are not there.
    let r = run(&format!(
        "{points}#3=B_SPLINE_CURVE_WITH_KNOTS('',9999,(#1,#2),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED.);\n#4=GEOMETRIC_CURVE_SET('c',(#3));\n"
    ));
    assert!(r.is_err());
    // Multiplicities that add up to more than the knots.
    let r = run(&format!(
        "{points}#3=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#2),.UNSPECIFIED.,.F.,.F.,(2000000000,2),(0.,1.),.UNSPECIFIED.);\n#4=GEOMETRIC_CURVE_SET('c',(#3));\n"
    ));
    assert!(r.is_err());
    // References to entities that do not exist, and the wrong kind of entity.
    let r = run("#1=MANIFOLD_SOLID_BREP('s',#999);\n#2=GEOMETRIC_CURVE_SET('c',(#1,#5,#999));\n");
    assert!(r.is_err());
    let r = run("#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=MANIFOLD_SOLID_BREP('s',#1);\n");
    assert!(r.is_err());
    // Two entities with one number, and a number that is huge.
    let r = run(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#1=CARTESIAN_POINT('',(1.,1.,1.));\n#18446744073709551615=CARTESIAN_POINT('',(1.,1.,1.));\n",
    );
    assert!(r.is_err());
    let r = run("#99999999999999999999999=CARTESIAN_POINT('',(1.,1.,1.));\n");
    assert!(matches!(r, Err(MeshError::Malformed(_))));
}

#[test]
fn the_limits_stop_a_read_with_the_counts() {
    let cyl = text("cylinder.step");
    let bytes = cyl.as_bytes();
    let o = |limits: Limits| StepOptions {
        limits,
        ..Default::default()
    };
    let e = read(
        bytes,
        &o(Limits {
            max_bytes: 100,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MeshError::FileTooLarge { limit: 100, .. }));
    let e = read(
        bytes,
        &o(Limits {
            max_triangles: 10,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MeshError::TooManyTriangles { limit: 10, .. }));
    let e = read(
        bytes,
        &o(Limits {
            max_vertices: 10,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MeshError::TooManyVertices { limit: 10, .. }));
    let e = read(
        bytes,
        &o(Limits {
            max_memory: 1000,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MeshError::TooMuchMemory { limit: 1000, .. }));
    let step = |l: StepLimits| StepOptions {
        step: l,
        ..Default::default()
    };
    let e = read(
        bytes,
        &step(StepLimits {
            max_entities: 5,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MeshError::TooManyParts { limit: 5, .. }));
    // The assembly limits.
    let asm = text("assembly.step");
    let e = read(
        asm.as_bytes(),
        &step(StepLimits {
            max_instances: 1,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MeshError::TooManyParts { limit: 1, .. }));
    // With no depth at all the parts of the assembly are left out.
    let e = read(
        asm.as_bytes(),
        &step(StepLimits {
            max_assembly_depth: 0,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert!(matches!(e, MeshError::Empty));
    // A face is cut at its own limit and the report says so.
    let sph = text("sphere.step");
    let cut = read(
        sph.as_bytes(),
        &step(StepLimits {
            max_face_triangles: 300,
            ..Default::default()
        }),
    )
    .unwrap();
    assert!(cut.set.totals.triangles > 0);
    assert!(
        cut.set.notes.iter().any(|n| n.contains("limit of 300")),
        "{:?}",
        cut.set.notes
    );
    // Control points over the limit skip the face.
    let patch = text("patch_flat.step");
    let r = read(
        patch.as_bytes(),
        &step(StepLimits {
            max_control_points: 4,
            ..Default::default()
        }),
    );
    assert!(matches!(r, Err(MeshError::Unsupported(_))), "{r:?}");
    let patch = text("patch_rational.step");
    assert!(
        read(
            patch.as_bytes(),
            &step(StepLimits {
                max_control_points: 2,
                ..Default::default()
            })
        )
        .is_err()
    );
}

#[test]
fn files_that_are_not_step_say_so() {
    let o = StepOptions::default();
    for junk in [
        "",
        "hello",
        "ISO-10303-21;",
        "ISO-10303-21;\nHEADER;",
        "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=A(",
    ] {
        let e = read(junk.as_bytes(), &o).unwrap_err();
        assert!(!e.to_string().is_empty(), "{junk:?}");
    }
    let bom = format!("\u{feff}{}", text("box.step"));
    assert!(read(bom.as_bytes(), &o).is_ok());
    let latin1: Vec<u8> = text("box.step")
        .replace("Kadakas test file", "Pähkinä")
        .bytes()
        .collect();
    assert!(read(&latin1, &o).is_ok());
    let nothing = format!("{}{TAIL}", head("SOMETHING_ELSE"));
    assert!(matches!(
        read(nothing.as_bytes(), &o),
        Err(MeshError::Empty)
    ));
    // An unknown schema is a note.
    let other = text("box.step").replace(
        "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }",
        "MY_OWN_SCHEMA",
    );
    let imp = read(other.as_bytes(), &o).unwrap();
    assert!(imp.set.notes.iter().any(|n| n.contains("MY_OWN_SCHEMA")));
}
