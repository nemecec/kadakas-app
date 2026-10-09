//! Document-level resources: reusable named definitions that objects refer to.
//! Plant styles are the first kind. Symbols, hatches and other styles can add
//! a variant to [`ResourceBody`] later, without changing the store.

use serde::{Deserialize, Serialize};

use crate::hardscape::style::HardscapeStyle;
use crate::landscape::style::LandscapeAreaStyle;
use crate::plants::style::PlantStyle;

/// Identifier of a resource. Resources share one ID space with objects, layers and classes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceId(pub u64);

impl std::fmt::Display for ResourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The content of a resource. The `type` key in JSON names the variant.
/// A resource of a type that this version does not know (from a newer app) is kept as raw JSON
/// and saved again unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum ResourceBody {
    PlantStyle(Box<PlantStyle>),
    /// A record format: typed fields that can be attached to objects (see `crate::record`).
    RecordFormat(Box<crate::record::RecordFormat>),
    /// A Data Manager mapping table (see `crate::datamap`).
    DataMap(Box<crate::datamap::DataMap>),
    /// A worksheet definition. The core keeps it as JSON. `cadektor-sheet` reads and writes it.
    Worksheet(serde_json::Value),
    HardscapeStyle(Box<HardscapeStyle>),
    LandscapeAreaStyle(Box<LandscapeAreaStyle>),
    Unknown(serde_json::Value),
}

impl Serialize for ResourceBody {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ResourceBody::PlantStyle(style) => {
                let mut v = serde_json::to_value(style).map_err(serde::ser::Error::custom)?;
                v["type"] = "plant_style".into();
                v.serialize(s)
            }
            ResourceBody::RecordFormat(f) => tagged(f.as_ref(), "record_format", s),
            ResourceBody::DataMap(m) => tagged(m.as_ref(), "data_map", s),
            ResourceBody::Worksheet(v) => v.serialize(s),
            ResourceBody::HardscapeStyle(style) => {
                let mut v = serde_json::to_value(style).map_err(serde::ser::Error::custom)?;
                v["type"] = "hardscape_style".into();
                v.serialize(s)
            }
            ResourceBody::LandscapeAreaStyle(style) => {
                let mut v = serde_json::to_value(style).map_err(serde::ser::Error::custom)?;
                v["type"] = "landscape_area_style".into();
                v.serialize(s)
            }
            ResourceBody::Unknown(v) => v.serialize(s),
        }
    }
}

fn tagged<T: Serialize, S: serde::Serializer>(body: &T, name: &str, s: S) -> Result<S::Ok, S::Error> {
    let mut v = serde_json::to_value(body).map_err(serde::ser::Error::custom)?;
    v["type"] = name.into();
    v.serialize(s)
}

impl<'de> Deserialize<'de> for ResourceBody {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        match v.get("type").and_then(serde_json::Value::as_str) {
            Some("plant_style") => {
                serde_json::from_value::<PlantStyle>(v).map(|s| ResourceBody::PlantStyle(Box::new(s))).map_err(serde::de::Error::custom)
            }
            Some("record_format") => {
                let f: crate::record::RecordFormat = serde_json::from_value(v.clone()).map_err(serde::de::Error::custom)?;
                // A field type from a newer app: keep the whole resource as it is.
                if f.fields.iter().any(|x| x.ty == crate::record::FieldType::Unknown) { Ok(ResourceBody::Unknown(v)) } else { Ok(ResourceBody::RecordFormat(Box::new(f))) }
            }
            Some("data_map") => serde_json::from_value(v).map(|m| ResourceBody::DataMap(Box::new(m))).map_err(serde::de::Error::custom),
            Some("worksheet") => Ok(ResourceBody::Worksheet(v)),
            Some("hardscape_style") => serde_json::from_value::<HardscapeStyle>(v)
                .map(|s| ResourceBody::HardscapeStyle(Box::new(s)))
                .map_err(serde::de::Error::custom),
            Some("landscape_area_style") => serde_json::from_value::<LandscapeAreaStyle>(v)
                .map(|s| ResourceBody::LandscapeAreaStyle(Box::new(s)))
                .map_err(serde::de::Error::custom),
            _ => Ok(ResourceBody::Unknown(v)),
        }
    }
}

impl ResourceBody {
    /// A plant style resource body.
    pub fn plant_style(style: PlantStyle) -> Self {
        ResourceBody::PlantStyle(Box::new(style))
    }

    /// The type name, as used in JSON. Unknown types all give `unknown`.
    pub fn type_name(&self) -> &'static str {
        match self {
            ResourceBody::PlantStyle(_) => "plant_style",
            ResourceBody::RecordFormat(_) => "record_format",
            ResourceBody::DataMap(_) => "data_map",
            ResourceBody::Worksheet(_) => "worksheet",
            ResourceBody::HardscapeStyle(_) => "hardscape_style",
            ResourceBody::LandscapeAreaStyle(_) => "landscape_area_style",
            ResourceBody::Unknown(_) => "unknown",
        }
    }

    /// Checks the numbers and rules of the content. Unknown content is not checked.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ResourceBody::PlantStyle(s) => s.validate(),
            ResourceBody::RecordFormat(f) => f.validate(),
            ResourceBody::DataMap(m) => m.validate(),
            ResourceBody::Worksheet(v) if !v.is_object() => Err("a worksheet is a JSON object".into()),
            ResourceBody::Worksheet(_) => Ok(()),
            ResourceBody::HardscapeStyle(s) => s.validate(),
            ResourceBody::LandscapeAreaStyle(s) => s.validate(),
            ResourceBody::Unknown(_) => Ok(()),
        }
    }
}

/// A named resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub id: ResourceId,
    pub name: String,
    pub body: ResourceBody,
}

/// All resources of a document, in creation order. In files they are a JSON array.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Resources(Vec<Resource>);

impl Resources {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Resource> {
        self.0.iter()
    }

    pub fn get(&self, id: ResourceId) -> Option<&Resource> {
        self.0.iter().find(|r| r.id == id)
    }

    pub fn get_mut(&mut self, id: ResourceId) -> Option<&mut Resource> {
        self.0.iter_mut().find(|r| r.id == id)
    }

    pub fn push(&mut self, r: Resource) {
        self.0.push(r);
    }

    pub fn remove(&mut self, id: ResourceId) -> Option<Resource> {
        let i = self.0.iter().position(|r| r.id == id)?;
        Some(self.0.remove(i))
    }

    /// The plant style with this ID.
    pub fn plant_style(&self, id: ResourceId) -> Option<&PlantStyle> {
        match &self.get(id)?.body {
            ResourceBody::PlantStyle(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    /// All plant styles with their IDs and names.
    pub fn plant_styles(&self) -> impl Iterator<Item = (ResourceId, &str, &PlantStyle)> {
        self.0.iter().filter_map(|r| match &r.body {
            ResourceBody::PlantStyle(s) => Some((r.id, r.name.as_str(), s.as_ref())),
            _ => None,
        })
    }

    /// The data map with this ID.
    pub fn data_map(&self, id: ResourceId) -> Option<&crate::datamap::DataMap> {
        match &self.get(id)?.body {
            ResourceBody::DataMap(m) => Some(m.as_ref()),
            _ => None,
        }
    }

    /// The hardscape style with this ID.
    pub fn hardscape_style(&self, id: ResourceId) -> Option<&HardscapeStyle> {
        match &self.get(id)?.body {
            ResourceBody::HardscapeStyle(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    /// The landscape area style with this ID.
    pub fn landscape_area_style(&self, id: ResourceId) -> Option<&LandscapeAreaStyle> {
        match &self.get(id)?.body {
            ResourceBody::LandscapeAreaStyle(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    /// All hardscape styles with their IDs and names.
    pub fn hardscape_styles(&self) -> impl Iterator<Item = (ResourceId, &str, &HardscapeStyle)> {
        self.0.iter().filter_map(|r| match &r.body {
            ResourceBody::HardscapeStyle(s) => Some((r.id, r.name.as_str(), s.as_ref())),
            _ => None,
        })
    }

    /// All landscape area styles with their IDs and names.
    pub fn landscape_area_styles(&self) -> impl Iterator<Item = (ResourceId, &str, &LandscapeAreaStyle)> {
        self.0.iter().filter_map(|r| match &r.body {
            ResourceBody::LandscapeAreaStyle(s) => Some((r.id, r.name.as_str(), s.as_ref())),
            _ => None,
        })
    }

    /// The ID of the hardscape style with this name (compared without case).
    pub fn find_hardscape_style(&self, name: &str) -> Option<ResourceId> {
        self.hardscape_styles().find(|(_, n, _)| n.eq_ignore_ascii_case(name)).map(|(id, _, _)| id)
    }

    /// The ID of the landscape area style with this name (compared without case).
    pub fn find_landscape_area_style(&self, name: &str) -> Option<ResourceId> {
        self.landscape_area_styles().find(|(_, n, _)| n.eq_ignore_ascii_case(name)).map(|(id, _, _)| id)
    }

    /// The landscape area styles whose mix uses this plant style.
    pub fn mixes_using(&self, plant: ResourceId) -> Vec<ResourceId> {
        self.landscape_area_styles().filter(|(_, _, s)| s.mix.iter().any(|e| e.plant == plant)).map(|(id, _, _)| id).collect()
    }

    /// The ID of the plant style with this name (compared without case).
    pub fn find_plant_style(&self, name: &str) -> Option<ResourceId> {
        self.plant_styles().find(|(_, n, _)| n.eq_ignore_ascii_case(name)).map(|(id, _, _)| id)
    }

    /// Checks every resource: valid content, a non-empty name, and a name that no other
    /// resource of the same type has.
    pub fn validate(&self) -> Result<(), String> {
        for (i, r) in self.0.iter().enumerate() {
            if r.name.trim().is_empty() {
                return Err(format!("resource {} has no name", r.id));
            }
            r.body.validate().map_err(|e| format!("resource {}: {e}", r.id))?;
            if !matches!(r.body, ResourceBody::Unknown(_)) && self.0[..i].iter().any(|o| o.body.type_name() == r.body.type_name() && o.name.eq_ignore_ascii_case(&r.name)) {
                return Err(format!("two {} resources are named '{}'", r.body.type_name(), r.name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(id: u64, name: &str) -> Resource {
        Resource { id: ResourceId(id), name: name.into(), body: ResourceBody::plant_style(PlantStyle::default()) }
    }

    #[test]
    fn store_basics() {
        let mut r = Resources::default();
        assert!(r.is_empty());
        r.push(res(5, "Oak"));
        r.push(res(6, "Birch"));
        assert_eq!(r.len(), 2);
        assert_eq!(r.get(ResourceId(6)).unwrap().name, "Birch");
        assert!(r.plant_style(ResourceId(5)).is_some());
        assert!(r.plant_style(ResourceId(9)).is_none());
        assert_eq!(r.find_plant_style("oak"), Some(ResourceId(5)));
        assert_eq!(r.find_plant_style("elm"), None);
        r.get_mut(ResourceId(5)).unwrap().name = "Elm".into();
        assert_eq!(r.find_plant_style("elm"), Some(ResourceId(5)));
        assert_eq!(r.plant_styles().count(), 2);
        assert!(r.remove(ResourceId(5)).is_some());
        assert!(r.remove(ResourceId(5)).is_none());
        assert_eq!(r.iter().count(), 1);
        assert_eq!(ResourceId(3).to_string(), "3");
    }

    #[test]
    fn validate_rejects_bad_names_and_content() {
        let mut r = Resources::default();
        r.push(res(1, "Oak"));
        assert!(r.validate().is_ok());
        r.push(res(2, "OAK"));
        assert!(r.validate().unwrap_err().contains("named"));
        r.remove(ResourceId(2));
        r.push(res(3, "  "));
        assert!(r.validate().unwrap_err().contains("no name"));
        r.remove(ResourceId(3));
        let mut bad = res(4, "Bad");
        let ResourceBody::PlantStyle(s) = &mut bad.body else { panic!() };
        s.spread = f64::NAN;
        r.push(bad);
        assert!(r.validate().unwrap_err().contains("resource 4"));
    }

    #[test]
    fn unknown_types_are_kept_and_saved_again() {
        let j = serde_json::json!({"id": 9, "name": "Hatch 1", "body": {"type": "hatch", "angle": 45, "rows": [1, 2]}});
        let r: Resource = serde_json::from_value(j.clone()).unwrap();
        assert!(matches!(r.body, ResourceBody::Unknown(_)));
        assert_eq!(r.body.type_name(), "unknown");
        assert!(r.body.validate().is_ok());
        assert_eq!(serde_json::to_value(&r).unwrap(), j);
        let mut store = Resources::default();
        store.push(r.clone());
        store.push(Resource { id: ResourceId(10), name: "Hatch 1".into(), body: r.body.clone() });
        assert!(store.validate().is_ok(), "unknown resources may share a name");
        assert!(store.plant_style(ResourceId(9)).is_none());
        assert_eq!(store.plant_styles().count(), 0);
        // A body without a type is also kept.
        let r: Resource = serde_json::from_value(serde_json::json!({"id": 1, "name": "x", "body": 5})).unwrap();
        assert_eq!(r.body, ResourceBody::Unknown(serde_json::json!(5)));
        // A plant style with a bad field is an error.
        let bad = serde_json::json!({"id": 1, "name": "x", "body": {"type": "plant_style", "spread": "wide"}});
        assert!(serde_json::from_value::<Resource>(bad).is_err());
    }

    #[test]
    fn data_resources_round_trip() {
        use crate::record::{FieldDef, FieldType, RecordFormat};
        let fmt = RecordFormat { fields: vec![FieldDef::new("Depth", FieldType::Dimension)] };
        let r = Resource { id: ResourceId(1), name: "Soil".into(), body: ResourceBody::RecordFormat(Box::new(fmt)) };
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["body"]["type"], "record_format");
        assert_eq!(serde_json::from_value::<Resource>(j).unwrap(), r);
        assert_eq!(r.body.type_name(), "record_format");
        let m = Resource { id: ResourceId(2), name: "Map".into(), body: ResourceBody::DataMap(Box::new(crate::datamap::DataMap::landscape_template())) };
        let j = serde_json::to_value(&m).unwrap();
        assert_eq!(j["body"]["type"], "data_map");
        assert_eq!(serde_json::from_value::<Resource>(j).unwrap(), m);
        assert!(m.body.validate().is_ok());
        let mut store = Resources::default();
        store.push(m);
        assert!(store.data_map(ResourceId(2)).is_some());
        assert!(store.data_map(ResourceId(1)).is_none());
        let w = Resource { id: ResourceId(3), name: "Sheet".into(), body: ResourceBody::Worksheet(serde_json::json!({"type": "worksheet", "cells": {}})) };
        let j = serde_json::to_value(&w).unwrap();
        assert_eq!(serde_json::from_value::<Resource>(j).unwrap(), w);
        assert_eq!(w.body.type_name(), "worksheet");
        assert!(w.body.validate().is_ok());
        assert!(ResourceBody::Worksheet(serde_json::json!(5)).validate().is_err());
        // A field type from the future keeps the resource, unchanged.
        let future = serde_json::json!({"id": 4, "name": "Later", "body": {"type": "record_format", "fields": [{"name": "When", "type": "hologram"}]}});
        let r: Resource = serde_json::from_value(future.clone()).unwrap();
        assert!(matches!(r.body, ResourceBody::Unknown(_)));
        assert_eq!(serde_json::to_value(&r).unwrap(), future);
        let bad = serde_json::json!({"id": 1, "name": "x", "body": {"type": "record_format", "fields": 5}});
        assert!(serde_json::from_value::<Resource>(bad).is_err());
        let bad = serde_json::json!({"id": 1, "name": "x", "body": {"type": "data_map", "rules": 5}});
        assert!(serde_json::from_value::<Resource>(bad).is_err());
        let bad_fmt = ResourceBody::RecordFormat(Box::new(RecordFormat { fields: vec![FieldDef::new("", FieldType::Text)] }));
        assert!(bad_fmt.validate().is_err());
    }

    #[test]
    fn json_shape() {
        let j = serde_json::to_value(res(1, "Oak")).unwrap();
        assert_eq!(j["body"]["type"], "plant_style");
        assert_eq!(j["name"], "Oak");
        let back: Resource = serde_json::from_value(j).unwrap();
        assert_eq!(back, res(1, "Oak"));
        assert_eq!(back.body.type_name(), "plant_style");
    }
}
