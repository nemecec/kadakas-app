//! Document-level resources: reusable named definitions that objects refer to.
//! Plant styles are the first kind. Symbols, hatches and other styles can add
//! a variant to [`ResourceBody`] later, without changing the store.

use serde::{Deserialize, Serialize};

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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceBody {
    PlantStyle(Box<PlantStyle>),
}

impl ResourceBody {
    /// A plant style resource body.
    pub fn plant_style(style: PlantStyle) -> Self {
        ResourceBody::PlantStyle(Box::new(style))
    }

    /// The type name, as used in JSON.
    pub fn type_name(&self) -> &'static str {
        match self {
            ResourceBody::PlantStyle(_) => "plant_style",
        }
    }

    /// Checks the numbers and rules of the content.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ResourceBody::PlantStyle(s) => s.validate(),
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
        }
    }

    /// All plant styles with their IDs and names.
    pub fn plant_styles(&self) -> impl Iterator<Item = (ResourceId, &str, &PlantStyle)> {
        self.0.iter().map(|r| match &r.body {
            ResourceBody::PlantStyle(s) => (r.id, r.name.as_str(), s.as_ref()),
        })
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
            if self.0[..i].iter().any(|o| o.body.type_name() == r.body.type_name() && o.name.eq_ignore_ascii_case(&r.name)) {
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
        let ResourceBody::PlantStyle(s) = &mut bad.body;
        s.spread = f64::NAN;
        r.push(bad);
        assert!(r.validate().unwrap_err().contains("resource 4"));
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
