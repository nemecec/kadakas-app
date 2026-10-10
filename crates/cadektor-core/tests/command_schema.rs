//! The JSON Schema of commands is part of the API that agents read, so it must not change by
//! accident. This test compares it with the saved copy `command_schema.json`. After a change of a
//! command on purpose, renew the copy: `UPDATE_SCHEMA=1 cargo test -p cadektor-core --test command_schema`.

use std::path::PathBuf;

use cadektor_core::schema::command_schema;

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/command_schema.json")
}

fn pretty() -> String {
    let mut text = serde_json::to_string_pretty(&command_schema()).unwrap();
    text.push('\n');
    text
}

#[test]
fn the_command_schema_matches_the_saved_copy() {
    let now = pretty();
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(golden_path(), &now).unwrap();
        return;
    }
    let saved = std::fs::read_to_string(golden_path()).expect("tests/command_schema.json is missing: run with UPDATE_SCHEMA=1");
    assert!(
        saved == now,
        "the command schema changed. If this is on purpose, run: UPDATE_SCHEMA=1 cargo test -p cadektor-core --test command_schema"
    );
}

#[test]
fn the_schema_describes_known_commands() {
    let s = command_schema();
    let ops = s["oneOf"].as_array().unwrap();
    let find = |op: &str| {
        ops.iter()
            .find(|v| v["properties"]["op"]["const"] == op)
            .unwrap_or_else(|| panic!("{op} is missing"))
            .clone()
    };
    // A command has its required fields.
    let mv = find("move_objects");
    let required: Vec<&str> = mv["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(required.contains(&"ids") && required.contains(&"delta") && required.contains(&"op"));
    // A point is a pair of numbers.
    assert_eq!(mv["properties"]["delta"]["type"], "array");
    assert_eq!(mv["properties"]["delta"]["maxItems"], 2);
    // Optional fields are not required.
    let add = find("add_object");
    let required: Vec<&str> = add["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(required, ["op", "shape"]);
}
