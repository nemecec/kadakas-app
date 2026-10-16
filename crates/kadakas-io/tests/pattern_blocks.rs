//! Whole units as INSERTs of one block per unit type (PAT-11), and zones and courses in the DXF
//! export (PAT-07, PAT-08): the counts of the INSERTs plus the polylines are the counts of the
//! report.

use std::collections::BTreeMap;

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::hardscape::HardscapePatch;
use kadakas_core::hardscape::style::{HardscapeStyle, JointKind, JointPattern};
use kadakas_core::layout::object::{pattern_of, region_of};
use kadakas_core::layout::plan::{self, groups_of, merge};
use kadakas_core::layout::summary::SummaryOptions;
use kadakas_core::model::{Attributes, Document, ObjectId};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{ExportOptions, Exporter, PatternExport};

fn lot(kind: JointKind, w: f64, h: f64, angle: f64) -> (Editor, ObjectId) {
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
                origin: Some(DVec2::ZERO),
                angle: Some(angle),
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

fn export(doc: &Document, patterns: PatternExport) -> String {
    String::from_utf8(
        DxfFormat
            .export(
                doc,
                &ExportOptions {
                    patterns,
                    ..ExportOptions::default()
                },
            )
            .unwrap()
            .bytes,
    )
    .unwrap()
}

/// The code and the value of every pair of the file.
fn pairs(dxf: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = dxf.lines().collect();
    lines
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].trim().parse().unwrap_or(-1), c[1].trim().to_string()))
        .collect()
}

#[derive(Debug)]
struct Insert {
    layer: String,
    block: String,
    at: DVec2,
    angle: f64,
}

/// The INSERTs, the polygon of each block, and the polylines by layer.
type Read = (
    Vec<Insert>,
    BTreeMap<String, Vec<DVec2>>,
    BTreeMap<String, Vec<Vec<DVec2>>>,
);

/// The INSERTs of the ENTITIES section, and the polygon of each block.
fn read(dxf: &str) -> Read {
    let p = pairs(dxf);
    let mut inserts = Vec::new();
    let mut blocks: BTreeMap<String, Vec<DVec2>> = BTreeMap::new();
    let mut polys: BTreeMap<String, Vec<Vec<DVec2>>> = BTreeMap::new();
    let mut section = String::new();
    let mut i = 0;
    while i < p.len() {
        if p[i].0 == 0 && p[i].1 == "SECTION" {
            section = p[i + 1].1.clone();
        }
        if p[i].0 == 0
            && (p[i].1 == "INSERT" || p[i].1 == "LWPOLYLINE")
            && (section == "ENTITIES" || section == "BLOCKS")
        {
            let kind = p[i].1.clone();
            let (mut layer, mut name, mut x, mut y, mut rot) =
                (String::new(), String::new(), 0.0, 0.0, 0.0);
            let mut pts: Vec<DVec2> = Vec::new();
            let mut px = 0.0;
            let mut j = i + 1;
            while j < p.len() && p[j].0 != 0 {
                match p[j].0 {
                    8 => layer = p[j].1.clone(),
                    2 => name = p[j].1.clone(),
                    10 if kind == "INSERT" => x = p[j].1.parse().unwrap(),
                    20 if kind == "INSERT" => y = p[j].1.parse().unwrap(),
                    10 => px = p[j].1.parse().unwrap(),
                    20 => pts.push(DVec2::new(px, p[j].1.parse().unwrap())),
                    50 => rot = p[j].1.parse().unwrap(),
                    _ => {}
                }
                j += 1;
            }
            if kind == "INSERT" {
                inserts.push(Insert {
                    layer,
                    block: name,
                    at: DVec2::new(x, y),
                    angle: rot,
                });
            } else if section == "ENTITIES" {
                polys.entry(layer).or_default().push(pts);
            }
            i = j;
            continue;
        }
        // The polygon of a block: the polyline between BLOCK and ENDBLK.
        if section == "BLOCKS"
            && p[i].0 == 2
            && i > 0
            && p[i - 1].0 == 100
            && p[i - 1].1 == "AcDbBlockBegin"
        {
            let name = p[i].1.clone();
            let mut j = i;
            let mut pts: Vec<DVec2> = Vec::new();
            let mut px = 0.0;
            let mut in_poly = false;
            while j < p.len() && !(p[j].0 == 0 && p[j].1 == "ENDBLK") {
                if p[j].0 == 0 {
                    in_poly = p[j].1 == "LWPOLYLINE";
                }
                if in_poly {
                    if p[j].0 == 10 {
                        px = p[j].1.parse().unwrap();
                    }
                    if p[j].0 == 20 {
                        pts.push(DVec2::new(px, p[j].1.parse().unwrap()));
                    }
                }
                j += 1;
            }
            if !pts.is_empty() {
                blocks.insert(name, pts);
            }
        }
        i += 1;
    }
    (inserts, blocks, polys)
}

fn area(p: &[DVec2]) -> f64 {
    kadakas_core::geom::polygon_area(p)
}

#[test]
fn whole_units_are_inserts_of_one_block_and_the_counts_are_the_report() {
    for kind in JointKind::ALL {
        for angle in [0.0, 17.0] {
            let (ed, id) = lot(kind, 6000.0, 4000.0, angle);
            let doc = ed.doc();
            let o = doc.object(id).unwrap();
            let planned = groups_of(doc, o, 1.0, 1_000_000).unwrap().unwrap();
            let parts: Vec<_> = planned
                .groups
                .iter()
                .map(|g| g.summarize(&SummaryOptions::default()).unwrap())
                .collect();
            let sum = merge(
                &parts,
                planned.groups[0].region.area,
                &SummaryOptions::default(),
            );
            let text = export(doc, PatternExport::Blocks);
            let (inserts, blocks, polys) = read(&text);
            let cut = polys.get("0-UNITS-CUT").map_or(0, Vec::len);
            let insert_count = inserts
                .iter()
                .filter(|i| i.layer == "0-UNITS-WHOLE")
                .count();
            assert_eq!(
                (insert_count, cut),
                (sum.whole_units, sum.cut_units),
                "{kind:?} at {angle}"
            );
            assert!(
                !polys.contains_key("0-UNITS-WHOLE"),
                "no whole unit is a polyline"
            );
            assert!(inserts.iter().all(|i| i.block == "KADAKAS_200X100"));
            // One block for the unit type (a herringbone has units turned by 90 degrees, a basket
            // weave too, and they are the same shape).
            let names: Vec<&String> = blocks
                .keys()
                .filter(|n| n.starts_with("KADAKAS_"))
                .collect();
            assert_eq!(names.len(), 1, "{kind:?}: {names:?}");
            assert_eq!(names[0], "KADAKAS_200X100");
            // The block has the size of the unit, and the INSERTs rebuild the units exactly.
            let shape = &blocks["KADAKAS_200X100"];
            assert!((area(shape) - 20_000.0).abs() < 1e-6);
            let (pattern, place) = pattern_of(doc, o).unwrap();
            let region = region_of(doc, o, 1.0).unwrap();
            let (units, _) =
                kadakas_core::layout::lay_units(&region, &pattern, &place, &Default::default())
                    .unwrap();
            let mut want: BTreeMap<(i64, i64), Vec<DVec2>> = BTreeMap::new();
            for u in units.iter().filter(|u| u.whole) {
                let c = u.rings[0].points.iter().copied().sum::<DVec2>() / 4.0;
                want.insert(
                    ((c.x * 100.0).round() as i64, (c.y * 100.0).round() as i64),
                    u.rings[0].points.clone(),
                );
            }
            for ins in inserts.iter().filter(|i| i.layer == "0-UNITS-WHOLE") {
                let key = (
                    (ins.at.x * 100.0).round() as i64,
                    (ins.at.y * 100.0).round() as i64,
                );
                let corners_want = want
                    .get(&key)
                    .unwrap_or_else(|| panic!("no unit at {:?}", ins.at));
                let turn = DVec2::from_angle(ins.angle.to_radians());
                let mut got: Vec<DVec2> = shape.iter().map(|p| ins.at + turn.rotate(*p)).collect();
                // The same corners, whatever the start.
                let mut ok = false;
                for _ in 0..4 {
                    got.rotate_left(1);
                    if got
                        .iter()
                        .zip(corners_want)
                        .all(|(a, b)| a.distance(*b) < 1e-3)
                    {
                        ok = true;
                    }
                }
                let mut rev = got.clone();
                rev.reverse();
                for _ in 0..4 {
                    rev.rotate_left(1);
                    if rev
                        .iter()
                        .zip(corners_want)
                        .all(|(a, b)| a.distance(*b) < 1e-3)
                    {
                        ok = true;
                    }
                }
                assert!(
                    ok,
                    "{kind:?}: the insert at {:?} does not rebuild its unit",
                    ins.at
                );
            }
        }
    }
}

#[test]
fn blocks_make_the_file_smaller_and_polylines_are_as_before() {
    let (ed, _) = lot(JointKind::RunningBond, 12_000.0, 8_000.0, 17.0);
    let units = export(ed.doc(), PatternExport::Units);
    let blocks = export(ed.doc(), PatternExport::Blocks);
    // About 3,500 whole units. On a turned pattern the corners are long numbers, and an INSERT
    // holds one point and an angle instead of four points.
    assert!(
        blocks.len() * 3 < units.len() * 2,
        "{} vs {}",
        blocks.len(),
        units.len()
    );
    // The cut units are the same polylines in both.
    let (_, _, pu) = read(&units);
    let (_, _, pb) = read(&blocks);
    assert_eq!(
        pu.get("0-UNITS-CUT").map(Vec::len),
        pb.get("0-UNITS-CUT").map(Vec::len)
    );
    assert_eq!(pu["0-UNITS-WHOLE"].len(), read(&blocks).0.len());
}

#[test]
fn zones_and_courses_are_exported_by_group() {
    let (mut ed, id) = lot(JointKind::RunningBond, 8000.0, 4000.0, 0.0);
    let plan = serde_json::json!({
        "splits": [{"points": [[3000.0, -10.0], [3000.0, 4010.0]]}],
        "zones": [{"name": "Stalls", "seed": [1000.0, 1000.0], "angle": 90.0}],
        "courses": [{"name": "Kerb", "path": [[0.0, 0.0], [8000.0, 0.0]], "side": "left",
                     "along": 100.0, "depth": 200.0, "gap": 5.0, "unit": "200x100"}],
    });
    ed.execute(&Command::SetData {
        id,
        set: [(plan::KEY.to_string(), plan)].into_iter().collect(),
        remove: vec![],
    })
    .unwrap();
    let doc = ed.doc();
    let o = doc.object(id).unwrap();
    let planned = groups_of(doc, o, 1.0, 1_000_000).unwrap().unwrap();
    assert_eq!(planned.groups.len(), 3, "{:?}", planned.notes);
    let opts = SummaryOptions::default();
    let parts: Vec<_> = planned
        .groups
        .iter()
        .map(|g| g.summarize(&opts).unwrap())
        .collect();
    let region = region_of(doc, o, 1.0).unwrap();
    let sum = merge(&parts, region.area, &opts);
    assert_eq!(
        sum.by_unit.len(),
        1,
        "the course and the field use one stone: {:?}",
        sum.by_unit
    );
    for mode in [PatternExport::Units, PatternExport::Blocks] {
        let text = export(doc, mode);
        let (inserts, _, polys) = read(&text);
        let mut whole = 0;
        let mut cut = 0;
        for g in &planned.groups {
            let base = if g.slug == "FIELD" {
                "0".to_string()
            } else {
                format!("0-{}", g.slug)
            };
            let w = if mode == PatternExport::Blocks {
                inserts
                    .iter()
                    .filter(|i| i.layer == format!("{base}-UNITS-WHOLE"))
                    .count()
            } else {
                polys
                    .get(&format!("{base}-UNITS-WHOLE"))
                    .map_or(0, Vec::len)
            };
            let c = polys.get(&format!("{base}-UNITS-CUT")).map_or(0, Vec::len);
            let part = g.summarize(&opts).unwrap();
            assert_eq!(
                (w, c),
                (part.whole_units, part.cut_units),
                "{mode:?} {}",
                g.name
            );
            whole += w;
            cut += c;
        }
        assert_eq!((whole, cut), (sum.whole_units, sum.cut_units), "{mode:?}");
        assert!(
            text.contains("0-ZONE-STALLS-UNITS-")
                || text.contains("ZONE-1-UNITS-")
                || text.contains("-ZONE-")
        );
        assert!(text.contains("0-COURSE-KERB-UNITS-WHOLE"));
    }
}

/// Size and time of the export of a lot of 5,000 m2 (about 233,000 units): polylines against
/// blocks. Run in release mode:
/// `cargo test -p kadakas-io --release --test pattern_blocks measure_blocks -- --ignored --nocapture`
#[test]
#[ignore = "timing test: run in release mode"]
fn measure_blocks() {
    for angle in [0.0, 17.0] {
        let (ed, _) = lot(JointKind::RunningBond, 100_000.0, 50_000.0, angle);
        let mut sizes = Vec::new();
        for mode in [PatternExport::Units, PatternExport::Blocks] {
            let t = std::time::Instant::now();
            let text = export(ed.doc(), mode);
            let secs = t.elapsed().as_secs_f64();
            let (inserts, _, polys) = read(&text);
            let n = inserts.len() + polys.values().map(Vec::len).sum::<usize>();
            println!(
                "angle {angle}: {mode:?}: {:.1} MB, {:.2} s, {} inserts + polylines (with the outline)",
                text.len() as f64 / 1e6,
                secs,
                n
            );
            sizes.push(text.len());
        }
        println!(
            "angle {angle}: blocks are {:.2} times smaller",
            sizes[0] as f64 / sizes[1] as f64
        );
    }
}

/// The units that the drawing shows are the units that the DXF export writes, for a plan with a
/// zone of its own pattern and a course: INSERTs and cut polylines on the unit layers of all groups.
#[test]
fn the_drawing_has_the_units_that_the_dxf_export_writes() {
    use kadakas_core::curve::Vertex;
    use kadakas_core::hardscape::draw::{DrawOptions, prepared};
    use kadakas_core::layout::course::Side;
    use kadakas_core::layout::plan::{CourseDef, Plan, SplitLine, ZoneSpec};
    use kadakas_core::layout::{Bond, Pattern};
    let (mut ed, id) = lot(JointKind::RunningBond, 8000.0, 4000.0, 0.0);
    let vx = |x: f64, y: f64| Vertex::corner(DVec2::new(x, y));
    let plan = Plan {
        splits: vec![SplitLine {
            source: None,
            points: vec![vx(5000.0, -50.0), vx(5000.0, 4050.0)],
        }],
        zones: vec![ZoneSpec {
            name: Some("Stalls".into()),
            seed: DVec2::new(7000.0, 2000.0),
            pattern: Some(Pattern::new(Bond::Stack, 300.0, 150.0, 4.0)),
            origin: Some(DVec2::new(5000.0, 0.0)),
            angle: Some(90.0),
            ..Default::default()
        }],
        courses: vec![CourseDef {
            name: "Kerb".into(),
            path: vec![vx(0.0, 100.0), vx(5000.0, 100.0)],
            side: Side::Left,
            along: 100.0,
            depth: 200.0,
            gap: 5.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    ed.execute(&Command::SetData {
        id,
        set: [(plan::KEY.to_string(), serde_json::to_value(&plan).unwrap())]
            .into_iter()
            .collect(),
        remove: vec![],
    })
    .unwrap();
    let doc = ed.doc();
    let drawn = prepared(doc, doc.object(id).unwrap())
        .unwrap()
        .unwrap()
        .draw(None, &DrawOptions::units(usize::MAX / 8));
    assert!(!drawn.is_preview() && drawn.groups.len() >= 3);
    let (whole, cut) = drawn.unit_counts();
    let text = export(doc, PatternExport::Blocks);
    let (inserts, _, polys) = read(&text);
    let exported_whole = inserts
        .iter()
        .filter(|i| i.layer.ends_with("-UNITS-WHOLE"))
        .count();
    let exported_cut: usize = polys
        .iter()
        .filter(|(l, _)| l.ends_with("-UNITS-CUT"))
        .map(|(_, v)| v.len())
        .sum();
    assert!(whole > 500 && cut > 20, "{whole} {cut}");
    assert_eq!((exported_whole, exported_cut), (whole, cut));
}
