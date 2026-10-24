//! Text and block references in a tilted plane (IOP-27). A block reference is placed in 3D (lines,
//! polylines, circles and arcs become 3D polylines), a text is placed flat by its projection. Every
//! entity is imported or has a line in the report, and the notes count what was projected.

mod support;

use kadakas_core::command::Editor;
use kadakas_core::model::{Object, Shape};
use kadakas_core::{DVec2, DVec3};
use kadakas_io::Report;
use support::import;

fn accounted(rep: &Report) {
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
}

fn file(blocks: &str, entities: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

fn block(name: &str, entities: &str) -> String {
    format!("0\nBLOCK\n8\n0\n2\n{name}\n70\n0\n10\n0\n20\n0\n30\n0\n{entities}0\nENDBLK\n8\n0\n")
}

fn objects(ed: &Editor) -> Vec<&Object> {
    ed.doc().objects.values().collect()
}

fn near(a: DVec3, b: DVec3) -> bool {
    a.distance(b) < 1e-6
}

fn points3(o: &Object) -> Vec<DVec3> {
    match &o.shape {
        Shape::Polyline3d { points, .. } => points.clone(),
        other => panic!("{other:?}"),
    }
}

fn note(rep: &Report, needle: &str) {
    assert!(
        rep.notes.iter().any(|n| n.contains(needle)),
        "{needle}\n{:?}",
        rep.notes
    );
}

/// The plane x = 0: the normal is +X, OCS x is the world Y axis, OCS y is world Z, OCS z is world X.
const PLANE_X: &str = "210\n1\n220\n0\n230\n0\n";

#[test]
fn a_block_in_a_vertical_plane_stands_up() {
    let b = block(
        "B",
        "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n\
0\nCIRCLE\n8\n0\n10\n5\n20\n0\n40\n2\n\
0\nLWPOLYLINE\n8\n0\n90\n3\n70\n1\n10\n0\n20\n1\n10\n4\n20\n1\n10\n4\n20\n3\n\
0\nPOINT\n8\n0\n10\n1\n20\n2\n30\n3\n",
    );
    // The insertion point is in the OCS: (100, 50) in the plane at OCS z = 7.
    let ins = format!("0\nINSERT\n8\n0\n2\nB\n{PLANE_X}10\n100\n20\n50\n30\n7\n");
    let (ed, rep) = import(file(&b, &ins).as_bytes()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 4, "{:?}", rep.skipped);
    // The line: OCS (100, 50, 7) to (110, 50, 7) is world (7, 100, 50) to (7, 110, 50).
    let p = points3(objs[0]);
    assert!(
        near(p[0], DVec3::new(7.0, 100.0, 50.0)) && near(p[1], DVec3::new(7.0, 110.0, 50.0)),
        "{p:?}"
    );
    // The circle: center OCS (105, 50), radius 2, in the plane x = 7.
    let ring = points3(objs[1]);
    assert!(ring.len() >= 8);
    for q in &ring {
        assert!((q.x - 7.0).abs() < 1e-6);
        let r = ((q.y - 105.0).powi(2) + (q.z - 50.0).powi(2)).sqrt();
        assert!((r - 2.0).abs() < 1e-6, "{r}");
    }
    // The closed polyline: its corners are the OCS points moved the same way.
    let Shape::Polyline3d { points, closed } = &objs[2].shape else {
        panic!()
    };
    assert!(*closed);
    assert!(near(points[0], DVec3::new(7.0, 100.0, 51.0)), "{points:?}");
    assert!(near(points[2], DVec3::new(7.0, 104.0, 53.0)));
    // The 3D point of the block moves too: block (1, 2, 3) is OCS (101, 52, 10): world (10, 101, 52).
    let Shape::Locus3d { p } = &objs[3].shape else {
        panic!()
    };
    assert!(near(*p, DVec3::new(10.0, 101.0, 52.0)), "{p:?}");
    note(
        &rep,
        "1 block reference lies in a tilted plane: the content is placed in 3D",
    );
    note(&rep, "4 objects became 3D polylines");
}

#[test]
fn the_turn_and_the_scale_of_the_reference_apply_in_its_plane() {
    let b = block("B", "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n");
    // Turned 90 degrees in its plane and twice as large: the line runs along OCS y (world Z) for 20.
    let ins = format!(
        "0\nINSERT\n8\n0\n2\nB\n{PLANE_X}10\n0\n20\n0\n30\n0\n41\n2\n42\n2\n43\n2\n50\n90\n"
    );
    let (ed, rep) = import(file(&b, &ins).as_bytes()).unwrap();
    accounted(&rep);
    let p = points3(objects(&ed)[0]);
    assert!(
        near(p[0], DVec3::ZERO) && near(p[1], DVec3::new(0.0, 0.0, 20.0)),
        "{p:?}"
    );
}

#[test]
fn a_tilted_block_inside_a_block_that_turns_pivots_about_its_own_origin() {
    // Block IN has a line along OCS x. Block OUT inserts it in the plane x = 0 at the OCS point
    // (10, 0). The outer reference turns 90 degrees about (1000, 0) and doubles the size: the
    // tilt happens inside the block, then the turn and the scale of the outer reference apply.
    let inner = block("IN", "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n0\n");
    let outer = block(
        "OUT",
        &format!("0\nINSERT\n8\n0\n2\nIN\n{PLANE_X}10\n10\n20\n0\n"),
    );
    let ins = "0\nINSERT\n8\n0\n2\nOUT\n10\n1000\n20\n0\n41\n2\n42\n2\n43\n2\n50\n90\n";
    let (ed, rep) = import(file(&(inner + &outer), ins).as_bytes()).unwrap();
    accounted(&rep);
    let p = points3(objects(&ed)[0]);
    // Inside OUT the line runs from world (0, 10, 0) to (0, 15, 0). The outer reference scales by 2
    // and turns by 90 degrees about z: (x, y) -> (-2y, 2x), then moves by (1000, 0).
    assert!(near(p[0], DVec3::new(1000.0 - 20.0, 0.0, 0.0)), "{p:?}");
    assert!(near(p[1], DVec3::new(1000.0 - 30.0, 0.0, 0.0)), "{p:?}");
}

#[test]
fn a_fill_in_a_tilted_block_loses_its_fill_and_the_island_goes() {
    let ring = |pts: &[(f64, f64)]| {
        let mut s = format!("92\n7\n72\n0\n73\n1\n93\n{}\n", pts.len());
        for (x, y) in pts {
            s += &format!("10\n{x}\n20\n{y}\n");
        }
        s + "97\n0\n"
    };
    let hatch = format!(
        "0\nHATCH\n8\n0\n62\n3\n210\n0\n220\n0\n230\n1\n2\nSOLID\n70\n1\n71\n0\n91\n2\n{}{}75\n0\n76\n1\n98\n0\n",
        ring(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
        ring(&[(3.0, 3.0), (6.0, 3.0), (6.0, 6.0), (3.0, 6.0)])
    );
    let b = block("B", &hatch);
    let ins = format!("0\nINSERT\n8\n0\n2\nB\n{PLANE_X}10\n0\n20\n0\n");
    let (ed, rep) = import(file(&b, &ins).as_bytes()).unwrap();
    accounted(&rep);
    // The outer loop is an outline in 3D without a fill. The island is gone.
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    assert!(objs[0].attrs.fill.is_none());
    assert!(matches!(
        objs[0].shape,
        Shape::Polyline3d { closed: true, .. }
    ));
    note(
        &rep,
        "1 filled shape lost its fill because a fill cannot lie in a tilted plane",
    );
    // The island is counted.
    note(&rep, "1 island of hatches was left out");
}

#[test]
fn text_in_a_tilted_plane_is_placed_by_its_projection() {
    // A plane tilted by 60 degrees about the X axis: normal (0, -sin 60, cos 60).
    let (s, c) = 60f64.to_radians().sin_cos();
    let tilt = format!("210\n0\n220\n{}\n230\n{}\n", -s, c);
    let text = format!("0\nTEXT\n8\n0\n{tilt}10\n100\n20\n200\n30\n0\n40\n10\n50\n0\n1\nPlan\n");
    let (ed, rep) = import(file("", &text).as_bytes()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    let Shape::Text { at, height, text } = &objs[0].shape else {
        panic!()
    };
    assert_eq!(text, "Plan");
    // OCS x is world X. OCS y is (0, cos 60, sin 60) in the world, so a point 200 up the plane is 100 along Y.
    assert!(
        (at.x - 100.0).abs() < 1e-6 && (at.y - 200.0 * c).abs() < 1e-6,
        "{at:?}"
    );
    // The height runs up the plane: its projection is cos 60.
    assert!((height - 10.0 * c).abs() < 1e-6, "{height}");
    assert!(!objs[0].data.contains_key("rotation_deg"));
    note(
        &rep,
        "1 text in a tilted plane was placed flat: the point, the turn and the height come from the projection onto the drawing plane",
    );
    assert!(!rep.notes.iter().any(|n| n.contains("nearly upright")));
}

#[test]
fn an_mtext_in_the_drawing_with_a_tilted_plane_and_a_text_on_a_wall() {
    // The text on a wall along X: it keeps its height and its turn follows the wall.
    let wall = "0\nTEXT\n8\n0\n210\n0\n220\n-1\n230\n0\n10\n500\n20\n300\n30\n0\n40\n20\n1\nWall\n";
    // An MTEXT on the same wall: its point and its direction are world coordinates.
    let mtext = "0\nMTEXT\n8\n0\n210\n0\n220\n-1\n230\n0\n10\n700\n20\n0\n30\n150\n40\n25\n11\n0\n21\n0\n31\n1\n1\nSide\n";
    let (ed, rep) = import(file("", &format!("{wall}{mtext}")).as_bytes()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 2);
    let Shape::Text { at, height, .. } = &objs[0].shape else {
        panic!()
    };
    // OCS x is the world X axis and OCS y the world Z axis: the point is world (500, 0, 300).
    assert!((at.x - 500.0).abs() < 1e-6 && at.y.abs() < 1e-6, "{at:?}");
    assert!((height - 20.0).abs() < 1e-9);
    // The MTEXT: world point (700, 0, 150), baseline straight up the wall: the plan turns it by 90 degrees.
    let Shape::Text { at, height, .. } = &objs[1].shape else {
        panic!()
    };
    assert!((at - DVec2::new(700.0, 0.0)).length() < 1e-6, "{at:?}");
    assert!((height - 25.0).abs() < 1e-9);
    assert!((objs[1].data["rotation_deg"].as_f64().unwrap() - 90.0).abs() < 1e-6);
    note(&rep, "2 texts in a tilted plane were placed flat");
    // The MTEXT runs straight up the wall: its height lies flat in the plan, so it is not counted as upright.
    note(
        &rep,
        "1 stand in a nearly upright plane and keep their full height",
    );
}

#[test]
fn a_text_seen_from_the_back_is_counted() {
    let (s, c) = 120f64.to_radians().sin_cos();
    let text = format!(
        "0\nTEXT\n8\n0\n210\n0\n220\n{}\n230\n{}\n10\n0\n20\n0\n30\n0\n40\n10\n1\nBack\n",
        -s, c
    );
    let (_, rep) = import(file("", &text).as_bytes()).unwrap();
    accounted(&rep);
    note(&rep, "1 is seen from the back and is not mirrored");
}

#[test]
fn a_text_in_a_tilted_block_follows_the_block() {
    let b = block("B", "0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n10\n50\n30\n1\nIn\n");
    let ins = format!("0\nINSERT\n8\n0\n2\nB\n{PLANE_X}10\n40\n20\n60\n30\n0\n");
    let (ed, rep) = import(file(&b, &ins).as_bytes()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    let Shape::Text { at, height, .. } = &objs[0].shape else {
        panic!()
    };
    // OCS (40, 60) is world (0, 40, 60): the point is (0, 40). The text is turned by 30 degrees in
    // the plane, so its height runs (0, -sin 30, cos 30): the projected height is half.
    assert!((at - DVec2::new(0.0, 40.0)).length() < 1e-6, "{at:?}");
    assert!((height - 5.0).abs() < 1e-9, "{height}");
    // The baseline was turned by 30 degrees in the plane: along (cos 30 in OCS x, sin 30 in OCS y).
    // OCS x is world Y, so the projected baseline is (0, cos 30): it runs along Y.
    assert!((objs[0].data["rotation_deg"].as_f64().unwrap() - 90.0).abs() < 1e-6);
}

#[test]
fn a_clip_works_in_a_tilted_block_before_the_tilt() {
    // A clip is a polygon of the plane. It cuts the geometry in the plane, and then the plane tilts.
    // (The clip objects are written by the test of the clips; here a block without a clip is enough
    // to show that nothing breaks when both are used.)
    let b = block("B", "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n");
    let ins = format!("0\nINSERT\n8\n0\n2\nB\n{PLANE_X}10\n0\n20\n0\n70\n2\n71\n1\n44\n20\n");
    let (ed, rep) = import(file(&b, &ins).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(objects(&ed).len(), 2);
}

#[test]
fn a_block_whose_container_cannot_be_inverted_is_reported() {
    let b = block("B", "0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n");
    // The scale 0 of the outer reference makes the transform around the tilted one singular.
    let outer = block(
        "OUT",
        &format!("0\nINSERT\n8\n0\n2\nB\n{PLANE_X}10\n0\n20\n0\n"),
    );
    let ins = "0\nINSERT\n8\n0\n2\nOUT\n10\n0\n20\n0\n41\n0\n42\n0\n";
    let (ed, rep) = import(file(&(b + &outer), ins).as_bytes()).unwrap();
    accounted(&rep);
    // The inner reference is a skipped entity with its reason.
    assert!(objects(&ed).is_empty());
    assert!(
        rep.skipped
            .iter()
            .any(|s| s.reason.contains("cannot be inverted")),
        "{:?}",
        rep.skipped
    );
}

#[test]
fn a_flat_shape_keeps_its_own_elevation_inside_a_tilted_block() {
    // The plane x = 0. A circle at the elevation 300 and a text at the elevation 400 stand 300 and
    // 400 mm in front of the plane: their world x is the elevation (OCS z is the world X axis).
    let b = block(
        "B",
        "0\nCIRCLE\n8\n0\n10\n0\n20\n0\n30\n300\n40\n50\n\
0\nTEXT\n8\n0\n10\n0\n20\n0\n30\n400\n40\n10\n1\nUp\n\
0\nLWPOLYLINE\n8\n0\n90\n2\n70\n0\n38\n500\n10\n0\n20\n0\n10\n10\n20\n0\n",
    );
    let ins = format!("0\nINSERT\n8\n0\n2\nB\n{PLANE_X}10\n0\n20\n0\n30\n0\n");
    let (ed, rep) = import(file(&b, &ins).as_bytes()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert!(points3(objs[0]).iter().all(|p| (p.x - 300.0).abs() < 1e-6));
    let Shape::Text { at, .. } = &objs[1].shape else {
        panic!()
    };
    assert!((at.x - 400.0).abs() < 1e-6, "{at:?}");
    assert!(points3(objs[2]).iter().all(|p| (p.x - 500.0).abs() < 1e-6));
}
