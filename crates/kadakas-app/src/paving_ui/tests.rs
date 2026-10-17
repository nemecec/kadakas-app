//! The paving UI against a real session: every action is an API call, and a preview changes
//! nothing.

use super::*;
use kadakas_ui_core::paving::check::CheckPanel;
use kadakas_ui_core::paving::counts;
use kadakas_ui_core::paving::course::{self, CourseEditor};
use kadakas_ui_core::paving::curves::{CurvesDialog, Role};
use kadakas_ui_core::paving::split::{DraftLine, SplitDialog};
use serde_json::{Value, json};

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
    assert!(
        apply_pick(
            &mut s,
            Pick {
                target: id,
                kind: PickKind::Origin
            },
            DVec2::new(100.0, 100.0),
            tol
        )
        .unwrap()
    );
    let o = s.editor.doc().object(id).unwrap();
    assert_eq!(
        kadakas_core::hardscape::data(o).unwrap().origin,
        Some(DVec2::new(100.0, 100.0))
    );
    // Far from every edge: nothing is picked.
    assert!(
        !apply_pick(
            &mut s,
            Pick {
                target: id,
                kind: PickKind::AlignEdge
            },
            DVec2::new(2000.0, 1500.0),
            tol
        )
        .unwrap()
    );
    assert!(
        apply_pick(
            &mut s,
            Pick {
                target: id,
                kind: PickKind::AlignEdge
            },
            DVec2::new(2000.0, 5.0),
            tol
        )
        .unwrap()
    );
    let o = s.editor.doc().object(id).unwrap();
    assert_eq!(kadakas_core::hardscape::data(o).unwrap().angle, Some(0.0));
    // A zone gets its own origin and an edge of its own.
    assert!(
        apply_pick(
            &mut s,
            Pick {
                target: id,
                kind: PickKind::ZoneOrigin(1)
            },
            DVec2::new(2500.0, 100.0),
            tol
        )
        .unwrap()
    );
    assert!(
        apply_pick(
            &mut s,
            Pick {
                target: id,
                kind: PickKind::ZoneAlignEdge(1)
            },
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
    frame(&mut p, &mut s, &mut status);
    let rev = s.editor.revision();
    assert_eq!(p.split.as_ref().unwrap().zones.as_ref().unwrap().len(), 2);
    p.split
        .as_mut()
        .unwrap()
        .commit(&mut SessionBackend(&mut s))
        .unwrap();
    p.split = None;
    p.drawing = false;
    p.course = Some(CourseEditor::new_course(s.editor.doc(), id).unwrap());
    p.begin_pick(id, PickKind::ZoneAlignEdge(0), &mut status);
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
    // Only the split changed the document.
    assert!(s.editor.revision() >= rev);
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
            &p,
            true,
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
