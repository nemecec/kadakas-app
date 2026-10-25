//! Openings (`IfcRelVoidsElement`) are cut from their hosts: the volume is the gross volume less the
//! openings. When a cut is not possible, the gross geometry stays and the opening is an object.

use kadakas_core::command::Editor;
use kadakas_core::model::Document;

use super::tests_more::{extr, objs, run, slab, volume_of};
use super::*;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/ifc/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn a_wall_with_a_door_and_a_window_loses_their_volume() {
    let doc = Document::with_sequential_ids();
    let r = import_bytes(
        fixture("wall_with_openings.ifc"),
        &doc,
        &IfcImportOptions::default(),
    )
    .unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    let o = objs(&ed);
    assert_eq!(o.len(), 1, "{:?}", r.report);
    let gross = 4000.0 * 200.0 * 2800.0;
    let want = gross - 1000.0 * 200.0 * 2100.0 - 800.0 * 200.0 * 1200.0;
    assert!(
        (volume_of(o[0]) - want).abs() < 1.0,
        "{} {want}",
        volume_of(o[0])
    );
    assert!(
        r.report
            .notes
            .iter()
            .any(|n| n == "2 openings (IfcRelVoidsElement) were cut from their walls and slabs"),
        "{:?}",
        r.report.notes
    );
    // The openings are consumed by the cut: two products skipped with a line, one object.
    assert_eq!(r.report.entities_read, 3);
    assert_eq!(r.report.entities_imported, 1);
    assert_eq!(r.report.entities_skipped, 2);
    assert!(
        r.report
            .skipped
            .iter()
            .any(|s| s.entity == "IfcOpeningElement" && s.count == 2 && s.reason.contains("cut"))
    );
}

#[test]
fn a_slab_with_a_hole_and_a_blind_pocket() {
    // A 4 x 3 m slab, 200 thick; a hole 1 x 1 m through it and a pocket 0.5 x 0.5 m, 100 deep.
    let data = format!(
        "{}{}{}{}{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,4000.,3000.)", 200.0),
        slab(30, "#21"),
        extr(50, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1000.,1000.)", 400.0),
        "#60=IFCCARTESIANPOINT((0.,0.,-100.));\n#61=IFCAXIS2PLACEMENT3D(#60,$,$);\n#62=IFCEXTRUDEDAREASOLID(#50,#61,#4,400.);\n",
        "#63=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#62));\n#64=IFCPRODUCTDEFINITIONSHAPE($,$,(#63));\n#65=IFCOPENINGELEMENT('go',$,'Hole',$,$,#7,#64,$,.OPENING.);\n#66=IFCRELVOIDSELEMENT('gv',$,$,$,#30,#65);\n",
        "#70=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,500.,500.);\n#71=IFCCARTESIANPOINT((1200.,0.,100.));\n#72=IFCAXIS2PLACEMENT3D(#71,$,$);\n#73=IFCEXTRUDEDAREASOLID(#70,#72,#4,300.);\n#74=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#73));\n#75=IFCPRODUCTDEFINITIONSHAPE($,$,(#74));\n#76=IFCOPENINGELEMENT('gp',$,'Pocket',$,$,#7,#75,$,.OPENING.);\n#77=IFCRELVOIDSELEMENT('gq',$,$,$,#30,#76);\n"
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    let o = objs(&ed);
    assert_eq!(o.len(), 1, "{:?}", r);
    let want = 4000.0 * 3000.0 * 200.0 - 1000.0 * 1000.0 * 200.0 - 500.0 * 500.0 * 100.0;
    assert!(
        (volume_of(o[0]) - want).abs() < 1.0,
        "{} {want}",
        volume_of(o[0])
    );
}

#[test]
fn an_opening_that_misses_the_host_changes_nothing() {
    let data = format!(
        "{}{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1000.,1000.)", 200.0),
        slab(30, "#21"),
        "#50=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#51=IFCCARTESIANPOINT((9000.,9000.,0.));\n#52=IFCAXIS2PLACEMENT3D(#51,$,$);\n#53=IFCEXTRUDEDAREASOLID(#50,#52,#4,100.);\n#54=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#53));\n#55=IFCPRODUCTDEFINITIONSHAPE($,$,(#54));\n#56=IFCOPENINGELEMENT('fo',$,'Far',$,$,#7,#55,$,.OPENING.);\n#57=IFCRELVOIDSELEMENT('fv',$,$,$,#30,#56);\n"
    );
    let (ed, _) = run(&data, &IfcImportOptions::default());
    assert!((volume_of(objs(&ed)[0]) - 1000.0 * 1000.0 * 200.0).abs() < 1.0);
}

#[test]
fn a_reference_representation_is_not_subtracted() {
    let data = format!(
        "{}{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1000.,1000.)", 200.0),
        slab(30, "#21"),
        "#50=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#52=IFCAXIS2PLACEMENT3D(#6,$,$);\n#53=IFCEXTRUDEDAREASOLID(#50,#52,#4,500.);\n#54=IFCSHAPEREPRESENTATION($,'Reference','SweptSolid',(#53));\n#55=IFCPRODUCTDEFINITIONSHAPE($,$,(#54));\n#56=IFCOPENINGELEMENT('ro',$,'Ref',$,$,#7,#55,$,.OPENING.);\n#57=IFCRELVOIDSELEMENT('rv',$,$,$,#30,#56);\n"
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1);
    assert!((volume_of(objs(&ed)[0]) - 1000.0 * 1000.0 * 200.0).abs() < 1.0);
    assert!(r.skipped.iter().any(|s| {
        s.reason
            .contains("Reference representation is not subtracted")
    }));
}

#[test]
fn a_host_that_cannot_be_cut_keeps_its_gross_shape_and_the_opening_becomes_an_object() {
    // The host is one triangle (an open surface): no solid. The opening stays as a separate object.
    let data = "#20=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1000.,0.,0.),(0.,1000.,0.)));\n#21=IFCTRIANGULATEDFACESET(#20,$,.F.,((1,2,3)),$);\n#50=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#52=IFCAXIS2PLACEMENT3D(#6,$,$);\n#53=IFCEXTRUDEDAREASOLID(#50,#52,#4,500.);\n#54=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#53));\n#55=IFCPRODUCTDEFINITIONSHAPE($,$,(#54));\n#56=IFCOPENINGELEMENT('xo',$,'Hole',$,$,#7,#55,$,.OPENING.);\n#57=IFCRELVOIDSELEMENT('xv',$,$,$,#30,#56);\n".to_string()
        + &slab(30, "#21");
    let (ed, r) = run(&data, &IfcImportOptions::default());
    let o = objs(&ed);
    assert_eq!(o.len(), 2, "{r:?}");
    assert!(o.iter().any(|x| x.data["ifc_class"] == "IfcOpeningElement"));
    assert!(
        r.notes.iter().any(
            |n| n.contains("1 openings kept as objects") && n.contains("a solid is not closed")
        ),
        "{:?}",
        r.notes
    );
}

fn opening_at_origin(n: u64, host: u64) -> String {
    format!(
        "#{a}=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#{b}=IFCEXTRUDEDAREASOLID(#{a},#5,#4,500.);\n#{c}=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{b}));\n#{d}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{c}));\n#{e}=IFCOPENINGELEMENT('o{n}',$,'Hole {n}',$,$,#7,#{d},$,.OPENING.);\n#{f}=IFCRELVOIDSELEMENT('v{n}',$,$,$,#{host},#{e});\n",
        a = n,
        b = n + 1,
        c = n + 2,
        d = n + 3,
        e = n + 4,
        f = n + 5,
    )
}

#[test]
fn an_opening_cut_from_one_piece_but_not_from_another_is_reported_as_cut_in_part() {
    // The slab has two pieces: a solid and an open surface. The hole reaches both.
    let data = format!(
        "{}#40=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1000.,0.,0.),(0.,1000.,0.)));\n#41=IFCTRIANGULATEDFACESET(#40,$,.F.,((1,2,3)),$);\n{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,200.)", 100.0),
        slab(30, "#21,#41"),
        opening_at_origin(50, 30)
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    // The hole is not an object of its own: it was cut from the solid piece.
    assert!(
        objs(&ed)
            .iter()
            .all(|o| o.data["ifc_class"] != "IfcOpeningElement"),
        "{r:?}"
    );
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("1 openings cut from some parts only")
                && n.contains("a solid is not closed")
                && n.contains("IfcSlab 'N30'")),
        "{:?}",
        r.notes
    );
}

#[test]
fn a_used_up_work_budget_keeps_the_gross_shape_and_counts_the_openings() {
    let doc = Document::with_sequential_ids();
    let opts = IfcImportOptions {
        boolean_work: 1,
        ..Default::default()
    };
    let r = import_bytes(fixture("wall_with_openings.ifc"), &doc, &opts).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    assert_eq!(
        objs(&ed).len(),
        3,
        "the wall and its two openings: {:?}",
        r.report
    );
    let wall = objs(&ed)
        .into_iter()
        .find(|o| o.data["ifc_class"] == "IfcWall")
        .unwrap();
    assert!((volume_of(wall) - 4000.0 * 200.0 * 2800.0).abs() < 1.0);
    assert!(
        r.report.notes.iter().any(|n| n
            .contains("IfcWall 'Garden wall': 2 openings kept as objects")
            && n.contains("work budget")),
        "{:?}",
        r.report.notes
    );
}

#[test]
fn many_walls_that_fail_the_same_way_give_one_note() {
    let mut data = String::new();
    for i in 0..6u64 {
        let base = 100 + i * 100;
        data.push_str(&format!(
            "#{p}=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1000.,0.,0.),(0.,1000.,0.)));\n#{q}=IFCTRIANGULATEDFACESET(#{p},$,.F.,((1,2,3)),$);\n{}{}",
            slab(base + 10, &format!("#{}", base + 1)),
            opening_at_origin(base + 50, base + 10),
            p = base,
            q = base + 1
        ));
    }
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 12);
    let lines: Vec<&String> = r
        .notes
        .iter()
        .filter(|n| n.contains("kept as objects"))
        .collect();
    assert_eq!(lines.len(), 1, "{:?}", r.notes);
    assert!(
        lines[0].starts_with("6 hosts (IfcSlab 'N110'"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains("6 openings kept as objects"),
        "{}",
        lines[0]
    );
}

/// A ring wall: the profile with its void, 3000 high, and a 400 x 600 x 1000 opening through the left wall.
fn ring_wall(profile: &str) -> String {
    format!(
        "{}{}#60=IFCCARTESIANPOINT((-1900.,0.,1000.));\n#61=IFCAXIS2PLACEMENT3D(#60,$,$);\n#62=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,400.,600.);\n#63=IFCEXTRUDEDAREASOLID(#62,#61,#4,1000.);\n\
#64=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#63));\n#65=IFCPRODUCTDEFINITIONSHAPE($,$,(#64));\n#66=IFCOPENINGELEMENT('ro',$,'Door',$,$,#7,#65,$,.OPENING.);\n#67=IFCRELVOIDSELEMENT('rv',$,$,$,#30,#66);\n",
        extr(20, profile, 3000.0),
        slab(30, "#21")
    )
}

#[test]
fn walls_with_a_void_in_their_profile_are_closed_and_take_their_openings() {
    let want = (4000.0 * 2000.0 - 3600.0 * 1600.0) * 3000.0 - 200.0 * 600.0 * 1000.0;
    let (ed, r) = run(
        &ring_wall("IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,$,4000.,2000.,200.)"),
        &IfcImportOptions::default(),
    );
    assert_eq!(objs(&ed).len(), 1, "no opening object: {r:?}");
    assert!(
        (volume_of(objs(&ed)[0]) - want).abs() < 1.0,
        "{}",
        volume_of(objs(&ed)[0])
    );
    assert!(
        r.notes.iter().any(|n| n.starts_with("1 openings")),
        "{:?}",
        r.notes
    );
    // A hollow circle: an outer radius 2500 and a wall of 200, cut by the same box (its two faces
    // are curved there, so the volume is the polygon one).
    let (ed, r) = run(
        &ring_wall("IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,$,2500.,200.)"),
        &IfcImportOptions::default(),
    );
    assert_eq!(objs(&ed).len(), 1, "no opening object: {r:?}");
    let ring = std::f64::consts::PI * (2500.0f64.powi(2) - 2300.0f64.powi(2)) * 3000.0;
    let v = volume_of(objs(&ed)[0]);
    assert!(v < ring && v > ring - 2.0e8, "{v} {ring}");
}
