//! Text from a module is shown to people. Bidi and zero-width characters are removed.

use super::*;
use crate::plugin::clean_message;

#[test]
fn a_message_loses_format_characters_and_control_characters() {
    assert_eq!(
        clean_message("no\u{202E} way\u{200B}\nat all"),
        "no way at all"
    );
    assert_eq!(clean_message("\u{FEFF}"), "");
}

#[test]
fn a_manifest_from_a_module_loses_format_characters() {
    // JSON escapes, so that the text of the WAT module has no confusing character.
    let manifest = r#"{"id":"test.bidi","name":"Safe\u202EgnirtS","version":"1","abi_version":2,
        "author":"A\u200BB","kinds":["object"],"object":{"shape_type":"point","params":[
        {"name":"n","type":"number","label":"L\u2066x"}]}}"#
        .replace('\n', "");
    let wasm = module_with(&manifest, ONE_CIRCLE, None, "");
    let p = load(&wasm, &quick()).unwrap();
    assert_eq!(p.manifest().name, "SafegnirtS");
    assert_eq!(p.manifest().author, "AB");
    assert_eq!(p.manifest().object.as_ref().unwrap().params[0].label, "Lx");
}
