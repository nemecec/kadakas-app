//! The Data Manager: a declarative mapping table from object parameters to record fields and
//! to IFC property sets.
//!
//! A data map is a resource (`ResourceBody::DataMap`). It has two tables.
//!
//! - `entities` give the IFC class of each kind of object (for example `plant` is an
//!   `IfcGeographicElement`).
//! - `rules` read a source on the object (a built-in property, a plant style field, a record
//!   field or a constant) and write it to a target (an IFC property in a property set, or a field
//!   of an attached record).
//!
//! [`evaluate`] applies the map to one object and returns the IFC class, the IFC properties
//! and the record values. The IFC export and data tags use this result. [`record_commands`]
//! turns the record targets into commands. Nothing here changes the document by itself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::command::Command;
use crate::model::{Document, Object};
use crate::props::{self, Prop};
use crate::record;
use crate::resource::ResourceBody;

/// The IFC value type of a mapped property.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfcType {
    /// IfcLabel: short text.
    #[default]
    Label,
    /// IfcText: long text.
    Text,
    Real,
    Integer,
    Boolean,
    /// IfcLengthMeasure. The number is in the unit of the source property.
    Length,
    /// IfcAreaMeasure.
    Area,
    /// IfcVolumeMeasure.
    Volume,
}

impl IfcType {
    pub fn id(self) -> &'static str {
        match self {
            IfcType::Label => "label",
            IfcType::Text => "text",
            IfcType::Real => "real",
            IfcType::Integer => "integer",
            IfcType::Boolean => "boolean",
            IfcType::Length => "length",
            IfcType::Area => "area",
            IfcType::Volume => "volume",
        }
    }

    /// Converts a value to this IFC type.
    pub fn coerce(self, v: &Value) -> Result<Value, String> {
        let bad = || Err(format!("cannot read {v} as {}", self.id()));
        match self {
            IfcType::Label | IfcType::Text => match v {
                Value::String(_) => Ok(v.clone()),
                Value::Number(n) => Ok(Value::String(n.to_string())),
                Value::Bool(b) => Ok(Value::String(b.to_string())),
                _ => bad(),
            },
            IfcType::Boolean => v.as_bool().map(Value::Bool).map_or_else(bad, Ok),
            IfcType::Integer => match v.as_f64() {
                Some(n) if n.is_finite() && n.fract() == 0.0 => Ok(Value::from(n as i64)),
                _ => bad(),
            },
            IfcType::Real | IfcType::Length | IfcType::Area | IfcType::Volume => match v.as_f64() {
                Some(n) if n.is_finite() => Ok(Value::from(n)),
                _ => bad(),
            },
        }
    }
}

/// Where a rule reads its value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// A built-in property such as `Area`, `Layer` or `Class` (see `props::NAMES`).
    Property { name: String },
    /// A field of the effective plant style, such as `price`.
    PlantField { field: String },
    /// A field of an attached record.
    RecordField { record: String, field: String },
    /// A fixed value.
    Constant { value: Value },
}

/// Where a rule writes its value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    /// A property in an IFC property set.
    IfcProperty {
        pset: String,
        property: String,
        #[serde(default, rename = "ifc_type")]
        ty: IfcType,
    },
    /// A field of a record on the object. The object needs the record attached.
    RecordField { record: String, field: String },
}

/// One mapping rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapRule {
    /// A note for people.
    #[serde(default)]
    pub name: String,
    /// The object kinds that the rule applies to (`Shape::kind` names). Empty means all.
    #[serde(default)]
    pub kinds: Vec<String>,
    pub source: Source,
    pub target: Target,
}

/// The IFC class of one kind of object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityMap {
    /// A `Shape::kind` name.
    pub kind: String,
    pub ifc_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predefined_type: Option<String>,
}

/// A mapping table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DataMap {
    pub entities: Vec<EntityMap>,
    pub rules: Vec<MapRule>,
}

/// One IFC property value.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IfcValue {
    pub ifc_type: IfcType,
    pub value: Value,
}

/// The result of applying a data map to one object.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Mapped {
    pub ifc_class: Option<String>,
    pub predefined_type: Option<String>,
    /// Property sets: set name, then property name.
    pub properties: BTreeMap<String, BTreeMap<String, IfcValue>>,
    /// Record values to write: record name, then field name.
    pub record_values: BTreeMap<String, Map<String, Value>>,
    /// Rules that could not run, with the reason. A rule whose source does not apply to the
    /// object (a hardscape has no `Height`) is not a problem and is left out.
    pub problems: Vec<String>,
}

fn name_ok(kind: &str, s: &str) -> Result<(), String> {
    record::check_name(kind, s)
}

impl DataMap {
    /// Checks the table itself (not the document that it is applied to).
    pub fn validate(&self) -> Result<(), String> {
        for (i, e) in self.entities.iter().enumerate() {
            name_ok("kind", &e.kind)?;
            name_ok("IFC class", &e.ifc_class)?;
            if !e.ifc_class.trim().to_ascii_lowercase().starts_with("ifc") {
                return Err(format!("the IFC class '{}' must start with Ifc", e.ifc_class.trim()));
            }
            if self.entities[..i].iter().any(|o| o.kind.eq_ignore_ascii_case(e.kind.trim())) {
                return Err(format!("kind '{}' is mapped twice", e.kind.trim()));
            }
        }
        for (i, r) in self.rules.iter().enumerate() {
            let at = |m: String| format!("rule {}: {m}", i + 1);
            match &r.source {
                Source::Property { name } => name_ok("property", name).map_err(&at)?,
                Source::PlantField { field } => name_ok("plant field", field).map_err(&at)?,
                Source::RecordField { record, field } => {
                    name_ok("record", record).map_err(&at)?;
                    name_ok("field", field).map_err(&at)?;
                }
                Source::Constant { value } => {
                    if value.is_null() || value.is_array() || value.is_object() || value.as_f64().is_some_and(|n| !n.is_finite()) {
                        return Err(at("a constant must be text, a finite number or a boolean".into()));
                    }
                }
            }
            match &r.target {
                Target::IfcProperty { pset, property, .. } => {
                    name_ok("property set", pset).map_err(&at)?;
                    name_ok("IFC property", property).map_err(&at)?;
                }
                Target::RecordField { record, field } => {
                    name_ok("record", record).map_err(&at)?;
                    name_ok("field", field).map_err(&at)?;
                }
            }
        }
        Ok(())
    }

    /// Problems of the table against a document: records and fields that do not exist.
    pub fn check_refs(&self, doc: &Document) -> Vec<String> {
        let mut out = Vec::new();
        let mut check = |what: &str, record_name: &str, field: &str| match doc.resources.record_format_by_name(record_name) {
            None => out.push(format!("{what}: no record format '{record_name}'")),
            Some((_, f)) if f.field(field).is_none() => out.push(format!("{what}: record '{record_name}' has no field '{field}'")),
            _ => {}
        };
        for (i, r) in self.rules.iter().enumerate() {
            let what = format!("rule {}", i + 1);
            if let Source::RecordField { record, field } = &r.source {
                check(&what, record, field);
            }
            if let Target::RecordField { record, field } = &r.target {
                check(&what, record, field);
            }
        }
        out
    }

    fn entity(&self, kind: &str) -> Option<&EntityMap> {
        self.entities.iter().find(|e| e.kind.trim().eq_ignore_ascii_case(kind))
    }

    /// A starting table for landscape objects. Property set names start with `Cadektor_`.
    pub fn landscape_template() -> DataMap {
        let ent = |kind: &str, class: &str, pre: Option<&str>| EntityMap { kind: kind.into(), ifc_class: class.into(), predefined_type: pre.map(Into::into) };
        let ifc = |pset: &str, property: &str, ty: IfcType| Target::IfcProperty { pset: pset.into(), property: property.into(), ty };
        let prop = |n: &str| Source::Property { name: n.into() };
        let rule = |name: &str, kinds: &[&str], source: Source, target: Target| MapRule { name: name.into(), kinds: kinds.iter().map(|k| k.to_string()).collect(), source, target };
        let plants = ["plant", "existing_tree"];
        DataMap {
            entities: vec![
                ent("plant", "IfcGeographicElement", Some("VEGETATION")),
                ent("existing_tree", "IfcGeographicElement", Some("VEGETATION")),
                ent("hardscape", "IfcPavement", None),
                ent("landscape_area", "IfcGeographicElement", None),
                ent("site_model", "IfcGeographicElement", Some("TERRAIN")),
            ],
            rules: vec![
                rule("Layer", &[], prop("Layer"), ifc("Cadektor_Source", "Layer", IfcType::Label)),
                rule("Class", &[], prop("Class"), ifc("Cadektor_Source", "Class", IfcType::Label)),
                rule("Botanical name", &plants, prop("Botanical"), ifc("Cadektor_Planting", "BotanicalName", IfcType::Label)),
                rule("Common name", &["plant"], prop("CommonName"), ifc("Cadektor_Planting", "CommonName", IfcType::Label)),
                rule("Plant code", &["plant"], prop("IdCode"), ifc("Cadektor_Planting", "Code", IfcType::Label)),
                rule("Spread", &plants, prop("Spread"), ifc("Cadektor_Planting", "Spread", IfcType::Length)),
                rule("Height", &plants, prop("Height"), ifc("Cadektor_Planting", "Height", IfcType::Length)),
                rule("Trunk diameter", &["existing_tree"], prop("DBH"), ifc("Cadektor_Planting", "TrunkDiameter", IfcType::Length)),
                rule("Material", &["hardscape"], prop("Material"), ifc("Cadektor_Hardscape", "Material", IfcType::Label)),
                rule("Thickness", &["hardscape"], prop("Thickness"), ifc("Cadektor_Hardscape", "Thickness", IfcType::Length)),
                rule("Area", &["hardscape", "landscape_area"], prop("Area"), ifc("Cadektor_Quantities", "Area", IfcType::Area)),
                rule("Volume", &["hardscape"], prop("Volume"), ifc("Cadektor_Quantities", "Volume", IfcType::Volume)),
            ],
        }
    }
}

fn source_value(doc: &Document, obj: &Object, s: &Source) -> Prop {
    match s {
        Source::Property { name } => props::property(doc, obj, &[name]),
        Source::PlantField { field } => props::property(doc, obj, &["Plant", field]),
        Source::RecordField { record, field } => props::property(doc, obj, &[record, field]),
        Source::Constant { value } => Prop::Value(value.clone()),
    }
}

/// Applies the table to one object.
pub fn evaluate(doc: &Document, map: &DataMap, obj: &Object) -> Mapped {
    let kind = obj.shape.kind();
    let mut out = Mapped::default();
    if let Some(e) = map.entity(kind) {
        out.ifc_class = Some(e.ifc_class.trim().to_string());
        out.predefined_type = e.predefined_type.clone();
    }
    for (i, r) in map.rules.iter().enumerate() {
        if !r.kinds.is_empty() && !r.kinds.iter().any(|k| k.trim().eq_ignore_ascii_case(kind)) {
            continue;
        }
        let label = if r.name.is_empty() { format!("rule {}", i + 1) } else { format!("rule {} ({})", i + 1, r.name) };
        let v = match source_value(doc, obj, &r.source) {
            Prop::Value(v) => v,
            Prop::NotApplicable => continue,
            Prop::Unknown => {
                out.problems.push(format!("{label}: the source does not exist"));
                continue;
            }
        };
        match &r.target {
            Target::IfcProperty { pset, property, ty } => match ty.coerce(&v) {
                Ok(value) => {
                    out.properties.entry(pset.trim().to_string()).or_default().insert(property.trim().to_string(), IfcValue { ifc_type: *ty, value });
                }
                Err(e) => out.problems.push(format!("{label}: {e}")),
            },
            Target::RecordField { record, field } => {
                let Some((rid, fmt)) = doc.resources.record_format_by_name(record) else {
                    out.problems.push(format!("{label}: no record format '{record}'"));
                    continue;
                };
                let Some(def) = fmt.field(field) else {
                    out.problems.push(format!("{label}: record '{record}' has no field '{field}'"));
                    continue;
                };
                match def.coerce(&v) {
                    Ok(value) => {
                        let name = doc.resources.get(rid).map(|r| r.name.clone()).unwrap_or_default();
                        out.record_values.entry(name).or_default().insert(def.name.clone(), value);
                    }
                    Err(e) => out.problems.push(format!("{label}: {e}")),
                }
            }
        }
    }
    out
}

/// Commands that write the record targets of a map into the objects. Only objects that have
/// the record attached are changed, and only fields whose value differs. One command per
/// object and record. Returns the commands and the problems that `evaluate` found.
pub fn record_commands(doc: &Document, map: &DataMap) -> (Vec<Command>, Vec<String>) {
    let mut cmds = Vec::new();
    let mut problems = Vec::new();
    for obj in doc.objects.values() {
        let m = evaluate(doc, map, obj);
        problems.extend(m.problems.iter().map(|p| format!("object {}: {p}", obj.id)));
        for (record_name, values) in m.record_values {
            let Some((rid, fmt)) = doc.resources.record_format_by_name(&record_name) else { continue };
            let name = doc.resources.get(rid).map(|r| r.name.clone()).unwrap_or_default();
            let Some(current) = record::values(obj, &name) else { continue };
            let changed: Map<String, Value> = values.into_iter().filter(|(k, v)| fmt.field(k).is_some() && current.get(k) != Some(v)).collect();
            if !changed.is_empty() {
                cmds.push(Command::SetRecordFields { ids: vec![obj.id], record: rid, values: changed });
            }
        }
    }
    (cmds, problems)
}

impl crate::resource::Resources {
    /// The record format with this name (compared without case).
    pub fn record_format_by_name(&self, name: &str) -> Option<(crate::resource::ResourceId, &record::RecordFormat)> {
        self.iter().find_map(|r| match &r.body {
            ResourceBody::RecordFormat(f) if r.name.eq_ignore_ascii_case(name.trim()) => Some((r.id, f.as_ref())),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Editor;
    use crate::model::{Attributes, ObjectId, Shape};
    use crate::record::{FieldDef, FieldType, RecordFormat};
    use glam::DVec2;
    use serde_json::json;

    fn setup() -> (Editor, ObjectId) {
        let mut ed = Editor::default();
        let b = crate::curve::corners(&[DVec2::ZERO, DVec2::new(2000.0, 0.0), DVec2::new(2000.0, 3000.0), DVec2::new(0.0, 3000.0)]);
        let out = ed.execute(&Command::AddObject { shape: Shape::Hardscape { boundary: b, material: "Granite".into(), thickness: None, elevation: None }, layer: None, class: None, attrs: Attributes::default() }).unwrap();
        (ed, ObjectId(out.created[0]))
    }

    fn add_record(ed: &mut Editor) -> crate::resource::ResourceId {
        let fmt = RecordFormat { fields: vec![FieldDef::new("Area", FieldType::Area), FieldDef::new("Mat", FieldType::Text), FieldDef::new("Qty", FieldType::Integer)] };
        crate::resource::ResourceId(ed.execute(&Command::AddResource { name: "Takeoff".into(), body: ResourceBody::RecordFormat(Box::new(fmt)) }).unwrap().created[0])
    }

    #[test]
    fn template_is_valid_and_maps_a_hardscape() {
        let (ed, id) = setup();
        let map = DataMap::landscape_template();
        map.validate().unwrap();
        let m = evaluate(ed.doc(), &map, ed.doc().object(id).unwrap());
        assert_eq!(m.ifc_class.as_deref(), Some("IfcPavement"));
        assert_eq!(m.predefined_type, None);
        assert!(m.problems.is_empty(), "{:?}", m.problems);
        let q = &m.properties["Cadektor_Quantities"];
        assert_eq!(q["Area"], IfcValue { ifc_type: IfcType::Area, value: json!(6.0) });
        assert_eq!(m.properties["Cadektor_Hardscape"]["Material"].value, json!("Granite"));
        assert_eq!(m.properties["Cadektor_Source"]["Class"].value, json!("None"));
        assert!(!m.properties.contains_key("Cadektor_Planting"));
        // Round trip through JSON.
        let back: DataMap = serde_json::from_value(serde_json::to_value(&map).unwrap()).unwrap();
        assert_eq!(back, map);
    }

    #[test]
    fn plants_and_trees_map_to_vegetation() {
        let mut ed = Editor::default();
        let add = |ed: &mut Editor, s: Shape| ObjectId(ed.execute(&Command::AddObject { shape: s, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0]);
        let p = add(&mut ed, Shape::Plant { at: DVec2::ZERO, species: "Birch".into(), spread: 2000.0, height: 5000.0 });
        let t = add(&mut ed, Shape::ExistingTree { at: DVec2::ZERO, species: "Oak".into(), dbh: 400.0, spread: 6000.0, height: 12000.0, condition: Default::default(), action: Default::default(), rpz_factor: None, rpz_cap: None });
        let map = DataMap::landscape_template();
        let m = evaluate(ed.doc(), &map, ed.doc().object(p).unwrap());
        assert_eq!(m.predefined_type.as_deref(), Some("VEGETATION"));
        assert_eq!(m.properties["Cadektor_Planting"]["Spread"].value, json!(2000.0));
        let m = evaluate(ed.doc(), &map, ed.doc().object(t).unwrap());
        assert_eq!(m.properties["Cadektor_Planting"]["TrunkDiameter"].value, json!(400.0));
        assert_eq!(m.properties["Cadektor_Planting"]["BotanicalName"].value, json!("Oak"));
    }

    #[test]
    fn record_targets_and_commands() {
        let (mut ed, id) = setup();
        let rid = add_record(&mut ed);
        let map = DataMap {
            entities: vec![],
            rules: vec![
                MapRule { name: "area".into(), kinds: vec![], source: Source::Property { name: "Area".into() }, target: Target::RecordField { record: "takeoff".into(), field: "area".into() } },
                MapRule { name: String::new(), kinds: vec!["Hardscape".into()], source: Source::Constant { value: json!("stone") }, target: Target::RecordField { record: "Takeoff".into(), field: "Mat".into() } },
                MapRule { name: String::new(), kinds: vec!["plant".into()], source: Source::Constant { value: json!(1) }, target: Target::RecordField { record: "Takeoff".into(), field: "Qty".into() } },
            ],
        };
        map.validate().unwrap();
        assert!(map.check_refs(ed.doc()).is_empty());
        // Not attached: nothing to write.
        let (cmds, problems) = record_commands(ed.doc(), &map);
        assert!(cmds.is_empty() && problems.is_empty());
        ed.execute(&Command::AttachRecord { ids: vec![id], record: rid, values: Map::new() }).unwrap();
        let (cmds, problems) = record_commands(ed.doc(), &map);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(cmds.len(), 1);
        for c in &cmds {
            ed.execute(c).unwrap();
        }
        let vals = record::values(ed.doc().object(id).unwrap(), "Takeoff").unwrap();
        assert_eq!(vals["Area"], json!(6.0));
        assert_eq!(vals["Mat"], json!("stone"));
        assert_eq!(vals["Qty"], json!(0));
        // A second run finds nothing to change.
        assert!(record_commands(ed.doc(), &map).0.is_empty());
    }

    #[test]
    fn problems_are_reported() {
        let (mut ed, id) = setup();
        add_record(&mut ed);
        let rule = |source, target| MapRule { name: String::new(), kinds: vec![], source, target };
        let map = DataMap {
            entities: vec![],
            rules: vec![
                rule(Source::Property { name: "Bogus".into() }, Target::IfcProperty { pset: "P".into(), property: "X".into(), ty: IfcType::Label }),
                rule(Source::Property { name: "Layer".into() }, Target::IfcProperty { pset: "P".into(), property: "N".into(), ty: IfcType::Real }),
                rule(Source::Property { name: "Layer".into() }, Target::RecordField { record: "Nope".into(), field: "F".into() }),
                rule(Source::Property { name: "Layer".into() }, Target::RecordField { record: "Takeoff".into(), field: "F".into() }),
                rule(Source::Property { name: "Layer".into() }, Target::RecordField { record: "Takeoff".into(), field: "Qty".into() }),
                rule(Source::Property { name: "Height".into() }, Target::IfcProperty { pset: "P".into(), property: "H".into(), ty: IfcType::Real }),
            ],
        };
        let m = evaluate(ed.doc(), &map, ed.doc().object(id).unwrap());
        assert_eq!(m.problems.len(), 5, "{:?}", m.problems);
        assert!(m.problems[0].contains("rule 1") && m.problems[0].contains("does not exist"));
        assert!(m.problems[1].contains("cannot read"));
        assert!(m.problems[2].contains("no record format"));
        assert!(m.problems[3].contains("no field"));
        assert!(m.properties.is_empty() && m.record_values.is_empty());
        assert_eq!(map.check_refs(ed.doc()).len(), 2);
        let (cmds, problems) = record_commands(ed.doc(), &map);
        assert!(cmds.is_empty());
        assert_eq!(problems.len(), 5);
        assert!(problems[0].starts_with("object "));
    }

    #[test]
    fn validate_rejects_bad_tables() {
        let ok = DataMap::landscape_template();
        let mut m = ok.clone();
        m.entities.push(m.entities[0].clone());
        assert!(m.validate().unwrap_err().contains("twice"));
        let mut m = ok.clone();
        m.entities[0].ifc_class = "Slab".into();
        assert!(m.validate().unwrap_err().contains("start with Ifc"));
        let mut m = ok.clone();
        m.entities[0].kind = " ".into();
        assert!(m.validate().is_err());
        let mut m = ok.clone();
        m.entities[0].ifc_class = " ".into();
        assert!(m.validate().is_err());
        let rule = |source, target| DataMap { entities: vec![], rules: vec![MapRule { name: String::new(), kinds: vec![], source, target }] };
        let t = Target::RecordField { record: "R".into(), field: "F".into() };
        let ifc = Target::IfcProperty { pset: "P".into(), property: "X".into(), ty: IfcType::Text };
        assert!(rule(Source::Property { name: "".into() }, t.clone()).validate().unwrap_err().starts_with("rule 1"));
        assert!(rule(Source::PlantField { field: "".into() }, t.clone()).validate().is_err());
        assert!(rule(Source::RecordField { record: "".into(), field: "f".into() }, t.clone()).validate().is_err());
        assert!(rule(Source::RecordField { record: "r".into(), field: "".into() }, t.clone()).validate().is_err());
        assert!(rule(Source::Constant { value: json!(null) }, t.clone()).validate().is_err());
        assert!(rule(Source::Constant { value: json!([1]) }, t.clone()).validate().is_err());
        assert!(rule(Source::Constant { value: json!(true) }, t.clone()).validate().is_ok());
        let p = Source::Property { name: "Area".into() };
        assert!(rule(p.clone(), Target::IfcProperty { pset: "".into(), property: "X".into(), ty: IfcType::Text }).validate().is_err());
        assert!(rule(p.clone(), Target::IfcProperty { pset: "P".into(), property: "".into(), ty: IfcType::Text }).validate().is_err());
        assert!(rule(p.clone(), Target::RecordField { record: "".into(), field: "F".into() }).validate().is_err());
        assert!(rule(p.clone(), Target::RecordField { record: "R".into(), field: "".into() }).validate().is_err());
        assert!(rule(p, ifc).validate().is_ok());
    }

    #[test]
    fn ifc_types_coerce() {
        assert_eq!(IfcType::Label.coerce(&json!(3)).unwrap(), json!("3"));
        assert_eq!(IfcType::Text.coerce(&json!(true)).unwrap(), json!("true"));
        assert_eq!(IfcType::Label.coerce(&json!("a")).unwrap(), json!("a"));
        assert!(IfcType::Label.coerce(&json!(null)).is_err());
        assert_eq!(IfcType::Boolean.coerce(&json!(true)).unwrap(), json!(true));
        assert!(IfcType::Boolean.coerce(&json!(1)).is_err());
        assert_eq!(IfcType::Integer.coerce(&json!(2.0)).unwrap(), json!(2));
        assert!(IfcType::Integer.coerce(&json!(2.5)).is_err());
        assert_eq!(IfcType::Length.coerce(&json!(2.5)).unwrap(), json!(2.5));
        assert!(IfcType::Area.coerce(&json!("x")).is_err());
        for t in [IfcType::Label, IfcType::Text, IfcType::Real, IfcType::Integer, IfcType::Boolean, IfcType::Length, IfcType::Area, IfcType::Volume] {
            assert!(!t.id().is_empty());
        }
    }
}
