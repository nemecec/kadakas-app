use super::*;
use serde_json::{Value, json};

fn view() -> MarkupViewpoint {
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

fn stroke(n: usize) -> Mark {
    Mark::new(
        MarkKind::Pen,
        (0..n).map(|i| [i as f64 / n as f64, 0.5]).collect(),
    )
}

#[test]
fn a_plain_markup_is_valid() {
    let mut m = Markup::new(view());
    m.marks.push(stroke(10));
    m.prepare();
    m.validate().unwrap();
    assert_eq!(m.marks[0].id, 1);
}

#[test]
fn points_are_relative_and_scale_with_the_canvas() {
    let rel = rel_of([400.0, 150.0], [800.0, 600.0]);
    assert_eq!(rel, [0.5, 0.25]);
    assert_eq!(canvas_of(rel, [1600.0, 1200.0]), [800.0, 300.0]);
    // A point outside the canvas goes to the edge.
    assert_eq!(rel_of([-5.0, 700.0], [800.0, 600.0]), [0.0, 1.0]);
    // The precision is bounded.
    assert_eq!(round_rel(0.123_456_789), 0.123_46);
}

#[test]
fn coalescing_drops_close_points_and_keeps_the_ends() {
    let canvas = [1000.0, 1000.0];
    // 100 points 0.1 point apart: only the ends and a few more stay.
    let dense: Vec<[f64; 2]> = (0..100).map(|i| [i as f64 * 0.0001, 0.5]).collect();
    let out = coalesce(&dense, canvas, 1.5);
    assert_eq!(out.first(), dense.first());
    assert_eq!(out.last(), dense.last());
    assert!(out.len() < 12, "{}", out.len());
    // Points far apart all stay.
    let sparse = vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]];
    assert_eq!(coalesce(&sparse, canvas, 1.5), sparse);
    // A single point stays.
    assert_eq!(coalesce(&[[0.2, 0.2]], canvas, 1.5).len(), 1);
    assert!(coalesce(&[], canvas, 1.5).is_empty());
}

#[test]
fn the_number_of_points_fits_the_kind() {
    let mut a = Mark::new(MarkKind::Arrow, vec![[0.1, 0.1]]);
    assert!(a.validate().unwrap_err().contains("exactly 2"));
    a.points.push([0.9, 0.9]);
    a.validate().unwrap();
    let mut n = Mark::new(MarkKind::Note, vec![[0.5, 0.5]]);
    assert!(n.validate().unwrap_err().contains("text"));
    n.text = "tree here".into();
    n.validate().unwrap();
    assert!(Mark::new(MarkKind::Pen, vec![]).validate().is_err());
    assert!(
        Mark::new(MarkKind::Rectangle, vec![[0.0, 0.0], [1.5, 1.0]])
            .validate()
            .unwrap_err()
            .contains("0 to 1")
    );
}

#[test]
fn bounds_are_refused_with_a_message() {
    assert!(
        stroke(MAX_POINTS + 1)
            .validate()
            .unwrap_err()
            .contains("points")
    );
    let mut m = Markup::new(view());
    m.marks = (0..=MAX_MARKS).map(|_| stroke(2)).collect();
    assert!(m.validate().unwrap_err().contains("marks"));
    let mut m = Markup::new(view());
    m.marks = (0..21).map(|_| stroke(MAX_POINTS)).collect();
    assert!(m.validate().unwrap_err().contains("in all"));
    let mut k = stroke(2);
    k.width = 0.0;
    assert!(k.validate().unwrap_err().contains("width"));
    let mut m = Markup::new(view());
    m.note = "x".repeat(MAX_NOTE_CHARS + 1);
    assert!(m.validate().is_err());
    let mut m = Markup::new(view());
    m.viewpoint.canvas = [0.0, 10.0];
    assert!(m.validate().unwrap_err().contains("canvas"));
}

#[test]
fn colours_read_as_text_or_as_parts() {
    let c: Rgb = serde_json::from_value(json!("#D62828")).unwrap();
    assert_eq!(c, PALETTE[0]);
    let c: Rgb = serde_json::from_value(json!([1, 2, 3])).unwrap();
    assert_eq!(serde_json::to_value(c).unwrap(), json!("#010203"));
    assert!(serde_json::from_value::<Rgb>(json!("red")).is_err());
}

#[test]
fn the_reader_keeps_what_it_does_not_know() {
    let raw = json!([
        {
            "id": "00000000-0000-4000-8000-000000000001",
            "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 0.1}, "canvas": [800, 600]},
            "marks": [
                {"id": 1, "kind": "pen", "points": [[0.1, 0.1], [0.2, 0.2]], "pressure": [1, 2]},
                {"id": 2, "kind": "lasso", "points": [[0.3, 0.3]], "color": "#112233"}
            ],
            "future": {"a": 1}
        },
        "not a markup",
        {"id": 7}
    ]);
    let ms: Markups = serde_json::from_value(raw).unwrap();
    assert_eq!(ms.len(), 1);
    assert_eq!(ms.unreadable_count(), 2);
    let m = ms.iter().next().unwrap();
    assert_eq!(m.marks[1].kind, MarkKind::Other("lasso".into()));
    assert!(m.marks[1].kind.name() == "lasso");
    assert!(m.extra.contains_key("future"));
    assert!(m.marks[0].extra.contains_key("pressure"));
    // Written again, nothing is lost.
    let back = serde_json::to_value(&ms).unwrap();
    assert_eq!(back[0]["future"], json!({"a": 1}));
    assert_eq!(back[0]["marks"][0]["pressure"], json!([1, 2]));
    assert_eq!(
        back[0]["marks"][1],
        json!({"id": 2, "kind": "lasso", "points": [[0.3, 0.3]], "color": "#112233"}),
        "a mark of an unknown kind is written as it was, with nothing added"
    );
    assert_eq!(back[1], json!("not a markup"));
    assert_eq!(back[2], json!({"id": 7}));
    let again: Markups = serde_json::from_value(back).unwrap();
    assert_eq!(again, ms);
}

#[test]
fn repair_gives_unique_ids_and_never_cuts() {
    let one = json!({
        "id": "00000000-0000-4000-8000-000000000001",
        "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 0.1}, "canvas": [800, 600]},
        "marks": [
            {"id": 1, "kind": "pen", "points": [[0.1, 0.1]]},
            {"id": 1, "kind": "pen", "points": [[0.2, 0.2]]},
            {"kind": "pen", "points": [[0.3, 0.3]]}
        ]
    });
    let mut ms: Markups = serde_json::from_value(json!([one.clone(), one])).unwrap();
    let notes = ms.repair();
    assert!(notes.iter().any(|n| n.contains("new IDs")), "{notes:?}");
    let ids: Vec<_> = ms.iter().map(|m| m.id).collect();
    assert_ne!(ids[0], ids[1]);
    for m in ms.iter() {
        let marks: std::collections::HashSet<u32> = m.marks.iter().map(|k| k.id).collect();
        assert_eq!(marks.len(), 3);
        assert!(!marks.contains(&0));
    }
    assert_eq!(ms.len(), 2);
}

#[test]
fn closed_strokes_and_shapes_enclose_an_area() {
    let canvas = [1000.0, 1000.0];
    let ring: Vec<[f64; 2]> = (0..=20)
        .map(|i| {
            let a = i as f64 / 20.0 * std::f64::consts::TAU;
            [0.5 + 0.2 * a.cos(), 0.5 + 0.2 * a.sin()]
        })
        .collect();
    assert!(Mark::new(MarkKind::Pen, ring.clone()).is_closed(canvas));
    assert!(!Mark::new(MarkKind::Pen, ring[..10].to_vec()).is_closed(canvas));
    assert!(Mark::new(MarkKind::Rectangle, vec![[0.1, 0.1], [0.2, 0.2]]).is_closed(canvas));
    assert!(!Mark::new(MarkKind::Arrow, vec![[0.1, 0.1], [0.2, 0.2]]).is_closed(canvas));
}

#[test]
fn a_patch_changes_what_it_names() {
    let mut m = Markup::new(view());
    m.name = Some("a".into());
    let p: MarkupPatch = serde_json::from_value(json!({"note": "n"})).unwrap();
    let n = p.apply(&m);
    assert_eq!(n.note, "n");
    assert_eq!(n.name.as_deref(), Some("a"));
    let id = IssueId::from_num(5);
    m.issue = Some(id);
    let p: MarkupPatch = serde_json::from_value(json!({"issue": null})).unwrap();
    assert_eq!(p.apply(&m).issue, None);
    let p: MarkupPatch = serde_json::from_value(json!({})).unwrap();
    assert_eq!(p.apply(&m), m);
}

#[test]
fn marks_give_primitives_that_scale_with_the_canvas() {
    use draw::*;
    let mut m = Markup::new(view());
    let mut arrow = Mark::new(MarkKind::Arrow, vec![[0.1, 0.1], [0.5, 0.5]]);
    arrow.width = 4.0;
    m.marks.push(arrow);
    m.marks.push(Mark::new(MarkKind::Pen, vec![[0.5, 0.5]]));
    m.marks
        .push(Mark::new(MarkKind::Rectangle, vec![[0.1, 0.1], [0.2, 0.3]]));
    m.marks
        .push(Mark::new(MarkKind::Ellipse, vec![[0.1, 0.1], [0.2, 0.3]]));
    let mut hi = Mark::new(MarkKind::Highlighter, vec![[0.1, 0.1], [0.3, 0.1]]);
    hi.width = 5.0;
    m.marks.push(hi);
    m.marks
        .push(Mark::new(MarkKind::Other("lasso".into()), vec![[0.1, 0.1]]));
    let small = markup_prims(&m, [800.0, 600.0]);
    let big = markup_prims(&m, [1600.0, 1200.0]);
    assert_eq!(small.len(), big.len());
    // arrow: shaft + head, dot, rectangle, ellipse, highlighter; the unknown kind gives none.
    assert_eq!(small.len(), 6);
    match (&small[0], &big[0]) {
        (
            Prim::Line {
                points: a,
                width: wa,
                ..
            },
            Prim::Line {
                points: b,
                width: wb,
                ..
            },
        ) => {
            assert!((b[1][0] - 2.0 * a[1][0]).abs() < 1e-9, "points scale");
            assert!((wb - 2.0 * wa).abs() < 1e-9, "the width scales");
        }
        _ => panic!("a shaft first"),
    }
    match small.last().unwrap() {
        Prim::Line { alpha, width, .. } => {
            assert_eq!(*alpha, HIGHLIGHT_ALPHA);
            assert_eq!(*width, 5.0 * HIGHLIGHT_FACTOR);
        }
        _ => panic!("a highlighter line"),
    }
}

#[test]
fn a_bad_value_of_one_mark_keeps_that_mark_and_not_the_markup() {
    let raw = json!([{
        "id": "00000000-0000-4000-8000-000000000001",
        "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 0.1}, "canvas": [800, 600]},
        "marks": [
            {"id": 1, "kind": "pen", "color": "red", "points": [[0.1, 0.1]]},
            {"id": 2, "kind": "pen", "points": [[0.2, 0.2], [0.3, "x"]]},
            {"id": 3, "kind": "pen", "points": [[0.4, 0.4]], "width": -5},
            {"id": 4, "kind": "pen", "points": [[1e300, 0.4]]},
            {"id": 5, "kind": "note", "points": [[0.5, 0.5]], "text": "x".repeat(100_000)},
            {"id": 6, "kind": "pen", "points": [[0.6, 0.6]], "color": [1, 2, 3]}
        ]
    }]);
    let mut ms: Markups = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!((ms.len(), ms.unreadable_count()), (1, 0));
    let m = ms.iter().next().unwrap();
    assert!(
        m.marks[0].is_raw() && m.marks[1].is_raw(),
        "bad colour and bad point"
    );
    assert!(!m.marks[2].is_raw() && !m.marks[2].drawable(), "width -5");
    assert!(!m.marks[3].drawable() && !m.marks[4].drawable());
    assert!(
        m.marks[5].drawable(),
        "[1, 2, 3] is a colour that is understood"
    );
    let notes = ms.repair();
    assert!(notes.iter().any(|n| n.contains("5 mark(s)")), "{notes:?}");
    // Nothing is cut or changed.
    let back = serde_json::to_value(&ms).unwrap();
    assert_eq!(back[0]["marks"][0], raw[0]["marks"][0]);
    assert_eq!(back[0]["marks"][1], raw[0]["marks"][1]);
    assert_eq!(back[0]["marks"][2]["width"], json!(-5.0));
    assert_eq!(back[0]["marks"][4]["text"].as_str().unwrap().len(), 100_000);
    // Nothing is drawn from the marks that are not drawable.
    let m = ms.iter().next().unwrap();
    let prims = draw::markup_prims(m, [800.0, 600.0]);
    assert_eq!(prims.len(), 1, "only the mark 6");
}

#[test]
fn a_hand_edited_file_round_trips_and_keeps_the_order_of_entries() {
    let text = r#"[
        "first, not a markup",
        {"id": "00000000-0000-4000-8000-000000000001",
         "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 0.1}, "canvas": [800, 600]},
         "marks": [{"id": 1, "kind": "cloud", "points": [[0.1, 0.1]], "future": {"a": [1, 2.50]}}],
         "from_the_future": 12345678901234567890},
        {"id": 7},
        {"id": "00000000-0000-4000-8000-000000000002",
         "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 0.1}, "canvas": [800, 600]}}
    ]"#;
    let ms: Markups = serde_json::from_str(text).unwrap();
    assert_eq!((ms.len(), ms.unreadable_count()), (2, 2));
    let back = serde_json::to_value(&ms).unwrap();
    let want: Value = serde_json::from_str(text).unwrap();
    assert_eq!(back[0], want[0]);
    assert_eq!(back[2], want[2]);
    assert_eq!(back[1]["from_the_future"], want[1]["from_the_future"]);
    assert_eq!(back[1]["marks"][0], want[1]["marks"][0]);
    assert_eq!(back[3]["id"], want[3]["id"]);
    // The second write is the same as the first.
    let again: Markups = serde_json::from_value(back.clone()).unwrap();
    assert_eq!(serde_json::to_value(&again).unwrap(), back);
}

#[test]
fn a_rename_works_when_a_mark_is_out_of_range() {
    use crate::command::{Command, Editor};
    let raw = json!([{
        "id": "00000000-0000-4000-8000-000000000001",
        "viewpoint": {"view": {"kind": "plan", "center": [0, 0], "zoom": 0.1}, "canvas": [800, 600]},
        "marks": [{"id": 1, "kind": "pen", "points": [[1e300, 0.4]]}]
    }]);
    let mut doc = crate::model::Document::default();
    doc.markups = serde_json::from_value(raw).unwrap();
    let id = doc.markups.iter().next().unwrap().id;
    let mut ed = Editor::new(doc);
    let patch: MarkupPatch = serde_json::from_value(json!({"name": "Renamed"})).unwrap();
    ed.execute(&Command::ModifyMarkup {
        id,
        patch: Box::new(patch),
    })
    .unwrap();
    assert_eq!(
        ed.doc().markups.get(id).unwrap().name.as_deref(),
        Some("Renamed")
    );
}

#[test]
fn colours_that_do_not_read_say_so() {
    let e = serde_json::from_value::<Rgb>(json!([1, 2, 300]))
        .unwrap_err()
        .to_string();
    assert!(e.contains("colour"), "{e}");
}
