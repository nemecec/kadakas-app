//! Property sets and quantity sets as records. One record format is made for every set name, with
//! one field for every property that any object has. An object keeps the values of its own set.

use std::collections::{BTreeMap, HashSet};

use cadektor_core::command::Command;
use cadektor_core::model::{Document, ObjectId};
use cadektor_core::record::{
    FieldDef, FieldType, RecordFormat, check_name, is_reserved_record_name,
};
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_io::builder::Builder;
use serde_json::{Map, Value};

use super::model::PropSet;

/// Most record formats and fields per format that one import makes.
const MAX_FORMATS: usize = 300;
const MAX_FIELDS: usize = 120;
/// Data keys that other code uses.
const RESERVED: [&str; 6] = [
    "ifc",
    "ifc_class",
    "ifc_psets",
    "landscape_area",
    "hardscape",
    "plant",
];

struct Fmt {
    fields: Vec<(String, FieldType)>,
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.chars().take(10_000).collect(),
        other => other.to_string(),
    }
}

pub fn write_records(b: &mut Builder, doc: &Document, rows: Vec<(ObjectId, Vec<PropSet>)>) {
    // 1. The fields of every set.
    let mut formats: BTreeMap<String, Fmt> = BTreeMap::new();
    let mut dropped_sets: HashSet<String> = HashSet::new();
    let mut dropped_fields = 0usize;
    for (_, sets) in &rows {
        for s in sets {
            if !formats.contains_key(&s.name) {
                if formats.len() >= MAX_FORMATS {
                    dropped_sets.insert(s.name.clone());
                    continue;
                }
                formats.insert(s.name.clone(), Fmt { fields: Vec::new() });
            }
            let f = formats.get_mut(&s.name).expect("inserted");
            for p in &s.props {
                let at = f
                    .fields
                    .iter()
                    .position(|(n, _)| n.eq_ignore_ascii_case(&p.name));
                match at {
                    Some(i) => {
                        if f.fields[i].1 != p.ty {
                            f.fields[i].1 = FieldType::Text;
                        }
                    }
                    None if f.fields.len() < MAX_FIELDS => f.fields.push((p.name.clone(), p.ty)),
                    None => dropped_fields += 1,
                }
            }
        }
    }
    if !dropped_sets.is_empty() {
        b.report.note(&format!(
            "{} property sets were not imported (the limit is {MAX_FORMATS} record formats)",
            dropped_sets.len()
        ));
    }
    if dropped_fields > 0 {
        b.report.note(&format!(
            "{dropped_fields} properties were not imported (the limit is {MAX_FIELDS} fields per record)"
        ));
    }

    // 2. Record formats, with names that no resource or object data uses yet.
    let mut taken: HashSet<String> = doc
        .resources
        .iter()
        .map(|r| r.name.to_ascii_lowercase())
        .chain(
            doc.objects
                .values()
                .flat_map(|o| o.data.keys().map(|k| k.to_ascii_lowercase())),
        )
        .chain(RESERVED.iter().map(|s| s.to_string()))
        .collect();
    let mut ids: BTreeMap<String, ResourceId> = BTreeMap::new();
    let mut fields_of: BTreeMap<String, Vec<(String, FieldType)>> = BTreeMap::new();
    for (set, f) in formats {
        let mut base: String = set.chars().filter(|c| !c.is_control()).take(60).collect();
        if base.trim().is_empty() {
            base = "IFC property set".into();
        }
        let mut name = base.clone();
        let mut n = 1;
        while taken.contains(&name.to_ascii_lowercase())
            || is_reserved_record_name(&name)
            || check_name("record", &name).is_err()
        {
            n += 1;
            name = format!("{base} ({n})");
            if n > 1000 {
                break;
            }
        }
        let format = RecordFormat {
            fields: f.fields.iter().map(|(n, t)| FieldDef::new(n, *t)).collect(),
        };
        if format.validate().is_err() {
            b.report.skip(
                "property set",
                &format!("'{set}' has a field name that is not valid"),
            );
            continue;
        }
        taken.insert(name.to_ascii_lowercase());
        let id = b.resource(&name, ResourceBody::RecordFormat(Box::new(format)));
        ids.insert(set.clone(), id);
        fields_of.insert(set, f.fields);
    }

    // 3. The values.
    for (obj, sets) in rows {
        for s in sets {
            let (Some(rid), Some(fields)) = (ids.get(&s.name), fields_of.get(&s.name)) else {
                continue;
            };
            let mut values = Map::new();
            for p in &s.props {
                let Some((fname, fty)) =
                    fields.iter().find(|(n, _)| n.eq_ignore_ascii_case(&p.name))
                else {
                    continue;
                };
                let v = if *fty == FieldType::Text {
                    Value::String(text_of(&p.value))
                } else {
                    p.value.clone()
                };
                values.insert(fname.clone(), v);
            }
            b.push(Command::AttachRecord {
                ids: vec![obj],
                record: *rid,
                values,
            });
        }
    }
}
