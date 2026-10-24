//! The less common paths of the reader: voids, inverted shells, loops with no seam, cones,
//! other B-spline kinds, curves of every kind, tessellated variants, styles and units in
//! degrees, mapped items and loose solids. The files are made by `make_step_files.py`.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::PathBuf;

use glam::DVec3;
use kadakas_step::{StepImport, StepOptions, read};

fn load(name: &str) -> StepImport {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step")
        .join(name);
    read(&std::fs::read(path).unwrap(), &StepOptions::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn volume(p: &kadakas_meshio::Part) -> f64 {
    p.triangles
        .iter()
        .map(|t| {
            let q = |i: u32| DVec3::from(p.positions[i as usize]);
            q(t[0]).dot(q(t[1]).cross(q(t[2]))) / 6.0
        })
        .sum()
}

fn open_edges(p: &kadakas_meshio::Part) -> usize {
    let mut e: HashMap<(u32, u32), (i32, i32)> = HashMap::new();
    for t in &p.triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let c = e.entry((a.min(b), a.max(b))).or_default();
            if a < b { c.0 += 1 } else { c.1 += 1 }
        }
    }
    e.values().filter(|v| **v != (1, 1)).count()
}

fn bounds(p: &kadakas_meshio::Part) -> ([f64; 3], [f64; 3]) {
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

fn accounted(i: &StepImport) {
    assert_eq!(
        i.report.entities_read,
        i.report.entities_used + i.report.entities_skipped
    );
}

#[test]
fn an_inverted_shell_is_turned_and_a_void_takes_volume_away() {
    let inv = load("inverted.step");
    accounted(&inv);
    assert!(
        inv.set
            .notes
            .iter()
            .any(|n| n.contains("turned inside out"))
    );
    let p = &inv.set.parts[0];
    assert!((volume(p) - 6000.0).abs() < 1e-6 && open_edges(p) == 0);
    // The payload has the right orientation after the turn: it cuts the same positive volume.
    let e = inv.exact[0].as_ref().unwrap();
    let r = kadakas_step::retessellate(e, 0.1, 15.0, &kadakas_meshio::Limits::default()).unwrap();
    let v: f64 = r
        .triangles
        .iter()
        .map(|t| {
            let q = |i: u32| DVec3::from(r.positions[i as usize]);
            q(t[0]).dot(q(t[1]).cross(q(t[2]))) / 6.0
        })
        .sum();
    assert!((v - 6000.0).abs() < 1e-6);
    let hollow = load("hollow.step");
    let p = &hollow.set.parts[0];
    // 40 x 40 x 40 with a hole of 20 x 20 x 20.
    assert!(
        (volume(p) - (64000.0 - 8000.0)).abs() < 1e-6,
        "{}",
        volume(p)
    );
    assert_eq!(open_edges(p), 0);
    assert_eq!(hollow.exact[0].as_ref().unwrap().shells.len(), 2);
}

#[test]
fn loops_without_a_seam_and_a_cone_with_its_apex() {
    let n = load("no_seam.step");
    accounted(&n);
    let p = &n.set.parts[0];
    let want = PI * 100.0 * 20.0;
    assert!(volume(p) < want && volume(p) > want * 0.98, "{}", volume(p));
    assert_eq!(open_edges(p), 0);
    let c = load("cone.step");
    let p = &c.set.parts[0];
    let want = PI * 100.0 * 25.0 / 3.0;
    assert!(
        volume(p) < want && volume(p) > want * 0.97,
        "{} {want}",
        volume(p)
    );
    assert_eq!(open_edges(p), 0);
}

#[test]
fn faces_without_surfaces_oriented_faces_and_the_other_spline_kinds() {
    let f = load("oriented_faces.step");
    accounted(&f);
    assert_eq!(f.set.parts[0].triangles.len(), 3);
    let (lo, hi) = bounds(&f.set.parts[0]);
    assert_eq!((lo, hi), ([0.0, 0.0, 0.0], [30.0, 10.0, 0.0]));
    let p = load("patch_variants.step");
    accounted(&p);
    // Four patches are cut, the offset surface is skipped and named.
    assert_eq!(p.exact[0].as_ref().unwrap().faces.len(), 4);
    assert!(p.report.skipped.iter().any(|s| s.0 == "OFFSET_SURFACE"));
    assert!(
        p.report
            .skipped
            .iter()
            .any(|s| s.0 == "ADVANCED_FACE" && s.1.contains("OFFSET_SURFACE"))
    );
    assert!(
        p.report
            .tessellated
            .iter()
            .any(|t| t.0 == "B_SPLINE_SURFACE" && t.1 == 3)
    );
}

#[test]
fn every_kind_of_curve_gives_a_polyline_and_the_rest_is_reported() {
    let z = load("curve_zoo.step");
    accounted(&z);
    assert_eq!(z.curves.len(), 7);
    assert!(z.set.parts.is_empty());
    // The composite curve: a line from (0,0,0) to (10,0,0) and an arc to (10,10,0).
    let comp = &z.curves[0];
    assert_eq!(comp.points.first().unwrap(), &[0.0, 0.0, 0.0]);
    let last = comp.points.last().unwrap();
    assert!(
        (last[0] - 10.0).abs() < 1e-6 && (last[1] - 10.0).abs() < 1e-6,
        "{last:?}"
    );
    // Bezier, quasi uniform and uniform curves end where they should, or inside the points.
    assert!(
        z.curves[2].points.len() > 4
            && z.curves[3].points.len() > 4
            && z.curves[4].points.len() > 4
    );
    assert!(z.report.skipped.iter().any(|s| s.0 == "HYPERBOLA"));
    assert!(
        z.report
            .skipped
            .iter()
            .any(|s| s.0 == "LINE" && s.1.contains("without ends"))
    );
    // The ellipse trimmed backwards and the arc by parameter.
    assert!(z.curves[5].points.len() > 4 && z.curves[6].points.len() > 4);
    // A face whose edges are an arc, a polyline, a spline and a composite curve.
    let e = load("edge_zoo.step");
    accounted(&e);
    assert!(e.set.parts[0].triangles.len() > 10);
    let area: f64 = e.set.parts[0]
        .triangles
        .iter()
        .map(|t| {
            let q = |i: u32| DVec3::from(e.set.parts[0].positions[i as usize]);
            (q(t[1]) - q(t[0])).cross(q(t[2]) - q(t[0])).length() / 2.0
        })
        .sum();
    // The pentagon 20 x 10 plus a roof, with a bulge: between 300 and 400.
    assert!(area > 300.0 && area < 420.0, "{area}");
}

#[test]
fn tessellated_variants_use_the_point_index_strips_and_fans() {
    let t = load("tessellated_variants.step");
    accounted(&t);
    assert_eq!(t.exact, vec![None]);
    // 3 + 2 + (3 + 2) given triangles are taken, and a bad one is left out and counted.
    assert_eq!(t.set.parts[0].triangles.len(), 9);
    assert!(
        t.set
            .notes
            .iter()
            .any(|n| n.contains("do not exist") || n.contains("does not exist"))
    );
    assert!(
        t.report
            .skipped
            .iter()
            .any(|s| s.0 == "TESSELLATED_CURVE_SET")
    );
}

#[test]
fn styles_units_in_degrees_and_colors_by_name() {
    let s = load("styles.step");
    accounted(&s);
    // The box has the green of the style with a quarter of transparency.
    let colors: Vec<_> = s.set.parts.iter().map(|p| p.color).collect();
    assert!(colors.contains(&Some([0, 255, 0, 191])), "{colors:?}");
    // The cone with a half angle in degrees has the volume of a cone of that angle.
    let cone = s
        .set
        .parts
        .iter()
        .find(|p| p.name == "Styled" && p.color.is_none());
    let cone = cone.expect("a part without a color");
    let want = PI * 100.0 * 25.0 / 3.0;
    assert!(
        volume(cone) < want && volume(cone) > want * 0.97,
        "{} {want}",
        volume(cone)
    );
    // A quarter circle given in degrees: a polyline of a quarter of the circle.
    assert_eq!(s.curves.len(), 1);
    let pts = &s.curves[0].points;
    let a = pts.first().unwrap();
    let b = pts.last().unwrap();
    assert!(
        (a[0] - 10.0).abs() < 1e-6 && (b[1] - 10.0).abs() < 1e-6 && b[0].abs() < 1e-6,
        "{a:?} {b:?}"
    );
}

#[test]
fn mapped_items_and_related_representations_and_loose_solids() {
    let m = load("mapped.step");
    accounted(&m);
    assert_eq!(m.set.parts.len(), 2);
    let frame = m.set.parts.iter().find(|p| p.name == "Frame").unwrap();
    assert_eq!(frame.triangles.len(), 24);
    let (lo, hi) = bounds(frame);
    // One plate at x = 50, one at x = 100 turned a quarter: it extends along -x then.
    assert_eq!(lo[2], 0.0);
    assert!(
        lo[0] >= 40.0 - 1e-9 && hi[0] <= 110.0 + 1e-9,
        "{lo:?} {hi:?}"
    );
    let r = load("related.step");
    accounted(&r);
    assert_eq!(r.set.parts[0].name, "Related");
    assert!((volume(&r.set.parts[0]) - 6000.0).abs() < 1e-6);
    let l = load("loose.step");
    assert_eq!(l.set.parts.len(), 2);
    assert!(l.set.notes.iter().any(|n| n.contains("no length unit")));
    assert!(!l.report.unit_from_file);
}

#[test]
fn per_body_parts_and_a_tolerance_option_change_the_split() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/step/mapped.step");
    let bytes = std::fs::read(path).unwrap();
    let per_body = read(
        &bytes,
        &StepOptions {
            per_product: false,
            ..Default::default()
        },
    )
    .unwrap();
    // The two mapped plates are two bodies now.
    assert_eq!(per_body.set.parts.len(), 3);
    let coarse = read(
        &std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/step/cylinder.step"),
        )
        .unwrap(),
        &StepOptions {
            tolerance_mm: Some(2.0),
            angle_deg: 40.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(coarse.set.totals.triangles < 200);
    assert!((coarse.report.tolerance_mm.unwrap() - 2.0).abs() < 1e-9);
}

#[test]
fn a_bead_with_two_loops_on_a_sphere_and_edges_that_are_closed_curves() {
    let b = load("bead.step");
    accounted(&b);
    let p = &b.set.parts[0];
    // Between z = -6 and z = 6 on a ball of radius 12.
    let want = PI * 12.0 * (3.0 * 108.0 + 3.0 * 108.0 + 144.0) / 6.0;
    assert!(
        volume(p) < want * 1.001 && volume(p) > want * 0.98,
        "{} {want}",
        volume(p)
    );
    assert_eq!(open_edges(p), 0);
    // A rational circle, a closed polyline and a trimmed spline against its direction.
    let c = load("closed_edges.step");
    accounted(&c);
    let p = &c.set.parts[0];
    let area: f64 = p
        .triangles
        .iter()
        .map(|t| {
            let q = |i: u32| DVec3::from(p.positions[i as usize]);
            (q(t[1]) - q(t[0])).cross(q(t[2]) - q(t[0])).length() / 2.0
        })
        .sum();
    let want = 2.0 * PI * 100.0 + 100.0;
    assert!(area < want * 1.001 && area > want * 0.99, "{area} {want}");
}

#[test]
fn every_si_prefix_scales_the_numbers() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/step/box.step");
    let text = std::fs::read_to_string(path).unwrap();
    let cases = [
        ("EXA", 1e21),
        ("PETA", 1e18),
        ("TERA", 1e15),
        ("GIGA", 1e12),
        ("MEGA", 1e9),
        ("KILO", 1e6),
        ("HECTO", 1e5),
        ("DECA", 1e4),
        ("DECI", 1e2),
        ("CENTI", 10.0),
        ("MILLI", 1.0),
        ("MICRO", 1e-3),
        ("NANO", 1e-6),
        ("PICO", 1e-9),
        ("FEMTO", 1e-12),
        ("ATTO", 1e-15),
        ("WHATEVER", 1000.0),
    ];
    for (prefix, mm) in cases {
        let t = text.replace(".MILLI.,.METRE.", &format!(".{prefix}.,.METRE."));
        let r = read(t.as_bytes(), &StepOptions::default());
        // Huge boxes are refused for their size, the others have the size of the unit.
        match r {
            Ok(i) => {
                let (lo, hi) = bounds(&i.set.parts[0]);
                assert!(
                    ((hi[0] - lo[0]) / (20.0 * mm) - 1.0).abs() < 1e-9,
                    "{prefix}"
                );
            }
            // Boxes as small as a nanometer vanish in the weld. Huge ones are refused.
            Err(e) => assert!(
                (mm >= 1e9 || mm <= 1e-5) && !e.to_string().is_empty(),
                "{prefix}: {e}"
            ),
        }
    }
}
