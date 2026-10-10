//! Commands for records: attach, detach and change field values. They run inside `Tx` like all others.

use super::*;
use crate::record::{self, RecordFormat};

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    /// True if the data key is the name of a record format. `SetData` may not write these keys.
    pub(super) fn is_record_key(&self, key: &str) -> bool {
        self.doc.resources.iter().any(|r| {
            matches!(r.body, ResourceBody::RecordFormat(_))
                && r.name.eq_ignore_ascii_case(key.trim())
        })
    }

    /// Objects that have a record of this name attached.
    pub(super) fn record_users(&self, format: ResourceId) -> Vec<ObjectId> {
        let tag = Value::from(format.0);
        self.doc
            .objects
            .values()
            .filter(|o| o.data.values().any(|v| v.get(record::MARKER) == Some(&tag)))
            .map(|o| o.id)
            .collect()
    }

    /// After a record format changed: the attached records follow the new name and fields.
    pub(super) fn conform_records(&mut self, id: ResourceId, old_name: &str) {
        let Some(r) = self.doc.resources.get(id) else {
            return;
        };
        let ResourceBody::RecordFormat(fmt) = &r.body else {
            return;
        };
        let (fmt, new_name) = (fmt.clone(), r.name.clone());
        for oid in self.record_users(id) {
            self.touch(oid);
            let o = self.doc.object_mut(oid).expect("exists");
            let tag = Value::from(id.0);
            let key = o
                .data
                .iter()
                .find(|(k, v)| v.get(record::MARKER) == Some(&tag) && k.as_str() == old_name)
                .map(|(k, _)| k.clone());
            let Some(key) = key else { continue };
            let stored = o
                .data
                .remove(&key)
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            o.data.insert(
                new_name.clone(),
                Value::Object(record::tagged(fmt.conform(&stored), id.0)),
            );
        }
    }

    fn record_format(&self, id: ResourceId) -> Result<(String, RecordFormat), CommandError> {
        let r = self
            .doc
            .resources
            .get(id)
            .ok_or(CommandError::NoResource(id))?;
        match &r.body {
            ResourceBody::RecordFormat(f) => Ok((r.name.clone(), f.as_ref().clone())),
            _ => Err(invalid("the resource is not a record format")),
        }
    }

    pub(super) fn run_record(&mut self, cmd: &Command) -> Result<(), CommandError> {
        match cmd {
            Command::AttachRecord {
                ids,
                record,
                values,
            } => {
                let (name, fmt) = self.record_format(*record)?;
                let ids = unique(ids);
                if ids.is_empty() {
                    return Err(invalid("give at least one object"));
                }
                let built = record::tagged(fmt.build(values).map_err(invalid)?, record.0);
                for &id in &ids {
                    self.editable(id)?;
                    if self
                        .doc
                        .object(id)
                        .expect("checked")
                        .data
                        .contains_key(&name)
                    {
                        return Err(invalid(format!(
                            "object {id} has the record '{name}' already"
                        )));
                    }
                }
                for &id in &ids {
                    self.touch(id);
                    self.doc
                        .object_mut(id)
                        .expect("checked")
                        .data
                        .insert(name.clone(), Value::Object(built.clone()));
                }
            }
            Command::DetachRecord { ids, record } => {
                let (name, _) = self.record_format(*record)?;
                let ids = unique(ids);
                if ids.is_empty() {
                    return Err(invalid("give at least one object"));
                }
                for &id in &ids {
                    self.editable(id)?;
                    if record::values(self.doc.object(id).expect("checked"), &name).is_none() {
                        return Err(invalid(format!("object {id} has no record '{name}'")));
                    }
                }
                for &id in &ids {
                    self.touch(id);
                    self.doc.object_mut(id).expect("checked").data.remove(&name);
                }
            }
            Command::SetRecordFields {
                ids,
                record,
                values,
            } => {
                let (name, fmt) = self.record_format(*record)?;
                let ids = unique(ids);
                if ids.is_empty() {
                    return Err(invalid("give at least one object"));
                }
                let mut next = Vec::with_capacity(ids.len());
                for &id in &ids {
                    self.editable(id)?;
                    let o = self.doc.object(id).expect("checked");
                    let Some(stored) = record::values(o, &name) else {
                        return Err(invalid(format!("object {id} has no record '{name}'")));
                    };
                    next.push(fmt.merge(stored, values).map_err(invalid)?);
                }
                for (&id, m) in ids.iter().zip(next) {
                    self.touch(id);
                    self.doc
                        .object_mut(id)
                        .expect("checked")
                        .data
                        .insert(name.clone(), Value::Object(m));
                }
            }
            _ => unreachable!("run_record gets record commands only"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{FieldDef, FieldType};
    use serde_json::json;

    fn fmt() -> RecordFormat {
        let mut grade = FieldDef::new("Grade", FieldType::Choice);
        grade.choices = vec!["A".into(), "B".into()];
        RecordFormat {
            fields: vec![
                FieldDef::new("Depth", FieldType::Dimension),
                FieldDef::new("Qty", FieldType::Integer),
                grade,
            ],
        }
    }

    fn setup() -> (Editor, ResourceId, Vec<ObjectId>) {
        let mut ed = Editor::default();
        let rid = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Soil".into(),
                body: ResourceBody::RecordFormat(Box::new(fmt())),
            })
            .unwrap()
            .created[0],
        );
        let mut ids = vec![];
        for x in 0..3 {
            let out = ed
                .execute(&Command::AddObject {
                    shape: Shape::Circle {
                        center: DVec2::new(x as f64 * 10.0, 0.0),
                        radius: 1.0,
                    },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap();
            ids.push(ObjectId(out.created[0]));
        }
        (ed, rid, ids)
    }

    fn vals(v: serde_json::Value) -> serde_json::Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn rec(ed: &Editor, id: ObjectId, name: &str) -> Option<Value> {
        record::values(ed.doc().object(id).unwrap(), name)
            .map(|m| Value::Object(record::fields_only(m)))
    }

    #[test]
    fn attach_set_detach_and_undo() {
        let (mut ed, rid, ids) = setup();
        let before = ed.doc().clone();
        ed.execute(&Command::AttachRecord {
            ids: vec![ids[0], ids[1], ids[0]],
            record: rid,
            values: vals(json!({"Depth": 250, "grade": "b"})),
        })
        .unwrap();
        assert_eq!(
            rec(&ed, ids[0], "Soil").unwrap(),
            json!({"Depth": 250.0, "Qty": 0, "Grade": "B"})
        );
        assert_eq!(rec(&ed, ids[1], "Soil"), rec(&ed, ids[0], "Soil"));
        assert!(rec(&ed, ids[2], "Soil").is_none());
        ed.execute(&Command::SetRecordFields {
            ids: vec![ids[1]],
            record: rid,
            values: vals(json!({"Qty": "5"})),
        })
        .unwrap();
        assert_eq!(rec(&ed, ids[1], "Soil").unwrap()["Qty"], json!(5));
        assert_eq!(rec(&ed, ids[0], "Soil").unwrap()["Qty"], json!(0));
        ed.execute(&Command::DetachRecord {
            ids: vec![ids[0]],
            record: rid,
        })
        .unwrap();
        assert!(rec(&ed, ids[0], "Soil").is_none());
        assert!(ed.undo() && ed.undo() && ed.undo());
        assert_eq!(*ed.doc(), before);
        assert!(ed.redo());
        assert!(rec(&ed, ids[0], "Soil").is_some());
    }

    #[test]
    fn errors_change_nothing() {
        let (mut ed, rid, ids) = setup();
        let attach = |ids: Vec<ObjectId>, record, v| Command::AttachRecord {
            ids,
            record,
            values: vals(v),
        };
        assert!(ed.execute(&attach(vec![], rid, json!({}))).is_err());
        assert!(
            ed.execute(&attach(vec![ids[0]], ResourceId::from_num(999), json!({})))
                .is_err()
        );
        assert!(
            ed.execute(&attach(vec![ObjectId::from_num(999)], rid, json!({})))
                .is_err()
        );
        assert!(
            ed.execute(&attach(vec![ids[0]], rid, json!({"Qty": 1.5})))
                .is_err()
        );
        assert!(
            ed.execute(&attach(vec![ids[0]], rid, json!({"Nope": 1})))
                .is_err()
        );
        assert!(
            ed.execute(&attach(vec![ids[0]], rid, json!({"Grade": "Z"})))
                .is_err()
        );
        let style = ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: ResourceBody::plant_style(Default::default()),
            })
            .unwrap()
            .created[0],
        );
        let e = ed
            .execute(&attach(vec![ids[0]], style, json!({})))
            .unwrap_err();
        assert!(e.to_string().contains("not a record format"));
        ed.execute(&attach(vec![ids[0]], rid, json!({}))).unwrap();
        let snap = ed.doc().clone();
        // Already attached: nothing changes, not even for the free object.
        let e = ed
            .execute(&attach(vec![ids[1], ids[0]], rid, json!({})))
            .unwrap_err();
        assert!(e.to_string().contains("already"), "{e}");
        assert_eq!(*ed.doc(), snap);
        // Set on an object without the record.
        let set = |ids: Vec<ObjectId>, v| Command::SetRecordFields {
            ids,
            record: rid,
            values: vals(v),
        };
        assert!(
            ed.execute(&set(vec![ids[0], ids[1]], json!({"Qty": 2})))
                .unwrap_err()
                .to_string()
                .contains("no record")
        );
        assert!(ed.execute(&set(vec![ids[0]], json!({"Qty": "x"}))).is_err());
        assert!(ed.execute(&set(vec![], json!({}))).is_err());
        assert_eq!(*ed.doc(), snap);
        let det = |ids: Vec<ObjectId>| Command::DetachRecord { ids, record: rid };
        assert!(
            ed.execute(&det(vec![ids[0], ids[1]]))
                .unwrap_err()
                .to_string()
                .contains("no record")
        );
        assert!(ed.execute(&det(vec![])).is_err());
        assert_eq!(*ed.doc(), snap);
    }

    #[test]
    fn locked_layers_block_record_edits() {
        let (mut ed, rid, ids) = setup();
        ed.execute(&Command::ModifyLayer {
            id: crate::model::FIRST_LAYER,
            name: None,
            visible: None,
            locked: Some(true),
            elevation: None,
            scale: None,
        })
        .unwrap();
        let e = ed
            .execute(&Command::AttachRecord {
                ids: vec![ids[0]],
                record: rid,
                values: Default::default(),
            })
            .unwrap_err();
        assert!(matches!(e, CommandError::LayerLocked(_)));
    }

    #[test]
    fn records_are_tagged_and_plain_data_is_never_taken_for_one() {
        let (mut ed, rid, ids) = setup();
        // Plain data under a name that is not a format yet.
        ed.execute(&Command::SetData {
            id: ids[1],
            set: [("Water".to_string(), json!({"Depth": 5}))].into(),
            remove: vec![],
        })
        .unwrap();
        ed.execute(&Command::AttachRecord {
            ids: vec![ids[0]],
            record: rid,
            values: Default::default(),
        })
        .unwrap();
        assert_eq!(
            ed.doc().object(ids[0]).unwrap().data["Soil"][record::MARKER],
            json!(rid.0)
        );
        // A format may not take a name that plain data uses already.
        let e = ed
            .execute(&Command::AddResource {
                name: "water".into(),
                body: ResourceBody::RecordFormat(Box::default()),
            })
            .unwrap_err();
        assert!(e.to_string().contains("already have data"), "{e}");
        let other = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Other".into(),
                body: ResourceBody::RecordFormat(Box::default()),
            })
            .unwrap()
            .created[0],
        );
        let e = ed
            .execute(&Command::ModifyResource {
                id: other,
                name: Some("Water".into()),
                body: None,
            })
            .unwrap_err();
        assert!(e.to_string().contains("already have data"), "{e}");
        // Renaming a format onto its own records is fine, and the marker is not a name.
        ed.execute(&Command::ModifyResource {
            id: rid,
            name: Some("SOIL".into()),
            body: None,
        })
        .unwrap();
        assert!(
            ed.execute(&Command::AddResource {
                name: "_record".into(),
                body: ResourceBody::RecordFormat(Box::default())
            })
            .is_err()
        );
        // Plain data blocks attach (the key is taken) and is not seen as a record.
        let e = ed
            .execute(&Command::SetData {
                id: ids[1],
                set: [("Soil".to_string(), json!({}))].into(),
                remove: vec![],
            })
            .unwrap_err();
        assert!(e.to_string().contains("record"));
        // Detach refuses data without the tag, and an object without the record is not a user.
        let e = ed
            .execute(&Command::DetachRecord {
                ids: vec![ids[1]],
                record: other,
            })
            .unwrap_err();
        assert!(e.to_string().contains("no record"));
        assert_eq!(
            ed.doc().object(ids[1]).unwrap().data["Water"],
            json!({"Depth": 5})
        );
        // A format cannot have a field called like the marker.
        let f = RecordFormat {
            fields: vec![FieldDef::new("_Record", FieldType::Text)],
        };
        assert!(f.validate().unwrap_err().contains("reserved"));
        // Deleting the format while an untagged key has the same name is fine: only tagged records count.
        let ids1 = ids[1];
        ed.execute(&Command::DeleteResource {
            id: other,
            reassign: None,
        })
        .unwrap();
        assert!(ed.doc().object(ids1).unwrap().data.contains_key("Water"));
    }

    #[test]
    fn set_data_cannot_write_records() {
        let (mut ed, _, ids) = setup();
        let e = ed
            .execute(&Command::SetData {
                id: ids[0],
                set: [("soil".to_string(), json!({"Depth": 1}))].into(),
                remove: vec![],
            })
            .unwrap_err();
        assert!(e.to_string().contains("record"), "{e}");
        let e = ed
            .execute(&Command::SetData {
                id: ids[0],
                set: Default::default(),
                remove: vec!["Soil".into()],
            })
            .unwrap_err();
        assert!(e.to_string().contains("record"), "{e}");
        ed.execute(&Command::SetData {
            id: ids[0],
            set: [("other".to_string(), json!(1))].into(),
            remove: vec![],
        })
        .unwrap();
    }

    #[test]
    fn renaming_and_editing_the_format_updates_the_records() {
        let (mut ed, rid, ids) = setup();
        ed.execute(&Command::AttachRecord {
            ids: vec![ids[0]],
            record: rid,
            values: vals(json!({"Depth": 100, "Qty": 4})),
        })
        .unwrap();
        let before = ed.doc().clone();
        let mut f = fmt();
        f.fields.remove(0);
        f.fields.push(FieldDef::new("Note", FieldType::Text));
        ed.execute(&Command::ModifyResource {
            id: rid,
            name: Some("Ground".into()),
            body: Some(ResourceBody::RecordFormat(Box::new(f))),
        })
        .unwrap();
        assert!(rec(&ed, ids[0], "Soil").is_none());
        assert_eq!(
            rec(&ed, ids[0], "Ground").unwrap(),
            json!({"Qty": 4, "Grade": "A", "Note": ""})
        );
        assert_eq!(
            ed.doc().object(ids[0]).unwrap().data["Ground"][record::MARKER],
            json!(rid.0),
            "the tag survives the rename"
        );
        assert!(rec(&ed, ids[1], "Ground").is_none());
        // The old name is free for SetData now, the new name is not.
        assert!(
            ed.execute(&Command::SetData {
                id: ids[1],
                set: [("Ground".to_string(), json!(1))].into(),
                remove: vec![]
            })
            .is_err()
        );
        assert!(ed.undo());
        assert_eq!(*ed.doc(), before);
    }

    #[test]
    fn delete_needs_detached_records_and_names_are_checked() {
        let (mut ed, rid, ids) = setup();
        ed.execute(&Command::AttachRecord {
            ids: vec![ids[0]],
            record: rid,
            values: Default::default(),
        })
        .unwrap();
        let e = ed
            .execute(&Command::DeleteResource {
                id: rid,
                reassign: None,
            })
            .unwrap_err();
        assert!(matches!(e, CommandError::ResourceInUse(_, 1)));
        let other = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Other".into(),
                body: ResourceBody::RecordFormat(Box::default()),
            })
            .unwrap()
            .created[0],
        );
        let e = ed
            .execute(&Command::DeleteResource {
                id: rid,
                reassign: Some(other),
            })
            .unwrap_err();
        assert!(e.to_string().contains("Detach"), "{e}");
        ed.execute(&Command::DetachRecord {
            ids: vec![ids[0]],
            record: rid,
        })
        .unwrap();
        ed.execute(&Command::DeleteResource {
            id: rid,
            reassign: None,
        })
        .unwrap();
        for bad in ["plant", "Rotation", " "] {
            assert!(
                ed.execute(&Command::AddResource {
                    name: bad.into(),
                    body: ResourceBody::RecordFormat(Box::default())
                })
                .is_err(),
                "{bad}"
            );
        }
        let e = ed
            .execute(&Command::AddResource {
                name: "x".repeat(100),
                body: ResourceBody::RecordFormat(Box::default()),
            })
            .unwrap_err();
        assert!(e.to_string().contains("too long"));
    }
}
