use super::*;
use crate::paving::fake::Fake;
use crate::paving::lot;
use kadakas_core::command::{Command, Editor};
use kadakas_core::layout::plan::{Plan, SplitLine};
use serde_json::json;

fn vertical(x: f64) -> Vec<Vertex> {
    curve::corners(&[DVec2::new(x, -500.0), DVec2::new(x, 3500.0)])
}

/// The lot with one split line at x = 2000.
fn split_lot() -> (Editor, ObjectId) {
    let (mut editor, id) = lot::new();
    let plan = Plan {
        splits: vec![SplitLine {
            source: None,
            points: vertical(2000.0),
        }],
        ..Default::default()
    };
    editor
        .execute(&Command::SetData {
            id,
            set: [(plan::KEY.to_string(), serde_json::to_value(&plan).unwrap())]
                .into_iter()
                .collect(),
            remove: Vec::new(),
        })
        .unwrap();
    (editor, id)
}

#[test]
fn a_draft_collects_clicks_into_lines_and_takes_them_back() {
    let (editor, id) = lot::new();
    let doc = editor.doc();
    let mut d = SplitDraft::of(doc, id).unwrap();
    assert!(d.is_empty() && d.follow);
    d.click(DVec2::new(1000.0, 0.0));
    d.click(DVec2::new(1000.0, 0.0));
    assert_eq!(d.open.len(), 1, "a click on the same place adds nothing");
    assert!(!d.finish_line(), "one point is no line");
    assert_eq!(d.open.len(), 1, "and the point stays");
    d.click(DVec2::new(1000.0, 3000.0));
    d.click(DVec2::new(2000.0, 3000.0));
    assert!(d.back());
    assert_eq!(d.polylines(doc).len(), 1, "the open line is drawn");
    assert!(d.finish_line());
    assert!(d.open.is_empty() && d.lines.len() == 1);
    assert_eq!(d.polylines(doc).len(), 1);
    let p = d.params();
    assert_eq!(p["id"], json!(id));
    assert_eq!(p["lines"].as_array().unwrap().len(), 1);
    assert_eq!(p["zone_units"], "cut");
    assert_eq!(p["follow"], true);
    d.remove(0);
    d.remove(7);
    assert!(d.is_empty());
    assert!(!d.back());
}

#[test]
fn drawn_lines_are_picked_by_id_and_a_hardscape_is_not_one() {
    let (mut editor, id) = lot::new();
    let line = editor
        .execute(&Command::AddObject {
            shape: Shape::Polyline {
                points: vertical(1000.0),
                closed: false,
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap()
        .created[0];
    let line = ObjectId(line);
    let doc = editor.doc();
    let mut d = SplitDraft::of(doc, id).unwrap();
    assert_eq!(d.add_objects(doc, &[line, id, line]), 1);
    assert_eq!(d.lines, vec![DraftLine::Object(line)]);
    assert_eq!(d.params()["lines"], json!([line]));
    assert_eq!(d.polylines(doc)[0].len(), 2);
    assert!(SplitDraft::of(doc, line).is_none());
}

#[test]
fn a_draft_starts_with_the_split_lines_the_hardscape_has() {
    let (editor, id) = split_lot();
    let d = SplitDraft::of(editor.doc(), id).unwrap();
    assert_eq!(d.lines, vec![DraftLine::Path(vertical(2000.0))]);
}

#[test]
fn the_zones_are_the_ones_of_the_report_and_add_up_to_the_area() {
    let (editor, id) = split_lot();
    let zones = zone_shapes(editor.doc(), id, 100_000).unwrap();
    assert_eq!(zones.len(), 2);
    let total: f64 = zones.iter().map(|z| z.area_m2).sum();
    assert!((total - 12.0).abs() < 1e-6, "{total}");
    assert!(zones.iter().all(|z| !z.shapes.is_empty() && !z.has_setting));
    // No split: no zones.
    let (plain, pid) = lot::new();
    assert!(zone_shapes(plain.doc(), pid, 100_000).unwrap().is_empty());
    // A missing object is an error.
    assert!(zone_shapes(plain.doc(), ObjectId(kadakas_core::uid::Uid::default()), 10).is_err());
}

#[test]
fn zone_settings_are_read_from_the_plan_and_the_edit_says_only_what_changed() {
    let (editor, id) = split_lot();
    let zones = zone_shapes(editor.doc(), id, 100_000).unwrap();
    let doc = editor.doc();
    let o = doc.object(id).unwrap();
    let base = ZoneSettings::read(doc, o, &zones[1]);
    // The zone continues the pattern of the field: its numbers are those of the style.
    assert_eq!(base.pattern, PatternChoice::Field);
    assert_eq!(
        (base.unit_length, base.unit_width, base.gap),
        (200.0, 100.0, 5.0)
    );
    let mut e = ZoneEdit::new(id, 1, base.clone());
    assert!(!e.changed() && e.params().is_none());
    assert!(e.follows(id, 1, &base) && !e.follows(id, 0, &base));
    e.settings.name = " Drive ".into();
    e.settings.pattern = PatternChoice::Bond(Bond::Herringbone);
    e.settings.unit_length = 300.0;
    e.settings.angle = Some(45.0);
    e.settings.origin = Some(DVec2::new(2000.0, 0.0));
    assert!(e.changed());
    let p = e.params().unwrap();
    assert_eq!(p["id"], json!(id));
    assert_eq!(p["zone"], 1);
    assert_eq!(p["name"], "Drive");
    assert_eq!(p["pattern"]["bond"], "herringbone");
    assert_eq!(p["pattern"]["unit_length"], 300.0);
    assert_eq!(p["angle"], 45.0);
    assert_eq!(p["origin"], json!([2000.0, 0.0]));
    assert!(p.get("clear").is_none());
    // The edit does not follow a zone that the document changed.
    let mut other = base.clone();
    other.name = "x".into();
    assert!(!e.follows(id, 1, &other));
}

#[test]
fn clearing_a_setting_is_a_clear_and_an_alignment_replaces_the_other() {
    let (editor, id) = split_lot();
    let zones = zone_shapes(editor.doc(), id, 100_000).unwrap();
    let mut base = ZoneSettings::read(editor.doc(), editor.doc().object(id).unwrap(), &zones[0]);
    base.origin = Some(DVec2::new(1.0, 2.0));
    base.angle = Some(10.0);
    base.align_to_split = Some(0);
    let mut e = ZoneEdit::new(id, 0, base);
    e.settings.pattern = PatternChoice::Field;
    e.settings.align_edge(2);
    let p = e.params().unwrap();
    assert_eq!(p["align_to_edge"], 2);
    let clear: Vec<&str> = p["clear"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert!(clear.contains(&"origin") && clear.contains(&"angle"));
    assert!(!clear.contains(&"align_to_split"), "the call replaces it");
    // A pattern the API gave stays when nothing about it changed.
    let mut kept = ZoneSettings::read(editor.doc(), editor.doc().object(id).unwrap(), &zones[0]);
    kept.pattern = PatternChoice::Kept;
    let mut e = ZoneEdit::new(id, 0, kept);
    e.settings.gap = 9.0;
    assert!(e.params().is_none());
    e.settings.align_split(1);
    assert_eq!(e.params().unwrap()["align_to_split"], 1);
}

#[test]
fn storing_an_edit_is_one_api_call_and_nothing_without_a_change() {
    let (editor, id) = split_lot();
    let zones = zone_shapes(editor.doc(), id, 100_000).unwrap();
    let base = ZoneSettings::read(editor.doc(), editor.doc().object(id).unwrap(), &zones[0]);
    let mut be = Fake::new().reply(json!({}));
    be.doc = editor.doc().clone();
    let mut e = ZoneEdit::new(id, 0, base);
    store_zone(&mut be, &e).unwrap();
    assert!(be.calls.is_empty());
    e.settings.angle = Some(30.0);
    store_zone(&mut be, &e).unwrap();
    assert_eq!(be.calls.len(), 1);
    assert_eq!(be.calls[0].0, "hardscape.zone_set");
}

#[test]
fn the_report_of_a_split_is_read_from_its_reply() {
    let r = SplitReport::parse(&json!({
        "zones": [{}, {}],
        "area_m2": 12.0,
        "zones_area_m2": 12.0,
        "zone_edge_cuts": {"whole_units_lost": 3, "extra_pieces": 4},
        "notes": ["a line stops short"],
    }));
    assert_eq!((r.zones, r.area_m2, r.zones_area_m2), (2, 12.0, 12.0));
    assert_eq!(r.edge_cuts, Some((3, 4)));
    assert_eq!(r.notes, vec!["a line stops short".to_string()]);
    let none = SplitReport::parse(&json!({"zones": [], "zone_edge_cuts": null}));
    assert_eq!(none.edge_cuts, None);
}

#[test]
fn the_dialog_refuses_a_plain_object_and_previews_and_stores_through_the_api() {
    let (editor, id) = lot::new();
    let mut be = Fake::new();
    be.doc = editor.doc().clone();
    // A missing object and a hardscape without a pattern are refused.
    assert!(SplitDialog::open(&be, ObjectId(kadakas_core::uid::Uid::default())).is_err());
    let mut d = SplitDialog::open(&be, id).unwrap();
    assert!(d.zones.as_ref().unwrap().is_empty());
    assert!(d.report.is_none());
    assert!(d.commit(&mut be).is_err(), "no line to store");
    // A line asks for a preview of hardscape.split, once.
    d.draft.click(DVec2::new(2000.0, -500.0));
    d.draft.click(DVec2::new(2000.0, 3500.0));
    assert!(d.draft.finish_line());
    be.previews.borrow_mut().push_back(Ok(
        json!({"zones": [{}, {}], "area_m2": 12.0, "zones_area_m2": 12.0, "notes": []}),
    ));
    d.refresh(&be);
    d.refresh(&be);
    assert_eq!(be.previewed.borrow().len(), 1);
    assert_eq!(be.previewed.borrow()[0].0, "hardscape.split");
    assert_eq!(d.report.as_ref().unwrap().zones, 2);
    // The copy of the fake has no plan, so the zones are empty; a failing preview is shown.
    be.previews.borrow_mut().push_back(Err("boom".into()));
    d.draft.tolerance = 2.0;
    d.refresh(&be);
    assert_eq!(d.zones.as_ref().unwrap_err(), "boom");
    // Store: the same call, and the reply is read.
    be.replies.push_back(Ok(
        json!({"zones": [{}, {}], "area_m2": 12.0, "zones_area_m2": 12.0}),
    ));
    let r = d.commit(&mut be).unwrap();
    assert_eq!(r.zones, 2);
    assert_eq!(be.calls[0].0, "hardscape.split");
    assert_eq!(be.calls[0].1["tolerance"], 2.0);
}

#[test]
fn an_edit_is_stored_only_when_done_fresh_unlocked_and_changed() {
    let (editor, id) = split_lot();
    let zones = zone_shapes(editor.doc(), id, 100_000).unwrap();
    let base = ZoneSettings::read(editor.doc(), editor.doc().object(id).unwrap(), &zones[0]);
    let mut e = ZoneEdit::new(id, 0, base);
    assert_eq!(e.step(true, true, false), EditStep::Keep, "nothing changed");
    e.settings.angle = Some(12.0);
    assert_eq!(e.step(false, true, false), EditStep::Keep, "still typing");
    assert_eq!(e.step(true, false, false), EditStep::Keep, "old zones");
    assert_eq!(
        e.step(true, true, true),
        EditStep::Keep,
        "locked: the values stay"
    );
    assert_eq!(e.step(true, true, false), EditStep::Store);
}
