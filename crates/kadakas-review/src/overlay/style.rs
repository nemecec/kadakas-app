//! The look of the compare overlay: colors, line patterns and marks for each status, in the light
//! and the dark theme. Pure data, no renderer.
//!
//! A person who cannot tell green from red must still read the overlay. So the colors are never
//! the only sign. Each status also has its own line pattern (added: solid and thick, removed:
//! dashed, changed: dotted) and its own mark (`+`, `-`, `~`), and a second palette
//! ([`Palette::ColorBlind`]) uses blue, vermillion and dark yellow, which stay apart for the
//! common kinds of color blindness.

use serde::Serialize;

use super::Status;

/// The set of colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    /// Added green, removed red, changed orange.
    #[default]
    Standard,
    /// Added blue, removed vermillion, changed dark yellow.
    ColorBlind,
}

impl Palette {
    pub const ALL: [Palette; 2] = [Palette::Standard, Palette::ColorBlind];

    pub fn name(self) -> &'static str {
        match self {
            Palette::Standard => "standard",
            Palette::ColorBlind => "color_blind",
        }
    }

    pub fn parse(name: &str) -> Option<Palette> {
        Palette::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// What the overlay does with an object that did not change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnchangedMode {
    /// Draw it as it is.
    Show,
    /// Draw it faint, so that the changes stand out.
    #[default]
    Dim,
    /// Leave it out.
    Hide,
}

impl UnchangedMode {
    pub const ALL: [UnchangedMode; 3] =
        [UnchangedMode::Show, UnchangedMode::Dim, UnchangedMode::Hide];

    pub fn name(self) -> &'static str {
        match self {
            UnchangedMode::Show => "show",
            UnchangedMode::Dim => "dim",
            UnchangedMode::Hide => "hide",
        }
    }

    pub fn parse(name: &str) -> Option<UnchangedMode> {
        UnchangedMode::ALL.into_iter().find(|m| m.name() == name)
    }
}

/// The line pattern of an outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Solid,
    Dashed,
    Dotted,
}

/// How one status is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub rgb: [u8; 3],
    pub pattern: Pattern,
    /// The line weight is multiplied by this.
    pub weight: f32,
    pub symbol: char,
}

/// The look of the overlay. Every field has a default; a person or an agent changes what they
/// need.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompareStyle {
    pub palette: Palette,
    pub unchanged: UnchangedMode,
    /// The colors for the dark theme (a dark paper) instead of the light theme.
    pub dark: bool,
    /// Opacity of a dimmed object, 0 to 1.
    pub dim: f32,
    /// Opacity of a ghost (a removed object), 0 to 1.
    pub ghost: f32,
    /// Draw the old shape of a changed object, faint, behind the new one.
    pub show_previous: bool,
    /// Draw the ghosts of removed objects. Off: removed objects are only counted.
    pub show_ghosts: bool,
}

impl Default for CompareStyle {
    fn default() -> Self {
        CompareStyle {
            palette: Palette::Standard,
            unchanged: UnchangedMode::Dim,
            dark: false,
            dim: 0.25,
            ghost: 0.5,
            show_previous: true,
            show_ghosts: true,
        }
    }
}

impl CompareStyle {
    /// The colors of a status for the theme of this style.
    pub fn color(&self, status: Status) -> [u8; 3] {
        use Palette::*;
        use Status::*;
        match (self.palette, self.dark, status) {
            (Standard, false, Added) => [0, 128, 48],
            (Standard, false, Removed) => [200, 30, 45],
            (Standard, false, Changed) => [205, 102, 0],
            (Standard, true, Added) => [70, 200, 110],
            (Standard, true, Removed) => [255, 95, 105],
            (Standard, true, Changed) => [255, 165, 50],
            (ColorBlind, false, Added) => [0, 114, 178],
            (ColorBlind, false, Removed) => [213, 94, 0],
            (ColorBlind, false, Changed) => [150, 118, 0],
            (ColorBlind, true, Added) => [86, 180, 233],
            (ColorBlind, true, Removed) => [255, 140, 60],
            (ColorBlind, true, Changed) => [240, 228, 66],
        }
    }

    /// The tint of a status.
    pub fn tint(&self, status: Status) -> Tint {
        let (pattern, weight) = match status {
            Status::Added => (Pattern::Solid, 2.0),
            Status::Removed => (Pattern::Dashed, 1.5),
            Status::Changed => (Pattern::Dotted, 2.0),
        };
        Tint {
            rgb: self.color(status),
            pattern,
            weight,
            symbol: status.symbol(),
        }
    }

    /// The paper that the colors are made for.
    pub fn paper(&self) -> [u8; 3] {
        if self.dark {
            [30, 32, 36]
        } else {
            [255, 255, 255]
        }
    }
}
