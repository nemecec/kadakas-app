use super::*;
use crate::paving::fake::Fake;
use crate::paving::lot;
use kadakas_core::command::Command;
use kadakas_core::curve::corners;
use kadakas_core::hardscape::HardscapePatch;

fn reply() -> Value {
    json!({
        "rows": [{
            "id": "x", "name": "Pavers", "bond": "stack",
            "whole_units": 480, "cut_units": 32, "slivers": 2, "unfillable_pieces": 1,
            "area_laid_m2": 10.4, "cut_area_m2": 0.9, "widest_joint_mm": 5.0,
            "order_units": 512, "order_units_min": 490,
            "joint_warning": "wide",
            "by_unit": [{"name": "200x100", "whole_units": 480, "cut_units": 32, "order_units": 512}],
            "groups": [
                {"name": "zone 0", "role": "zone", "whole_units": 200, "cut_units": 16},
                {"name": "Kerb", "role": "course", "whole_units": 40, "cut_units": 2},
            ],
            "notes": ["a note"],
        }],
        "totals": {},
    })
}

#[test]
fn the_row_of_the_report_becomes_the_counts() {
    let c = UnitCounts::parse(&reply()).unwrap();
    assert_eq!((c.whole, c.cut, c.slivers, c.unfillable), (480, 32, 2, 1));
    assert_eq!((c.order_units, c.order_units_min), (512, 490));
    assert_eq!(c.area_laid_m2, 10.4);
    assert_eq!(c.joint_warning.as_deref(), Some("wide"));
    assert_eq!(c.types.len(), 1);
    assert_eq!(c.types[0].name, "200x100");
    assert_eq!(c.parts.len(), 2);
    assert_eq!((c.parts[1].name.as_str(), c.parts[1].role.as_str()), ("Kerb", "course"));
    assert_eq!(c.notes, vec!["a note".to_string()]);
    // An error row is the error; an empty report is refused.
    let e = UnitCounts::parse(&json!({"rows": [{"error": "no pattern"}]}));
    assert_eq!(e.unwrap_err(), "no pattern");
    assert!(UnitCounts::parse(&json!({"rows": []})).is_err());
}

#[test]
fn the_count_is_the_report_call_for_the_object() {
    let (editor, id) = lot::new();
    let mut be = Fake::new().reply(reply());
    be.doc = editor.doc().clone();
    let c = count(&mut be, id).unwrap();
    assert_eq!(c.whole, 480);
    assert_eq!(be.calls[0].0, "report.unit_pattern");
    assert_eq!(be.calls[0].1["ids"], json!([id]));
}

#[test]
fn a_small_hardscape_is_counted_by_itself_and_a_big_one_when_asked() {
    let (mut editor, id) = lot::new();
    assert!(auto(editor.doc(), id));
    // 40 m by 40 m of 200 x 100 mm units is 70,000 units.
    editor
        .execute(&Command::AddHardscape {
            boundary: Some(corners(&[
                glam::DVec2::new(0.0, 0.0),
                glam::DVec2::new(40_000.0, 0.0),
                glam::DVec2::new(40_000.0, 40_000.0),
                glam::DVec2::new(0.0, 40_000.0),
            ])),
            patch: Box::new(HardscapePatch {
                style: kadakas_core::hardscape::data(editor.doc().object(id).unwrap())
                    .unwrap()
                    .style,
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
    let big = editor
        .doc()
        .objects
        .values()
        .map(|o| o.id)
        .find(|i| *i != id)
        .unwrap();
    assert!(!auto(editor.doc(), big));
    // No pattern, no count.
    assert!(!auto(editor.doc(), kadakas_core::model::ObjectId(kadakas_core::uid::Uid::default())));
}
