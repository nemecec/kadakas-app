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

/// What a tracked DXF or DWG import keeps for a later update (IOP-22, IOP-23).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaseMode {
    /// Keep the summaries of the entities in the document (the default). A drawing with more than
    /// `MAX_BASE_RECS` entities is refused with an error that lists the other two choices: the
    /// import never skips the base without asking.
    #[default]
    #[serde(rename = "auto")]
    Auto,
    /// Keep a link to the original file on the layer source instead of the summaries: the absolute
    /// path, the size and the SHA-256. An update reads the old file from there, checks the hash and
    /// diffs it against the new one.
    #[serde(rename = "link")]
    Link,
    /// Keep no update data. An update of this drawing will need the old file.
    #[serde(rename = "none")]
    Off,
}

impl BaseMode {
    /// The name in the options and in the report.
    pub fn name(self) -> &'static str {
        match self {
            BaseMode::Auto => "auto",
            BaseMode::Link => "link",
            BaseMode::Off => "none",
        }
    }
}

/// The limits of the update data of an import. The defaults are the limits of the document
/// (`kadakas_core::provenance`). A program may lower them (the API session limit
/// `base_records`, tests); they are not options of a client, so a client cannot raise them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseCaps {
    /// Most entities whose summaries are kept.
    pub records: usize,
    /// The size that one part of the summaries aims at, in bytes of JSON.
    pub part_bytes: usize,
    /// Most parts.
    pub parts: u64,
}

impl Default for BaseCaps {
    fn default() -> Self {
        BaseCaps {
            records: kadakas_core::provenance::MAX_BASE_RECS,
            part_bytes: kadakas_core::provenance::BASE_PART_BYTES,
            parts: kadakas_core::provenance::MAX_BASE_PARTS,
        }
    }
}

impl BaseCaps {
    /// The default limits with a lower number of entities (never a higher one).
    pub fn with_records(records: usize) -> BaseCaps {
        BaseCaps {
            records: records.min(kadakas_core::provenance::MAX_BASE_RECS),
            ..BaseCaps::default()
        }
    }
}

/// The identity and the placement that an update gives to the import of a new revision (IOP-23),
/// so that its objects carry the tags of the drawing that is updated and sit where the old ones
/// do. Not an option in JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct Lineage {
    /// The drawing (lineage) of the tags, instead of the one the new file would get.
    pub drawing: String,
    /// The revision of the tags, instead of the one the new file would get.
    pub revision: Option<String>,
    /// The shift in mm that the objects get (the model is the file times the unit minus this),
    /// instead of a shift that is found from the new file.
    pub shift: [f64; 2],
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
    /// DXF: a layout of paper space becomes a sheet layer with its viewports and annotations
    /// (default). False: the entities of paper space are skipped and counted.
    pub paper_space: bool,
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
    /// DXF and DWG with `track_source`: what to keep for a later update (`auto`, `link` or `none`).
    pub base: BaseMode,
    /// The limits of the update data. Not an option in JSON.
    #[serde(skip)]
    pub base_caps: BaseCaps,
    /// An update of a drawing (IOP-23): the identity and the shift of the old import. Not an
    /// option in JSON.
    #[serde(skip)]
    pub lineage: Option<Lineage>,
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
            paper_space: true,
            tolerance: 2.0,
            default_text_height: 250.0,
            source_crs: None,
            target_crs: None,
            class_field: None,
            max_objects: 500_000,
            track_source: true,
            source_name: None,
            max_bytes: DEFAULT_MAX_BYTES,
            base: BaseMode::Auto,
            base_caps: BaseCaps::default(),
            lineage: None,
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
    /// One MESH entity (a subdivision mesh with subdivision level 0) with shared vertices and
    /// triangle faces (MOD-12). A CAD program that reads DXF 2010 or later shows it as a mesh
    /// object.
    Mesh,
}

/// What a DXF export writes of the unit pattern of an object (a hardscape with a joint pattern).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternExport {
    /// Only the outline of the object. This is the default: a pattern can have hundreds of
    /// thousands of units, and the file would be much larger.
    #[default]
    None,
    /// The center lines of the gaps, as LINE entities on the layer `<layer>-GAPS`.
    Gaps,
    /// Every unit, whole or cut, as a closed polyline with the gap taken off, on the layers
    /// `<layer>-UNITS-WHOLE` and `<layer>-UNITS-CUT`. A hole inside a cut unit is a closed
    /// polyline on `<layer>-UNITS-VOID`.
    Units,
    /// Like `Units`, but each whole unit is an INSERT of the block of its unit type (on
    /// `<layer>-UNITS-WHOLE`), and only cut units are polylines. The file is several times
    /// smaller, and CAD programs count the blocks directly.
    Blocks,
}

impl PatternExport {
    pub fn name(self) -> &'static str {
        match self {
            PatternExport::None => "none",
            PatternExport::Gaps => "gaps",
            PatternExport::Units => "units",
            PatternExport::Blocks => "blocks",
        }
    }
}

/// The release of the DXF file that an export writes (IOP-15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DxfVersion {
    /// Release 12 (`AC1009`): 2D plain entities only (LINE, CIRCLE, ARC, POINT, TEXT, POLYLINE).
    /// What it cannot hold is dropped, and the report counts it. Old CAD, CAM and laser programs
    /// read this release. It has no unit, no layouts and no Unicode.
    #[serde(rename = "AC1009", alias = "R12", alias = "12")]
    R12,
    /// AutoCAD 2000 (`AC1015`): the same entities as 2004, with no true colors (the nearest
    /// indexed color is written) and no MESH entity (the polyface mesh is written instead).
    #[serde(rename = "AC1015", alias = "2000")]
    R2000,
    /// AutoCAD 2004 (`AC1018`): the default.
    #[default]
    #[serde(rename = "AC1018", alias = "2004")]
    R2004,
    /// AutoCAD 2013 (`AC1027`). Checked with the ezdxf reader and its audit. Not opened in AutoCAD.
    #[serde(rename = "AC1027", alias = "2013")]
    R2013,
    /// AutoCAD 2018 (`AC1032`). Checked with the ezdxf reader and its audit. Not opened in AutoCAD.
    #[serde(rename = "AC1032", alias = "2018")]
    R2018,
}

impl DxfVersion {
    /// The value of `$ACADVER`.
    pub fn code(self) -> &'static str {
        match self {
            DxfVersion::R12 => "AC1009",
            DxfVersion::R2000 => "AC1015",
            DxfVersion::R2004 => "AC1018",
            DxfVersion::R2013 => "AC1027",
            DxfVersion::R2018 => "AC1032",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DxfVersion::R12 => "12",
            DxfVersion::R2000 => "2000",
            DxfVersion::R2004 => "2004",
            DxfVersion::R2013 => "2013",
            DxfVersion::R2018 => "2018",
        }
    }

    /// True from release 2004 on: group code 420 (true color) exists.
    pub fn has_true_color(self) -> bool {
        !matches!(self, DxfVersion::R12 | DxfVersion::R2000)
    }

    /// True when a MESH entity may be written (`mesh_entity: mesh`). The entity is from release
    /// 2010. The 2004 file keeps it as it always did, and CAD programs of 2010 and newer show
    /// it. Release 2000 gets the polyface mesh instead.
    pub fn allows_mesh_entity(self) -> bool {
        !matches!(self, DxfVersion::R12 | DxfVersion::R2000)
    }
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
    /// DXF: the release of the file: `AC1018` (2004, the default), `AC1015` (2000), `AC1027`
    /// (2013), `AC1032` (2018) or `AC1009` (release 12, 2D plain entities only). The names
    /// `2000`, `2004`, `2013`, `2018` and `R12` work too.
    pub version: DxfVersion,
    /// DXF: sheet layers become layouts of paper space, with their viewports and annotations
    /// (default). False: sheets are left out of the file.
    pub sheets: bool,
    /// DXF: dimensions, callouts and text annotations become native DIMENSION, LEADER and MTEXT
    /// entities where they can (default). False: all annotations are lines and text.
    pub native_annotations: bool,
    /// DXF: also write the surface of site models as a 3D mesh.
    pub site_mesh: SiteMesh,
    /// DXF: how meshes (mesh objects and the site mesh) are written.
    pub mesh_entity: MeshEntity,
    /// DXF: what to write of the unit pattern of hardscapes: `none` (default), `gaps`, `units` or
    /// `blocks` (whole units as INSERTs of one block per unit type).
    /// A pattern with more units than the export limit is an error that names the object.
    pub patterns: PatternExport,
    /// The most units (or gap lines) that the pattern export of one file may write in all objects.
    /// A program sets it from its limits; it is not an option in JSON. `None`: the default bound.
    #[serde(skip)]
    pub pattern_budget: Option<usize>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            origin: ExportOrigin::File,
            layer_mode: LayerMode::Classes,
            unit: Unit::Mm,
            version: DxfVersion::R2004,
            sheets: true,
            visible_only: false,
            tolerance: 1.0,
            native_annotations: true,
            site_mesh: SiteMesh::None,
            mesh_entity: MeshEntity::Polyface,
            patterns: PatternExport::None,
            pattern_budget: None,
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
    fn the_dxf_release_has_names_and_aliases() {
        let v = |x: serde_json::Value| ExportOptions::from_json(&json!({"version": x}));
        assert_eq!(ExportOptions::default().version, DxfVersion::R2004);
        assert_eq!(v(json!("AC1015")).unwrap().version, DxfVersion::R2000);
        assert_eq!(v(json!("2000")).unwrap().version, DxfVersion::R2000);
        assert_eq!(v(json!("AC1018")).unwrap().version, DxfVersion::R2004);
        assert_eq!(v(json!("2013")).unwrap().version, DxfVersion::R2013);
        assert_eq!(v(json!("AC1032")).unwrap().version, DxfVersion::R2018);
        assert_eq!(v(json!("R12")).unwrap().version, DxfVersion::R12);
        assert_eq!(v(json!("AC1009")).unwrap().version, DxfVersion::R12);
        assert!(v(json!("AC9999")).is_err());
        for x in [
            DxfVersion::R12,
            DxfVersion::R2000,
            DxfVersion::R2004,
            DxfVersion::R2013,
            DxfVersion::R2018,
        ] {
            assert_eq!(v(json!(x.code())).unwrap().version, x);
            assert!(!x.name().is_empty());
        }
        assert!(!DxfVersion::R2000.has_true_color() && DxfVersion::R2004.has_true_color());
        assert!(!DxfVersion::R2000.allows_mesh_entity() && DxfVersion::R2004.allows_mesh_entity());
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
