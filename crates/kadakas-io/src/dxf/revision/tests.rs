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

/// A LINE on a layer. The handle is left out if it is `None`.
fn line(handle: Option<u32>, layer: &str, a: (f64, f64), b: (f64, f64)) -> Entity {
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

/// The place of line number `k` on a lattice with 100 units between the lines.
fn spot(k: usize) -> (f64, f64) {
    ((k % 20) as f64 * 100.0, (k / 20) as f64 * 100.0)
}

fn lattice(n: usize, handles: bool) -> Vec<Entity> {
    (0..n)
        .map(|k| {
            let (x, y) = spot(k);
            line(
                handles.then_some(0x100 + k as u32),
                "A",
                (x, y),
                (x + 10.0, y + 5.0),
            )
        })
        .collect()
}

fn status(d: &Diff, st: Status) -> usize {
    d.entries.iter().filter(|e| e.status == st).count()
}

/// The result without the positions in the input, for a test of the order.
fn canonical_form(d: &Diff, base: &[Rec], new: &[Rec]) -> Vec<String> {
    let id = |r: &Rec| format!("{:?}/{:x}", r.handle, r.hash);
    let mut v: Vec<String> = d
        .entries
        .iter()
        .map(|e| {
            format!(
                "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                e.base.map(|i| id(&base[i])),
                e.new.map(|j| id(&new[j])),
                e.status,
                e.via,
                e.changes,
                e.distance
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn a_file_matches_itself() {
    for handles in [true, false] {
        let r = recs(&lattice(30, handles));
        let d = diff(&r, &r, &Options::default());
        assert_eq!(d.counts.unchanged, 30);
        assert_eq!(d.entries.len(), 30);
        assert!(!d.truncated && !d.renumbered);
        assert!(d.entries.iter().all(|e| e.changes.is_empty()));
        let via = if handles { Via::Handle } else { Via::Hash };
        assert!(d.entries.iter().all(|e| e.via == Some(via)));
        assert_eq!(d.counts.by_handle + d.counts.by_hash, 30);
    }
    let none = diff(&[], &[], &Options::default());
    assert!(none.entries.is_empty() && !none.truncated);
}

#[test]
fn edits_are_found_by_handle() {
    let base = lattice(10, true);
    let mut new = base.clone();
    new[1] = line(Some(0x101), "A", (1000.0, 1000.0), (1010.0, 1005.0)); // moved far away
    new[2] = line(
        Some(0x102),
        "B",
        spot(2),
        (spot(2).0 + 10.0, spot(2).1 + 5.0),
    ); // layer
    new[3].items.push((62, s(3))); // color
    new[4].items.push((6, s("DASHED"))); // linetype
    new[5].items.push((370, s(50))); // line weight
    new.remove(6); // deleted
    new.push(line(Some(0x999), "A", (5000.0, 0.0), (5010.0, 0.0))); // added
    let (b, n) = (recs(&base), recs(&new));
    let d = diff(&b, &n, &Options::default());
    assert_eq!(
        (
            d.counts.unchanged,
            d.counts.changed,
            d.counts.added,
            d.counts.removed
        ),
        (4, 5, 1, 1)
    );
    let of = |h: u32| {
        d.entries
            .iter()
            .find(|e| {
                e.base
                    .is_some_and(|i| b[i].handle == Some(format!("{h:X}")))
            })
            .unwrap()
    };
    assert_eq!(of(0x101).changes, vec![Change::Geometry]);
    assert_eq!(of(0x101).via, Some(Via::Handle));
    assert_eq!(of(0x102).changes, vec![Change::Layer]);
    assert_eq!(of(0x103).changes, vec![Change::Color]);
    assert_eq!(of(0x104).changes, vec![Change::Linetype]);
    assert_eq!(of(0x105).changes, vec![Change::Lineweight]);
    assert_eq!(of(0x106).status, Status::Removed);
    assert!(!d.renumbered);
}

#[test]
fn text_and_other_changes_are_named() {
    let t = |text: &str, flag: &str| {
        ent(
            "TEXT",
            &[
                (5, s("A1")),
                (8, s("T")),
                (1, s(text)),
                (10, s(0)),
                (20, s(0)),
                (71, s(flag)),
            ],
        )
    };
    let (b, n) = (recs(&[t("Oak", "0")]), recs(&[t("Elm", "0")]));
    let d = diff(&b, &n, &Options::default());
    assert_eq!(d.entries[0].changes, vec![Change::Text]);
    // A flag is no named property.
    let n = recs(&[t("Oak", "2")]);
    let d = diff(&b, &n, &Options::default());
    assert_eq!(d.entries[0].changes, vec![Change::Other]);
    // The same text moved and renamed names both.
    let mut moved = t("Elm", "0");
    moved.items[3].1 = s(1);
    let d = diff(&b, &recs(&[moved]), &Options::default());
    assert_eq!(d.entries[0].changes, vec![Change::Geometry, Change::Text]);
}

#[test]
fn without_handles_the_rest_matches_by_layer_type_and_geometry() {
    let base = lattice(12, false);
    let mut new = base.clone();
    // Moved a little: found by proximity.
    new[1] = line(None, "A", (115.0, 3.0), (125.0, 8.0));
    // Same geometry on another layer: found by the geometry hash.
    new[2] = line(None, "B", spot(2), (spot(2).0 + 10.0, spot(2).1 + 5.0));
    // Longer: edited, found by proximity.
    new[3] = line(None, "A", spot(3), (spot(3).0 + 14.0, spot(3).1 + 5.0));
    new.remove(4);
    new.push(line(None, "A", (9000.0, 0.0), (9010.0, 0.0)));
    let (b, n) = (recs(&base), recs(&new));
    let d = diff(&b, &n, &Options::default());
    assert_eq!(
        (
            d.counts.unchanged,
            d.counts.changed,
            d.counts.added,
            d.counts.removed
        ),
        (8, 3, 1, 1)
    );
    assert_eq!(d.counts.by_proximity, 2);
    let near: Vec<_> = d
        .entries
        .iter()
        .filter(|e| e.via == Some(Via::Proximity))
        .collect();
    assert!(near.iter().all(|e| e.distance.is_some_and(|x| x > 0.0)));
    assert!(near.iter().all(|e| e.changes == vec![Change::Geometry]));
    let layer = d
        .entries
        .iter()
        .find(|e| e.changes == vec![Change::Layer])
        .unwrap();
    assert_eq!(layer.via, Some(Via::Hash));
    assert!(d.tolerance > 0.0);
}

#[test]
fn a_move_with_a_layer_change_is_found_on_any_layer() {
    let base = recs(&[
        line(None, "A", (0.0, 0.0), (10.0, 0.0)),
        line(None, "A", (5000.0, 0.0), (5010.0, 0.0)),
    ]);
    let new = recs(&[
        line(None, "B", (2.0, 1.0), (12.0, 1.0)),
        line(None, "A", (5000.0, 0.0), (5010.0, 0.0)),
    ]);
    let d = diff(&base, &new, &Options::default());
    assert_eq!(d.counts.changed, 1);
    let e = d
        .entries
        .iter()
        .find(|e| e.status == Status::Changed)
        .unwrap();
    assert_eq!(e.changes, vec![Change::Geometry, Change::Layer]);
    // The same layer wins over a nearer entity on another layer.
    let base = recs(&[
        line(None, "A", (0.0, 0.0), (10.0, 0.0)),
        line(None, "B", (9000.0, 0.0), (9010.0, 0.0)),
    ]);
    let new = recs(&[
        line(None, "A", (3.0, 0.0), (13.0, 0.0)),
        line(None, "B", (1.0, 0.0), (11.0, 0.0)),
        line(None, "B", (9001.0, 0.0), (9011.0, 0.0)),
    ]);
    let d = diff(
        &base,
        &new,
        &Options {
            tolerance: 50.0,
            ..Options::default()
        },
    );
    let first = d
        .entries
        .iter()
        .find(|e| e.base.is_some_and(|i| i == 0))
        .unwrap();
    assert_eq!(first.new, Some(0), "same layer first");
    assert_eq!(d.counts.added, 1);
}

#[test]
fn types_and_scopes_never_mix() {
    let a = Rec::from_entity(&line(None, "A", (0.0, 0.0), (10.0, 0.0)), MODEL);
    let mut circle = ent("CIRCLE", &[(8, s("A")), (10, s(1)), (20, s(0)), (40, s(3))]);
    let b = Rec::from_entity(&circle, MODEL);
    let d = diff(
        std::slice::from_ref(&a),
        &[b],
        &Options {
            tolerance: 100.0,
            ..Options::default()
        },
    );
    assert_eq!((d.counts.added, d.counts.removed), (1, 1));
    // The same line inside a block is another entity.
    let in_block = Rec::from_entity(&line(None, "A", (0.0, 0.0), (10.0, 0.0)), "block:B");
    let d = diff(&[a], &[in_block], &Options::default());
    assert_eq!((d.counts.added, d.counts.removed), (1, 1));
    circle.items[3].1 = s(4);
    let (c1, c2) = (
        Rec::from_entity(&circle, MODEL),
        Rec::from_entity(&circle, MODEL),
    );
    assert_eq!(diff(&[c1], &[c2], &Options::default()).counts.unchanged, 1);
}

#[test]
fn a_changed_radius_is_an_edit() {
    let circle = |r: f64| {
        Rec::from_entity(
            &ent("CIRCLE", &[(10, s(50)), (20, s(50)), (40, s(r))]),
            MODEL,
        )
    };
    let far = Rec::from_entity(
        &ent("CIRCLE", &[(10, s(9000)), (20, s(0)), (40, s(1))]),
        MODEL,
    );
    let d = diff(
        &[circle(5.0), far.clone()],
        &[circle(6.0), far],
        &Options::default(),
    );
    assert_eq!(d.counts.changed, 1);
    assert_eq!(
        d.entries
            .iter()
            .find(|e| e.status == Status::Changed)
            .unwrap()
            .via,
        Some(Via::Proximity)
    );
}

#[test]
fn renumbered_handles_fall_back_to_geometry() {
    let base = lattice(20, true);
    let mut new = base.clone();
    // Every entity takes the handle of the next one.
    for (k, e) in new.iter_mut().enumerate() {
        e.items[0].1 = format!("{:X}", 0x100 + (k + 1) % 20);
    }
    // And two entities are edited.
    let (x, y) = spot(3);
    new[3] = line(Some(0x104), "A", (x + 2.0, y), (x + 12.0, y + 5.0));
    new[7].items[1].1 = s("B");
    let (b, n) = (recs(&base), recs(&new));
    let d = diff(&b, &n, &Options::default());
    assert!(d.renumbered);
    assert_eq!(
        (
            d.counts.unchanged,
            d.counts.changed,
            d.counts.added,
            d.counts.removed
        ),
        (18, 2, 0, 0)
    );
    assert_eq!(d.counts.by_handle, 0);
    // The same drawing with a few real edits is not taken for renumbered.
    let mut few = base.clone();
    few[0].items[1].1 = s("B");
    let d = diff(&b, &recs(&few), &Options::default());
    assert!(!d.renumbered);
    assert_eq!((d.counts.changed, d.counts.by_handle), (1, 20));
}

#[test]
fn shared_handles_and_handles_of_other_types_identify_nothing() {
    let a = recs(&[
        line(Some(1), "A", (0.0, 0.0), (10.0, 0.0)),
        line(Some(1), "A", (500.0, 0.0), (510.0, 0.0)),
    ]);
    let d = diff(&a, &a, &Options::default());
    assert_eq!(d.counts.by_handle, 0, "a shared handle is ignored");
    assert_eq!(d.counts.unchanged, 2);
    let line_ = recs(&[line(Some(5), "A", (0.0, 0.0), (10.0, 0.0))]);
    let text = recs(&[ent(
        "TEXT",
        &[
            (5, s("5")),
            (8, s("A")),
            (1, s("x")),
            (10, s(0)),
            (20, s(0)),
        ],
    )]);
    let d = diff(
        &line_,
        &text,
        &Options {
            proximity: false,
            ..Options::default()
        },
    );
    assert_eq!((d.counts.added, d.counts.removed), (1, 1));
}

#[test]
fn identical_entities_pair_in_a_fixed_order() {
    let same = |h: u32| line(Some(h), "A", (0.0, 0.0), (10.0, 0.0));
    let base = recs(&[same(1), same(2), same(3)]);
    let new = recs(&[same(9), same(8)]);
    let d = diff(&base, &new, &Options::default());
    assert_eq!((d.counts.unchanged, d.counts.removed), (2, 1));
    let rev: Vec<Rec> = new.iter().rev().cloned().collect();
    let d2 = diff(&base, &rev, &Options::default());
    assert_eq!(
        canonical_form(&d, &base, &new),
        canonical_form(&d2, &base, &rev)
    );
}

#[test]
fn the_entity_cap_cuts_the_same_entities_in_any_order() {
    let base = recs(&lattice(10, true));
    let rev: Vec<Rec> = base.iter().rev().cloned().collect();
    let o = Options {
        max_entities: 6,
        ..Options::default()
    };
    let d1 = diff(&base, &base, &o);
    let d2 = diff(&rev, &rev, &o);
    assert!(d1.truncated && d2.truncated);
    assert_eq!(d1.entries.len(), 6);
    assert_eq!(
        canonical_form(&d1, &base, &base),
        canonical_form(&d2, &rev, &rev)
    );
}

#[test]
fn the_work_budget_stops_the_nearest_match() {
    // 200 entities in the same place in both files, all different: 200 * 200 candidates.
    let make = |dy: f64| -> Vec<Rec> {
        (0..200)
            .map(|k| {
                Rec::from_entity(
                    &line(None, "A", (0.0, 0.0), (10.0 + k as f64 * 1e-3 + dy, 0.0)),
                    MODEL,
                )
            })
            .collect()
    };
    let (base, new) = (make(0.0), make(0.5));
    let full = diff(
        &base,
        &new,
        &Options {
            tolerance: 10.0,
            ..Options::default()
        },
    );
    assert!(!full.truncated);
    assert_eq!(full.counts.changed, 200);
    assert!(full.work > 40_000);
    for budget in [0, 1, 500, 5_000, full.work - 1] {
        let o = Options {
            tolerance: 10.0,
            work_budget: budget,
            ..Options::default()
        };
        let d = diff(&base, &new, &o);
        assert!(d.truncated, "{budget}");
        assert!(d.work <= budget, "{budget}: {}", d.work);
        assert_eq!(
            d.entries.len(),
            d.counts.unchanged + d.counts.changed + d.counts.added + d.counts.removed
        );
        // Every entity is accounted for once on each side.
        let bs: Vec<_> = d.entries.iter().filter_map(|e| e.base).collect();
        assert_eq!(bs.len(), 200);
    }
    let off = diff(
        &base,
        &new,
        &Options {
            proximity: false,
            ..Options::default()
        },
    );
    assert_eq!(
        (off.counts.added, off.counts.removed, off.work),
        (200, 200, 0)
    );
}

#[test]
fn bad_numbers_and_options_do_not_panic() {
    let mut nan = Rec::from_entity(&line(None, "A", (0.0, 0.0), (1.0, 1.0)), MODEL);
    nan.points = vec![[f64::NAN, f64::INFINITY]];
    nan.sizes = vec![f64::NAN];
    nan.diag = f64::NAN;
    nan.center = Some([f64::NAN, 0.0]);
    let huge = Rec {
        points: vec![[1e300, -1e300]],
        center: Some([1e300, -1e300]),
        kind: "LINE".into(),
        ..Rec::default()
    };
    let ok = Rec::from_entity(&line(None, "A", (0.0, 0.0), (1.0, 1.0)), MODEL);
    for tol in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-300, 1e300] {
        let o = Options {
            tolerance: tol,
            ..Options::default()
        };
        let d = diff(
            &[nan.clone(), huge.clone(), ok.clone()],
            &[huge.clone(), ok.clone(), nan.clone()],
            &o,
        );
        assert!(d.entries.len() >= 3);
    }
    // The automatic tolerance has nothing to work with.
    let none = Rec {
        kind: "X".into(),
        ..Rec::default()
    };
    let d = diff(
        std::slice::from_ref(&none),
        std::slice::from_ref(&none),
        &Options::default(),
    );
    assert_eq!((d.tolerance, d.counts.unchanged), (0.0, 1));
}

#[test]
fn real_files_give_blocks_and_paper_space_their_own_scope() {
    let file = |extra: &str| {
        format!(
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n5\n20\n0\nLINE\n5\n21\n8\n0\n10\n0\n20\n0\n11\n5\n21\n0\n0\nENDBLK\n0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n30\n8\nA\n10\n0\n20\n0\n11\n9\n21\n9\n0\nINSERT\n5\n31\n2\nB\n10\n100\n20\n0\n{extra}\
             0\nENDSEC\n0\nEOF\n"
        )
    };
    let load = |t: &str| records(&parse::parse(t.as_bytes()).unwrap(), 1000).0;
    let a = load(&file(""));
    let b = load(&file("50\n45\n"));
    let d = diff(&a, &b, &Options::default());
    assert_eq!((d.counts.unchanged, d.counts.changed), (2, 1));
    let changed = d
        .entries
        .iter()
        .find(|e| e.status == Status::Changed)
        .unwrap();
    assert_eq!(b[changed.new.unwrap()].kind, "INSERT");
    assert_eq!(changed.changes, vec![Change::Geometry]);
}

fn small_text() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u8>(), 0..400)
        .prop_map(|b| String::from_utf8_lossy(&b).into_owned())
}

proptest! {
    #[test]
    fn hostile_input_never_panics(
        junk in small_text(),
        codes in prop::collection::vec((any::<i16>(), "[ -~]{0,12}"), 0..60),
        tol in prop::sample::select(vec![0.0, 1.0, -3.0, f64::NAN, 1e-9, 1e12]),
    ) {
        // Raw bytes inside a file frame, and random pairs as an entity.
        let framed = format!("0\nSECTION\n2\nENTITIES\n{junk}\n0\nENDSEC\n0\nEOF\n");
        if let Ok(d) = parse::parse(framed.as_bytes()) {
            let (r, _) = records(&d, 50);
            let _ = diff(&r, &r, &Options::default());
        }
        let mut e = Entity { kind: "LWPOLYLINE".into(), items: vec![], children: vec![] };
        e.items = codes.iter().map(|(c, v)| (i32::from(*c), v.clone())).collect();
        let a = Rec::from_entity(&e, MODEL);
        let mut moved = e.clone();
        moved.items.push((10, "1e308".into()));
        moved.items.push((20, "-1e308".into()));
        let b = Rec::from_entity(&moved, MODEL);
        let o = Options { tolerance: tol, work_budget: 1000, ..Options::default() };
        let d = diff(&[a.clone(), b.clone()], &[b, a], &o);
        prop_assert!(d.work <= 1000);
    }

    /// Edits, deletes, adds and moves of a drawing are found exactly: with handles, without
    /// handles, and with renumbered handles.
    #[test]
    fn random_revisions_are_matched(
        n in 4usize..60,
        plan in prop::collection::vec((0u8..4, 1.0f64..6.0, -6.0f64..6.0), 0..60),
        adds in 0usize..5,
        mode in 0u8..3,
    ) {
        let base = lattice(n, mode != 1);
        let mut new = base.clone();
        let (mut moved, mut layered, mut deleted) = (0, 0, 0);
        // At most a quarter of the entities change, so the handles of mode 2 stay detectable.
        let edits = n / 4;
        let mut drop_at = Vec::new();
        for (k, (op, dx, dy)) in plan.iter().take(edits).enumerate() {
            let (x, y) = spot(k);
            match op % 3 {
                0 => {
                    new[k] = line(
                        (mode != 1).then_some(0x100 + k as u32),
                        "A",
                        (x + dx, y + dy),
                        (x + dx + 10.0, y + dy + 5.0),
                    );
                    moved += 1;
                }
                1 => {
                    new[k].items.iter_mut().find(|(c, _)| *c == 8).unwrap().1 = "B".into();
                    layered += 1;
                }
                _ => {
                    drop_at.push(k);
                    deleted += 1;
                }
            }
        }
        for &k in drop_at.iter().rev() {
            new.remove(k);
        }
        for j in 0..adds {
            let (x, y) = spot(n + 40 + j);
            new.push(line((mode != 1).then_some(0x900 + j as u32), "A", (x, y), (x + 10.0, y + 5.0)));
        }
        if mode == 2 {
            // The new file numbers its entities again, from the other end.
            let count = new.len() as u32;
            for (k, e) in new.iter_mut().enumerate() {
                e.items[0].1 = format!("{:X}", 0x100 + count - 1 - k as u32);
            }
        }
        let (b, nw) = (recs(&base), recs(&new));
        let d = diff(&b, &nw, &Options { tolerance: 30.0, ..Options::default() });
        prop_assert!(!d.truncated);
        prop_assert_eq!(status(&d, Status::Removed), deleted);
        prop_assert_eq!(status(&d, Status::Added), adds);
        prop_assert_eq!(status(&d, Status::Changed), moved + layered);
        prop_assert_eq!(status(&d, Status::Unchanged), n - deleted - moved - layered);
        // Every entity is in the result exactly once.
        let mut bs: Vec<usize> = d.entries.iter().filter_map(|e| e.base).collect();
        let mut ns: Vec<usize> = d.entries.iter().filter_map(|e| e.new).collect();
        bs.sort();
        ns.sort();
        prop_assert_eq!(bs, (0..n).collect::<Vec<_>>());
        prop_assert_eq!(ns, (0..nw.len()).collect::<Vec<_>>());
        if mode == 0 {
            prop_assert_eq!(d.counts.by_handle, n - deleted);
        }

        // The order of the input does not change the result.
        let rb: Vec<Rec> = b.iter().rev().cloned().collect();
        let mut rn: Vec<Rec> = nw.clone();
        let half = rn.len() / 2;
        rn.rotate_left(half);
        let d2 = diff(&rb, &rn, &Options { tolerance: 30.0, ..Options::default() });
        prop_assert_eq!(canonical_form(&d, &b, &nw), canonical_form(&d2, &rb, &rn));
    }
}
