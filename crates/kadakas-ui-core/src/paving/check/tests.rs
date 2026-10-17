use super::*;
use crate::paving::fake::Fake;
use crate::paving::lot;
use serde_json::json;

fn reply(id: ObjectId) -> Value {
    json!({
        "objects": [
            {"id": id, "name": "Pavers", "units": 600, "ok": false,
             "counts": {"overlap": 1, "joint_too_narrow": 2},
             "findings": [
                {"kind": "overlap", "group": "field", "at": [100.0, 200.0], "size": 12.5, "message": "two units overlap"},
                {"kind": "joint_too_narrow", "group": "zone 0", "at": [300.0, 400.0], "size": 3.5, "message": "narrow"},
                {"kind": "joint_too_narrow", "group": "zone 0", "at": [500.0, 600.0], "size": 3.0, "message": "narrow"},
             ],
             "truncated": [], "coverage": []},
            {"id": id, "name": "Broken", "error": "no pattern"},
        ],
        "totals": {"units": 600, "counts": {"overlap": 1, "joint_too_narrow": 2}, "ok": false, "complete": false, "errors": 1},
    })
}

#[test]
fn the_reply_becomes_rows_with_places() {
    let (_, id) = lot::new();
    let r = CheckResult::parse(&reply(id)).unwrap();
    assert_eq!(r.objects.len(), 2);
    assert_eq!(r.units, 600);
    assert!(!r.ok && !r.complete);
    assert_eq!(r.finding_count(), 3);
    assert_eq!(r.errors(), 1);
    assert_eq!(r.objects[1].error.as_deref(), Some("no pattern"));
    let f: Vec<_> = r.findings().collect();
    assert_eq!(f.len(), 3);
    assert_eq!(f[0].at, DVec2::new(100.0, 200.0));
    assert_eq!(f[0].object, Some(id));
    assert_eq!(f[0].zoom_box(), [-500.0, -400.0, 700.0, 800.0]);
    assert_eq!(f[1].zoom_box(), [-100.0, 0.0, 700.0, 800.0]);
    assert!(!f[0].size_text().is_empty() && !f[1].size_text().is_empty());
    assert!(
        !FindingRow {
            kind: "small_piece".into(),
            ..f[0].clone()
        }
        .size_text()
        .is_empty()
    );
    assert!(CheckResult::parse(&json!({})).is_err());
}

#[test]
fn every_kind_has_a_label_in_the_language() {
    for k in kinds() {
        assert_ne!(kind_label(k), k, "{k}");
    }
    assert_eq!(kind_label("unknown"), "unknown");
    assert_eq!(kinds().len(), 7);
}

#[test]
fn the_check_looks_at_the_selected_hardscapes_or_at_all_of_them() {
    let (editor, id) = lot::new();
    let mut be = Fake::new();
    be.doc = editor.doc().clone();
    assert_eq!(targets(&be), vec![id], "none selected: all");
    be.selection = vec![id];
    assert_eq!(targets(&be), vec![id]);
    // A selection of something else means all of them again.
    be.selection = vec![ObjectId(kadakas_core::uid::Uid::default())];
    assert_eq!(targets(&be), vec![id]);
    // A document without a hardscape has nothing to check.
    let mut empty = Fake::new();
    let mut panel = CheckPanel::default();
    panel.run(&mut empty);
    assert!(panel.result().is_none());
    assert!(matches!(panel.result, Some(Err(_))));
    assert!(empty.calls.is_empty());
}

#[test]
fn the_panel_runs_the_check_keeps_the_choice_and_knows_when_it_is_old() {
    let (editor, id) = lot::new();
    let mut be = Fake::new().reply(reply(id));
    be.doc = editor.doc().clone();
    let mut panel = CheckPanel::default();
    assert!(!panel.stale(&be));
    panel.run(&mut be);
    assert_eq!(be.calls[0].0, "unit_pattern.check");
    assert_eq!(be.calls[0].1["ids"], json!([id]));
    assert_eq!(panel.result().unwrap().finding_count(), 3);
    assert!(panel.current().is_none());
    panel.selected = Some((0, 1));
    assert_eq!(panel.current().unwrap().at, DVec2::new(300.0, 400.0));
    panel.selected = Some((5, 0));
    assert!(panel.current().is_none());
    assert!(panel.shows("overlap"));
    panel.only = Some("overlap".into());
    assert!(panel.shows("overlap") && !panel.shows("coverage"));
    // The drawing changed: the findings may be old.
    assert!(!panel.stale(&be));
    be.revision += 1;
    assert!(panel.stale(&be));
    // A failing call is shown as the error.
    panel.run(&mut be);
    assert!(panel.result().is_none());
    assert!(panel.selected.is_none());
}

#[test]
fn the_scope_of_the_check_is_kept_when_the_selection_changes() {
    let (editor, id) = lot::new();
    let mut be = Fake::new().reply(reply(id)).reply(reply(id));
    be.doc = editor.doc().clone();
    be.selection = vec![id];
    let mut panel = CheckPanel::default();
    panel.set_scope(&be);
    assert_eq!(panel.scope, vec![id]);
    // The person selects something else (a finding was clicked): the next run is the same check.
    be.selection = vec![];
    panel.run(&mut be);
    panel.run(&mut be);
    assert_eq!(be.calls.len(), 2);
    assert_eq!(be.calls[1].1["ids"], json!([id]));
    // An object that is gone leaves nothing to check.
    be.doc = kadakas_core::model::Document::default();
    panel.run(&mut be);
    assert!(panel.result().is_none() && !panel.running);
}

#[test]
fn a_run_is_begun_and_finished_in_two_steps_for_a_background_job() {
    let (editor, id) = lot::new();
    let mut be = Fake::new();
    be.doc = editor.doc().clone();
    let mut panel = CheckPanel::default();
    panel.set_scope(&be);
    let p = panel.begin(&be).unwrap();
    assert!(panel.running);
    assert_eq!(p["ids"], json!([id]));
    panel.finish(Ok(reply(id)));
    assert!(!panel.running);
    assert_eq!(panel.result().unwrap().finding_count(), 3);
    panel.finish(Err("boom".into()));
    assert!(panel.result().is_none());
}
