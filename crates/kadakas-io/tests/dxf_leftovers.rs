//! The leftovers of IOP-15 (IOP-27) on public sample files: pattern hatches, block clips, the
//! twist and perspective of viewports, and text and blocks in a tilted plane. The facts come from
//! the ezdxf reader (`fixtures/dxf_iop27.json`, made by `tests/data/make_iop27_facts.py`).
//! The files are fetched at run time (see `support::corpus`) and the tests skip when they are not there.

mod support;

use std::collections::BTreeSet;

use kadakas_core::graphics::GraphicResource;
use kadakas_core::resource::ResourceBody;
use kadakas_io::{ImportOptions, OriginMode, Report, Unit};
use support::corpus;

fn facts() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/dxf_iop27.json")).unwrap()
}

fn fetch(key: &str) -> Option<Vec<u8>> {
    let (repo, path) = key.split_once(':').unwrap();
    match repo {
        "ezdxf" => corpus::need(corpus::EZDXF, path),
        "gdal" => corpus::need(corpus::GDAL, path),
        other => panic!("{other}"),
    }
}

/// The numbers of a drawing in its own units, like the ezdxf reader has them.
fn raw_units() -> ImportOptions {
    ImportOptions {
        origin: OriginMode::File,
        unit: Some(Unit::Mm),
        ..ImportOptions::default()
    }
}

fn accounted(rep: &Report) {
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
}

fn line_key(angle: f64, spacing: f64, stagger: f64) -> String {
    let r = |x: f64| (x * 1.0e5).round() / 1.0e5 + 0.0;
    format!(
        "{:.4} {:.5} {:.5}",
        r(angle.rem_euclid(360.0) % 360.0),
        r(spacing),
        r(stagger)
    )
}

/// (angle, spacing, stagger) of a DXF pattern line, from the rule of the DXF reference: the offset
/// is the vector from a line to the next one. A negative part across counts the lines the other way.
fn expected_line(l: &serde_json::Value) -> String {
    let n = |i: usize| l[i].as_f64().unwrap();
    let a = n(0).to_radians();
    let (u, v) = ((a.cos(), a.sin()), (n(3), n(4)));
    let perp = -v.0 * u.1 + v.1 * u.0;
    let along = v.0 * u.0 + v.1 * u.1;
    line_key(n(0), perp.abs(), along * perp.signum())
}

#[test]
fn pattern_hatches_of_public_files_have_the_lines_of_the_ezdxf_reader() {
    let facts = facts();
    for (key, file) in facts["patterns"].as_object().unwrap() {
        let Some(data) = fetch(key) else { continue };
        let (ed, rep) = support::import_with(&data, &raw_units()).unwrap();
        accounted(&rep);
        let hatches = file["hatches"].as_array().unwrap();
        assert!(!hatches.is_empty(), "{key}");
        let expected: BTreeSet<String> = hatches
            .iter()
            .flat_map(|h| h["lines"].as_array().unwrap())
            .map(expected_line)
            .collect();
        let mut actual = BTreeSet::new();
        for r in ed.doc().resources.iter() {
            if let ResourceBody::Graphic(g) = &r.body
                && let GraphicResource::Hatch(h) = &**g
            {
                for l in &h.lines {
                    actual.insert(line_key(l.angle, l.spacing, l.offset));
                }
            }
        }
        // Every line of the ezdxf reader is a line of a hatch fill. The import has more when the
        // file has block references that hold hatches.
        let missing: Vec<_> = expected.difference(&actual).collect();
        assert!(missing.is_empty(), "{key}: {missing:?}");
        if file["inserts"] == 0 {
            assert_eq!(actual, expected, "{key}");
        }
        // Every pattern hatch became a hatch fill: the note counts them.
        let total: usize = rep
            .notes
            .iter()
            .find(|n| n.contains("with a pattern became hatch fills of the document"))
            .and_then(|n| n.split_whitespace().next()?.parse().ok())
            .unwrap_or(0);
        assert!(total >= hatches.len(), "{key}: {:?}", rep.notes);
        // The boundary objects have the fill: one per boundary path, or an island.
        let filled = ed
            .doc()
            .objects
            .values()
            .filter(|o| o.attrs.fill_pattern.is_some())
            .count();
        assert!(filled >= hatches.len(), "{key}: {filled}");
    }
}

/// The length of what a drawing shows as lines: lines, polylines, circles and arcs.
fn drawn_length(ed: &kadakas_core::command::Editor) -> f64 {
    use kadakas_core::model::Shape;
    ed.doc()
        .objects
        .values()
        .map(|o| match &o.shape {
            Shape::Line { a, b } => a.distance(*b),
            Shape::Polyline { points, closed } => {
                let pts = kadakas_core::curve::flatten(points, *closed, 0.001);
                let n = pts.len();
                let segs = if *closed { n } else { n.saturating_sub(1) };
                (0..segs).map(|i| pts[i].distance(pts[(i + 1) % n])).sum()
            }
            Shape::Circle { radius, .. } => std::f64::consts::TAU * radius,
            Shape::Arc {
                radius, start, end, ..
            } => radius * kadakas_core::model::arc_sweep(*start, *end),
            _ => 0.0,
        })
        .sum()
}

#[test]
fn block_clips_of_the_ezdxf_example_cut_the_same_length_as_shapely() {
    let facts = facts();
    let Some(data) = corpus::need(corpus::EZDXF, "examples_dxf/xclip.dxf") else {
        return;
    };
    let opts = ImportOptions {
        tolerance: 0.001,
        ..raw_units()
    };
    let (ed, rep) = support::import_with(&data, &opts).unwrap();
    accounted(&rep);
    let expected = facts["xclip"]["total_length"].as_f64().unwrap();
    let got = drawn_length(&ed);
    assert!(
        (got - expected).abs() / expected < 0.01,
        "{got} against {expected}"
    );
    // The notes count the references: 8 clipped (2 of them inverted), 1 switched off.
    let all = rep.notes.join("\n");
    assert!(
        all.contains("8 block references have a clip (XCLIP) that was applied"),
        "{all}"
    );
    assert!(all.contains("2 clips are inverted"), "{all}");
    assert!(
        all.contains("1 block reference has a clip (XCLIP) that is switched off"),
        "{all}"
    );
}
