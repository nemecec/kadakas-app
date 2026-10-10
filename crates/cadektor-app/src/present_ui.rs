//! The render style and the sun in the view bar.
//!
//! All changes go through the API methods of `cadektor-api` (`render_style.*`, `sun.set`), so they
//! are undoable commands, and an agent sees the same state as the user.

use cadektor_api::Session;
use cadektor_core::present::{RenderMode, RenderStyle, SunSettings};
use cadektor_core::resource::ResourceId;
use serde_json::{Value, json};

/// Applies a built-in render style (`Shaded`, `Realistic`, `Hidden Line`, ...): the style becomes a
/// resource of the document, if it is not one yet, and the active style. The view mode follows it.
pub fn use_builtin_style(session: &mut Session, name: &str) -> Result<(), String> {
    let (preset_name, preset) = RenderStyle::presets()
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("no built-in style '{name}'"))?;
    let doc = session.editor.doc();
    let id = doc
        .resources
        .iter()
        .find(|r| {
            r.name.eq_ignore_ascii_case(preset_name)
                && cadektor_core::present::render_style(doc, r.id).is_some()
        })
        .map(|r| r.id);
    let id = match id {
        Some(id) => id,
        None => {
            let r = session
                .handle(
                    "render_style.create",
                    json!({
                        "name": preset_name,
                        "mode": preset.mode.id(),
                        "edges": preset.edges,
                        "shadows": preset.shadows,
                        "sun": preset.sun,
                        "textures": preset.textures,
                        "background": preset.background.0,
                        "line_weight": preset.line_weight,
                    }),
                )
                .map_err(|e| e.message)?;
            ResourceId(r["id"].as_u64().ok_or("no style id")?)
        }
    };
    set_active(session, id)
}

/// Makes a document style the active one. The view mode follows the style.
pub fn set_active(session: &mut Session, id: ResourceId) -> Result<(), String> {
    session
        .handle("render_style.set_active", json!({"id": id}))
        .map_err(|e| e.message)?;
    if let Some(s) = cadektor_core::present::render_style(session.editor.doc(), id) {
        session.view.mode = s.mode;
    }
    Ok(())
}

/// The active style of the document, created from the built-in Shaded style when there is none.
fn ensure_active(session: &mut Session) -> Result<ResourceId, String> {
    if let Some(id) = session.editor.doc().drawing.presentation.active_style {
        return Ok(id);
    }
    use_builtin_style(session, "Shaded")?;
    session
        .editor
        .doc()
        .drawing
        .presentation
        .active_style
        .ok_or_else(|| "no active style".into())
}

/// Changes one field of the active style (creating the Shaded style first if needed).
pub fn update_active(session: &mut Session, field: &str, value: Value) -> Result<(), String> {
    let id = ensure_active(session)?;
    session
        .handle("render_style.update", json!({"id": id, field: value}))
        .map_err(|e| e.message)?;
    Ok(())
}

/// What the user changed in the sun panel.
#[derive(Clone, Debug, PartialEq)]
pub enum SunEdit {
    Set(Value),
    FromGeoref,
    Off,
}

/// Applies a sun edit through `sun.set`.
pub fn apply_sun(session: &mut Session, edit: SunEdit) -> Result<(), String> {
    let params = match edit {
        SunEdit::Set(v) => v,
        SunEdit::FromGeoref => json!({"from_georef": true}),
        SunEdit::Off => json!({"off": true}),
    };
    session
        .handle("sun.set", params)
        .map(|_| ())
        .map_err(|e| e.message)
}

/// The names for the style list: built-in styles, then the styles of the document.
fn style_names(session: &Session) -> (Vec<&'static str>, Vec<(ResourceId, String)>) {
    let doc = session.editor.doc();
    let builtin: Vec<&'static str> = RenderStyle::presets().into_iter().map(|(n, _)| n).collect();
    let own = doc
        .resources
        .iter()
        .filter(|r| cadektor_core::present::render_style(doc, r.id).is_some())
        .filter(|r| !builtin.iter().any(|b| b.eq_ignore_ascii_case(&r.name)))
        .map(|r| (r.id, r.name.clone()))
        .collect();
    (builtin, own)
}

/// The row below the view bar: render style, its switches and the sun. Returns an error text for
/// the status line, if a change failed.
pub fn show(ui: &mut egui::Ui, session: &mut Session, gpu_ready: bool) -> Option<String> {
    let mut error = None;
    ui.horizontal(|ui| {
        ui.add_enabled_ui(gpu_ready && !session.view.view.is_plan(), |ui| {
            let active = session.editor.doc().drawing.presentation.active_style;
            let style = cadektor_render::styled::resolve_style(session.editor.doc(), None);
            let label = active
                .and_then(|id| session.editor.doc().resources.get(id))
                .map_or("Shaded".to_string(), |r| r.name.clone());
            ui.label("Style");
            let (builtin, own) = style_names(session);
            egui::ComboBox::from_id_salt("render_style")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for name in &builtin {
                        if ui.selectable_label(false, *name).clicked()
                            && let Err(e) = use_builtin_style(session, name)
                        {
                            error = Some(e);
                        }
                    }
                    if !own.is_empty() {
                        ui.separator();
                    }
                    for (id, name) in &own {
                        if ui.selectable_label(active == Some(*id), name).clicked()
                            && let Err(e) = set_active(session, *id)
                        {
                            error = Some(e);
                        }
                    }
                });
            let mut flag = |ui: &mut egui::Ui, text: &str, field: &str, mut on: bool| {
                if ui.checkbox(&mut on, text).changed()
                    && let Err(e) = update_active(session, field, json!(on))
                {
                    error = Some(e);
                }
            };
            if style.mode == RenderMode::Shaded {
                flag(ui, "Edges", "edges", style.edges);
                flag(ui, "Textures", "textures", style.textures);
                flag(ui, "Sun", "sun", style.sun);
                flag(ui, "Shadows", "shadows", style.shadows);
            }
            ui.menu_button("Sun settings", |ui| {
                if let Some(e) = sun_panel(ui, session) {
                    error = Some(e);
                }
            });
        });
    });
    error
}

/// The sun settings: place, date, time and north.
fn sun_panel(ui: &mut egui::Ui, session: &mut Session) -> Option<String> {
    let mut error = None;
    let current = session.editor.doc().drawing.presentation.sun;
    let mut s = current.unwrap_or_default();
    let mut edits: Vec<SunEdit> = Vec::new();
    egui::Grid::new("sun_grid").num_columns(2).show(ui, |ui| {
        let mut num = |ui: &mut egui::Ui,
                       label: &str,
                       key: &str,
                       v: &mut f64,
                       speed: f64,
                       range: std::ops::RangeInclusive<f64>| {
            ui.label(label);
            if ui
                .add(egui::DragValue::new(v).speed(speed).range(range))
                .changed()
            {
                edits.push(SunEdit::Set(json!({ key: *v })));
            }
            ui.end_row();
        };
        num(
            ui,
            "Latitude",
            "latitude",
            &mut s.latitude,
            0.05,
            -90.0..=90.0,
        );
        num(
            ui,
            "Longitude",
            "longitude",
            &mut s.longitude,
            0.05,
            -180.0..=180.0,
        );
        num(
            ui,
            "Time zone (h from UTC)",
            "tz_offset",
            &mut s.tz_offset,
            0.5,
            -12.0..=14.0,
        );
        num(
            ui,
            "North angle",
            "north_angle",
            &mut s.north_angle,
            1.0,
            -360.0..=360.0,
        );
        ui.label("Date");
        ui.horizontal(|ui| {
            let (mut y, mut m, mut d) = (s.year, s.month, s.day);
            let c1 = ui
                .add(egui::DragValue::new(&mut y).range(1900..=2100))
                .changed();
            let c2 = ui.add(egui::DragValue::new(&mut m).range(1..=12)).changed();
            let c3 = ui.add(egui::DragValue::new(&mut d).range(1..=31)).changed();
            if c1 || c2 || c3 {
                // Keep the day inside the month.
                let d = d.min(cadektor_core::sun::days_in_month(y, m));
                edits.push(SunEdit::Set(
                    json!({"date": format!("{y:04}-{m:02}-{d:02}")}),
                ));
            }
        });
        ui.end_row();
        ui.label("Time");
        let mut hour = s.hour;
        if ui
            .add(
                egui::Slider::new(&mut hour, 0.0..=24.0)
                    .custom_formatter(|h, _| cadektor_core::sun::hhmm(h)),
            )
            .changed()
        {
            edits.push(SunEdit::Set(
                json!({"time": cadektor_core::sun::hhmm(hour.min(23.99))}),
            ));
        }
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui.button("From georeference").clicked() {
            edits.push(SunEdit::FromGeoref);
        }
        if ui
            .add_enabled(current.is_some(), egui::Button::new("Remove sun"))
            .clicked()
        {
            edits.push(SunEdit::Off);
        }
    });
    if current.is_none() && edits.is_empty() {
        ui.label("No sun yet. Change a value to set one.");
    } else {
        ui.label(position_text(&s));
    }
    for e in edits {
        if let Err(msg) = apply_sun(session, e) {
            error = Some(msg);
        }
    }
    error
}

/// "Azimuth 180.0, elevation 53.3" for the sun settings, or a note when the sun is down.
pub fn position_text(s: &SunSettings) -> String {
    let p = s.position();
    if p.elevation > 0.0 {
        format!("Azimuth {:.1}, elevation {:.1}", p.azimuth, p.elevation)
    } else {
        format!("Sun below the horizon (elevation {:.1})", p.elevation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_styles_become_document_styles_once() {
        let mut s = Session::default();
        use_builtin_style(&mut s, "Realistic").unwrap();
        let doc = s.editor.doc();
        let active = doc.drawing.presentation.active_style.unwrap();
        let st = cadektor_core::present::render_style(doc, active).unwrap();
        assert!(st.shadows && st.sun);
        assert_eq!(doc.resources.get(active).unwrap().name, "Realistic");
        let count = doc.resources.len();
        // Using it again does not add another resource.
        use_builtin_style(&mut s, "realistic").unwrap();
        assert_eq!(s.editor.doc().resources.len(), count);
        // The view mode follows the style.
        use_builtin_style(&mut s, "Hidden Line").unwrap();
        assert_eq!(s.view.mode, RenderMode::HiddenLine);
        use_builtin_style(&mut s, "Dashed Hidden Line").unwrap();
        assert_eq!(s.view.mode, RenderMode::DashedHiddenLine);
        assert!(use_builtin_style(&mut s, "Sketch").is_err());
        // Going back to a style of the document.
        set_active(&mut s, active).unwrap();
        assert_eq!(s.view.mode, RenderMode::Shaded);
        assert!(set_active(&mut s, ResourceId(777)).is_err());
    }

    #[test]
    fn switches_edit_the_active_style_and_create_one_when_needed() {
        let mut s = Session::default();
        assert!(s.editor.doc().drawing.presentation.active_style.is_none());
        update_active(&mut s, "shadows", json!(true)).unwrap();
        let doc = s.editor.doc();
        let id = doc
            .drawing
            .presentation
            .active_style
            .expect("Shaded was created");
        assert!(
            cadektor_core::present::render_style(doc, id)
                .unwrap()
                .shadows
        );
        update_active(&mut s, "edges", json!(false)).unwrap();
        let st = cadektor_render::styled::resolve_style(s.editor.doc(), None);
        assert!(!st.edges && st.shadows);
        assert!(update_active(&mut s, "edges", json!("maybe")).is_err());
        // Every change is one undo step.
        assert!(s.editor.undo());
        assert!(cadektor_render::styled::resolve_style(s.editor.doc(), None).edges);
    }

    #[test]
    fn sun_edits_use_the_api() {
        let mut s = Session::default();
        apply_sun(
            &mut s,
            SunEdit::Set(json!({"latitude": 60.17, "longitude": 24.94})),
        )
        .unwrap();
        let sun = s.editor.doc().drawing.presentation.sun.unwrap();
        assert_eq!(sun.latitude, 60.17);
        assert_eq!(sun.tz_offset, 2.0);
        assert!(
            apply_sun(&mut s, SunEdit::FromGeoref).is_err(),
            "no georeference"
        );
        assert!(apply_sun(&mut s, SunEdit::Set(json!({"date": "2026-02-31"}))).is_err());
        apply_sun(&mut s, SunEdit::Off).unwrap();
        assert!(s.editor.doc().drawing.presentation.sun.is_none());
        let text = position_text(&SunSettings::default());
        assert!(text.starts_with("Azimuth"), "{text}");
        let night = position_text(&SunSettings {
            hour: 0.0,
            ..Default::default()
        });
        assert!(night.contains("below the horizon"), "{night}");
    }

    #[test]
    fn the_row_draws_and_edits_through_the_session() {
        let ctx = egui::Context::default();
        let mut s = Session::default();
        s.view.view = cadektor_render::StandardView::RightIsometric;
        let mut errors = Vec::new();
        for _ in 0..2 {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 200.0),
                    )),
                    ..Default::default()
                },
                |ui| errors.extend(show(ui, &mut s, true)),
            );
        }
        assert!(errors.is_empty(), "{errors:?}");
        // With a style that has a sun, the sun panel (a menu) also draws.
        let mut errors = Vec::new();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| errors.extend(sun_panel(ui, &mut s)),
        );
        assert!(errors.is_empty());
    }
}
