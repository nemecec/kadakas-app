//! Parameters of `resource.create` and `resource.update`: the graphic resources.
//!
//! `resource.create` is a tagged union on `type`: each of the five graphic resource types has its
//! own fields, and a field of another type is refused. `resource.update` changes the resource it
//! names, so the type comes from that resource and the fields are the union of all types.

#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use serde_json::Value;

use super::*;

param_enum! {
    /// What the sizes of a pattern or symbol are measured in.
    ResourceUnits {
        Paper = "paper",
        World = "world",
    }
}

param_enum! {
    /// The type of a graphic resource.
    GraphicType {
        Symbol = "symbol",
        Hatch = "hatch",
        Tile = "tile",
        LineType = "line_type",
        Texture = "texture",
    }
}

/// One family of parallel lines of a hatch.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HatchLineParams {
    /// Direction of the lines, in degrees counterclockwise from the X axis (default 0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// Distance between neighboring lines, perpendicular to them
    pub spacing: Num,
    /// Each line moves along its direction by this much more than the one before (staggers dashes)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Num>,
    /// A point that one line of the family passes through
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    /// Lengths of dash, gap, dash, gap, ... Empty: a solid line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<Num>>,
    /// Color of these lines (default: the pen color of the object)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Line weight in mm on paper (default: the weight of the object)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<Num>,
}

/// One part of a line type.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineElementParams {
    /// A drawn part
    Dash(LineLength),
    /// A part without a line
    Gap(LineLength),
    /// A symbol placed on the line, with no length of its own
    Mark(LineMark),
}

/// The length of a dash or a gap.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineLength {
    /// Length in the units of the line type
    pub length: Num,
}

/// A symbol placed on a line.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineMark {
    /// The symbol: ID or name
    pub symbol: ResourceRef,
    /// Scale of the symbol (default 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
    /// Turn the symbol with the direction of the line (default true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<bool>,
}

/// A symbol: 2D and 3D components that an instance places.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolResource {
    /// The name of the resource
    pub name: String,
    /// paper: the components are in paper mm; world (default): in model mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<ResourceUnits>,
    /// The class that a new instance gets when the caller gives none
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_class: Option<ClassRef>,
    /// A new instance turns toward a second point that the caller gives (rotate_to)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate_on_insert: Option<bool>,
    /// [{shape, attrs?, class?}]: 2D components in definition coordinates
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Obj>>,
    /// [{kind: extrusion | cylinder, ...}]: 3D components
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solids: Option<Vec<Obj>>,
    /// The insertion point, in definition coordinates
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion: Option<Point>,
}

/// A hatch: families of parallel lines.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HatchResource {
    /// The name of the resource
    pub name: String,
    /// paper (default): sizes in paper mm; world: in model mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<ResourceUnits>,
    /// The families of lines
    pub lines: Vec<HatchLineParams>,
}

/// A tile: a symbol repeated on a grid.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TileResource {
    /// The name of the resource
    pub name: String,
    /// paper (default): sizes in paper mm; world: in model mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<ResourceUnits>,
    /// The symbol that is repeated: ID or name
    pub symbol: ResourceRef,
    /// Width of one cell of the grid
    pub width: Num,
    /// Height of one cell of the grid
    pub height: Num,
    /// Every second row moves along the row by this much (a brick pattern)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_shift: Option<Num>,
    /// Turns the whole grid, in degrees
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// Scale of the symbol in each cell (default 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
}

/// A line type: dashes, gaps and marks that repeat along a line.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineTypeResource {
    /// The name of the resource
    pub name: String,
    /// paper (default): sizes in paper mm; world: in model mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<ResourceUnits>,
    /// The parts that repeat along the line
    pub elements: Vec<LineElementParams>,
}

/// A texture for 3D views.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureResource {
    /// The name of the resource
    pub name: String,
    /// The base color
    pub color: Color,
    /// Size of one repeat of the texture, [x, y] in mm (default 1000 by 1000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Point>,
    /// 0 to 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflectivity: Option<Num>,
    /// 0 to 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparency: Option<Num>,
    /// A name or path of an image, kept as text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// Parameters of `resource.create`: a graphic resource. `type` picks the fields.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceCreateParams {
    Symbol(SymbolResource),
    Hatch(HatchResource),
    Tile(TileResource),
    LineType(LineTypeResource),
    Texture(TextureResource),
}

/// The values of `type`.
const CREATE_TYPES: &[&str] = &["symbol", "hatch", "tile", "line_type", "texture"];

/// Reads the set of parameters that `type` names. The reading is by hand (not the derive of an
/// internally tagged enum) so that an error keeps the way to the parameter.
impl<'de> Deserialize<'de> for ResourceCreateParams {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut map = Obj::deserialize(d)?;
        let ty = match map.remove("type") {
            Some(Value::String(t)) => t,
            Some(_) => {
                return Err(D::Error::invalid_type(
                    serde::de::Unexpected::Other("a value that is not text"),
                    &"a string",
                ));
            }
            None => return Err(D::Error::missing_field("type")),
        };
        fn set<T: serde::de::DeserializeOwned, E: serde::de::Error>(map: Obj) -> Result<T, E> {
            serde_path_to_error::deserialize(Value::Object(map))
                .map_err(|e| E::custom(failure::nested_error(&e)))
        }
        // The case does not matter, as for every enum of the parameters.
        match ty.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "symbol" => set(map).map(Self::Symbol),
            "hatch" => set(map).map(Self::Hatch),
            "tile" => set(map).map(Self::Tile),
            "line_type" => set(map).map(Self::LineType),
            "texture" => set(map).map(Self::Texture),
            other => Err(D::Error::unknown_variant(other, CREATE_TYPES)),
        }
    }
}

/// Parameters of `resource.update`: the fields that change. The type of the resource decides
/// which of them apply (the same fields as `resource.create`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceUpdateParams {
    /// The resource: ID or name
    pub id: ResourceRef,
    /// Renames
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The type of the resource (it is known from the resource; give it only to be sure)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<GraphicType>,
    /// symbol, hatch, tile, line_type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<ResourceUnits>,
    /// symbol: the class that a new instance gets
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_class: Option<ClassRef>,
    /// symbol
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate_on_insert: Option<bool>,
    /// symbol: [{shape, attrs?, class?}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Obj>>,
    /// symbol: [{kind: extrusion | cylinder, ...}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solids: Option<Vec<Obj>>,
    /// symbol: the insertion point
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion: Option<Point>,
    /// hatch: the families of lines
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<HatchLineParams>>,
    /// tile: the symbol that is repeated
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<ResourceRef>,
    /// tile: width of a cell
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    /// tile: height of a cell
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    /// tile
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_shift: Option<Num>,
    /// tile: degrees
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// tile: scale of the symbol
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
    /// line_type: the parts that repeat
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<Vec<LineElementParams>>,
    /// texture: the base color
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// texture: size of one repeat, [x, y] in mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Point>,
    /// texture
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflectivity: Option<Num>,
    /// texture
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparency: Option<Num>,
    /// texture: a name or path of an image
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

entries! {
    "resource.create" => ResourceCreateParams,
    "resource.update" => ResourceUpdateParams,
}
