//! `kadakas-modeler`: the 3D-first modeler of Kadakas (PLAT-11, MOD-01 and MOD-02), a second UI
//! flavor next to `kadakas-app`. One window with a large 3D view, a tool bar, the Measurements
//! box and a tray with Entity Info, Materials and Scenes. It opens and saves the same `.kad` files
//! as the Vectorworks-like app and draws them with the same 3D renderer. See `docs/modeler.md`.
//!
//! The logic is in `kadakas-ui-core` (`nav3d`, `modeler`, the SketchUp-like profile); this crate
//! only reads the pointer and the keys, and draws.

// A release build on Windows has no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod about;
mod app;
mod canvas;
mod edit3d;
mod files;
mod icons;
mod launch;
mod menu;
mod prefs;
mod soften;
mod statusbar;
mod tags_panel;
mod toolbar;
mod tray;

#[cfg(test)]
mod tests;

use kadakas_l10n::tr;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    // The variables of the launcher leave the environment first, with the secrets.
    launch::capture_env();
    launch::install_crash_capture();
    kadakas_sys::process::remove_secrets_from_env();
    // The language is needed for the title of the window.
    let (prefs, warning) = prefs::load();
    prefs.apply_language(&prefs::system_languages());
    let files = launch::files_from_args(&args);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title(tr!("modeler-window-title"))
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Kadakas Modeler",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::ModelerApp::new(
                cc,
                &files,
                prefs.clone(),
                warning.clone(),
            )))
        }),
    )
}

/// The window icon: the juniper icon of Kadakas (`packaging/icons`). An icon that cannot be
/// decoded gives an empty icon (the system default), never a crash.
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icons/256x256.png"))
        .unwrap_or_default()
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn window_icon_is_the_juniper_icon() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        // Not blank: the picture has opaque and see-through pixels.
        assert!(icon.rgba.chunks(4).any(|p| p[3] == 255));
        assert!(icon.rgba.chunks(4).any(|p| p[3] < 255));
    }
}
