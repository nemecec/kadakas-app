//! Block clips (XCLIP, IOP-27): the geometry of a clipped block reference is cut by the polygon.
//! Lines and polylines are exact at the clip edge, curves that are cut become chords, filled shapes
//! become the boolean of the shape and the clip, text and points stay or go by their point.
//! Whatever is outside has a line in the report.

mod support;

use kadakas_core::DVec2;
use kadakas_core::model::{Object, Shape};
use kadakas_io::{ImportOptions, Report};
use support::import_with;

fn accounted(rep: &Report) {
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
}

fn opts() -> ImportOptions {
    ImportOptions {
        tolerance: 0.001,
        origin: kadakas_io::OriginMode::File,
        unit: Some(kadakas_io::Unit::Mm),
        ..ImportOptions::default()
    }
}

/// How a block reference is placed.
#[derive(Clone, Copy)]
struct Place {
    at: (f64, f64),
    rotation: f64,
    scale: f64,
}

const HERE: Place = Place {
    at: (0.0, 0.0),
    rotation: 0.0,
    scale: 1.0,
};

impl Place {
    /// The 12 numbers of the inverse of the placement (the rows of a 3 x 4 matrix).
    fn inverse(&self) -> [f64; 12] {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let k = self.scale;
        let (tx, ty) = self.at;
        [
            c / k,
            s / k,
            0.0,
            -(c * tx + s * ty) / k,
            -s / k,
            c / k,
            0.0,
            (s * tx - c * ty) / k,
            0.0,
            0.0,
            1.0 / k,
            0.0,
        ]
    }

    fn world(&self, p: (f64, f64)) -> DVec2 {
        let (s, c) = self.rotation.to_radians().sin_cos();
        DVec2::new(
            self.at.0 + self.scale * (c * p.0 - s * p.1),
            self.at.1 + self.scale * (s * p.0 + c * p.1),
        )
    }
}

/// A block reference with the objects of its clip.
struct Clipped {
    handle: &'static str,
    block: &'static str,
    place: Place,
    clip: Vec<(f64, f64)>,
    enabled: i32,
    /// The part that an inverted clip cuts away.
    cut_away: Option<Vec<(f64, f64)>>,
    /// Write the filter without its matrix.
    no_matrix: bool,
    /// An array of columns and rows with these distances.
    array: Option<(i32, i32, f64, f64)>,
    /// The reference is in the section ENTITIES. Else the test puts it in a block.
    top: bool,
}

impl Clipped {
    fn new(
        handle: &'static str,
        block: &'static str,
        place: Place,
        clip: &[(f64, f64)],
    ) -> Clipped {
        Clipped {
            handle,
            block,
            place,
            clip: clip.to_vec(),
            enabled: 1,
            cut_away: None,
            no_matrix: false,
            array: None,
            top: true,
        }
    }

    fn insert(&self) -> String {
        let p = &self.place;
        let mut s = format!(
            "0\nINSERT\n5\n{h}\n102\n{{ACAD_XDICTIONARY\n360\n{h}D1\n102\n}}\n8\n0\n2\n{b}\n10\n{x}\n20\n{y}\n30\n0\n41\n{k}\n42\n{k}\n43\n{k}\n50\n{r}\n",
            h = self.handle,
            b = self.block,
            x = p.at.0,
            y = p.at.1,
            k = p.scale,
            r = p.rotation
        );
        if let Some((c, r, dx, dy)) = self.array {
            s += &format!("70\n{c}\n71\n{r}\n44\n{dx}\n45\n{dy}\n");
        }
        s
    }

    fn objects(&self) -> String {
        let h = self.handle;
        let mut s = format!(
            "0\nDICTIONARY\n5\n{h}D1\n330\n{h}\n100\nAcDbDictionary\n3\nACAD_FILTER\n360\n{h}D2\n\
0\nDICTIONARY\n5\n{h}D2\n330\n{h}D1\n100\nAcDbDictionary\n3\nSPATIAL\n360\n{h}D3\n"
        );
        s += &format!("0\nSPATIAL_FILTER\n5\n{h}D3\n330\n{h}D2\n");
        if self.cut_away.is_some() {
            s += &format!("102\n{{ACAD_XDICTIONARY\n360\n{h}D4\n102\n}}\n");
        }
        s += &format!(
            "100\nAcDbFilter\n100\nAcDbSpatialFilter\n70\n{}\n",
            self.clip.len()
        );
        for (x, y) in &self.clip {
            s += &format!("10\n{x}\n20\n{y}\n");
        }
        s += &format!(
            "210\n0\n220\n0\n230\n1\n11\n0\n21\n0\n31\n0\n71\n{}\n72\n0\n73\n0\n",
            self.enabled
        );
        if !self.no_matrix {
            for v in self.place.inverse() {
                s += &format!("40\n{v}\n");
            }
            for v in [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
                s += &format!("40\n{v}\n");
            }
        }
        if let Some(inner) = &self.cut_away {
            s += &format!(
                "0\nDICTIONARY\n5\n{h}D4\n330\n{h}D3\n100\nAcDbDictionary\n3\nACAD_XREC_ROUNDTRIP\n360\n{h}D5\n\
0\nXRECORD\n5\n{h}D5\n330\n{h}D4\n100\nAcDbXrecord\n280\n1\n102\nACAD_INVERTEDCLIP_ROUNDTRIP\n"
            );
            for (x, y) in inner {
                s += &format!("10\n{x}\n20\n{y}\n30\n0\n");
            }
        }
        s
    }
}

/// A DXF file with these blocks, entities and clipped references.
fn file(blocks: &str, entities: &str, clipped: &[Clipped]) -> String {
    let inserts: String = clipped
        .iter()
        .filter(|c| c.top)
        .map(Clipped::insert)
        .collect();
    let objects: String = clipped.iter().map(Clipped::objects).collect();
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}{inserts}0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n{objects}0\nENDSEC\n0\nEOF\n"
    )
}

fn block(name: &str, entities: &str) -> String {
    format!("0\nBLOCK\n8\n0\n2\n{name}\n70\n0\n10\n0\n20\n0\n30\n0\n{entities}0\nENDBLK\n8\n0\n")
}

fn line(a: (f64, f64), b: (f64, f64)) -> String {
    format!(
        "0\nLINE\n8\n0\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
        a.0, a.1, b.0, b.1
    )
}

fn objects(ed: &kadakas_core::command::Editor) -> Vec<&Object> {
    ed.doc().objects.values().collect()
}

fn near(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-6
}

fn ends(o: &Object) -> (DVec2, DVec2) {
    match &o.shape {
        Shape::Line { a, b } => (*a, *b),
        Shape::Polyline { points, .. } => (points[0].p, points[points.len() - 1].p),
        other => panic!("{other:?}"),
    }
}

fn skipped(rep: &Report, needle: &str) -> usize {
    rep.skipped
        .iter()
        .filter(|s| s.reason.contains(needle))
        .map(|s| s.count)
        .sum()
}

const RECT: [(f64, f64); 2] = [(20.0, -50.0), (60.0, 50.0)];

#[test]
fn a_line_is_cut_at_the_edge_of_the_rectangle_and_stays_exact_there() {
    // Three lines: across the clip, inside it, and outside it.
    let b = block(
        "B",
        &(line((0.0, 0.0), (100.0, 0.0))
            + &line((30.0, 10.0), (50.0, 10.0))
            + &line((70.0, 0.0), (90.0, 0.0))),
    );
    let c = Clipped::new("A1", "B", HERE, &RECT);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 2, "{objs:?}");
    let (a, z) = ends(objs[0]);
    assert!(
        near(a, DVec2::new(20.0, 0.0)) && near(z, DVec2::new(60.0, 0.0)),
        "{a:?} {z:?}"
    );
    assert!(near(ends(objs[1]).0, DVec2::new(30.0, 10.0)));
    // The line outside is not imported, and the report says why.
    assert_eq!(
        skipped(&rep, "outside the clip (XCLIP) of its block reference"),
        1
    );
    let notes = rep.notes.join("\n");
    assert!(
        notes.contains("1 block reference has a clip (XCLIP) that was applied"),
        "{notes}"
    );
    assert!(
        notes.contains("1 object was cut at the edge of the clip"),
        "{notes}"
    );
}

#[test]
fn the_clip_follows_the_turn_the_scale_and_the_place_of_the_reference() {
    let b = block("B", &line((0.0, 0.0), (100.0, 0.0)));
    // At (200, 100), turned 90 degrees, twice as large: the line runs from (200, 100) up to (200, 300).
    let place = Place {
        at: (200.0, 100.0),
        rotation: 90.0,
        scale: 2.0,
    };
    // A clip in the drawing: x from 150 to 250, y from 150 to 250.
    let c = Clipped::new("A1", "B", place, &[(150.0, 150.0), (250.0, 250.0)]);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    let (a, z) = ends(objs[0]);
    assert!(
        near(a, DVec2::new(200.0, 150.0)) && near(z, DVec2::new(200.0, 250.0)),
        "{a:?} {z:?}"
    );
    assert!(near(place.world((100.0, 0.0)), DVec2::new(200.0, 300.0)));
}

#[test]
fn a_concave_clip_cuts_a_line_in_two() {
    // A U shape: the line at y = 50 runs through the two arms and over the gap.
    let u = [
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 100.0),
        (70.0, 100.0),
        (70.0, 30.0),
        (30.0, 30.0),
        (30.0, 100.0),
        (0.0, 100.0),
    ];
    let b = block("B", &line((-10.0, 50.0), (110.0, 50.0)));
    let c = Clipped::new("A1", "B", HERE, &u);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 2);
    let mut xs: Vec<(f64, f64)> = objs.iter().map(|o| (ends(o).0.x, ends(o).1.x)).collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        (xs[0].0 - 0.0).abs() < 1e-6 && (xs[0].1 - 30.0).abs() < 1e-6,
        "{xs:?}"
    );
    assert!(
        (xs[1].0 - 70.0).abs() < 1e-6 && (xs[1].1 - 100.0).abs() < 1e-6,
        "{xs:?}"
    );
}

#[test]
fn a_circle_inside_stays_exact_and_a_circle_that_is_cut_becomes_chords() {
    let circle = |x: f64, y: f64, r: f64| format!("0\nCIRCLE\n8\n0\n10\n{x}\n20\n{y}\n40\n{r}\n");
    let b = block(
        "B",
        &(circle(40.0, 0.0, 10.0) + &circle(60.0, 0.0, 20.0) + &circle(200.0, 0.0, 5.0)),
    );
    let c = Clipped::new("A1", "B", HERE, &RECT);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    // The first circle is whole. The second is cut at x = 60 and becomes one arc of chords. The third is outside.
    assert_eq!(objs.len(), 2);
    assert!(matches!(objs[0].shape, Shape::Circle { .. }));
    let Shape::Polyline { points, closed } = &objs[1].shape else {
        panic!("{:?}", objs[1].shape)
    };
    assert!(!closed);
    // The ends are on the edge x = 60, and the radius is kept.
    let (a, z) = (points[0].p, points[points.len() - 1].p);
    assert!(
        (a.x - 60.0).abs() < 1e-6 && (z.x - 60.0).abs() < 1e-6,
        "{a:?} {z:?}"
    );
    assert!(
        points
            .iter()
            // The chords are within the tolerance of 0.001 mm of the circle.
            .all(|p| ((p.p - DVec2::new(60.0, 0.0)).length() - 20.0).abs() < 1.1e-3)
    );
    assert_eq!(skipped(&rep, "outside the clip"), 1);
    let notes = rep.notes.join("\n");
    assert!(notes.contains("1 object was cut at the edge of the clip (the cut points are exact. 1 with curves became chords within 0.001 mm)"), "{notes}");
}

#[test]
fn a_polyline_with_arcs_inside_keeps_its_arcs() {
    // A closed polyline with two bulges, inside the clip.
    let poly = "0\nLWPOLYLINE\n8\n0\n90\n3\n70\n1\n10\n25\n20\n0\n42\n0.5\n10\n45\n20\n0\n10\n35\n20\n20\n";
    let b = block("B", poly);
    let c = Clipped::new("A1", "B", HERE, &RECT);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    let Shape::Polyline { points, .. } = &objs[0].shape else {
        panic!()
    };
    assert!(
        points
            .iter()
            .any(|v| v.kind != kadakas_core::curve::VertexKind::Corner),
        "{points:?}"
    );
}

#[test]
fn text_and_points_stay_or_go_by_their_point() {
    let text = |x: f64, s: &str| format!("0\nTEXT\n8\n0\n10\n{x}\n20\n0\n40\n5\n1\n{s}\n");
    let point = |x: f64| format!("0\nPOINT\n8\n0\n10\n{x}\n20\n0\n30\n3\n");
    let b = block(
        "B",
        &(text(30.0, "IN") + &text(90.0, "OUT") + &point(40.0) + &point(95.0)),
    );
    let c = Clipped::new("A1", "B", HERE, &RECT);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 2);
    assert!(matches!(&objs[0].shape, Shape::Text { text, .. } if text == "IN"));
    assert_eq!(skipped(&rep, "outside the clip"), 2);
    let notes = rep.notes.join("\n");
    assert!(
        notes.contains("2 objects (text, points, meshes) stayed or went whole"),
        "{notes}"
    );
}

fn area_of(o: &Object) -> f64 {
    match &o.shape {
        Shape::Polyline { points, .. } => kadakas_core::curve::area(points).abs(),
        other => panic!("{other:?}"),
    }
}

fn solid_hatch(ring: &[(f64, f64)]) -> String {
    let mut s = format!(
        "0\nHATCH\n8\n0\n62\n3\n210\n0\n220\n0\n230\n1\n2\nSOLID\n70\n1\n71\n0\n91\n1\n92\n7\n72\n0\n73\n1\n93\n{}\n",
        ring.len()
    );
    for (x, y) in ring {
        s += &format!("10\n{x}\n20\n{y}\n");
    }
    s + "97\n0\n75\n0\n76\n1\n98\n0\n"
}

#[test]
fn a_filled_shape_becomes_the_overlap_of_the_shape_and_the_clip() {
    // A square 0..100 and the clip x 20..60, y -50..50: the overlap is 40 x 50.
    let b = block(
        "B",
        &solid_hatch(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]),
    );
    let c = Clipped::new("A1", "B", HERE, &RECT);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    assert!((area_of(objs[0]) - 40.0 * 50.0).abs() < 1e-6);
    // The fill stays.
    assert!(objs[0].attrs.fill.is_some());
}

#[test]
fn a_pattern_hatch_keeps_its_pattern_in_the_pieces() {
    let h = "0\nHATCH\n8\n0\n210\n0\n220\n0\n230\n1\n2\nANSI31\n70\n0\n71\n0\n91\n1\n92\n7\n72\n0\n73\n1\n93\n4\n10\n0\n20\n0\n10\n100\n20\n0\n10\n100\n20\n100\n10\n0\n20\n100\n97\n0\n75\n0\n76\n1\n52\n0\n41\n1\n77\n0\n78\n1\n53\n45\n43\n0\n44\n0\n45\n-2.245\n46\n2.245\n79\n0\n98\n0\n";
    let b = block("B", h);
    let c = Clipped::new("A1", "B", HERE, &RECT);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    assert!(objs[0].attrs.fill_pattern.is_some());
    assert!((area_of(objs[0]) - 40.0 * 50.0).abs() < 1e-6);
}

#[test]
fn an_inverted_clip_cuts_away_the_inside() {
    // The outer polygon is 0..100 x -50..50 and the part cut away is 20..60 x -10..10.
    let outer = [(0.0, -50.0), (100.0, -50.0), (100.0, 50.0), (0.0, 50.0)];
    let inner = [(20.0, -10.0), (60.0, -10.0), (60.0, 10.0), (20.0, 10.0)];
    let b = block(
        "B",
        &(line((0.0, 0.0), (100.0, 0.0))
            + &solid_hatch(&[(0.0, -50.0), (100.0, -50.0), (100.0, 50.0), (0.0, 50.0)])),
    );
    let mut c = Clipped::new("A1", "B", HERE, &outer);
    c.cut_away = Some(inner.to_vec());
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    // The line is in two pieces, 0..20 and 60..100.
    let lines: Vec<_> = objs
        .iter()
        .filter(|o| matches!(o.shape, Shape::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 2);
    let mut spans: Vec<(f64, f64)> = lines.iter().map(|o| (ends(o).0.x, ends(o).1.x)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        (spans[0].1 - 20.0).abs() < 1e-6 && (spans[1].0 - 60.0).abs() < 1e-6,
        "{spans:?}"
    );
    // The fill is a square with a white island in it: 100 x 100 less 40 x 20.
    let fills: Vec<_> = objs
        .iter()
        .filter(|o| matches!(o.shape, Shape::Polyline { .. }))
        .collect();
    assert_eq!(fills.len(), 2);
    let net = area_of(fills[0]) - area_of(fills[1]);
    assert!((net - (10_000.0 - 800.0)).abs() < 1e-6, "{net}");
    assert_eq!(fills[1].data["hatch_island"], true);
    let notes = rep.notes.join("\n");
    assert!(notes.contains("1 clip is inverted"), "{notes}");
}

#[test]
fn a_clip_that_is_switched_off_or_cannot_be_read_shows_the_whole_block() {
    let b = block("B", &line((0.0, 0.0), (100.0, 0.0)));
    let mut off = Clipped::new("A1", "B", HERE, &RECT);
    off.enabled = 0;
    let mut no_matrix = Clipped::new("A2", "B", HERE, &RECT);
    no_matrix.no_matrix = true;
    let few = Clipped::new("A3", "B", HERE, &[(0.0, 0.0)]);
    let flat = Clipped::new("A4", "B", HERE, &[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)]);
    let (ed, rep) = import_with(
        file(&b, "", &[off, no_matrix, few, flat]).as_bytes(),
        &opts(),
    )
    .unwrap();
    accounted(&rep);
    assert_eq!(objects(&ed).len(), 4);
    let notes = rep.notes.join("\n");
    assert!(notes.contains("1 block reference has a clip (XCLIP) that is switched off in the file: the whole block shows"), "{notes}");
    assert!(
        notes.contains("3 block references have a clip (XCLIP) that cannot be read"),
        "{notes}"
    );
    assert_eq!(skipped(&rep, "outside the clip"), 0);
}

#[test]
fn every_copy_of_an_array_is_clipped() {
    // 2 columns, 150 apart. The clip is in the space of the reference: x 20..60 in the block.
    let b = block("B", &line((0.0, 0.0), (100.0, 0.0)));
    let mut c = Clipped::new("A1", "B", HERE, &RECT);
    c.array = Some((2, 1, 150.0, 0.0));
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 2);
    let mut xs: Vec<(f64, f64)> = objs.iter().map(|o| (ends(o).0.x, ends(o).1.x)).collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        (xs[0].0 - 20.0).abs() < 1e-6 && (xs[0].1 - 60.0).abs() < 1e-6,
        "{xs:?}"
    );
    assert!(
        (xs[1].0 - 170.0).abs() < 1e-6 && (xs[1].1 - 210.0).abs() < 1e-6,
        "{xs:?}"
    );
    // The reference is counted once.
    let notes = rep.notes.join("\n");
    assert!(
        notes.contains("1 block reference has a clip (XCLIP) that was applied"),
        "{notes}"
    );
}

#[test]
fn clips_of_nested_references_all_apply() {
    // Block INNER has a long line. Block OUTER inserts INNER with a clip x 0..80. The outer
    // reference is clipped x 20..60.
    let inner = block("INNER", &line((0.0, 0.0), (200.0, 0.0)));
    let mut inner_clip = Clipped::new("B1", "INNER", HERE, &[(0.0, -50.0), (80.0, 50.0)]);
    inner_clip.top = false;
    let outer = block("OUTER", &inner_clip.insert());
    let top = Clipped::new("A1", "OUTER", HERE, &RECT);
    let (ed, rep) = import_with(
        file(&(inner + &outer), "", &[top, inner_clip]).as_bytes(),
        &opts(),
    )
    .unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1, "{objs:?}");
    let (a, z) = ends(objs[0]);
    assert!(
        near(a, DVec2::new(20.0, 0.0)) && near(z, DVec2::new(60.0, 0.0)),
        "{a:?} {z:?}"
    );
}

#[test]
fn a_3d_polyline_is_cut_with_its_height() {
    let poly = "0\nPOLYLINE\n8\n0\n66\n1\n70\n8\n0\nVERTEX\n8\n0\n10\n0\n20\n0\n30\n0\n70\n32\n0\nVERTEX\n8\n0\n10\n100\n20\n0\n30\n100\n70\n32\n0\nSEQEND\n8\n0\n";
    let b = block("B", poly);
    let c = Clipped::new("A1", "B", HERE, &RECT);
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    let Shape::Polyline3d { points, .. } = &objs[0].shape else {
        panic!("{:?}", objs[0].shape)
    };
    assert!(
        (points[0].z - 20.0).abs() < 1e-6 && (points[points.len() - 1].z - 60.0).abs() < 1e-6,
        "{points:?}"
    );
}

#[test]
fn a_reference_without_a_clip_shows_whole() {
    let b = block("B", &line((0.0, 0.0), (100.0, 0.0)));
    let plain = "0\nINSERT\n8\n0\n2\nB\n10\n0\n20\n0\n30\n0\n";
    let (ed, rep) = import_with(file(&b, plain, &[]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    assert_eq!(objects(&ed).len(), 1);
    assert!(!rep.notes.iter().any(|n| n.contains("XCLIP")));
}

#[test]
fn a_large_circle_is_cut_within_the_tolerance_and_a_tiny_tolerance_says_that_it_is_capped() {
    // A circle of 20 m radius, cut by a rectangle. The tolerance is 2 mm.
    let circle = "0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n20000\n";
    let b = block("B", circle);
    let c = Clipped::new("A1", "B", HERE, &[(-30_000.0, -30_000.0), (0.0, 30_000.0)]);
    let o = ImportOptions {
        tolerance: 2.0,
        ..opts()
    };
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &o).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    let Shape::Polyline { points, .. } = &objs[0].shape else {
        panic!()
    };
    for w in points.windows(2) {
        let mid = (w[0].p + w[1].p) / 2.0;
        assert!(
            20_000.0 - mid.length() <= 2.0 + 1e-6,
            "{}",
            20_000.0 - mid.length()
        );
    }
    // A very small tolerance meets the cap, and the note says so.
    let c = Clipped::new("A1", "B", HERE, &[(-30_000.0, -30_000.0), (0.0, 30_000.0)]);
    let o = ImportOptions {
        tolerance: 1e-7,
        ..opts()
    };
    let (_, rep) = import_with(file(&b, "", &[c]).as_bytes(), &o).unwrap();
    accounted(&rep);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("chords were cut to 20000 at most")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn an_inverted_clip_whose_filter_polygon_is_the_inner_one_is_read_like_ezdxf() {
    // The polygon of the filter is the part that is cut away (20..60). The XRECORD holds the
    // outer polygon (0..100). The smaller box is the part that is cut away.
    let inner = [(20.0, -10.0), (60.0, -10.0), (60.0, 10.0), (20.0, 10.0)];
    let outer = [(0.0, -50.0), (100.0, -50.0), (100.0, 50.0), (0.0, 50.0)];
    let b = block("B", &line((0.0, 0.0), (100.0, 0.0)));
    let mut c = Clipped::new("A1", "B", HERE, &inner);
    c.cut_away = Some(outer.to_vec());
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    let mut spans: Vec<(f64, f64)> = objects(&ed)
        .iter()
        .map(|o| (ends(o).0.x, ends(o).1.x))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert!((spans[0].1 - 20.0).abs() < 1e-6 && (spans[1].0 - 60.0).abs() < 1e-6);
}

fn circle_points(n: usize, r: f64) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            (r * a.cos(), r * a.sin())
        })
        .collect()
}

#[test]
fn a_clip_with_too_many_points_is_refused_with_a_note_and_stays_fast() {
    let b = block("B", &line((-10.0, 0.0), (10.0, 0.0)));
    let start = std::time::Instant::now();
    let c = Clipped::new("A1", "B", HERE, &circle_points(200_000, 500.0));
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    assert_eq!(objects(&ed).len(), 1);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("with more than 10000 points: it is not applied")),
        "{:?}",
        rep.notes
    );
    assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
}

#[test]
fn a_clip_of_10000_points_cuts_20000_lines_fast() {
    let mut lines = String::new();
    for i in 0..20_000 {
        let y = -900.0 + f64::from(i) * 0.09;
        lines += &line((-2000.0, y), (2000.0, y));
    }
    let b = block("B", &lines);
    let start = std::time::Instant::now();
    let c = Clipped::new("A1", "B", HERE, &circle_points(10_000, 1000.0));
    let (ed, rep) = import_with(file(&b, "", &[c]).as_bytes(), &opts()).unwrap();
    accounted(&rep);
    assert_eq!(objects(&ed).len(), 20_000);
    // Each chord of the circle at the height y is 2 sqrt(r^2 - y^2) long.
    let l = ends(objects(&ed)[10_000]);
    assert!((l.1.x - l.0.x - 2.0 * (1000.0_f64.powi(2) - (l.0.y).powi(2)).sqrt()).abs() < 0.1);
    assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
}
