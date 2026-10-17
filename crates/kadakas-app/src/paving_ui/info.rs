//! The unit pattern in Object Info: the pattern with its start, the counts of whole and cut units,
//! the zones with their settings and the courses along the edges. It follows the hardscape
//! section of `area_ui`. The slow parts (counts, zones, courses) are computed on a thread by
//! revision, so a big lot or a fast agent does not stop the window.

use egui::{DragValue, RichText, TextEdit, Ui};
use kadakas_api::Session;
use kadakas_core::layout::plan;
use kadakas_core::layout::{Bond, object::pattern_of};
use kadakas_core::model::ObjectId;
use kadakas_l10n::tr;
use kadakas_ui_core::paving::Backend as _;
use kadakas_ui_core::paving::counts::{self, UnitCounts};
use kadakas_ui_core::paving::course::{
    self, Along, CourseEditor, CourseForm, Orientation, Placing,
};
use kadakas_ui_core::paving::split::{self, PatternChoice, ZoneEdit, ZoneSettings, ZoneShape};

use super::jobs::Poll;
use super::{PavingUi, PickKind, Request, Scratch, SessionBackend};
use crate::units_ui::DragValueExt as _;

/// The paving section of a hardscape in Object Info. Nothing is drawn for a hardscape without a
/// joint pattern.
pub fn hardscape_section(
    ui: &mut Ui,
    session: &mut Session,
    id: ObjectId,
    p: &mut PavingUi,
    status: &mut String,
) {
    let doc = session.editor.doc();
    let Some(obj) = doc.object(id) else { return };
    let Some((pattern, _)) = pattern_of(doc, obj) else {
        return;
    };
    let style = kadakas_core::hardscape::data(obj).and_then(|d| d.style);
    let units = kadakas_units::current();
    ui.separator();
    ui.strong(tr!("paving-ui-unit-pattern"));
    // The pattern: it belongs to the style, so it changes for every hardscape of the style.
    let bond = match pattern.bond {
        Bond::RunningBond => tr!("joint-ui-bond-running-bond"),
        Bond::Stack => tr!("joint-ui-bond-stack"),
        Bond::Herringbone => tr!("joint-ui-bond-herringbone"),
        Bond::BasketWeave => tr!("joint-ui-bond-basket-weave"),
        Bond::Module => tr!("joint-ui-bond-module"),
        Bond::Generator => tr!("joint-ui-bond-generator"),
    };
    ui.horizontal_wrapped(|ui| {
        match pattern.bond {
            Bond::Module | Bond::Generator => {
                ui.label(tr!(
                    "paving-ui-pattern-module",
                    bond = bond.as_str(),
                    gap = units.length_text_d(pattern.gap, 1)
                ));
            }
            _ => {
                ui.label(tr!(
                    "paving-ui-pattern-bond",
                    bond = bond.as_str(),
                    length = units.length_text_d(pattern.unit_length, 1),
                    width = units.length_text_d(pattern.unit_width, 1),
                    gap = units.length_text_d(pattern.gap, 1)
                ));
            }
        }
        if let Some(s) = style
            && ui
                .button(tr!("paving-ui-edit-pattern"))
                .on_hover_text(tr!(
                    "paving-ui-the-pattern-belongs-to-the-style-it-changes-every-hardscape-with-it"
                ))
                .clicked()
        {
            p.request(Request::EditJoints(s));
        }
    });
    ui.horizontal_wrapped(|ui| {
        let picking = p.pick.map(|k| (k.target, k.kind));
        if ui
            .selectable_label(
                picking == Some((id, PickKind::Origin)),
                tr!("paving-ui-pick-origin"),
            )
            .on_hover_text(tr!(
                "paving-ui-click-a-point-in-the-drawing-where-the-pattern-starts"
            ))
            .clicked()
        {
            p.begin_pick(session, id, PickKind::Origin, status);
        }
        if ui
            .selectable_label(
                picking == Some((id, PickKind::AlignEdge)),
                tr!("paving-ui-align-to-edge"),
            )
            .on_hover_text(tr!(
                "paving-ui-click-an-edge-of-the-outline-the-pattern-starts-there-and-runs-along-it"
            ))
            .clicked()
        {
            p.begin_pick(session, id, PickKind::AlignEdge, status);
        }
    });
    counts_block(ui, session, id, p);
    zones_block(ui, session, id, p, status);
    courses_block(ui, session, id, p, status);
}

fn counting(ui: &mut Ui) {
    ui.label(RichText::new(tr!("paving-ui-counting")).small());
}

// ---------------------------------------------------------------- counts

fn counts_block(ui: &mut Ui, session: &mut Session, id: ObjectId, p: &mut PavingUi) {
    let rev = session.editor.revision();
    let auto = counts::auto(session.editor.doc(), id) || p.count_asked == Some(id);
    if !auto {
        if ui
            .button(tr!("paving-ui-count-the-units"))
            .on_hover_text(tr!(
                "paving-ui-a-big-area-takes-time-to-count-so-it-is-counted-when-you-ask"
            ))
            .clicked()
        {
            p.count_asked = Some(id);
        }
        return;
    }
    let ctx = ui.ctx().clone();
    let doc = session.editor.doc();
    let limits = session.limits().clone();
    let shared = &p.shared;
    let result = match p.counts.poll(&ctx, rev, id, || {
        let doc = shared.get(doc, rev);
        Box::new(move || counts::count(&mut Scratch::new((*doc).clone(), limits, Vec::new()), id))
    }) {
        Poll::Fresh(r) => Some(r),
        Poll::Old(r) => {
            ui.label(RichText::new(tr!("paving-ui-the-counts-are-of-an-earlier-state")).small());
            Some(r)
        }
        Poll::Busy => {
            counting(ui);
            None
        }
    };
    match result {
        Some(Err(e)) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                tr!("paving-ui-error", e = e.as_str()),
            );
        }
        Some(Ok(c)) => counts_grid(ui, id, c),
        None => {}
    }
}

fn counts_grid(ui: &mut Ui, id: ObjectId, c: &UnitCounts) {
    let units = kadakas_units::current();
    egui::Grid::new(("paving-counts", id))
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr!("paving-ui-whole-units"));
            ui.label(c.whole.to_string());
            ui.end_row();
            ui.label(tr!("paving-ui-cut-units"));
            ui.label(if c.slivers > 0 || c.unfillable > 0 {
                tr!(
                    "paving-ui-cut-units-detail",
                    count = c.cut,
                    slivers = c.slivers,
                    unfillable = c.unfillable
                )
            } else {
                c.cut.to_string()
            });
            ui.end_row();
            ui.label(tr!("paving-ui-order"));
            ui.label(if c.order_units_min != c.order_units {
                tr!(
                    "paving-ui-order-range",
                    min = c.order_units_min,
                    max = c.order_units
                )
            } else {
                c.order_units.to_string()
            });
            ui.end_row();
            ui.label(tr!("paving-ui-laid-area"));
            ui.label(units.area_text(c.area_laid_m2 * 1e6));
            ui.end_row();
            if c.types.len() > 1 {
                for t in &c.types {
                    ui.label(t.name.as_str());
                    ui.label(tr!("paving-ui-whole-and-cut", whole = t.whole, cut = t.cut));
                    ui.end_row();
                }
            }
            for part in &c.parts {
                ui.label(part.name.as_str());
                ui.label(tr!(
                    "paving-ui-whole-and-cut",
                    whole = part.whole,
                    cut = part.cut
                ));
                ui.end_row();
            }
        });
    if let Some(w) = &c.joint_warning {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            tr!("uic-paving-note-api", text = w.as_str()),
        );
    }
}

// ---------------------------------------------------------------- zones

fn zones_block(
    ui: &mut Ui,
    session: &mut Session,
    id: ObjectId,
    p: &mut PavingUi,
    status: &mut String,
) {
    let rev = session.editor.revision();
    let ctx = ui.ctx().clone();
    let doc = session.editor.doc();
    let max = session.limits().pattern_units;
    let splits = doc
        .object(id)
        .and_then(plan::data)
        .map_or(0, |pl| pl.splits.len());
    let shared = &p.shared;
    let (zones, fresh) = match p.zones.poll(&ctx, rev, id, || {
        let doc = shared.get(doc, rev);
        Box::new(move || split::zone_shapes(&doc, id, max))
    }) {
        Poll::Fresh(Ok(z)) => (z.clone(), true),
        Poll::Old(Ok(z)) => (z.clone(), false),
        Poll::Fresh(Err(e)) | Poll::Old(Err(e)) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                tr!("paving-ui-error", e = e.as_str()),
            );
            (Vec::new(), false)
        }
        Poll::Busy => {
            counting(ui);
            (Vec::new(), false)
        }
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(if zones.len() >= 2 {
            tr!("paving-ui-zones-count", count = zones.len())
        } else {
            tr!("paving-ui-no-zones")
        });
        if ui
            .button(tr!("paving-ui-split-into-zones-button"))
            .clicked()
        {
            p.open_split_for(session, id, status);
        }
        if ui
            .add_enabled(
                splits > 0,
                egui::Button::new(tr!("paving-ui-remove-the-split-lines")),
            )
            .clicked()
        {
            // The zone settings go with the lines: the person is asked first.
            p.confirm_clear = Some(id);
        }
    });
    if p.confirm_clear == Some(id) {
        ui.group(|ui| {
            ui.label(tr!(
                "paving-ui-remove-the-split-lines-and-the-zone-settings"
            ));
            ui.horizontal(|ui| {
                if ui.button(tr!("paving-ui-remove")).clicked() {
                    p.confirm_clear = None;
                    if !p.blocked(status) {
                        let r = SessionBackend(session).call(
                            "hardscape.plan_clear",
                            serde_json::json!({"id": id, "what": "splits"}),
                        );
                        if let Err(e) = r {
                            *status = tr!("paving-ui-error", e = e);
                        }
                    }
                }
                if ui.button(tr!("paving-ui-cancel")).clicked() {
                    p.confirm_clear = None;
                }
            });
        });
    }
    for z in &zones {
        let open = p.open_zone == Some(z.index);
        let header = egui::CollapsingHeader::new(tr!(
            "paving-ui-zone-row",
            name = z.name.as_str(),
            area = kadakas_units::current().area_text(z.area_m2 * 1e6)
        ))
        .id_salt(("paving-zone", id, z.index))
        .open(Some(open))
        .show(ui, |ui| {
            // The zones of an earlier state are not edited: their numbers may be other zones now.
            ui.add_enabled_ui(fresh, |ui| {
                zone_editor(ui, session, id, p, z, (splits, fresh), status);
            });
        });
        if header.header_response.clicked() {
            p.open_zone = if open { None } else { Some(z.index) };
        }
    }
}

fn zone_editor(
    ui: &mut Ui,
    session: &mut Session,
    id: ObjectId,
    p: &mut PavingUi,
    z: &ZoneShape,
    (splits, fresh): (usize, bool),
    status: &mut String,
) {
    let doc = session.editor.doc();
    let Some(obj) = doc.object(id) else { return };
    let base = ZoneSettings::read(doc, obj, z);
    let field_angle = pattern_of(doc, obj).map_or(0.0, |(_, pl)| pl.angle);
    // What is typed stays between frames, until it is stored.
    let mut edit = match p.zone_edit.take() {
        Some(e) if e.follows(id, z.index, &base) => e,
        _ => ZoneEdit::new(id, z.index, base.clone()),
    };
    let units = kadakas_units::current();
    let mut done = false;
    let s = &mut edit.settings;
    ui.horizontal(|ui| {
        ui.label(tr!("paving-ui-name"));
        let r = ui.add(TextEdit::singleline(&mut s.name).desired_width(140.0));
        done |= r.lost_focus();
    });
    ui.horizontal(|ui| {
        ui.label(tr!("paving-ui-zone-pattern"));
        egui::ComboBox::from_id_salt(("paving-zone-pattern", id, z.index))
            .selected_text(pattern_choice_label(s.pattern))
            .show_ui(ui, |ui| {
                let mut pick = |ui: &mut Ui, c: PatternChoice| {
                    if ui
                        .selectable_label(s.pattern == c, pattern_choice_label(c))
                        .clicked()
                    {
                        s.pattern = c;
                        done = true;
                    }
                };
                pick(ui, PatternChoice::Field);
                for b in Bond::ALL {
                    pick(ui, PatternChoice::Bond(b));
                }
                if base.pattern == PatternChoice::Kept {
                    pick(ui, PatternChoice::Kept);
                }
            });
    });
    if matches!(s.pattern, PatternChoice::Bond(_)) {
        ui.horizontal_wrapped(|ui| {
            for (label, v) in [
                (tr!("joint-ui-unit-length"), &mut s.unit_length),
                (tr!("joint-ui-unit-width"), &mut s.unit_width),
                (tr!("joint-ui-gap"), &mut s.gap),
            ] {
                ui.label(label);
                let r = ui.add(DragValue::new(v).speed(1.0).range(1.0..=10_000.0).length());
                done |= r.drag_stopped() || r.lost_focus();
            }
        });
    }
    // The angle.
    ui.horizontal(|ui| {
        ui.label(tr!("area-ui-pattern-angle"));
        match s.angle {
            Some(mut a) => {
                let r = ui.add(DragValue::new(&mut a).speed(1.0).degrees());
                if r.changed() {
                    s.angle = Some(a);
                }
                done |= r.drag_stopped() || r.lost_focus();
                if ui.button(tr!("paving-ui-use-the-field-s")).clicked() {
                    s.angle = None;
                    done = true;
                }
            }
            None => {
                ui.label(tr!(
                    "paving-ui-as-the-field",
                    angle = units.angle_text(field_angle)
                ));
                if ui.button(tr!("area-ui-set-angle")).clicked() {
                    s.angle = Some(field_angle);
                    done = true;
                }
            }
        }
    });
    // The start.
    ui.horizontal_wrapped(|ui| {
        match s.origin {
            Some(o) => {
                ui.label(tr!(
                    "area-ui-pattern-origin",
                    x = units.length_text_d(o.x, 1),
                    y = units.length_text_d(o.y, 1)
                ));
                if ui.button(tr!("area-ui-clear-origin")).clicked() {
                    s.origin = None;
                    done = true;
                }
            }
            None => {
                ui.label(tr!("paving-ui-origin-as-the-field"));
            }
        }
        if ui.button(tr!("paving-ui-pick-origin")).clicked() {
            p.begin_pick(session, id, PickKind::ZoneOrigin(z.index), status);
        }
    });
    ui.horizontal_wrapped(|ui| {
        match s.align_to_edge {
            Some(n) => {
                ui.label(tr!("paving-ui-aligned-to-edge", number = n + 1));
                if ui.button(tr!("paving-ui-clear-alignment")).clicked() {
                    s.align_to_edge = None;
                    done = true;
                }
            }
            None => {
                ui.label(tr!("paving-ui-not-aligned-to-an-edge"));
            }
        }
        if ui.button(tr!("paving-ui-align-to-edge")).clicked() {
            p.begin_pick(session, id, PickKind::ZoneAlignEdge(z.index), status);
        }
    });
    if splits > 0 {
        ui.horizontal(|ui| {
            ui.label(tr!("paving-ui-align-to-split-line"));
            let text = match s.align_to_split {
                Some(n) => tr!("paving-ui-split-line-number", number = n + 1),
                None => tr!("paving-ui-none"),
            };
            egui::ComboBox::from_id_salt(("paving-zone-split", id, z.index))
                .selected_text(text)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(s.align_to_split.is_none(), tr!("paving-ui-none"))
                        .clicked()
                    {
                        s.align_to_split = None;
                        done = true;
                    }
                    for n in 0..splits {
                        if ui
                            .selectable_label(
                                s.align_to_split == Some(n),
                                tr!("paving-ui-split-line-number", number = n + 1),
                            )
                            .clicked()
                        {
                            s.align_split(n);
                            done = true;
                        }
                    }
                });
        });
    }
    let locked = done && p.blocked(status);
    if edit.step(done, fresh, locked) == split::EditStep::Store {
        // The edit is dropped: the document changes, and the next frame starts from it.
        if let Err(e) = split::store_zone(&mut SessionBackend(session), &edit) {
            *status = tr!("paving-ui-error", e = e);
        }
        return;
    }
    // The typed values stay (locked, or the zones are old), so they can be stored later.
    p.zone_edit = Some(edit);
}

fn pattern_choice_label(c: PatternChoice) -> String {
    match c {
        PatternChoice::Field => tr!("paving-ui-the-pattern-of-the-field"),
        PatternChoice::Bond(Bond::RunningBond) => tr!("joint-ui-bond-running-bond"),
        PatternChoice::Bond(Bond::Stack) => tr!("joint-ui-bond-stack"),
        PatternChoice::Bond(Bond::Herringbone) => tr!("joint-ui-bond-herringbone"),
        PatternChoice::Bond(Bond::BasketWeave) => tr!("joint-ui-bond-basket-weave"),
        PatternChoice::Bond(_) | PatternChoice::Kept => tr!("paving-ui-a-pattern-of-its-own"),
    }
}

// ---------------------------------------------------------------- courses

fn courses_block(
    ui: &mut Ui,
    session: &mut Session,
    id: ObjectId,
    p: &mut PavingUi,
    status: &mut String,
) {
    let rev = session.editor.revision();
    let ctx = ui.ctx().clone();
    let doc = session.editor.doc();
    let max = session.limits().pattern_units;
    let shared = &p.shared;
    let rows = match p.courses.poll(&ctx, rev, id, || {
        let doc = shared.get(doc, rev);
        Box::new(move || course::rows(&doc, id, max))
    }) {
        Poll::Fresh(Ok(r)) | Poll::Old(Ok(r)) => r.clone(),
        Poll::Fresh(Err(e)) | Poll::Old(Err(e)) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                tr!("paving-ui-error", e = e.as_str()),
            );
            Vec::new()
        }
        Poll::Busy => {
            counting(ui);
            Vec::new()
        }
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(tr!("paving-ui-courses-count", count = rows.len()));
        if ui
            .button(tr!("paving-ui-add-a-course"))
            .on_hover_text(tr!(
                "paving-ui-a-row-of-units-along-an-edge-for-example-a-soldier-course-along-a-kerb"
            ))
            .clicked()
        {
            match CourseEditor::new_course(session.editor.doc(), id) {
                Ok(e) => p.course = Some(e),
                Err(e) => *status = e,
            }
        }
    });
    let units = kadakas_units::current();
    for r in &rows {
        ui.horizontal_wrapped(|ui| {
            let mut text = tr!(
                "paving-ui-course-row",
                name = r.name.as_str(),
                units = r.units,
                length = units.length_text_d(r.length_m * 1000.0, 2)
            );
            if let Some((lo, hi)) = r.joints {
                text.push_str(&tr!(
                    "paving-ui-course-joints",
                    narrowest = units.length_text_d(lo, 2),
                    widest = units.length_text_d(hi, 2)
                ));
            }
            ui.label(text);
            if r.editable && ui.small_button(tr!("paving-ui-edit")).clicked() {
                match CourseEditor::edit(session.editor.doc(), id, &r.name) {
                    Ok(e) => p.course = Some(e),
                    Err(e) => *status = e,
                }
            }
            if ui.small_button(tr!("paving-ui-remove")).clicked() && !p.blocked(status) {
                match course::remove(&mut SessionBackend(session), id, &r.name) {
                    Ok(()) => {
                        if p.course.as_ref().is_some_and(|c| c.form.name == r.name) {
                            p.course = None;
                        }
                    }
                    Err(e) => *status = tr!("paving-ui-error", e = e),
                }
            }
        });
    }
    if p.course.as_ref().is_some_and(|c| c.target != id) {
        p.course = None;
    }
    if let Some(mut ed) = p.course.take() {
        // The preview is made again when the pointer is up: not at every step of a drag.
        if !ui.input(|i| i.pointer.any_down()) {
            ed.refresh(&SessionBackend(session));
        }
        let mut keep = true;
        let mut apply = false;
        ui.group(|ui| {
            course_form(ui, session, id, &mut ed);
            course_readout(ui, &ed);
            ui.horizontal(|ui| {
                let ok = ed.preview.is_ok();
                if ui
                    .add_enabled(ok, egui::Button::new(tr!("paving-ui-apply")))
                    .clicked()
                {
                    apply = true;
                }
                if ui.button(tr!("paving-ui-cancel")).clicked() {
                    keep = false;
                }
            });
        });
        if apply && !p.blocked(status) {
            match ed.apply(&mut SessionBackend(session)) {
                Ok(()) => {
                    keep = false;
                    *status = tr!("paving-ui-course-saved", name = ed.form.name.as_str());
                }
                Err(e) => *status = tr!("paving-ui-error", e = e),
            }
        }
        if keep {
            p.course = Some(ed);
        }
    }
}

fn course_form(ui: &mut Ui, session: &Session, id: ObjectId, ed: &mut CourseEditor) {
    let f = &mut ed.form;
    let edges = CourseForm::edges(session.editor.doc(), id);
    ui.horizontal(|ui| {
        ui.label(tr!("paving-ui-name"));
        ui.add_enabled(
            !f.existing,
            TextEdit::singleline(&mut f.name).desired_width(140.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label(tr!("paving-ui-along"));
        egui::ComboBox::from_id_salt(("paving-course-along", id))
            .selected_text(f.along.label())
            .show_ui(ui, |ui| {
                for a in &edges {
                    ui.selectable_value(&mut f.along, *a, a.label());
                }
            });
    });
    // A hole that is gone leaves the course on the outline.
    if !edges.contains(&f.along) {
        f.along = Along::Outline;
    }
    ui.horizontal(|ui| {
        ui.label(tr!("paving-ui-lies"));
        egui::ComboBox::from_id_salt(("paving-course-placing", id))
            .selected_text(f.placing.label())
            .show_ui(ui, |ui| {
                for pl in Placing::ALL {
                    ui.selectable_value(&mut f.placing, pl, pl.label());
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label(tr!("paving-ui-unit"));
        let mut o = f.orientation;
        egui::ComboBox::from_id_salt(("paving-course-orientation", id))
            .selected_text(f.orientation.label())
            .show_ui(ui, |ui| {
                for x in Orientation::ALL {
                    ui.selectable_value(&mut o, x, x.label());
                }
            });
        f.set_orientation(o);
    });
    ui.horizontal_wrapped(|ui| {
        for (label, v, range) in [
            (
                tr!("paving-ui-size-along-the-row"),
                &mut f.unit_length,
                10.0..=10_000.0,
            ),
            (
                tr!("paving-ui-size-across-the-row"),
                &mut f.unit_width,
                10.0..=10_000.0,
            ),
            (tr!("joint-ui-gap"), &mut f.gap, 0.0..=1000.0),
            (tr!("paving-ui-offset"), &mut f.offset, -10_000.0..=10_000.0),
        ] {
            ui.label(label);
            ui.add(DragValue::new(v).speed(1.0).range(range).length());
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr!("paving-ui-corner"));
        egui::ComboBox::from_id_salt(("paving-course-corner", id))
            .selected_text(course::corner_label(f.corner))
            .show_ui(ui, |ui| {
                for c in [
                    kadakas_core::layout::course::Corner::Miter,
                    kadakas_core::layout::course::Corner::Butt,
                    kadakas_core::layout::course::Corner::CornerUnit,
                ] {
                    ui.selectable_value(&mut f.corner, c, course::corner_label(c));
                }
            });
    });
    ui.checkbox(
        &mut f.align_field,
        tr!("paving-ui-start-the-field-at-the-course"),
    )
    .on_hover_text(tr!(
        "paving-ui-moves-the-origin-and-angle-of-the-pattern-so-that-its-first-row-is-whole"
    ));
}

fn course_readout(ui: &mut Ui, ed: &CourseEditor) {
    let units = kadakas_units::current();
    match &ed.preview {
        Err(e) => {
            if !e.is_empty() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    tr!("paving-ui-error", e = e.as_str()),
                );
            }
        }
        Ok(pv) => {
            if let Some(r) = &pv.row {
                ui.label(tr!(
                    "paving-ui-course-preview",
                    units = r.units,
                    corners = r.corners,
                    length = units.length_text_d(r.length_m * 1000.0, 2)
                ));
                if let Some((lo, hi)) = r.joints {
                    ui.label(tr!(
                        "paving-ui-course-joints-range",
                        narrowest = units.length_text_d(lo, 2),
                        widest = units.length_text_d(hi, 2)
                    ));
                }
            }
            for n in &pv.notes {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    tr!("uic-paving-note-api", text = n.as_str()),
                );
            }
        }
    }
}
