//! `import.file` and `export.file`. Native only: they read and write files.

use std::path::PathBuf;

use kadakas_io::{ExportOptions, Format, ImportOptions, IoError};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{RpcError, Session, params};

#[derive(Deserialize)]
struct FileParams {
    path: PathBuf,
    format: Option<String>,
    #[serde(default)]
    options: Value,
    /// Export: replace a file that exists. Needed in a restricted session.
    #[serde(default)]
    overwrite: bool,
}

fn rpc(e: IoError) -> RpcError {
    match e {
        IoError::Option(_) => RpcError::invalid_params(e),
        IoError::Format(_) => RpcError::validation(e),
        e => RpcError::app(e),
    }
}

fn format_of(name: Option<&str>) -> Result<Option<Format>, RpcError> {
    match name {
        None => Ok(None),
        Some(n) => Format::from_name(n).map(Some).ok_or_else(|| {
            RpcError::invalid_params(format!(
                "unknown format '{n}'. Use dxf, shp, geojson, ifc or dwg (export also: gltf, glb, obj)"
            ))
        }),
    }
}

/// Formats, options and examples, for `describe`.
pub(crate) fn describe() -> Value {
    json!({
        "import_formats": ["dxf (ASCII)", "shp (with .dbf and .prj next to it)", "geojson", "ifc (IFC2x3, IFC4, IFC4x3; options in describe.io.ifc.import)", "dwg (needs a converter that you install, see describe.io.dwg)"],
        "export_formats": ["dxf (ASCII)", "geojson", "dwg (needs a converter that you install, see describe.io.dwg)", "ifc (IFC4, IFC2X3 or IFC4X3, see describe.io.ifc)", "gltf, glb, obj (3D, see describe.io.x3d)", "pdf, sheet_svg, sheet_png (sheet layers, see describe.sheets.export)"],
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
            "origin": "center (default) | file. A drawing whose center is more than 1 km from (0, 0) is shifted to the origin, and the shift is saved as the document georeference (empty CRS if the file has none). file keeps the numbers of the file (objects beyond 1e9 mm are skipped). A document that has a georeference keeps its own origin",
            "faces": "DXF: polygons (default: each 3DFACE is a closed polyline3d, source data for a site model) | mesh (all faces of a DXF layer become one mesh3d). POLYFACE and POLYGON MESH polylines always become mesh3d",
            "max_exploded": "DXF: most objects that exploded blocks may make. Default 100000. The import fails above it",
            "max_objects": "stop after this many objects. Default 500000",
            "track_source": "DXF and DWG: true (default). Each object keeps its source in data.import: {source: 'file name#hash of the file header', handle: DXF handle (group code 5), kind: DXF type, hash: hash of the entity, block, via, part}. The objects go to a layer of their own named after the file (classes mode), or to one layer per DXF layer (layers mode). These layers never reuse a design layer, and they carry the source too (layer.import_source). false: the objects go to the active layer without tags. A copy of an object does not keep the tag",
            "source_name": "DXF and DWG: the file name that the tags record. Default: the name of the file"
        },
        "export_options": {
            "layer_mode": "classes (default: one DXF layer per class) | layers (one per design layer)",
            "unit": "mm (default) | cm | m | km | inch | foot | yard | mile. DXF only",
            "origin": "file (default): real-world coordinates, the origin shift of the georeference is added back (a rotation is not applied in DXF). document: model coordinates",
            "visible_only": "false (default)",
            "native_annotations": "DXF: true (default). Linear, radial and diameter dimensions become DIMENSION entities (each with its block), callouts become LEADER and MTEXT, text becomes MTEXT. Other annotations are lines and text. false: all annotations are lines and text",
            "site_mesh": "DXF: none (default) | existing | proposed. Also writes the triangles of a site model surface as a 3D mesh",
            "mesh_entity": "DXF: polyface (default: a POLYLINE polyface mesh) | face3d (one 3DFACE per triangle). For mesh3d objects and the site mesh",
            "tolerance": "mm, curve approximation. Default 1"
        },
        "ifc": crate::ifc_api::describe(),
        "dwg": crate::dwg_api::describe(),
        "x3d": crate::x3d_api::describe()["export"],
        "section_viewport": crate::x3d_api::describe()["section_viewport"],
        "examples": [
            {"method": "import.file", "params": {"path": "/data/site.dxf", "options": {"layer_mode": "classes"}}},
            {"method": "import.file", "params": {"path": "/data/trees.geojson", "options": {"class_field": "species"}}},
            {"method": "export.file", "params": {"path": "/data/out.dxf", "format": "dxf", "overwrite": true}},
            {"method": "export.file", "params": {"path": "/data/site.ifc", "format": "ifc", "overwrite": true}}
        ],
        "paths": "A server for agents only reads and writes inside the folder of the open document and the folders that its owner allows with --allow-dir (default: the folder where it started). The extension must match the format, and export.file needs {overwrite: true} to replace a file.",
        "dxf_georeference": "A DXF export of a document with an EPSG georeference writes a GEODATA object (projected grid, the design point and the reference point, the unit and the north direction, and the CRS as an EPSG alias). The coordinates are real-world numbers (origin: file) or model numbers (origin: document). Other CRSs get a note and no GEODATA",
        "georeference": "Imports of GIS files set the document georeference: {crs, origin: [x, y] in meters, rotation, user_origin}. Geometry stays near 0. Set it with the command {op: set_georeference, georef: {crs: \"EPSG:3067\", origin: [385000, 6672000]}}. document.summary shows it."
    })
}

/// Extensions of a format, for the path check.
fn extensions(f: Format) -> &'static [&'static str] {
    match f {
        Format::Dxf => &["dxf"],
        Format::Shapefile => &["shp"],
        Format::GeoJson => &["geojson", "json"],
    }
}

impl Session {
    /// The path that a file request may use. In a restricted session the file must be in an allowed folder and have
    /// the extension of its format.
    fn file_path(
        &self,
        path: PathBuf,
        format: Option<Format>,
        write: bool,
    ) -> Result<PathBuf, RpcError> {
        if !self.paths.is_restricted() {
            return Ok(path);
        }
        let format = format.or_else(|| Format::from_path(&path)).ok_or_else(|| {
            RpcError::invalid_params("unknown file type: give a format (dxf, shp or geojson)")
        })?;
        self.check_path(&path, write, extensions(format))
            .map_err(RpcError::policy)
    }

    pub(crate) fn import_file(&mut self, p: Value) -> Result<Value, RpcError> {
        self.import_impl(p, true)
    }

    pub(crate) fn export_file(&mut self, p: Value) -> Result<Value, RpcError> {
        self.export_impl(p, true)
    }

    /// `import.file` for the user's own file dialog: the path policy for agents does not apply.
    pub fn import_file_trusted(&mut self, p: Value) -> Result<Value, RpcError> {
        self.import_impl(p, false)
    }

    /// `export.file` for the user's own file dialog: the path policy for agents does not apply.
    pub fn export_file_trusted(&mut self, p: Value) -> Result<Value, RpcError> {
        self.export_impl(p, false)
    }

    fn import_impl(&mut self, p: Value, checked: bool) -> Result<Value, RpcError> {
        let FileParams {
            path,
            format,
            options,
            ..
        } = params(p)?;
        if crate::dwg_api::is_dwg(format.as_deref(), &path) {
            return self.import_dwg_file(path, &options, checked);
        }
        if crate::ifc_api::is_ifc(format.as_deref(), &path) {
            return self.import_ifc_file(path, &options, checked);
        }
        let opts = ImportOptions::from_json(&options).map_err(rpc)?;
        let format = format_of(format.as_deref())?;
        let path = if checked {
            self.file_path(path, format, false)?
        } else {
            path
        };
        let result =
            kadakas_io::import_file(&path, format, &opts, self.editor.doc()).map_err(rpc)?;
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

    fn export_impl(&mut self, p: Value, checked: bool) -> Result<Value, RpcError> {
        let FileParams {
            path,
            format,
            options,
            overwrite,
        } = params(p)?;
        if crate::dwg_api::is_dwg(format.as_deref(), &path) {
            return self.export_dwg_file(path, &options, overwrite, checked);
        }
        if crate::ifc_api::is_ifc(format.as_deref(), &path) {
            return self.export_ifc_file(path, &options, overwrite, checked);
        }
        if let Some(kind) = crate::x3d_api::kind_of(format.as_deref(), &path) {
            return self.export_x3d_file(kind, path, &options, overwrite, checked);
        }
        if let Some(f) = format
            .as_deref()
            .filter(|f| crate::sheets_api::is_sheet_format(f))
        {
            return self.export_sheet_file(f, path, &options, overwrite, checked);
        }
        let opts = ExportOptions::from_json(&options).map_err(rpc)?;
        let format = format_of(format.as_deref())?;
        let path = if checked {
            self.file_path(path, format, true)?
        } else {
            path
        };
        if checked && self.paths.is_restricted() && path.exists() && !overwrite {
            return Err(RpcError::conflict(format!(
                "{} exists: pass \"overwrite\": true to replace it",
                path.display()
            )));
        }
        let report =
            kadakas_io::export_file(&path, format, &opts, self.editor.doc()).map_err(rpc)?;
        Ok(json!({ "path": path, "exported": report.total(), "report": report }))
    }
}
