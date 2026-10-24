//! The STEP test files of tests/data/step (made by make_step_files.py): volumes and areas
//! against the analytic values, closedness, units, assemblies, colors and the report.

use std::f64::consts::PI;
use std::path::PathBuf;

use kadakas_step::{StepImport, StepOptions, read};

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data/step")
        .join(name)
}

fn load(name: &str, opts: &StepOptions) -> StepImport {
    let bytes = std::fs::read(path(name)).unwrap();
    read(&bytes, opts).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn load_default(name: &str) -> StepImport {
    load(name, &StepOptions::default())
}

/// Signed volume, area and the number of edges that are not shared by exactly two triangles.
fn metrics(imp: &StepImport) -> (f64, f64, usize) {
    let mut vol = 0.0;
    let mut area = 0.0;
    let mut open = 0;
    for p in &imp.set.parts {
        let mut edges = std::collections::HashMap::<(u32, u32), i32>::new();
        for t in &p.triangles {
            let v: Vec<glam::DVec3> = t.iter().map(|i| glam::DVec3::from(p.positions[*i as usize])).collect();
            vol += v[0].dot(v[1].cross(v[2])) / 6.0;
            area += (v[1] - v[0]).cross(v[2] - v[0]).length() / 2.0;
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
            }
        }
        // A closed, consistently turned surface: every edge is used once in each direction.
        let mut count = std::collections::HashMap::<(u32, u32), i32>::new();
        for t in &p.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *count.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        open += count.iter().filter(|(k, n)| **n != 2 || edges[*k] != 0).count();
    }
    (vol, area, open)
}

fn close(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * b.abs(), "{a} is not within {rel} of {b}");
}

fn accounted(imp: &StepImport) {
    let r = &imp.report;
    assert_eq!(r.entities_read, r.entities_used + r.entities_skipped, "{r:?}");
}

#[test]
fn box_is_exact_and_closed() {
    let imp = load_default("box.step");
    accounted(&imp);
    let (vol, area, open) = metrics(&imp);
    close(vol, 20.0 * 30.0 * 40.0, 1e-9);
    close(area, 2.0 * (20.0 * 30.0 + 20.0 * 40.0 + 30.0 * 40.0), 1e-9);
    assert_eq!(open, 0);
    assert_eq!(imp.set.parts.len(), 1);
    assert_eq!(imp.set.parts[0].name, "Box");
    assert_eq!(imp.set.parts[0].color, Some([255, 0, 0, 255]));
    assert_eq!(imp.set.parts[0].triangles.len(), 12);
    assert_eq!(imp.set.parts[0].positions.len(), 8);
}

#[test]
fn units_are_applied() {
    let inch = load_default("box_inch.step");
    close(metrics(&inch).0, 25.4 * 50.8 * 76.2, 1e-9);
    let m = load_default("box_metre.step");
    close(metrics(&m).0, 20.0 * 30.0 * 40.0, 1e-9);
    // The option overrides the unit of the file.
    let o = StepOptions {
        unit: Some(kadakas_meshio::Unit::Centimetre),
        ..Default::default()
    };
    close(metrics(&load("box.step", &o)).0, 200.0 * 300.0 * 400.0, 1e-9);
}

#[test]
fn cylinder_matches_within_tessellation_error() {
    let imp = load_default("cylinder.step");
    accounted(&imp);
    let (vol, area, open) = metrics(&imp);
    assert_eq!(open, 0, "{:?}", imp.report);
    // Inscribed polygons are a little smaller.
    let want_v = PI * 100.0 * 25.0;
    assert!(vol < want_v && vol > want_v * 0.99, "{vol} {want_v}");
    let want_a = 2.0 * PI * 10.0 * 25.0 + 2.0 * PI * 100.0;
    assert!(area < want_a && area > want_a * 0.99, "{area} {want_a}");
}

#[test]
fn sphere_hemisphere_and_torus() {
    let h = load_default("hemisphere.step");
    accounted(&h);
    let (vol, _, open) = metrics(&h);
    assert_eq!(open, 0, "{:?}", h.report);
    let want = 2.0 / 3.0 * PI * 12f64.powi(3);
    assert!(vol < want && vol > want * 0.97, "{vol} {want}");
    let s = load_default("sphere.step");
    let (vol, area, open) = metrics(&s);
    let want = 4.0 / 3.0 * PI * 12f64.powi(3);
    assert!(vol < want && vol > want * 0.97, "{vol} {want}");
    close(area, 4.0 * PI * 144.0, 0.04);
    assert_eq!(open, 0, "{:?}", s.report);
    let t = load_default("torus.step");
    let (vol, area, open) = metrics(&t);
    let want = 2.0 * PI * PI * 20.0 * 25.0;
    assert!(vol < want && vol > want * 0.97, "{vol} {want}");
    close(area, 4.0 * PI * PI * 20.0 * 5.0, 0.04);
    assert_eq!(open, 0, "{:?}", t.report);
}

#[test]
fn cone_revolution_and_extrusion() {
    let want_frustum = PI * 20.0 / 3.0 * (144.0 + 72.0 + 36.0);
    for f in ["frustum.step", "revolved.step"] {
        let imp = load_default(f);
        accounted(&imp);
        let (vol, _, open) = metrics(&imp);
        assert!(vol < want_frustum && vol > want_frustum * 0.98, "{f} {vol} {want_frustum}");
        assert_eq!(open, 0, "{f} {:?}", imp.report);
    }
    let e = load_default("extrusion.step");
    let (vol, _, open) = metrics(&e);
    let want = PI * 15.0 * 8.0 * 10.0;
    assert!(vol < want && vol > want * 0.98, "{vol} {want}");
    assert_eq!(open, 0, "{:?}", e.report);
}

#[test]
fn plate_with_a_hole() {
    let imp = load_default("plate_hole.step");
    accounted(&imp);
    let (vol, _, open) = metrics(&imp);
    let want = 60.0 * 40.0 * 8.0 - PI * 36.0 * 8.0;
    // The hole polygon is inscribed, so the volume is a little larger.
    assert!(vol > want && vol < want * 1.01, "{vol} {want}");
    assert_eq!(open, 0, "{:?}", imp.report);
}

#[test]
fn patches() {
    let q = load_default("patch_rational.step");
    accounted(&q);
    let (_, area, _) = metrics(&q);
    let want = PI / 2.0 * 10.0 * 20.0;
    assert!(area < want && area > want * 0.99, "{area} {want}");
    let flat = load_default("patch_flat.step");
    close(metrics(&flat).1, 600.0, 1e-6);
    let bump = load_default("patch_bump.step");
    assert!(metrics(&bump).1 > 900.0);
    assert!(bump.set.totals.triangles > 20);
}

#[test]
fn faceted_tessellated_and_curves() {
    let t = load_default("faceted.step");
    close(metrics(&t).0, 1000.0 / 6.0, 1e-9);
    assert_eq!(metrics(&t).2, 0);
    let ts = load_default("tessellated.step");
    accounted(&ts);
    close(metrics(&ts).0, 10.0 * 20.0 * 30.0, 1e-9);
    assert_eq!(metrics(&ts).2, 0);
    let c = load_default("curves.step");
    assert_eq!(c.curves.len(), 3);
    assert_eq!(c.curves[0].points.len(), 3);
    assert!(c.curves[2].closed && !c.curves[1].closed);
    assert!(c.set.parts.is_empty());
}

#[test]
fn assembly_places_two_instances() {
    let imp = load_default("assembly.step");
    accounted(&imp);
    assert_eq!(imp.report.instances, 2);
    assert_eq!(imp.set.parts.len(), 2);
    for p in &imp.set.parts {
        assert_eq!(p.name, "Leg");
        assert_eq!(p.color, Some([0, 0, 255, 255]));
    }
    let bounds = |i: usize| {
        let p = &imp.set.parts[i];
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &p.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        (lo, hi)
    };
    let (lo, hi) = bounds(0);
    assert_eq!((lo, hi), ([100.0, 0.0, 0.0], [110.0, 10.0, 50.0]));
    // The second leg is turned 90 degrees about Z: x -> y, y -> -x.
    let (lo, hi) = bounds(1);
    assert_eq!((lo, hi), ([-10.0, 200.0, 0.0], [0.0, 210.0, 50.0]));
}
