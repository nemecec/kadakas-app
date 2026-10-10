//! Property sets and quantity sets as records. One record format is made for every set name, with
//! one field for every property that any object has. An object keeps the values of its own set.
//!
//! - A record format of the document with the same name and compatible fields is reused, and new
//!   fields are added to it. So an import of a second file, or the same file again, adds no formats.
//! - Many one-of-a-kind sets would fill the Resource Manager. Past a limit, a set that only one object
//!   has goes into the data of that object (`ifc_psets`) instead of a record format.

use std::collections::{BTreeMap, HashMap, HashSet};

use kadakas_core::command::Command;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::record::{
    FieldDef, FieldType, RecordFormat, check_name, is_reserved_record_name,
};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_io::builder::Builder;
use serde_json::{Map, Value};

use super::model::PropSet;

/// Most record formats that one import makes.
const MAX_FORMATS: usize = 300;
/// Most fields in a record format.
const MAX_FIELDS: usize = 120;
/// Past this many formats, sets that only one object has go into the object data.
const SINGLE_LIMIT: usize = 40;
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

/// The record format of the document that a set can use: same name, and every field that both have
/// has the same type, or the field of the document is text.
fn reusable(
    doc: &Document,
    name: &str,
    fields: &[(String, FieldType)],
) -> Option<(ResourceId, Vec<FieldDef>)> {
    let r = doc.resources.iter().find(|r| {
        r.name.eq_ignore_ascii_case(name) && matches!(r.body, ResourceBody::RecordFormat(_))
    })?;
    let ResourceBody::RecordFormat(f) = &r.body else {
        return None;
    };
    let ok = fields.iter().all(|(n, t)| match f.field(n) {
        Some(e) => e.ty == *t || e.ty == FieldType::Text,
        None => true,
    });
    let added = fields.iter().filter(|(n, _)| f.field(n).is_none()).count();
    (ok && f.fields.len() + added <= MAX_FIELDS).then(|| (r.id, f.fields.clone()))
}

pub fn write_records(b: &mut Builder, doc: &Document, rows: Vec<(ObjectId, Vec<PropSet>)>) {
    // 1. Who has which set, and its fields.
    let mut formats: BTreeMap<String, Fmt> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut users: HashMap<String, usize> = HashMap::new();
    let mut dropped_fields = 0usize;
    for (_, sets) in &rows {
        for s in sets {
            *users.entry(s.name.clone()).or_default() += 1;
            if !formats.contains_key(&s.name) {
                order.push(s.name.clone());
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
    if dropped_fields > 0 {
        b.report.note(&format!(
            "{dropped_fields} properties were not imported (the limit is {MAX_FIELDS} fields per record)"
        ));
    }

    // 2. Which sets are record formats. Sets of many objects first, then one-of-a-kind sets while there is room.
    let mut as_record: HashSet<String> = HashSet::new();
    for name in order.iter().filter(|n| users[*n] > 1) {
        if as_record.len() < MAX_FORMATS {
            as_record.insert(name.clone());
        }
    }
    for name in order.iter().filter(|n| users[*n] == 1) {
        if as_record.len() < SINGLE_LIMIT {
            as_record.insert(name.clone());
        }
    }
    let in_data = order.iter().filter(|n| !as_record.contains(*n)).count();
    if in_data > 0 {
        b.report.note(&format!(
            "{in_data} property sets went into the data of their objects (ifc_psets) to keep the number of record formats small"
        ));
    }

    // 3. Record formats: reuse one of the document, or make one with a name that nothing uses yet.
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
    let mut reused = 0usize;
    let record_sets: Vec<String> = order
        .iter()
        .filter(|n| as_record.contains(*n))
        .cloned()
        .collect();
    for set in &record_sets {
        let f = &formats[set];
        if let Some((id, existing)) = reusable(doc, set, &f.fields) {
            let mut all = existing.clone();
            for (n, t) in &f.fields {
                if !all.iter().any(|e| e.name.eq_ignore_ascii_case(n)) {
                    all.push(FieldDef::new(n, *t));
                }
            }
            if all.len() > existing.len() {
                b.push(Command::ModifyResource {
                    id,
                    name: None,
                    body: Some(ResourceBody::RecordFormat(Box::new(RecordFormat {
                        fields: all.clone(),
                    }))),
                });
            }
            fields_of.insert(
                set.clone(),
                all.iter().map(|d| (d.name.clone(), d.ty)).collect(),
            );
            ids.insert(set.clone(), id);
            reused += 1;
            continue;
        }
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
            as_record.remove(set);
            continue;
        }
        taken.insert(name.to_ascii_lowercase());
        let id = b.resource(&name, ResourceBody::RecordFormat(Box::new(format)));
        ids.insert(set.clone(), id);
        fields_of.insert(set.clone(), f.fields.clone());
    }
    if reused > 0 {
        b.report.note(&format!(
            "{reused} property sets use record formats that the document had already"
        ));
    }

    // 4. The values.
    for (obj, sets) in rows {
        let mut data_sets = Map::new();
        for s in sets {
            let (Some(rid), Some(fields)) = (ids.get(&s.name), fields_of.get(&s.name)) else {
                let mut m = Map::new();
                for p in &s.props {
                    m.insert(p.name.clone(), p.value.clone());
                }
                data_sets.insert(s.name.clone(), Value::Object(m));
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
        if !data_sets.is_empty() {
            let mut set = BTreeMap::new();
            set.insert("ifc_psets".to_string(), Value::Object(data_sets));
            b.push(Command::SetData {
                id: obj,
                set,
                remove: vec![],
            });
        }
    }
}
