//! Pattern hatches (IOP-27): the pattern of a HATCH entity becomes a hatch fill of the document,
//! and a hatch fill is written as a HATCH entity. Boundaries, islands, scale, turn, units,
//! blocks, limits and the round trip.

mod support;

use kadakas_core::DVec2;
use kadakas_core::command::Editor;
use kadakas_core::graphics::{GraphicResource, HatchDef, HatchLine, PatternUnits};
use kadakas_core::model::{Attributes, Color, Document, Object, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_io::{
    Context, ExportOptions, Format, ImportOptions, Report, Source, exporter, importer,
};
use support::{dxf, import, import_with};

/// The accounting rule: no entity is lost without a line in the report.
fn accounted(rep: &Report) {
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
}

/// Millimeter drawings.
fn mm(entities: &str) -> String {
    dxf("9\n$INSUNITS\n70\n4\n9\n$ACADVER\n1\nAC1027\n", entities)
}

/// One pattern line: (angle, base x, base y, offset x, offset y, dashes).
type Line = (f64, f64, f64, f64, f64, &'static [f64]);

const ANSI31: Line = (45.0, 0.0, 0.0, -2.2450640303, 2.2450640303, &[]);

/// The boundary paths: polyline paths with these corners.
fn paths(rings: &[&[(f64, f64)]]) -> String {
    let mut s = format!("91\n{}\n", rings.len());
    for (i, r) in rings.iter().enumerate() {
        s += &format!(
            "92\n{}\n72\n0\n73\n1\n93\n{}\n",
            if i == 0 { 7 } else { 23 },
            r.len()
        );
        for (x, y) in *r {
            s += &format!("10\n{x}\n20\n{y}\n");
        }
        s += "97\n0\n";
    }
    s
}

/// A HATCH entity with a pattern.
fn hatch(rings: &[&[(f64, f64)]], name: &str, scale: f64, angle: f64, lines: &[Line]) -> String {
    hatch_ext(rings, name, scale, angle, lines, 0, "")
}

fn hatch_ext(
    rings: &[&[(f64, f64)]],
    name: &str,
    scale: f64,
    angle: f64,
    lines: &[Line],
    double: u8,
    extra: &str,
) -> String {
    let mut s = format!("0\nHATCH\n8\nH\n{extra}210\n0\n220\n0\n230\n1\n2\n{name}\n70\n0\n71\n0\n");
    s += &paths(rings);
    s += &format!(
        "75\n0\n76\n1\n52\n{angle}\n41\n{scale}\n77\n{double}\n78\n{}\n",
        lines.len()
    );
    for (a, bx, by, ox, oy, d) in lines {
        s += &format!(
            "53\n{a}\n43\n{bx}\n44\n{by}\n45\n{ox}\n46\n{oy}\n79\n{}\n",
            d.len()
        );
        for x in *d {
            s += &format!("49\n{x}\n");
        }
    }
    s + "47\n1\n98\n1\n10\n0\n20\n0\n"
}

const SQ100: [(f64, f64); 4] = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
const SQ40: [(f64, f64); 4] = [(30.0, 30.0), (70.0, 30.0), (70.0, 70.0), (30.0, 70.0)];

fn objects(ed: &Editor) -> Vec<&Object> {
    ed.doc().objects.values().collect()
}

fn hatches(ed: &Editor) -> Vec<(ResourceId, String, HatchDef)> {
    ed.doc()
        .resources
        .iter()
        .filter_map(|r| match &r.body {
            ResourceBody::Graphic(g) => match &**g {
                GraphicResource::Hatch(h) => Some((r.id, r.name.clone(), h.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn def_of(ed: &Editor, o: &Object) -> HatchDef {
    let id = o.attrs.fill_pattern.expect("a pattern");
    ed.doc().resources.hatch(id).expect("a hatch").clone()
}

fn total_length(ed: &Editor, o: &Object) -> f64 {
    let attrs = ed.doc().resolved_attrs(o);
    kadakas_core::pattern::decorate(ed.doc(), o, &attrs, 100.0)
        .map_or(0.0, |d| d.fill.iter().map(|s| s.a.distance(s.b)).sum())
}

fn area(o: &Object) -> f64 {
    match &o.shape {
        Shape::Polyline { points, .. } => kadakas_core::curve::area(points).abs(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_pattern_hatch_becomes_a_hatch_fill() {
    let (ed, rep) =
        import(mm(&hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31])).as_bytes()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 1);
    let o = objs[0];
    assert!(matches!(o.shape, Shape::Polyline { closed: true, .. }));
    // The fill is the pattern, not a color.
    assert_eq!(o.attrs.fill, None);
    let all = hatches(&ed);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].1, "ANSI31");
    let d = &all[0].2;
    assert_eq!(d.units, PatternUnits::World);
    assert_eq!(d.lines.len(), 1);
    let l = &d.lines[0];
    assert!(
        (l.angle - 45.0).abs() < 1e-9 && (l.spacing - 3.175).abs() < 1e-6,
        "{l:?}"
    );
    assert!(l.dash.is_empty() && l.offset.abs() < 1e-9);
    // The lines cover the square: their length is the area over the spacing.
    let len = total_length(&ed, o);
    assert!((len - 10_000.0 / 3.175).abs() / len < 0.01, "{len}");
    assert!(
        rep.notes
            .iter()
            .any(|n| n
                .contains("1 hatch with a pattern became hatch fills of the document: ANSI31 1")),
        "{:?}",
        rep.notes
    );
    assert!(
        !rep.notes
            .iter()
            .any(|n| n.contains("outlines without the pattern"))
    );
}

#[test]
fn islands_keep_working_and_the_net_area_is_right() {
    let ents = hatch(&[&SQ100, &SQ40], "ANSI31", 1.0, 0.0, &[ANSI31]);
    let (ed, rep) = import(mm(&ents).as_bytes()).unwrap();
    accounted(&rep);
    let objs = objects(&ed);
    assert_eq!(objs.len(), 2);
    // The outer loop first, with the pattern. The island on top of it is white, without a pattern.
    assert!(objs[0].attrs.fill_pattern.is_some() && !objs[0].data.contains_key("hatch_island"));
    assert!(objs[1].attrs.fill_pattern.is_none());
    assert_eq!(objs[1].data["hatch_island"], true);
    assert_eq!(objs[1].attrs.fill, Some(Color([255, 255, 255, 255])));
    let net = area(objs[0]) - area(objs[1]);
    assert!((net - 8400.0).abs() < 1e-6, "{net}");
    assert!(
        rep.notes.iter().any(|n| n.contains("1 island of hatches")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn an_island_that_is_listed_first_is_still_drawn_on_top() {
    // The file lists the small loop before the big one. The outer loop must still come first.
    let ents = hatch(&[&SQ40, &SQ100], "ANSI31", 1.0, 0.0, &[ANSI31]);
    let (ed, _) = import(mm(&ents).as_bytes()).unwrap();
    let objs = objects(&ed);
    assert!(area(objs[0]) > area(objs[1]));
    assert!(objs[0].attrs.fill_pattern.is_some());
    assert!(objs[1].data.contains_key("hatch_island"));
}

#[test]
fn the_direction_of_the_boundary_does_not_change_the_result() {
    let cw: Vec<(f64, f64)> = SQ100.iter().rev().copied().collect();
    let (a, _) = import(mm(&hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31])).as_bytes()).unwrap();
    let (b, _) = import(mm(&hatch(&[&cw], "ANSI31", 1.0, 0.0, &[ANSI31])).as_bytes()).unwrap();
    let (la, lb) = (
        total_length(&a, objects(&a)[0]),
        total_length(&b, objects(&b)[0]),
    );
    assert!(la > 0.0 && (la - lb).abs() < 1e-6, "{la} {lb}");
    assert!((area(objects(&a)[0]) - area(objects(&b)[0])).abs() < 1e-9);
}

#[test]
fn a_mirrored_ocs_mirrors_the_pattern() {
    // Extrusion -Z: the x axis of the drawing runs the other way, and so do the lines of 45 degrees.
    let flip = "210\n0\n220\n0\n230\n-1\n";
    let mut e = hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31]);
    e = e.replace("210\n0\n220\n0\n230\n1\n", flip);
    let (ed, rep) = import(mm(&e).as_bytes()).unwrap();
    accounted(&rep);
    let o = objects(&ed)[0];
    let l = &def_of(&ed, o).lines[0];
    assert!((l.angle - 135.0).abs() < 1e-6, "{l:?}");
    assert!((l.spacing - 3.175).abs() < 1e-6);
    // The square is mirrored too: the pattern still covers it.
    assert!(total_length(&ed, o) > 3000.0);
}

#[test]
fn the_unit_of_the_file_scales_the_pattern() {
    // A file in meters: the spacing is 3.175 mm, written as 0.003175.
    let line: Line = (45.0, 0.0, 0.0, -0.0022450640303, 0.0022450640303, &[]);
    let sq: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let e = dxf(
        "9\n$INSUNITS\n70\n6\n",
        &hatch(&[&sq], "ANSI31", 1.0, 0.0, &[line]),
    );
    let (ed, rep) = import(e.as_bytes()).unwrap();
    accounted(&rep);
    let o = objects(&ed)[0];
    let l = &def_of(&ed, o).lines[0];
    assert!((l.spacing - 3.175).abs() < 1e-6, "{l:?}");
    assert!((area(o) - 1.0e6).abs() < 1.0);
}

#[test]
fn a_block_turns_and_scales_the_pattern_with_the_hatch() {
    let block = format!(
        "0\nBLOCK\n8\n0\n2\nB\n70\n0\n10\n0\n20\n0\n30\n0\n{}0\nENDBLK\n8\n0\n",
        hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31])
    );
    let ents = "0\nINSERT\n8\n0\n2\nB\n10\n1000\n20\n500\n41\n2\n42\n2\n43\n2\n50\n90\n";
    let src = format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{block}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
    );
    let (ed, rep) = import(src.as_bytes()).unwrap();
    accounted(&rep);
    let o = objects(&ed)[0];
    let l = &def_of(&ed, o).lines[0];
    // 45 degrees turned by 90, twice as large.
    assert!((l.angle - 135.0).abs() < 1e-6, "{l:?}");
    assert!((l.spacing - 6.35).abs() < 1e-6, "{l:?}");
    // The lines still fill the (200 x 200) square.
    let len = total_length(&ed, o);
    assert!((len - 40_000.0 / 6.35).abs() / len < 0.01, "{len}");
}

#[test]
fn equal_patterns_share_one_resource() {
    let mut ents = String::new();
    for i in 0..5 {
        let d = f64::from(i) * 500.0;
        let sq: Vec<(f64, f64)> = SQ100.iter().map(|(x, y)| (x + d, *y)).collect();
        // Different places of the same pattern: the lines only differ by whole steps when the
        // distance is a multiple of the step, so use steps of 3.175 * 40 along the line.
        ents += &hatch(&[&sq], "ANSI31", 1.0, 0.0, &[ANSI31]);
    }
    let (ed, rep) = import(mm(&ents).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(objects(&ed).len(), 5);
    assert_eq!(hatches(&ed).len(), 1);
    let ids: std::collections::BTreeSet<_> =
        objects(&ed).iter().map(|o| o.attrs.fill_pattern).collect();
    assert_eq!(ids.len(), 1);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("5 hatches with a pattern became hatch fills"))
    );
}

#[test]
fn a_hatch_of_the_document_with_the_same_lines_is_used() {
    // The document has ANSI31 already (the same lines), and a different hatch called ANSI32.
    let mut doc = Document::with_sequential_ids();
    let mut ed = Editor::new(std::mem::take(&mut doc));
    let add = |ed: &mut Editor, name: &str, line: HatchLine| {
        ed.execute(&kadakas_core::command::Command::AddResource {
            name: name.into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Hatch(HatchDef {
                units: PatternUnits::World,
                lines: vec![line],
            }))),
        })
        .unwrap();
    };
    add(&mut ed, "My diagonal", HatchLine::solid(45.0, 3.175));
    add(&mut ed, "ANSI32", HatchLine::solid(10.0, 5.0));
    let before = hatches(&ed);
    let src = Source {
        name: "t".into(),
        data: mm(&format!(
            "{}{}",
            hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31]),
            // The name ANSI32 is taken by another hatch: this one gets a free name.
            hatch(
                &[&SQ100],
                "ANSI32",
                1.0,
                0.0,
                &[(0.0, 0.0, 0.0, 0.0, 7.0, &[])]
            )
        ))
        .into_bytes(),
        extra: Default::default(),
    };
    let r = importer(Format::Dxf)
        .import(&src, &Context::of(ed.doc()), &ImportOptions::default())
        .unwrap();
    ed.execute(&r.command).unwrap();
    let after = hatches(&ed);
    assert_eq!(after.len(), before.len() + 1, "{after:?}");
    let new = after
        .iter()
        .find(|h| !before.iter().any(|b| b.0 == h.0))
        .unwrap();
    assert_eq!(new.1, "ANSI32 (2)");
    let objs = objects(&ed);
    assert_eq!(objs.len(), 2);
    // The first hatch took "My diagonal".
    assert_eq!(objs[0].attrs.fill_pattern, Some(before[0].0));
    assert_eq!(objs[1].attrs.fill_pattern, Some(new.0));
}

#[test]
fn a_name_without_lines_takes_the_standard_pattern() {
    // No pattern lines (78 is 0): ANSI31 at scale 2 and 90 degrees from the table.
    let e = "0\nHATCH\n8\nH\n210\n0\n220\n0\n230\n1\n2\nANSI31\n70\n0\n71\n0\n91\n1\n92\n7\n72\n0\n73\n1\n93\n4\n10\n0\n20\n0\n10\n100\n20\n0\n10\n100\n20\n100\n10\n0\n20\n100\n97\n0\n75\n0\n76\n1\n52\n90\n41\n2\n77\n0\n78\n0\n98\n1\n10\n1\n20\n1\n";
    let (ed, rep) = import(mm(e).as_bytes()).unwrap();
    accounted(&rep);
    let all = hatches(&ed);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].1, "ANSI31 x2 90 deg");
    let l = &all[0].2.lines[0];
    assert!(
        (l.angle - 135.0).abs() < 1e-6 && (l.spacing - 6.35).abs() < 1e-6,
        "{l:?}"
    );
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("1 hatch had a pattern name but no pattern lines"))
    );
    // The same entity with a name that is not a standard pattern stays an outline.
    let e = e.replace("2\nANSI31\n", "2\nMYPAT\n");
    let (ed, rep) = import(mm(&e).as_bytes()).unwrap();
    accounted(&rep);
    assert!(hatches(&ed).is_empty());
    assert_eq!(objects(&ed)[0].attrs.fill_pattern, None);
    assert!(
        rep.notes.iter().any(|n| n.contains("1 hatch with a pattern was imported as outlines without the pattern (1 with no pattern lines and not a standard pattern): MYPAT 1")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn every_standard_pattern_name_imports_with_or_without_lines() {
    for name in [
        "ANSI31", "ANSI32", "ANSI33", "ANSI34", "ANSI35", "ANSI36", "ANSI37", "ANSI38", "AR-B816",
        "AR-CONC", "AR-HBONE", "EARTH", "GRASS", "GRAVEL", "STEEL", "NET", "DOTS", "HEX",
    ] {
        let e = format!(
            "0\nHATCH\n8\nH\n210\n0\n220\n0\n230\n1\n2\n{name}\n70\n0\n71\n0\n{}75\n0\n76\n1\n52\n0\n41\n1\n77\n0\n78\n0\n98\n0\n",
            paths(&[&SQ100])
        );
        let (ed, rep) = import(mm(&e).as_bytes()).unwrap();
        accounted(&rep);
        let all = hatches(&ed);
        assert_eq!(all.len(), 1, "{name}");
        assert_eq!(all[0].1, name);
        assert!(total_length(&ed, objects(&ed)[0]) > 0.0, "{name}");
    }
}

#[test]
fn dashes_dots_and_the_stagger_are_kept() {
    // A dashed line: dash 6.35, gap 3.175, dot, gap 3.175. The next line is 5 across and 2 along.
    let line: Line = (0.0, 1.0, 2.0, 2.0, 5.0, &[6.35, -3.175, 0.0, -3.175]);
    let (ed, rep) = import(mm(&hatch(&[&SQ100], "DASHDOT", 1.0, 0.0, &[line])).as_bytes()).unwrap();
    accounted(&rep);
    let d = &hatches(&ed)[0].2;
    let l = &d.lines[0];
    assert_eq!(l.spacing, 5.0);
    assert!((l.offset - 2.0).abs() < 1e-9);
    // The period is the same as in the file: 12.7.
    assert!(
        (l.dash.iter().sum::<f64>() - 12.7).abs() < 1e-9,
        "{:?}",
        l.dash
    );
    assert_eq!(l.dash.len(), 4);
    let o = objects(&ed)[0];
    // 20 lines of 100 mm, about 6.35 + 0.1 of every 12.7 drawn.
    let len = total_length(&ed, o);
    let expect = 20.0 * 100.0 * (6.35 + 0.1) / 12.7;
    assert!((len - expect).abs() / expect < 0.1, "{len} {expect}");
}

#[test]
fn a_double_hatch_adds_the_lines_turned_by_90_degrees() {
    let e = hatch_ext(
        &[&SQ100],
        "_USER",
        1.0,
        0.0,
        &[(45.0, 0.0, 0.0, -2.0, 2.0, &[])],
        1,
        "",
    );
    let (ed, rep) = import(mm(&e).as_bytes()).unwrap();
    accounted(&rep);
    let all = hatches(&ed);
    // The name of the file is kept: a user pattern has none.
    assert_eq!(all[0].1, "DXF pattern");
    let angles: Vec<f64> = all[0].2.lines.iter().map(|l| l.angle).collect();
    assert_eq!(angles.len(), 2);
    assert!((angles[1] - angles[0] - 90.0).abs() < 1e-6, "{angles:?}");
}

#[test]
fn a_pattern_keeps_at_most_128_lines_and_says_so() {
    let lines: Vec<Line> = (0..200)
        .map(|i| (f64::from(i), 0.0, 0.0, -1.0, 4.0 + f64::from(i), &[][..]))
        .collect();
    let (ed, rep) = import(mm(&hatch(&[&SQ100], "BIG", 1.0, 0.0, &lines)).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(hatches(&ed)[0].2.lines.len(), 128);
    assert!(
        rep.notes.iter().any(|n| n.contains(
            "72 lines of hatch patterns were left out: a pattern keeps at most 128 lines"
        )),
        "{:?}",
        rep.notes
    );
}

#[test]
fn lines_that_lie_on_each_other_are_left_out() {
    // The second line has no part across the line: its lines would lie on each other.
    let good: Line = (0.0, 0.0, 0.0, 0.0, 5.0, &[]);
    let bad: Line = (0.0, 0.0, 0.0, 5.0, 0.0, &[]);
    let (ed, rep) =
        import(mm(&hatch(&[&SQ100], "HALF", 1.0, 0.0, &[good, bad])).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(hatches(&ed)[0].2.lines.len(), 1);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("1 line of hatch patterns was left out")),
        "{:?}",
        rep.notes
    );
    // Nothing is usable: the hatch is an outline.
    let (ed, rep) = import(mm(&hatch(&[&SQ100], "NONE", 1.0, 0.0, &[bad])).as_bytes()).unwrap();
    accounted(&rep);
    assert!(hatches(&ed).is_empty());
    assert_eq!(objects(&ed).len(), 1);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("1 with no usable pattern line")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn a_pattern_denser_than_the_engine_draws_stays_an_outline() {
    // Lines 0.001 mm apart in a square of 1 km: a million lines.
    let line: Line = (0.0, 0.0, 0.0, 0.0, 0.001, &[]);
    let sq: [(f64, f64); 4] = [(0.0, 0.0), (1.0e6, 0.0), (1.0e6, 1.0e6), (0.0, 1.0e6)];
    let (ed, rep) = import(mm(&hatch(&[&sq], "FINE", 1.0, 0.0, &[line])).as_bytes()).unwrap();
    accounted(&rep);
    assert!(hatches(&ed).is_empty());
    assert_eq!(objects(&ed).len(), 1);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("1 with lines closer than the drawing engine draws")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn hostile_pattern_data_does_not_panic() {
    // Missing numbers, text instead of numbers, a count that is far too big.
    let e = "0\nHATCH\n8\nH\n210\n0\n220\n0\n230\n1\n2\nX\n70\n0\n71\n0\n91\n1\n92\n7\n72\n0\n73\n1\n93\n3\n10\n0\n20\n0\n10\n9\n20\n0\n10\n9\n20\n9\n97\n0\n75\n0\n76\n1\n52\nabc\n41\n-3\n77\n1\n78\n999999999\n53\nxyz\n43\n1\n45\n3\n46\n3\n79\n2\n49\n1\n53\n10\n43\n0\n44\n0\n45\n0\n46\n4\n79\n1\n49\n5\n98\n1\n10\n1\n20\n1\n";
    let (ed, rep) = import(mm(e).as_bytes()).unwrap();
    accounted(&rep);
    // The first line has no angle: it is dropped and counted. The second is good.
    assert_eq!(objects(&ed).len(), 1);
    assert_eq!(hatches(&ed).len(), 1);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("1 line of hatch patterns was left out")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn a_solid_hatch_and_a_gradient_still_work() {
    let solid = "0\nHATCH\n8\nH\n62\n1\n210\n0\n220\n0\n230\n1\n2\nSOLID\n70\n1\n71\n0\n91\n1\n92\n7\n72\n0\n73\n1\n93\n3\n10\n0\n20\n0\n10\n9\n20\n0\n10\n9\n20\n9\n97\n0\n75\n0\n76\n1\n98\n0\n";
    let (ed, rep) = import(mm(solid).as_bytes()).unwrap();
    accounted(&rep);
    assert!(hatches(&ed).is_empty());
    assert_eq!(objects(&ed)[0].attrs.fill_pattern, None);
    assert!(objects(&ed)[0].attrs.fill.is_some());
}

#[test]
fn a_hatch_in_a_tilted_plane_has_no_fill_pattern() {
    let tilted = hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31])
        .replace("210\n0\n220\n0\n230\n1\n", "210\n1\n220\n0\n230\n0\n");
    let (ed, rep) = import(mm(&tilted).as_bytes()).unwrap();
    accounted(&rep);
    assert!(hatches(&ed).is_empty());
    assert!(matches!(objects(&ed)[0].shape, Shape::Polyline3d { .. }));
}

#[test]
fn hatches_are_off_with_the_option() {
    let (ed, rep) = import_with(
        mm(&hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31])).as_bytes(),
        &ImportOptions {
            hatches: false,
            ..ImportOptions::default()
        },
    )
    .unwrap();
    accounted(&rep);
    assert!(objects(&ed).is_empty() && hatches(&ed).is_empty());
    assert_eq!(rep.entities_skipped, 1);
}

// Export ---------------------------------------------------------------------------------------

/// A document with one square and the hatch fill that `lines` make.
fn doc_with_fill(name: &str, units: PatternUnits, lines: Vec<HatchLine>, shape: Shape) -> Editor {
    let mut ed = Editor::new(Document::with_sequential_ids());
    ed.execute(&kadakas_core::command::Command::AddResource {
        name: name.into(),
        body: ResourceBody::Graphic(Box::new(GraphicResource::Hatch(HatchDef { units, lines }))),
    })
    .unwrap();
    let id = hatches(&ed)[0].0;
    ed.execute(&kadakas_core::command::Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes {
            pen: Some(Color([200, 0, 0, 255])),
            fill_pattern: Some(id),
            ..Attributes::default()
        },
    })
    .unwrap();
    ed
}

fn square_shape(side: f64) -> Shape {
    Shape::Polyline {
        points: [(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)]
            .iter()
            .map(|(x, y)| kadakas_core::curve::Vertex::new(DVec2::new(*x, *y), Default::default()))
            .collect(),
        closed: true,
    }
}

fn export_text(ed: &Editor, opts: ExportOptions) -> (String, Report) {
    let r = exporter(Format::Dxf)
        .unwrap()
        .export(ed.doc(), &opts)
        .unwrap();
    (String::from_utf8(r.bytes).unwrap(), r.report)
}

fn pairs_of(text: &str, kind: &str) -> Vec<(i32, String)> {
    let all = support::pairs(text);
    let mut out = Vec::new();
    let mut on = false;
    for (c, v) in all {
        if c == 0 {
            on = v == kind;
        }
        if on {
            out.push((c, v));
        }
    }
    out
}

#[test]
fn a_hatch_fill_is_written_as_a_hatch_entity_with_an_ansi_name() {
    let ed = doc_with_fill(
        "Diagonal",
        PatternUnits::World,
        vec![HatchLine::solid(45.0, 3.175)],
        square_shape(100.0),
    );
    let (text, report) = export_text(&ed, ExportOptions::default());
    let h = pairs_of(&text, "HATCH");
    assert!(!h.is_empty(), "{text}");
    let get = |code: i32| h.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
    assert_eq!(get(2).as_deref(), Some("ANSI31"));
    assert_eq!(get(70).as_deref(), Some("0"));
    assert_eq!(get(76).as_deref(), Some("1"));
    assert_eq!(get(78).as_deref(), Some("1"));
    // The boundary path is the square, a polyline path.
    assert_eq!(get(91).as_deref(), Some("1"));
    assert_eq!(h.iter().filter(|(c, _)| *c == 10).count(), 4 + 1 + 1);
    // No LINE entities for the pattern: the hatch is the pattern.
    assert!(pairs_of(&text, "LINE").is_empty());
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("1 hatch fill was written as HATCH entities")),
        "{:?}",
        report.notes
    );
    // The offset is the vector across the lines: 3.175 at 45 degrees.
    let off = (
        h.iter()
            .find(|(c, _)| *c == 45)
            .unwrap()
            .1
            .parse::<f64>()
            .unwrap(),
        h.iter()
            .find(|(c, _)| *c == 46)
            .unwrap()
            .1
            .parse::<f64>()
            .unwrap(),
    );
    assert!(
        (off.0 + 2.2450640303).abs() < 1e-6 && (off.1 - 2.2450640303).abs() < 1e-6,
        "{off:?}"
    );
}

#[test]
fn a_hatch_that_is_not_standard_gets_its_own_name_and_a_scaled_one_the_scale() {
    // Paper units at 1:100: 2 paper mm are 200 model mm.
    let ed = doc_with_fill(
        "Wet sand 7",
        PatternUnits::Paper,
        vec![HatchLine::solid(10.0, 2.0)],
        square_shape(2000.0),
    );
    let (text, _) = export_text(&ed, ExportOptions::default());
    let h = pairs_of(&text, "HATCH");
    let get = |code: i32| h.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
    assert_ne!(get(2).as_deref(), Some("ANSI31"));
    // One line of 10 degrees is ANSI31 at a turn other than 0, and a single family is named at the
    // turn 0 only: it gets its own name.
    assert_eq!(get(2).as_deref(), Some("WET_SAND_7"));
    // ANSI37 (a cross of lines) drawn twice as large and turned 30 degrees is found with scale and angle.
    let ed = doc_with_fill(
        "x",
        PatternUnits::World,
        vec![HatchLine::solid(75.0, 6.35), HatchLine::solid(165.0, 6.35)],
        square_shape(500.0),
    );
    let (text, _) = export_text(&ed, ExportOptions::default());
    let h = pairs_of(&text, "HATCH");
    let get = |code: i32| {
        h.iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.parse::<f64>().ok())
    };
    assert_eq!(h.iter().find(|(c, _)| *c == 2).unwrap().1, "ANSI37");
    assert_eq!(get(41), Some(Some(2.0)));
    assert_eq!(get(52), Some(Some(30.0)));
}

#[test]
fn a_hatch_with_a_color_on_a_line_stays_plain_lines() {
    let ed = doc_with_fill(
        "Red",
        PatternUnits::World,
        vec![HatchLine {
            color: Some(Color([255, 0, 0, 255])),
            ..HatchLine::solid(0.0, 10.0)
        }],
        square_shape(100.0),
    );
    let (text, _) = export_text(&ed, ExportOptions::default());
    assert!(pairs_of(&text, "HATCH").is_empty());
    assert!(!pairs_of(&text, "LINE").is_empty());
}

#[test]
fn a_hatch_fill_round_trips_through_the_file() {
    let line = HatchLine {
        angle: 30.0,
        spacing: 12.0,
        offset: 3.0,
        origin: DVec2::new(7.0, 11.0),
        dash: vec![20.0, 5.0, 4.0, 5.0],
        color: None,
        weight: None,
    };
    let ed = doc_with_fill(
        "Custom",
        PatternUnits::World,
        vec![line.clone(), HatchLine::solid(120.0, 25.0)],
        square_shape(400.0),
    );
    let (text, _) = export_text(&ed, ExportOptions::default());
    let (back, rep) = import_with(text.as_bytes(), &ImportOptions::default()).unwrap();
    accounted(&rep);
    // The outline of the object and the boundary of the HATCH.
    assert_eq!(objects(&back).len(), 2);
    let objs: Vec<&Object> = objects(&back)
        .into_iter()
        .filter(|o| o.attrs.fill_pattern.is_some())
        .collect();
    assert_eq!(objs.len(), 1);
    let d = def_of(&back, objs[0]);
    assert_eq!(d.lines.len(), 2);
    // The same lines (the origin may be another point of the same lines).
    let a = &d.lines[0];
    assert!(
        (a.angle - 30.0).abs() < 1e-6
            && (a.spacing - 12.0).abs() < 1e-6
            && (a.offset - 3.0).abs() < 1e-6,
        "{a:?}"
    );
    assert_eq!(a.dash.len(), 4);
    for (x, y) in a.dash.iter().zip(&line.dash) {
        assert!((x - y).abs() < 1e-6, "{:?}", a.dash);
    }
    // Same area and the same length of lines.
    assert!((area(objs[0]) - 160_000.0).abs() < 1e-3);
    let (len_a, len_b) = (
        total_length(&ed, ed.doc().objects.values().next().unwrap()),
        total_length(&back, objs[0]),
    );
    assert!((len_a - len_b).abs() / len_a < 0.02, "{len_a} {len_b}");
}

#[test]
fn a_standard_pattern_round_trips_with_its_name() {
    let ed = doc_with_fill(
        "x",
        PatternUnits::World,
        vec![HatchLine::solid(45.0, 3.175)],
        square_shape(100.0),
    );
    let (text, _) = export_text(&ed, ExportOptions::default());
    let (back, _) = import(text.as_bytes()).unwrap();
    assert_eq!(hatches(&back)[0].1, "ANSI31");
}

#[test]
fn the_unit_and_the_origin_of_the_export_apply_to_the_pattern() {
    // In meters: the numbers of the pattern are in meters too.
    let ed = doc_with_fill(
        "x",
        PatternUnits::World,
        vec![HatchLine::solid(0.0, 500.0)],
        square_shape(10_000.0),
    );
    let (text, _) = export_text(
        &ed,
        ExportOptions {
            unit: kadakas_io::Unit::M,
            ..ExportOptions::default()
        },
    );
    let h = pairs_of(&text, "HATCH");
    let off_y = h
        .iter()
        .find(|(c, _)| *c == 46)
        .unwrap()
        .1
        .parse::<f64>()
        .unwrap();
    assert!((off_y - 0.5).abs() < 1e-9, "{off_y}");
    let (back, _) = import(text.as_bytes()).unwrap();
    let l = &hatches(&back)[0].2.lines[0];
    assert!((l.spacing - 500.0).abs() < 1e-6, "{l:?}");
}

#[test]
fn a_landscape_area_with_a_hole_is_a_hatch_with_two_paths() {
    use kadakas_core::curve::Vertex;
    let v = |x: f64, y: f64| Vertex::new(DVec2::new(x, y), Default::default());
    let mut ed = doc_with_fill(
        "Grass",
        PatternUnits::World,
        vec![HatchLine::solid(0.0, 20.0)],
        Shape::LandscapeArea {
            boundary: vec![v(0.0, 0.0), v(300.0, 0.0), v(300.0, 300.0), v(0.0, 300.0)],
            name: "Lawn".into(),
        },
    );
    let id = *ed.doc().objects.keys().next().unwrap();
    ed.execute(&kadakas_core::command::Command::SetLandscapeArea {
        ids: vec![id],
        patch: kadakas_core::landscape::LandscapePatch {
            holes: Some(vec![vec![
                v(100.0, 100.0),
                v(200.0, 100.0),
                v(200.0, 200.0),
                v(100.0, 200.0),
            ]]),
            ..Default::default()
        },
    })
    .unwrap();
    let (text, _) = export_text(&ed, ExportOptions::default());
    let h = pairs_of(&text, "HATCH");
    assert_eq!(h.iter().find(|(c, _)| *c == 91).unwrap().1, "2");
    // The outer path is the external one (3), the hole an outermost polyline path (18).
    let flags: Vec<&str> = h
        .iter()
        .filter(|(c, _)| *c == 92)
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(flags, ["3", "18"]);
    // The seed point is in the ring, not in the hole.
    let seed = h
        .iter()
        .rev()
        .find(|(c, _)| *c == 10)
        .unwrap()
        .1
        .parse::<f64>()
        .unwrap();
    assert!(seed > 0.0 && seed < 300.0);
}

#[test]
fn a_circle_and_a_rectangle_are_hatch_boundaries_and_an_open_line_is_not() {
    for (shape, paths_expected) in [
        (
            Shape::Circle {
                center: DVec2::new(50.0, 50.0),
                radius: 40.0,
            },
            1,
        ),
        (
            Shape::Rect {
                min: DVec2::new(0.0, 0.0),
                max: DVec2::new(90.0, 60.0),
            },
            1,
        ),
        (
            Shape::Line {
                a: DVec2::new(0.0, 0.0),
                b: DVec2::new(10.0, 10.0),
            },
            0,
        ),
    ] {
        let ed = doc_with_fill(
            "x",
            PatternUnits::World,
            vec![HatchLine::solid(45.0, 3.175)],
            shape,
        );
        let (text, _) = export_text(&ed, ExportOptions::default());
        let n = pairs_of(&text, "HATCH").len();
        assert_eq!(n > 0, paths_expected > 0);
    }
}

#[test]
fn the_pattern_of_a_hatch_is_written_in_every_release_with_hatches() {
    use kadakas_io::DxfVersion;
    let ed = doc_with_fill(
        "x",
        PatternUnits::World,
        vec![HatchLine::solid(45.0, 3.175)],
        square_shape(100.0),
    );
    for v in [
        DxfVersion::R2000,
        DxfVersion::R2004,
        DxfVersion::R2013,
        DxfVersion::R2018,
    ] {
        let (text, _) = export_text(
            &ed,
            ExportOptions {
                version: v,
                ..ExportOptions::default()
            },
        );
        assert!(!pairs_of(&text, "HATCH").is_empty(), "{v:?}");
    }
    // Release 12 has no HATCH. The pattern is not drawn there (as before).
    let (text, _) = export_text(
        &ed,
        ExportOptions {
            version: DxfVersion::R12,
            ..ExportOptions::default()
        },
    );
    assert!(pairs_of(&text, "HATCH").is_empty());
}

#[test]
fn release_12_says_that_it_writes_the_outline_only() {
    let ed = doc_with_fill(
        "x",
        PatternUnits::World,
        vec![HatchLine::solid(45.0, 3.175)],
        square_shape(100.0),
    );
    let (_, report) = export_text(
        &ed,
        ExportOptions {
            version: kadakas_io::DxfVersion::R12,
            ..ExportOptions::default()
        },
    );
    assert!(
        report.notes.iter().any(|n| n.contains(
            "1 object has a hatch fill, a tile or a line type: release 12 has no HATCH entity"
        )),
        "{:?}",
        report.notes
    );
}

// The review of IOP-27 -------------------------------------------------------------------------

/// The numbers of a code in the order of the file.
fn codes(text: &str, kind: &str, code: i32) -> Vec<String> {
    pairs_of(text, kind)
        .into_iter()
        .filter(|(c, _)| *c == code)
        .map(|(_, v)| v)
        .collect()
}

#[test]
fn a_user_defined_double_hatch_that_has_both_halves_is_not_doubled_again() {
    // The file holds the lines at 0 and at 90 degrees, and says "double" (77).
    let both: [Line; 2] = [
        (0.0, 0.0, 0.0, 0.0, 5.0, &[]),
        (90.0, 0.0, 0.0, -5.0, 0.0, &[]),
    ];
    let e = hatch_ext(&[&SQ100], "_USER", 1.0, 0.0, &both, 1, "");
    let (ed, rep) = import(mm(&e).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(hatches(&ed)[0].2.lines.len(), 2);
    let ed2 = doc_with_fill(
        "x",
        PatternUnits::World,
        hatches(&ed)[0].2.lines.clone(),
        square_shape(100.0),
    );
    let (text, _) = export_text(&ed2, ExportOptions::default());
    assert_eq!(codes(&text, "HATCH", 78), ["2"]);
    // A table pattern with the double flag gets its second half.
    let t = format!(
        "0\nHATCH\n8\nH\n210\n0\n220\n0\n230\n1\n2\nANSI31\n70\n0\n71\n0\n{}75\n0\n76\n1\n52\n0\n41\n1\n77\n1\n78\n0\n98\n0\n",
        paths(&[&SQ100])
    );
    let (ed, _) = import(mm(&t).as_bytes()).unwrap();
    assert_eq!(hatches(&ed)[0].2.lines.len(), 2);
}

#[test]
fn a_dash_list_that_starts_with_a_gap_does_not_gain_a_dot_and_a_short_dash_stays() {
    for dashes in [
        &[-3.0, 5.0][..],
        &[0.12, -3.0][..],
        &[4.0, -1.0, 0.0, -1.0][..],
    ] {
        let line: Line = (0.0, 0.0, 0.0, 0.0, 10.0, dashes);
        let (ed, rep) =
            import(mm(&hatch(&[&SQ100], "GAPFIRST", 1.0, 0.0, &[line])).as_bytes()).unwrap();
        accounted(&rep);
        let ed2 = doc_with_fill(
            "x",
            PatternUnits::World,
            hatches(&ed)[0].2.lines.clone(),
            square_shape(100.0),
        );
        let (text, _) = export_text(&ed2, ExportOptions::default());
        let back: Vec<f64> = codes(&text, "HATCH", 49)
            .iter()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(back.len(), dashes.len(), "{dashes:?} {back:?}");
        for (x, y) in back.iter().zip(dashes) {
            assert!((x - y).abs() < 1e-6, "{dashes:?} {back:?}");
        }
    }
}

#[test]
fn thirty_hatches_of_one_pattern_on_the_lattice_make_one_resource() {
    let mut ents = String::new();
    for k in 0..30 {
        // The base points are steps of the offset vector (-2.2450640303, 2.2450640303), with rounding noise.
        let m = f64::from(k) * 7.0 + 3.0;
        let line: Line = (
            45.0,
            -2.2450640303 * m + 1e-11,
            2.2450640303 * m - 1e-11,
            -2.2450640303,
            2.2450640303,
            &[],
        );
        ents += &hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[line]);
    }
    let (ed, rep) = import(mm(&ents).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(hatches(&ed).len(), 1, "{:?}", hatches(&ed));
    assert_eq!(objects(&ed).len(), 30);
}

#[test]
fn a_hatch_of_the_document_whose_origin_was_never_reduced_is_used() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    // The same lines as ANSI31, but the origin is far out on the lattice.
    let far = HatchLine {
        origin: DVec2::new(-2.2450640303 * 40.0, 2.2450640303 * 40.0),
        ..HatchLine::solid(45.0, 3.175)
    };
    ed.execute(&kadakas_core::command::Command::AddResource {
        name: "Mine".into(),
        body: ResourceBody::Graphic(Box::new(GraphicResource::Hatch(HatchDef {
            units: PatternUnits::World,
            lines: vec![far],
        }))),
    })
    .unwrap();
    let before = hatches(&ed);
    let src = Source {
        name: "t".into(),
        data: mm(&hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31])).into_bytes(),
        extra: Default::default(),
    };
    let r = importer(Format::Dxf)
        .import(&src, &Context::of(ed.doc()), &ImportOptions::default())
        .unwrap();
    ed.execute(&r.command).unwrap();
    assert_eq!(hatches(&ed).len(), before.len());
    assert_eq!(objects(&ed)[0].attrs.fill_pattern, Some(before[0].0));
}

#[test]
fn a_scaled_and_turned_single_family_keeps_its_ansi_name() {
    // ANSI31 at twice the size and turned by 90 degrees is one line at 135 degrees.
    let mut ed = doc_with_fill(
        "ANSI31 x2 90 deg",
        PatternUnits::World,
        vec![HatchLine::solid(135.0, 6.35)],
        square_shape(500.0),
    );
    let (text, _) = export_text(&ed, ExportOptions::default());
    assert_eq!(codes(&text, "HATCH", 2), ["ANSI31"]);
    assert_eq!(codes(&text, "HATCH", 41), ["2"]);
    assert_eq!(codes(&text, "HATCH", 52), ["90"]);
    // A small scale keeps its name too.
    ed = doc_with_fill(
        "ANSI31 x0.0003",
        PatternUnits::World,
        vec![HatchLine::solid(45.0, 3.175 * 0.0003)],
        square_shape(5.0),
    );
    let (text, _) = export_text(&ed, ExportOptions::default());
    assert_eq!(codes(&text, "HATCH", 2), ["ANSI31"]);
}

#[test]
fn dots_show_on_a_sheet_at_1_to_100() {
    // DOTS at the scale 100: the dot is at least 0.1 mm on paper, 10 mm of model.
    let e = format!(
        "0\nHATCH\n8\nH\n210\n0\n220\n0\n230\n1\n2\nDOTS\n70\n0\n71\n0\n{}75\n0\n76\n1\n52\n0\n41\n100\n77\n0\n78\n0\n98\n0\n",
        paths(&[&[(0.0, 0.0), (5000.0, 0.0), (5000.0, 5000.0), (0.0, 5000.0)]])
    );
    let (ed, rep) = import(mm(&e).as_bytes()).unwrap();
    accounted(&rep);
    let o = objects(&ed)[0];
    let attrs = ed.doc().resolved_attrs(o);
    let d = kadakas_core::pattern::decorate(ed.doc(), o, &attrs, 100.0).unwrap();
    assert!(d.fill.len() > 50);
    assert!(
        d.fill.iter().all(|s| s.a.distance(s.b) >= 10.0),
        "{:?}",
        d.fill.first()
    );
}

#[test]
fn a_pattern_in_a_tilted_block_makes_no_resource_and_one_note() {
    let b = format!(
        "0\nBLOCK\n8\n0\n2\nB\n70\n0\n10\n0\n20\n0\n30\n0\n{}0\nENDBLK\n8\n0\n",
        hatch(&[&SQ100], "ANSI31", 1.0, 0.0, &[ANSI31])
    );
    let ins = "0\nINSERT\n8\n0\n2\nB\n210\n1\n220\n0\n230\n0\n10\n0\n20\n0\n30\n0\n";
    let src = format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{b}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ins}0\nENDSEC\n0\nEOF\n"
    );
    let (ed, rep) = import(src.as_bytes()).unwrap();
    accounted(&rep);
    assert!(hatches(&ed).is_empty(), "no orphan resource");
    let n = rep.notes.join("\n");
    assert!(n.contains("1 filled shape lost its fill"), "{n}");
    assert!(!n.contains("became hatch fills"), "{n}");
}

#[test]
fn lists_that_draw_nothing_or_have_no_period_are_reported_not_imported() {
    for dashes in [&[-5.0][..], &[0.0][..], &[1e-300, -1e-300][..]] {
        let line: Line = (0.0, 0.0, 0.0, 0.0, 10.0, dashes);
        let (ed, rep) = import(mm(&hatch(&[&SQ100], "BAD", 1.0, 0.0, &[line])).as_bytes()).unwrap();
        accounted(&rep);
        assert!(hatches(&ed).is_empty(), "{dashes:?}");
        let n = rep.notes.join("\n");
        assert!(n.contains("no usable pattern line"), "{dashes:?} {n}");
        assert!(!n.contains("became hatch fills"), "{n}");
    }
}

#[test]
fn many_families_together_that_are_too_dense_stay_an_outline() {
    // 100 families, each of 90,000 lines in the box: 9 million lines, more than the engine draws.
    let lines: Vec<Line> = (0..100)
        .map(|i| (f64::from(i), 0.0, 0.0, 0.0, 1.0, &[][..]))
        .collect();
    let sq: [(f64, f64); 4] = [
        (0.0, 0.0),
        (90_000.0, 0.0),
        (90_000.0, 90_000.0),
        (0.0, 90_000.0),
    ];
    let (ed, rep) = import(mm(&hatch(&[&sq], "MANY", 1.0, 0.0, &lines)).as_bytes()).unwrap();
    accounted(&rep);
    assert!(hatches(&ed).is_empty());
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("lines closer than the drawing engine draws"))
    );
}

#[test]
fn the_islands_of_a_pattern_hatch_are_holes_of_the_hatch_on_export() {
    let ents = hatch(&[&SQ100, &SQ40], "ANSI31", 1.0, 0.0, &[ANSI31]);
    let (ed, _) = import(mm(&ents).as_bytes()).unwrap();
    let outer = objects(&ed)[0];
    assert!(outer.data.contains_key("hatch_islands"));
    let (text, _) = export_text(&ed, ExportOptions::default());
    let h = pairs_of(&text, "HATCH");
    assert_eq!(h.iter().find(|(c, _)| *c == 91).unwrap().1, "2");
    let flags: Vec<&str> = h
        .iter()
        .filter(|(c, _)| *c == 92)
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(flags, ["3", "18"]);
    // The file reads back as the same hatch: an outer loop and one island.
    let (back, rep) = import(text.as_bytes()).unwrap();
    accounted(&rep);
    let islands = objects(&back)
        .iter()
        .filter(|o| o.data.contains_key("hatch_island"))
        .count();
    assert!(islands >= 1);
}

#[test]
fn lines_beyond_the_read_limit_are_counted_as_not_read() {
    let lines: Vec<Line> = (0..10_050)
        .map(|i| (0.0, 0.0, 0.0, 0.0, 1.0 + f64::from(i % 5), &[][..]))
        .collect();
    let (ed, rep) = import(mm(&hatch(&[&SQ100], "HUGE", 1.0, 0.0, &lines)).as_bytes()).unwrap();
    accounted(&rep);
    assert_eq!(hatches(&ed)[0].2.lines.len(), 128);
    let n = rep.notes.join("\n");
    assert!(
        n.contains("50 lines of hatch patterns were not read"),
        "{n}"
    );
    assert!(
        !n.contains("cannot be used") && !n.contains("lie on each other"),
        "{n}"
    );
}
