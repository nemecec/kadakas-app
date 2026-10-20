//! Keyboard input for the UI programs: the keys of egui turned into the keys of
//! `kadakas-ui-core`, and the lookup of the key action of an event.

use kadakas_ui_core::keymap::{Action, Chord, Keymap};
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
    let m = modifiers(input.modifiers);
    keys.chords(action).iter().any(|c| {
        c.matches(
            |k| egui_key(k).is_some_and(|k| input.key_down(k)),
            m,
            cfg!(target_os = "macos"),
        )
    })
}
