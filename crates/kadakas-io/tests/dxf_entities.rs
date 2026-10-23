//! Entities that real CAD files hold and the importer used to skip (IOP-14, IOP-15): WIPEOUT,
//! SOLID, MESH, DIMENSION, LEADER, IMAGE, ACIS solids, hatches, text codes, splines and the
//! entities in a tilted plane. Every entity is imported or has a line in `report.skipped`.

mod support;

use kadakas_core::model::{Color, Shape};
use kadakas_core::{DVec2, DVec3};
use kadakas_io::{ImportOptions, Report};
use support::{import, import_with};

/// The accounting rule: no entity is lost without a line in the report.
fn accounted(rep: &Report) {
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
    assert!(rep.entities_skipped <= rep.skipped_total(), "{rep:?}");
}

fn doc(entities: &str) -> String {
    support::dxf("9\n$INSUNITS\n70\n4\n9\n$ACADVER\n1\nAC1027\n", entities)
}

fn with_blocks(blocks: &str, entities: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

fn shapes_of(ed: &kadakas_core::command::Editor) -> Vec<Shape> {
    ed.doc().objects.values().map(|o| o.shape.clone()).collect()
}

#[test]
fn a_wipeout_covers_a_hatch_and_keeps_its_order() {
    let ents = "0\nHATCH\n8\nH\n70\n1\n62\n1\n91\n1\n92\n7\n72\n0\n73\n1\n93\n4\n10\n0\n20\n0\n10\n100\n20\n0\n10\n100\n20\n100\n10\n0\n20\n100\n97\n0\n\
0\nWIPEOUT\n8\nW\n10\n40\n20\n40\n30\n0\n11\n20\n21\n0\n31\n0\n12\n0\n22\n20\n32\n0\n13\n1\n23\n1\n71\n1\n91\n2\n14\n-0.5\n24\n0.5\n14\n0.5\n24\n-0.5\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(rep.entities_read, 2);
    let objs: Vec<_> = ed.doc().objects.values().collect();
    assert_eq!(objs.len(), 2);
    // The hatch comes first, the wipeout above it, white, with no line.
    assert!(matches!(
        objs[0].shape,
        Shape::Polyline { closed: true, .. }
    ));
    assert_eq!(objs[0].attrs.fill, Some(Color([255, 0, 0, 255])));
    assert_eq!(objs[1].attrs.fill, Some(Color([255, 255, 255, 255])));
    assert_eq!(objs[1].attrs.pen, Some(Color::NONE));
    assert_eq!(objs[1].data["wipeout"], true);
    let Shape::Polyline { points, .. } = &objs[1].shape else {
        panic!()
    };
    let (lo, hi) = points
        .iter()
        .fold((DVec2::splat(1e9), DVec2::splat(-1e9)), |(lo, hi), v| {
            (lo.min(v.p), hi.max(v.p))
        });
    assert!(
        (lo - DVec2::new(40.0, 40.0)).length() < 1e-9
            && (hi - DVec2::new(60.0, 60.0)).length() < 1e-9
    );
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("1 WIPEOUT became polygons")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn solid_and_trace_are_filled_with_the_pen_color() {
    let ents = "0\nSOLID\n8\nA\n62\n3\n10\n0\n20\n0\n11\n10\n21\n0\n12\n0\n22\n10\n13\n10\n23\n10\n\
0\nTRACE\n8\nA\n10\n20\n20\n0\n11\n30\n21\n0\n12\n20\n22\n5\n13\n20\n23\n5\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    let o: Vec<_> = ed.doc().objects.values().collect();
    assert_eq!(o.len(), 2);
    assert_eq!(o[0].attrs.fill, Some(Color([0, 255, 0, 255])));
    let Shape::Polyline { points, closed } = &o[0].shape else {
        panic!()
    };
    assert!(*closed);
    // The order 1, 2, 4, 3 makes a square, not a bow tie.
    let p: Vec<_> = points.iter().map(|v| (v.p.x, v.p.y)).collect();
    assert_eq!(p, vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    // The TRACE repeats a corner: a triangle.
    assert!(matches!(&o[1].shape, Shape::Polyline { points, .. } if points.len() == 3));
}

#[test]
fn a_mesh_entity_becomes_a_mesh() {
    let ents = "0\nMESH\n8\nM\n71\n2\n72\n0\n91\n0\n92\n4\n10\n0\n20\n0\n30\n0\n10\n10\n20\n0\n30\n0\n10\n10\n20\n10\n30\n0\n10\n0\n20\n10\n30\n3\n93\n5\n90\n4\n90\n0\n90\n1\n90\n2\n90\n3\n94\n0\n95\n0\n90\n0\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    let s = shapes_of(&ed);
    assert_eq!(s.len(), 1);
    let Shape::Mesh3d(m) = &s[0] else { panic!() };
    assert_eq!((m.vertices.len(), m.triangles.len()), (4, 2));
    assert_eq!(m.vertices[3], DVec3::new(0.0, 10.0, 3.0));
    assert_eq!(rep.objects.get("mesh3d"), Some(&1));
}

#[test]
fn a_dimension_is_drawn_from_its_block() {
    let blocks = "0\nBLOCK\n8\n0\n2\n*D1\n70\n1\n10\n0\n20\n0\n30\n0\n\
0\nLINE\n8\n0\n10\n0\n20\n5\n11\n100\n21\n5\n\
0\nLINE\n8\n0\n10\n0\n20\n0\n11\n0\n21\n8\n\
0\nMTEXT\n8\n0\n10\n50\n20\n7\n40\n2.5\n1\n100\n\
0\nENDBLK\n";
    let ents = "0\nDIMENSION\n8\nDims\n2\n*D1\n70\n32\n10\n50\n20\n5\n13\n0\n23\n0\n14\n100\n24\n0\n\
0\nDIMENSION\n8\nDims\n2\n*D9\n70\n32\n";
    let (ed, rep) = import(with_blocks(blocks, ents).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(rep.entities_read, 5, "{rep:?}");
    assert_eq!(rep.objects.get("line"), Some(&2));
    assert_eq!(rep.objects.get("text"), Some(&1));
    // The geometry is on the layer of the dimension, because the block entities were on layer 0.
    let classes: Vec<_> = ed
        .doc()
        .objects
        .values()
        .filter_map(|o| ed.doc().class(o.class).map(|c| c.name.clone()))
        .collect();
    assert!(classes.iter().all(|c| c == "Dims"), "{classes:?}");
    // A dimension without its block is reported, not lost.
    let s = rep
        .skipped
        .iter()
        .find(|s| s.entity == "DIMENSION")
        .expect("skipped");
    assert_eq!(s.count, 1);
    assert!(s.reason.contains("no geometry block"));
}

#[test]
fn a_leader_is_a_polyline_and_says_so() {
    let ents = "0\nLEADER\n8\nL\n76\n3\n10\n0\n20\n0\n30\n0\n10\n50\n20\n50\n30\n0\n10\n90\n20\n50\n30\n0\n71\n1\n72\n0\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    assert!(
        matches!(&shapes_of(&ed)[0], Shape::Polyline { points, closed: false } if points.len() == 3)
    );
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("1 LEADER was imported as lines")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn images_solids_and_multileaders_are_reported_by_name() {
    let objects = "0\nSECTION\n2\nOBJECTS\n0\nIMAGEDEF\n5\n2A\n1\nphotos/site.png\n0\nENDSEC\n";
    let text = format!(
        "0\nSECTION\n2\nENTITIES\n0\nIMAGE\n8\nI\n340\n2A\n\
0\n3DSOLID\n8\nS\n\
0\nBODY\n8\nS\n\
0\nSURFACE\n8\nS\n\
0\nMULTILEADER\n8\nS\n\
0\nXLINE\n8\nS\n10\n0\n20\n0\n11\n1\n21\n0\n\
0\nLINE\n8\nS\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n{objects}0\nEOF\n"
    );
    let (_, rep) = import(text.as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(rep.total(), 1);
    assert_eq!(rep.entities_skipped, 6);
    let reason = |e: &str| {
        rep.skipped
            .iter()
            .find(|s| s.entity == e)
            .unwrap()
            .reason
            .clone()
    };
    assert!(reason("IMAGE").contains("'photos/site.png'"));
    assert!(reason("3DSOLID").contains("ACIS"));
    assert!(reason("BODY").contains("ACIS"));
    assert!(reason("SURFACE").contains("ACIS"));
    assert!(reason("MULTILEADER").contains("multileader"));
    assert!(reason("XLINE").contains("infinite"));
}

#[test]
fn text_codes_are_read() {
    let ents = "0\nTEXT\n8\nT\n10\n0\n20\n0\n40\n5\n1\n90%%176 %%c20 %%p1 %%uunder%%u^Jnext\n\
0\nMTEXT\n8\nT\n10\n0\n20\n10\n40\n5\n1\n{\\fArial|b0;\\H2x;A}\\pxqc;B\\P\\S1/2;%%d\\U+00E4\n\
0\nATTRIB\n8\nT\n10\n0\n20\n20\n40\n5\n1\nTag value\n2\nTAG\n70\n0\n\
0\nATTRIB\n8\nT\n10\n0\n20\n30\n40\n5\n1\nhidden\n2\nTAG\n70\n1\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    let texts: Vec<String> = shapes_of(&ed)
        .into_iter()
        .map(|s| match s {
            Shape::Text { text, .. } => text,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(texts[0], "90\u{b0} \u{d8}20 \u{b1}1 under\nnext");
    assert_eq!(texts[1], "AB\n1/2\u{b0}\u{e4}");
    assert_eq!(texts[2], "Tag value");
    assert_eq!(texts.len(), 3);
    assert_eq!(rep.skipped[0].reason, "invisible attribute");
}

#[test]
fn hatches_are_imported_with_their_islands_and_the_patterns_are_reported() {
    let outer = "92\n7\n72\n0\n73\n1\n93\n4\n10\n0\n20\n0\n10\n100\n20\n0\n10\n100\n20\n100\n10\n0\n20\n100\n97\n0\n";
    let inner = "92\n7\n72\n0\n73\n1\n93\n4\n10\n40\n20\n40\n10\n60\n20\n40\n10\n60\n20\n60\n10\n40\n20\n60\n97\n0\n";
    let ents = format!(
        "0\nHATCH\n8\nH\n2\nSOLID\n70\n1\n62\n5\n91\n2\n{outer}{inner}\
0\nHATCH\n8\nH\n2\nANSI31\n70\n0\n91\n1\n{outer}\
0\nHATCH\n8\nH\n2\nANSI31\n70\n0\n91\n1\n{outer}\
0\nHATCH\n8\nH\n2\nNET\n70\n0\n91\n1\n{outer}\
0\nHATCH\n8\nH\n2\nLINEAR\n70\n1\n450\n1\n421\n255\n91\n1\n{outer}"
    );
    let (ed, rep) = import(doc(&ents).as_bytes()).unwrap();
    accounted(&rep);
    let o: Vec<_> = ed.doc().objects.values().collect();
    assert_eq!(o.len(), 6);
    // The solid hatch: the outer loop in the pen color, the island in the color of the paper.
    assert_eq!(o[0].attrs.fill, Some(Color([0, 0, 255, 255])));
    assert_eq!(o[1].attrs.fill, Some(Color([255, 255, 255, 255])));
    assert_eq!(o[1].data["hatch_island"], true);
    // Pattern hatches are outlines without a fill, and the pattern names are counted.
    assert_eq!(o[2].attrs.fill, None);
    let n = rep.notes.join("\n");
    assert!(
        n.contains("3 hatches with a pattern were imported as outlines"),
        "{n}"
    );
    assert!(n.contains("ANSI31 2, NET 1"), "{n}");
    assert!(
        n.contains("1 gradient hatch was imported with a flat fill"),
        "{n}"
    );
    assert!(n.contains("1 island of solid hatches is drawn"), "{n}");
    // The gradient has the fill of its first color (true color 255 = blue).
    assert_eq!(o[5].attrs.fill, Some(Color([0, 0, 255, 255])));
}

#[test]
fn splines_and_ellipses_say_how_exact_they_stay() {
    let spline = "0\nSPLINE\n8\nS\n70\n8\n71\n3\n72\n8\n73\n4\n74\n0\n\
40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n\
10\n0\n20\n0\n30\n0\n10\n3000\n20\n5000\n30\n0\n10\n7000\n20\n5000\n30\n0\n10\n10000\n20\n0\n30\n0\n";
    let fit = "0\nSPLINE\n8\nS\n70\n8\n71\n3\n74\n3\n11\n0\n21\n0\n31\n0\n11\n100\n21\n100\n31\n0\n11\n200\n21\n0\n31\n0\n";
    let polygon = "0\nSPLINE\n8\nS\n70\n8\n71\n3\n73\n3\n10\n0\n20\n0\n30\n0\n10\n100\n20\n100\n30\n0\n10\n200\n20\n0\n30\n0\n";
    let ellipse = "0\nELLIPSE\n8\nS\n10\n0\n20\n0\n30\n0\n11\n1000\n21\n0\n31\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n";
    let (ed, rep) = import(doc(&format!("{spline}{fit}{polygon}{ellipse}")).as_bytes()).unwrap();
    accounted(&rep);
    let n = rep.notes.join("\n");
    assert!(n.contains("2 splines became polylines within 2 mm"), "{n}");
    assert!(n.contains("1 spline with fit points only"), "{n}");
    assert!(
        n.contains("1 spline had no usable knots, so uniform knots were made"),
        "{n}"
    );
    assert!(n.contains("1 ellipse or elliptical arc became"), "{n}");
    // The cubic Bezier through (0,0) (3000,5000) (7000,5000) (10000,0) is within the tolerance.
    let Shape::Polyline { points, .. } = &shapes_of(&ed)[0] else {
        panic!()
    };
    let p = |t: f64| {
        let u = 1.0 - t;
        DVec2::new(0.0, 0.0) * (u * u * u)
            + DVec2::new(3000.0, 5000.0) * (3.0 * u * u * t)
            + DVec2::new(7000.0, 5000.0) * (3.0 * u * t * t)
            + DVec2::new(10000.0, 0.0) * (t * t * t)
    };
    let dense: Vec<DVec2> = (0..=2000).map(|i| p(i as f64 / 2000.0)).collect();
    let worst = dense
        .iter()
        .map(|c| {
            points
                .windows(2)
                .map(|w| {
                    let (a, b) = (w[0].p, w[1].p);
                    let ab = b - a;
                    let t = ((*c - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                    (*c - (a + ab * t)).length()
                })
                .fold(f64::MAX, f64::min)
        })
        .fold(0.0, f64::max);
    assert!(worst <= 2.0 + 1e-6, "{worst}");
    assert!(points.len() > 30);
    // A smaller tolerance gives more points and a smaller deviation.
    let (ed2, _) = import_with(
        doc(spline).as_bytes(),
        &ImportOptions {
            tolerance: 0.1,
            ..ImportOptions::default()
        },
    )
    .unwrap();
    let Shape::Polyline { points: finer, .. } = &shapes_of(&ed2)[0] else {
        panic!()
    };
    assert!(finer.len() > points.len());
}

#[test]
fn a_closed_spline_stays_closed() {
    let ents = "0\nSPLINE\n8\nS\n70\n11\n71\n3\n72\n12\n73\n8\n74\n0\n\
40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n2\n40\n3\n40\n4\n40\n4\n40\n4\n40\n4\n\
10\n0\n20\n0\n30\n0\n10\n1000\n20\n0\n30\n0\n10\n1000\n20\n1000\n30\n0\n10\n0\n20\n1000\n30\n0\n10\n0\n20\n0\n30\n0\n10\n1000\n20\n0\n30\n0\n10\n1000\n20\n1000\n30\n0\n10\n0\n20\n1000\n30\n0\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(shapes_of(&ed).len(), 1, "{:?}", rep.skipped);
    assert!(matches!(
        &shapes_of(&ed)[0],
        Shape::Polyline { closed: true, .. }
    ));
}

#[test]
fn circles_arcs_and_polylines_in_a_tilted_plane_stand_up() {
    // The plane x = 0 (normal along +X): OCS x is world Y, OCS y is world Z.
    let ents = "0\nCIRCLE\n8\nC\n210\n1\n220\n0\n230\n0\n10\n0\n20\n0\n30\n0\n40\n500\n\
0\nARC\n8\nC\n210\n1\n220\n0\n230\n0\n10\n0\n20\n0\n30\n100\n40\n500\n50\n0\n51\n90\n\
0\nLWPOLYLINE\n8\nC\n90\n2\n70\n0\n210\n1\n220\n0\n230\n0\n38\n7\n10\n0\n20\n0\n10\n1000\n20\n500\n\
0\nTEXT\n8\nC\n210\n1\n220\n0\n230\n0\n10\n0\n20\n0\n40\n5\n1\nflat\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    let s = shapes_of(&ed);
    assert_eq!(s.len(), 3);
    let Shape::Polyline3d { points, closed } = &s[0] else {
        panic!()
    };
    assert!(*closed && points.iter().all(|p| p.x.abs() < 1e-6));
    assert!(points.iter().any(|p| (p.z - 500.0).abs() < 1.0));
    let Shape::Polyline3d { points, closed } = &s[1] else {
        panic!()
    };
    // The arc from the OCS angle 0 to 90 at OCS z 100: from world (100, 500, 0) to (100, 0, 500).
    assert!(!*closed);
    assert!((points[0] - DVec3::new(100.0, 500.0, 0.0)).length() < 1e-6);
    assert!((*points.last().unwrap() - DVec3::new(100.0, 0.0, 500.0)).length() < 1e-6);
    let Shape::Polyline3d { points, .. } = &s[2] else {
        panic!()
    };
    assert!((points[1] - DVec3::new(7.0, 1000.0, 500.0)).length() < 1e-6);
    // The text is reported: it cannot lie in a tilted plane.
    let t = rep.skipped.iter().find(|s| s.entity == "TEXT").unwrap();
    assert!(t.reason.contains("tilted"));
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("3 entities in a tilted plane were imported as 3D polylines")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn a_tilted_ellipse_keeps_its_plane() {
    let ents = "0\nELLIPSE\n8\nE\n10\n0\n20\n0\n30\n0\n11\n1000\n21\n0\n31\n0\n210\n0\n220\n-1\n230\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    let Shape::Polyline3d { points, closed } = &shapes_of(&ed)[0] else {
        panic!()
    };
    assert!(*closed);
    assert!(points.iter().all(|p| p.y.abs() < 1e-6));
    let top = points.iter().map(|p| p.z).fold(f64::MIN, f64::max);
    assert!((top - 500.0).abs() < 5.0, "{top}");
}

#[test]
fn xrefs_and_unreferenced_blocks_are_reported() {
    let blocks =
        "0\nBLOCK\n8\n0\n2\nSITE_XREF\n70\n4\n1\nsite.dwg\n10\n0\n20\n0\n30\n0\n0\nENDBLK\n";
    let ents =
        "0\nINSERT\n8\n0\n2\nSITE_XREF\n10\n0\n20\n0\n0\nINSERT\n8\n0\n2\nNO_SUCH\n10\n0\n20\n0\n";
    let (_, rep) = import(with_blocks(blocks, ents).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(rep.entities_skipped, 2);
    assert!(rep.skipped.iter().any(|s| {
        s.reason
            .contains("external reference (XREF) 'SITE_XREF' to site.dwg")
    }));
}

#[test]
fn the_object_limit_is_accounted_for_too() {
    let mut ents = String::new();
    for i in 0..20 {
        ents.push_str(&format!("0\nLINE\n8\n0\n10\n{i}\n20\n0\n11\n{i}\n21\n1\n"));
    }
    let (_, rep) = import_with(
        doc(&ents).as_bytes(),
        &ImportOptions {
            max_objects: 5,
            ..ImportOptions::default()
        },
    )
    .unwrap();
    accounted(&rep);
    assert_eq!(rep.entities_read, 20);
    assert_eq!(rep.total(), 5);
}

#[test]
fn a_constant_attribute_definition_shows_its_value() {
    let ents = "0\nATTDEF\n8\nT\n10\n0\n20\n0\n40\n5\n1\nFixed value\n2\nTAG\n70\n2\n\
0\nATTDEF\n8\nT\n10\n0\n20\n10\n40\n5\n1\nTemplate\n2\nTAG\n70\n0\n\
0\nATTDEF\n8\nT\n10\n0\n20\n20\n40\n5\n1\nHidden constant\n2\nTAG\n70\n3\n";
    let (ed, rep) = import(doc(ents).as_bytes()).unwrap();
    accounted(&rep);
    let t: Vec<String> = shapes_of(&ed)
        .into_iter()
        .map(|s| match s {
            Shape::Text { text, .. } => text,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(t, vec!["Fixed value"]);
    assert_eq!(rep.entities_skipped, 2);
}
