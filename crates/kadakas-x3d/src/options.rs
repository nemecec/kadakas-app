//! Options of the 3D exports.

use serde::{Deserialize, Serialize};

use kadakas_core::model::ObjectId;

/// How the objects become nodes (glTF) or groups (OBJ).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    /// One node for each object.
    #[default]
    Object,
    /// One node for each layer, with the meshes of its objects joined.
    Layer,
    /// One node for each class, with the meshes of its objects joined.
    Class,
}

/// The unit of the coordinates in the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    /// Metres. This is the unit of glTF.
    #[default]
    #[serde(alias = "m")]
    Metres,
    /// The unit of the model.
    #[serde(alias = "mm")]
    Millimetres,
}

impl Units {
    /// The factor from model units (mm) to file units.
    pub fn factor(self) -> f64 {
        match self {
            Units::Metres => 0.001,
            Units::Millimetres => 1.0,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Units::Metres => "m",
            Units::Millimetres => "mm",
        }
    }
}

/// How detailed the plants of an export are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlantDetail {
    /// Full meshes. Plants of one style share a few meshes in the file (instancing), so a large
    /// park stays small. This is the default for exports.
    #[default]
    Fine,
    /// Coarser meshes.
    Coarse,
    /// Crossed quads with the outline of the plant.
    Billboard,
    /// As the 3D view: the detail follows the number of plants (billboards above 20 000).
    Auto,
}

/// What an export does. The defaults follow the glTF rules: metres, Y up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportOptions {
    pub grouping: Grouping,
    pub units: Units,
    /// Y up (the glTF convention). `false` keeps the Z up axis of the model.
    pub y_up: bool,
    /// Write where the model sits on the earth: the CRS, the map position of the model origin and
    /// the scene origin. The positions in the file stay local, near (0, 0, 0).
    pub georef: bool,
    /// Give objects with the same shape one mesh and many nodes (glTF only).
    pub instancing: bool,
    /// Instances may differ by up to this distance in mm.
    pub instance_tolerance: f64,
    /// Write the data of the objects (records, settings) under `extras`.
    pub data: bool,
    /// Export only these objects. `None`: all objects that have 3D geometry.
    pub objects: Option<Vec<ObjectId>>,
    /// Draw 3D lines (polylines, outlines of lines) as line primitives.
    pub lines: bool,
    /// The detail of plants. Exports use full meshes by default, because the 3D view turns plants
    /// into billboards when there are very many (`auto`).
    pub plant_detail: PlantDetail,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            grouping: Grouping::Object,
            units: Units::Metres,
            y_up: true,
            georef: true,
            instancing: true,
            instance_tolerance: 1.0,
            data: true,
            objects: None,
            lines: true,
            plant_detail: PlantDetail::Fine,
        }
    }
}

impl ExportOptions {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.instance_tolerance.is_finite()
            && (0.001..=1000.0).contains(&self.instance_tolerance))
        {
            return Err("instance_tolerance must be from 0.001 to 1000 mm".into());
        }
        Ok(())
    }
}
