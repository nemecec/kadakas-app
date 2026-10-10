//! The manifest of a plug-in. The module returns it from `describe`.

use serde::{Deserialize, Serialize};

use crate::ABI_VERSION;
use crate::param::{ParamSchema, resolve};

/// Most parameters in one plug-in object.
pub const MAX_PARAMS: usize = 64;

/// What a plug-in provides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A parametric object. The host calls `reset` and stores the shapes in the document.
    Object,
    /// A drawing tool. The kind is reserved: ABI version 1 has no tool messages yet, and a host
    /// lists the kind but does not run it.
    Tool,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Object => "object",
            Kind::Tool => "tool",
        }
    }
}

/// What a plug-in may ask the host for. ABI version 1 has no calls from the module to the host, so
/// a host grants none of these, and it refuses a module that asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read the open document.
    ReadDocument,
    /// Read or write files.
    Filesystem,
    /// Use the network.
    Network,
    /// Read the clock.
    Clock,
}

impl Permission {
    pub fn name(self) -> &'static str {
        match self {
            Permission::ReadDocument => "read_document",
            Permission::Filesystem => "filesystem",
            Permission::Network => "network",
            Permission::Clock => "clock",
        }
    }
}

/// What kind of geometry places the object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeType {
    /// One point, with a rotation.
    Point,
    /// A straight line from a start to an end point.
    Linear,
    /// A rectangle (two corners, parallel to the axes).
    Rectangle,
    /// A 2D path of points, open or closed.
    Path,
}

impl ShapeType {
    pub fn name(self) -> &'static str {
        match self {
            ShapeType::Point => "point",
            ShapeType::Linear => "linear",
            ShapeType::Rectangle => "rectangle",
            ShapeType::Path => "path",
        }
    }
}

/// The object part of a manifest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectSpec {
    pub shape_type: ShapeType,
    #[serde(default)]
    pub params: Vec<ParamSchema>,
}

/// The manifest of a plug-in module.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// A stable id such as `org.example.planter`: lower case letters, digits, `.`, `-` and `_`.
    pub id: String,
    /// The name that users see.
    pub name: String,
    /// The version of the plug-in, such as `1.2.0`.
    pub version: String,
    /// The ABI version the module was built for ([`ABI_VERSION`]).
    pub abi_version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// What the plug-in provides. At least one entry.
    pub kinds: Vec<Kind>,
    /// The object part. Needed when `kinds` has `object`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectSpec>,
    /// What the plug-in asks for. Leave it empty if you can.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<Permission>,
}

impl Manifest {
    /// A manifest for an object plug-in at the current ABI version.
    pub fn object(id: &str, name: &str, version: &str, spec: ObjectSpec) -> Manifest {
        Manifest {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            abi_version: ABI_VERSION,
            description: String::new(),
            kinds: vec![Kind::Object],
            object: Some(spec),
            permissions: vec![],
        }
    }

    /// Reads and checks a manifest from the JSON bytes of `describe`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Manifest, String> {
        let m: Manifest =
            serde_json::from_slice(bytes).map_err(|e| format!("the manifest is not valid: {e}"))?;
        m.validate()?;
        Ok(m)
    }

    /// True if the plug-in provides this kind.
    pub fn provides(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }

    /// Checks the manifest.
    pub fn validate(&self) -> Result<(), String> {
        if self.abi_version != ABI_VERSION {
            return Err(format!(
                "the module uses ABI version {}, but this host runs version {ABI_VERSION}",
                self.abi_version
            ));
        }
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self.id.starts_with(|c: char| c.is_ascii_lowercase())
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_".contains(c));
        if !id_ok {
            return Err(format!(
                "the id '{}' must have 1 to 64 characters: lower case letters, digits, '.', '-' and '_', starting with a letter",
                self.id
            ));
        }
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
            return Err("the name must have 1 to 100 characters and no control characters".into());
        }
        let version_ok = !self.version.is_empty()
            && self.version.len() <= 32
            && self.version.starts_with(|c: char| c.is_ascii_digit())
            && self
                .version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c));
        if !version_ok {
            return Err(format!(
                "the version '{}' must start with a digit and use only letters, digits, '.', '+' and '-'",
                self.version
            ));
        }
        if self.description.chars().count() > 1000
            || self
                .description
                .chars()
                .any(|c| c.is_control() && c != '\n')
        {
            return Err(
                "the description has at most 1000 characters and no control characters".into(),
            );
        }
        if self.kinds.is_empty() {
            return Err("the plug-in must provide at least one kind".into());
        }
        for (i, k) in self.kinds.iter().enumerate() {
            if self.kinds[..i].contains(k) {
                return Err(format!("the kind '{}' appears twice", k.name()));
            }
        }
        for (i, p) in self.permissions.iter().enumerate() {
            if self.permissions[..i].contains(p) {
                return Err(format!("the permission '{}' appears twice", p.name()));
            }
        }
        match (&self.object, self.provides(Kind::Object)) {
            (None, true) => return Err("the kind 'object' needs an 'object' part".into()),
            (Some(_), false) => return Err("an 'object' part needs the kind 'object'".into()),
            (Some(o), true) => {
                if o.params.len() > MAX_PARAMS {
                    return Err(format!("an object has at most {MAX_PARAMS} parameters"));
                }
                let mut seen = std::collections::HashSet::new();
                for p in &o.params {
                    p.validate()?;
                    if !seen.insert(&p.name) {
                        return Err(format!("parameter '{}' appears twice", p.name));
                    }
                }
            }
            (None, false) => {}
        }
        Ok(())
    }

    /// The parameter values, with defaults, checked against the schema of the object.
    pub fn resolve_params(
        &self,
        given: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        let spec = self
            .object
            .as_ref()
            .ok_or("this plug-in provides no object")?;
        resolve(&spec.params, given)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param::ParamType;
    use serde_json::json;

    fn good() -> Manifest {
        Manifest::object(
            "org.example.box",
            "Box",
            "1.0.0",
            ObjectSpec {
                shape_type: ShapeType::Rectangle,
                params: vec![ParamSchema::new("w", ParamType::Length)],
            },
        )
    }

    #[test]
    fn a_good_manifest_round_trips() {
        let m = good();
        m.validate().unwrap();
        let bytes = serde_json::to_vec(&m).unwrap();
        assert_eq!(Manifest::from_bytes(&bytes).unwrap(), m);
        assert!(m.provides(Kind::Object) && !m.provides(Kind::Tool));
        assert_eq!(Kind::Tool.name(), "tool");
        assert_eq!(ShapeType::Linear.name(), "linear");
        assert_eq!(ShapeType::Point.name(), "point");
        assert_eq!(ShapeType::Rectangle.name(), "rectangle");
        assert_eq!(ShapeType::Path.name(), "path");
        for p in [
            Permission::ReadDocument,
            Permission::Filesystem,
            Permission::Network,
            Permission::Clock,
        ] {
            assert_eq!(json!(p), json!(p.name()));
        }
        assert_eq!(Kind::Object.name(), "object");
    }

    #[test]
    fn bad_manifests_are_refused() {
        let check = |f: &dyn Fn(&mut Manifest), what: &str| {
            let mut m = good();
            f(&mut m);
            let e = m.validate().unwrap_err();
            assert!(e.contains(what), "{e}");
        };
        check(&|m| m.abi_version = 2, "ABI version 2");
        check(&|m| m.id = "Bad Id".into(), "the id");
        check(&|m| m.id = String::new(), "the id");
        check(&|m| m.id = "1abc".into(), "the id");
        check(&|m| m.name = "  ".into(), "the name");
        check(&|m| m.name = "a\u{7}b".into(), "the name");
        check(&|m| m.version = "v1".into(), "the version");
        check(&|m| m.version = String::new(), "the version");
        check(&|m| m.description = "x".repeat(1001), "description");
        check(&|m| m.description = "a\u{7}".into(), "description");
        check(&|m| m.kinds = vec![], "at least one kind");
        check(
            &|m| m.kinds = vec![Kind::Object, Kind::Object],
            "appears twice",
        );
        check(
            &|m| m.permissions = vec![Permission::Network, Permission::Network],
            "permission 'network' appears twice",
        );
        check(&|m| m.object = None, "needs an 'object' part");
        check(&|m| m.kinds = vec![Kind::Tool], "needs the kind 'object'");
        check(
            &|m| {
                m.object.as_mut().unwrap().params = (0..65)
                    .map(|i| ParamSchema::new(&format!("p{i}"), ParamType::Bool))
                    .collect()
            },
            "at most 64",
        );
        check(
            &|m| {
                m.object.as_mut().unwrap().params = vec![
                    ParamSchema::new("a", ParamType::Bool),
                    ParamSchema::new("a", ParamType::Bool),
                ]
            },
            "'a' appears twice",
        );
        check(
            &|m| m.object.as_mut().unwrap().params = vec![ParamSchema::new("1", ParamType::Bool)],
            "must start",
        );
        assert!(
            Manifest::from_bytes(b"{")
                .unwrap_err()
                .contains("not valid")
        );
    }

    #[test]
    fn a_tool_only_plugin_is_valid_but_has_no_object() {
        let mut m = good();
        m.kinds = vec![Kind::Tool];
        m.object = None;
        m.validate().unwrap();
        assert!(m.resolve_params(&serde_json::Map::new()).is_err());
    }

    #[test]
    fn params_resolve_against_the_object_schema() {
        let m = good();
        let out = m.resolve_params(&serde_json::Map::new()).unwrap();
        assert_eq!(out["w"], json!(0.0));
    }
}
