//! Massing models and map underlays in the app: drawing, property fields and the textures of
//! underlays. The dialogs for map sources are in `gis_dialogs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use cadektor_core::massing::{Massing, RoofStyle};
use cadektor_core::underlay::Underlay;
use egui::{Color32, DragValue, Painter, Pos2, Rect, TextureHandle, Ui};
use glam::DVec2;

use crate::view::View;

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
    cadektor_core::massing::roof_lines(m)
        .into_iter()
        .map(|(p, q)| [view.to_screen(p + offset), view.to_screen(q + offset)])
        .collect()
}

fn num(ui: &mut Ui, label: &str, v: &mut f64, speed: f64, suffix: &str) -> bool {
    ui.label(label);
    let r = ui.add(DragValue::new(v).speed(speed).suffix(suffix));
    ui.end_row();
    r.drag_stopped() || r.lost_focus()
}

/// Fields of a massing model for the property panel. Returns true when a change is ready to commit.
pub fn massing_fields(ui: &mut Ui, m: &mut Massing) -> bool {
    let mut commit = false;
    ui.label("Name");
    commit |= ui.text_edit_singleline(&mut m.name).lost_focus();
    ui.end_row();
    let mut by_floors = m.floors.is_some();
    ui.label("Height by");
    ui.horizontal(|ui| {
        let a = ui.radio_value(&mut by_floors, false, "Height").changed();
        let b = ui.radio_value(&mut by_floors, true, "Floors").changed();
        if a || b {
            m.floors = by_floors.then(|| m.floor_count());
            commit = true;
        }
    });
    ui.end_row();
    if let Some(floors) = &mut m.floors {
        ui.label("Floors");
        let mut n = *floors as i64;
        let r = ui.add(DragValue::new(&mut n).range(1..=300));
        *floors = n.clamp(1, 300) as u32;
        commit |= r.drag_stopped() || r.lost_focus();
        ui.end_row();
        commit |= num(ui, "Floor height", &mut m.floor_height, 50.0, " mm");
    } else {
        commit |= num(ui, "Eave height", &mut m.height, 50.0, " mm");
    }
    ui.label("Roof");
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
        commit |= num(ui, "Pitch", &mut m.pitch_deg, 1.0, "\u{b0}");
    }
    ui.label("Base");
    let mut on_site = m.base.is_none();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut on_site, "On the site model").changed() {
            m.base = if on_site { None } else { Some(0.0) };
            commit = true;
        }
        if let Some(b) = &mut m.base {
            let r = ui.add(DragValue::new(b).speed(50.0).suffix(" mm"));
            commit |= r.drag_stopped() || r.lost_focus();
        }
    });
    ui.end_row();
    let q = m.quantities();
    ui.label("Floor area");
    ui.label(format!(
        "{:.1} m2 ({} floors)",
        q.gross_floor_area / 1e6,
        q.floors
    ));
    ui.end_row();
    ui.label("Volume");
    ui.label(format!("{:.1} m3", q.volume / 1e9));
    ui.end_row();
    commit
}

/// Fields of an underlay for the property panel.
pub fn underlay_fields(ui: &mut Ui, u: &mut Underlay) -> bool {
    let mut commit = false;
    ui.label("Name");
    commit |= ui.text_edit_singleline(&mut u.name).lost_focus();
    ui.end_row();
    ui.label("Opacity");
    let r = ui.add(egui::Slider::new(&mut u.opacity, 0.0..=1.0));
    commit |= r.drag_stopped() || r.lost_focus();
    ui.end_row();
    ui.label("Raster");
    ui.label(format!("{} x {} px", u.width, u.height));
    ui.end_row();
    if !u.source.is_empty() {
        ui.label("Source");
        ui.label(&u.source);
        ui.end_row();
    }
    commit
}

/// Settings of the Massing Model tool.
#[derive(Clone, Debug, PartialEq)]
pub struct MassingToolSettings {
    /// True: the height is floors times the floor height. False: an eave height.
    pub by_floors: bool,
    pub height: f64,
    pub floors: u32,
    pub floor_height: f64,
    pub roof: RoofStyle,
    pub pitch_deg: f64,
}

impl Default for MassingToolSettings {
    fn default() -> Self {
        MassingToolSettings {
            by_floors: true,
            height: cadektor_core::massing::DEFAULT_HEIGHT,
            floors: 2,
            floor_height: cadektor_core::massing::DEFAULT_FLOOR_HEIGHT,
            roof: RoofStyle::Flat,
            pitch_deg: cadektor_core::massing::DEFAULT_PITCH_DEG,
        }
    }
}

impl MassingToolSettings {
    /// The massing model for a footprint drawn with the tool.
    pub fn massing(&self, footprint: Vec<cadektor_core::curve::Vertex>) -> Massing {
        let mut m = Massing::new(footprint, self.height.max(1.0));
        m.floors = self.by_floors.then_some(self.floors.max(1));
        m.floor_height = self.floor_height.max(1.0);
        m.roof = self.roof;
        m.pitch_deg = self
            .pitch_deg
            .clamp(0.0, cadektor_core::massing::MAX_PITCH_DEG);
        m
    }

    /// The options in the Data bar.
    pub fn bar(&mut self, ui: &mut Ui) {
        ui.radio_value(&mut self.by_floors, false, "Height");
        ui.radio_value(&mut self.by_floors, true, "Floors");
        if self.by_floors {
            let mut n = self.floors as i64;
            ui.add(DragValue::new(&mut n).range(1..=300));
            self.floors = n.clamp(1, 300) as u32;
            ui.label("x");
            ui.add(
                DragValue::new(&mut self.floor_height)
                    .speed(50.0)
                    .range(500.0..=20000.0)
                    .suffix(" mm"),
            );
        } else {
            ui.add(
                DragValue::new(&mut self.height)
                    .speed(50.0)
                    .range(100.0..=500000.0)
                    .suffix(" mm"),
            );
        }
        egui::ComboBox::from_id_salt("massing_tool_roof")
            .selected_text(format!("Roof: {}", self.roof.name()))
            .show_ui(ui, |ui| {
                for r in [RoofStyle::Flat, RoofStyle::Gable, RoofStyle::Hip] {
                    ui.selectable_value(&mut self.roof, r, r.name());
                }
            });
        if self.roof != RoofStyle::Flat {
            ui.add(
                DragValue::new(&mut self.pitch_deg)
                    .speed(1.0)
                    .range(1.0..=80.0)
                    .suffix("\u{b0}"),
            );
        }
    }
}

/// The coordinate readout of the Data bar: prepared conversions that follow the georeference.
#[derive(Default)]
pub struct ReadoutState {
    reader: Option<(
        cadektor_core::georef::Georeference,
        cadektor_gis::coords::Reader,
    )>,
    /// Show longitude and latitude instead of northing and easting.
    pub lonlat: bool,
}

impl ReadoutState {
    /// The text for the cursor position (model mm), or `None` if the document has no CRS.
    pub fn text(
        &mut self,
        georef: Option<&cadektor_core::georef::Georeference>,
        cursor: DVec2,
    ) -> Option<String> {
        let g = georef?;
        if self.reader.as_ref().is_none_or(|(old, _)| old != g) {
            self.reader = cadektor_gis::coords::Reader::new(g).map(|r| (g.clone(), r));
        }
        let (_, reader) = self.reader.as_ref()?;
        reader.at(cursor).map(|r| r.text(self.lonlat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::curve::corners;
    use cadektor_core::georef::Georeference;

    #[test]
    fn tool_settings_make_a_valid_model() {
        let ring = corners(&[
            DVec2::ZERO,
            DVec2::new(5000.0, 0.0),
            DVec2::new(5000.0, 4000.0),
        ]);
        let mut s = MassingToolSettings::default();
        let m = s.massing(ring.clone());
        assert_eq!((m.floors, m.roof), (Some(2), RoofStyle::Flat));
        assert!(m.problem().is_none());
        s.by_floors = false;
        s.roof = RoofStyle::Hip;
        s.pitch_deg = 200.0;
        s.height = 0.0;
        let m = s.massing(ring);
        assert_eq!(
            (m.floors, m.pitch_deg),
            (None, cadektor_core::massing::MAX_PITCH_DEG)
        );
        assert!(m.height >= 1.0);
    }

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
