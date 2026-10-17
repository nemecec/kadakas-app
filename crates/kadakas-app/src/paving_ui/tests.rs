//! The paving UI against a real session: every action is an API call, and a preview changes
//! nothing.

use super::*;
use kadakas_ui_core::paving::check::CheckPanel;
use kadakas_ui_core::paving::counts;
use kadakas_ui_core::paving::course::{self, CourseEditor};
use kadakas_ui_core::paving::curves::{CurvesDialog, Role};
use kadakas_ui_core::paving::split::{DraftLine, SplitDialog};
use serde_json::{Value, json};

fn pick(s: &Session, target: ObjectId, kind: PickKind) -> Pick {
    Pick {
        target,
        kind,
        rev: s.editor.revision(),
    }
}

fn do_pick(
    s: &mut Session,
    id: ObjectId,
    kind: PickKind,
    at: DVec2,
    tol: f64,
) -> Result<bool, String> {
    let pick = pick(s, id, kind);
    apply_pick(s, pick, at, tol)
}

fn session() -> Session {
    let mut s = Session::with_sequential_ids();
    s.handle(
        "hardscape_style.create",
        json!({"name": "Pavers", "joints": {"kind": "stack", "unit_length": 200, "unit_width": 100, "spacing": 5}}),
    )
    .unwrap();
    s
}

fn lot(s: &mut Session) -> ObjectId {
    let r = s
        .handle(
            "hardscape.add",
            json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "style": "Pavers"}),
        )
        .unwrap();
    serde_json::from_value(r["id"].clone()).unwrap()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> ObjectId {
    let r = s
        .handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": a, "b": b}}}),
        )
        .unwrap();
    serde_json::from_value(r["created"][0].clone()).unwrap()
}

#[test]
fn loose_lines_become_a_hardscape_through_the_preview_and_one_undo_step() {
    let mut s = session();
    let ids = [
        line(&mut s, [0.0, 0.0], [5000.0, 0.0]),
        line(&mut s, [5000.0, 0.0], [5000.0, 4000.0]),
        line(&mut s, [5000.0, 4000.0], [0.0, 4000.0]),
        // The last line stops 0.5 mm short: a gap that the tolerance closes.
        line(&mut s, [0.0, 4000.0], [0.0, 0.5]),
    ];
    s.selection = ids.to_vec();
    let before = s.editor.doc().objects.len();
    let mut d = CurvesDialog::open(&SessionBackend(&mut s)).unwrap();
    // The preview changed nothing.
    assert_eq!(s.editor.doc().objects.len(), before);
    let found = d.found.as_ref().unwrap();
    assert_eq!(found.regions.len(), 1);
    assert_eq!(found.regions[0].role, Role::Area);
    assert!((found.regions[0].area_m2 - 20.0).abs() < 0.01);
    assert!(found.gaps >= 1, "the gap was closed and reported");
    assert!(found.notes.iter().any(|n| n.at.is_some()));
    d.form.style = Some("Pavers".into());
    let made = d.commit(&mut SessionBackend(&mut s)).unwrap();
    assert_eq!(made.created.len(), 1);
    let o = s.editor.doc().object(made.created[0]).unwrap();
    assert!(matches!(o.shape, Shape::Hardscape { .. }));
    assert!(kadakas_core::hardscape::data(o).unwrap().style.is_some());
    assert_eq!(s.selection, made.created);
    // One undo step takes the hardscape away again.
    assert!(s.editor.undo());
    assert_eq!(s.editor.doc().objects.len(), before);
}

#[test]
fn shapes_that_do_not_close_say_so() {
    let mut s = session();
    let a = line(&mut s, [0.0, 0.0], [3000.0, 0.0]);
    let b = line(&mut s, [3000.0, 0.0], [3000.0, 3000.0]);
    s.selection = vec![a, b];
    let d = CurvesDialog::open(&SessionBackend(&mut s)).unwrap();
    let f = d.found.as_ref().unwrap();
    assert!(f.regions.is_empty());
    assert!(!f.complete);
    assert!(
        f.notes.iter().any(|n| n.at.is_some()),
        "the open end has a place"
    );
    assert!(!f.marks.is_empty());
}

#[test]
fn a_split_line_previews_zones_and_the_split_is_stored_with_one_undo_step() {
    let mut s = session();
    let id = lot(&mut s);
    let mut d = SplitDialog::open(&SessionBackend(&mut s), id).unwrap();
    d.draft.click(DVec2::new(2000.0, -500.0));
    d.draft.click(DVec2::new(2000.0, 3500.0));
    assert!(d.draft.finish_line());
    d.refresh(&SessionBackend(&mut s));
    let zones = d.zones.as_ref().unwrap();
    assert_eq!(zones.len(), 2);
    let sum: f64 = zones.iter().map(|z| z.area_m2).sum();
    assert!((sum - 12.0).abs() < 1e-6);
    // The preview stored nothing.
    assert!(
        split::zone_shapes(s.editor.doc(), id, 100_000)
            .unwrap()
            .is_empty()
    );
    let depth = s.editor.undo_depth();
    d.commit(&mut SessionBackend(&mut s)).unwrap();
    assert_eq!(s.editor.undo_depth(), depth + 1, "one step");
    let zones = split::zone_shapes(s.editor.doc(), id, 100_000).unwrap();
    assert_eq!(zones.len(), 2);
    assert!(s.editor.undo());
    assert_eq!(s.editor.undo_depth(), depth);
    assert!(
        split::zone_shapes(s.editor.doc(), id, 100_000)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_drawn_line_can_be_picked_and_the_zones_follow_it() {
    let mut s = session();
    let id = lot(&mut s);
    let l = line(&mut s, [1000.0, -500.0], [1000.0, 3500.0]);
    s.selection = vec![l];
    let mut d = SplitDialog::open(&SessionBackend(&mut s), id).unwrap();
    assert_eq!(d.draft.add_objects(s.editor.doc(), &[l]), 1);
    assert_eq!(d.draft.lines, vec![DraftLine::Object(l)]);
    d.commit(&mut SessionBackend(&mut s)).unwrap();
    let first = split::zone_shapes(s.editor.doc(), id, 100_000).unwrap();
    s.handle(
        "execute",
        json!({"command": {"op": "move_objects", "ids": [l], "delta": [1000, 0]}}),
    )
    .unwrap();
    let zones = split::zone_shapes(s.editor.doc(), id, 100_000).unwrap();
    assert_eq!(zones.len(), 2);
    // The cut moved from x = 1000 to x = 2000: the first zone grew by 1 m x 3 m = 3 m2.
    let area = |z: &[ZoneShape]| z.iter().map(|z| z.area_m2).fold(f64::MAX, f64::min);
    assert!((area(&zones) - area(&first)).abs() > 2.9);
}

#[test]
fn zone_settings_change_the_pattern_and_the_picks_work() {
    let mut s = session();
    let id = lot(&mut s);
    let mut d = SplitDialog::open(&SessionBackend(&mut s), id).unwrap();
    d.draft.click(DVec2::new(2000.0, -500.0));
    d.draft.click(DVec2::new(2000.0, 3500.0));
    d.draft.finish_line();
    d.commit(&mut SessionBackend(&mut s)).unwrap();
    // Pick an origin for the hardscape and an edge to align to.
    let tol = 20.0;
    assert!(do_pick(&mut s, id, PickKind::Origin, DVec2::new(100.0, 100.0), tol).unwrap());
    let o = s.editor.doc().object(id).unwrap();
    assert_eq!(
        kadakas_core::hardscape::data(o).unwrap().origin,
        Some(DVec2::new(100.0, 100.0))
    );
    // Far from every edge: nothing is picked.
    assert!(
        !do_pick(
            &mut s,
            id,
            PickKind::AlignEdge,
            DVec2::new(2000.0, 1500.0),
            tol
        )
        .unwrap()
    );
    assert!(
        do_pick(
            &mut s,
            id,
            PickKind::AlignEdge,
            DVec2::new(2000.0, 5.0),
            tol
        )
        .unwrap()
    );
    let o = s.editor.doc().object(id).unwrap();
    assert_eq!(kadakas_core::hardscape::data(o).unwrap().angle, Some(0.0));
    // A zone gets its own origin and an edge of its own.
    assert!(
        do_pick(
            &mut s,
            id,
            PickKind::ZoneOrigin(1),
            DVec2::new(2500.0, 100.0),
            tol
        )
        .unwrap()
    );
    assert!(
        do_pick(
            &mut s,
            id,
            PickKind::ZoneAlignEdge(1),
            DVec2::new(3000.0, 2.0),
            tol
        )
        .unwrap()
    );
    let zones = split::zone_shapes(s.editor.doc(), id, 100_000).unwrap();
    let z = zones
        .iter()
        .find(|z| z.has_setting)
        .expect("a zone has a setting");
    let st = ZoneSettings::read(s.editor.doc(), s.editor.doc().object(id).unwrap(), z);
    assert!(st.align_to_edge.is_some());
    // The report sees the change: the units of the zone are laid from the new start.
    let rep = s
        .handle("report.unit_pattern", json!({"ids": [id]}))
        .unwrap();
    assert!(rep["rows"][0]["whole_units"].as_u64().unwrap() > 0);
}

#[test]
fn a_course_previews_on_a_copy_and_is_stored_by_the_api_with_the_counts_of_the_report() {
    let mut s = session();
    let id = lot(&mut s);
    let mut ed = CourseEditor::new_course(s.editor.doc(), id).unwrap();
    ed.refresh(&SessionBackend(&mut s));
    let pv = ed.preview.as_ref().unwrap();
    assert!(pv.units.len() > 50, "{}", pv.units.len());
    let row = pv.row.clone().unwrap();
    assert_eq!(row.units, pv.units.len());
    // Nothing is stored by the preview.
    assert!(
        course::rows(s.editor.doc(), id, 100_000)
            .unwrap()
            .is_empty()
    );
    ed.apply(&mut SessionBackend(&mut s)).unwrap();
    let rows = course::rows(s.editor.doc(), id, 100_000).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].editable);
    // The units of the preview are the units of the report for that course.
    let rep = s
        .handle("report.unit_pattern", json!({"ids": [id]}))
        .unwrap();
    let groups = rep["rows"][0]["groups"].as_array().unwrap();
    let c = groups.iter().find(|g| g["role"] == "course").unwrap();
    assert_eq!(
        c["whole_units"].as_u64().unwrap() + c["cut_units"].as_u64().unwrap(),
        row.units as u64
    );
    // Editing it replaces it; removing it is one call.
    let mut again = CourseEditor::edit(s.editor.doc(), id, &rows[0].name).unwrap();
    again.form.offset = 50.0;
    again.apply(&mut SessionBackend(&mut s)).unwrap();
    assert_eq!(course::rows(s.editor.doc(), id, 100_000).unwrap().len(), 1);
    course::remove(&mut SessionBackend(&mut s), id, &rows[0].name).unwrap();
    assert!(
        course::rows(s.editor.doc(), id, 100_000)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_counts_are_those_of_the_report_and_the_check_lists_its_findings() {
    let mut s = session();
    let id = lot(&mut s);
    let c = counts::count(&mut SessionBackend(&mut s), id).unwrap();
    let rep: Value = s
        .handle("report.unit_pattern", json!({"ids": [id]}))
        .unwrap();
    assert_eq!(
        c.whole as u64,
        rep["totals"]["whole_units"].as_u64().unwrap()
    );
    assert_eq!(c.cut as u64, rep["totals"]["cut_units"].as_u64().unwrap());
    assert!(c.whole > 0);
    // 4 m x 3 m of 200 x 100 mm units laid in a stack: a clean lot has no findings.
    let mut panel = CheckPanel::default();
    panel.run(&mut SessionBackend(&mut s));
    let r = panel.result().expect("the check ran");
    assert!(r.complete);
    assert_eq!(r.objects.len(), 1);
    // A split with a zone of another unit size shows joints at the zone edge only by design;
    // a pattern with a unit that does not fit shows small pieces. The panel reads what comes.
    assert!(r.units > 0);
    assert!(!panel.stale(&SessionBackend(&mut s)));
    s.handle("hardscape.set", json!({"id": id, "angle": 10}))
        .unwrap();
    assert!(panel.stale(&SessionBackend(&mut s)));
}

#[test]
fn the_windows_and_object_info_draw_in_every_state_without_changing_the_document() {
    let mut s = session();
    let id = lot(&mut s);
    let l = line(&mut s, [0.0, 0.0], [1000.0, 0.0]);
    let mut p = PavingUi::default();
    let mut status = String::new();
    // A split and a course, then the check.
    s.selection = vec![id];
    p.open_split(&mut s, &mut status);
    assert!(p.captures());
    p.press(&mut s, DVec2::new(2000.0, -500.0), 10.0, &mut status);
    p.press(&mut s, DVec2::new(2000.0, 3500.0), 10.0, &mut status);
    p.double_click();
    p.set_hover(DVec2::new(10.0, 10.0));
    let ctx = egui::Context::default();
    let frame = |p: &mut PavingUi, s: &mut Session, status: &mut String| {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            p.windows(ui.ctx(), s, status);
            let mut e = crate::panels::InfoEdit::default();
            crate::panels::object_info(ui, s, &mut e, p, status);
        });
    };
    let before = s.editor.revision();
    frame(&mut p, &mut s, &mut status);
    let rev = s.editor.revision();
    assert_eq!(rev, before, "the dialogs and the preview changed nothing");
    assert_eq!(p.split.as_ref().unwrap().zones.as_ref().unwrap().len(), 2);
    p.split
        .as_mut()
        .unwrap()
        .commit(&mut SessionBackend(&mut s))
        .unwrap();
    p.split = None;
    p.drawing = false;
    p.course = Some(CourseEditor::new_course(s.editor.doc(), id).unwrap());
    p.begin_pick(&s, id, PickKind::ZoneAlignEdge(0), &mut status);
    p.open_zone = Some(0);
    frame(&mut p, &mut s, &mut status);
    frame(&mut p, &mut s, &mut status);
    assert!(p.course.as_ref().unwrap().preview.is_ok());
    p.cancel();
    s.selection = vec![l];
    p.open_curves(&mut s, &mut status);
    frame(&mut p, &mut s, &mut status);
    assert!(p.curves.is_some() || !status.is_empty());
    s.selection = vec![id];
    p.run_check(&mut s);
    frame(&mut p, &mut s, &mut status);
    // A zoom request is handed out once.
    p.zoom_to(DVec2::ZERO, DVec2::ONE);
    assert!(p.take_zoom().is_some() && p.take_zoom().is_none());
    // The painting runs with every preview there is.
    let view = crate::view::View::default();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        let r = ui.max_rect();
        paint(
            &ui.painter().clone(),
            r,
            &view,
            s.editor.doc(),
            &s.selection,
            s.editor.revision(),
            &p,
        );
    });
    p.reset();
    assert!(!p.captures());
}

#[test]
fn a_hardscape_without_a_pattern_or_a_selection_gives_a_message() {
    let mut s = Session::with_sequential_ids();
    let mut p = PavingUi::default();
    let mut status = String::new();
    p.open_split(&mut s, &mut status);
    assert!(p.split.is_none() && !status.is_empty());
    status.clear();
    p.open_curves(&mut s, &mut status);
    assert!(p.curves.is_none() && !status.is_empty());
    p.run_check(&mut s);
    assert!(p.check.result().is_none());
}

/// The plan data of a hardscape, for the comparison of two ways to the same settings.
fn plan_of(s: &Session, id: ObjectId) -> kadakas_core::layout::plan::Plan {
    kadakas_core::layout::plan::data(s.editor.doc().object(id).unwrap()).unwrap()
}

fn split_lot(s: &mut Session) -> ObjectId {
    let id = lot(s);
    s.handle(
        "hardscape.split",
        json!({"id": id, "lines": [[[2000, -500], [2000, 3500]]]}),
    )
    .unwrap();
    id
}

#[test]
fn a_zone_setting_and_a_pick_are_one_undo_step_each() {
    let mut s = session();
    let id = split_lot(&mut s);
    let depth = s.editor.undo_depth();
    assert!(
        do_pick(
            &mut s,
            id,
            PickKind::ZoneOrigin(1),
            DVec2::new(2500.0, 100.0),
            20.0
        )
        .unwrap()
    );
    assert_eq!(s.editor.undo_depth(), depth + 1);
    assert!(do_pick(&mut s, id, PickKind::Origin, DVec2::new(10.0, 10.0), 20.0).unwrap());
    assert_eq!(s.editor.undo_depth(), depth + 2);
    assert!(
        do_pick(
            &mut s,
            id,
            PickKind::ZoneAlignEdge(1),
            DVec2::new(3000.0, 2.0),
            20.0
        )
        .unwrap()
    );
    assert_eq!(s.editor.undo_depth(), depth + 3);
    assert!(s.editor.undo());
    assert!(s.editor.undo());
    assert!(s.editor.undo());
    assert!(
        plan_of(&s, id)
            .zones
            .iter()
            .all(|z| z.origin.is_none() && z.align_to_edge.is_none())
    );
}

#[test]
fn the_edit_of_the_form_stores_the_same_plan_as_the_api_call_of_an_agent() {
    // The person's way: the edit of the form.
    let mut a = session();
    let id_a = split_lot(&mut a);
    let zones = split::zone_shapes(a.editor.doc(), id_a, 100_000).unwrap();
    let base = ZoneSettings::read(
        a.editor.doc(),
        a.editor.doc().object(id_a).unwrap(),
        &zones[1],
    );
    let mut edit = ZoneEdit::new(id_a, 1, base);
    edit.settings.name = "Drive".into();
    edit.settings.pattern = split::PatternChoice::Bond(kadakas_core::layout::Bond::Herringbone);
    edit.settings.unit_length = 300.0;
    edit.settings.unit_width = 150.0;
    edit.settings.gap = 4.0;
    edit.settings.angle = Some(45.0);
    edit.settings.origin = Some(DVec2::new(2100.0, 50.0));
    split::store_zone(&mut SessionBackend(&mut a), &edit).unwrap();
    // The agent's way: hardscape.split with the settings of the zone.
    let mut b = session();
    let id_b = lot(&mut b);
    b.handle(
        "hardscape.split",
        json!({"id": id_b, "lines": [[[2000, -500], [2000, 3500]]], "zones": [{
            "index": 1, "name": "Drive",
            "pattern": {"bond": "herringbone", "unit_length": 300, "unit_width": 150, "gap": 4},
            "angle": 45, "origin": [2100, 50]}]}),
    )
    .unwrap();
    let (pa, pb) = (plan_of(&a, id_a), plan_of(&b, id_b));
    let strip = |p: &kadakas_core::layout::plan::Plan| {
        let mut z = p.zones.clone();
        z.sort_by(|x, y| x.name.cmp(&y.name));
        z
    };
    assert_eq!(strip(&pa), strip(&pb));
    assert_eq!(pa.splits, pb.splits);
    // And the report counts the same units.
    let ra = a
        .handle("report.unit_pattern", json!({"ids": [id_a]}))
        .unwrap();
    let rb = b
        .handle("report.unit_pattern", json!({"ids": [id_b]}))
        .unwrap();
    assert_eq!(ra["totals"]["whole_units"], rb["totals"]["whole_units"]);
    assert_eq!(ra["totals"]["cut_units"], rb["totals"]["cut_units"]);
}

#[test]
fn zone_set_leaves_the_other_zones_and_refuses_what_cannot_be_laid() {
    let mut s = session();
    let id = split_lot(&mut s);
    s.handle(
        "hardscape.zone_set",
        json!({"id": id, "zone": 0, "angle": 30}),
    )
    .unwrap();
    s.handle(
        "hardscape.zone_set",
        json!({"id": id, "zone": 1, "name": "Right", "origin": [2500, 0]}),
    )
    .unwrap();
    let p = plan_of(&s, id);
    assert_eq!(p.zones.iter().filter(|z| z.angle == Some(30.0)).count(), 1);
    assert!(
        p.zones
            .iter()
            .any(|z| z.name.as_deref() == Some("Right") && z.origin.is_some())
    );
    s.handle(
        "hardscape.zone_set",
        json!({"id": id, "zone": 0, "clear": ["angle"]}),
    )
    .unwrap();
    assert!(plan_of(&s, id).zones.iter().all(|z| z.angle.is_none()));
    // No such zone, a split line that is not there, a setting set and cleared together.
    assert!(
        s.handle(
            "hardscape.zone_set",
            json!({"id": id, "zone": 9, "angle": 1})
        )
        .is_err()
    );
    assert!(
        s.handle(
            "hardscape.zone_set",
            json!({"id": id, "zone": 0, "align_to_split": 5})
        )
        .is_err()
    );
    assert!(
        s.handle(
            "hardscape.zone_set",
            json!({"id": id, "zone": 0, "angle": 1, "clear": ["angle"]})
        )
        .is_err()
    );
    assert!(
        s.handle(
            "hardscape.zone_set",
            json!({"id": id, "zone": 0, "clear": ["nonsense"]})
        )
        .is_err()
    );
}

#[test]
fn an_edit_of_an_agent_between_the_preview_and_the_commit_is_not_overwritten() {
    let mut s = session();
    let ids = [
        line(&mut s, [0.0, 0.0], [5000.0, 0.0]),
        line(&mut s, [5000.0, 0.0], [5000.0, 4000.0]),
        line(&mut s, [5000.0, 4000.0], [0.0, 4000.0]),
        line(&mut s, [0.0, 4000.0], [0.0, 0.0]),
    ];
    s.selection = ids.to_vec();
    let mut d = CurvesDialog::open(&SessionBackend(&mut s)).unwrap();
    // An agent deletes one of the lines after the preview.
    s.handle(
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [ids[3]]}}),
    )
    .unwrap();
    assert!(d.stale(&SessionBackend(&mut s)));
    d.refresh(&SessionBackend(&mut s));
    // The square is open now: nothing can be created, and the dialog says so.
    assert!(d.found.as_ref().unwrap().regions.is_empty());
    assert!(d.commit(&mut SessionBackend(&mut s)).is_err());
    // Split: the draft is looked at again after the agent's edit.
    let id = lot(&mut s);
    let mut sd = SplitDialog::open(&SessionBackend(&mut s), id).unwrap();
    sd.draft.click(DVec2::new(2000.0, -500.0));
    sd.draft.click(DVec2::new(2000.0, 3500.0));
    sd.draft.finish_line();
    sd.refresh(&SessionBackend(&mut s));
    assert_eq!(sd.zones.as_ref().unwrap().len(), 2);
    s.handle("hardscape.set", json!({"id": id, "angle": 20}))
        .unwrap();
    sd.refresh(&SessionBackend(&mut s));
    assert_eq!(sd.zones.as_ref().unwrap().len(), 2);
    sd.commit(&mut SessionBackend(&mut s)).unwrap();
    // The agent's angle is still there.
    let d = kadakas_core::hardscape::data(s.editor.doc().object(id).unwrap()).unwrap();
    assert_eq!(d.angle, Some(20.0));
}

#[test]
fn a_locked_document_stops_every_change_and_a_pick_of_a_zone_ends_when_the_drawing_changes() {
    let mut s = session();
    let id = split_lot(&mut s);
    let mut p = PavingUi::default();
    let mut status = String::new();
    p.frame(s.editor.revision(), Some("locked".into()), &mut status);
    assert!(p.blocked(&mut status) && status == "locked");
    p.begin_pick(&s, id, PickKind::Origin, &mut status);
    assert!(!p.captures(), "no pick while locked");
    p.frame(s.editor.revision(), None, &mut status);
    p.begin_pick(&s, id, PickKind::ZoneOrigin(0), &mut status);
    assert!(p.captures());
    // An agent changes the document: the zone number may be another zone now.
    s.handle("hardscape.set", json!({"id": id, "angle": 5}))
        .unwrap();
    p.frame(s.editor.revision(), None, &mut status);
    assert!(!p.captures());
    // A pick of the whole hardscape stays.
    p.begin_pick(&s, id, PickKind::Origin, &mut status);
    s.handle("hardscape.set", json!({"id": id, "angle": 6}))
        .unwrap();
    p.frame(s.editor.revision(), None, &mut status);
    assert!(p.captures());
    // A press while locked ends the pick and changes nothing.
    let depth = s.editor.undo_depth();
    p.frame(s.editor.revision(), Some("locked".into()), &mut status);
    assert!(p.press(&mut s, DVec2::new(5.0, 5.0), 10.0, &mut status));
    assert!(!p.captures());
    assert_eq!(s.editor.undo_depth(), depth);
}

#[test]
fn the_check_runs_in_the_background_on_a_copy_and_keeps_its_scope() {
    let mut s = session();
    let id = lot(&mut s);
    let other = lot(&mut s);
    s.selection = vec![id];
    let mut p = PavingUi::default();
    let mut status = String::new();
    p.run_check(&mut s);
    let ctx = egui::Context::default();
    let frame = |p: &mut PavingUi, s: &mut Session, status: &mut String| {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            p.windows(ui.ctx(), s, status)
        });
    };
    for _ in 0..400 {
        frame(&mut p, &mut s, &mut status);
        if !p.check.running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let r = p.check.result().expect("the check came back");
    assert_eq!(r.objects.len(), 1);
    // A click on a finding selects through the API and does not change what is checked.
    s.selection = vec![other];
    assert_eq!(p.check.scope, vec![id]);
    assert!(!p.check.stale(&SessionBackend(&mut s)));
}
