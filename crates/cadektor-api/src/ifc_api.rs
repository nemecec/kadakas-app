//! `export.file` and `import.file` with the format `ifc`. Native only: they read and write files.

use std::path::{Path, PathBuf};

use cadektor_ifc::{IfcError, IfcImportError, IfcImportOptions, IfcOptions};
use serde_json::{Value, json};

use crate::{RpcError, Session};

/// True if the request asks for IFC: the format is `ifc`, or there is no format and the file ends in `.ifc`.
pub(crate) fn is_ifc(format: Option<&str>, path: &Path) -> bool {
    match format {
        Some(f) => f.trim().trim_start_matches('.').eq_ignore_ascii_case("ifc"),
        None => path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ifc")),
    }
}

/// Options and mappings of the IFC export, for `describe`.
pub(crate) fn describe() -> Value {
    json!({
        "format": "ifc: export IFC4 (IFC4 ADD2 TC1, STEP physical file). import.file reads IFC2x3, IFC4 and IFC4x3 (see import below).",
        "options": {
            "version": "IFC4 (the only value for now)",
            "classes_as_layers": "true (default): one IfcPresentationLayerAssignment per class",
            "include_2d": "false (default). true writes 2D objects, text and survey points as IfcAnnotation",
            "layers_as_storeys": "false (default): all elements belong to the IfcSite. true adds an IfcBuilding with one IfcBuildingStorey per design layer",
            "massing_as_building": "false (default): a massing model is an IfcBuildingElementProxy. true: an IfcBuilding in the IfcSite",
            "visible_only": "false (default)",
            "data_map": "name of a data map (Data Manager) for IFC classes and property sets. Default: the first data map of the document, else the built-in landscape mapping",
            "area_thickness": "mm, thickness of the solid of a landscape area. Default 100",
            "time_stamp": "YYYY-MM-DDThh:mm:ss for the file header. Default: the project date, so the same document gives the same bytes"
        },
        "mapping": {
            "site_model": "IfcGeographicElement TERRAIN, IfcTriangulatedFaceSet of the proposed surface",
            "hardscape": "IfcSlab with the predefined type BASESLAB. IfcExtrudedAreaSolid when flat, IfcTriangulatedFaceSet when sloped or draped. Style components become an IfcMaterialLayerSet. Pset_SlabCommon and Qto_SlabBaseQuantities",
            "landscape_area": "IfcGeographicElement (USERDEFINED, ObjectType VEGETATION: IFC4 has no VEGETATION type), thin extrusion with holes",
            "plant, existing_tree": "IfcGeographicElement (USERDEFINED, ObjectType VEGETATION), trunk and canopy cylinders plus an IfcBoundingBox",
            "massing": "IfcBuildingElementProxy (NOTDEFINED) with an extruded solid (flat roof) or a closed IfcTriangulatedFaceSet (gable, hip). Pset_BuildingElementProxyCommon, Cadektor_Massing, Cadektor_MassingQuantities (Height, EavesHeight, FootprintArea, GrossFloorArea, GrossVolume). With massing_as_building: IfcBuilding, Pset_BuildingCommon and Qto_BuildingBaseQuantities",
            "other": "2D objects and map underlays are skipped and listed in the report"
        },
        "import": import_describe(),
        "georeference": "IfcMapConversion and IfcProjectedCRS (EPSG name) from the document georeference, and RefLatitude/RefLongitude of the IfcSite when the CRS is known. Needs a CRS in the georeference.",
        "global_ids": "GlobalIds come from the project GUID (project.get shows it) and the object IDs, so they stay the same in every export. Every new or loaded document has one. An export never changes the document. project.new_guid makes a fresh GUID, for a copy of a template.",
        "examples": [
            {"method": "export.file", "params": {"path": "/data/site.ifc", "format": "ifc", "options": {"layers_as_storeys": false}, "overwrite": true}}
        ]
    })
}

/// Options and mappings of the IFC import, for `describe`.
fn import_describe() -> Value {
    json!({
        "call": "import.file with a path that ends in .ifc (or format \"ifc\"). One undo step. The result has imported, skipped, report (counts, skipped items with reasons, notes) and georeference.",
        "options": {
            "layers_from_storeys": "true (default): one design layer per IfcBuildingStorey with its elevation; other elements go to the layer \"Site\". false: the active layer",
            "classes_from_layers": "true (default): one class per IfcPresentationLayerAssignment. false: the active class",
            "property_sets": "records (default: one record format per property set or quantity set, attached to the objects) | data (object data ifc_psets) | none",
            "terrain": "site_model (default: the vertices of the terrain are the points of a site model, which Cadektor triangulates again) | mesh (a 3D mesh object with the original triangles)",
            "instances": "true (default): an IfcMappedItem that is used twice or more and consists of upright extrusions becomes a symbol with 3D components. false: every use is a mesh",
            "vegetation": "true (default): vegetation with a thin flat extrusion becomes a landscape area, vegetation with a size becomes a plant. false: a mesh",
            "georef": "true (default): IfcMapConversion sets the georeference of an empty document, or moves the geometry into the georeference of a document that has the same CRS",
            "max_objects": "Default 500000",
            "max_triangles": "Default 20000000",
            "max_file_mb": "Default 1024"
        },
        "mapping": {
            "IfcSlab, IfcCovering, IfcWall and every other element with a body": "mesh3d (generic 3D mesh). data.ifc_class holds the IFC class, data.ifc the GlobalId, name, type and tag. Nothing is turned into a hardscape",
            "IfcGeographicElement TERRAIN, IfcSite with a face set": "site_model (or mesh3d)",
            "IfcGeographicElement vegetation": "landscape_area, plant or mesh3d",
            "IfcAnnotation": "polyline (arcs stay exact), polyline3d, locus3d",
            "geometry": "IfcExtrudedAreaSolid (rectangle, rounded rectangle, circle, ellipse, I shape, arbitrary profile with voids, composite curves with arcs), IfcTriangulatedFaceSet, IfcPolygonalFaceSet, IfcFacetedBrep, IfcShellBasedSurfaceModel, IfcMappedItem, IfcBooleanClippingResult with an IfcHalfSpaceSolid, IfcBoundingBox",
            "not supported": "openings are not cut, bounded half spaces, IfcAdvancedBrep, sweeps, revolutions and CSG. They are listed in the report; an IfcBoundingBox representation stands in for them when the product has one"
        },
        "units": "IfcSIUnit prefixes and conversion based units (feet, inches) for lengths, areas, volumes and angles",
        "examples": [
            {"method": "import.file", "params": {"path": "/data/site.ifc", "options": {"terrain": "mesh", "property_sets": "data"}}}
        ]
    })
}

fn import_rpc(e: IfcImportError) -> RpcError {
    match e {
        IfcImportError::Option(_) => RpcError::invalid_params(e),
        e => RpcError::app(e),
    }
}

fn rpc(e: IfcError) -> RpcError {
    match e {
        IfcError::Option(_) => RpcError::invalid_params(e),
        e => RpcError::app(e),
    }
}

impl Session {
    pub(crate) fn import_ifc_file(
        &mut self,
        path: PathBuf,
        options: &Value,
        checked: bool,
    ) -> Result<Value, RpcError> {
        let opts = IfcImportOptions::from_json(options).map_err(import_rpc)?;
        let path = if checked {
            self.check_path(&path, false, &["ifc"])
                .map_err(RpcError::policy)?
        } else {
            path
        };
        let result =
            cadektor_ifc::import_file(&path, self.editor.doc(), &opts).map_err(import_rpc)?;
        self.editor
            .execute(&result.command)
            .map_err(RpcError::from)?;
        self.prune_selection();
        Ok(json!({
            "imported": result.report.total(),
            "skipped": result.report.skipped_total(),
            "report": result.report,
            "georeference": self.editor.doc().georef,
        }))
    }

    pub(crate) fn export_ifc_file(
        &mut self,
        path: PathBuf,
        options: &Value,
        overwrite: bool,
        checked: bool,
    ) -> Result<Value, RpcError> {
        let opts = IfcOptions::from_json(options).map_err(rpc)?;
        let path = if checked {
            self.check_path(&path, true, &["ifc"])
                .map_err(RpcError::policy)?
        } else {
            path
        };
        if checked && self.paths.is_restricted() && path.exists() && !overwrite {
            return Err(RpcError::conflict(format!(
                "{} exists: pass \"overwrite\": true to replace it",
                path.display()
            )));
        }
        let doc = self.editor.doc();
        let report = cadektor_ifc::export_file(&path, doc, &opts).map_err(rpc)?;
        Ok(
            json!({ "path": path, "exported": report.total(), "report": report, "project_guid": cadektor_ifc::project_guid(doc).0 }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_ifc_requests() {
        assert!(is_ifc(Some("IFC"), Path::new("a.dxf")));
        assert!(is_ifc(Some(".ifc"), Path::new("a")));
        assert!(is_ifc(None, Path::new("model.IFC")));
        assert!(!is_ifc(None, Path::new("model.dxf")));
        assert!(!is_ifc(Some("dxf"), Path::new("a.ifc")));
        assert!(describe()["options"]["version"].is_string());
    }
}
