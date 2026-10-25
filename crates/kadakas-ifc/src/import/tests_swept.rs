//! Revolved solids, swept disks, CSG solids and advanced breps: volumes against the analytic values.

use std::f64::consts::PI;

use kadakas_core::command::Editor;
use kadakas_core::model::Document;

use super::tests_more::{extr, objs, product, run, volume_of};
use super::*;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/ifc/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn one(data: &str) -> f64 {
    let (ed, r) = run(data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
    volume_of(objs(&ed)[0])
}

fn close(got: f64, want: f64, rel: f64) -> bool {
    ((got - want) / want).abs() < rel
}

fn proxy(n: u64, item: &str) -> String {
    product(n, "IFCBUILDINGELEMENTPROXY", item, "$,@,$,$")
}

#[test]
fn a_revolved_area_in_degrees() {
    // A 100 x 50 rectangle (x from 200 to 300) turned 90 degrees about the Y axis of the profile.
    let data = format!(
        "#20=IFCCARTESIANPOINT((250.,25.));\n#21=IFCAXIS2PLACEMENT2D(#20,$);\n#22=IFCRECTANGLEPROFILEDEF(.AREA.,$,#21,100.,50.);\n\
#23=IFCCARTESIANPOINT((0.,0.,0.));\n#24=IFCDIRECTION((0.,1.,0.));\n#25=IFCAXIS1PLACEMENT(#23,#24);\n#26=IFCREVOLVEDAREASOLID(#22,#5,#25,90.);\n{}",
        proxy(30, "#26")
    );
    let want = PI / 4.0 * (300.0f64.powi(2) - 200.0f64.powi(2)) * 50.0;
    let v = one(&data);
    assert!(close(v, want, 0.01), "{v} {want}");
}

#[test]
fn a_swept_disk_along_a_polyline_is_a_pipe() {
    let data = format!(
        "#20=IFCCARTESIANPOINT((0.,0.,0.));\n#21=IFCCARTESIANPOINT((1000.,0.,0.));\n#22=IFCCARTESIANPOINT((1000.,800.,600.));\n#23=IFCPOLYLINE((#20,#21,#22));\n\
#24=IFCSWEPTDISKSOLID(#23,25.,$,$,$);\n{}",
        proxy(30, "#24")
    );
    let len = 1000.0 + (800.0f64.powi(2) + 600.0f64.powi(2)).sqrt();
    let v = one(&data);
    assert!(close(v, PI * 625.0 * len, 0.03), "{v}");
    // A hollow pipe with a trimmed end.
    let hollow = format!(
        "#20=IFCCARTESIANPOINT((0.,0.,0.));\n#21=IFCCARTESIANPOINT((1000.,0.,0.));\n#23=IFCPOLYLINE((#20,#21));\n#24=IFCSWEPTDISKSOLID(#23,25.,15.,0.,0.5);\n{}",
        proxy(30, "#24")
    );
    let v = one(&hollow);
    assert!(close(v, PI * (625.0 - 225.0) * 500.0, 0.03), "{v}");
}

#[test]
fn csg_primitives_and_booleans_of_them() {
    // A block 2000 x 800 x 800 less a cylinder (radius 200, height 1000) through its middle, and a sphere.
    let data = format!(
        "#20=IFCCARTESIANPOINT((0.,0.,0.));\n#21=IFCAXIS2PLACEMENT3D(#20,$,$);\n#22=IFCBLOCK(#21,2000.,800.,800.);\n\
#23=IFCCARTESIANPOINT((1000.,400.,-100.));\n#24=IFCAXIS2PLACEMENT3D(#23,$,$);\n#25=IFCRIGHTCIRCULARCYLINDER(#24,1000.,200.);\n\
#26=IFCBOOLEANRESULT(.DIFFERENCE.,#22,#25);\n#27=IFCCSGSOLID(#26);\n{}",
        proxy(30, "#27")
    );
    let want = 2000.0 * 800.0 * 800.0 - PI * 200.0 * 200.0 * 800.0;
    let v = one(&data);
    assert!(close(v, want, 0.03), "{v} {want}");
    // A sphere, a cone and a pyramid on their own.
    let sphere = format!(
        "#20=IFCSPHERE(#5,300.);\n#21=IFCCSGSOLID(#20);\n{}",
        proxy(30, "#21")
    );
    assert!(close(one(&sphere), 4.0 / 3.0 * PI * 300.0f64.powi(3), 0.03));
    let cone = format!(
        "#20=IFCRIGHTCIRCULARCONE(#5,600.,200.);\n{}",
        proxy(30, "#20")
    );
    assert!(close(one(&cone), PI * 200.0 * 200.0 * 600.0 / 3.0, 0.03));
    let pyr = format!(
        "#20=IFCRECTANGULARPYRAMID(#5,300.,200.,450.);\n{}",
        proxy(30, "#20")
    );
    assert!(close(one(&pyr), 300.0 * 200.0 * 450.0 / 3.0, 1e-6));
    // The union and the intersection of two blocks.
    let two = |op: &str| {
        format!(
            "#20=IFCBLOCK(#5,100.,100.,100.);\n#21=IFCCARTESIANPOINT((50.,50.,50.));\n#22=IFCAXIS2PLACEMENT3D(#21,$,$);\n#23=IFCBLOCK(#22,100.,100.,100.);\n#24=IFCBOOLEANRESULT(.{op}.,#20,#23);\n{}",
            proxy(30, "#24")
        )
    };
    assert!(close(one(&two("UNION")), 2.0e6 - 125000.0, 1e-6));
    assert!(close(one(&two("INTERSECTION")), 125000.0, 1e-6));
    assert!(close(one(&two("DIFFERENCE")), 1.0e6 - 125000.0, 1e-6));
}

#[test]
fn a_boolean_difference_of_extrusions_cuts_a_notch() {
    let data = format!(
        "{}{}{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,400.,400.)", 300.0),
        extr(30, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 400.0),
        "#40=IFCBOOLEANRESULT(.DIFFERENCE.,#21,#31);\n",
        proxy(50, "#40")
    );
    assert!(close(
        one(&data),
        400.0 * 400.0 * 300.0 - 100.0 * 100.0 * 300.0,
        1e-6
    ));
}

fn load(name: &str) -> (Editor, kadakas_io::Report) {
    let doc = Document::with_sequential_ids();
    let r = import_bytes(fixture(name), &doc, &IfcImportOptions::default()).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    (ed, r.report)
}

#[test]
fn an_advanced_brep_with_planar_faces() {
    let (ed, r) = load("advanced_brep_box.ifc");
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
    assert!(
        close(volume_of(objs(&ed)[0]), 400.0 * 300.0 * 200.0, 1e-9),
        "{}",
        volume_of(objs(&ed)[0])
    );
    assert!(r.skipped.is_empty() && r.notes.is_empty(), "{r:?}");
    // The exact surfaces stay with the triangles: 6 planes, in millimeters, in step with the mesh.
    let kadakas_core::model::Shape::Mesh3d(m) = &objs(&ed)[0].shape else {
        panic!()
    };
    assert_eq!(m.exact_state(), Some(kadakas_core::exact::State::Fresh));
    let exact = m.exact.as_ref().unwrap();
    assert_eq!(exact.faces.len(), 6);
    assert_eq!(exact.vertices.len(), 8);
    let hi = exact
        .vertices
        .iter()
        .fold(glam::DVec3::NEG_INFINITY, |a, v| a.max(*v));
    assert_eq!(hi, glam::DVec3::new(400.0, 300.0, 200.0));
}

#[test]
fn an_advanced_brep_cylinder_with_a_seam() {
    let (ed, r) = load("advanced_brep_cylinder.ifc");
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
    let v = volume_of(objs(&ed)[0]);
    assert!(close(v, PI * 100.0 * 100.0 * 300.0, 0.04), "{v}");
    assert!(r.skipped.is_empty(), "{r:?}");
}

#[test]
fn a_brep_with_a_face_that_cannot_be_cut_is_kept_with_a_note() {
    // Take the box and break a face: its surface is a type that is not supported.
    let text = String::from_utf8(fixture("advanced_brep_box.ifc"))
        .unwrap()
        .replacen("IFCPLANE(", "IFCSURFACEOFREVOLUTION(", 1);
    let doc = Document::with_sequential_ids();
    let r = import_bytes(text.into_bytes(), &doc, &IfcImportOptions::default()).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    assert_eq!(objs(&ed).len(), 1);
    assert!(
        r.report
            .notes
            .iter()
            .any(|n| n.contains("1 faces of an advanced brep were left out")),
        "{:?}",
        r.report
    );
}
