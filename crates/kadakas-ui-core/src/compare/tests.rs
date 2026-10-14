use serde_json::json;

use super::*;
use crate::versions::VersionList;

fn list(ids: &[(u64, &str)], current_is_newest: bool) -> VersionList {
    let rows: Vec<_> = ids
        .iter()
        .map(|(id, branch)| json!({"id": id, "name": format!("V{id}"), "branch": branch}))
        .collect();
    VersionList::parse(&json!({
        "versions": rows,
        "branch": "main",
        "current_is_newest": current_is_newest,
    }))
}

fn v(id: u64) -> Side {
    Side::Version(id)
}

#[test]
fn the_slider_goes_from_the_newest_pair_back_in_time() {
    let l = list(&[(1, "main"), (2, "main"), (3, "main")], true);
    let mut s = HistorySlider::for_branch(&l, None);
    // The drawing equals the newest version: it is not a step of its own.
    assert_eq!(s.steps(), &[v(3), v(2), v(1)]);
    assert_eq!(s.len(), 2);
    assert_eq!(s.mode(), SliderMode::Neighbours);
    assert_eq!(
        s.pair(),
        Some((v(2), v(3))),
        "what the newest version changed"
    );
    assert!(s.step(1));
    assert_eq!(s.pair(), Some((v(1), v(2))));
    assert!(!s.step(1), "the end stops the slider");
    assert_eq!(s.position(), 1);
    assert!(s.step(-5));
    assert_eq!(s.position(), 0);
    assert!(!s.step(-1));
    s.set_position(99);
    assert_eq!(s.position(), 1);
    assert_eq!(s.pair_at(2), None);
    assert!(!s.is_empty());
    // A drawing that differs from the newest version is the first step.
    let l = list(&[(1, "main"), (2, "main")], false);
    let s = HistorySlider::for_branch(&l, None);
    assert_eq!(s.steps(), &[Side::Current, v(2), v(1)]);
    assert_eq!(s.pair(), Some((v(2), Side::Current)));
}

#[test]
fn the_slider_follows_one_branch() {
    let l = list(&[(1, "main"), (2, "idea"), (3, "main"), (4, "idea")], true);
    let main = HistorySlider::for_branch(&l, None);
    assert_eq!(main.steps(), &[v(3), v(1)]);
    let idea = HistorySlider::for_branch(&l, Some("idea"));
    assert_eq!(idea.steps(), &[v(4), v(2)]);
    assert_eq!(idea.pair(), Some((v(2), v(4))));
    // A list without a branch name is the main branch.
    let none = VersionList {
        rows: list(&[(1, "main")], true).rows,
        current_is_newest: Some(true),
        ..VersionList::default()
    };
    assert_eq!(HistorySlider::for_branch(&none, None).steps(), &[v(1)]);
    assert!(HistorySlider::for_branch(&none, None).is_empty());
    assert_eq!(HistorySlider::for_branch(&none, None).pair(), None);
}

#[test]
fn a_fixed_base_compares_every_step_with_the_base() {
    let l = list(&[(1, "main"), (2, "main"), (3, "main"), (4, "main")], false);
    let mut s = HistorySlider::for_branch(&l, None);
    assert_eq!(s.steps(), &[Side::Current, v(4), v(3), v(2), v(1)]);
    s.set_mode(SliderMode::FixedBase);
    // The base is the oldest version: every other side is compared with it.
    assert_eq!(s.base(), Some(v(1)));
    assert_eq!(s.len(), 4);
    let pairs: Vec<_> = (0..4).map(|p| s.pair_at(p).unwrap()).collect();
    assert_eq!(
        pairs,
        vec![
            (v(1), Side::Current),
            (v(1), v(4)),
            (v(1), v(3)),
            (v(1), v(2)),
        ]
    );
    // Another base: only the sides newer than it have steps.
    assert!(s.set_base(v(3)));
    assert_eq!(s.len(), 2);
    assert_eq!(s.pair_at(1), Some((v(3), v(4))));
    assert_eq!(s.pair_at(2), None);
    assert!(!s.set_base(v(99)), "not a side of the slider");
    assert_eq!(s.base(), Some(v(3)));
    // The newest side as the base leaves nothing to compare it with.
    assert!(s.set_base(Side::Current));
    assert!(s.is_empty() && s.pair().is_none() && s.neighbours().is_empty());
    // A position past the end of a shorter mode is brought back.
    s.set_base(v(1));
    s.set_position(3);
    s.set_mode(SliderMode::Neighbours);
    assert_eq!(s.len(), 4);
    s.set_mode(SliderMode::FixedBase);
    s.set_base(v(3));
    assert_eq!(s.position(), 1);
}

#[test]
fn the_pairs_next_to_a_position_are_given_for_a_cache() {
    let l = list(&[(1, "main"), (2, "main"), (3, "main"), (4, "main")], true);
    let mut s = HistorySlider::for_branch(&l, None);
    assert_eq!(
        s.neighbours(),
        vec![(v(2), v(3))],
        "only one way from the start"
    );
    s.set_position(1);
    assert_eq!(s.neighbours(), vec![(v(1), v(2)), (v(3), v(4))]);
    s.set_position(2);
    assert_eq!(s.neighbours(), vec![(v(2), v(3))]);
    assert!(HistorySlider::new(vec![]).neighbours().is_empty());
    assert_eq!(HistorySlider::new(vec![v(1)]).len(), 0);
}

#[test]
fn a_new_list_keeps_the_place_of_the_person() {
    let l = list(&[(1, "main"), (2, "main"), (3, "main")], true);
    let mut s = HistorySlider::for_branch(&l, None);
    s.set_position(1);
    s.set_mode(SliderMode::FixedBase);
    s.set_base(v(2));
    s.set_position(0);
    // A new version arrives: the pair that the person looks at is still there, one step later.
    let more = list(&[(1, "main"), (2, "main"), (3, "main"), (4, "main")], true);
    s.update(&more, None);
    assert_eq!(s.mode(), SliderMode::FixedBase);
    assert_eq!(s.base(), Some(v(2)));
    assert_eq!(s.pair(), Some((v(2), v(3))));
    assert_eq!(s.position(), 1);
    // The version of the pair is deleted: the nearest position stays.
    let fewer = list(&[(1, "main"), (2, "main")], true);
    s.update(&fewer, None);
    assert_eq!(s.base(), Some(v(2)));
    assert!(s.position() < s.len().max(1));
    // The base is gone: the base is the oldest side again.
    let only = list(&[(5, "main"), (6, "main")], true);
    s.update(&only, None);
    assert_eq!(s.base(), Some(v(5)));
    // No place to keep: a new slider position 0.
    let mut n = HistorySlider::for_branch(&list(&[(1, "main"), (2, "main")], true), None);
    n.update(&list(&[], true), None);
    assert_eq!((n.len(), n.position()), (0, 0));
}

#[test]
fn the_form_needs_two_different_sides_and_forgets_deleted_versions() {
    let mut f = CompareForm::default();
    assert_eq!(f.pair(), None);
    assert!(!f.swap());
    f.from = Some(v(2));
    assert_eq!(f.pair(), Some((v(2), Side::Current)));
    assert!(f.swap());
    assert_eq!((f.from, f.to), (Some(Side::Current), v(2)));
    assert_eq!(f.pair(), Some((Side::Current, v(2))));
    f.from = Some(v(2));
    assert_eq!(f.pair(), None, "the same side on both ends");
    let l = list(&[(1, "main")], true);
    f.from = Some(v(1));
    f.to = v(2);
    f.retain(&l);
    assert_eq!((f.from, f.to), (Some(v(1)), Side::Current));
    f.from = Some(v(9));
    f.retain(&l);
    assert_eq!(f.from, None);
}

fn row(change: Change, group: &str) -> ChangeRow {
    ChangeRow {
        change,
        group: group.into(),
        id: "abc".into(),
        shape: Some("rect".into()),
        name: None,
        paths: vec![],
    }
}

#[test]
fn the_list_shows_objects_by_status_and_a_click_selects_or_frames() {
    let rows = vec![
        row(Change::Added, "objects"),
        row(Change::Removed, "objects"),
        row(Change::Changed, "objects"),
        row(Change::Changed, "layers"),
    ];
    assert_eq!(visible_rows(&rows, &RowFilter::default()).len(), 3);
    let only_removed = RowFilter {
        added: false,
        removed: true,
        changed: false,
    };
    let shown = visible_rows(&rows, &only_removed);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].change, Change::Removed);
    assert!(only_removed.shows(Change::Removed) && !only_removed.shows(Change::Added));
    assert!(!only_removed.shows(Change::Changed));
    assert_eq!(
        click_action(&rows[0]),
        ClickAction::SelectAndZoom("abc".into())
    );
    assert_eq!(
        click_action(&rows[2]),
        ClickAction::SelectAndZoom("abc".into())
    );
    assert_eq!(
        click_action(&rows[1]),
        ClickAction::FrameGhost("abc".into())
    );
    assert_eq!(click_action(&rows[3]), ClickAction::Nothing);
    assert_eq!(
        [Change::Added, Change::Removed, Change::Changed].map(symbol),
        ['+', '-', '~']
    );
}

#[test]
fn an_object_is_framed_with_a_margin_and_a_point_gets_room() {
    let (lo, hi) = frame_box(DVec2::new(0.0, 0.0), DVec2::new(10_000.0, 2_000.0)).unwrap();
    assert_eq!(
        (lo, hi),
        (DVec2::splat(-4000.0), DVec2::new(14_000.0, 6_000.0))
    );
    // A point (a plant, a text): half a meter all round at least.
    let (lo, hi) = frame_box(DVec2::new(100.0, 100.0), DVec2::new(100.0, 100.0)).unwrap();
    assert_eq!((lo, hi), (DVec2::splat(-400.0), DVec2::splat(600.0)));
    // Corners in the wrong order are put right; a box that is not finite is refused.
    assert_eq!(
        frame_box(DVec2::new(10.0, 10.0), DVec2::ZERO),
        frame_box(DVec2::ZERO, DVec2::new(10.0, 10.0))
    );
    assert_eq!(frame_box(DVec2::new(f64::NAN, 0.0), DVec2::ZERO), None);
    assert_eq!(frame_box(DVec2::ZERO, DVec2::new(f64::INFINITY, 0.0)), None);
}
