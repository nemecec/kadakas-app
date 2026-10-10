//! `export.file` with the format `ifc`. Native only: it writes a file.

use std::path::{Path, PathBuf};

use cadektor_ifc::{IfcError, IfcOptions};
use serde_json::{json, Value};

use crate::{RpcError, Session};

/// True if the request asks for IFC: the format is `ifc`, or there is no format and the file ends in `.ifc`.
pub(crate) fn is_ifc(format: Option<&str>, path: &Path) -> bool {
    match format {
        Some(f) => f.trim().trim_start_matches('.').eq_ignore_ascii_case("ifc"),
        None => path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifc")),
    }
}

/// Options and mappings of the IFC export, for `describe`.
pub(crate) fn describe() -> Value {
    json!({
        "format": "ifc: IFC4 (IFC4 ADD2 TC1, STEP physical file). Export only.",
        "options": {
            "version": "IFC4 (the only value for now)",
            "classes_as_layers": "true (default): one IfcPresentationLayerAssignment per class",
            "include_2d": "false (default). true writes 2D objects, text and survey points as IfcAnnotation",
            "layers_as_storeys": "false (default): all elements belong to the IfcSite. true adds an IfcBuilding with one IfcBuildingStorey per design layer",
            "visible_only": "false (default)",
            "data_map": "name of a data map (Data Manager) for IFC classes and property sets. Default: the first data map of the document, else the built-in landscape mapping",
            "area_thickness": "mm, thickness of the solid of a landscape area. Default 100",
            "time_stamp": "YYYY-MM-DDThh:mm:ss for the file header. Default: the project date, so the same document gives the same bytes"
        },
        "mapping": {
            "site_model": "IfcGeographicElement TERRAIN, IfcTriangulatedFaceSet of the proposed surface",
            "hardscape": "IfcSlab BASESLAB (Vectorworks default). IfcExtrudedAreaSolid when flat, IfcTriangulatedFaceSet when sloped or draped. Style components become an IfcMaterialLayerSet. Pset_SlabCommon and Qto_SlabBaseQuantities",
            "landscape_area": "IfcGeographicElement (USERDEFINED, ObjectType VEGETATION: IFC4 has no VEGETATION type), thin extrusion with holes",
            "plant, existing_tree": "IfcGeographicElement (USERDEFINED, ObjectType VEGETATION), trunk and canopy cylinders plus an IfcBoundingBox",
            "other": "2D objects are skipped and listed in the report"
        },
        "georeference": "IfcMapConversion and IfcProjectedCRS (EPSG name) from the document georeference, and RefLatitude/RefLongitude of the IfcSite when the CRS is known. Needs a CRS in the georeference.",
        "global_ids": "GlobalIds come from the project GUID (project.get shows it) and the object IDs, so they stay the same in every export. Every new or loaded document has one. An export never changes the document. project.new_guid makes a fresh GUID, for a copy of a template.",
        "examples": [
            {"method": "export.file", "params": {"path": "/data/site.ifc", "format": "ifc", "options": {"layers_as_storeys": false}, "overwrite": true}}
        ]
    })
}

fn rpc(e: IfcError) -> RpcError {
    match e {
        IfcError::Option(_) => RpcError::invalid_params(e),
        e => RpcError::app(e),
    }
}

impl Session {
    pub(crate) fn export_ifc_file(&mut self, path: PathBuf, options: &Value, overwrite: bool, checked: bool) -> Result<Value, RpcError> {
        let opts = IfcOptions::from_json(options).map_err(rpc)?;
        let path = if checked { self.check_path(&path, true, &["ifc"]).map_err(RpcError::app)? } else { path };
        if checked && self.paths.is_restricted() && path.exists() && !overwrite {
            return Err(RpcError::app(format!("{} exists: pass \"overwrite\": true to replace it", path.display())));
        }
        let doc = self.editor.doc();
        let report = cadektor_ifc::export_file(&path, doc, &opts).map_err(rpc)?;
        Ok(json!({ "path": path, "exported": report.total(), "report": report, "project_guid": cadektor_ifc::project_guid(doc).0 }))
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
