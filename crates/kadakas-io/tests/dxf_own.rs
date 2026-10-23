//! The small DXF files that we wrote ourselves (`tests/data/dxf-own`, made by
//! `tests/data/make_own_files.py`). They are in the repository, so these tests run offline in CI.

mod support;

use std::path::PathBuf;

use kadakas_core::model::{LayerKind, Shape};
use support::{import, import_with};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/dxf-own")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn every_own_file_is_imported_and_accounted_for() {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".dxf"))
        .collect();
    names.sort();
    assert!(names.len() >= 10, "{names:?}");
    for name in names {
        let (ed, rep) = import(&read(&name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            rep.entities_read,
            rep.entities_imported + rep.entities_skipped,
            "{name}: {rep:?}"
        );
        assert!(rep.total() > 0, "{name}: nothing imported");
        assert!(ed.doc().objects.values().all(|o| o.is_finite()), "{name}");
        eprintln!(
            "{name}: {:?} skipped {:?} notes {:?}",
            rep.objects, rep.skipped, rep.notes
        );
    }
}

fn texts(ed: &kadakas_core::command::Editor) -> Vec<String> {
    ed.doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn longest_line(ed: &kadakas_core::command::Editor) -> f64 {
    ed.doc()
        .objects
        .values()
        .map(|o| {
            let (a, b) = o.shape.bounds();
            (b - a).max_element()
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_release_12_file_without_unit_is_guessed_and_the_option_changes_it() {
    for name in ["r12_unitless.dxf", "r12_binary.dxf"] {
        let (ed, rep) = import(&read(name)).unwrap();
        assert!(
            rep.notes.iter().any(|n| n.contains("assuming millimeters")),
            "{name}"
        );
        assert_eq!(
            rep.release.as_deref(),
            Some("AC1009 (release 12)"),
            "{name}"
        );
        let _ = ed;
    }
    let (ed, _) = import(&read("r12_unitless.dxf")).unwrap();
    assert!((longest_line(&ed) - 10_000.0).abs() < 1e-6);
    let (ed, rep) = import_with(
        &read("r12_unitless.dxf"),
        &kadakas_io::ImportOptions {
            unit: Some(kadakas_io::Unit::M),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!rep.notes.iter().any(|n| n.contains("no unit")));
    assert!((longest_line(&ed) - 10_000_000.0).abs() < 1e-3);
    // The polyline keeps its bulge as an arc.
    assert!(
        ed.doc()
            .objects
            .values()
            .any(|o| matches!(&o.shape, Shape::Polyline { points, .. }
        if points.iter().any(|p| matches!(p.kind, kadakas_core::curve::VertexKind::PointOnArc))))
    );
}

#[test]
fn binary_files_have_the_same_objects_as_text() {
    let (_, rep) = import(&read("r2000_binary.dxf")).unwrap();
    assert_eq!(rep.objects.get("line"), Some(&2));
    assert_eq!(rep.release.as_deref(), Some("AC1015 (release 2000)"));
    let (ed, _) = import(&read("r12_binary.dxf")).unwrap();
    assert_eq!(texts(&ed), vec!["Binary"]);
}

#[test]
fn old_files_keep_their_text_in_the_code_page_and_new_files_in_utf8() {
    let (ed, rep) = import(&read("r2000_cp1251.dxf")).unwrap();
    assert_eq!(
        texts(&ed),
        vec!["\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}"]
    );
    assert!(
        rep.classes_created
            .contains(&"\u{421}\u{442}\u{435}\u{43d}\u{44b}".to_string()),
        "{:?}",
        rep.classes_created
    );
    let (ed, _) = import(&read("r2000_cp1257.dxf")).unwrap();
    assert_eq!(texts(&ed), vec!["\u{d5}unapuu \u{160}okolaad"]);
    let (ed, rep) = import(&read("r2018_unicode.dxf")).unwrap();
    let t = texts(&ed);
    assert!(
        t.contains(&"\u{d5}unapuu \u{d8}300 \u{b1} 5\u{b0}".to_string()),
        "{t:?}"
    );
    assert!(
        t.contains(&"\u{dc}lemine\nalumine 1/2".to_string()),
        "{t:?}"
    );
    assert!(t.contains(&"\u{4e2d}\u{6587}".to_string()));
    assert!(
        rep.classes_created
            .contains(&"P\u{f5}\u{f5}sad ja puud".to_string()),
        "{:?}",
        rep.classes_created
    );
}

#[test]
fn the_unit_of_the_file_sets_the_size() {
    let (ed, rep) = import(&read("r2018_meters.dxf")).unwrap();
    assert!((longest_line(&ed) - 30_000.0).abs() < 1e-6);
    assert!(rep.notes.is_empty(), "{:?}", rep.notes);
    let (ed, _) = import(&read("r2018_feet.dxf")).unwrap();
    assert!((longest_line(&ed) - 30_480.0).abs() < 1e-6);
    // A metre file read as feet is 3.28 times too big: the option is how to say it.
    let (ed, rep) = import_with(
        &read("r2018_meters.dxf"),
        &kadakas_io::ImportOptions {
            unit: Some(kadakas_io::Unit::Foot),
            ..Default::default()
        },
    )
    .unwrap();
    assert!((longest_line(&ed) - 30.0 * 304.8).abs() < 1e-6);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("replaces the unit of the file"))
    );
}

#[test]
fn layouts_become_sheets_with_viewports_title_block_and_attributes() {
    let (ed, rep) = import(&read("r2018_layouts.dxf")).unwrap();
    let doc = ed.doc();
    let sheets: Vec<_> = doc
        .layers
        .iter()
        .filter(|l| l.kind == LayerKind::Sheet)
        .collect();
    assert_eq!(sheets.len(), 2, "{:?}", rep.notes);
    let a = sheets.iter().find(|l| l.name == "A101").unwrap();
    let b = sheets.iter().find(|l| l.name == "A102").unwrap();
    let sa = doc.sheet(a.id).unwrap();
    assert_eq!(
        (sa.paper, sa.orientation),
        (
            kadakas_core::sheets::PaperSize::A3,
            kadakas_core::sheets::Orientation::Landscape
        )
    );
    let sb = doc.sheet(b.id).unwrap();
    assert_eq!(
        (sb.paper, sb.orientation),
        (
            kadakas_core::sheets::PaperSize::A3,
            kadakas_core::sheets::Orientation::Portrait
        )
    );
    let (_, v) = doc.viewports_of(a.id).next().unwrap();
    // A frame of 280 x 200 mm that shows 20 m of height: 1:100. The view center is (15, 10) m.
    assert!((v.scale - 100.0).abs() < 1e-6, "{}", v.scale);
    assert_eq!((v.max - v.min).to_array(), [280.0, 200.0]);
    assert!(
        (v.center - kadakas_core::DVec2::new(15_000.0, 10_000.0)).length() < 1e-6,
        "{:?}",
        v.center
    );
    let old = doc.classes.iter().find(|c| c.name == "Old").unwrap();
    assert_eq!(v.class_overrides[&old.id].visible, Some(false));
    let on_a: Vec<String> = doc
        .objects
        .values()
        .filter(|o| o.layer == a.id)
        .filter_map(|o| match &o.shape {
            Shape::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        on_a.contains(&"Project".to_string())
            && on_a.contains(&"A101".to_string())
            && on_a.contains(&"Plan 1:100".to_string()),
        "{on_a:?}"
    );
    // The title block lies at (320, 10): its rectangle starts there.
    let rect = doc
        .objects
        .values()
        .filter(|o| o.layer == a.id)
        .find_map(|o| match &o.shape {
            Shape::Polyline { points, .. }
                if points
                    .iter()
                    .any(|p| p.p == kadakas_core::DVec2::new(320.0, 10.0)) =>
            {
                Some(())
            }
            _ => None,
        });
    assert!(rect.is_some());
    // The model has its three objects on a design layer, apart from the sheets.
    assert_eq!(
        doc.objects
            .values()
            .filter(|o| doc.layer(o.layer).unwrap().kind == LayerKind::Design)
            .count(),
        3
    );
}

#[test]
fn the_file_with_many_entities_has_a_line_for_what_is_not_imported() {
    let (ed, rep) = import(&read("r2018_entities.dxf")).unwrap();
    assert_eq!(rep.objects.get("mesh3d"), Some(&1));
    assert_eq!(
        rep.objects.get("polyline3d"),
        Some(&1),
        "the circle in the tilted plane"
    );
    assert!(
        rep.skipped
            .iter()
            .any(|s| s.entity == "IMAGE" && s.reason.contains("photos/site.png"))
    );
    assert!(
        rep.skipped
            .iter()
            .any(|s| s.entity == "POINT" && s.count == 3)
    );
    // The wipeout is above the hatch in the same layer.
    let objs: Vec<_> = ed.doc().objects.values().collect();
    let hatch = objs
        .iter()
        .position(|o| o.attrs.fill == Some(kadakas_core::model::Color([255, 0, 0, 255])))
        .unwrap();
    let wipe = objs
        .iter()
        .position(|o| o.attrs.pen == Some(kadakas_core::model::Color::NONE))
        .unwrap();
    assert!(hatch < wipe);
}
