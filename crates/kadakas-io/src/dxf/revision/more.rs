//! More tests of the matching: the tolerance, the budget, handle pairs judged one by one, scopes,
//! colors, anonymous blocks and stored records.

use proptest::prelude::*;

use super::*;
use crate::dxf::parse::{self, Entity};

fn ent(kind: &str, items: &[(i32, String)]) -> Entity {
    Entity {
        kind: kind.into(),
        items: items.to_vec(),
        children: vec![],
    }
}

fn s(v: impl ToString) -> String {
    v.to_string()
}

fn line_at(handle: Option<u32>, layer: &str, a: (f64, f64), b: (f64, f64)) -> Entity {
    let mut items = Vec::new();
    if let Some(h) = handle {
        items.push((5, format!("{h:X}")));
    }
    items.extend([
        (8, s(layer)),
        (10, s(a.0)),
        (20, s(a.1)),
        (11, s(b.0)),
        (21, s(b.1)),
    ]);
    ent("LINE", &items)
}

fn recs(es: &[Entity]) -> Vec<Rec> {
    es.iter().map(|e| Rec::from_entity(e, MODEL)).collect()
}

fn spot(k: usize) -> (f64, f64) {
    ((k % 20) as f64 * 100.0, (k / 20) as f64 * 100.0)
}

fn lattice(n: usize) -> Vec<Entity> {
    (0..n)
        .map(|k| {
            let (x, y) = spot(k);
            line_at(Some(0x100 + k as u32), "A", (x, y), (x + 10.0, y + 5.0))
        })
        .collect()
}

fn counts(d: &Diff) -> (usize, usize, usize, usize) {
    let c = &d.counts;
    (c.unchanged, c.changed, c.added, c.removed)
}

#[test]
fn a_stray_entity_does_not_widen_the_tolerance() {
    // A survey: the drawing lies at 6.5 million, and one stray line sits at (0, 0).
    let make = |dx: f64| -> Vec<Entity> {
        let mut v: Vec<Entity> = (0..30)
            .map(|k| {
                let x = 6_500_000.0 + k as f64 * 100.0;
                let d = if k == 7 { dx } else { 0.0 };
                line_at(None, "A", (x + d, 6_000_000.0), (x + d + 10.0, 6_000_005.0))
            })
            .collect();
        v.push(line_at(None, "A", (0.0, 0.0), (1.0, 1.0)));
        v
    };
    let (b, n) = (recs(&make(0.0)), recs(&make(3.0)));
    let d = diff(&b, &n, &Options::default());
    assert_eq!(counts(&d), (30, 1, 0, 0));
    assert_eq!(d.counts.by_proximity, 1);
    assert!(
        d.tolerance > 3.0 && d.tolerance < 100.0,
        "the stray entity is ignored: {}",
        d.tolerance
    );
}

#[test]
fn the_tolerance_has_an_absolute_cap_for_the_unit() {
    let spread = |dx: f64| -> Vec<Rec> {
        let es: Vec<Entity> = (0..20)
            .map(|k| {
                line_at(
                    None,
                    "A",
                    (k as f64 * 1e7 + dx, 0.0),
                    (k as f64 * 1e7 + dx + 10.0, 0.0),
                )
            })
            .collect();
        recs(&es)
    };
    let d = diff(&spread(0.0), &spread(1.0), &Options::default());
    assert_eq!(d.tolerance, 1000.0, "one meter in millimeters");
    let meters = Options::for_unit(1000.0);
    assert_eq!(meters.max_tolerance, 1.0);
    let d = diff(&spread(0.0), &spread(0.5), &meters);
    assert_eq!(d.tolerance, 1.0);
    assert_eq!(Options::for_unit(0.0).max_tolerance, 1000.0);
    // No cap: the part of the size of the drawing.
    let free = Options {
        max_tolerance: 0.0,
        ..Options::default()
    };
    assert!(diff(&spread(0.0), &spread(1.0), &free).tolerance > 1000.0);
}

#[test]
fn entities_that_share_a_center_use_the_size_for_the_tolerance() {
    let circle = |r: f64| {
        Rec::from_entity(
            &ent("CIRCLE", &[(10, s(50)), (20, s(50)), (40, s(r))]),
            MODEL,
        )
    };
    let d = diff(&[circle(5.0)], &[circle(6.0)], &Options::default());
    assert!(d.tolerance > 0.0);
    assert_eq!(counts(&d), (0, 1, 0, 0));
}

#[test]
fn a_pair_may_not_jump_far_for_its_size() {
    // Two small lines 80 units apart are two entities when the tolerance is 100: a line of 10
    // does not move 8 times its own length.
    let a = recs(&[line_at(None, "A", (0.0, 0.0), (10.0, 0.0))]);
    let b = recs(&[line_at(None, "A", (80.0, 0.0), (90.0, 0.0))]);
    let o = Options {
        tolerance: 100.0,
        ..Options::default()
    };
    assert_eq!(counts(&diff(&a, &b, &o)), (0, 0, 1, 1));
    let near = recs(&[line_at(None, "A", (15.0, 0.0), (25.0, 0.0))]);
    assert_eq!(counts(&diff(&a, &near, &o)), (0, 1, 0, 0));
}

#[test]
fn the_cost_is_charged_before_the_point_loop() {
    // Many entities with the same center and very different shapes.
    let shape = |k: usize, w: f64| -> Rec {
        let n = 4000;
        let mut r = Rec {
            kind: "LWPOLYLINE".into(),
            layer: "A".into(),
            points: (0..n)
                .map(|i| {
                    let t = i as f64 / n as f64 * std::f64::consts::TAU;
                    [t.cos() * (w + k as f64), t.sin() * (w + k as f64)]
                })
                .collect(),
            hash: k as u64 + (w as u64) * 1000,
            geo_hash: k as u64 + (w as u64) * 1000,
            ..Rec::default()
        };
        r.finish();
        r
    };
    let base: Vec<Rec> = (0..20).map(|k| shape(k, 100.0)).collect();
    let new: Vec<Rec> = (0..20).map(|k| shape(k, 200.0)).collect();
    let o = Options {
        tolerance: 1000.0,
        work_budget: 10_000,
        ..Options::default()
    };
    let d = diff(&base, &new, &o);
    assert!(d.truncated);
    assert!(d.work <= 10_000, "{}", d.work);
    // A budget for two candidates stops at two.
    let tiny = Options {
        work_budget: 2 * 4001,
        ..o
    };
    assert!(diff(&base, &new, &tiny).work <= 2 * 4001);
}

#[test]
fn a_far_move_with_the_same_handle_is_a_move_when_the_scope_is_clean() {
    let a = recs(&[line_at(Some(1), "A", (0.0, 0.0), (10.0, 0.0))]);
    let b = recs(&[line_at(Some(1), "A", (5000.0, 0.0), (5010.0, 0.0))]);
    let o = Options {
        tolerance: 100.0,
        ..Options::default()
    };
    let d = diff(&a, &b, &o);
    assert_eq!(counts(&d), (0, 1, 0, 0));
    assert_eq!(d.counts.by_handle, 1);
    assert_eq!(d.entries[0].changes, vec![Change::Geometry]);
    assert!(!d.renumbered);
}

#[test]
fn a_survey_that_moved_as_a_whole_has_every_entity_moved() {
    // A re-base: every coordinate is a million units away. Nothing is added or removed.
    let make = |dx: f64| -> Vec<Rec> {
        recs(
            &(0..20)
                .map(|k| {
                    let (x, y) = spot(k);
                    line_at(
                        Some(0x100 + k as u32),
                        "A",
                        (x + dx, y),
                        (x + dx + 10.0, y + 5.0),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let d = diff(&make(0.0), &make(1.0e6), &Options::default());
    assert_eq!(counts(&d), (0, 20, 0, 0));
    assert_eq!((d.counts.by_handle, d.renumbered), (20, false));
    assert!(
        d.entries
            .iter()
            .all(|e| e.changes == vec![Change::Geometry])
    );
}

#[test]
fn the_distance_counts_only_where_renumbering_is_suspected() {
    // Two entities swapped their handles (each has a better partner), so the scope is suspected
    // but not renumbered; a third entity with a handle moved far.
    let base: Vec<Entity> = (0..10)
        .map(|k| {
            let (x, y) = spot(k);
            line_at(Some(0x100 + k as u32), "A", (x, y), (x + 10.0, y + 5.0))
        })
        .collect();
    let mut new = base.clone();
    new[0].items[0].1 = "101".into();
    new[1].items[0].1 = "100".into();
    let (x, y) = spot(5);
    new[5] = line_at(Some(0x105), "A", (x + 5000.0, y), (x + 5010.0, y + 5.0));
    let o = Options {
        tolerance: 50.0,
        ..Options::default()
    };
    let d = diff(&recs(&base), &recs(&new), &o);
    assert!(!d.renumbered);
    // The far pair is dropped (removed and added); the swapped pair is found again by content.
    assert_eq!(counts(&d), (9, 0, 1, 1));
}

#[test]
fn each_scope_decides_for_itself_whether_its_handles_are_renumbered() {
    let mut base = Vec::new();
    let mut new = Vec::new();
    for k in 0..10u32 {
        let (x, y) = (k as f64 * 100.0, 0.0);
        base.push(Rec::from_entity(
            &line_at(Some(0x100 + k), "A", (x, y), (x + 10.0, y)),
            MODEL,
        ));
        // The model of the new file has its handles in the other order.
        new.push(Rec::from_entity(
            &line_at(Some(0x100 + 9 - k), "A", (x, y), (x + 10.0, y)),
            MODEL,
        ));
    }
    for k in 0..3u32 {
        let e = line_at(
            Some(0x200 + k),
            "A",
            (k as f64 * 50.0, 900.0),
            (k as f64 * 50.0 + 5.0, 900.0),
        );
        base.push(Rec::from_entity(&e, "block:rec:1"));
        new.push(Rec::from_entity(&e, "block:rec:1"));
    }
    let d = diff(&base, &new, &Options::default());
    assert!(d.renumbered);
    assert_eq!(counts(&d), (13, 0, 0, 0));
    assert_eq!(d.counts.by_handle, 3, "the block keeps its handles");
    assert_eq!(d.counts.by_hash, 10);
}

#[test]
fn equal_geometry_prefers_the_pair_with_the_same_color() {
    let l = |color: &str, dashed: bool| {
        let mut items = vec![
            (8, s("A")),
            (62, s(color)),
            (10, s(0)),
            (20, s(0)),
            (11, s(10)),
            (21, s(0)),
        ];
        if dashed {
            items.push((6, s("DASHED")));
        }
        Rec::from_entity(&ent("LINE", &items), MODEL)
    };
    let base = [l("1", false), l("2", false)];
    let new = [l("2", true), l("1", true)];
    let d = diff(&base, &new, &Options::default());
    assert_eq!(counts(&d), (0, 2, 0, 0));
    assert!(
        d.entries
            .iter()
            .all(|e| e.changes == vec![Change::Linetype])
    );
}

fn file_with_block(name: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$A\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n20\n330\n1F\n2\n{name}\n0\nLINE\n5\n21\n8\n0\n10\n0\n20\n0\n11\n5\n21\n0\n0\nENDBLK\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nINSERT\n5\n30\n8\nA\n2\n{name}\n10\n100\n20\n0\n0\nENDSEC\n0\nEOF\n"
    )
}

#[test]
fn anonymous_block_names_do_not_make_changes() {
    let load = |t: &str| records(&parse::parse(t.as_bytes()).unwrap(), 1000).0;
    // The same drawing saved twice: the program numbers the anonymous block again.
    let (a, b) = (load(&file_with_block("*U1")), load(&file_with_block("*U9")));
    // The BLOCK_RECORD handle names the block first.
    assert!(a.iter().any(|r| r.scope == "block:rec:1F"));
    assert!(a.iter().all(|r| !r.text.contains('*')));
    let d = diff(&a, &b, &Options::default());
    assert_eq!(counts(&d), (2, 0, 0, 0), "{:?}", d.entries);
    // Without a handle the content names an anonymous block, so an edit inside it shows.
    let bare = |name: &str, len: &str| {
        load(
            &file_with_block(name)
                .replace("\n330\n1F", "")
                .replace("11\n5\n", &format!("11\n{len}\n")),
        )
    };
    let (a, b) = (bare("*U1", "5"), bare("*U9", "5"));
    assert!(a.iter().any(|r| r.scope.starts_with("block:anon:")));
    assert_eq!(counts(&diff(&a, &b, &Options::default())), (2, 0, 0, 0));
    let edited = bare("*U9", "6");
    let d = diff(&a, &edited, &Options::default());
    // The block has a new key, so its entity is removed and added; the INSERT does not change.
    assert_eq!(counts(&d), (1, 0, 1, 1), "{:?}", d.entries);
    // A named block that is renamed is a change of the INSERT, and its content keeps its scope
    // by the record handle.
    let (a, b) = (
        load(&file_with_block("TREE")),
        load(&file_with_block("OAK")),
    );
    assert_eq!(a[1].scope, b[1].scope);
    let d = diff(&a, &b, &Options::default());
    assert_eq!(counts(&d), (1, 1, 0, 0));
    assert_eq!(
        d.entries
            .iter()
            .find(|e| e.status == Status::Changed)
            .map(|e| e.changes.clone()),
        Some(vec![Change::Text])
    );
}

#[test]
fn stored_records_round_trip_and_stay_small() {
    let mut r = Rec::from_entity(
        &line_at(Some(0x2F), "Walls", (0.0, 0.0), (10.0, 5.0)),
        "block:rec:1F",
    );
    r.points = (0..200).map(|i| [i as f64, 0.0]).collect();
    r.finish();
    r.compact();
    assert_eq!(r.points.len(), MAX_STORED_POINTS);
    let v = r.to_value();
    assert_eq!(Rec::from_value(&v), Some(r.clone()));
    assert!(
        v["hash"].as_str().is_some_and(|h| h.len() == 16),
        "a hash is hex text"
    );
    // Bad values are refused or cut.
    assert_eq!(Rec::from_value(&serde_json::json!(5)), None);
    assert_eq!(Rec::from_value(&serde_json::json!({"hash": "zz"})), None);
    let mut long = v.clone();
    long["points"] = serde_json::json!(vec![[1.0, 2.0]; 500]);
    assert_eq!(
        Rec::from_value(&long).unwrap().points.len(),
        MAX_STORED_POINTS
    );
    // A stored record matches a record made again from the file.
    let again = Rec::from_value(&v).unwrap();
    let d = diff(
        &[again],
        &[Rec::from_entity(
            &line_at(Some(0x2F), "Walls", (0.0, 0.0), (10.0, 5.0)),
            "block:rec:1F",
        )],
        &Options::default(),
    );
    assert_eq!(counts(&d), (1, 0, 0, 0));
    let big =
        serde_json::json!({"type": "import_base", "drawing": "d", "revision": "r", "recs": [v]});
    assert!(kadakas_core::provenance::check_base(&big).is_ok());
}

proptest! {
    /// A drawing with an entity inserted or deleted in the middle, and the handles numbered again
    /// in sequence: every entity after the change has a new handle, and the result is still one
    /// added or removed entity.
    #[test]
    fn an_insert_or_delete_in_the_middle_with_new_handles(
        n in 6usize..60,
        at in 1usize..5,
        delete in any::<bool>(),
    ) {
        let base = lattice(n);
        let mut new = base.clone();
        let m = at.min(n - 2);
        if delete {
            new.remove(m);
        } else {
            new.insert(m, line_at(None, "A", (9000.0, 9000.0), (9010.0, 9005.0)));
        }
        for (k, e) in new.iter_mut().enumerate() {
            let h = format!("{:X}", 0x100 + k);
            match e.items.iter_mut().find(|(c, _)| *c == 5) {
                Some(item) => item.1 = h,
                None => e.items.insert(0, (5, h)),
            }
        }
        let (b, nw) = (recs(&base), recs(&new));
        let o = Options { tolerance: 30.0, ..Options::default() };
        let d = diff(&b, &nw, &o);
        if delete {
            prop_assert_eq!(counts(&d), (n - 1, 0, 0, 1));
        } else {
            prop_assert_eq!(counts(&d), (n, 0, 1, 0));
        }
        prop_assert!(!d.truncated);
    }
}
