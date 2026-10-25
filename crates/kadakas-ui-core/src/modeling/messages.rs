//! The refusals and notes of the MOD-08 edits for the status line: the core gives English text
//! with ids (for agents and logs); the window shows the same reason in the language of the UI,
//! without internal ids. A text that is not known here is shown as it is.

use kadakas_core::surface::{CurveError, Operand, SolidError, SurfaceError};
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
    if has("lost its circle description") {
        tr!("uic-curve-note-dropped")
    } else if has("its start and end angle are updated") {
        tr!("uic-curve-note-angles")
    } else if has(" is now an arc") || has(" is now a circle") {
        tr!(
            "uic-curve-note-kind",
            kind = if has("arc") { "arc" } else { "circle" }
        )
    } else if has("no curve was made") {
        tr!("uic-curve-note-not-made")
    } else if has("has no circle description") {
        tr!("uic-curve-refused-no-description")
    } else if has("the number of segments must be from") {
        tr!("uic-curve-refused-range")
    } else if has("has other edges, so the chain cannot be rebuilt") {
        tr!("uic-curve-refused-shared")
    } else if has("is not in the plane of the circle") {
        tr!("uic-curve-refused-plane")
    } else if has("in a way that cannot be rebuilt") {
        tr!("uic-curve-refused-loop")
    } else if has("the new chain would make the model invalid") {
        tr!("uic-curve-refused-invalid")
    } else if has("is not fully inside it") {
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

/// The text of a refusal that is a [`SurfaceError`], in the language of the window and without ids.
/// The solid tools (MOD-09) and the limits are told apart by the kind of the error, not by its
/// English text. Other errors go through [`localize`].
pub fn localize_error(e: &SurfaceError) -> String {
    match e {
        SurfaceError::Solid(s) => solid_text(s),
        SurfaceError::Curve(c) => curve_text(c),
        SurfaceError::Limit { .. } => tr!("uic-solid-refused-limit"),
        other => localize(&other.to_string()),
    }
}

/// The refusal of a curve edit (MOD-15), without ids.
fn curve_text(e: &CurveError) -> String {
    match e {
        CurveError::NoCurve(_) => tr!("uic-curve-refused-no-curve"),
        CurveError::NoDescription(_) => tr!("uic-curve-refused-no-description"),
        CurveError::Segments { .. } => tr!("uic-curve-refused-range"),
        CurveError::VertexShared { .. } => tr!("uic-curve-refused-shared"),
        CurveError::BordersOtherPlane { .. } => tr!("uic-curve-refused-plane"),
        CurveError::LoopUnclear { .. } => tr!("uic-curve-refused-loop"),
        CurveError::Invalid { .. } => tr!("uic-curve-refused-invalid"),
    }
}

fn operand_key(which: Operand) -> &'static str {
    match which {
        Operand::First => "first",
        Operand::Second => "second",
    }
}

/// The refusal of a solid operation.
fn solid_text(e: &SolidError) -> String {
    match e {
        SolidError::Empty(w) => tr!("uic-solid-refused-empty", which = operand_key(*w)),
        SolidError::Open { which, .. } => {
            tr!("uic-solid-refused-open", which = operand_key(*which))
        }
        SolidError::SelfCrossing { which, .. } => {
            tr!("uic-solid-refused-crossing", which = operand_key(*which))
        }
        SolidError::Inverted(w) => tr!("uic-solid-refused-inward", which = operand_key(*w)),
        SolidError::Flat(w) => tr!("uic-solid-refused-flat", which = operand_key(*w)),
        SolidError::Attached(w) => tr!("uic-solid-refused-attached", which = operand_key(*w)),
        SolidError::NoResult(_) => tr!("uic-solid-refused-empty-result"),
        SolidError::Cut(_) => tr!("uic-solid-refused-cut"),
        SolidError::ResultOpen(_) => tr!("uic-solid-refused-result"),
        SolidError::ResultVolume => tr!("uic-solid-refused-volume"),
        SolidError::ResultInvalid(_) => tr!("uic-solid-refused-invalid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_of_the_solid_tools_is_known_and_loses_its_ids() {
        use kadakas_core::surface::{EdgeId, FaceId, OpenKind, SolidOp};
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
            SolidError::ResultOpen("edge 9 is open".into()),
            SolidError::ResultVolume,
            SolidError::ResultInvalid("edge 3 crosses edge 4".into()),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for e in errors {
            let err = SurfaceError::Solid(e);
            let core = err.to_string();
            let text = localize_error(&err);
            assert!(!text.chars().any(|c| c.is_ascii_digit()), "{text}");
            assert!(!text.contains("{ "), "{text}");
            assert!(seen.insert(text), "{core}");
        }
        let limit = SurfaceError::Limit {
            what: "faces",
            max: 100,
            count: 101,
        };
        assert_eq!(localize_error(&limit), tr!("uic-solid-refused-limit"));
        // The second solid is told from the first, and an error of another kind goes on as text.
        let second = SurfaceError::Solid(SolidError::Flat(Operand::Second));
        assert_eq!(
            localize_error(&second),
            tr!("uic-solid-refused-flat", which = "second")
        );
        assert_ne!(
            localize_error(&second),
            localize_error(&SurfaceError::Solid(SolidError::Flat(Operand::First)))
        );
        assert_eq!(
            localize_error(&SurfaceError::NoFace(FaceId(3))),
            "face 3 does not exist"
        );
    }

    #[test]
    fn every_refusal_of_a_curve_edit_is_known_and_loses_its_ids() {
        use kadakas_core::surface::{CurveId, FaceId, VertexId};
        let errors = [
            CurveError::NoCurve(CurveId(3)),
            CurveError::NoDescription(CurveId(3)),
            CurveError::Segments {
                segments: 2,
                min: 3,
                max: 1000,
            },
            CurveError::VertexShared {
                vertex: VertexId(4),
            },
            CurveError::BordersOtherPlane { face: FaceId(5) },
            CurveError::LoopUnclear { face: FaceId(5) },
            CurveError::Invalid {
                reason: "edge 9 is too short".into(),
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for e in errors {
            let text = localize_error(&SurfaceError::Curve(e.clone()));
            assert!(
                !text.chars().any(|c| c.is_ascii_digit()) || text.contains("1,000"),
                "{text}"
            );
            assert!(seen.insert(text), "{e}");
            // The text path of a command error finds the same reasons, except the missing curve.
            if !matches!(e, CurveError::NoCurve(_)) {
                assert_ne!(localize(&e.to_string()), e.to_string(), "{e}");
            }
        }
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
