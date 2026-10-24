//! `import.file` and `export.file` with the mesh formats of MOD-12. Native only: they read and
//! write files.
//!
//! - Import: `obj`, `stl` (ASCII and binary), `gltf` and `glb` become `mesh3d` objects, one for
//!   each object and material of the file (the option `join` makes one). The option `to_surface`
//!   (implied by `soften_below_angle`) makes surface models of the parts that fit the limits of a
//!   surface model, and softens their edges under the angle. `preview` reads and converts the
//!   file and changes nothing. The result always says how many edges were softened.
//! - Export: `stl` (binary or ASCII) of the 3D objects of the document. OBJ and glTF are
//!   written by `x3d_api`, IFC by `ifc_api` and DXF by `kadakas-io`.
//!
//! The parsers are in `kadakas-meshio`, the conversion in `kadakas_core::surface`.

use std::path::{Path, PathBuf};

use glam::DVec3;
use serde::Deserialize;
use serde_json::{Value, json};

use kadakas_core::command::{Command, CommandError};
use kadakas_core::exact::{self, ExactSolid};
use kadakas_core::mesh3d::{self, Mesh3d};
use kadakas_core::model::{Attributes, Color, Document, LayerKind, ObjectId, Shape};
use kadakas_core::surface::{ConvertError, ConvertOptions, Converted, surface_from_triangles};
use kadakas_meshio::{Format, Frame, Limits, MeshError, MeshSet, Unit, Up};

use crate::errors::{app, bad, convert_rpc};
use crate::{RpcError, Session};

/// The most parts that an import lists one by one in its report.
const LISTED_PARTS: usize = 200;

/// The mesh format that a request asks for: the format name, or without one the file extension.
pub(crate) fn format_of(format: Option<&str>, path: &Path) -> Option<Format> {
    match format {
        Some(f) => Format::from_name(f),
        None => Format::from_name(path.extension()?.to_str()?),
    }
}

/// Is this format one that `import.file` reads? (STL is also written by `export.file`.)
pub(crate) fn extensions(f: Format) -> &'static [&'static str] {
    match f {
        Format::Obj => &["obj"],
        Format::Stl => &["stl"],
        Format::Gltf => &["gltf"],
        Format::Glb => &["glb"],
        Format::Step => &["step", "stp"],
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ImportOptions {
    unit: Option<String>,
    up_axis: Option<String>,
    layer: Option<String>,
    name: Option<String>,
    to_surface: Option<bool>,
    soften_below_angle: Option<f64>,
    tolerance: Option<f64>,
    merge: Option<bool>,
    preview: bool,
    center: bool,
    join: bool,
    restore_origin: Option<bool>,
    max_bytes: Option<u64>,
    max_triangles: Option<u64>,
    max_vertices: Option<u64>,
    /// STEP: one mesh for each product and color (default) or for each body and color.
    per_product: Option<bool>,
    /// STEP: keep the exact surfaces next to the triangles (default true).
    keep_exact: Option<bool>,
    /// STEP: the largest turn of the normal between neighbouring points, degrees.
    angle: Option<f64>,
}

/// The options of the surface conversion that a request holds, if it asks for one.
pub(crate) struct SurfaceAsk {
    pub opts: ConvertOptions,
}

impl SurfaceAsk {
    /// `to_surface`, `soften_below_angle`, `tolerance` and `merge` of an options object. `None`
    /// when no conversion is asked for. A softening angle asks for one.
    pub(crate) fn of(
        to_surface: Option<bool>,
        soften: Option<f64>,
        tolerance: Option<f64>,
        merge: Option<bool>,
    ) -> Result<Option<SurfaceAsk>, RpcError> {
        if let Some(a) = soften
            && !(a.is_finite() && (0.0..=180.0).contains(&a))
        {
            return Err(bad("soften_below_angle: degrees, 0 to 180"));
        }
        if let Some(t) = tolerance
            && !(t.is_finite() && (0.001..=1000.0).contains(&t))
        {
            return Err(bad("tolerance: mm, 0.001 to 1000"));
        }
        let wanted = to_surface.unwrap_or(soften.is_some());
        if !wanted {
            if soften.is_some() {
                return Err(bad(
                    "soften_below_angle needs a surface model: leave to_surface out or set it true",
                ));
            }
            return Ok(None);
        }
        Ok(Some(SurfaceAsk {
            opts: ConvertOptions {
                tolerance,
                soften_below_angle: soften,
                merge: merge.unwrap_or(true),
            },
        }))
    }
}

pub(crate) use crate::errors::mesh_rpc;

/// The counts of the conversions of one import, for `report.to_surface`.
pub(crate) struct Tally {
    converted: usize,
    faces: usize,
    edges: usize,
    softened: usize,
    welded: usize,
    degenerate: usize,
    duplicate: usize,
    reversed: usize,
    unmerged: usize,
    joined_slivers: usize,
    refused: Vec<Value>,
    notes: Vec<String>,
    parts: usize,
    angle: Option<f64>,
}

impl Tally {
    pub(crate) fn new(angle: Option<f64>, parts: usize) -> Tally {
        Tally {
            converted: 0,
            faces: 0,
            edges: 0,
            softened: 0,
            welded: 0,
            degenerate: 0,
            duplicate: 0,
            reversed: 0,
            unmerged: 0,
            joined_slivers: 0,
            refused: Vec::new(),
            notes: Vec::new(),
            parts,
            angle,
        }
    }

    pub(crate) fn ok(&mut self, name: &str, c: &Converted) {
        let r = &c.report;
        self.converted += 1;
        self.faces += r.faces;
        self.edges += r.edges;
        self.softened += r.softened_edges;
        self.welded += r.welded_vertices;
        self.degenerate += r.degenerate_triangles;
        self.duplicate += r.duplicate_triangles;
        self.reversed += r.reversed_triangles;
        self.unmerged += r.unmerged_regions;
        self.joined_slivers += r.joined_slivers;
        for n in &c.notes {
            self.notes.push(if self.parts > 1 {
                format!("{name}: {n}")
            } else {
                n.clone()
            });
        }
        if r.unmerged_regions > 0 {
            self.notes.push(format!(
                "{name}: {} regions of triangles in one plane could not become one face",
                r.unmerged_regions
            ));
        }
    }

    pub(crate) fn refused(&mut self, name: &str, object: Option<ObjectId>, e: &ConvertError) {
        let err = convert_rpc(e);
        self.refused.push(json!({
            "part": name,
            "object": object,
            "reason": e.to_string(),
            "data": err.details,
        }));
    }

    pub(crate) fn softened(&self) -> usize {
        self.softened
    }

    pub(crate) fn json(&self) -> Value {
        json!({
            "converted": self.converted,
            "refused": self.refused,
            "faces": self.faces,
            "edges": self.edges,
            "softened_edges": self.softened,
            "soften_below_angle": self.angle,
            "welded_vertices": self.welded,
            "degenerate_triangles": self.degenerate,
            "duplicate_triangles": self.duplicate,
            "reversed_triangles": self.reversed,
            "joined_slivers": self.joined_slivers,
            "unmerged_regions": self.unmerged,
            "notes": self.notes,
        })
    }
}

/// Turns the mesh objects that the commands of a step made into surface models, as far as they
/// fit the limits (`from` is the number of created ids before those commands). A mesh over a
/// limit stays a mesh and is listed in `refused` with the counts. The answer is the summary for
/// the report: `{converted, refused, faces, edges, softened_edges}`.
pub(crate) fn convert_created(
    step: &mut kadakas_core::command::StepRun<'_, '_>,
    from: usize,
    ask: &SurfaceAsk,
) -> Result<Value, CommandError> {
    let ids: Vec<ObjectId> = step.created()[from..]
        .iter()
        .map(|u| ObjectId(*u))
        .filter(|id| {
            matches!(
                step.doc().object(*id).map(|o| &o.shape),
                Some(Shape::Mesh3d(_))
            )
        })
        .collect();
    let mut tally = Tally::new(ask.opts.soften_below_angle, ids.len());
    for id in ids {
        let (result, layer, color, name) = {
            let o = step.doc().object(id).expect("listed above");
            let Shape::Mesh3d(m) = &o.shape else { continue };
            (
                surface_from_triangles(&m.vertices, &m.triangles, &ask.opts),
                o.layer,
                m.color,
                m.name.clone(),
            )
        };
        match result {
            Ok(c) => {
                let elevation = step.doc().layer(layer).map_or(0.0, |l| l.elevation);
                tally.ok(&name, &c);
                let Converted {
                    mut model, origin, ..
                } = c;
                model.transform = glam::DAffine3::from_translation(DVec3::new(
                    origin.x,
                    origin.y,
                    origin.z - elevation,
                ));
                model.name = name;
                let mut attrs = step
                    .doc()
                    .object(id)
                    .map(|o| o.attrs.clone())
                    .unwrap_or_default();
                if attrs.fill.is_none() {
                    attrs.fill = color;
                }
                step.run(&Command::ModifyObject {
                    id,
                    shape: Some(Shape::SurfaceModel(Box::new(model))),
                    layer: None,
                    class: None,
                    attrs: Some(attrs),
                })?;
            }
            Err(e) => tally.refused(&name, Some(id), &e),
        }
    }
    Ok(tally.json())
}

/// Formats, options and examples for `describe`.
pub(crate) fn describe() -> Value {
    json!({
        "import": {
            "formats": {
                "obj": "Wavefront OBJ (v, f with any number of corners, negative indices, o, g, usemtl, mtllib). Colors come from the Kd and d of the MTL file next to it. Normals and texture coordinates are ignored. Default unit m, Y up.",
                "stl": "STL, binary or ASCII (found by the size). Equal corners are joined. A binary header with COLOR=rgba sets the color. Default unit mm, Z up.",
                "gltf": "glTF 2.0 with the buffers inside or in files next to it. The nodes of the default scene with their transforms. Base color factors become colors. Always meters and Y up. Draco, Meshopt, quantized and sparse data are refused.",
                "glb": "glTF 2.0 in one binary file",
                "step": "STEP (ISO 10303-21 text, .step and .stp) with the schemas AUTOMOTIVE_DESIGN (AP214), CONFIG_CONTROL_DESIGN (AP203) and AP242. Solids (MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS, FACETED_BREP), surface models and tessellated items are cut into triangles on load. Faces on planes, cylinders, cones, spheres, tori, surfaces of revolution and extrusion and B-spline surfaces (also rational) are read. Edges are lines, circles, ellipses, B-spline curves, trimmed and composite curves. Units come from the file (mm, m, inch and so on). Assemblies keep product names and placements. Colors are kept. Lines and polylines become 3D polylines. The report lists every entity type that was read, skipped (with count and reason) and cut. Each mesh keeps the exact surfaces next to its triangles (option keep_exact). Always Z up in mm."
            },
            "call": "import.file with a path (the extension or format picks the reader). One undo step. Normals in the file are not used. The sides of a mesh follow the winding of its triangles. The SketchUp file format is not read.",
            "options": {
                "unit": "OBJ and STL: mm | cm | m | km | inch | foot | yard | mile. Default m for OBJ and mm for STL. glTF is always m. STEP: the unit of the numbers, which replaces the unit that the file names. Default: the file",
                "up_axis": "OBJ and STL: y | z. Default y for OBJ, z for STL. Y up files turn so that Z is up in the model (x, y, z becomes x, -z, y)",
                "layer": "name of the design layer that receives the objects (created if needed). Default the active layer",
                "name": "name of the objects. Default the names in the file, else the file name",
                "join": "false (default: one object for each object and material of the file) | true (one mesh for the whole file; the color stays if all parts have the same)",
                "restore_origin": "true (default). OBJ and glTF files that Kadakas wrote say where their origin was in the model (a comment in the OBJ, the extension KADAKAS_georeference in glTF). The model goes back to that place, so a round trip returns it to where it was. false keeps the local numbers of the file. STL files carry no origin, but the STL export writes the coordinates of the model",
                "center": "false (default: the coordinates of the file) | true (the middle of the box around the file goes to x = 0 and y = 0, and the lowest point to z = 0)",
                "to_surface": "false (default: mesh3d objects). true: parts that fit a surface model (55,000 vertices, 150,000 edges, 100,000 faces) become surface models, with vertices welded and coplanar triangles merged into faces. A part over a limit stays a mesh3d and is listed in report.to_surface.refused with the counts",
                "soften_below_angle": "degrees, 0 to 180, default off. Edges between faces at an angle below it become soft and smooth. It asks for to_surface",
                "tolerance": "mm, 0.001 to 1000. STEP: the largest distance of a triangle from its surface, default from the size of the file (1/2000 of the diagonal, between 0.01 and 0.5). The other formats: the weld distance of to_surface, default 0.01",
                "angle": "STEP only. Degrees, 1 to 90, default 15. The largest turn of the normal between neighbouring points",
                "per_product": "STEP only. true (default): one mesh for each product and color. false: one mesh for each body and color. join true makes one mesh for the whole file",
                "keep_exact": "STEP only. true (default): each mesh keeps the exact surfaces (radii, normals, curves) next to its triangles, up to 8 MiB for one object and 64 MiB for the document. false keeps the triangles only. Over a cap the mesh keeps its triangles and the report names the part",
                "merge": "to_surface: true (default) merges coplanar triangles into faces, false keeps every triangle a face",
                "preview": "true reads the file and answers with the report, including the number of edges that soften_below_angle would soften, and changes nothing",
                "max_bytes": "the largest file in bytes, checked before the file is read. Default 536870912 and never above the limit import_text_bytes of the session. A STEP file is limited to 134217728 (128 MiB) because it takes about 18 times its size in memory",
                "max_triangles": "Default and most 8000000, in all parts together",
                "max_vertices": "Default and most 4000000, in all parts together"
            },
            "step_result": "A STEP import adds report.step: {schemas, entities_read, entities_used, entities_skipped (read equals used plus skipped), used: [{entity, count}], skipped: [{entity, reason, count}], tessellated: [{surface, faces, triangles}], products, instances, length_unit_mm, unit_from_file, tolerance_mm, angle_deg, welded_vertices}. It adds report.exact: {enabled, kept_objects, approx_bytes, lost_to_surface, without: [{part, reason}]} and report.curves (3D polylines made). A file with no readable geometry is refused with error_id mesh_unsupported and the reasons.",
            "result": "{imported, skipped, softened_edges (always, 0 when nothing was softened), report: {format, bytes, unit, up_axis, parts, vertices, triangles, degenerate_triangles, bounds_mm: {min, max, size}, objects: [{id, name, kind, vertices, triangles, color}], notes, to_surface?: {converted, refused: [{part, object?, reason, data}], faces, edges, softened_edges, welded_vertices, degenerate_triangles, duplicate_triangles, reversed_triangles, joined_slivers, unmerged_regions, notes}, preview?}, georeference}. A file that is too large, has too many triangles, ends too soon, has a number that is not finite or an index that does not exist is refused with an error whose reason names the problem (mesh_file_too_large, mesh_too_many_triangles, mesh_truncated, mesh_not_finite, mesh_bad_index and so on) and whose data has the counts. Nothing changes",
            "examples": [
                {"method": "import.file", "params": {"path": "/data/model.obj", "options": {"unit": "mm", "up_axis": "z"}}},
                {"method": "import.file", "params": {"path": "/data/part.stl", "options": {"to_surface": true, "soften_below_angle": 20}}},
                {"method": "import.file", "params": {"path": "/data/scene.glb", "options": {"preview": true, "soften_below_angle": 20}}},
                {"method": "import.file", "params": {"path": "/data/bench.step", "options": {"tolerance": 0.05, "per_product": true}}},
                {"method": "mesh3d.exact_info", "params": {"id": "00000000-0000-0000-0000-000000000002"}},
                {"method": "mesh3d.retessellate", "params": {"id": "00000000-0000-0000-0000-000000000002", "tolerance": 0.01}}
            ]
        },
        "export": {
            "format": "stl (binary by default, or ASCII) of the 3D objects of the document: surface models, meshes and the other solids. Triangles only, no colors. OBJ and glTF are written by the formats obj, gltf and glb, IFC by ifc, DXF by dxf (see mesh_entity)",
            "options": {
                "encoding": "binary (default) | ascii",
                "units": "mm (default) | m",
                "y_up": "false (default, Z up) | true",
                "objects": "list of object IDs to export. Default all objects that have 3D geometry",
                "plant_detail": "fine (default) | coarse | billboard | auto",
                "name": "the name in the solid line of an ASCII file and in the header of a binary file"
            },
            "result": "{path, exported (objects), report: {triangles, objects, bytes, encoding, units, y_up, origin_mm, open_models (exported surface models that are not closed solids), leaky_models (closed solids whose triangles are not watertight), skipped: [{id, reason}], notes}}. The coordinates are those of the model, so reading the file again puts it where it was. STL holds no colors, materials or lines. A printer wants closed solids: open_models says how many surface models are not.",
            "examples": [
                {"method": "export.file", "params": {"path": "/data/part.stl", "format": "stl", "options": {"encoding": "ascii"}, "overwrite": true}}
            ]
        }
    })
}

fn unit_of(s: &str) -> Result<Unit, RpcError> {
    Unit::from_name(s).ok_or_else(|| {
        bad(format!(
            "unit '{s}': use mm, cm, m, km, inch, foot, yard or mile"
        ))
    })
}

fn up_of(s: &str) -> Result<Up, RpcError> {
    Up::from_name(s).ok_or_else(|| bad(format!("up_axis '{s}': use y or z")))
}

/// What the importer made of one part.
struct Prepared {
    name: String,
    color: Option<[u8; 4]>,
    /// The triangles that were read.
    triangles: usize,
    mesh: Mesh3d,
    /// The conversion, when it was asked for and the part fits.
    surface: Option<Converted>,
    refused: bool,
}

impl Session {
    /// A file next to the one being imported (a material file, a glTF buffer). Under the path
    /// policy of the session. Missing or not allowed gives `None`.
    fn sibling(
        &self,
        main: &Path,
        name: &str,
        exts: &[&str],
        checked: bool,
        max: u64,
    ) -> Option<Vec<u8>> {
        if name.contains("..")
            || name.starts_with('/')
            || name.starts_with('\\')
            || name.contains(':')
        {
            return None;
        }
        let p = main.parent()?.join(name);
        let p = if checked {
            self.check_path(&p, false, exts).ok()?
        } else {
            p
        };
        let len = std::fs::metadata(&p).ok()?.len();
        if len > max {
            return None;
        }
        std::fs::read(&p).ok()
    }

    pub(crate) fn import_mesh_file(
        &mut self,
        format: Format,
        path: PathBuf,
        options: &Value,
        checked: bool,
    ) -> Result<Value, RpcError> {
        let o: ImportOptions = match options {
            Value::Null => ImportOptions::default(),
            v => serde_json::from_value(v.clone()).map_err(|e| bad(format!("options: {e}")))?,
        };
        let gltf = matches!(format, Format::Gltf | Format::Glb);
        let step = format == Format::Step;
        if gltf && (o.unit.is_some() || o.up_axis.is_some()) {
            return Err(bad(
                "unit and up_axis: glTF is always in meters with Y up, leave them out",
            ));
        }
        if step && o.up_axis.is_some() {
            return Err(bad("up_axis: a STEP file has Z up, leave it out"));
        }
        if !step && (o.per_product.is_some() || o.keep_exact.is_some() || o.angle.is_some()) {
            return Err(bad(
                "per_product, keep_exact and angle are options of STEP files only",
            ));
        }
        let mut frame: Frame = format.default_frame();
        // The unit of a STEP file is a choice that replaces the unit that the file names.
        let step_unit = match (&o.unit, step) {
            (Some(u), true) => Some(unit_of(u)?),
            _ => None,
        };
        if !step && let Some(u) = &o.unit {
            frame.unit = unit_of(u)?;
        }
        if let Some(u) = &o.up_axis {
            frame.up = up_of(u)?;
        }
        if step
            && let Some(t) = o.tolerance
            && !(t.is_finite() && (0.001..=1000.0).contains(&t))
        {
            return Err(bad("tolerance: mm, 0.001 to 1000"));
        }
        if step
            && let Some(a) = o.angle
            && !(a.is_finite() && (1.0..=90.0).contains(&a))
        {
            return Err(bad("angle: degrees, 1 to 90"));
        }
        // For STEP the tolerance is that of the tessellation, not the weld distance.
        let ask = SurfaceAsk::of(
            o.to_surface,
            o.soften_below_angle,
            if step { None } else { o.tolerance },
            o.merge,
        )?;
        let path = if checked {
            self.check_path(&path, false, extensions(format))
                .map_err(RpcError::policy)?
        } else {
            path
        };
        let limits = Limits {
            max_bytes: o
                .max_bytes
                .unwrap_or(Limits::default().max_bytes)
                .min(self.limits.import_text_bytes),
            max_triangles: o
                .max_triangles
                .unwrap_or(mesh3d::MAX_TRIANGLES as u64)
                .min(mesh3d::MAX_TRIANGLES as u64),
            max_vertices: o
                .max_vertices
                .unwrap_or(mesh3d::MAX_VERTICES as u64)
                .min(mesh3d::MAX_VERTICES as u64),
            ..Limits::default()
        };
        // A STEP file takes about 18 times its size in memory: its limit is lower.
        let limits = if step {
            Limits {
                max_bytes: limits.max_bytes.min(kadakas_step::DEFAULT_MAX_BYTES),
                ..limits
            }
        } else {
            limits
        };
        let size = std::fs::metadata(&path)
            .map_err(|e| RpcError::invalid_params(format!("{}: {e}", path.display())))?
            .len();
        // The size is checked before the file is read.
        if size > limits.max_bytes {
            return Err(mesh_rpc(&MeshError::FileTooLarge {
                bytes: size,
                limit: limits.max_bytes,
            }));
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| RpcError::invalid_params(format!("{}: {e}", path.display())))?;
        let side_max = limits.max_bytes;
        let mut step_report: Option<Value> = None;
        let mut exacts: Vec<Option<ExactSolid>> = Vec::new();
        let mut curves: Vec<kadakas_step::CurvePart> = Vec::new();
        let mut set: MeshSet = match format {
            Format::Step => {
                let sopts = kadakas_step::StepOptions {
                    unit: step_unit,
                    tolerance_mm: o.tolerance,
                    angle_deg: o.angle.unwrap_or(15.0),
                    per_product: o.per_product.unwrap_or(true),
                    keep_exact: o.keep_exact.unwrap_or(true),
                    limits,
                    step: kadakas_step::StepLimits::default(),
                };
                kadakas_step::read(&bytes, &sopts).map(|imp| {
                    step_report = Some(imp.report.to_json());
                    exacts = imp.exact;
                    curves = imp.curves;
                    imp.set
                })
            }
            Format::Obj => {
                let loader = |name: &str| self.sibling(&path, name, &["mtl"], checked, 16 << 20);
                kadakas_meshio::obj::read(
                    &bytes,
                    &kadakas_meshio::obj::ObjOptions { frame, limits },
                    &loader,
                )
            }
            Format::Stl => kadakas_meshio::stl::read(
                &bytes,
                &kadakas_meshio::stl::StlOptions { frame, limits },
            ),
            Format::Gltf | Format::Glb => {
                let loader = |name: &str| self.sibling(&path, name, &["bin"], checked, side_max);
                kadakas_meshio::gltf::read(
                    &bytes,
                    &kadakas_meshio::gltf::GltfOptions { limits },
                    &loader,
                )
            }
        }
        .map_err(|e| mesh_rpc(&e))?;
        drop(bytes);
        // A file that Kadakas wrote says where its origin was in the model: the model returns to
        // its place.
        let mut origin_note = None;
        if let Some(d) = set.origin_mm.filter(|d| d.iter().any(|x| *x != 0.0))
            && o.restore_origin.unwrap_or(true)
        {
            set.move_by(d);
            origin_note = Some(format!(
                "the file says that its origin was at ({}, {}, {}) mm in the model: the model is back at its place",
                d[0], d[1], d[2]
            ));
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mesh")
            .to_string();
        // The exact data of each part (STEP). A joined mesh joins the exact data too.
        let mut part_exact: Vec<Option<ExactSolid>> = if exacts.len() == set.parts.len() {
            exacts
        } else {
            vec![None; set.parts.len()]
        };
        let mut join_note = None;
        let mut join_without: Vec<Value> = Vec::new();
        let mut parts = if o.join && !set.parts.is_empty() {
            let total: usize = part_exact.iter().flatten().map(|e| e.approx_bytes()).sum();
            let merged = if total > exact::MAX_OBJECT_BYTES {
                // One mesh cannot hold more exact data than one object may: the triangles stay.
                join_without.push(json!({
                    "part": stem,
                    "reason": format!(
                        "the exact data of the joined parts is about {total} bytes, over the limit of {} bytes for one object: only the triangles are kept",
                        exact::MAX_OBJECT_BYTES
                    ),
                    "approx_bytes": total,
                }));
                None
            } else if part_exact.iter().all(Option::is_some) {
                let mut it = part_exact.drain(..).flatten();
                it.next().map(|mut first| {
                    for e in it {
                        first.merge(&e);
                    }
                    first
                })
            } else {
                if part_exact.iter().any(Option::is_some) {
                    join_note = Some(
                        "the exact data was dropped: some parts that were joined have none"
                            .to_string(),
                    );
                }
                None
            };
            part_exact = vec![merged];
            vec![set.joined()]
        } else {
            std::mem::take(&mut set.parts)
        };
        let (lo, hi) = {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in parts.iter().flat_map(|p| p.positions.iter()) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            for p in curves.iter().flat_map(|c| c.points.iter()) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            (lo, hi)
        };
        let shift = if o.center {
            DVec3::new(-(lo[0] + hi[0]) / 2.0, -(lo[1] + hi[1]) / 2.0, -lo[2]).round()
        } else {
            DVec3::ZERO
        };
        let single = parts.len() == 1;
        let mut prepared: Vec<Prepared> = Vec::with_capacity(parts.len());
        // The exact data that the document holds already counts against the cap.
        let mut exact_total: usize = self
            .editor
            .doc()
            .objects
            .values()
            .filter_map(|o| match &o.shape {
                Shape::Mesh3d(m) => m.exact.as_ref().map(|e| e.approx_bytes()),
                _ => None,
            })
            .sum();
        let mut exact_over_cap: Vec<Value> = Vec::new();
        for (index, p) in parts.drain(..).enumerate() {
            let name = match (&o.name, single) {
                (Some(n), true) => n.clone(),
                (Some(n), false) => format!("{n} {}", p.name),
                (None, _) if p.name == "mesh" => stem.clone(),
                (None, _) => p.name.clone(),
            };
            let mesh = Mesh3d {
                name: name.clone(),
                vertices: p
                    .positions
                    .iter()
                    .map(|v| DVec3::from(*v) + shift)
                    .collect(),
                triangles: p.triangles,
                color: p.color.map(Color),
                exact: None,
                exact_unusable: None,
            };
            let mut mesh = mesh;
            if let Some(Some(x)) = part_exact.get_mut(index).map(Option::take) {
                let bytes = x.approx_bytes();
                if exact_total + bytes > exact::MAX_DOCUMENT_BYTES {
                    exact_over_cap.push(json!({
                        "part": name,
                        "reason": format!(
                            "the exact data of the document would pass the limit of {} bytes: only the triangles of this part are kept",
                            exact::MAX_DOCUMENT_BYTES
                        ),
                        "approx_bytes": bytes,
                    }));
                } else {
                    match x.transformed(&glam::DAffine3::from_translation(shift)) {
                        Ok(moved) => {
                            exact_total += bytes;
                            mesh.set_exact(moved);
                        }
                        Err(e) => exact_over_cap.push(json!({
                            "part": name,
                            "reason": format!(
                                "the exact data could not follow the move to the origin ({e}): only the triangles are kept"
                            ),
                            "approx_bytes": bytes,
                        })),
                    }
                }
            }
            if let Some(why) = mesh.problem() {
                return Err(RpcError::validation(format!("part '{name}': {why}"))
                    .with_reason("mesh_malformed"));
            }
            let triangles = mesh.triangles.len();
            prepared.push(Prepared {
                name,
                color: p.color,
                triangles,
                mesh,
                surface: None,
                refused: false,
            });
        }
        // The conversion into surface models is made before anything changes, so that a preview
        // and an import give the same counts.
        let mut to_surface = None;
        let mut softened_total = 0usize;
        if let Some(ask) = &ask {
            let mut tally = Tally::new(ask.opts.soften_below_angle, prepared.len());
            for p in &mut prepared {
                match surface_from_triangles(&p.mesh.vertices, &p.mesh.triangles, &ask.opts) {
                    Ok(c) => {
                        tally.ok(&p.name, &c);
                        p.surface = Some(c);
                    }
                    Err(e) => {
                        tally.refused(&p.name, None, &e);
                        p.refused = true;
                    }
                }
            }
            softened_total = tally.softened();
            to_surface = Some(tally.json());
        }
        let mut notes = set.notes.clone();
        notes.extend(origin_note);
        notes.extend(join_note);
        // A part that became a surface model has no triangles to match the exact data.
        let lost_to_surface = prepared
            .iter()
            .filter(|p| p.surface.is_some() && p.mesh.exact.is_some())
            .count();
        if lost_to_surface > 0 {
            notes.push(format!(
                "{lost_to_surface} parts became surface models: their exact data is not kept (a surface model has faces of its own)"
            ));
        }
        if let Some(ask) = &ask
            && prepared.iter().any(|p| p.refused)
        {
            notes.push(format!(
                "{} parts are over the limits of a surface model and stay meshes{}",
                prepared.iter().filter(|p| p.refused).count(),
                if ask.opts.soften_below_angle.is_some() {
                    ", with no edges softened"
                } else {
                    ""
                }
            ));
        }
        if shift != DVec3::ZERO {
            notes.push(format!(
                "the model was moved by ({}, {}, {}) mm to the origin",
                shift.x, shift.y, shift.z
            ));
        }
        let total_tris: usize = prepared.iter().map(|p| p.mesh.triangles.len()).sum();
        let total_verts: usize = prepared.iter().map(|p| p.mesh.vertices.len()).sum();
        let (blo, bhi) = (
            [lo[0] + shift.x, lo[1] + shift.y, lo[2] + shift.z],
            [hi[0] + shift.x, hi[1] + shift.y, hi[2] + shift.z],
        );
        let mut report = json!({
            "format": format.name(),
            "bytes": size,
            "unit": frame.unit.id(),
            "up_axis": frame.up.id(),
            "parts": prepared.len(),
            "vertices": total_verts,
            "triangles": total_tris,
            "degenerate_triangles": set.totals.degenerate,
            "bounds_mm": {
                "min": blo,
                "max": bhi,
                "size": [bhi[0] - blo[0], bhi[1] - blo[1], bhi[2] - blo[2]],
            },
            "notes": notes,
        });
        if let Some(t) = &to_surface {
            report["to_surface"] = t.clone();
        }
        if let Some(mut sr) = step_report {
            // The exact data: what the file gave, what the document cap took away.
            let kept = prepared
                .iter()
                .filter(|p| p.surface.is_none() && p.mesh.exact.is_some())
                .count();
            let bytes: usize = prepared
                .iter()
                .filter(|p| p.surface.is_none())
                .filter_map(|p| p.mesh.exact.as_ref().map(|e| e.approx_bytes()))
                .sum();
            let mut without = sr["exact"]["without"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            without.extend(exact_over_cap);
            without.extend(join_without);
            report["exact"] = json!({
                "enabled": o.keep_exact.unwrap_or(true),
                "kept_objects": kept,
                "approx_bytes": bytes,
                "lost_to_surface": lost_to_surface,
                "without": without,
            });
            sr.as_object_mut().map(|m| m.remove("exact"));
            report["step"] = sr;
            report["curves"] = json!(curves.len());
        }
        if o.preview {
            report["preview"] = json!(true);
            report["objects"] = Value::Array(
                prepared
                    .iter()
                    .take(LISTED_PARTS)
                    .map(|p| part_json(None, p))
                    .collect(),
            );
            return Ok(json!({
                "imported": 0,
                "skipped": 0,
                "softened_edges": softened_total,
                "preview": true,
                "report": report,
                "georeference": self.editor.doc().georef,
            }));
        }
        // The change: the layer, then one object for each part, in one undo step.
        let layer_name = o.layer.clone();
        let mut listed: Vec<(usize, ObjectId)> = Vec::new();
        let mut curves_added = 0usize;
        let outcome = self.editor.execute_stepwise(|step| {
            let lid = match layer_name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
            {
                None => step.doc().active_layer,
                Some(n) => {
                    let found = step
                        .doc()
                        .layers
                        .iter()
                        .find(|l| {
                            l.kind == LayerKind::Design
                                && l.name.eq_ignore_ascii_case(n)
                                && !l.locked
                        })
                        .map(|l| l.id);
                    match found {
                        Some(id) => id,
                        None => {
                            let before = step.created().len();
                            step.run(&Command::AddLayer {
                                name: n.to_string(),
                                elevation: 0.0,
                                scale: None,
                                kind: None,
                            })?;
                            // The layer that the command made, not a layer of another kind that
                            // has the same name.
                            step.created()
                                .get(before)
                                .map(|u| kadakas_core::model::LayerId(*u))
                                .ok_or_else(|| {
                                    CommandError::Invalid("the layer was not made".into())
                                })?
                        }
                    }
                }
            };
            let elevation = step.doc().layer(lid).map_or(0.0, |l| l.elevation);
            for (i, p) in prepared.iter_mut().enumerate() {
                let (shape, attrs) = match p.surface.take() {
                    Some(c) => {
                        let Converted {
                            mut model, origin, ..
                        } = c;
                        model.transform = glam::DAffine3::from_translation(DVec3::new(
                            origin.x,
                            origin.y,
                            origin.z - elevation,
                        ));
                        model.name = p.name.clone();
                        (
                            Shape::SurfaceModel(Box::new(model)),
                            Attributes {
                                fill: p.color.map(Color),
                                ..Default::default()
                            },
                        )
                    }
                    None => (
                        Shape::Mesh3d(Box::new(std::mem::take(&mut p.mesh))),
                        Attributes::default(),
                    ),
                };
                let before = step.created().len();
                step.run(&Command::AddObject {
                    shape,
                    layer: Some(lid),
                    class: None,
                    attrs,
                })?;
                if let Some(id) = step.created().get(before) {
                    listed.push((i, ObjectId(*id)));
                }
            }
            // The lines and polylines of a STEP file are 3D polylines.
            for c in &curves {
                let points: Vec<DVec3> = c.points.iter().map(|p| DVec3::from(*p) + shift).collect();
                if points.len() < 2 {
                    continue;
                }
                step.run(&Command::AddObject {
                    shape: Shape::Polyline3d {
                        points,
                        closed: c.closed,
                    },
                    layer: Some(lid),
                    class: None,
                    attrs: Attributes {
                        pen: c.color.map(Color),
                        ..Default::default()
                    },
                })?;
                curves_added += 1;
            }
            Ok(())
        });
        let outcome = outcome.map_err(RpcError::from)?;
        self.prune_selection();
        let doc = self.editor.doc();
        report["objects"] = Value::Array(
            listed
                .iter()
                .take(LISTED_PARTS)
                .map(|(i, id)| object_json(doc, *id, &prepared[*i]))
                .collect(),
        );
        if listed.len() > LISTED_PARTS {
            report["objects_listed"] = json!(LISTED_PARTS);
        }
        report["created"] = json!(outcome.created.len());
        Ok(json!({
            "imported": listed.len() + curves_added,
            "skipped": 0,
            "softened_edges": softened_total,
            "report": report,
            "georeference": doc.georef,
        }))
    }
}

fn part_json(id: Option<ObjectId>, p: &Prepared) -> Value {
    let (kind, vertices, faces) = match &p.surface {
        Some(c) => ("surface_model", c.report.vertices, Some(c.report.faces)),
        None => ("mesh3d", p.mesh.vertices.len(), None),
    };
    json!({
        "id": id,
        "name": p.name,
        "kind": kind,
        "vertices": vertices,
        "triangles": p.triangles,
        "faces": faces,
        "color": p.color,
    })
}

fn object_json(doc: &Document, id: ObjectId, p: &Prepared) -> Value {
    let kind = doc.object(id).map_or("", |o| o.shape.kind());
    let (vertices, faces) = match doc.object(id).map(|o| &o.shape) {
        Some(Shape::SurfaceModel(m)) => (m.vertex_count(), Some(m.face_count())),
        Some(Shape::Mesh3d(m)) => (m.vertices.len(), None),
        _ => (0, None),
    };
    json!({
        "id": id,
        "name": p.name,
        "kind": kind,
        "vertices": vertices,
        "triangles": p.triangles,
        "faces": faces,
        "color": p.color,
    })
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct StlOptions {
    encoding: Option<String>,
    units: Option<String>,
    y_up: Option<bool>,
    objects: Option<Vec<ObjectId>>,
    plant_detail: Option<kadakas_x3d::PlantDetail>,
    name: Option<String>,
}

impl Session {
    /// `export.file` with the format `stl`.
    pub(crate) fn export_stl_file(
        &mut self,
        path: PathBuf,
        options: &Value,
        overwrite: bool,
        checked: bool,
    ) -> Result<Value, RpcError> {
        use kadakas_core::plants::form3d::Detail;
        use kadakas_x3d::PlantDetail;
        let o: StlOptions = match options {
            Value::Null => StlOptions::default(),
            v => serde_json::from_value(v.clone()).map_err(|e| bad(format!("options: {e}")))?,
        };
        let ascii = match o.encoding.as_deref().map(str::trim) {
            None | Some("binary") => false,
            Some("ascii") => true,
            Some(other) => {
                return Err(bad(format!("encoding '{other}': use binary or ascii")));
            }
        };
        let units = match o.units.as_deref().map(str::trim) {
            None | Some("mm") => "mm",
            Some("m") => "m",
            Some(other) => return Err(bad(format!("units '{other}': use mm or m"))),
        };
        let y_up = o.y_up.unwrap_or(false);
        let path = if checked {
            self.check_path(&path, true, &["stl"])
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
        let detail = o.plant_detail.unwrap_or(PlantDetail::Fine);
        let opts: kadakas_x3d::ExportOptions = serde_json::from_value(json!({
            "units": units,
            "y_up": y_up,
            "georef": false,
            "instancing": false,
            "data": false,
            "lines": false,
            "objects": o.objects,
            "plant_detail": detail,
        }))
        .map_err(|e| bad(format!("options: {e}")))?;
        let scene = match detail {
            PlantDetail::Auto => self.scene(),
            PlantDetail::Fine => self.scene_with_detail(Detail::Fine),
            PlantDetail::Coarse => self.scene_with_detail(Detail::Coarse),
            PlantDetail::Billboard => self.scene_with_detail(Detail::Billboard),
        };
        let doc = self.editor.doc();
        let model = kadakas_x3d::parts::build(doc, &scene, &opts, &kadakas_x3d::BaseColors)
            .map_err(|e| match e {
                kadakas_x3d::X3dError::Options(_) => bad(e),
                kadakas_x3d::X3dError::Empty => RpcError::validation(e),
                e => app(e),
            })?;
        // The coordinates of the model, not the local ones of the 3D scene: an STL file carries
        // no origin, so a file that is read again lands where the model was.
        let org = model.conv.point(model.origin);
        let exported: std::collections::HashSet<kadakas_core::uid::Uid> = model
            .parts
            .iter()
            .flat_map(|p| p.ids.iter().copied())
            .collect();
        let mut closed: std::collections::HashSet<kadakas_core::uid::Uid> =
            std::collections::HashSet::new();
        let mut open = 0usize;
        for obj in doc.objects.values() {
            if let Shape::SurfaceModel(m) = &obj.shape
                && exported.contains(&obj.id.0)
            {
                if m.check().is_closed() {
                    closed.insert(obj.id.0);
                } else {
                    open += 1;
                }
            }
        }
        let mut tris: Vec<[[f32; 3]; 3]> = Vec::new();
        let mut leaky = 0usize;
        for part in &model.parts {
            let first = tris.len();
            for prim in part.prims.iter().filter(|p| !p.lines) {
                let at = |i: u32| {
                    let p = prim.positions[i as usize];
                    [
                        (p[0] as f64 + part.anchor[0] + org[0]) as f32,
                        (p[1] as f64 + part.anchor[1] + org[1]) as f32,
                        (p[2] as f64 + part.anchor[2] + org[2]) as f32,
                    ]
                };
                for t in prim.indices.as_chunks::<3>().0 {
                    tris.push([at(t[0]), at(t[1]), at(t[2])]);
                }
            }
            // A closed solid must give a watertight mesh: every edge is used twice.
            if !part.ids.is_empty() && part.ids.iter().all(|id| closed.contains(id)) {
                let key = |p: [f32; 3]| p.map(|v| if v == 0.0 { 0u32 } else { v.to_bits() });
                let mut uses: std::collections::HashMap<([u32; 3], [u32; 3]), u32> =
                    std::collections::HashMap::new();
                for t in &tris[first..] {
                    for k in 0..3 {
                        let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
                        *uses.entry((a.min(b), a.max(b))).or_default() += 1;
                    }
                }
                if uses.values().any(|n| *n != 2) {
                    leaky += 1;
                }
            }
        }
        if tris.is_empty() {
            return Err(RpcError::validation(
                "the document has no 3D objects to export",
            ));
        }
        let name = o
            .name
            .clone()
            .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
            .unwrap_or_else(|| "kadakas".into());
        let count = tris.len();
        let data: Vec<u8> = if ascii {
            kadakas_meshio::stl::write_ascii(&name, tris.into_iter()).into_bytes()
        } else {
            kadakas_meshio::stl::write_binary(&name, tris.into_iter()).map_err(|e| mesh_rpc(&e))?
        };
        let mut notes: Vec<String> = Vec::new();
        notes.push("STL holds triangles only: colors, materials and lines are not written".into());
        notes.push(format!(
            "the coordinates are those of the model ({units}); the local origin of the 3D scene is added back"
        ));
        if open > 0 {
            notes.push(format!(
                "{open} surface models are not closed solids: a printer needs closed solids"
            ));
        }
        if leaky > 0 {
            notes.push(format!(
                "{leaky} closed solids did not give a watertight mesh: some edges are not shared by two triangles"
            ));
        }
        kadakas_sys::fsio::write_atomic(&path, &data)
            .map_err(|e| app(format!("{}: {e}", path.display())))?;
        let skipped: Vec<Value> = model
            .skipped
            .iter()
            .map(|(id, why)| json!({"id": id, "reason": why}))
            .collect();
        let objects: usize = model.parts.iter().map(|p| p.ids.len()).sum();
        Ok(json!({
            "path": path,
            "exported": objects,
            "report": {
                "triangles": count,
                "objects": objects,
                "bytes": data.len(),
                "encoding": if ascii { "ascii" } else { "binary" },
                "units": units,
                "y_up": y_up,
                "open_models": open,
                "leaky_models": leaky,
                "origin_mm": [model.origin.x, model.origin.y, model.origin.z],
                "skipped": skipped,
                "notes": notes,
            },
        }))
    }
}
