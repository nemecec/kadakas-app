//! `import.file` and `export.file`. Native only: they read and write files.

use std::path::PathBuf;

use cadektor_io::{ExportOptions, Format, ImportOptions, IoError};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{params, RpcError, Session};

#[derive(Deserialize)]
struct FileParams {
    path: PathBuf,
    format: Option<String>,
    #[serde(default)]
    options: Value,
}

fn rpc(e: IoError) -> RpcError {
    match e {
        IoError::Option(_) => RpcError::invalid_params(e),
        e => RpcError::app(e),
    }
}

fn format_of(name: Option<&str>) -> Result<Option<Format>, RpcError> {
    match name {
        None => Ok(None),
        Some(n) => Format::from_name(n)
            .map(Some)
            .ok_or_else(|| RpcError::invalid_params(format!("unknown format '{n}'. Use dxf, shp or geojson"))),
    }
}

/// Formats, options and examples, for `describe`.
pub(crate) fn describe() -> Value {
    json!({
        "import_formats": ["dxf (ASCII)", "shp (with .dbf and .prj next to it)", "geojson"],
        "export_formats": ["dxf (ASCII)", "geojson"],
        "import_options": {
            "layer_mode": "classes (default: DXF layers become classes on one design layer) | layers (DXF layers become design layers)",
            "layer": "name of the design layer that receives the objects (created if needed)",
            "unit": "mm | cm | m | km | inch | foot | yard | mile. DXF only. Default: $INSUNITS of the file, else mm",
            "explode_blocks": "true (default). DXF INSERT becomes objects",
            "points": "all (default) | with_z. DXF POINT becomes locus3d",
            "hatches": "true (default). DXF HATCH boundaries become closed polylines",
            "tolerance": "mm, curve approximation for ellipses and splines. Default 2",
            "source_crs": "GIS: CRS of the file if it does not say. EPSG:n, PROJ string or WKT",
            "target_crs": "GIS: CRS of a new georeference. Default: UTM zone for lon/lat data, else the file CRS",
            "class_field": "GIS: attribute whose value names the class of each object. Default: the file name",
            "max_objects": "stop after this many objects"
        },
        "export_options": {
            "layer_mode": "classes (default: one DXF layer per class) | layers (one per design layer)",
            "unit": "mm (default) | cm | m | km | inch | foot | yard | mile. DXF only",
            "visible_only": "false (default)",
            "tolerance": "mm, curve approximation. Default 1"
        },
        "examples": [
            {"method": "import.file", "params": {"path": "/data/site.dxf", "options": {"layer_mode": "classes"}}},
            {"method": "import.file", "params": {"path": "/data/trees.geojson", "options": {"class_field": "species"}}},
            {"method": "export.file", "params": {"path": "/data/out.dxf", "format": "dxf"}}
        ],
        "georeference": "Imports of GIS files set the document georeference: {crs, origin: [x, y] in meters, rotation, user_origin}. Geometry stays near 0. Set it with the command {op: set_georeference, georef: {crs: \"EPSG:3067\", origin: [385000, 6672000]}}. document.summary shows it."
    })
}

impl Session {
    pub(crate) fn import_file(&mut self, p: Value) -> Result<Value, RpcError> {
        let FileParams { path, format, options } = params(p)?;
        let opts = ImportOptions::from_json(&options).map_err(rpc)?;
        let result = cadektor_io::import_file(&path, format_of(format.as_deref())?, &opts, self.editor.doc()).map_err(rpc)?;
        self.editor.execute(&result.command).map_err(RpcError::app)?;
        self.prune_selection();
        Ok(json!({
            "imported": result.report.total(),
            "skipped": result.report.skipped_total(),
            "report": result.report,
            "georeference": self.editor.doc().georef,
        }))
    }

    pub(crate) fn export_file(&mut self, p: Value) -> Result<Value, RpcError> {
        let FileParams { path, format, options } = params(p)?;
        let opts = ExportOptions::from_json(&options).map_err(rpc)?;
        let report = cadektor_io::export_file(&path, format_of(format.as_deref())?, &opts, self.editor.doc()).map_err(rpc)?;
        Ok(json!({ "path": path, "exported": report.total(), "report": report }))
    }
}
