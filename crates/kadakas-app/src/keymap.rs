//! Keyboard shortcuts in the app. The table, the default keys and the file format are in
//! `kadakas_ui_core::keymap`. This module turns egui keys into ui-core keys, and finds and reads
//! the user file.

use kadakas_l10n::tr;
pub use kadakas_ui_core::keymap::{Action, Keymap, allowed_in_text_field};

#[cfg(test)]
use kadakas_view3d::input::egui_key;
pub use kadakas_view3d::input::{action_for_event, held, key, modifiers};

/// Loads the user file if it exists.
pub fn load() -> (Keymap, Vec<String>) {
    let Some(path) = user_file() else {
        return (Keymap::defaults(), vec![]);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => Keymap::with_overrides(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Keymap::defaults(), vec![]),
        Err(e) => (
            Keymap::defaults(),
            vec![tr!(
                "keymap-keymap-file-ignored",
                path = path.display(),
                e = e
            )],
        ),
    }
}

/// `keymap.json` in the per-user config directory. `KADAKAS_KEYMAP` overrides the path.
pub fn user_file() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("KADAKAS_KEYMAP").filter(|v| !v.is_empty()) {
        return Some(std::path::PathBuf::from(p));
    }
    Some(kadakas_sys::dirs::config_dir()?.join("keymap.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_egui_key_converts_both_ways() {
        for k in egui::Key::ALL {
            let ours = key(*k).expect("same key names");
            assert_eq!(egui_key(ours), Some(*k));
        }
    }

    #[test]
    fn an_egui_event_finds_its_action() {
        let km = Keymap::defaults();
        let none = egui::Modifiers::NONE;
        let hit = action_for_event(&km, egui::Key::Num2, Some(egui::Key::Num2), none);
        assert_eq!(hit.map(|x| x.0), Some(Action::ToolLine));
        let shift = egui::Modifiers {
            alt: true,
            shift: true,
            ..none
        };
        let hit = action_for_event(
            &km,
            egui::Key::Exclamationmark,
            Some(egui::Key::Num1),
            shift,
        );
        assert_eq!(hit.map(|x| x.0), Some(Action::ViewLeftIso));
    }

    #[test]
    fn user_file_is_keymap_json() {
        // The folders per platform are tested in `kadakas_sys::dirs`.
        if let Some(p) = user_file() {
            assert!(
                p.to_string_lossy().ends_with("keymap.json")
                    || std::env::var_os("KADAKAS_KEYMAP").is_some()
            );
        }
    }
}
