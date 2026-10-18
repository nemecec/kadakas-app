//! A test helper for the rule "every text in the UI is localised" (PLAT-05). A test draws a part of
//! the UI with egui in Estonian, reads the texts that egui painted, and asks which of them are
//! English messages that have another text in Estonian. Such a text came from the core crates
//! (or from a literal) and did not go through `tr!`. The source scan in
//! `crates/kadakas-l10n/tests/l10n_check.rs` finds literals in calls; this probe finds the rest.

use std::collections::BTreeMap;

use egui::epaint::Shape;

/// The texts that egui painted in a frame.
pub fn shown_text(out: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Text(t) => out.push(t.galley.text().to_string()),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut texts);
    }
    texts
}

/// The English texts (of messages without variables) whose Estonian text is another one, with
/// the id of the message.
fn english_only_texts() -> BTreeMap<String, String> {
    use std::sync::OnceLock;
    static TEXTS: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    TEXTS
        .get_or_init(|| {
            let ids: Vec<String> = kadakas_l10n::message_ids(kadakas_l10n::FALLBACK)
                .unwrap()
                .into_iter()
                .collect();
            let vars = kadakas_l10n::message_variables(kadakas_l10n::FALLBACK).unwrap();
            let ids: Vec<String> = ids
                .into_iter()
                .filter(|id| vars.get(id).is_none_or(|v| v.is_empty()))
                .collect();
            // One switch of the language for all messages: a switch is slow.
            let all = |tag: &str| -> Vec<String> {
                kadakas_l10n::with_language(tag, || {
                    ids.iter()
                        .map(|id| kadakas_l10n::translate(id, &[]))
                        .collect()
                })
            };
            let (en, et) = (all("en-US"), all("et"));
            let mut map = BTreeMap::new();
            for ((id, en), et) in ids.iter().zip(en).zip(et) {
                if en != et && en.chars().filter(|c| c.is_alphabetic()).count() >= 3 {
                    map.insert(en, id.clone());
                }
            }
            map
        })
        .clone()
}

/// The shown texts that are the English text of a message that Estonian translates. Run it while
/// the language is Estonian (`kadakas_l10n::with_language("et", ..)`), and give it the texts of
/// that frame.
pub fn english_leaks(texts: &[String]) -> Vec<String> {
    let english = english_only_texts();
    let mut leaks = Vec::new();
    for t in texts {
        // A text can hold several lines or parts; test the whole text and each line.
        for part in std::iter::once(t.as_str()).chain(t.lines()) {
            let part = part.trim();
            if let Some(id) = english.get(part) {
                leaks.push(format!("{part:?} (the English text of {id})"));
            }
        }
    }
    leaks.sort();
    leaks.dedup();
    leaks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_finds_an_english_text_and_not_an_estonian_one() {
        // "Top/Plan" is English; the Estonian text is another one.
        let english = vec!["Top/Plan".to_string(), "x".to_string()];
        assert_eq!(english_leaks(&english).len(), 1);
        let estonian = vec![kadakas_l10n::with_language("et", || {
            kadakas_l10n::translate("enum-view-plan", &[])
        })];
        assert!(english_leaks(&estonian).is_empty());
    }

    #[test]
    fn a_painted_text_is_read_from_the_frame() {
        let ctx = egui::Context::default();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("Hello probe");
        });
        assert!(shown_text(&out).iter().any(|t| t == "Hello probe"));
    }
}
