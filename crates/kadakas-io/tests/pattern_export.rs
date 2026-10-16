//! The unit pattern of hardscapes in a DXF export: counts, areas, layers, limits and a round trip.

use std::collections::BTreeMap;

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{self, corners};
use kadakas_core::hardscape::style::{HardscapeStyle, JointKind, JointPattern};
use kadakas_core::hardscape::{self, HardscapePatch};
use kadakas_core::layout::summary::{SummaryOptions, summarize};
use kadakas_core::layout::{self, object};
use kadakas_core::model::{Attributes, Document, ObjectId};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{
    Context, ExportOptions, Exporter, ImportOptions, Importer, IoError, PatternExport, Source, Unit,
};

fn lot(
    kind: JointKind,
    w: f64,
    h: f64,
    holes: Vec<Vec<DVec2>>,
    origin: Option<DVec2>,
    angle: Option<f64>,
) -> (Editor, ObjectId) {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Pavers".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                joints: Some(JointPattern {
                    kind,
                    unit_length: 200.0,
                    unit_width: 100.0,
                    spacing: 5.0,
                    ..Default::default()
                }),
                ..Default::default()
            })),
        })
        .unwrap()
        .created[0],
    );
    let id = ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: Some(corners(&[
                DVec2::ZERO,
                DVec2::new(w, 0.0),
                DVec2::new(w, h),
                DVec2::new(0.0, h),
            ])),
            patch: Box::new(HardscapePatch {
                style: Some(style),
                holes: (!holes.is_empty()).then(|| holes.iter().map(|h| corners(h)).collect()),
                origin,
                angle,
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    (ed, id)
}

fn island() -> Vec<DVec2> {
    vec![
        DVec2::new(3000.0, 1500.0),
        DVec2::new(4200.0, 1700.0),
        DVec2::new(4000.0, 3000.0),
        DVec2::new(3100.0, 2800.0),
    ]
}

fn export(doc: &Document, patterns: PatternExport) -> Result<kadakas_io::ExportResult, IoError> {
    DxfFormat.export(
        doc,
        &ExportOptions {
            patterns,
            ..ExportOptions::default()
        },
    )
}

/// Entities of one type by layer: (count, vertex lists).
fn entities(dxf: &str, kind: &str) -> BTreeMap<String, Vec<Vec<DVec2>>> {
    let lines: Vec<&str> = dxf.lines().collect();
    let mut out: BTreeMap<String, Vec<Vec<DVec2>>> = BTreeMap::new();
    let mut i = 0;
    let mut in_entities = false;
    while i + 1 < lines.len() {
        let (code, value) = (lines[i].trim(), lines[i + 1].trim());
        if code == "2" && value == "ENTITIES" {
            in_entities = true;
        }
        if code == "0" && value == "ENDSEC" {
            in_entities = false;
        }
        if in_entities && code == "0" && value == kind {
            let mut layer = String::new();
            let mut pts: Vec<DVec2> = Vec::new();
            let mut x = None;
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let (c, v) = (lines[j].trim(), lines[j + 1].trim());
                match c {
                    "8" => layer = v.to_string(),
                    "10" => x = v.parse::<f64>().ok(),
                    "20" => {
                        if let (Some(px), Ok(py)) = (x.take(), v.parse::<f64>()) {
                            pts.push(DVec2::new(px, py));
                        }
                    }
                    "11" | "21" if kind == "LINE" => {
                        if c == "11" {
                            x = v.parse::<f64>().ok();
                        } else if let (Some(px), Ok(py)) = (x.take(), v.parse::<f64>()) {
                            pts.push(DVec2::new(px, py));
                        }
                    }
                    _ => {}
                }
                j += 2;
            }
            out.entry(layer).or_default().push(pts);
            i = j;
            continue;
        }
        i += 2;
    }
    out
}

fn area(pts: &[DVec2]) -> f64 {
    kadakas_core::geom::polygon_area(pts)
}

#[test]
fn units_in_the_dxf_match_the_report_for_every_bond() {
    for kind in JointKind::ALL {
        let (ed, id) = lot(
            kind,
            9000.0,
            5000.0,
            vec![island()],
            Some(DVec2::new(0.0, 0.0)),
            Some(12.0),
        );
        let doc = ed.doc();
        let o = doc.object(id).unwrap();
        let (pattern, place) = object::pattern_of(doc, o).unwrap();
        let region = object::region_of(doc, o, 1.0).unwrap();
        let sum = summarize(&region, &pattern, &place, &SummaryOptions::default()).unwrap();
        let out = export(doc, PatternExport::Units).unwrap();
        let text = String::from_utf8(out.bytes).unwrap();
        let polys = entities(&text, "LWPOLYLINE");
        let base = "0";
        let whole = polys
            .get(&format!("{base}-UNITS-WHOLE"))
            .map_or(0, Vec::len);
        let cut = polys.get(&format!("{base}-UNITS-CUT")).map_or(0, Vec::len);
        assert_eq!((whole, cut), (sum.whole_units, sum.cut_units), "{kind:?}");
        assert!(sum.cut_units > 0 && sum.whole_units > 100);
        // The outline is still there, once.
        assert_eq!(polys.get(base).map_or(0, Vec::len), 1);
        // The area of the polylines is the area laid (a hole inside a unit would be a VOID polyline).
        let laid: f64 = polys
            .iter()
            .filter(|(l, _)| l.ends_with("-UNITS-WHOLE") || l.ends_with("-UNITS-CUT"))
            .flat_map(|(_, v)| v.iter())
            .map(|p| area(p))
            .sum();
        let voids: f64 = polys
            .get(&format!("{base}-UNITS-VOID"))
            .map_or(0.0, |v| v.iter().map(|p| area(p)).sum());
        assert!(
            (laid - voids - sum.area_laid_m2 * 1e6).abs() < 1.0,
            "{kind:?}: {laid} vs {}",
            sum.area_laid_m2 * 1e6
        );
        // Sum of unit areas + gap area == outline minus the hole.
        let hole = area(&island());
        assert!((sum.region_area_m2 * 1e6 - (9000.0 * 5000.0 - hole)).abs() < 1e-3);
        assert!(
            (sum.area_laid_m2 + sum.gap_area_m2 - sum.region_area_m2).abs() < 1e-9,
            "{kind:?}"
        );
        // The report of the export names the counts.
        assert!(
            out.report.notes.iter().any(|n| n.contains(&format!(
                "wrote {} whole and {} cut",
                sum.whole_units, sum.cut_units
            ))),
            "{:?}",
            out.report.notes
        );
    }
}

#[test]
fn units_stay_off_the_island_and_inside_the_lot() {
    let (ed, _) = lot(
        JointKind::RunningBond,
        9000.0,
        5000.0,
        vec![island()],
        None,
        None,
    );
    let text = String::from_utf8(export(ed.doc(), PatternExport::Units).unwrap().bytes).unwrap();
    let polys = entities(&text, "LWPOLYLINE");
    let hole = island();
    for (layer, list) in &polys {
        if !layer.contains("-UNITS-") {
            continue;
        }
        for p in list {
            let n = hole.len();
            for q in p {
                let near_edge = (0..n).any(|i| {
                    kadakas_core::geom::dist_to_segment(*q, hole[i], hole[(i + 1) % n]) < 1e-6
                });
                assert!(
                    near_edge || !kadakas_core::geom::point_in_polygon(*q, &hole),
                    "a unit corner lies on the island: {q:?}"
                );
            }
            assert!(p.iter().all(|q| q.x >= -1e-6
                && q.x <= 9000.0 + 1e-6
                && q.y >= -1e-6
                && q.y <= 5000.0 + 1e-6));
        }
    }
}

#[test]
fn gaps_are_lines_and_nothing_is_written_by_default() {
    let (ed, id) = lot(
        JointKind::Stack,
        2050.0,
        1050.0,
        vec![],
        Some(DVec2::ZERO),
        None,
    );
    let out = export(ed.doc(), PatternExport::Gaps).unwrap();
    let text = String::from_utf8(out.bytes).unwrap();
    let lines = entities(&text, "LINE");
    let o = ed.doc().object(id).unwrap();
    let (pattern, place) = object::pattern_of(ed.doc(), o).unwrap();
    let region = object::region_of(ed.doc(), o, 1.0).unwrap();
    let g = layout::gap_lines(&region, &pattern, &place, layout::MAX_GAP_SEGMENTS, false);
    let written = lines.get("0-GAPS").unwrap();
    assert_eq!(written.len(), g.segments.len());
    let len: f64 = written.iter().map(|s| s[0].distance(s[1])).sum();
    assert!((len - layout::gaps::length(&g.segments)).abs() < 1e-6);
    assert!(!text.contains("-UNITS-"));
    // Default: only the outline.
    let plain = String::from_utf8(
        DxfFormat
            .export(ed.doc(), &ExportOptions::default())
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(!plain.contains("-GAPS") && !plain.contains("-UNITS-"));
    assert_eq!(
        entities(&plain, "LWPOLYLINE")
            .values()
            .map(Vec::len)
            .sum::<usize>(),
        1
    );
}

#[test]
fn a_pattern_over_the_limit_is_an_error_that_names_the_hardscape() {
    let (ed, id) = lot(JointKind::Stack, 200_000.0, 150_000.0, vec![], None, None);
    let err = export(ed.doc(), PatternExport::Units)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&id.to_string()) && err.contains("limit") && err.contains("Pavers"),
        "{err}"
    );
    // Without patterns the same drawing exports fine.
    assert!(export(ed.doc(), PatternExport::None).is_ok());
}

#[test]
fn units_survive_an_import_and_the_unit_option() {
    let (ed, id) = lot(
        JointKind::Herringbone,
        6000.0,
        4000.0,
        vec![island()],
        Some(DVec2::new(100.0, 100.0)),
        None,
    );
    let o = ed.doc().object(id).unwrap();
    let (pattern, place) = object::pattern_of(ed.doc(), o).unwrap();
    let sum = summarize(
        &object::region_of(ed.doc(), o, 1.0).unwrap(),
        &pattern,
        &place,
        &SummaryOptions::default(),
    )
    .unwrap();
    // In meters, the polylines have the area in square meters.
    let bytes = DxfFormat
        .export(
            ed.doc(),
            &ExportOptions {
                patterns: PatternExport::Units,
                unit: Unit::M,
                ..ExportOptions::default()
            },
        )
        .unwrap()
        .bytes;
    let text = String::from_utf8(bytes.clone()).unwrap();
    let laid_m2: f64 = entities(&text, "LWPOLYLINE")
        .iter()
        .filter(|(l, _)| l.ends_with("-UNITS-WHOLE") || l.ends_with("-UNITS-CUT"))
        .flat_map(|(_, v)| v.iter())
        .map(|p| area(p))
        .sum();
    assert!(
        (laid_m2 - sum.area_laid_m2).abs() < 1e-3,
        "{laid_m2} vs {}",
        sum.area_laid_m2
    );
    // Import again: the polylines are objects with the same count and area.
    let r = DxfFormat
        .import(
            &Source {
                name: "paving".into(),
                data: bytes,
                extra: Default::default(),
            },
            &Context::of(&Document::with_sequential_ids()),
            &ImportOptions::default(),
        )
        .unwrap();
    let mut back = Editor::new(Document::with_sequential_ids());
    back.execute(&r.command).unwrap();
    let doc = back.doc();
    let mut counts: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for o in doc.objects.values() {
        if let kadakas_core::model::Shape::Polyline { points, .. } = &o.shape {
            let class = doc
                .class(o.class)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let e = counts.entry(class).or_default();
            e.0 += 1;
            e.1 += curve::area(points);
        }
    }
    let whole = counts.get("0-UNITS-WHOLE").copied().unwrap_or_default();
    let cut = counts.get("0-UNITS-CUT").copied().unwrap_or_default();
    assert_eq!((whole.0, cut.0), (sum.whole_units, sum.cut_units));
    assert!(((whole.1 + cut.1) / 1e6 - sum.area_laid_m2).abs() < 1e-3);
}

fn module_lot(preset: &str, w: f64, h: f64) -> (Editor, ObjectId) {
    use kadakas_core::layout::module;
    let m = match preset {
        "hexagon" => module::hexagon(120.0).unwrap(),
        "octagon_square" => module::octagon_square(100.0).unwrap(),
        _ => module::i_shape(200.0, 165.0, 25.0).unwrap(),
    };
    let mut ed = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Mod".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                joints: Some(JointPattern {
                    kind: JointKind::Module,
                    spacing: 4.0,
                    units: m.units.clone(),
                    a: Some(m.a),
                    b: Some(m.b),
                    ..Default::default()
                }),
                ..Default::default()
            })),
        })
        .unwrap()
        .created[0],
    );
    let id = ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: Some(corners(&[
                DVec2::ZERO,
                DVec2::new(w, 0.0),
                DVec2::new(w, h),
                DVec2::new(0.0, h),
            ])),
            patch: Box::new(HardscapePatch {
                style: Some(style),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    (ed, id)
}

#[test]
fn module_units_are_counted_and_typed_per_unit_type_in_the_dxf() {
    for preset in ["hexagon", "octagon_square", "i_shape"] {
        let (ed, id) = module_lot(preset, 5000.0, 3000.0);
        let o = ed.doc().object(id).unwrap();
        let (pattern, place) = object::pattern_of(ed.doc(), o).unwrap();
        let region = object::region_of(ed.doc(), o, 1.0).unwrap();
        let sum = summarize(&region, &pattern, &place, &SummaryOptions::default()).unwrap();
        let text =
            String::from_utf8(export(ed.doc(), PatternExport::Units).unwrap().bytes).unwrap();
        let polys = entities(&text, "LWPOLYLINE");
        let whole = polys.get("0-UNITS-WHOLE").map_or(0, Vec::len);
        let cut = polys.get("0-UNITS-CUT").map_or(0, Vec::len);
        assert_eq!((whole, cut), (sum.whole_units, sum.cut_units), "{preset}");
        // Each unit polyline has its type as extended data, and the application is in the table.
        let typed = text.matches("1001\nKADAKAS_UNIT\n").count();
        assert_eq!(typed, whole + cut, "{preset}");
        assert!(text.contains("2\nKADAKAS_UNIT\n"));
        for u in &sum.by_unit {
            let n = text.matches(&format!("1000\n{}\n", u.name)).count();
            assert_eq!(n, u.whole_units + u.cut_units, "{preset} {}", u.name);
        }
        let notes = export(ed.doc(), PatternExport::Units).unwrap().report.notes;
        assert!(
            notes
                .iter()
                .any(|n| n.contains("whole") && n.contains("cut")),
            "{notes:?}"
        );
        if sum.by_unit.len() > 1 {
            assert!(
                notes
                    .iter()
                    .any(|n| n.contains("octagon:") && n.contains("square:")),
                "{notes:?}"
            );
        }
        // The gap lines of a module are its outlines, each edge once.
        let g = export(ed.doc(), PatternExport::Gaps).unwrap();
        let lines = entities(&String::from_utf8(g.bytes).unwrap(), "LINE");
        assert!(
            lines.get("0-GAPS").is_some_and(|l| l.len() > 20),
            "{preset}"
        );
    }
    // A bond writes no extended data.
    let (ed, _) = lot(
        JointKind::Stack,
        2050.0,
        1050.0,
        vec![],
        Some(DVec2::ZERO),
        None,
    );
    let text = String::from_utf8(export(ed.doc(), PatternExport::Units).unwrap().bytes).unwrap();
    assert!(!text.contains("KADAKAS_UNIT"));
}

#[test]
fn the_file_budget_is_shared_by_all_objects() {
    let (mut ed, _) = lot(JointKind::Stack, 20_000.0, 10_000.0, vec![], None, None);
    // A second hardscape with the same style.
    let style = ed
        .doc()
        .resources
        .hardscape_styles()
        .next()
        .map(|(id, _, _)| id)
        .unwrap();
    ed.execute(&Command::AddHardscape {
        boundary: Some(corners(&[
            DVec2::new(30_000.0, 0.0),
            DVec2::new(50_000.0, 0.0),
            DVec2::new(50_000.0, 10_000.0),
            DVec2::new(30_000.0, 10_000.0),
        ])),
        patch: Box::new(HardscapePatch {
            style: Some(style),
            ..Default::default()
        }),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    // Each lot has about 9,700 units. A budget of 12,000 takes the first and refuses the second.
    let opts = ExportOptions {
        patterns: PatternExport::Units,
        pattern_budget: Some(12_000),
        ..ExportOptions::default()
    };
    let err = DxfFormat.export(ed.doc(), &opts).unwrap_err().to_string();
    assert!(err.contains("in all objects"), "{err}");
    let opts = ExportOptions {
        pattern_budget: Some(100_000),
        ..opts
    };
    assert!(DxfFormat.export(ed.doc(), &opts).is_ok());
}

#[test]
fn the_outlines_of_a_block_of_a_dxf_file() {
    use kadakas_io::dxf::block::{block_names, block_outlines};
    let poly = |layer: &str, pts: &[(f64, f64)], bulge: f64| {
        let mut s = format!("0\nLWPOLYLINE\n8\n{layer}\n90\n{}\n70\n1\n", pts.len());
        for (i, (x, y)) in pts.iter().enumerate() {
            s.push_str(&format!("10\n{x}\n20\n{y}\n"));
            if i == 0 && bulge != 0.0 {
                s.push_str(&format!("42\n{bulge}\n"));
            }
        }
        s
    };
    let blocks = format!(
        "0\nBLOCK\n8\n0\n2\nTWO\n70\n0\n10\n0\n20\n0\n{}{}0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n0\nENDBLK\n0\nBLOCK\n8\n0\n2\nLINES\n70\n0\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n0\nENDBLK\n",
        poly(
            "BIG",
            &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
            0.0
        ),
        poly(
            "ROUND",
            &[(100.0, 0.0), (150.0, 0.0), (150.0, 100.0), (100.0, 100.0)],
            0.5
        ),
    );
    let text = format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n"
    );
    let names = block_names(text.as_bytes()).unwrap();
    assert_eq!(names, vec!["LINES".to_string(), "TWO".to_string()]);
    let outlines = block_outlines(text.as_bytes(), "TWO").unwrap();
    let mut layers: Vec<&str> = outlines.iter().map(|o| o.layer.as_str()).collect();
    layers.sort();
    assert_eq!(layers, ["BIG", "ROUND"]);
    let round = outlines.iter().find(|o| o.layer == "ROUND").unwrap();
    assert!(
        round.outline.iter().any(|v| !v.is_straight()),
        "the bulge stays an arc"
    );
    let e = block_outlines(text.as_bytes(), "NOPE")
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("no block 'NOPE'") && e.contains("TWO") && e.contains("LINES"),
        "{e}"
    );
    let e = block_outlines(text.as_bytes(), "LINES")
        .unwrap_err()
        .to_string();
    assert!(e.contains("no closed"), "{e}");
    assert!(block_outlines(b"not a dxf", "X").is_err());
}

/// Measures the time and the size of big exports. Run it in a release build under
/// `/usr/bin/time -l` to see the memory; `MEASURE_SIZE=100x50` picks the lot (meters).
#[test]
#[ignore]
fn measure_export() {
    let (w, h): (f64, f64) = std::env::var("MEASURE_SIZE")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((100.0, 50.0));
    for mode in [PatternExport::Units, PatternExport::Gaps] {
        let (ed, _) = lot(
            JointKind::RunningBond,
            w * 1000.0,
            h * 1000.0,
            vec![island()],
            None,
            Some(11.0),
        );
        let t = std::time::Instant::now();
        let out = export(ed.doc(), mode).unwrap();
        println!(
            "{} {w} x {h} m: {} bytes in {} ms",
            mode.name(),
            out.bytes.len(),
            t.elapsed().as_millis(),
        );
    }
}

#[test]
fn a_dxf_file_without_an_entities_section_still_gives_its_block() {
    use kadakas_io::dxf::block::block_outlines;
    let text = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\nB\n70\n0\n10\n0\n20\n0\n0\nLWPOLYLINE\n8\nA\n90\n4\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n10\n0\n20\n10\n0\nENDBLK\n0\nENDSEC\n0\nEOF\n";
    let o = block_outlines(text.as_bytes(), "B").unwrap();
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].layer, "A");
}

#[test]
fn only_a_count_over_the_limit_gets_the_limit_text() {
    // A long thin lot at an angle to the pattern: not too many units, but a scan that is too wide.
    let (mut ed, id) = lot(JointKind::Stack, 100.0, 100.0, vec![], None, None);
    let o = ed.doc().object(id).unwrap().clone();
    let style = hardscape::data::style_id(&o).unwrap();
    ed.execute(&Command::AddHardscape {
        boundary: Some(corners(&[
            DVec2::new(0.0, 0.0),
            DVec2::new(2_000_000.0, 2_000_000.0),
            DVec2::new(2_000_000.0, 2_000_100.0),
            DVec2::new(0.0, 100.0),
        ])),
        patch: Box::new(HardscapePatch {
            style: Some(style),
            ..Default::default()
        }),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let err = export(ed.doc(), PatternExport::Units)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Turn the pattern"), "{err}");
    assert!(!err.contains("limit of a DXF export"), "{err}");
}
