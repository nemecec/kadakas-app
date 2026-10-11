//! Document-level resources: reusable named definitions that objects refer to.
//! Plant styles are the first kind. Symbols, hatches and other styles can add
//! a variant to [`ResourceBody`] later, without changing the store.

use serde::{Deserialize, Serialize};

use crate::hardscape::style::HardscapeStyle;
use crate::landscape::style::LandscapeAreaStyle;
use crate::plants::style::PlantStyle;

crate::uid::id_type!(
    /// Identifier of a resource. Like all IDs, it is unique in the world (see `crate::uid`).
    ResourceId
);

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
    /// A worksheet definition. The core keeps it as JSON. `kadakas-sheet` reads and writes it.
    Worksheet(serde_json::Value),
    HardscapeStyle(Box<HardscapeStyle>),
    LandscapeAreaStyle(Box<LandscapeAreaStyle>),
    /// A symbol, hatch, tile, line type or texture (see `crate::graphics`).
    Graphic(Box<crate::graphics::GraphicResource>),
    /// An irrigation catalog (head models, pipe sizes, pipe materials). The core keeps it as JSON.
    /// `kadakas-irrigation` reads and writes it.
    IrrigationCatalog(serde_json::Value),
    /// A text style, dimension style, tag style or notes database (see `crate::annot::style`).
    Annot(Box<crate::annot::style::AnnotResource>),
    /// A material or a render style (see `crate::present`).
    Render(Box<crate::present::RenderResource>),
    /// A picture file with its thumbnail (see `crate::picture`). Plant styles refer to it.
    Image(Box<crate::picture::ImageResource>),
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
            ResourceBody::Graphic(g) => g.serialize(s),
            ResourceBody::IrrigationCatalog(v) => v.serialize(s),
            ResourceBody::Annot(a) => a.serialize(s),
            ResourceBody::Render(r) => r.serialize(s),
            ResourceBody::Image(i) => tagged(i.as_ref(), "image", s),
            ResourceBody::Unknown(v) => v.serialize(s),
        }
    }
}

fn tagged<T: Serialize, S: serde::Serializer>(
    body: &T,
    name: &str,
    s: S,
) -> Result<S::Ok, S::Error> {
    let mut v = serde_json::to_value(body).map_err(serde::ser::Error::custom)?;
    v["type"] = name.into();
    v.serialize(s)
}

impl<'de> Deserialize<'de> for ResourceBody {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        match v.get("type").and_then(serde_json::Value::as_str) {
            Some("plant_style") => serde_json::from_value::<PlantStyle>(v)
                .map(|s| ResourceBody::PlantStyle(Box::new(s)))
                .map_err(serde::de::Error::custom),
            Some("record_format") => {
                let f: crate::record::RecordFormat =
                    serde_json::from_value(v.clone()).map_err(serde::de::Error::custom)?;
                // A field type from a newer app: keep the whole resource as it is.
                if f.fields
                    .iter()
                    .any(|x| x.ty == crate::record::FieldType::Unknown)
                {
                    Ok(ResourceBody::Unknown(v))
                } else {
                    Ok(ResourceBody::RecordFormat(Box::new(f)))
                }
            }
            Some("data_map") => serde_json::from_value(v)
                .map(|m| ResourceBody::DataMap(Box::new(m)))
                .map_err(serde::de::Error::custom),
            Some("worksheet") => Ok(ResourceBody::Worksheet(v)),
            // A picture this version cannot read is kept as it is and saved again.
            Some("image") => Ok(serde_json::from_value(v.clone())
                .map_or(ResourceBody::Unknown(v), |i| {
                    ResourceBody::Image(Box::new(i))
                })),
            Some("irrigation_catalog") => Ok(ResourceBody::IrrigationCatalog(v)),
            Some("hardscape_style") => serde_json::from_value::<HardscapeStyle>(v)
                .map(|s| ResourceBody::HardscapeStyle(Box::new(s)))
                .map_err(serde::de::Error::custom),
            Some("landscape_area_style") => serde_json::from_value::<LandscapeAreaStyle>(v)
                .map(|s| ResourceBody::LandscapeAreaStyle(Box::new(s)))
                .map_err(serde::de::Error::custom),
            Some(t) if crate::graphics::TYPE_NAMES.contains(&t) => {
                // A hatch, symbol or the like in a shape that this version does not know (from an older or newer app) is kept as it is.
                Ok(
                    serde_json::from_value(v.clone()).map_or(ResourceBody::Unknown(v), |g| {
                        ResourceBody::Graphic(Box::new(g))
                    }),
                )
            }
            Some("material" | "render_style") => {
                // A value this version cannot read: keep the raw JSON and save it again.
                Ok(match crate::present::RenderResource::from_json(&v) {
                    Some(Ok(r)) => ResourceBody::Render(Box::new(r)),
                    _ => ResourceBody::Unknown(v),
                })
            }
            _ => match crate::annot::style::AnnotResource::from_json(&v) {
                // A value from a newer app (a unit this version does not know): keep the raw JSON and save it again.
                Some(r) => Ok(r.map_or_else(
                    |_| ResourceBody::Unknown(v.clone()),
                    |a| ResourceBody::Annot(Box::new(a)),
                )),
                None => Ok(ResourceBody::Unknown(v)),
            },
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
            ResourceBody::Graphic(g) => g.type_name(),
            ResourceBody::IrrigationCatalog(_) => "irrigation_catalog",
            ResourceBody::Annot(a) => a.type_name(),
            ResourceBody::Render(r) => r.type_name(),
            ResourceBody::Image(_) => "image",
            ResourceBody::Unknown(_) => "unknown",
        }
    }

    /// Checks the numbers and rules of the content. Unknown content is not checked.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ResourceBody::PlantStyle(s) => s.validate(),
            ResourceBody::RecordFormat(f) => f.validate(),
            ResourceBody::DataMap(m) => m.validate(),
            ResourceBody::Worksheet(v) if !v.is_object() => {
                Err("a worksheet is a JSON object".into())
            }
            ResourceBody::Worksheet(_) => Ok(()),
            ResourceBody::HardscapeStyle(s) => s.validate(),
            ResourceBody::LandscapeAreaStyle(s) => s.validate(),
            ResourceBody::Graphic(g) => g.validate(),
            ResourceBody::IrrigationCatalog(v) if !v.is_object() => {
                Err("an irrigation catalog is a JSON object".into())
            }
            ResourceBody::IrrigationCatalog(_) => Ok(()),
            ResourceBody::Annot(a) => a.validate(),
            ResourceBody::Render(r) => r.validate(),
            ResourceBody::Image(i) => i.validate(),
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

    /// The picture with this ID.
    pub fn image(&self, id: ResourceId) -> Option<&crate::picture::ImageResource> {
        match &self.get(id)?.body {
            ResourceBody::Image(i) => Some(i.as_ref()),
            _ => None,
        }
    }

    /// The picture resource with this file hash.
    pub fn find_image(&self, sha256: &str) -> Option<ResourceId> {
        self.0.iter().find_map(|r| match &r.body {
            ResourceBody::Image(i) if i.sha256 == sha256 => Some(r.id),
            _ => None,
        })
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
    pub fn landscape_area_styles(
        &self,
    ) -> impl Iterator<Item = (ResourceId, &str, &LandscapeAreaStyle)> {
        self.0.iter().filter_map(|r| match &r.body {
            ResourceBody::LandscapeAreaStyle(s) => Some((r.id, r.name.as_str(), s.as_ref())),
            _ => None,
        })
    }

    /// The ID of the hardscape style with this name (compared without case).
    pub fn find_hardscape_style(&self, name: &str) -> Option<ResourceId> {
        self.hardscape_styles()
            .find(|(_, n, _)| n.eq_ignore_ascii_case(name))
            .map(|(id, _, _)| id)
    }

    /// The ID of the landscape area style with this name (compared without case).
    pub fn find_landscape_area_style(&self, name: &str) -> Option<ResourceId> {
        self.landscape_area_styles()
            .find(|(_, n, _)| n.eq_ignore_ascii_case(name))
            .map(|(id, _, _)| id)
    }

    /// The landscape area styles whose mix uses this plant style.
    pub fn mixes_using(&self, plant: ResourceId) -> Vec<ResourceId> {
        self.landscape_area_styles()
            .filter(|(_, _, s)| s.mix.iter().any(|e| e.plant == plant))
            .map(|(id, _, _)| id)
            .collect()
    }

    /// The ID of the plant style with this name (compared without case).
    pub fn find_plant_style(&self, name: &str) -> Option<ResourceId> {
        self.plant_styles()
            .find(|(_, n, _)| n.eq_ignore_ascii_case(name))
            .map(|(id, _, _)| id)
    }

    /// Checks every resource: valid content, a non-empty name, and a name that no other
    /// resource of the same type has.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(id) = self.find_cycle() {
            let name = self.get(id).map_or_else(String::new, |r| r.name.clone());
            return Err(format!(
                "resource {id} ('{name}') uses itself, directly or through other symbols, tiles or line types"
            ));
        }
        for (i, r) in self.0.iter().enumerate() {
            if r.name.trim().is_empty() {
                return Err(format!("resource {} has no name", r.id));
            }
            r.body
                .validate()
                .map_err(|e| format!("resource {}: {e}", r.id))?;
            if !matches!(r.body, ResourceBody::Unknown(_))
                && self.0[..i].iter().any(|o| {
                    o.body.type_name() == r.body.type_name() && o.name.eq_ignore_ascii_case(&r.name)
                })
            {
                return Err(format!(
                    "two {} resources are named '{}'",
                    r.body.type_name(),
                    r.name
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(id: u64, name: &str) -> Resource {
        Resource {
            id: ResourceId(crate::uid::Uid::from_num(id)),
            name: name.into(),
            body: ResourceBody::plant_style(PlantStyle::default()),
        }
    }

    #[test]
    fn store_basics() {
        let mut r = Resources::default();
        assert!(r.is_empty());
        r.push(res(5, "Oak"));
        r.push(res(6, "Birch"));
        assert_eq!(r.len(), 2);
        assert_eq!(r.get(ResourceId::from_num(6)).unwrap().name, "Birch");
        assert!(r.plant_style(ResourceId::from_num(5)).is_some());
        assert!(r.plant_style(ResourceId::from_num(9)).is_none());
        assert_eq!(r.find_plant_style("oak"), Some(ResourceId::from_num(5)));
        assert_eq!(r.find_plant_style("elm"), None);
        r.get_mut(ResourceId::from_num(5)).unwrap().name = "Elm".into();
        assert_eq!(r.find_plant_style("elm"), Some(ResourceId::from_num(5)));
        assert_eq!(r.plant_styles().count(), 2);
        assert!(r.remove(ResourceId::from_num(5)).is_some());
        assert!(r.remove(ResourceId::from_num(5)).is_none());
        assert_eq!(r.iter().count(), 1);
        assert_eq!(
            ResourceId::from_num(3).to_string(),
            "00000000-0000-0000-0000-000000000003"
        );
    }

    #[test]
    fn a_picture_resource_round_trips_and_an_unreadable_one_is_kept() {
        let pic = crate::picture::tests::sample(7);
        let body = ResourceBody::Image(Box::new(pic.clone()));
        assert_eq!(body.type_name(), "image");
        body.validate().unwrap();
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(serde_json::from_value::<ResourceBody>(json).unwrap(), body);
        // A picture from a newer app (a format this version does not know) stays as it is.
        let newer = serde_json::json!({"type": "image", "format": "webp", "data": "x"});
        let read: ResourceBody = serde_json::from_value(newer.clone()).unwrap();
        assert_eq!(read, ResourceBody::Unknown(newer));
        let mut bad = pic;
        bad.sha256 = "0".repeat(64);
        assert!(ResourceBody::Image(Box::new(bad)).validate().is_err());
        let mut r = Resources::default();
        r.push(Resource {
            id: ResourceId::from_num(1),
            name: "Picture".into(),
            body,
        });
        assert!(r.image(ResourceId::from_num(1)).is_some());
        assert!(r.image(ResourceId::from_num(2)).is_none());
        assert_eq!(
            r.find_image(&crate::picture::tests::sample(7).sha256),
            Some(ResourceId::from_num(1))
        );
        assert_eq!(r.find_image("nope"), None);
    }

    #[test]
    fn validate_rejects_bad_names_and_content() {
        let mut r = Resources::default();
        r.push(res(1, "Oak"));
        assert!(r.validate().is_ok());
        r.push(res(2, "OAK"));
        assert!(r.validate().unwrap_err().contains("named"));
        r.remove(ResourceId::from_num(2));
        r.push(res(3, "  "));
        assert!(r.validate().unwrap_err().contains("no name"));
        r.remove(ResourceId::from_num(3));
        let mut bad = res(4, "Bad");
        let ResourceBody::PlantStyle(s) = &mut bad.body else {
            panic!()
        };
        s.spread = f64::NAN;
        r.push(bad);
        assert!(
            r.validate()
                .unwrap_err()
                .contains(&format!("resource {}", ResourceId::from_num(4)))
        );
    }

    #[test]
    fn unknown_types_are_kept_and_saved_again() {
        let j = serde_json::json!({"id": "00000000-0000-0000-0000-000000000009", "name": "Hatch 1", "body": {"type": "hologram", "angle": 45, "rows": [1, 2]}});
        let r: Resource = serde_json::from_value(j.clone()).unwrap();
        assert!(matches!(r.body, ResourceBody::Unknown(_)));
        assert_eq!(r.body.type_name(), "unknown");
        assert!(r.body.validate().is_ok());
        assert_eq!(serde_json::to_value(&r).unwrap(), j);
        let mut store = Resources::default();
        store.push(r.clone());
        store.push(Resource {
            id: ResourceId::from_num(10),
            name: "Hatch 1".into(),
            body: r.body.clone(),
        });
        assert!(
            store.validate().is_ok(),
            "unknown resources may share a name"
        );
        assert!(store.plant_style(ResourceId::from_num(9)).is_none());
        assert_eq!(store.plant_styles().count(), 0);
        // A body without a type is also kept.
        let r: Resource =
            serde_json::from_value(serde_json::json!({"id": "00000000-0000-0000-0000-000000000001", "name": "x", "body": 5})).unwrap();
        assert_eq!(r.body, ResourceBody::Unknown(serde_json::json!(5)));
        // A plant style with a bad field is an error.
        let bad = serde_json::json!({"id": "00000000-0000-0000-0000-000000000001", "name": "x", "body": {"type": "plant_style", "spread": "wide"}});
        assert!(serde_json::from_value::<Resource>(bad).is_err());
    }

    #[test]
    fn data_resources_round_trip() {
        use crate::record::{FieldDef, FieldType, RecordFormat};
        let fmt = RecordFormat {
            fields: vec![FieldDef::new("Depth", FieldType::Dimension)],
        };
        let r = Resource {
            id: ResourceId::from_num(1),
            name: "Soil".into(),
            body: ResourceBody::RecordFormat(Box::new(fmt)),
        };
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["body"]["type"], "record_format");
        assert_eq!(serde_json::from_value::<Resource>(j).unwrap(), r);
        assert_eq!(r.body.type_name(), "record_format");
        let m = Resource {
            id: ResourceId::from_num(2),
            name: "Map".into(),
            body: ResourceBody::DataMap(Box::new(crate::datamap::DataMap::landscape_template())),
        };
        let j = serde_json::to_value(&m).unwrap();
        assert_eq!(j["body"]["type"], "data_map");
        assert_eq!(serde_json::from_value::<Resource>(j).unwrap(), m);
        assert!(m.body.validate().is_ok());
        let mut store = Resources::default();
        store.push(m);
        assert!(store.data_map(ResourceId::from_num(2)).is_some());
        assert!(store.data_map(ResourceId::from_num(1)).is_none());
        let w = Resource {
            id: ResourceId::from_num(3),
            name: "Sheet".into(),
            body: ResourceBody::Worksheet(serde_json::json!({"type": "worksheet", "cells": {}})),
        };
        let j = serde_json::to_value(&w).unwrap();
        assert_eq!(serde_json::from_value::<Resource>(j).unwrap(), w);
        assert_eq!(w.body.type_name(), "worksheet");
        assert!(w.body.validate().is_ok());
        assert!(
            ResourceBody::Worksheet(serde_json::json!(5))
                .validate()
                .is_err()
        );
        // A field type from the future keeps the resource, unchanged.
        let future = serde_json::json!({"id": "00000000-0000-0000-0000-000000000004", "name": "Later", "body": {"type": "record_format", "fields": [{"name": "When", "type": "hologram"}]}});
        let r: Resource = serde_json::from_value(future.clone()).unwrap();
        assert!(matches!(r.body, ResourceBody::Unknown(_)));
        assert_eq!(serde_json::to_value(&r).unwrap(), future);
        let bad = serde_json::json!({"id": "00000000-0000-0000-0000-000000000001", "name": "x", "body": {"type": "record_format", "fields": 5}});
        assert!(serde_json::from_value::<Resource>(bad).is_err());
        let bad = serde_json::json!({"id": "00000000-0000-0000-0000-000000000001", "name": "x", "body": {"type": "data_map", "rules": 5}});
        assert!(serde_json::from_value::<Resource>(bad).is_err());
        let bad_fmt = ResourceBody::RecordFormat(Box::new(RecordFormat {
            fields: vec![FieldDef::new("", FieldType::Text)],
        }));
        assert!(bad_fmt.validate().is_err());
    }

    #[test]
    fn graphic_resources_round_trip_and_unreadable_ones_are_kept() {
        let j = serde_json::json!({"id": "00000000-0000-0000-0000-000000000003", "name": "Diag", "body": {"type": "hatch", "units": "world", "lines": [{"angle": 45.0, "spacing": 2.0}]}});
        let r: Resource = serde_json::from_value(j).unwrap();
        assert!(matches!(&r.body, ResourceBody::Graphic(_)));
        assert_eq!((r.body.type_name(), r.body.validate()), ("hatch", Ok(())));
        let back: Resource = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        let mut store = Resources::default();
        store.push(r);
        assert!(
            store.hatch(ResourceId::from_num(3)).is_some()
                && store.symbol(ResourceId::from_num(3)).is_none()
        );
        assert!(store.find_graphic("hatch", "diag").is_some());
        // A hatch in a shape that this version cannot read stays as it is.
        let old = serde_json::json!({"id": "00000000-0000-0000-0000-000000000004", "name": "Old", "body": {"type": "hatch", "angle": 45, "rows": [1]}});
        let r: Resource = serde_json::from_value(old.clone()).unwrap();
        assert!(matches!(r.body, ResourceBody::Unknown(_)));
        assert_eq!(serde_json::to_value(&r).unwrap(), old);
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

    #[test]
    fn a_file_with_a_resource_that_uses_itself_is_refused() {
        use crate::graphics::{
            GraphicResource, LineElement, LineTypeDef, PatternUnits, SymbolDef, SymbolItem,
        };
        let sym = |id: u64, line_type: u64| Resource {
            id: ResourceId(crate::uid::Uid::from_num(id)),
            name: format!("S{id}"),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                items: vec![SymbolItem {
                    shape: crate::model::Shape::Circle {
                        center: glam::DVec2::ZERO,
                        radius: 1.0,
                    },
                    attrs: crate::model::Attributes {
                        line_type: Some(ResourceId(crate::uid::Uid::from_num(line_type))),
                        ..Default::default()
                    },
                    class: None,
                }],
                ..Default::default()
            }))),
        };
        // A symbol whose circle has a line type with a mark of the same symbol.
        let lt = Resource {
            id: ResourceId::from_num(2),
            name: "Fence".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::LineType(LineTypeDef {
                units: PatternUnits::Paper,
                elements: vec![
                    LineElement::Dash { length: 1.0 },
                    LineElement::Mark {
                        symbol: ResourceId::from_num(1),
                        scale: 1.0,
                        rotate: true,
                    },
                ],
            }))),
        };
        let mut store = Resources::default();
        store.push(sym(1, 2));
        store.push(lt);
        let e = store.validate().unwrap_err();
        assert!(e.contains("uses itself"), "{e}");
        let mut ok = Resources::default();
        ok.push(sym(1, 99));
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn irrigation_catalog_resource_round_trips_and_validates() {
        let body = ResourceBody::IrrigationCatalog(serde_json::json!({
            "type": "irrigation_catalog",
            "heads": [{"name": "H", "radius": 1000, "flow": 2, "pressure": 100}]
        }));
        assert_eq!(body.type_name(), "irrigation_catalog");
        assert!(body.validate().is_ok());
        let text = serde_json::to_string(&body).unwrap();
        let back: ResourceBody = serde_json::from_str(&text).unwrap();
        assert_eq!(back, body);
        let bad = ResourceBody::IrrigationCatalog(serde_json::json!(5));
        assert!(bad.validate().unwrap_err().contains("JSON object"));
    }
}
