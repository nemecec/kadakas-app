//! Massing models and map underlays in the app: drawing, property fields and the textures of
//! underlays. The dialogs for map sources are in `gis_dialogs`.

use crate::units_ui::DragValueExt as _;
use kadakas_l10n::tr;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, DragValue, Painter, Pos2, Rect, TextureHandle, Ui};
use glam::DVec2;
use kadakas_core::massing::{Massing, RoofStyle};
use kadakas_core::underlay::Underlay;

use crate::view::{View, ViewExt as _};
use kadakas_ui_core::massing::MassingToolSettings;

/// Longest side of the texture of an underlay in pixels. Bigger rasters are reduced.
const TEXTURE_SIDE: u32 = 2048;

/// A texture with the PNG bytes it was made from (kept so the address stays this raster).
type KeptTexture = (Arc<Vec<u8>>, TextureHandle);

thread_local! {
    /// Textures by the address of the PNG bytes. The bytes are shared between clones of the object.
    static TEXTURES: RefCell<HashMap<usize, KeptTexture>> = RefCell::new(HashMap::new());
}

fn texture(painter: &Painter, u: &Underlay) -> Option<TextureHandle> {
    let key = Arc::as_ptr(&u.png.0) as usize;
    TEXTURES.with(|t| {
        let mut t = t.borrow_mut();
        if let Some((_, h)) = t.get(&key) {
            return Some(h.clone());
        }
        let img = u.decode(TEXTURE_SIDE)?;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width as usize, img.height as usize],
            &img.data,
        );
        let handle = painter
            .ctx()
            .load_texture("underlay", color, egui::TextureOptions::LINEAR);
        // The kept Arc makes sure that the address stays this raster.
        t.insert(key, (u.png.0.clone(), handle.clone()));
        // Rasters that no object uses any more go with the next change of the document.
        if t.len() > 32 {
            t.retain(|_, (bytes, _)| Arc::strong_count(bytes) > 1);
        }
        Some(handle)
    })
}

/// Draws the raster of an underlay. A raster that cannot be read gets a crossed frame.
pub fn draw_underlay(painter: &Painter, view: &View, u: &Underlay, offset: DVec2) {
    let a = view.to_screen(DVec2::new(u.min.x, u.max.y) + offset);
    let b = view.to_screen(DVec2::new(u.max.x, u.min.y) + offset);
    let rect = Rect::from_min_max(a, b);
    match texture(painter, u) {
        Some(tex) => {
            let alpha = (u.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::from_white_alpha(alpha),
            );
        }
        None => {
            let stroke = egui::Stroke::new(1.0, Color32::GRAY);
            painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Middle);
            painter.line_segment([rect.min, rect.max], stroke);
            painter.line_segment([rect.left_bottom(), rect.right_top()], stroke);
        }
    }
}

/// The roof lines of a massing model in screen space, to draw over the outline.
pub fn roof_lines(m: &Massing, view: &View, offset: DVec2) -> Vec<[Pos2; 2]> {
    kadakas_core::massing::roof_lines(m)
        .into_iter()
        .map(|(p, q)| [view.to_screen(p + offset), view.to_screen(q + offset)])
        .collect()
}

fn num(ui: &mut Ui, label: &str, v: &mut f64, speed: f64, suffix: &str) -> bool {
    ui.label(label);
    let drag = DragValue::new(v).speed(speed);
    let r = ui.add(match suffix {
        " mm" => drag.length(),
        "\u{b0}" => drag.degrees(),
        other => drag.suffix(other),
    });
    ui.end_row();
    r.drag_stopped() || r.lost_focus()
}

/// Fields of a massing model for the property panel. Returns true when a change is ready to commit.
pub fn massing_fields(ui: &mut Ui, m: &mut Massing) -> bool {
    let mut commit = false;
    ui.label(tr!("gis-ui-name"));
    commit |= ui.text_edit_singleline(&mut m.name).lost_focus();
    ui.end_row();
    let mut by_floors = m.floors.is_some();
    ui.label(tr!("gis-ui-height-by"));
    ui.horizontal(|ui| {
        let a = ui
            .radio_value(&mut by_floors, false, tr!("gis-ui-height"))
            .changed();
        let b = ui
            .radio_value(&mut by_floors, true, tr!("gis-ui-floors"))
            .changed();
        if a || b {
            m.floors = by_floors.then(|| m.floor_count());
            commit = true;
        }
    });
    ui.end_row();
    if let Some(floors) = &mut m.floors {
        ui.label(tr!("gis-ui-floors"));
        let mut n = *floors as i64;
        let r = ui.add(DragValue::new(&mut n).range(1..=300));
        *floors = n.clamp(1, 300) as u32;
        commit |= r.drag_stopped() || r.lost_focus();
        ui.end_row();
        commit |= num(
            ui,
            &tr!("gis-ui-floor-height"),
            &mut m.floor_height,
            50.0,
            " mm",
        );
    } else {
        commit |= num(ui, &tr!("gis-ui-eave-height"), &mut m.height, 50.0, " mm");
    }
    ui.label(tr!("gis-ui-roof"));
    let before = m.roof;
    egui::ComboBox::from_id_salt("massing_roof")
        .selected_text(m.roof.name())
        .show_ui(ui, |ui| {
            for r in [RoofStyle::Flat, RoofStyle::Gable, RoofStyle::Hip] {
                ui.selectable_value(&mut m.roof, r, r.name());
            }
        });
    commit |= m.roof != before;
    ui.end_row();
    if m.roof != RoofStyle::Flat {
        commit |= num(ui, &tr!("gis-ui-pitch"), &mut m.pitch_deg, 1.0, "\u{b0}");
    }
    ui.label(tr!("gis-ui-base"));
    let mut on_site = m.base.is_none();
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut on_site, tr!("gis-ui-on-the-site-model"))
            .changed()
        {
            m.base = if on_site { None } else { Some(0.0) };
            commit = true;
        }
        if let Some(b) = &mut m.base {
            let r = ui.add(DragValue::new(b).speed(50.0).length());
            commit |= r.drag_stopped() || r.lost_focus();
        }
    });
    ui.end_row();
    if let Some(w) = m.roof_warning() {
        ui.label(
            egui::RichText::new(w)
                .small()
                .color(Color32::from_rgb(190, 110, 0)),
        );
        ui.end_row();
    }
    let q = m.quantities();
    ui.label(tr!("gis-ui-floor-area"));
    ui.label(tr!(
        "gis-ui-area-floors",
        area = kadakas_units::current().area_text_d(q.gross_floor_area, 1),
        floors = q.floors
    ));
    ui.end_row();
    ui.label(tr!("gis-ui-volume"));
    ui.label(kadakas_units::current().volume_text(q.volume));
    ui.end_row();
    commit
}

/// Fields of an underlay for the property panel.
pub fn underlay_fields(ui: &mut Ui, u: &mut Underlay) -> bool {
    let mut commit = false;
    ui.label(tr!("gis-ui-name"));
    commit |= ui.text_edit_singleline(&mut u.name).lost_focus();
    ui.end_row();
    ui.label(tr!("gis-ui-opacity"));
    let r = ui.add(egui::Slider::new(&mut u.opacity, 0.0..=1.0));
    commit |= r.drag_stopped() || r.lost_focus();
    ui.end_row();
    ui.label(tr!("gis-ui-raster"));
    ui.label(tr!("gis-ui-x-px", width = u.width, height = u.height));
    ui.end_row();
    if !u.source.is_empty() {
        ui.label(tr!("gis-ui-source"));
        ui.label(&u.source);
        ui.end_row();
    }
    commit
}

/// The options in the Data bar.
pub fn massing_bar(s: &mut MassingToolSettings, ui: &mut Ui) {
    ui.radio_value(&mut s.by_floors, false, tr!("gis-ui-height"));
    ui.radio_value(&mut s.by_floors, true, tr!("gis-ui-floors"));
    if s.by_floors {
        let mut n = s.floors as i64;
        ui.add(DragValue::new(&mut n).range(1..=300));
        s.floors = n.clamp(1, 300) as u32;
        ui.label("x");
        ui.add(
            DragValue::new(&mut s.floor_height)
                .speed(50.0)
                .range(500.0..=20000.0)
                .length(),
        );
    } else {
        ui.add(
            DragValue::new(&mut s.height)
                .speed(50.0)
                .range(100.0..=500000.0)
                .length(),
        );
    }
    egui::ComboBox::from_id_salt("massing_tool_roof")
        .selected_text(tr!("gis-ui-roof-2", roof = s.roof.name()))
        .show_ui(ui, |ui| {
            for r in [RoofStyle::Flat, RoofStyle::Gable, RoofStyle::Hip] {
                ui.selectable_value(&mut s.roof, r, r.name());
            }
        });
    if s.roof != RoofStyle::Flat {
        ui.add(
            DragValue::new(&mut s.pitch_deg)
                .speed(1.0)
                .range(1.0..=80.0)
                .degrees(),
        );
    }
}

/// The coordinate readout of the Data bar: prepared conversions that follow the georeference.
#[derive(Default)]
pub struct ReadoutState {
    #[cfg(not(target_arch = "wasm32"))]
    reader: Option<(
        kadakas_core::georef::Georeference,
        kadakas_gis::coords::Reader,
    )>,
    /// Show longitude and latitude instead of northing and easting.
    pub lonlat: bool,
}

impl ReadoutState {
    /// The text for the cursor position (model mm), or `None` if the document has no CRS.
    pub fn text(
        &mut self,
        georef: Option<&kadakas_core::georef::Georeference>,
        cursor: DVec2,
    ) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            // The web build has no coordinate conversion.
            let _ = (georef, cursor);
            None
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.text_native(georef, cursor)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn text_native(
        &mut self,
        georef: Option<&kadakas_core::georef::Georeference>,
        cursor: DVec2,
    ) -> Option<String> {
        let g = georef?;
        if self.reader.as_ref().is_none_or(|(old, _)| old != g) {
            self.reader = kadakas_gis::coords::Reader::new(g).map(|r| (g.clone(), r));
        }
        let (_, reader) = self.reader.as_ref()?;
        reader.at(cursor).map(|r| r.text(self.lonlat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::georef::Georeference;

    #[test]
    fn the_readout_follows_the_georeference() {
        let mut r = ReadoutState::default();
        assert!(r.text(None, DVec2::ZERO).is_none());
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        assert_eq!(
            r.text(Some(&g), DVec2::new(1000.0, 2000.0)).unwrap(),
            "N 6672002.000  E 385001.000"
        );
        r.lonlat = true;
        assert!(
            r.text(Some(&g), DVec2::ZERO)
                .unwrap()
                .starts_with("lat 60.")
        );
        // A new georeference is picked up.
        let g2 = Georeference {
            origin: DVec2::new(386_000.0, 6_672_000.0),
            ..g.clone()
        };
        r.lonlat = false;
        assert_eq!(
            r.text(Some(&g2), DVec2::ZERO).unwrap(),
            "N 6672000.000  E 386000.000"
        );
        let none = Georeference {
            crs: String::new(),
            ..g
        };
        assert!(r.text(Some(&none), DVec2::ZERO).is_none());
    }
}
