//! Keyboard shortcuts in the app. The table, the default keys and the file format are in
//! `kadakas_ui_core::keymap`. This module turns egui keys into ui-core keys, and finds and reads
//! the user file.

pub use kadakas_ui_core::keymap::{Action, Chord, Keymap, allowed_in_text_field};
use kadakas_ui_core::keys::{Key, Modifiers};

/// The ui-core key of an egui key. The names are the same in both.
pub fn key(k: egui::Key) -> Option<Key> {
    Key::from_name(k.name())
}

/// The egui key of a ui-core key.
pub fn egui_key(k: Key) -> Option<egui::Key> {
    egui::Key::from_name(k.name())
}

pub fn modifiers(m: egui::Modifiers) -> Modifiers {
    Modifiers {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        command: m.command,
    }
}

/// The action of an egui key event and the chord that matched.
pub fn action_for_event(
    keys: &Keymap,
    k: egui::Key,
    physical: Option<egui::Key>,
    m: egui::Modifiers,
) -> Option<(Action, Chord)> {
    keys.action_for_event(key(k)?, physical.and_then(key), modifiers(m))
}

/// True while the chord of `action` is held down.
pub fn held(keys: &Keymap, action: Action, input: &egui::InputState) -> bool {
    keys.chords(action).iter().any(|c| {
        egui_key(c.key).is_some_and(|k| input.key_down(k))
            && input.modifiers.command == c.command
            && (input.modifiers.ctrl && cfg!(target_os = "macos")) == c.ctrl
            && input.modifiers.shift == c.shift
            && input.modifiers.alt == c.alt
    })
}

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
            vec![format!("keymap file {} ignored: {e}", path.display())],
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
