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
    } else if let Some(t) = solid_text(text) {
        t
    } else {
        text.to_string()
    }
}

/// The refusals of the solid tools (MOD-09), without ids. The core text says which input it is
/// about: "the first solid" or "the second solid".
fn solid_text(text: &str) -> Option<String> {
    let which = if text.contains("the second solid") {
        tr!("uic-solid-second")
    } else {
        tr!("uic-solid-first")
    };
    let has = |s: &str| text.contains(s);
    Some(if has("solid is not a closed solid") {
        tr!("uic-solid-refused-open", which = which)
    } else if has("passes through itself") {
        tr!("uic-solid-refused-crossing", which = which)
    } else if has("solid looks inward") {
        tr!("uic-solid-refused-inward", which = which)
    } else if has("solid has no volume") {
        tr!("uic-solid-refused-flat", which = which)
    } else if has("solid has no faces") {
        tr!("uic-solid-refused-empty", which = which)
    } else if has("is stuck to other geometry") {
        tr!("uic-solid-refused-attached", which = which)
    } else if has("would be empty") {
        tr!("uic-solid-refused-empty-result")
    } else if has("could not be cut cleanly") {
        tr!("uic-solid-refused-cut")
    } else if has("the result of the operation is not a closed solid") {
        tr!("uic-solid-refused-result")
    } else if has("a surface model has at most") {
        tr!("uic-solid-refused-limit")
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_of_the_solid_tools_is_known_and_loses_its_ids() {
        use kadakas_core::surface::{
            EdgeId, FaceId, OpenKind, Operand, SolidError, SolidOp, SurfaceError,
        };
        let errors = [
            SolidError::Empty(Operand::First),
            SolidError::Open {
                which: Operand::Second,
                count: 12,
                first: EdgeId(34),
                kind: OpenKind::Border,
            },
            SolidError::SelfCrossing {
                which: Operand::First,
                edge: EdgeId(7),
                face: FaceId(8),
            },
            SolidError::Inverted(Operand::Second),
            SolidError::Flat(Operand::First),
            SolidError::Attached(Operand::Second),
            SolidError::NoResult(SolidOp::Intersect),
            SolidError::Cut("edge 5 still passes through face 6".into()),
            SolidError::NotClosed("edge 9 is open".into()),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for e in errors {
            let core = SurfaceError::Solid(e).to_string();
            let text = localize(&core);
            assert_ne!(text, core, "{core}");
            assert!(!text.chars().any(|c| c.is_ascii_digit()), "{text}");
            assert!(seen.insert(text), "{core}");
        }
        let limit = SurfaceError::Limit {
            what: "faces",
            max: 100,
            count: 101,
        };
        assert_eq!(localize(&limit.to_string()), tr!("uic-solid-refused-limit"));
        // The second solid is told from the first.
        let second = SurfaceError::Solid(SolidError::Flat(Operand::Second)).to_string();
        assert_eq!(
            localize(&second),
            tr!("uic-solid-refused-flat", which = tr!("uic-solid-second"))
        );
    }

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
