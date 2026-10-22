//! The refusals and notes of the MOD-08 edits for the status line: the core gives English text
//! with ids (for agents and logs); the window shows the same reason in the language of the UI,
//! without internal ids. A text that is not known here is shown as it is.

use kadakas_l10n::tr;

/// The number in "at most about 12.5 mm" of a core message.
fn at_most(text: &str) -> Option<String> {
    let rest = &text[text.find("at most about ")? + "at most about ".len()..];
    let end = rest.find(" mm")?;
    Some(rest[..end].to_string())
}

/// The text of a refusal or a note of an edit, for the person.
pub fn localize(text: &str) -> String {
    let has = |s: &str| text.contains(s);
    if has("is not fully inside it") {
        tr!("uic-modeling-hole-partial")
    } else if has("lies back to back under the") {
        tr!("uic-modeling-hole-stacked")
    } else if has("was pushed through face") {
        tr!("uic-modeling-hole-cap-note")
    } else if has("the offset of") && has("does not fit") {
        match at_most(text) {
            Some(most) => tr!("uic-modeling-offset-too-large", most = most),
            None => tr!("uic-modeling-offset-no-room"),
        }
    } else if has("the profile is too wide for the turn") {
        tr!(
            "uic-modeling-follow-too-wide",
            most = at_most(text).unwrap_or_default()
        )
    } else if has("the profile lies along the path") {
        tr!("uic-modeling-follow-along")
    } else if has("turns back on itself") {
        tr!("uic-modeling-follow-turns-back")
    } else if has("does not bring the profile back") {
        tr!("uic-modeling-follow-not-closed")
    } else if has("erase the face inside the hole first") {
        tr!("uic-modeling-follow-hole-face")
    } else if has("was not at a corner of the closed path") {
        tr!("uic-modeling-follow-moved-note")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_refusals_lose_their_ids_and_keep_their_numbers() {
        let t = localize(
            "the command failed: face 12 reaches the opposite face 7 but is not fully inside it",
        );
        assert_eq!(t, tr!("uic-modeling-hole-partial"));
        assert!(!t.contains("12"));
        let t =
            localize("the offset of 600 mm does not fit face 3: x; at most about 499.99 mm fits");
        assert_eq!(t, tr!("uic-modeling-offset-too-large", most = "499.99"));
        assert_eq!(localize("something else"), "something else");
    }
}
