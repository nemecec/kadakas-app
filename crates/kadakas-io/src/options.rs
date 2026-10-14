//! Options for import and export. Both read from JSON, and every field has a default.

use serde::{Deserialize, Serialize};

/// A length unit of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Mm,
    Cm,
    M,
    Km,
    Inch,
    Foot,
    Yard,
    Mile,
}

impl Unit {
    pub fn mm_per_unit(self) -> f64 {
        match self {
            Unit::Mm => 1.0,
            Unit::Cm => 10.0,
            Unit::M => 1000.0,
            Unit::Km => 1.0e6,
            Unit::Inch => 25.4,
            Unit::Foot => 304.8,
            Unit::Yard => 914.4,
            Unit::Mile => 1_609_344.0,
        }
    }

    /// The unit for a DXF `$INSUNITS` code. Unitless (0) and unknown codes give `None`.
    pub fn from_insunits(code: i64) -> Option<Unit> {
        Some(match code {
            1 => Unit::Inch,
            2 => Unit::Foot,
            3 => Unit::Mile,
            4 => Unit::Mm,
            5 => Unit::Cm,
            6 => Unit::M,
            7 => Unit::Km,
            10 => Unit::Yard,
            _ => return None,
        })
    }

    /// The `$INSUNITS` code of the unit.
    pub fn insunits(self) -> i64 {
        match self {
            Unit::Inch => 1,
            Unit::Foot => 2,
            Unit::Mile => 3,
            Unit::Mm => 4,
            Unit::Cm => 5,
            Unit::M => 6,
            Unit::Km => 7,
            Unit::Yard => 10,
        }
    }
}

/// How DXF layers map to the document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerMode {
    /// DXF layers become classes on one design layer (the default).
    #[default]
    Classes,
    /// DXF layers become design layers.
    Layers,
}

/// Where the imported geometry sits in the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginMode {
    /// Center the drawing on (0, 0). The shift is saved as the origin of the document georeference.
    /// This is the default, because real-world coordinates are too large for model millimeters.
    #[default]
    Center,
    /// Keep the coordinates of the file. Objects beyond 1e9 mm are skipped.
    File,
}

/// Which DXF points to import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointMode {
    /// Every point becomes a survey point (`locus3d`). This is the default.
    #[default]
    All,
    /// Only points with a Z value other than 0.
    WithZ,
}

/// How DXF 3DFACE entities are imported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceMode {
    /// Each face is a closed 3D polygon (`polyline3d`), which can be source data of a site model.
    /// This is the default.
    #[default]
    Polygons,
    /// All faces of a DXF layer are merged into one mesh object (`mesh3d`).
    Mesh,
}

/// The default of `ImportOptions::max_bytes`.
pub const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportOptions {
    /// DXF: DXF layers become classes or design layers.
    pub layer_mode: LayerMode,
    /// The design layer that receives the objects (in `classes` mode, and for GIS files).
    /// Created if it does not exist. Default: the active layer.
    pub layer: Option<String>,
    /// DXF: unit of the drawing. Default: `$INSUNITS` of the file, else millimeters.
    pub unit: Option<Unit>,
    /// DXF: explode blocks (INSERT) into objects. Blocks that are not exploded are skipped.
    pub explode_blocks: bool,
    /// Where the geometry sits: `center` (default) or `file`. A document that has a georeference keeps its origin.
    pub origin: OriginMode,
    /// DXF: most objects that exploded blocks may make. The import fails above it.
    pub max_exploded: usize,
    pub points: PointMode,
    /// DXF: 3DFACE entities become 3D polygons (default) or one mesh per DXF layer.
    pub faces: FaceMode,
    /// DXF: import HATCH boundaries as closed polylines.
    pub hatches: bool,
    /// Largest distance of an approximated curve from the true curve, in mm.
    pub tolerance: f64,
    /// DXF: text height in mm for text that has none.
    pub default_text_height: f64,
    /// GIS: CRS of the file when the file does not say (`EPSG:4326`, a PROJ string or WKT).
    pub source_crs: Option<String>,
    /// GIS: CRS of a new georeference. Default: UTM for lon/lat data, else the file CRS.
    pub target_crs: Option<String>,
    /// GIS: attribute field whose value names the class of each object. Default: the file name.
    pub class_field: Option<String>,
    /// Import stops after this many objects.
    pub max_objects: usize,
    /// DXF and DWG: keep the source of each object (the DXF handle, the file and a hash of the
    /// entity) in its data, and put the objects on layers of their own, apart from the design
    /// work (IOP-22). With `layer` set, the objects go to that layer, but keep the tags.
    pub track_source: bool,
    /// DXF and DWG: the file name that the tags record. Default: the name of the file.
    pub source_name: Option<String>,
    /// DXF and DWG: the largest file to read, in bytes. The size is checked before the file is
    /// read. Default 512 MiB.
    pub max_bytes: u64,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            layer_mode: LayerMode::Classes,
            layer: None,
            unit: None,
            explode_blocks: true,
            origin: OriginMode::Center,
            max_exploded: 100_000,
            points: PointMode::All,
            faces: FaceMode::Polygons,
            hatches: true,
            tolerance: 2.0,
            default_text_height: 250.0,
            source_crs: None,
            target_crs: None,
            class_field: None,
            max_objects: 500_000,
            track_source: true,
            source_name: None,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl ImportOptions {
    /// Reads options from JSON. Null means all defaults.
    pub fn from_json(v: &serde_json::Value) -> Result<ImportOptions, crate::IoError> {
        if v.is_null() {
            return Ok(ImportOptions::default());
        }
        let o: ImportOptions = serde_json::from_value(v.clone())
            .map_err(|e| crate::IoError::Option(format!("bad import options: {e}")))?;
        if !(o.tolerance.is_finite()
            && o.tolerance > 0.0
            && o.default_text_height.is_finite()
            && o.default_text_height > 0.0)
        {
            return Err(crate::IoError::Option(
                "tolerance and default_text_height must be positive numbers".into(),
            ));
        }
        Ok(o)
    }
}

/// Which coordinates an export writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportOrigin {
    /// The map coordinates of the document georeference: the origin shift of an import is added back, so a survey
    /// keeps its real-world numbers. Without a georeference this is the same as `document`. This is the default.
    #[default]
    File,
    /// The coordinates of the model, relative to the document origin.
    Document,
}

/// Which surface of a site model a DXF export writes as a 3D mesh (in addition to the contours).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteMesh {
    /// No mesh: contours and source points only. This is the default.
    #[default]
    None,
    Existing,
    Proposed,
}

/// How a DXF export writes a triangle mesh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshEntity {
    /// One POLYLINE (polyface mesh) with shared vertices. This is the default.
    #[default]
    Polyface,
    /// One 3DFACE entity per triangle.
    Face3d,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// `file` (default): real-world coordinates. `document`: model coordinates.
    pub origin: ExportOrigin,
    /// DXF: layer names come from classes (default) or from design layers.
    pub layer_mode: LayerMode,
    /// Unit of the written coordinates (DXF). Default: millimeters.
    pub unit: Unit,
    /// Skip objects on hidden layers and classes.
    pub visible_only: bool,
    /// Largest distance of an approximated curve from the true curve, in mm.
    pub tolerance: f64,
    /// DXF: dimensions, callouts and text annotations become native DIMENSION, LEADER and MTEXT
    /// entities where they can (default). False: all annotations are lines and text.
    pub native_annotations: bool,
    /// DXF: also write the surface of site models as a 3D mesh.
    pub site_mesh: SiteMesh,
    /// DXF: how meshes (mesh objects and the site mesh) are written.
    pub mesh_entity: MeshEntity,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            origin: ExportOrigin::File,
            layer_mode: LayerMode::Classes,
            unit: Unit::Mm,
            visible_only: false,
            tolerance: 1.0,
            native_annotations: true,
            site_mesh: SiteMesh::None,
            mesh_entity: MeshEntity::Polyface,
        }
    }
}

impl ExportOptions {
    pub fn from_json(v: &serde_json::Value) -> Result<ExportOptions, crate::IoError> {
        if v.is_null() {
            return Ok(ExportOptions::default());
        }
        let o: ExportOptions = serde_json::from_value(v.clone())
            .map_err(|e| crate::IoError::Option(format!("bad export options: {e}")))?;
        if !(o.tolerance.is_finite() && o.tolerance > 0.0) {
            return Err(crate::IoError::Option(
                "tolerance must be a positive number".into(),
            ));
        }
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_and_json() {
        assert_eq!(
            ImportOptions::from_json(&json!(null)).unwrap(),
            ImportOptions::default()
        );
        let o = ImportOptions::from_json(
            &json!({"layer_mode": "layers", "unit": "foot", "points": "with_z"}),
        )
        .unwrap();
        assert_eq!(o.layer_mode, LayerMode::Layers);
        assert_eq!(o.unit, Some(Unit::Foot));
        assert_eq!(o.points, PointMode::WithZ);
        assert!(o.explode_blocks);
        assert!(ImportOptions::from_json(&json!({"unit": "parsec"})).is_err());
        assert!(ImportOptions::from_json(&json!({"tolerance": 0})).is_err());
        assert_eq!(
            ExportOptions::from_json(&json!(null)).unwrap(),
            ExportOptions::default()
        );
        let e = ExportOptions::from_json(&json!({"unit": "m", "visible_only": true})).unwrap();
        assert_eq!(e.unit, Unit::M);
        assert!(e.visible_only);
        assert!(ExportOptions::from_json(&json!({"tolerance": -1})).is_err());
        assert!(ExportOptions::from_json(&json!({"layer_mode": 3})).is_err());
    }

    #[test]
    fn unit_codes() {
        for u in [
            Unit::Mm,
            Unit::Cm,
            Unit::M,
            Unit::Km,
            Unit::Inch,
            Unit::Foot,
            Unit::Yard,
            Unit::Mile,
        ] {
            assert_eq!(Unit::from_insunits(u.insunits()), Some(u));
            assert!(u.mm_per_unit() >= 1.0);
        }
        assert_eq!(Unit::from_insunits(0), None);
        assert_eq!(Unit::from_insunits(99), None);
    }
}
