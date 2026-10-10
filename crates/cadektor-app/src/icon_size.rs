//! The icon size preference and the size of each kind of icon, in points.
//!
//! The user picks Small, Medium or Large. Palettes (tool palette and tool sets) use 16, 24 or 32
//! points. The view bar and the Object Info palette use 16, 20 or 24. Menus and lists always use
//! 16. The app stores the choice in the egui context each frame, so the icon code needs no extra
//! arguments for it.

use serde::{Deserialize, Serialize};

/// The icon size that the user chose in the preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// Where an icon is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconRole {
    /// The tool palette and the tool sets.
    Palette,
    /// The view bar and the Object Info palette.
    Bar,
    /// Menus and lists.
    Menu,
}

impl IconSize {
    pub const ALL: [IconSize; 3] = [IconSize::Small, IconSize::Medium, IconSize::Large];

    pub fn label(self) -> &'static str {
        match self {
            IconSize::Small => "Small",
            IconSize::Medium => "Medium",
            IconSize::Large => "Large",
        }
    }

    /// The icon size in points for `role`.
    pub fn points(self, role: IconRole) -> f32 {
        match (role, self) {
            (IconRole::Menu, _) => 16.0,
            (IconRole::Palette, IconSize::Small) => 16.0,
            (IconRole::Palette, IconSize::Medium) => 24.0,
            (IconRole::Palette, IconSize::Large) => 32.0,
            (IconRole::Bar, IconSize::Small) => 16.0,
            (IconRole::Bar, IconSize::Medium) => 20.0,
            (IconRole::Bar, IconSize::Large) => 24.0,
        }
    }
}

/// Stores the icon size for this frame.
pub fn set(ctx: &egui::Context, size: IconSize) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("icon-size"), size));
}

/// The icon size for this frame (Medium until the app sets it).
pub fn current(ctx: &egui::Context) -> IconSize {
    ctx.data(|d| d.get_temp(egui::Id::new("icon-size")))
        .unwrap_or_default()
}

/// The icon size in points for `role`, from the stored choice.
pub fn points(ctx: &egui::Context, role: IconRole) -> f32 {
    current(ctx).points(role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_table() {
        use IconRole::*;
        let rows = [
            (IconSize::Small, 16.0, 16.0),
            (IconSize::Medium, 24.0, 20.0),
            (IconSize::Large, 32.0, 24.0),
        ];
        for (s, palette, bar) in rows {
            assert_eq!(s.points(Palette), palette);
            assert_eq!(s.points(Bar), bar);
            assert_eq!(s.points(Menu), 16.0);
        }
        assert_eq!(IconSize::default(), IconSize::Medium);
    }

    #[test]
    fn the_context_keeps_the_choice() {
        let ctx = egui::Context::default();
        assert_eq!(current(&ctx), IconSize::Medium);
        assert_eq!(points(&ctx, IconRole::Palette), 24.0);
        set(&ctx, IconSize::Large);
        assert_eq!(points(&ctx, IconRole::Palette), 32.0);
        assert_eq!(points(&ctx, IconRole::Menu), 16.0);
    }

    #[test]
    fn the_choice_is_stored_as_text() {
        let text = serde_json::to_string(&IconSize::Large).unwrap();
        assert_eq!(text, "\"large\"");
        assert_eq!(
            serde_json::from_str::<IconSize>("\"small\"").unwrap(),
            IconSize::Small
        );
    }

    #[test]
    fn textures_differ_by_pixel_size() {
        // The icon cache key holds the pixel size, so a new size gets a new texture.
        let ctx = egui::Context::default();
        let a = crate::icons::texture(&ctx, "line", 24.0).unwrap();
        let b = crate::icons::texture(&ctx, "line", 32.0).unwrap();
        assert_eq!(a.size(), [24, 24]);
        assert_eq!(b.size(), [32, 32]);
    }
}
