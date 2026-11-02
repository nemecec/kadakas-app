//! The answer to a method name that does not exist (AGR-09): the closest listed methods, and a
//! hint for the names that an agent guesses for deleting objects. The names come from `METHODS`.

use serde_json::{Value, json};

use crate::METHODS;
use crate::params::refs::{NEAR_NAME_MAX, distance_within};

/// The most methods that the answer names.
pub const CLOSEST_MAX: usize = 3;

/// The words of the message that tell the reader where to look. A feedback draft for a missing
/// method is left out when the message has one of them: the answer has a place to go already.
pub const POINTS_ELSEWHERE: [&str; 2] = ["Closest listed methods:", "To delete objects call"];

/// True if the message of an unknown method already names methods to use.
pub fn points_elsewhere(message: &str) -> bool {
    POINTS_ELSEWHERE.iter().any(|w| message.contains(w))
}

/// The name as the message shows it: at most `NEAR_NAME_MAX` characters, then an ellipsis.
fn shown(name: &str) -> String {
    if name.chars().count() <= NEAR_NAME_MAX {
        return name.to_string();
    }
    let cut: String = name.chars().take(NEAR_NAME_MAX).collect();
    format!("{cut}...")
}

/// The words of a name, in the singular: `objects.delete_all` gives `object`, `delete`, `all`.
fn words(name: &str) -> Vec<String> {
    name.to_ascii_lowercase()
        .split(['.', '_', '-', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            w.strip_suffix('s')
                .filter(|s| s.len() >= 3)
                .unwrap_or(w)
                .to_string()
        })
        .collect()
}

/// Up to `CLOSEST_MAX` listed methods that are near `name`, nearest first. A method counts when it
/// shares a word with the name (`delete_objects` and `objects.delete`) or is a few letters away
/// (`object.delete`).
#[cfg(test)]
pub fn closest(name: &str) -> Vec<&'static str> {
    ranked(name).0
}

/// The closest methods, and whether the first of them is a near spelling of the name (a typing
/// mistake) and not only a method with a word in common.
fn ranked(name: &str) -> (Vec<&'static str>, bool) {
    // A long name is not a typing mistake, and the search is for short names.
    if name.chars().count() > NEAR_NAME_MAX {
        return (Vec::new(), false);
    }
    let wanted = words(name);
    let lower = name.to_ascii_lowercase();
    let mut scored: Vec<(usize, usize, usize, &'static str)> = METHODS
        .iter()
        .filter_map(|(m, _)| {
            let shared = words(m).iter().filter(|w| wanted.contains(w)).count();
            let limit = (lower.chars().count().max(m.chars().count()) / 3).clamp(1, 3);
            let d = distance_within(&lower, &m.to_ascii_lowercase(), limit);
            (shared > 0 || d <= limit).then_some((shared, d, limit, *m))
        })
        .collect();
    // The most shared words first, then the least distance, then the name for a stable order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.3.cmp(b.3)));
    let near = scored.iter().any(|(_, d, limit, _)| d <= limit);
    let names = scored
        .into_iter()
        .take(CLOSEST_MAX)
        .map(|(_, _, _, m)| m)
        .collect();
    (names, near)
}

/// A hint for the names that mean "delete objects": `object.delete`, `objects.remove`,
/// `delete_objects`. The method is `objects.delete`, and the command form works in a batch.
fn delete_hint(name: &str) -> Option<&'static str> {
    if name.chars().count() > NEAR_NAME_MAX {
        return None;
    }
    let w = words(name);
    let verb = ["delete", "remove", "erase", "destroy", "del", "rm", "drop"];
    let noun = ["object", "shape", "entity", "item", "element"];
    let has = |list: &[&str]| w.iter().any(|x| list.contains(&x.as_str()));
    (has(&verb) && has(&noun)).then_some(
        "To delete objects call objects.delete {ids}, or the command form execute {command: {op: \"delete_objects\", ids}}. The command form also works inside a batch command.",
    )
}

/// The message and the facts of the error for an unknown method.
pub fn unknown_method(method: &str) -> (String, Value) {
    let (near, spelled) = ranked(method);
    let hint = delete_hint(method);
    let mut message = format!("unknown method: {}", shown(method));
    if !near.is_empty() {
        // Only a near spelling is "closest": it points the reader to the method. Methods that
        // share a word are a list to look at, and the feedback draft stays.
        let label = if spelled {
            "Closest listed methods"
        } else {
            "Listed methods with a word in common"
        };
        message.push_str(&format!(". {label}: {}", near.join(", ")));
    }
    if let Some(h) = hint {
        message.push_str(&format!(". {h}"));
    }
    if near.is_empty() && hint.is_none() {
        message.push_str(". describe lists the methods");
    }
    // A name that is too long for the search carries no `closest` at all.
    if method.chars().count() > NEAR_NAME_MAX {
        return (message, json!({}));
    }
    (message, json!({"closest": near, "hint": hint}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_an_agent_guesses_for_delete_lead_to_objects_delete() {
        for guess in [
            "object.delete",
            "objects.remove",
            "delete_objects",
            "objects.erase",
            "object.remove",
        ] {
            let (message, facts) = unknown_method(guess);
            assert!(
                message.starts_with(&format!("unknown method: {guess}")),
                "{message}"
            );
            assert!(
                message.contains("objects.delete {ids}"),
                "{guess}: {message}"
            );
            assert!(message.contains("delete_objects"), "{guess}: {message}");
            assert!(
                facts["closest"]
                    .as_array()
                    .is_some_and(|a| a.len() <= CLOSEST_MAX)
            );
        }
        assert_eq!(closest("object.delete")[0], "objects.delete");
        assert!(closest("delete_objects").contains(&"objects.delete"));
    }

    #[test]
    fn a_typing_mistake_names_the_listed_method() {
        assert_eq!(closest("objcts.list")[0], "objects.list");
        assert_eq!(closest("mesh.faces2")[0], "mesh.faces");
        assert!(closest("zzzzzzzzzz").is_empty());
        let (message, facts) = unknown_method("zzzzzzzzzz");
        assert!(message.contains("describe lists the methods"));
        assert_eq!(facts["closest"], json!([]));
    }

    #[test]
    fn a_long_name_is_not_searched_and_is_cut_in_the_message() {
        let long = "x".repeat(1_000_000);
        let (message, facts) = unknown_method(&long);
        assert!(message.len() < 200, "{}", message.len());
        assert!(message.starts_with(&format!("unknown method: {}...", "x".repeat(64))));
        assert!(!message.contains("Closest"), "{message}");
        assert_eq!(facts, json!({}));
        assert!(closest(&long).is_empty());
        // A long name that spells a delete is not hinted either.
        let delete = format!("delete_objects{}", "_x".repeat(40));
        assert!(
            unknown_method(&delete)
                .0
                .contains("describe lists the methods")
        );
        assert!(!points_elsewhere(&message));
        assert!(points_elsewhere(&unknown_method("object.delete").0));
    }

    #[test]
    fn the_answer_names_at_most_three_listed_methods() {
        for guess in ["objects", "mesh", "view.get2", "layer.remove", "plant"] {
            let near = closest(guess);
            assert!(near.len() <= CLOSEST_MAX, "{guess}");
            assert!(near.iter().all(|m| METHODS.iter().any(|(n, _)| n == m)));
        }
    }
}
