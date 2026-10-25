//! Real-world habits of exporters: escapes in names, lower-case entity names, repeated ids.

use super::tests_more::{extr, objs, run, slab};
use super::*;

fn name_of(o: &kadakas_core::model::Object) -> String {
    o.data["ifc"]["name"].as_str().unwrap_or("").to_string()
}

fn body() -> String {
    extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,100.)", 10.0)
}

#[test]
fn escaped_names_are_decoded() {
    // The buildingSMART landscaping scene writes "earth\X\27s" for the apostrophe.
    let data = format!(
        "{}{}",
        body(),
        slab(30, "#21").replace("N30", "earth\\X\\27s \\S\\d \\PB\\\\S\\1 \\X2\\00D8\\X0\\")
    );
    let (ed, _) = run(&data, &IfcImportOptions::default());
    let o = objs(&ed)[0];
    // \S\d in part 1 is U+00E4; after \PB\ the code \S\1 is part 2 U+0105.
    assert_eq!(name_of(o), "earth's \u{E4} \u{105} \u{D8}");
}

#[test]
fn lower_case_names_and_spaces_after_the_equals_sign() {
    let data = format!("{}{}", body(), slab(30, "#21"))
        .replace("#21=IFCEXTRUDEDAREASOLID", "#21= ifcextrudedareasolid")
        .replace("IFCSLAB(", "ifcslab(");
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
}

#[test]
fn repeated_global_ids_and_step_ids_are_counted() {
    let data =
        format!("{}{}{}", body(), slab(30, "#21"), slab(40, "#21")).replace("'g40'", "'g30'");
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 2);
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("1 products repeat the GlobalId")),
        "{:?}",
        r.notes
    );
    // A repeated step id: the first one wins and the report says so.
    let data = format!(
        "{}{}\n#20=IFCCARTESIANPOINT((9.,9.));",
        body(),
        slab(30, "#21")
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1);
    assert!(
        r.notes.iter().any(|n| n.contains("repeat an ID")),
        "{:?}",
        r.notes
    );
}
