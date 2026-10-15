use glam::DVec2;
use kadakas_core::command::Editor;
use kadakas_core::mesh;
use kadakas_core::model::{Document, FIRST_LAYER, NONE_CLASS, Object, ObjectId, Shape};
use kadakas_core::svg_overlay::{SvgLook, SvgOverlay};
use serde_json::json;

use super::*;
use crate::testutil::{circle, cmd, editor};

fn rect_object(n: u64, x: f64) -> Object {
    Object::new(
        ObjectId(Uid::from_num(n)),
        FIRST_LAYER,
        NONE_CLASS,
        Shape::Rect {
            min: DVec2::new(x, 0.0),
            max: DVec2::new(x + 10.0, 10.0),
        },
    )
}

fn change(n: u64, status: Status, with_old: bool) -> Change {
    Change {
        id: ObjectId(Uid::from_num(n)),
        status,
        old: with_old.then(|| rect_object(n, 0.0)),
        new: None,
    }
}

/// Two states: `a` is moved, `b` is deleted, `c` is added, `d` stays.
fn two_states() -> (State, State, [ObjectId; 4]) {
    let mut ed = editor();
    let a = circle(&mut ed, 0.0);
    let b = circle(&mut ed, 20.0);
    let d = circle(&mut ed, 40.0);
    let s1 = State::of(ed.doc(), None);
    ed.execute(&cmd(
        json!({"op": "move_objects", "ids": [a], "delta": [1.0, 0.0]}),
    ))
    .unwrap();
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [b]})))
        .unwrap();
    let c = circle(&mut ed, 60.0);
    let s2 = State::of(ed.doc(), Some(&s1));
    (s1, s2, [a, b, c, d])
}

#[test]
fn the_overlay_of_two_states_has_added_removed_and_changed_objects() {
    let (s1, s2, [a, b, c, d]) = two_states();
    let o = CompareOverlay::from_states(&s1, &s2);
    assert_eq!(o.status_of(a), Some(Status::Changed));
    assert_eq!(o.status_of(b), Some(Status::Removed));
    assert_eq!(o.status_of(c), Some(Status::Added));
    assert_eq!(o.status_of(d), None, "an unchanged object has no status");
    assert_eq!(
        o.counts(),
        Counts {
            added: 1,
            removed: 1,
            changed: 1
        }
    );
    assert_eq!(o.total(), 3);
    assert!(o.exact() && o.notes().is_empty() && !o.is_empty());
    // The ghost is the removed object as it was; the previous shape is where `a` was.
    let ghost = o.ghosts().next().unwrap();
    assert_eq!(ghost.id, b);
    assert!(matches!(ghost.shape, Shape::Circle { center, .. } if center.x == 20.0));
    let before = o.previous().next().unwrap();
    assert!(matches!(before.shape, Shape::Circle { center, .. } if center.x == 0.0));
    assert_eq!(o.ghost_count(), 1);
    assert_eq!(o.ids(Status::Added, 10), vec![c]);
    assert_eq!(o.ids(Status::Removed, 0), Vec::<ObjectId>::new());
    assert_eq!(o.statuses().count(), 3);
    assert_eq!(
        o.summary(),
        Summary {
            added: 1,
            removed: 1,
            changed: 1,
            total: 3,
            ghosts_kept: 1,
            ghosts_capped: 0,
            ghosts_not_drawable: 0,
            exact: true
        }
    );
    assert!(o.shapes_of(c).is_none() && o.shapes_of(b).is_some());
    assert!(o.bytes() > 0);
    // The same state on both sides: nothing.
    let same = CompareOverlay::from_states(&s2, &s2);
    assert!(same.is_empty() && same.ghosts().next().is_none());
    assert_eq!(same.counts(), Counts::default());
}

#[test]
fn a_move_in_the_stacking_order_is_a_change_like_in_the_diff() {
    let mut ed = editor();
    let a = circle(&mut ed, 0.0);
    let b = circle(&mut ed, 5.0);
    let s1 = State::of(ed.doc(), None);
    ed.execute(&cmd(json!({"op": "arrange", "ids": [a], "how": "front"})))
        .unwrap();
    let s2 = State::of(ed.doc(), Some(&s1));
    let o = CompareOverlay::from_states(&s1, &s2);
    assert_eq!(o.status_of(a), Some(Status::Changed));
    assert_eq!(o.status_of(b), None);
    // The overlay and the diff agree on the count of changed objects.
    let d = crate::diff::diff(&s1, &s2, &crate::DiffOptions::default());
    assert_eq!(d.counts["objects"].changed, o.counts().changed);
}

#[test]
fn only_objects_count() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let s1 = State::of(ed.doc(), None);
    ed.execute(&cmd(json!({"op": "add_layer", "name": "Plan B"})))
        .unwrap();
    ed.execute(&cmd(json!({"op": "set_project", "name": "Garden"})))
        .unwrap();
    let s2 = State::of(ed.doc(), Some(&s1));
    assert!(CompareOverlay::from_states(&s1, &s2).is_empty());
}

#[test]
fn changes_from_any_source_make_the_same_overlay() {
    let o = CompareOverlay::from_changes([
        change(1, Status::Added, false),
        change(2, Status::Removed, true),
        Change {
            id: ObjectId(Uid::from_num(3)),
            status: Status::Changed,
            old: Some(rect_object(3, 0.0)),
            new: Some(rect_object(3, 50.0)),
        },
    ]);
    assert_eq!(o.counts().total(), 3);
    assert_eq!(o.ghosts().map(|g| g.id).collect::<Vec<_>>().len(), 1);
    let shapes = o.shapes_of(ObjectId(Uid::from_num(3))).unwrap();
    assert!(shapes.old.is_some() && shapes.new.is_some());
    // The new object of an added change is kept when the caller gives it (a revision that is not
    // in the drawing yet), and the old object of an added change is ignored.
    let o = CompareOverlay::from_changes([Change {
        id: ObjectId(Uid::from_num(9)),
        status: Status::Added,
        old: Some(rect_object(9, 0.0)),
        new: Some(rect_object(9, 5.0)),
    }]);
    let s = o.shapes_of(ObjectId(Uid::from_num(9))).unwrap();
    assert!(s.old.is_none() && s.new.is_some());
    assert_eq!(o.ghosts().count(), 0);
}

#[test]
fn the_first_change_of_an_object_counts() {
    let o = CompareOverlay::from_changes([
        change(1, Status::Added, false),
        change(1, Status::Removed, true),
    ]);
    assert_eq!(o.counts().total(), 1);
    assert_eq!(o.status_of(ObjectId(Uid::from_num(1))), Some(Status::Added));
    assert_eq!(o.ghosts().count(), 0);
}

#[test]
fn a_removed_object_without_an_old_object_is_counted_and_has_no_ghost() {
    let o = CompareOverlay::from_changes([change(1, Status::Removed, false)]);
    assert_eq!(o.counts().removed, 1);
    assert_eq!(o.ghost_count(), 0);
    assert!(o.notes().is_empty(), "nothing was left out");
}

#[test]
fn a_diff_of_100k_objects_stays_within_the_caps() {
    let limits = Limits {
        ghosts: 50,
        shapes: 30,
        tracked: 1_000,
    };
    let n = 100_000u64;
    let produced = std::cell::Cell::new(0u64);
    let all = (1..=n).map(|i| {
        produced.set(produced.get() + 1);
        let status = match i % 3 {
            0 => Status::Added,
            1 => Status::Removed,
            _ => Status::Changed,
        };
        change(i, status, status != Status::Added)
    });
    let o = CompareOverlay::from_changes_with(all, limits);
    assert_eq!(produced.get(), n, "the whole input is read, one by one");
    // The counts are exact; the lists and the shapes are bounded.
    assert_eq!(o.counts().total(), n as usize);
    assert_eq!(o.statuses().count(), 1_000);
    assert_eq!(o.untracked(), 99_000);
    assert!(o.ghost_count() <= 50 && o.ghosts().count() <= 50);
    assert!(o.previous().count() <= 30);
    assert!(o.bytes() < 10 * 1024 * 1024);
    let notes = o.notes();
    assert!(
        notes
            .iter()
            .any(|n| matches!(n, Note::GhostsCapped { drawn: 50, .. }))
    );
    assert!(
        notes
            .iter()
            .any(|n| matches!(n, Note::StatusesCapped { untracked: 99_000 }))
    );
    // Ghosts beyond the cap are counted as omitted, and the summary says so.
    let s = o.summary();
    assert_eq!(s.ghosts_kept, 50);
    assert!(s.ghosts_capped > 0 && s.removed > 50);
    // The IDs are counted once, also past the cap of statuses.
    let again = (1..=3000u64)
        .chain(1..=3000)
        .map(|i| change(i, Status::Added, false));
    let o = CompareOverlay::from_changes_with(
        again,
        Limits {
            ghosts: 1,
            shapes: 1,
            tracked: 10,
        },
    );
    assert_eq!(o.counts().added, 3000);
    assert!(o.bytes() > 0 && o.bytes() < 4096);
}

#[test]
fn the_shapes_cap_and_the_state_diff_cap_are_noted() {
    let (s1, s2, [a, ..]) = two_states();
    // No room for the shape of the changed object.
    let o = CompareOverlay::from_states_with(
        &s1,
        &s2,
        Limits {
            ghosts: 0,
            shapes: 0,
            tracked: 100,
        },
        usize::MAX,
    );
    assert_eq!(o.status_of(a), Some(Status::Changed));
    assert_eq!(o.previous().count(), 0);
    assert_eq!(o.omitted_ghosts(), 1);
    let notes = o.notes();
    assert!(notes.contains(&Note::GhostsCapped {
        drawn: 0,
        omitted: 1
    }));
    assert!(notes.contains(&Note::ShapesCapped { omitted: 1 }));
    // The status cap leaves the object counted but without a status by ID.
    let o = CompareOverlay::from_states_with(
        &s1,
        &s2,
        Limits {
            ghosts: 10,
            shapes: 10,
            tracked: 1,
        },
        usize::MAX,
    );
    assert_eq!(o.total(), 3);
    assert_eq!(o.statuses().count(), 1);
    assert_eq!(o.untracked(), 2);
    // The work limit stops the comparison and says so.
    let o = CompareOverlay::from_states_with(&s1, &s2, Limits::default(), 3);
    assert!(!o.exact());
    assert!(o.notes().contains(&Note::Partial));
    assert!(!o.summary().exact);
}

#[test]
fn a_huge_shape_is_not_kept_as_a_ghost() {
    let mut ed = Editor::with_sequential_ids();
    let points: Vec<[f64; 2]> = (0..40_000).map(|i| [f64::from(i), 0.0]).collect();
    let out = ed
        .execute(&cmd(json!({"op": "add_object", "shape":
            {"kind": "polyline", "points": points, "closed": false}})))
        .unwrap();
    let big = ObjectId(out.created[0]);
    let s1 = State::of(ed.doc(), None);
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [big]})))
        .unwrap();
    let s2 = State::of(ed.doc(), Some(&s1));
    let o = CompareOverlay::from_states(&s1, &s2);
    assert_eq!(o.status_of(big), Some(Status::Removed));
    assert_eq!(o.ghost_count(), 0);
    assert_eq!(o.omitted_ghosts(), 0, "not capped: too long");
    assert_eq!(o.unreadable_ghosts(), 1);
    assert!(o.notes().contains(&Note::GhostsNotDrawable { count: 1 }));
    assert_eq!(o.summary().ghosts_not_drawable, 1);
    // The same object given as a change is refused the same way.
    let object = s1
        .things()
        .find(|(t, _)| t.kind == Kind::Object)
        .and_then(|(t, p)| object_of(t.id, p))
        .unwrap();
    let o = CompareOverlay::from_changes([Change {
        id: big,
        status: Status::Removed,
        old: Some(object),
        new: None,
    }]);
    assert_eq!(o.unreadable_ghosts(), 1);
}

#[test]
fn object_of_reads_an_object_and_rejects_nonsense() {
    let (s1, ..) = two_states();
    let (t, p) = s1.things().find(|(t, _)| t.kind == Kind::Object).unwrap();
    assert_eq!(object_of(t.id, p).unwrap().id, ObjectId(t.id));
    let mut bad = p.clone();
    bad.insert("shape".into(), json!({"kind": "circle", "center": "far"}));
    assert!(object_of(t.id, &bad).is_none());
}

fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let lum = |c: [u8; 3]| {
        let f = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn the_colors_are_readable_on_both_papers_and_never_the_only_sign() {
    for palette in Palette::ALL {
        for dark in [false, true] {
            let style = CompareStyle {
                palette,
                dark,
                ..CompareStyle::default()
            };
            let tints: Vec<Tint> = Status::ALL.iter().map(|s| style.tint(*s)).collect();
            for t in &tints {
                assert!(
                    contrast(t.rgb, style.paper()) >= 3.0,
                    "{palette:?} dark {dark}: {:?} is hard to read",
                    t.rgb
                );
            }
            // Three different colors, three different patterns, three different marks.
            for i in 0..3 {
                for j in i + 1..3 {
                    assert_ne!(tints[i].rgb, tints[j].rgb);
                    assert_ne!(tints[i].pattern, tints[j].pattern);
                    assert_ne!(tints[i].symbol, tints[j].symbol);
                }
            }
        }
    }
    assert_eq!(Palette::parse("color_blind"), Some(Palette::ColorBlind));
    assert_eq!(Palette::parse("x"), None);
    assert_eq!(UnchangedMode::parse("hide"), Some(UnchangedMode::Hide));
    assert_eq!(UnchangedMode::parse("x"), None);
    assert_eq!(Status::parse("changed"), Some(Status::Changed));
    assert_eq!(Status::Removed.symbol(), '-');
    // The standard palette is green, red and orange.
    let s = CompareStyle::default();
    let (a, r, c) = (
        s.color(Status::Added),
        s.color(Status::Removed),
        s.color(Status::Changed),
    );
    assert!(a[1] > a[0] && a[1] > a[2]);
    assert!(r[0] > r[1] && r[0] > r[2]);
    assert!(c[0] > c[2] && c[1] > c[2] && c[0] > c[1]);
}

#[test]
fn the_svg_overlay_tints_by_status_and_dims_or_hides_the_rest() {
    let (s1, s2, [a, b, c, d]) = two_states();
    let o = CompareOverlay::from_states(&s1, &s2);
    let doc = s2.build(&Editor::with_sequential_ids().doc().clone()).doc;
    let style = CompareStyle::default();
    let svg = SvgCompare::new(&doc, None, &o, style);
    for (id, name) in [(a, "changed"), (b, "removed"), (c, "added")] {
        let SvgLook::Tint(t) = svg.look(id) else {
            panic!("{name} is tinted")
        };
        assert_eq!(t.status, name);
        assert!(t.symbol.is_some());
    }
    assert_eq!(svg.look(d), SvgLook::Dim(0.25));
    let show = SvgCompare::new(
        &doc,
        None,
        &o,
        CompareStyle {
            unchanged: UnchangedMode::Show,
            ..style
        },
    );
    assert_eq!(show.look(d), SvgLook::Keep);
    let hide = SvgCompare::new(
        &doc,
        None,
        &o,
        CompareStyle {
            unchanged: UnchangedMode::Hide,
            ..style
        },
    );
    assert_eq!(hide.look(d), SvgLook::Hide);
    // One ghost of the removed object, and one of the old shape of the changed object.
    let kinds: Vec<&str> = svg.ghosts().iter().map(|g| g.kind).collect();
    assert_eq!(kinds, vec!["removed", "previous"]);
    assert_eq!(svg.ghosts()[0].tint.status, "removed");
    assert!(svg.ghosts()[1].tint.symbol.is_none());
    assert!(svg.view().is_none());
    let view = (DVec2::ZERO, DVec2::splat(5.0));
    assert_eq!(
        SvgCompare::new(&doc, None, &o, style)
            .with_view(view)
            .view(),
        Some(view)
    );
    // The plan canvas gets the same ghosts.
    let listed = ghosts_in(&doc, &doc, &o, &style);
    assert_eq!(
        listed
            .items
            .iter()
            .map(|(g, k)| (g.id, *k))
            .collect::<Vec<_>>(),
        vec![(b, "removed"), (a, "previous")]
    );
    assert_eq!(listed.report.drawn, 1);
    // The old places go behind the live objects; the report names what became of each ghost.
    assert_eq!(
        svg.ghosts().iter().map(|g| g.below).collect::<Vec<_>>(),
        vec![false, true]
    );
    assert_eq!(svg.report(), listed.report);
    let dark = SvgCompare::new(
        &doc,
        None,
        &o,
        CompareStyle {
            dark: true,
            ..style
        },
    );
    assert!(dark.dark() && !svg.dark());
    // Ghosts can be switched off.
    let none = SvgCompare::new(
        &doc,
        None,
        &o,
        CompareStyle {
            show_ghosts: false,
            show_previous: false,
            ..style
        },
    );
    assert!(none.ghosts().is_empty());
}

#[test]
fn a_ghost_on_a_hidden_layer_is_reported_and_a_picture_is_not_drawable() {
    let ghost = rect_object(7, 0.0);
    let underlay = {
        let mut u = rect_object(8, 0.0);
        u.shape = Shape::Group { name: "g".into() };
        u
    };
    let o = CompareOverlay::from_changes([
        Change {
            id: ghost.id,
            status: Status::Removed,
            old: Some(ghost),
            new: None,
        },
        Change {
            id: underlay.id,
            status: Status::Removed,
            old: Some(underlay),
            new: None,
        },
    ]);
    let mut ed = Editor::with_sequential_ids();
    let found = ghosts_in(ed.doc(), ed.doc(), &o, &CompareStyle::default());
    assert_eq!(found.items.len(), 1);
    assert_eq!(
        found.report,
        GhostReport {
            drawn: 1,
            hidden: 0,
            not_drawable: 1,
            capped: 0
        }
    );
    ed.execute(&cmd(json!({"op": "modify_layer", "id": FIRST_LAYER,
        "visible": false})))
        .unwrap();
    let found = ghosts_in(ed.doc(), ed.doc(), &o, &CompareStyle::default());
    assert!(found.items.is_empty());
    assert_eq!((found.report.hidden, found.report.drawn), (1, 0));
    // The same layer in the old drawing, which has no such layer in the new one, decides.
    let new = Document::default();
    let mut old = Document::default();
    old.layers[0].id = kadakas_core::model::LayerId::from_num(55);
    let mut g = rect_object(9, 0.0);
    g.layer = kadakas_core::model::LayerId::from_num(55);
    let o2 = CompareOverlay::from_changes([Change {
        id: g.id,
        status: Status::Removed,
        old: Some(g),
        new: None,
    }]);
    assert_eq!(
        ghosts_in(&new, &old, &o2, &CompareStyle::default())
            .report
            .drawn,
        1
    );
    old.layers[0].visible = false;
    assert_eq!(
        ghosts_in(&new, &old, &o2, &CompareStyle::default())
            .report
            .hidden,
        1
    );
}

#[test]
fn the_scene_gets_tints_dimmed_unchanged_objects_and_ghosts() {
    let (s1, s2, [a, b, c, d]) = two_states();
    let o = CompareOverlay::from_states(&s1, &s2);
    let doc = s2.build(&Editor::with_sequential_ids().doc().clone()).doc;
    let scene = mesh::build(&doc);
    let style = CompareStyle::default();
    let t = tint_scene(&scene, &doc, &doc, &o, &style, None);
    let (tinted, ghosts) = (t.scene, t.ghosts);
    assert_eq!(t.report.drawn, 1);
    assert_eq!(ghosts, vec![b], "the removed object is a ghost");
    let color_of = |s: &mesh::Scene, id: ObjectId| {
        let t = (0..s.mesh.triangle_count())
            .find(|t| s.mesh.tri_object[*t] == id)
            .unwrap();
        s.mesh.colors[s.mesh.indices[t * 3] as usize]
    };
    let [r, g, bl] = style.color(Status::Added);
    assert_eq!(color_of(&tinted, c), [r, g, bl, 255]);
    let [r, g, bl] = style.color(Status::Changed);
    assert_eq!(color_of(&tinted, a), [r, g, bl, 255]);
    let [r, g, bl] = style.color(Status::Removed);
    assert_eq!(color_of(&tinted, b), [r, g, bl, 255]);
    // An unchanged object is faded toward the white paper.
    let before = color_of(&scene, d);
    let after = color_of(&tinted, d);
    assert!(
        after[0] >= before[0] && after[1] >= before[1] && after[2] > before[2] || before == after
    );
    assert_ne!(before, after);
    // The unchanged object can stay as it is, or go.
    let shown = tint_scene(
        &scene,
        &doc,
        &doc,
        &o,
        &CompareStyle {
            unchanged: UnchangedMode::Show,
            ..style
        },
        None,
    )
    .scene;
    assert_eq!(color_of(&shown, d), before);
    let hidden = tint_scene(
        &scene,
        &doc,
        &doc,
        &o,
        &CompareStyle {
            unchanged: UnchangedMode::Hide,
            ..style
        },
        None,
    )
    .scene;
    assert!(hidden.object_bounds(d).is_none());
    assert!(hidden.object_bounds(a).is_some() && hidden.object_bounds(b).is_some());
    // Without ghosts the removed object is not in the scene.
    let plain = tint_scene(
        &scene,
        &doc,
        &doc,
        &o,
        &CompareStyle {
            show_ghosts: false,
            ..style
        },
        None,
    );
    assert!(plain.ghosts.is_empty() && plain.scene.object_bounds(b).is_none());
    // A document that is not changed has no ghost document objects.
    let none = CompareOverlay::default();
    assert!(
        ghost_document(&doc, &doc, &none, &style)
            .0
            .objects
            .is_empty()
    );
    let g = ghost_document(&doc, &doc, &o, &style).0;
    assert_eq!(g.objects.len(), 1, "only removed objects, not old shapes");
    assert_eq!(g.layers.len(), doc.layers.len());
}
