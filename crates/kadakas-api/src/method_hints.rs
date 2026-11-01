//! The answer to a method name that does not exist (AGR-09): the closest listed methods, and a
//! hint for the names that an agent guesses for deleting objects. The names come from `METHODS`.

use serde_json::{Value, json};

use crate::METHODS;
use crate::params::refs::distance;

/// The most methods that the answer names.
pub const CLOSEST_MAX: usize = 3;

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
pub fn closest(name: &str) -> Vec<&'static str> {
    let wanted = words(name);
    let lower = name.to_ascii_lowercase();
    let mut scored: Vec<(usize, usize, &'static str)> = METHODS
        .iter()
        .filter_map(|(m, _)| {
            let shared = words(m).iter().filter(|w| wanted.contains(w)).count();
            let d = distance(&lower, &m.to_ascii_lowercase());
            let limit = (lower.chars().count().max(m.chars().count()) / 3).clamp(1, 4);
            (shared > 0 || d <= limit).then_some((shared, d, *m))
        })
        .collect();
    // The most shared words first, then the least distance, then the name for a stable order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));
    scored
        .into_iter()
        .take(CLOSEST_MAX)
        .map(|(_, _, m)| m)
        .collect()
}

/// A hint for the names that mean "delete objects": `object.delete`, `objects.remove`,
/// `delete_objects`. The method is `objects.delete`, and the command form works in a batch.
fn delete_hint(name: &str) -> Option<&'static str> {
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
    let near = closest(method);
    let hint = delete_hint(method);
    let mut message = format!("unknown method: {method}");
    if !near.is_empty() {
        message.push_str(&format!(". Closest listed methods: {}", near.join(", ")));
    }
    if let Some(h) = hint {
        message.push_str(&format!(". {h}"));
    }
    if near.is_empty() && hint.is_none() {
        message.push_str(". describe lists the methods");
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
    fn the_answer_names_at_most_three_listed_methods() {
        for guess in ["objects", "mesh", "view.get2", "layer.remove", "plant"] {
            let near = closest(guess);
            assert!(near.len() <= CLOSEST_MAX, "{guess}");
            assert!(near.iter().all(|m| METHODS.iter().any(|(n, _)| n == m)));
        }
    }
}
