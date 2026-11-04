//! Markup commands (REV-11): undo and redo, change sets, bounds, save and load, and the fact that
//! a markup is no drawing object.

use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::issue::ViewSpec;
use kadakas_core::markup::*;
use kadakas_core::model::Document;
use std::collections::BTreeMap;

fn editor() -> Editor {
    Editor::with_sequential_ids()
}

fn viewpoint() -> MarkupViewpoint {
    MarkupViewpoint {
        view: ViewSpec::Plan {
            center: [0.0, 0.0],
            zoom: 0.1,
        },
        fov: None,
        canvas: [800.0, 600.0],
        layers: BTreeMap::new(),
        classes: BTreeMap::new(),
    }
}

fn add_markup(ed: &mut Editor) -> MarkupId {
    let mut m = Markup::new(viewpoint());
    m.author = "Ann".into();
    MarkupId(
        ed.execute(&Command::AddMarkup {
            markup: Box::new(m),
        })
        .unwrap()
        .created[0],
    )
}

fn add_mark(ed: &mut Editor, id: MarkupId, kind: MarkKind, points: Vec<[f64; 2]>) -> u32 {
    let out = ed
        .execute(&Command::AddMark {
            markup: id,
            mark: Box::new(Mark::new(kind, points)),
        })
        .unwrap();
    out.report.unwrap()["mark"].as_u64().unwrap() as u32
}

#[test]
fn one_drawn_mark_is_one_undo_step() {
    let mut ed = editor();
    let id = add_markup(&mut ed);
    let depth = ed.undo_depth();
    let a = add_mark(&mut ed, id, MarkKind::Pen, vec![[0.1, 0.1], [0.2, 0.2]]);
    let b = add_mark(&mut ed, id, MarkKind::Arrow, vec![[0.1, 0.1], [0.5, 0.5]]);
    assert_eq!((a, b), (1, 2));
    assert_eq!(ed.undo_depth(), depth + 2);
    assert_eq!(ed.doc().markups.get(id).unwrap().marks.len(), 2);
    assert!(ed.undo());
    assert_eq!(ed.doc().markups.get(id).unwrap().marks.len(), 1);
    assert!(ed.redo());
    assert_eq!(ed.doc().markups.get(id).unwrap().marks.len(), 2);
    assert!(ed.undo() && ed.undo() && ed.undo());
    assert!(ed.doc().markups.is_empty());
}

#[test]
fn delete_recolour_clear_and_modify_are_undoable() {
    let mut ed = editor();
    let id = add_markup(&mut ed);
    let a = add_mark(&mut ed, id, MarkKind::Pen, vec![[0.1, 0.1], [0.2, 0.2]]);
    let b = add_mark(&mut ed, id, MarkKind::Pen, vec![[0.3, 0.3]]);
    ed.execute(&Command::SetMarkStyle {
        markup: id,
        mark: a,
        color: Some(PALETTE[4]),
        width: Some(9.0),
    })
    .unwrap();
    let m = ed.doc().markups.get(id).unwrap().mark(a).unwrap().clone();
    assert_eq!((m.color, m.width), (PALETTE[4], 9.0));
    // A change to the same value makes no step.
    let depth = ed.undo_depth();
    ed.execute(&Command::SetMarkStyle {
        markup: id,
        mark: a,
        color: Some(PALETTE[4]),
        width: None,
    })
    .unwrap();
    assert_eq!(ed.undo_depth(), depth);
    ed.execute(&Command::DeleteMark {
        markup: id,
        mark: b,
    })
    .unwrap();
    assert!(ed.doc().markups.get(id).unwrap().mark(b).is_none());
    ed.execute(&Command::ClearMarkup { markup: id }).unwrap();
    assert!(ed.doc().markups.get(id).unwrap().marks.is_empty());
    assert!(ed.undo());
    assert_eq!(ed.doc().markups.get(id).unwrap().marks.len(), 1);
    let patch: MarkupPatch = serde_json::from_value(serde_json::json!({"note": "a tree"})).unwrap();
    ed.execute(&Command::ModifyMarkup {
        id,
        patch: Box::new(patch),
    })
    .unwrap();
    assert_eq!(ed.doc().markups.get(id).unwrap().note, "a tree");
    ed.execute(&Command::DeleteMarkup { id }).unwrap();
    assert!(ed.doc().markups.is_empty());
    assert!(ed.undo());
    assert_eq!(ed.doc().markups.len(), 1);
    // Unknown ones are refused.
    let err = ed
        .execute(&Command::DeleteMark {
            markup: id,
            mark: 99,
        })
        .unwrap_err();
    assert!(matches!(err, CommandError::NoMark(..)));
    let err = ed
        .execute(&Command::ClearMarkup {
            markup: MarkupId::from_num(77),
        })
        .unwrap_err();
    assert!(matches!(err, CommandError::NoMarkup(_)));
}

#[test]
fn the_change_set_says_markups_and_the_objects_stay() {
    let mut ed = editor();
    let id = add_markup(&mut ed);
    let cs = ed.change_log().last().unwrap().clone();
    assert!(cs.document.markups);
    assert!(cs.touches("markups"));
    assert!(!cs.touches("objects"));
    assert!(!cs.touches("document"));
    assert!(cs.only(&["objects"]).is_none());
    assert!(cs.only(&["markups"]).is_some());
    add_mark(&mut ed, id, MarkKind::Pen, vec![[0.1, 0.1]]);
    ed.undo();
    assert!(ed.change_log().last().unwrap().document.markups);
    // A markup is no object: the object list stays empty, and it is in no count of objects.
    assert!(ed.doc().objects.is_empty());
}

#[test]
fn bounds_are_refused_and_nothing_changes() {
    let mut ed = editor();
    let id = add_markup(&mut ed);
    for _ in 0..MAX_MARKS {
        add_mark(&mut ed, id, MarkKind::Pen, vec![[0.5, 0.5]]);
    }
    let before = ed.doc().clone();
    let err = ed
        .execute(&Command::AddMark {
            markup: id,
            mark: Box::new(Mark::new(MarkKind::Pen, vec![[0.5, 0.5]])),
        })
        .unwrap_err();
    assert!(err.to_string().contains("at most"), "{err}");
    assert_eq!(ed.doc(), &before);
    // A bad mark is refused too.
    let id2 = add_markup(&mut ed);
    let err = ed
        .execute(&Command::AddMark {
            markup: id2,
            mark: Box::new(Mark::new(MarkKind::Arrow, vec![[0.5, 0.5]])),
        })
        .unwrap_err();
    assert!(err.to_string().contains("exactly 2"), "{err}");
    // Too many markups.
    let mut ed = editor();
    for _ in 0..MAX_MARKUPS {
        add_markup(&mut ed);
    }
    let err = ed
        .execute(&Command::AddMarkup {
            markup: Box::new(Markup::new(viewpoint())),
        })
        .unwrap_err();
    assert!(err.to_string().contains("at most"), "{err}");
}

#[test]
fn a_markup_that_is_added_with_marks_gets_numbers_and_rounded_points() {
    let mut ed = editor();
    let mut m = Markup::new(viewpoint());
    m.marks
        .push(Mark::new(MarkKind::Pen, vec![[0.123_456_789, 0.5]]));
    m.marks.push(Mark::new(MarkKind::Pen, vec![[0.3, 0.3]]));
    let id = MarkupId(
        ed.execute(&Command::AddMarkup {
            markup: Box::new(m),
        })
        .unwrap()
        .created[0],
    );
    let got = ed.doc().markups.get(id).unwrap();
    assert_eq!(got.marks[0].id, 1);
    assert_eq!(got.marks[1].id, 2);
    assert_eq!(got.marks[0].points[0][0], 0.123_46);
}

#[test]
fn markups_are_saved_and_loaded_and_older_files_have_none() {
    let mut ed = editor();
    let id = add_markup(&mut ed);
    add_mark(&mut ed, id, MarkKind::Pen, vec![[0.1, 0.1], [0.2, 0.2]]);
    let text = serde_json::to_string(ed.doc()).unwrap();
    let back = Document::from_json(&text).unwrap();
    assert_eq!(&back, ed.doc());
    assert_eq!(back.markups.get(id).unwrap().marks.len(), 1);
    let plain = serde_json::to_value(Document::default()).unwrap();
    assert!(plain.get("markups").is_none(), "an older file shape stays");
    // A file with an entry that this version cannot read keeps it and says so.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["markups"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"from": "the future"}));
    let loaded = Document::from_json(&v.to_string()).unwrap();
    assert_eq!(loaded.markups.len(), 1);
    assert!(
        loaded.load_notes().iter().any(|n| n.contains("markup")),
        "{:?}",
        loaded.load_notes()
    );
    let again = serde_json::to_value(&loaded).unwrap();
    assert_eq!(
        again["markups"][1],
        serde_json::json!({"from": "the future"})
    );
}

#[test]
fn a_squashed_run_keeps_the_markups() {
    let mut ed = editor();
    let mark = ed.begin_run();
    let id = add_markup(&mut ed);
    add_mark(&mut ed, id, MarkKind::Pen, vec![[0.1, 0.1]]);
    let after = ed.doc().clone();
    ed.squash_run(mark, "script");
    assert_eq!(ed.doc(), &after);
    assert!(ed.undo());
    assert!(ed.doc().markups.is_empty());
    assert!(ed.redo());
    assert_eq!(ed.doc().markups.get(id).unwrap().marks.len(), 1);
}

/// The size of the data: the numbers in `docs/modeler.md` come from this run.
#[test]
#[ignore = "measurement: run with --ignored --nocapture"]
fn measure_the_size_and_the_time_of_markup_data() {
    use std::time::Instant;
    for (markups, marks, points) in [
        (100usize, 20usize, 100usize),
        (2_000, 20, 10),
        (50, 200, 100),
    ] {
        let mut ed = editor();
        let mut doc = ed.doc().clone();
        let mut ms = Vec::new();
        for _ in 0..markups {
            let mut m = Markup::new(viewpoint());
            for k in 0..marks {
                let pts = (0..points)
                    .map(|i| {
                        let t = i as f64 / points as f64;
                        [
                            round_rel(t),
                            round_rel(0.5 + 0.3 * (t * 9.0 + k as f64).sin()),
                        ]
                    })
                    .collect();
                m.marks.push(Mark::new(MarkKind::Pen, pts));
            }
            m.prepare();
            ms.push(serde_json::to_value(&m).unwrap());
        }
        doc.markups = serde_json::from_value(serde_json::Value::Array(ms)).unwrap();
        let t = Instant::now();
        let text = serde_json::to_string(&doc).unwrap();
        let write = t.elapsed();
        let t = Instant::now();
        let back = Document::from_json(&text).unwrap();
        let read = t.elapsed();
        println!(
            "{markups} markups x {marks} marks x {points} points = {} points: {:.2} MiB JSON, write {write:?}, read {read:?}, {:.1} bytes per point",
            back.markups.point_count(),
            text.len() as f64 / 1048576.0,
            text.len() as f64 / back.markups.point_count() as f64
        );
        ed = Editor::new(back);
        let id = ed.doc().markups.iter().next().unwrap().id;
        let t = Instant::now();
        ed.execute(&Command::AddMark {
            markup: id,
            mark: Box::new(Mark::new(MarkKind::Pen, vec![[0.5, 0.5]])),
        })
        .ok();
        println!("  one stroke added: {:?}", t.elapsed());
    }
}
