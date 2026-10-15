//! Tests of the comparison of two files.

use super::*;
use kadakas_core::model::Shape;

fn file(guid: &str, ents: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$FINGERPRINTGUID\n2\n{guid}\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{ents}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(handle: u32, layer: &str, y: i64) -> String {
    format!("0\nLINE\n5\n{handle:X}\n8\n{layer}\n10\n0\n20\n{y}\n11\n1000\n21\n{y}\n")
}

fn circle(handle: u32, x: i64) -> String {
    format!("0\nCIRCLE\n5\n{handle:X}\n8\nA\n10\n{x}\n20\n2000\n40\n300\n")
}

fn run(base: &str, new: &str, layers: &[String]) -> Result<Compared, UpdateError> {
    compare(&Request {
        base: (base.as_bytes(), "base.dxf"),
        new: (new.as_bytes(), "new.dxf"),
        options: &ImportOptions::default(),
        tolerance: None,
        layers,
    })
}

fn base() -> String {
    file(
        "A1B2",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 100),
            line(0x12, "A", 200),
            circle(0x13, 500),
        ]
        .concat(),
    )
}

fn revised() -> String {
    // 0x11 moved, 0x12 removed, the circle is on another layer's place, a line is added.
    file(
        "A1B2",
        &[
            line(0x10, "A", 0),
            line(0x11, "A", 160),
            circle(0x13, 500),
            line(0x14, "B", 900),
        ]
        .concat(),
    )
}

fn count(c: &Compared, k: Kind) -> usize {
    c.changes.iter().filter(|x| x.status == k).count()
}

#[test]
fn two_files_compare_without_an_import() {
    let c = run(&base(), &revised(), &[]).unwrap();
    assert_eq!(c.counts.unchanged, 2);
    assert_eq!(
        (c.counts.changed, c.counts.added, c.counts.removed),
        (1, 1, 1)
    );
    assert_eq!(c.items.len(), 3);
    assert_eq!(c.summary().unchanged, 2);
    // The new document holds the new file only. The all document holds both.
    assert_eq!(c.new_doc.objects.len(), 4);
    assert_eq!(c.all_doc.objects.len(), 8);
    // The statuses point at objects of the right document.
    for ch in &c.changes {
        match ch.status {
            Kind::Added | Kind::Changed => assert!(c.new_doc.objects.contains_key(&ch.id)),
            Kind::Removed => {
                assert!(!c.new_doc.objects.contains_key(&ch.id));
                assert!(c.all_doc.objects.contains_key(&ch.id));
            }
        }
    }
    assert_eq!(count(&c, Kind::Added), 1);
    assert_eq!(count(&c, Kind::Removed), 1);
    let moved = c
        .changes
        .iter()
        .find(|x| x.status == Kind::Changed)
        .unwrap();
    // The old object of a changed one is the old line, at the old place.
    let old = c.all_doc.objects.get(&moved.old.unwrap()).unwrap();
    let new = c.all_doc.objects.get(&moved.id).unwrap();
    match (&old.shape, &new.shape) {
        (Shape::Line { a: oa, .. }, Shape::Line { a: na, .. }) => {
            assert!((oa.y - 100.0).abs() < 1e-9);
            assert!((na.y - 160.0).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    // The removed object sits where it was in the old file.
    let gone = c
        .changes
        .iter()
        .find(|x| x.status == Kind::Removed)
        .unwrap();
    match &c.all_doc.objects.get(&gone.id).unwrap().shape {
        Shape::Line { a, .. } => assert!((a.y - 200.0).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_same_file_has_no_changes_and_a_layer_limits_the_list() {
    let same = run(&base(), &base(), &[]).unwrap();
    assert!(same.items.is_empty());
    assert!(same.changes.is_empty());
    assert_eq!(same.counts.unchanged, 4);
    let only_b = run(&base(), &revised(), &["b".to_string()]).unwrap();
    assert_eq!(only_b.items.len(), 1);
    assert_eq!(only_b.items[0].status, Kind::Added);
}

#[test]
fn files_with_other_handles_and_other_names_still_match() {
    let renumbered = file(
        "ZZZZ9",
        &[
            line(0x11, "A", 0),
            line(0x12, "A", 100),
            line(0x13, "A", 200),
            circle(0x10, 500),
        ]
        .concat(),
    );
    let c = run(&base(), &renumbered, &[]).unwrap();
    assert!(c.renumbered || c.counts.changed + c.counts.added + c.counts.removed == 0);
    assert_eq!(c.counts.unchanged, 4, "{:?}", c.items);
}

#[test]
fn an_unreadable_file_is_told_and_the_limits_hold() {
    let err = run("not a dxf", &base(), &[]).err().unwrap();
    assert!(matches!(err, UpdateError::OldFile(_)), "{err}");
    let err = run(&base(), "not a dxf", &[]).err().unwrap();
    assert!(matches!(err, UpdateError::NewFile(_)), "{err}");
    let err = compare(&Request {
        tolerance: Some(f64::NAN),
        base: (base().as_bytes(), "a.dxf"),
        new: (base().as_bytes(), "b.dxf"),
        options: &ImportOptions::default(),
        layers: &[],
    })
    .err()
    .unwrap();
    assert!(err.to_string().contains("tolerance"));
}

#[test]
fn a_far_drawing_is_placed_the_same_for_both_files() {
    // The survey sits 5 km from the origin: the new file is centered, and the old file gets the
    // same shift, so the unchanged lines are at the same place in the two documents.
    let far = |y: i64, extra: &str| {
        file(
            "A1B2",
            &format!("0\nLINE\n5\n10\n8\nA\n10\n5000000\n20\n{y}\n11\n5001000\n21\n{y}\n{extra}"),
        )
    };
    let c = run(
        &far(7000000, ""),
        &far(7000000, &line(0x20, "A", 7000500)),
        &[],
    )
    .unwrap();
    assert_eq!(c.counts.unchanged, 1);
    assert_eq!(c.counts.added, 1);
    let ys: Vec<f64> = c
        .all_doc
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::Line { a, .. } => Some(a.y),
            _ => None,
        })
        .collect();
    // Two copies of the unchanged line (one for each side) at one place.
    let first = ys.iter().filter(|y| (**y - ys[0]).abs() < 1e-6).count();
    assert!(first >= 2, "{ys:?}");
}
