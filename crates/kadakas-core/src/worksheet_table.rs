//! A worksheet placed in the drawing. The object is a rectangle (the frame of the table) with
//! `data["worksheet_table"]` that names the worksheet. The table itself is drawn from the
//! calculated worksheet by the app and by `kadakas-print`, so it follows the document live.
//!
//! The frame is anchored at its top left corner. A table that is larger than its frame is cut at
//! the frame, so `SetWorksheetTable` with a size fits the frame to the table.

use serde::{Deserialize, Serialize};

use crate::model::{Document, Object, ObjectId, Shape};
use crate::resource::{ResourceBody, ResourceId};

/// The key in `Object::data`. Only commands write it.
pub const KEY: &str = "worksheet_table";

/// Default text height of a table in mm (paper mm on a sheet, model mm on a design layer).
pub const DEFAULT_TEXT_HEIGHT: f64 = 3.5;
/// Largest text height and frame side.
pub const MAX_SIZE: f64 = 1.0e7;

/// What a table object keeps in its data.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableData {
    pub worksheet: ResourceId,
    pub text_height: f64,
}

/// The table data of an object, if it is a table: a rectangle with valid data.
pub fn data(obj: &Object) -> Option<TableData> {
    if !matches!(obj.shape, Shape::Rect { .. }) {
        return None;
    }
    serde_json::from_value(obj.data.get(KEY)?.clone())
        .ok()
        .filter(|d: &TableData| d.text_height.is_finite() && d.text_height > 0.0)
}

pub fn set_data(obj: &mut Object, d: &TableData) {
    obj.data
        .insert(KEY.into(), serde_json::to_value(d).unwrap_or_default());
}

/// Checks the data against the document: the worksheet exists, the text height is usable.
pub fn validate(doc: &Document, d: &TableData) -> Result<(), String> {
    if !d.text_height.is_finite() || d.text_height <= 0.0 || d.text_height > MAX_SIZE {
        return Err("the text height must be a positive, finite number".into());
    }
    match doc.resources.get(d.worksheet).map(|r| &r.body) {
        Some(ResourceBody::Worksheet(_)) => Ok(()),
        Some(_) => Err(format!("resource {} is not a worksheet", d.worksheet)),
        None => Err(format!("worksheet {} does not exist", d.worksheet)),
    }
}

/// The table objects that show this worksheet.
pub fn users_of(doc: &Document, worksheet: ResourceId) -> Vec<ObjectId> {
    doc.objects
        .values()
        .filter(|o| data(o).is_some_and(|d| d.worksheet == worksheet))
        .map(|o| o.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::model::Attributes;
    use glam::DVec2;

    #[test]
    fn data_needs_a_rectangle_and_valid_numbers() {
        let mut ed = Editor::default();
        let id = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Rect {
                    min: DVec2::ZERO,
                    max: DVec2::ONE,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let mut o = ed.doc().object(id).unwrap().clone();
        assert!(data(&o).is_none());
        set_data(
            &mut o,
            &TableData {
                worksheet: ResourceId::from_num(5),
                text_height: 3.0,
            },
        );
        assert_eq!(
            data(&o),
            Some(TableData {
                worksheet: ResourceId::from_num(5),
                text_height: 3.0
            })
        );
        set_data(
            &mut o,
            &TableData {
                worksheet: ResourceId::from_num(5),
                text_height: -1.0,
            },
        );
        assert!(data(&o).is_none());
        set_data(
            &mut o,
            &TableData {
                worksheet: ResourceId::from_num(5),
                text_height: 3.0,
            },
        );
        o.shape = Shape::Circle {
            center: DVec2::ZERO,
            radius: 1.0,
        };
        assert!(data(&o).is_none(), "only a rectangle is a table");
        o.data.insert(KEY.into(), serde_json::json!("junk"));
        o.shape = Shape::Rect {
            min: DVec2::ZERO,
            max: DVec2::ONE,
        };
        assert!(data(&o).is_none());
    }

    #[test]
    fn validate_checks_the_worksheet() {
        let mut ed = Editor::default();
        let ws = ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: ResourceBody::Worksheet(serde_json::json!({})),
            })
            .unwrap()
            .created[0],
        );
        let st = ResourceId(
            ed.execute(&Command::AddResource {
                name: "P".into(),
                body: ResourceBody::plant_style(Default::default()),
            })
            .unwrap()
            .created[0],
        );
        let ok = TableData {
            worksheet: ws,
            text_height: 2.0,
        };
        assert!(validate(ed.doc(), &ok).is_ok());
        assert!(
            validate(
                ed.doc(),
                &TableData {
                    worksheet: st,
                    ..ok
                }
            )
            .unwrap_err()
            .contains("not a worksheet")
        );
        assert!(
            validate(
                ed.doc(),
                &TableData {
                    worksheet: ResourceId::from_num(999),
                    ..ok
                }
            )
            .unwrap_err()
            .contains("does not exist")
        );
        assert!(
            validate(
                ed.doc(),
                &TableData {
                    text_height: 0.0,
                    ..ok
                }
            )
            .is_err()
        );
        assert!(
            validate(
                ed.doc(),
                &TableData {
                    text_height: f64::NAN,
                    ..ok
                }
            )
            .is_err()
        );
        assert!(
            validate(
                ed.doc(),
                &TableData {
                    text_height: 1e9,
                    ..ok
                }
            )
            .is_err()
        );
        assert!(users_of(ed.doc(), ws).is_empty());
    }
}
