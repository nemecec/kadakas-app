//! The windows of the paving features: Create Objects from Shapes, Split Hardscape into Zones and
//! the pattern check.

use std::collections::BTreeMap;

use egui::{Context, DragValue, Id, RichText, ScrollArea, Ui, Window};
use glam::DVec2;
use kadakas_api::Session;
use kadakas_core::layout::plan::ZoneUnits;
use kadakas_l10n::tr;
use kadakas_ui_core::paving::check;
use kadakas_ui_core::paving::curves::{CurvesDialog, Make, NoteKind};
use kadakas_ui_core::paving::split::DraftLine;

use super::paint::zone_color;
use super::{PavingUi, SessionBackend};
use crate::units_ui::DragValueExt as _;

/// A box with room round it, for a zoom to a place or a shape.
fn around(lo: DVec2, hi: DVec2) -> (DVec2, DVec2) {
    let room = ((hi - lo).max_element() * 0.15).max(300.0);
    (lo - DVec2::splat(room), hi + DVec2::splat(room))
}

fn error_text(ui: &mut Ui, e: &str) {
    if !e.is_empty() {
        ui.colored_label(ui.visuals().error_fg_color, tr!("paving-ui-error", e = e));
    }
}

/// A length field that applies its value when the person is done, not at every step of a drag.
fn length_field(
    ui: &mut Ui,
    value: &mut f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
) -> bool {
    let mut v = *value;
    let r = ui.add(DragValue::new(&mut v).speed(speed).range(range).length());
    let done = r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged());
    if done && v != *value {
        *value = v;
        return true;
    }
    false
}

// ---------------------------------------------------------------- Create Objects from Shapes

pub(super) fn curves(p: &mut PavingUi, ctx: &Context, session: &mut Session, status: &mut String) {
    let Some(mut d) = p.curves.take() else {
        return;
    };
    d.refresh(&SessionBackend(session));
    let styles: Vec<String> = session
        .editor
        .doc()
        .resources
        .hardscape_styles()
        .map(|(_, n, _)| n.to_string())
        .collect();
    let mut open = true;
    let mut close = false;
    let mut create = false;
    let mut zoom = None;
    Window::new(tr!("paving-ui-create-objects-from-shapes"))
        .id(Id::new("win-paving-curves"))
        .open(&mut open)
        .collapsible(false)
        .default_width(400.0)
        .show(ctx, |ui| {
            let CurvesDialog {
                form, found, ids, ..
            } = &mut d;
            ui.label(tr!("paving-ui-shapes-selected", count = ids.len()));
            ui.horizontal(|ui| {
                ui.label(tr!("paving-ui-close-gaps-up-to"));
                length_field(ui, &mut form.tolerance, 0.1, 0.0..=1000.0);
            })
            .response
            .on_hover_text(tr!("paving-ui-the-widest-gap-between-ends-that-is-closed"));
            ui.horizontal(|ui| {
                ui.label(tr!("paving-ui-make"));
                egui::ComboBox::from_id_salt("paving-curves-make")
                    .selected_text(form.make.label())
                    .show_ui(ui, |ui| {
                        for m in Make::ALL {
                            ui.selectable_value(&mut form.make, m, m.label());
                        }
                    });
            });
            if form.make == Make::Hardscape {
                ui.horizontal(|ui| {
                    ui.label(tr!("area-ui-style"));
                    let text = form
                        .style
                        .clone()
                        .unwrap_or_else(|| tr!("area-ui-none-plain-slab"));
                    egui::ComboBox::from_id_salt("paving-curves-style")
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut form.style,
                                None,
                                tr!("area-ui-none-plain-slab"),
                            );
                            for s in &styles {
                                ui.selectable_value(&mut form.style, Some(s.clone()), s);
                            }
                        });
                });
            }
            ui.checkbox(
                &mut form.delete_source,
                tr!("paving-ui-delete-the-shapes-that-are-used-up"),
            )
            .on_hover_text(tr!(
                "paving-ui-a-shape-goes-only-when-all-its-pieces-are-in-regions-that-are-made"
            ));
            ui.separator();
            match found {
                Err(e) => error_text(ui, e),
                Ok(f) => {
                    ui.strong(tr!("paving-ui-regions-found", count = f.regions.len()));
                    let mut toggles = Vec::new();
                    ScrollArea::vertical()
                        .id_salt("paving-curves-regions")
                        .max_height(150.0)
                        .show(ui, |ui| {
                            for r in &f.regions {
                                ui.horizontal(|ui| {
                                    let mut on = form.is_on(r);
                                    let text = tr!(
                                        "paving-ui-region-row",
                                        number = r.index + 1,
                                        role = r.role.label(),
                                        area = kadakas_units::current().area_text(r.area_m2 * 1e6),
                                        holes = r.holes.len()
                                    );
                                    if ui.checkbox(&mut on, text).changed() {
                                        toggles.push(r.clone());
                                    }
                                    if ui.small_button(tr!("paving-ui-show")).clicked() {
                                        zoom = Some(around(r.lo, r.hi));
                                    }
                                });
                                if let Some(w) = &r.doubt {
                                    ui.label(
                                        RichText::new(tr!("uic-paving-note-api", text = w))
                                            .small()
                                            .color(ui.visuals().warn_fg_color),
                                    );
                                }
                            }
                        });
                    for r in toggles {
                        form.toggle(&r);
                    }
                    if f.regions.is_empty() {
                        ui.label(tr!("paving-ui-no-closed-regions"));
                    }
                    if !f.notes.is_empty() {
                        ui.separator();
                        ui.strong(tr!("paving-ui-report"));
                        ScrollArea::vertical()
                            .id_salt("paving-curves-notes")
                            .max_height(130.0)
                            .show(ui, |ui| {
                                for n in &f.notes {
                                    let color = match n.kind {
                                        NoteKind::DeadEnd
                                        | NoteKind::Warning
                                        | NoteKind::Skipped => ui.visuals().warn_fg_color,
                                        _ => ui.visuals().text_color(),
                                    };
                                    let text = RichText::new(&n.text).color(color);
                                    match n.at {
                                        Some(at) => {
                                            if ui
                                                .selectable_label(false, text)
                                                .on_hover_text(tr!(
                                                    "paving-ui-click-to-show-the-place"
                                                ))
                                                .clicked()
                                            {
                                                zoom = Some(around(
                                                    at - DVec2::splat(200.0),
                                                    at + DVec2::splat(200.0),
                                                ));
                                            }
                                        }
                                        None => {
                                            ui.label(text);
                                        }
                                    }
                                }
                            });
                    }
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                let ready = found.as_ref().is_ok_and(|f| !form.chosen(f).is_empty());
                if ui
                    .add_enabled(ready, egui::Button::new(tr!("paving-ui-create")))
                    .clicked()
                {
                    create = true;
                }
                if ui.button(tr!("paving-ui-cancel")).clicked() {
                    close = true;
                }
            });
        });
    open &= !close;
    if let Some((lo, hi)) = zoom {
        p.zoom_to(lo, hi);
    }
    if create {
        match d.commit(&mut SessionBackend(session)) {
            Ok(made) => {
                *status = tr!(
                    "paving-ui-made-objects",
                    count = made.created.len(),
                    kept = made.kept
                );
                open = false;
            }
            Err(e) => *status = tr!("paving-ui-error", e = e),
        }
    }
    if open {
        p.curves = Some(d);
    }
}

// ---------------------------------------------------------------- Split Hardscape into Zones

pub(super) fn split(p: &mut PavingUi, ctx: &Context, session: &mut Session, status: &mut String) {
    let Some(mut d) = p.split.take() else {
        return;
    };
    d.refresh(&SessionBackend(session));
    let selection = session.selection.clone();
    let doc = session.editor.doc();
    let mut open = true;
    let mut close = false;
    let mut commit = false;
    let mut zoom = None;
    let drawing = &mut p.drawing;
    Window::new(tr!("paving-ui-split-hardscape-into-zones"))
        .id(Id::new("win-paving-split"))
        .open(&mut open)
        .collapsible(false)
        .default_width(380.0)
        .show(ctx, |ui| {
            ui.label(tr!("paving-ui-draw-split-lines-help"));
            ui.horizontal(|ui| {
                ui.selectable_value(drawing, true, tr!("paving-ui-draw-a-split-line"));
                ui.selectable_value(drawing, false, tr!("paving-ui-stop-drawing"));
            });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        d.draft.open.len() >= 2,
                        egui::Button::new(tr!("paving-ui-finish-line")),
                    )
                    .clicked()
                {
                    d.draft.finish_line();
                }
                if ui
                    .add_enabled(
                        !d.draft.open.is_empty(),
                        egui::Button::new(tr!("paving-ui-back")),
                    )
                    .clicked()
                {
                    d.draft.back();
                }
            });
            if ui
                .button(tr!("paving-ui-use-the-selected-lines"))
                .on_hover_text(tr!(
                    "paving-ui-lines-arcs-and-polylines-that-are-selected-become-split-lines"
                ))
                .clicked()
            {
                let n = d.draft.add_objects(doc, &selection);
                *status = tr!("paving-ui-lines-added", count = n);
            }
            ui.separator();
            let mut remove = None;
            for (i, l) in d.draft.lines.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(match l {
                        DraftLine::Object(_) => tr!("paving-ui-split-line-object", number = i + 1),
                        DraftLine::Path(_) => tr!("paving-ui-split-line-drawn", number = i + 1),
                    });
                    if ui.small_button(tr!("paving-ui-remove")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                d.draft.remove(i);
            }
            if d.draft.is_empty() {
                ui.label(tr!("paving-ui-no-split-lines-yet"));
            }
            ui.checkbox(
                &mut d.draft.follow,
                tr!("paving-ui-zones-follow-the-lines-that-were-picked"),
            )
            .on_hover_text(tr!(
                "paving-ui-when-such-a-line-moves-the-zones-move-with-it"
            ));
            ui.horizontal(|ui| {
                ui.label(tr!("paving-ui-line-ends-join-an-edge-within"));
                length_field(ui, &mut d.draft.tolerance, 0.1, 0.01..=100.0);
            });
            ui.horizontal(|ui| {
                ui.label(tr!("paving-ui-units-at-a-zone-edge"));
                egui::ComboBox::from_id_salt("paving-split-units")
                    .selected_text(zone_units_label(d.draft.zone_units))
                    .show_ui(ui, |ui| {
                        for u in [ZoneUnits::Cut, ZoneUnits::Centroid] {
                            ui.selectable_value(&mut d.draft.zone_units, u, zone_units_label(u));
                        }
                    });
            });
            ui.separator();
            match &d.zones {
                Err(e) => error_text(ui, e),
                Ok(zones) => {
                    if !d.draft.is_empty() {
                        ui.strong(tr!("paving-ui-zones-made", count = zones.len()));
                    }
                    for z in zones {
                        ui.horizontal(|ui| {
                            let (r, _) = ui
                                .allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 2.0, zone_color(z.index, 255));
                            ui.label(tr!(
                                "paving-ui-zone-row",
                                name = z.name.as_str(),
                                area = kadakas_units::current().area_text(z.area_m2 * 1e6)
                            ));
                            if ui.small_button(tr!("paving-ui-show")).clicked() {
                                let (mut lo, mut hi) =
                                    (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
                                for pt in z.shapes.iter().flatten().flatten() {
                                    lo = lo.min(*pt);
                                    hi = hi.max(*pt);
                                }
                                if lo.x <= hi.x {
                                    zoom = Some(around(lo, hi));
                                }
                            }
                        });
                    }
                }
            }
            if let Some(r) = &d.report {
                if let Some((lost, extra)) = r.edge_cuts {
                    ui.label(tr!("paving-ui-zone-edges-cut", lost = lost, extra = extra));
                }
                if r.zones >= 2 {
                    ui.label(tr!("paving-ui-zone-edges-have-no-joint"));
                }
                for n in &r.notes {
                    ui.label(
                        RichText::new(tr!("uic-paving-note-api", text = n.as_str()))
                            .color(ui.visuals().warn_fg_color),
                    );
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                let ready = !d.draft.is_empty() && d.zones.as_ref().is_ok_and(|z| z.len() >= 2);
                if ui
                    .add_enabled(ready, egui::Button::new(tr!("paving-ui-split")))
                    .clicked()
                {
                    commit = true;
                }
                if ui.button(tr!("paving-ui-cancel")).clicked() {
                    close = true;
                }
            });
        });
    open &= !close;
    if let Some((lo, hi)) = zoom {
        p.zoom_to(lo, hi);
    }
    if commit {
        match d.commit(&mut SessionBackend(session)) {
            Ok(r) => {
                *status = tr!("paving-ui-split-into-zones", count = r.zones);
                open = false;
                p.zones = None;
            }
            Err(e) => *status = tr!("paving-ui-error", e = e),
        }
    }
    if open {
        p.split = Some(d);
    } else {
        p.drawing = false;
    }
}

fn zone_units_label(u: ZoneUnits) -> String {
    match u {
        ZoneUnits::Cut => tr!("paving-ui-zone-units-cut"),
        ZoneUnits::Centroid => tr!("paving-ui-zone-units-centroid"),
    }
}

// ---------------------------------------------------------------- the check

pub(super) fn check_window(
    p: &mut PavingUi,
    ctx: &Context,
    session: &mut Session,
    status: &mut String,
) {
    if !p.check_open {
        return;
    }
    let mut open = true;
    let mut run = false;
    let mut clicked: Option<(usize, usize)> = None;
    let stale = p.check.stale(&SessionBackend(session));
    Window::new(tr!("paving-ui-check-unit-patterns"))
        .id(Id::new("win-paving-check"))
        .open(&mut open)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(tr!("paving-ui-run-the-check")).clicked() {
                    run = true;
                }
                let mut on = crate::plan_draw::show_check();
                if ui
                    .checkbox(&mut on, tr!("area-ui-show-pattern-check"))
                    .on_hover_text(tr!(
                        "area-ui-the-check-marks-overlaps-small-pieces-wrong-joints-and-holes"
                    ))
                    .changed()
                {
                    crate::plan_draw::set_show_check(on);
                }
            });
            ui.label(tr!(
                "paving-ui-the-check-looks-at-the-selected-hardscapes-or-at-all"
            ));
            match &p.check.result {
                None => {
                    ui.label(tr!("paving-ui-no-result-yet"));
                }
                Some(Err(e)) => error_text(ui, e),
                Some(Ok(r)) => {
                    if stale {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            tr!("paving-ui-the-drawing-changed-run-the-check-again"),
                        );
                    }
                    let n = r.finding_count();
                    if r.ok {
                        ui.label(tr!(
                            "paving-ui-no-findings",
                            objects = r.objects.len(),
                            units = r.units
                        ));
                    } else {
                        ui.strong(tr!(
                            "paving-ui-findings",
                            count = n,
                            objects = r.objects.len(),
                            units = r.units
                        ));
                    }
                    if r.errors() > 0 {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            tr!("paving-ui-could-not-be-checked", count = r.errors()),
                        );
                    }
                    let mut per_kind: BTreeMap<&str, usize> = BTreeMap::new();
                    for o in &r.objects {
                        for (k, c) in &o.counts {
                            *per_kind.entry(k.as_str()).or_default() += c;
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .selectable_label(p.check.only.is_none(), tr!("paving-ui-all-kinds"))
                            .clicked()
                        {
                            p.check.only = None;
                        }
                        for k in check::kinds() {
                            let c = per_kind.get(k).copied().unwrap_or(0);
                            if c == 0 {
                                continue;
                            }
                            let sel = p.check.only.as_deref() == Some(k);
                            if ui
                                .selectable_label(
                                    sel,
                                    tr!(
                                        "paving-ui-kind-count",
                                        kind = check::kind_label(k),
                                        count = c
                                    ),
                                )
                                .clicked()
                            {
                                p.check.only = Some(k.to_string());
                            }
                        }
                    });
                    ui.separator();
                    ScrollArea::vertical()
                        .id_salt("paving-check-list")
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (oi, o) in r.objects.iter().enumerate() {
                                if let Some(e) = &o.error {
                                    ui.colored_label(
                                        ui.visuals().error_fg_color,
                                        tr!(
                                            "paving-ui-object-error",
                                            name = o.name.as_str(),
                                            e = e.as_str()
                                        ),
                                    );
                                    continue;
                                }
                                if o.total() == 0 {
                                    continue;
                                }
                                egui::CollapsingHeader::new(tr!(
                                    "paving-ui-object-findings",
                                    name = o.name.as_str(),
                                    count = o.total()
                                ))
                                .id_salt(("paving-check-object", oi))
                                .default_open(true)
                                .show(ui, |ui| {
                                    for (fi, f) in o.findings.iter().enumerate() {
                                        if !p.check.shows(&f.kind) {
                                            continue;
                                        }
                                        let text = tr!(
                                            "paving-ui-finding-row",
                                            kind = check::kind_label(&f.kind),
                                            size = f.size_text(),
                                            group = f.group.as_str()
                                        );
                                        let sel = p.check.selected == Some((oi, fi));
                                        if ui
                                            .selectable_label(sel, text)
                                            .on_hover_text(tr!(
                                                "uic-paving-note-api",
                                                text = f.message.as_str()
                                            ))
                                            .clicked()
                                        {
                                            clicked = Some((oi, fi));
                                        }
                                    }
                                    if !o.truncated.is_empty() {
                                        ui.label(
                                            RichText::new(tr!(
                                                "paving-ui-the-list-is-cut-the-counts-are-exact"
                                            ))
                                            .small(),
                                        );
                                    }
                                });
                            }
                        });
                }
            }
        });
    if let Some(sel) = clicked {
        p.check.selected = Some(sel);
        if let Some(f) = p.check.current().cloned() {
            let [x0, y0, x1, y1] = f.zoom_box();
            p.zoom_to(DVec2::new(x0, y0), DVec2::new(x1, y1));
            if let Some(id) = f.object {
                session.selection = vec![id];
            }
        }
    }
    if run {
        p.check.run(&mut SessionBackend(session));
        if let Some(Err(e)) = &p.check.result {
            *status = tr!("paving-ui-error", e = e.as_str());
        }
    }
    p.check_open = open;
}
