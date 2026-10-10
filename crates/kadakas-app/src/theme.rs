//! Light and dark themes.
//!
//! The choice (System, Light or Dark) is a user preference. It sets the egui theme for the panels.
//! The drawing canvas follows it: the paper is white in the light theme and dark grey in the dark
//! theme. In the dark theme the pen colors that are nearly black are drawn light (the usual CAD
//! convention for a dark background), so black lines stay visible. The document does not change.

use kadakas_l10n::tr;
use serde::{Deserialize, Serialize};

/// What the user chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Follow the system setting.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> String {
        match self {
            ThemeChoice::System => tr!("theme-system"),
            ThemeChoice::Light => tr!("theme-light"),
            ThemeChoice::Dark => tr!("theme-dark"),
        }
    }

    fn preference(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Sets the egui theme. Call it when the choice changes (and once at the start).
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(choice.preference());
}

/// True if the panels use the dark theme now.
pub fn is_dark(ctx: &egui::Context) -> bool {
    ctx.theme() == egui::Theme::Dark
}

/// Colors of the drawing canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasColors {
    pub paper: [u8; 3],
    pub grid_minor: u8,
    pub grid_major: u8,
    pub axis_x: [u8; 3],
    pub axis_y: [u8; 3],
    /// The paper around a sheet layer page.
    pub desk: u8,
}

pub fn canvas_colors(dark: bool) -> CanvasColors {
    if dark {
        CanvasColors {
            paper: [30, 32, 36],
            grid_minor: 46,
            grid_major: 62,
            axis_x: [170, 90, 90],
            axis_y: [90, 140, 90],
            desk: 20,
        }
    } else {
        CanvasColors {
            paper: [255, 255, 255],
            grid_minor: 228,
            grid_major: 205,
            axis_x: [220, 120, 120],
            axis_y: [120, 170, 120],
            desk: 205,
        }
    }
}

/// Brightness of a color, 0 to 255.
fn luma([r, g, b, _]: [u8; 4]) -> f32 {
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// The color to draw on the canvas for a color of the document. In the dark theme a color that is
/// nearly black becomes light grey and the others stay. The alpha stays.
pub fn canvas_color(c: [u8; 4], dark: bool) -> [u8; 4] {
    if dark && c[3] > 0 && luma(c) < 60.0 {
        [230, 230, 230, c[3]]
    } else {
        c
    }
}

/// The grey of an object on a grey layer or class: the brightness of the color, pulled to the
/// middle, with less opacity.
pub fn grey_color(c: [u8; 4], dark: bool) -> [u8; 4] {
    let l = luma(c);
    let mid = if dark { 140.0 } else { 150.0 };
    let g = (l * 0.4 + mid * 0.6).clamp(0.0, 255.0) as u8;
    [g, g, g, ((c[3] as f32) * 0.55) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_canvas_lightens_black_pens_only() {
        assert_eq!(canvas_color([0, 0, 0, 255], true), [230, 230, 230, 255]);
        assert_eq!(canvas_color([10, 20, 30, 128], true), [230, 230, 230, 128]);
        assert_eq!(canvas_color([200, 30, 30, 255], true), [200, 30, 30, 255]);
        assert_eq!(canvas_color([0, 0, 0, 255], false), [0, 0, 0, 255]);
        // Nothing to draw stays nothing.
        assert_eq!(canvas_color([0, 0, 0, 0], true), [0, 0, 0, 0]);
    }

    #[test]
    fn grey_is_grey_and_fainter() {
        for dark in [false, true] {
            let g = grey_color([220, 40, 40, 255], dark);
            assert_eq!(g[0], g[1]);
            assert_eq!(g[1], g[2]);
            assert!(g[3] < 255 && g[3] > 0);
        }
        assert_eq!(grey_color([1, 2, 3, 0], false)[3], 0);
    }

    #[test]
    fn the_papers_differ_and_choices_have_names() {
        assert_ne!(canvas_colors(true).paper, canvas_colors(false).paper);
        assert!(canvas_colors(true).grid_minor < canvas_colors(true).grid_major);
        assert!(canvas_colors(false).grid_minor > canvas_colors(false).grid_major);
        for c in ThemeChoice::ALL {
            assert!(!c.label().is_empty());
        }
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
        let j = serde_json::to_string(&ThemeChoice::Dark).unwrap();
        assert_eq!(j, "\"dark\"");
    }

    #[test]
    fn applying_a_theme_changes_the_context() {
        let ctx = egui::Context::default();
        apply(&ctx, ThemeChoice::Dark);
        let _ = ctx.run_ui(Default::default(), |_| {});
        assert!(is_dark(&ctx));
        apply(&ctx, ThemeChoice::Light);
        let _ = ctx.run_ui(Default::default(), |_| {});
        assert!(!is_dark(&ctx));
    }
}
