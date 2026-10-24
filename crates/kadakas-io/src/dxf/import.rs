//! DXF entities to Kadakas shapes.
//!
//! Supported: LINE, LWPOLYLINE, POLYLINE (2D and 3D), ARC, CIRCLE, ELLIPSE,
//! SPLINE, TEXT, MTEXT, ATTRIB, POINT, 3DFACE, SOLID, TRACE, HATCH, WIPEOUT, MESH, LEADER and INSERT
//! (exploded). DIMENSION and ACAD_TABLE are exploded from their anonymous block.
//! A bulge becomes an exact `PointOnArc` vertex at the middle of the arc.
//! Ellipses and splines become polylines, within the tolerance of the options.
//! Every entity is counted: it is imported, or it has a line in `report.skipped`.

use std::collections::{BTreeMap, HashMap};
use std::f64::consts::{PI, TAU};

use kadakas_core::DVec2;
use kadakas_core::DVec3;
use kadakas_core::curve::{Vertex, VertexKind, corners};
use kadakas_core::georef::Georeference;
use kadakas_core::mesh3d::Mesh3d;
use kadakas_core::model::{Attributes, ClassId, Color, LayerId, Shape};

mod approx;
mod clip;
mod extra;
mod hatch;
mod paper;
mod tilt;
mod tilted;

use approx::Tally;
use tilted::Plane;

use super::parse::{self, Dxf, Entity, LayerDef};
use super::source::{Path, Tracking};
use super::text::{clean_mtext, decode_percent, decode_text};
use super::{aci_rgb, true_color};
use super::{hash, revision};
use crate::BaseMode;
use crate::builder::Builder;
use crate::options::{LayerMode, OriginMode, PointMode};
use crate::{Context, ImportOptions, ImportResult, IoError, Source};
use kadakas_core::provenance::{self, FileLink};
use kadakas_core::resource::ResourceBody;

/// Most entities that one import visits, counting every copy of an exploded block.
const MAX_VISITS: usize = 5_000_000;
/// Deepest nesting of blocks.
const MAX_DEPTH: usize = 12;
/// Most copies that one array INSERT makes.
const MAX_ARRAY: i64 = 10_000;

pub fn import(
    src: &Source,
    ctx: &Context,
    opts: &ImportOptions,
    link: Option<&FileLink>,
) -> Result<ImportResult, IoError> {
    if src.data.len() as u64 > opts.max_bytes {
        return Err(IoError::Format(format!(
            "the DXF data is {} bytes, more than the limit of {} (option max_bytes)",
            src.data.len(),
            opts.max_bytes
        )));
    }
    let dxf = parse::parse(&src.data)?;
    import_parsed(&src.name, &dxf, ctx, opts, link)
}

/// Like [`import`], for a file that is parsed already (the caller checked the size). `name` is the
/// file name without extension, for the identity of a file that has no GUID.
pub(super) fn import_parsed(
    name: &str,
    dxf: &Dxf,
    ctx: &Context,
    opts: &ImportOptions,
    link: Option<&FileLink>,
) -> Result<ImportResult, IoError> {
    let mut b = Builder::new(ctx, "dxf", opts.max_objects);
    let release = dxf.release();
    b.report.release = release.as_ref().map(super::version::Release::label);
    let resolved = super::units::resolve(opts.unit, dxf, release.as_ref());
    if let Some(note) = &resolved.note {
        b.report.note(note);
    }
    if dxf.truncated {
        b.report
            .note("the file ends early: imported what could be read");
    }
    let scale = resolved.mm_per_unit;
    let mut tracking = opts.track_source.then(|| {
        let mut t = Tracking::new(opts.source_name.as_deref().unwrap_or(name), dxf);
        // An update names the drawing and the revision of the tags itself (IOP-23).
        if let Some(l) = &opts.lineage {
            t.drawing = l.drawing.clone();
            if let Some(r) = &l.revision {
                t.revision = r.clone();
            }
            t.headerless = false;
        }
        t
    });
    // What to keep for an update is decided now, before the walk: a drawing over the cap is
    // refused here and not imported without a word (IOP-22).
    let plan = match tracking.as_mut() {
        Some(t) => plan_base(
            opts.base,
            dxf,
            t,
            b.has_base(&t.drawing, &t.revision),
            link,
            (opts.base_caps, scale),
        )?,
        None => BasePlan::Off,
    };
    b.report.base = plan.name().map(str::to_string);
    plan.note(&mut b);
    let tracking = tracking.as_ref();
    b.report.drawing = tracking.map(|t| t.drawing.clone());
    b.report.revision = tracking.map(|t| t.revision.clone());
    if let Some(t) = tracking
        && t.headerless
    {
        b.report.note("the file has no header, so nothing says which drawing it is: it is a drawing of its own, and an update of another file with it asks you to confirm");
    }

    // Real-world drawings (survey grids in meters) are far from (0, 0). A probe pass finds the extent,
    // and the shift goes into the document georeference so that the geometry stays near the origin.
    // An update puts the objects where the old ones are: the shift of the old import, with no
    // search for a new one and no georeference of the file.
    if let Some(l) = &opts.lineage {
        b.set_offset(DVec2::from(l.shift));
        walk(dxf, opts, &mut b, scale, tracking)?;
        store_base(&mut b, plan);
        return Ok(b.finish());
    }
    let doc_has_georef = ctx.georef.is_some();
    // A GEODATA object with an EPSG code gives the georeference. The design point becomes the origin
    // of the model, and the grid north gives the rotation.
    if !doc_has_georef
        && opts.origin == OriginMode::Center
        && let Some(gd) = &dxf.geodata
        && let Some(crs) = gd.epsg()
        && gd.design.iter().chain(&gd.reference).all(|v| v.is_finite())
        && gd.reference.iter().all(|v| v.abs() < 1.0e9)
        && gd.design.iter().all(|v| (v * scale).abs() < 1.0e12)
    {
        b.set_offset(DVec2::from(gd.design) * scale);
        b.set_georef(Georeference {
            crs: crs.clone(),
            origin: DVec2::from(gd.reference),
            rotation: gd.north[0].atan2(gd.north[1]),
            user_origin: DVec2::ZERO,
        });
        b.report.georeferenced = true;
        b.report.note(&format!(
            "the file has a GEODATA object: the drawing is placed in {crs}"
        ));
        walk(dxf, opts, &mut b, scale, tracking)?;
        store_base(&mut b, plan);
        return Ok(b.finish());
    }
    if doc_has_georef || opts.origin == OriginMode::Center {
        let mut probe = Builder::new(ctx, "dxf", opts.max_objects);
        probe.set_probe();
        walk(dxf, opts, &mut probe, scale, tracking)?;
        if let Some((lo, hi)) = probe.bounds() {
            let center = (lo + hi) / 2.0;
            if center.is_finite() && center.abs().max_element() > FAR_MM {
                if let Some(g) = &ctx.georef {
                    b.set_offset(g.origin * 1000.0);
                    b.report.georeferenced = !g.crs.trim().is_empty();
                    b.report.note("the drawing is far from the origin: its coordinates are taken as map coordinates of the document");
                } else if opts.origin == OriginMode::Center && center.abs().max_element() <= 1.0e12
                {
                    // The georeference origin is in meters and must be below 1e9.
                    let shift = (center / 1000.0).round();
                    b.set_offset(shift * 1000.0);
                    b.set_georef(Georeference {
                        crs: String::new(),
                        origin: shift,
                        rotation: 0.0,
                        user_origin: DVec2::ZERO,
                    });
                    b.report.georeferenced = false;
                    b.report.note(&format!("the drawing is far from the origin: it is centered, the shift ({}, {}) m is saved in the document", shift.x, shift.y));
                }
            }
        }
    }
    walk(dxf, opts, &mut b, scale, tracking)?;
    store_base(&mut b, plan);
    Ok(b.finish())
}

/// What an import keeps for a later update.
#[derive(Clone)]
pub(super) enum BasePlan {
    /// No source tracking: nothing to decide.
    Off,
    /// The resources of the summaries (`base: "auto"`), made before the walk: one body for each
    /// part, and the key of their names.
    Summary(Vec<serde_json::Value>, String),
    /// The document keeps the summaries of this revision already.
    Existing,
    /// A link to the original file is on the layer source (`base: "link"`).
    Link,
    /// Nothing (`base: "none"`).
    Nothing,
    /// The summaries fail the checks of a base (odd names in a hand-made file): nothing is kept.
    Rejected,
}

impl BasePlan {
    /// The name in the report.
    fn name(&self) -> Option<&'static str> {
        match self {
            BasePlan::Off => None,
            BasePlan::Summary(..) | BasePlan::Existing => Some(BaseMode::Auto.name()),
            BasePlan::Link => Some(BaseMode::Link.name()),
            BasePlan::Nothing | BasePlan::Rejected => Some(BaseMode::Off.name()),
        }
    }

    /// Tells the person in the report what the update will use.
    fn note(&self, b: &mut Builder) {
        match self {
            BasePlan::Link => b.report.note("the entities are not kept in the document: an update reads the original file from its path and checks that it has not changed"),
            BasePlan::Nothing => b.report.note("no update data was kept (base: none): an update of this drawing will need the old file"),
            // A summary that fails the checks is not a question of size: it is dropped with a
            // note, as a bad base always was. The note names the other choice.
            BasePlan::Rejected => b.report.note("the entities have values that cannot be kept for an update: an update will need the old file. Import with the option base set to \"link\" to keep a link to the original file instead"),
            _ => {}
        }
    }
}

/// Most entities of all blocks and the model: what `revision::records` counts.
fn entity_count(dxf: &Dxf) -> usize {
    dxf.entities.len()
        + dxf
            .blocks
            .values()
            .map(|block| block.entities.len())
            .sum::<usize>()
}

/// Decides what to keep. `auto` keeps the summaries when they fit the limits (the number of
/// entities, and the size of the parts) and refuses above them, before anything is walked.
/// `scale` is the unit of the file for the bodies of the base.
pub(super) fn plan_base(
    mode: BaseMode,
    dxf: &Dxf,
    t: &mut Tracking,
    has_base: bool,
    link: Option<&FileLink>,
    (caps, scale): (crate::BaseCaps, f64),
) -> Result<BasePlan, IoError> {
    match mode {
        BaseMode::Off => Ok(BasePlan::Nothing),
        BaseMode::Link => {
            let link = link.ok_or_else(|| {
                IoError::Option(
                    "base \"link\" needs a file on disk: the importer did not get one".into(),
                )
            })?;
            t.link = Some(link.clone());
            Ok(BasePlan::Link)
        }
        BaseMode::Auto if has_base => Ok(BasePlan::Existing),
        BaseMode::Auto => {
            let (recs, cut) = revision::records(dxf, caps.records);
            if cut {
                return Err(IoError::BaseChoice {
                    entities: entity_count(dxf),
                    limit: crate::BaseLimit::TooMany { cap: caps.records },
                });
            }
            summary_plan(recs, t, caps, scale)
        }
    }
}

/// The summaries of `recs` as the bodies of a base, split into parts that fit a frame of the
/// collaboration. The records are compacted here.
pub(super) fn summary_plan(
    mut recs: Vec<revision::Rec>,
    t: &Tracking,
    caps: crate::BaseCaps,
    scale: f64,
) -> Result<BasePlan, IoError> {
    {
        if recs.len() > caps.records {
            return Err(IoError::BaseChoice {
                entities: recs.len(),
                limit: crate::BaseLimit::TooMany { cap: caps.records },
            });
        }
        recs.iter_mut().for_each(revision::Rec::compact);
        // Split into parts of about `part_bytes`, so that each part is a blob of the
        // collaboration that fits in a frame.
        let mut parts: Vec<Vec<serde_json::Value>> = vec![vec![]];
        let mut bytes = 0usize;
        let mut total = 0u64;
        for r in &recs {
            let v = r.to_value();
            let n = v.to_string().len();
            total += n as u64;
            if bytes + n > caps.part_bytes && !parts.last().is_some_and(Vec::is_empty) {
                parts.push(vec![]);
                bytes = 0;
            }
            parts.last_mut().expect("a part").push(v);
            bytes += n;
        }
        if parts.len() as u64 > caps.parts {
            return Err(IoError::BaseChoice {
                entities: recs.len(),
                limit: crate::BaseLimit::TooLarge {
                    bytes: total,
                    cap: caps.part_bytes as u64 * caps.parts,
                },
            });
        }
        let count = parts.len();
        let bodies: Vec<serde_json::Value> = parts
            .into_iter()
            .enumerate()
            .map(|(n, recs)| {
                let mut body = serde_json::json!({
                    "type": provenance::BASE_TYPE,
                    "drawing": t.drawing,
                    "revision": t.revision,
                    "unit_scale": scale,
                    "recs": recs,
                });
                if count > 1 {
                    body["part"] = n.into();
                    body["parts"] = count.into();
                }
                body
            })
            .collect();
        if bodies.iter().any(|v| provenance::check_base(v).is_err()) {
            return Ok(BasePlan::Rejected);
        }
        Ok(BasePlan::Summary(
            bodies,
            hash::hex(hash::content_hash_of(&t.drawing, &t.revision)),
        ))
    }
}

/// Keeps the summaries of the entities of this revision in the document (resources of type
/// `import_base`), so that an update can diff the old file against the new one without the old
/// file. A revision that the document keeps is not stored again (the plan is `Existing`).
pub(super) fn store_base(b: &mut Builder, plan: BasePlan) {
    let BasePlan::Summary(bodies, key) = plan else {
        return;
    };
    if b.is_probe() {
        return;
    }
    let count = bodies.len();
    for (n, body) in bodies.into_iter().enumerate() {
        let name = if count > 1 {
            format!("import base {key} part {} of {count}", n + 1)
        } else {
            format!("import base {key}")
        };
        b.resource(&name, ResourceBody::ImportBase(std::sync::Arc::new(body)));
    }
}

/// A drawing whose center is farther from (0, 0) than this (mm) is shifted. This is 1 km.
const FAR_MM: f64 = 1_000_000.0;

/// Converts all entities of the drawing into `b`.
fn walk(
    dxf: &Dxf,
    opts: &ImportOptions,
    b: &mut Builder,
    scale: f64,
    tracking: Option<&Tracking>,
) -> Result<(), IoError> {
    // A probe stores nothing, so it needs no tags and no source layers.
    let tracking = tracking.filter(|_| !b.is_probe());
    let xform = (scale, [b.offset().x, b.offset().y]);
    if let Some(t) = tracking
        && b.knows_drawing(&t.drawing)
    {
        b.report.note("this drawing was imported before: use Update to bring in its changes (the objects of this import are added next to the old ones)");
    }
    // With source tracking, the objects of a file go to a layer of their own, named after the
    // file, unless the caller names a layer. In `layers` mode each DXF layer gets its own layer.
    let target_layer = match (&opts.layer, tracking, opts.layer_mode) {
        (Some(name), ..) => b.layer(name, true),
        (None, Some(t), LayerMode::Classes) => b.source_layer(
            &t.name,
            true,
            &t.layer_source("", xform.0, xform.1),
            &t.name,
        ),
        _ => b.default_layer,
    };
    let hatches = b.hatches.clone();
    let mut w = Walker {
        dxf,
        opts,
        b,
        tracking,
        xform,
        defs: dxf.layers.iter().map(|l| (l.name.as_str(), l)).collect(),
        mapped: HashMap::new(),
        target_layer,
        visits: 0,
        exploded: 0,
        overflow: false,
        faces: Default::default(),
        repeated_closed: 0,
        repeated_gap_mm: 0.0,
        tally: Tally::default(),
        sheet: None,
        class_vis: HashMap::new(),
        hatches: hatch::HatchLib::new(&hatches),
        object_index: clip::handle_index(&dxf.objects),
    };
    let root = Cx {
        xf: Xf::scale(scale),
        layer: None,
        pen: None,
        weight: None,
        depth: 0,
        path: Path::default(),
        clips: Vec::new(),
        tilt: None,
    };
    // Paper space has its own pass; with the option off, or in an update, it is skipped and counted.
    let paper = opts.paper_space && opts.lineage.is_none() && !w.b.is_probe();
    for e in &dxf.entities {
        if paper && e.int(67) == Some(1) {
            continue;
        }
        w.entity(e, &root);
    }
    w.flush_faces();
    if w.repeated_closed > 0 {
        let note = format!(
            "{} {} (largest gap {:.3e} mm)",
            w.repeated_closed, REPEATED_NOTE, w.repeated_gap_mm
        );
        w.b.report.note(&note);
    }
    // The layouts of paper space: sheet layers. Without the pass, their entities are counted.
    if paper {
        w.paper_space();
    } else {
        w.skip_paper_space();
    }
    for note in w.tally.notes(opts.tolerance) {
        w.b.report.note(&note);
    }
    if w.overflow {
        return Err(IoError::Format(format!(
            "the blocks make more than {} objects (nested or array INSERTs): raise the max_exploded option or turn off explode_blocks",
            opts.max_exploded
        )));
    }
    if w.visits >= MAX_VISITS {
        w.b.report
            .note("import stopped: too many entities (a block may contain itself)");
    }
    Ok(())
}

/// A 2D transform with a scale and offset for Z.
#[derive(Clone, Copy, Debug)]
struct Xf {
    /// Columns are the images of the X and Y axes.
    cols: [DVec2; 2],
    t: DVec2,
    zs: f64,
    zt: f64,
}

impl Xf {
    fn scale(s: f64) -> Xf {
        Xf {
            cols: [DVec2::new(s, 0.0), DVec2::new(0.0, s)],
            t: DVec2::ZERO,
            zs: s,
            zt: 0.0,
        }
    }

    fn apply(&self, p: DVec2) -> DVec2 {
        self.cols[0] * p.x + self.cols[1] * p.y + self.t
    }

    fn z(&self, z: f64) -> f64 {
        self.zs * z + self.zt
    }

    /// First `inner`, then `self`.
    fn after(&self, inner: &Xf) -> Xf {
        Xf {
            cols: [
                self.apply(inner.cols[0]) - self.t,
                self.apply(inner.cols[1]) - self.t,
            ],
            t: self.apply(inner.t),
            zs: self.zs * inner.zs,
            zt: self.zs * inner.zt + self.zt,
        }
    }

    fn det(&self) -> f64 {
        self.cols[0].perp_dot(self.cols[1])
    }

    /// True if the transform keeps circles round (rotation, mirror, uniform scale).
    fn is_similarity(&self) -> bool {
        let (a, b) = (self.cols[0], self.cols[1]);
        let scale = a.length().max(b.length()).max(1e-300);
        (a.length() - b.length()).abs() <= 1e-9 * scale && a.dot(b).abs() <= 1e-9 * scale * scale
    }

    fn length_scale(&self) -> f64 {
        self.cols[0].length()
    }
}

/// What a nested INSERT passes to its content.
#[derive(Clone)]
struct Cx {
    xf: Xf,
    /// Layer of the INSERT: entities on layer "0" inside a block take it.
    layer: Option<String>,
    /// Color and line weight that BYBLOCK entities take.
    pen: Option<Color>,
    weight: Option<f64>,
    depth: usize,
    /// The block and the INSERTs that led here, for the source tags.
    path: Path,
    /// The clips (XCLIP) of the block references that led here, in the drawing (IOP-27).
    clips: Vec<std::rc::Rc<clip::Region>>,
    /// The tilt of the block references in a tilted plane that led here (IOP-27).
    tilt: tilt::Chain,
}

#[derive(Clone)]
struct Item {
    shape: Shape,
    fill: Option<Color>,
    /// The fill is the pen color of the entity (a SOLID, a solid HATCH).
    fill_pen: bool,
    /// The hatch fill of a pattern hatch (IOP-27).
    pattern: Option<kadakas_core::resource::ResourceId>,
    /// The height of a flat shape in a tilted plane, before the tilt (IOP-27). Without one the shape is at the height of its block.
    z: Option<f64>,
    /// The hatch that this loop belongs to: the number of its outer loop. An island carries the number of the outer loop that holds it (IOP-27).
    group: Option<u32>,
    /// The tilt of the plane of the entity itself (a text in a tilted plane), applied before the tilt of the blocks around.
    tilt: Option<kadakas_core::DAffine3>,
    /// A pen that replaces the one of the entity (a wipeout has none).
    pen: Option<Color>,
    /// Object data. Text keeps its rotation here (`rotation_deg`), because a text shape has none.
    data: BTreeMap<String, serde_json::Value>,
}

impl Item {
    fn new(shape: Shape) -> Item {
        Item {
            shape,
            fill: None,
            fill_pen: false,
            pattern: None,
            z: None,
            group: None,
            tilt: None,
            pen: None,
            data: BTreeMap::new(),
        }
    }
}

type Skip = &'static str;

struct Walker<'a> {
    dxf: &'a Dxf,
    opts: &'a ImportOptions,
    b: &'a mut Builder,
    tracking: Option<&'a Tracking>,
    /// Millimeters per unit, and the shift in mm, for the source of layers.
    xform: (f64, [f64; 2]),
    defs: HashMap<&'a str, &'a LayerDef>,
    /// DXF layer name to (design layer, class, attributes that the layer gives its objects).
    mapped: HashMap<String, (LayerId, ClassId, Attributes)>,
    target_layer: LayerId,
    visits: usize,
    /// Entities visited inside blocks. The import fails above `max_exploded`.
    exploded: usize,
    overflow: bool,
    /// 3DFACE meshes by DXF layer name (option `faces: mesh`).
    faces: std::collections::BTreeMap<String, FaceMesh>,
    /// Open polylines that were closed because they end where they start, and the largest gap
    /// that this closed, in mm.
    repeated_closed: usize,
    repeated_gap_mm: f64,
    /// What was approximated or changed, for the notes of the report.
    tally: Tally,
    /// The sheet layer that receives the objects, while a layout of paper space is imported.
    sheet: Option<LayerId>,
    /// The classes that the import made, by DXF layer name, and whether the layer shows.
    class_vis: HashMap<String, (ClassId, bool)>,
    /// The hatch fills of the document and of this import (IOP-27).
    hatches: hatch::HatchLib,
    /// The objects of the file by handle, for the clips of block references (IOP-27).
    object_index: HashMap<String, usize>,
}

/// Vertices and triangles of the 3D faces of one layer.
#[derive(Default)]
struct FaceMesh {
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    index: HashMap<[u64; 3], u32>,
}

impl FaceMesh {
    fn vertex(&mut self, p: DVec3) -> u32 {
        let key = [
            (p.x + 0.0).to_bits(),
            (p.y + 0.0).to_bits(),
            (p.z + 0.0).to_bits(),
        ];
        *self.index.entry(key).or_insert_with(|| {
            self.vertices.push(p);
            (self.vertices.len() - 1) as u32
        })
    }

    /// Adds a face of 3 or 4 corners as one or two triangles.
    fn face(&mut self, pts: &[DVec3]) {
        let ids: Vec<u32> = pts.iter().map(|p| self.vertex(*p)).collect();
        if ids.len() >= 3 {
            self.triangles.push([ids[0], ids[1], ids[2]]);
        }
        if ids.len() == 4 {
            self.triangles.push([ids[0], ids[2], ids[3]]);
        }
    }
}

/// A mesh from a polyface mesh (POLYLINE flag 64): vertices with flag 192 and face records with
/// flag 128 that name them by 1-based index. Faces of 4 corners become 2 triangles.
fn polyface_mesh(e: &Entity, xf: &Xf) -> Result<Mesh3d, Skip> {
    let mut vertices: Vec<DVec3> = Vec::new();
    for v in &e.children {
        if v.int(70).unwrap_or(0) & 64 != 0 {
            let p = DVec3::new(v.fd(10, 0.0), v.fd(20, 0.0), v.fd(30, 0.0));
            vertices.push(xf.apply(p.truncate()).extend(xf.z(p.z)));
        }
    }
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for v in &e.children {
        let flags = v.int(70).unwrap_or(0);
        if flags & 128 == 0 || flags & 64 != 0 {
            continue;
        }
        // A negative index marks an invisible edge. Zero ends the list.
        let idx: Vec<usize> = (71..=74)
            .map_while(|c| v.int(c))
            .take_while(|i| *i != 0)
            .map(|i| i.unsigned_abs() as usize)
            .collect();
        if idx.len() < 3 || idx.iter().any(|i| *i > vertices.len()) {
            continue;
        }
        let at = |k: usize| (idx[k] - 1) as u32;
        triangles.push([at(0), at(1), at(2)]);
        if idx.len() == 4 && idx[3] != idx[2] {
            triangles.push([at(0), at(2), at(3)]);
        }
    }
    if triangles.is_empty() {
        return Err("polyface mesh without faces");
    }
    Ok(Mesh3d {
        name: String::new(),
        vertices,
        triangles,
        color: None,
        exact: None,
    })
}

/// A mesh from a polygon mesh (POLYLINE flag 16): an M by N grid of vertices (codes 71 and 72).
/// Flags 1 and 32 close the mesh in the M and N directions.
fn grid_mesh(e: &Entity, xf: &Xf) -> Result<Mesh3d, Skip> {
    let (mc, nc) = (
        e.int(71).unwrap_or(0).max(0) as usize,
        e.int(72).unwrap_or(0).max(0) as usize,
    );
    let flags = e.int(70).unwrap_or(0);
    let verts: Vec<DVec3> = e
        .children
        .iter()
        .filter(|v| v.int(70).unwrap_or(0) & 128 == 0)
        .map(|v| {
            let p = DVec3::new(v.fd(10, 0.0), v.fd(20, 0.0), v.fd(30, 0.0));
            xf.apply(p.truncate()).extend(xf.z(p.z))
        })
        .collect();
    if mc < 2 || nc < 2 || verts.len() != mc * nc || mc * nc > 1_000_000 {
        return Err("polygon mesh with a wrong vertex count");
    }
    let at = |i: usize, j: usize| (i % mc) * nc + (j % nc);
    let (im, jn) = (
        if flags & 1 != 0 { mc } else { mc - 1 },
        if flags & 32 != 0 { nc } else { nc - 1 },
    );
    let mut triangles = Vec::new();
    for i in 0..im {
        for j in 0..jn {
            let (a, b, c, d) = (
                at(i, j) as u32,
                at(i + 1, j) as u32,
                at(i + 1, j + 1) as u32,
                at(i, j + 1) as u32,
            );
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    Ok(Mesh3d {
        name: String::new(),
        vertices: verts,
        triangles,
        color: None,
        exact: None,
    })
}

fn layer_pen(def: &LayerDef) -> Option<Color> {
    if let Some(t) = def.true_color {
        return Some(true_color(t));
    }
    let i = def.color.unsigned_abs();
    u8::try_from(i)
        .ok()
        .and_then(aci_rgb)
        .map(|[r, g, b]| Color([r, g, b, 255]))
}

fn weight_mm(v: i64) -> Option<f64> {
    (v >= 0).then(|| v as f64 / 100.0)
}

impl Walker<'_> {
    /// The design layer, class and layer attributes for a DXF layer name.
    fn target(&mut self, name: &str) -> (LayerId, ClassId, Attributes) {
        if let Some(t) = self.mapped.get(name) {
            return t.clone();
        }
        let def = self.defs.get(name).copied();
        let visible = def.is_none_or(|d| d.color >= 0 && !d.frozen);
        let pen = def.and_then(layer_pen);
        let weight = def.and_then(|d| weight_mm(d.lineweight));
        let result = match self.opts.layer_mode {
            LayerMode::Classes => {
                let (class, defaults) = self.class_for(name);
                (self.target_layer, class, defaults)
            }
            LayerMode::Layers => {
                let shown = decode_percent(name);
                let layer = match self.tracking {
                    Some(t) => {
                        let src = t.layer_source(name, self.xform.0, self.xform.1);
                        self.b.source_layer(&shown, visible, &src, &t.name)
                    }
                    None => self.b.layer(&shown, visible),
                };
                (
                    layer,
                    self.b.default_class,
                    Attributes {
                        pen,
                        fill: None,
                        line_weight: weight,
                        ..Default::default()
                    },
                )
            }
        };
        self.mapped.insert(name.to_string(), result.clone());
        result
    }

    /// Counts a polyline that was closed by its repeated first point (`gap_mm`: the gap there).
    fn repeated(&mut self, gap_mm: f64) {
        self.repeated_closed += 1;
        self.repeated_gap_mm = self.repeated_gap_mm.max(gap_mm);
    }

    /// An entity that gave nothing, with the reason in the report.
    fn skip_entity(&mut self, e: &Entity, why: &str) {
        self.b.report.entities_skipped += 1;
        self.b.report.skip(&e.kind, why);
    }

    fn entity(&mut self, e: &Entity, cx: &Cx) {
        self.visits += 1;
        if self.overflow {
            return;
        }
        if self.visits > MAX_VISITS || self.b.is_full() {
            let why = if self.b.is_full() {
                "object limit reached"
            } else {
                "import stopped: too many entities"
            };
            self.b.report.entities_read += 1;
            self.skip_entity(e, why);
            return;
        }
        if cx.depth > 0 {
            self.exploded += 1;
            if self.exploded > self.opts.max_exploded {
                self.overflow = true;
                return;
            }
        }
        self.b.report.entities_read += 1;
        if e.int(67) == Some(1) && self.sheet.is_none() {
            self.skip_entity(e, "in paper space");
            return;
        }
        if e.int(60) == Some(1) {
            self.skip_entity(e, "invisible");
            return;
        }
        // The points of a dimension that lie on the layer Defpoints do not plot.
        if e.kind == "POINT" && e.layer().eq_ignore_ascii_case("defpoints") {
            self.skip_entity(
                e,
                "definition point of a dimension (the layer Defpoints does not plot)",
            );
            return;
        }
        match e.kind.as_str() {
            "INSERT" => {
                let clip = self.read_clip(e);
                let ok = self.insert(e, cx, clip.as_ref());
                self.count(ok);
                return;
            }
            // These entities are drawn by the CAD program from their data. Their anonymous block
            // holds the lines and text that it drew.
            "DIMENSION" | "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" | "ACAD_TABLE" => {
                let ok = self.block_proxy(e, cx);
                self.count(ok);
                return;
            }
            "IMAGE" => {
                let why = extra::image_reason(e, self.dxf);
                self.skip_entity(e, &why);
                return;
            }
            _ => {}
        }
        let mut items = Vec::new();
        if let Err(why) = self.convert(e, cx, &mut items) {
            self.skip_entity(e, why);
            return;
        }
        if !cx.clips.is_empty() && !items.is_empty() {
            items = clip::apply(items, &cx.clips, self.opts.tolerance, &mut self.tally);
            if items.is_empty() {
                self.skip_entity(e, "outside the clip (XCLIP) of its block reference");
                return;
            }
        }
        if cx.tilt.is_some() || items.iter().any(|i| i.tilt.is_some()) {
            let z0 = cx.xf.z(0.0);
            let tol = self.opts.tolerance;
            // A flat shape keeps its own elevation: it is its height above the plane of the block.
            let elevation = if e.kind == "LWPOLYLINE" {
                e.fd(38, 0.0)
            } else {
                e.fd(30, 0.0)
            };
            for item in &mut items {
                if item.z.is_none() {
                    item.z = Some(cx.xf.z(elevation));
                }
            }
            let mut moved = Vec::with_capacity(items.len());
            for item in items {
                let map = tilt::combined(item.tilt, &cx.tilt);
                match map {
                    Some(m) => moved.extend(tilt::apply(vec![item], &m, z0, tol, &mut self.tally)),
                    None => moved.push(item),
                }
            }
            items = moved;
            if items.is_empty() {
                self.skip_entity(
                    e,
                    "a white island of a hatch has no meaning in a tilted plane",
                );
                return;
            }
        }
        if self.sheet.is_some() {
            items = self.sheet_items(items);
            if items.is_empty() {
                self.skip_entity(e, "has no 2D shape that a sheet layer can hold");
                return;
            }
        }
        self.b.report.entities_imported += 1;
        // A transparency has the flag 0x02000000 and the opacity in the low byte (255 is opaque).
        if e.int(440)
            .is_some_and(|t| t & 0x0300_0000 == 0x0200_0000 && t & 0xFF < 255)
        {
            self.tally.transparent += 1;
        }
        let layer_name = match (&cx.layer, e.layer()) {
            (Some(l), "0") => l.clone(),
            (_, l) => l.to_string(),
        };
        let (layer, class, defaults) = match self.sheet {
            Some(sheet) => {
                let (class, defaults) = self.class_for(&layer_name);
                (sheet, class, defaults)
            }
            None => self.target(&layer_name),
        };
        let attrs = self.attrs(e, cx, &defaults);
        let parts = items.len();
        // The ids of the outer loops of pattern hatches, and the islands that belong to them.
        let mut outers: HashMap<u32, kadakas_core::uid::Uid> = HashMap::new();
        let mut holes: Vec<(kadakas_core::uid::Uid, Vec<kadakas_core::uid::Uid>)> = Vec::new();
        for (n, mut item) in items.into_iter().enumerate() {
            let (group, is_island, is_pattern) = (
                item.group,
                item.data.contains_key("hatch_island"),
                item.pattern.is_some(),
            );
            let mut a = attrs.clone();
            if item.fill_pen {
                a.fill = Some(a.pen.unwrap_or(Color::BLACK));
            }
            if item.fill.is_some() {
                a.fill = item.fill;
            }
            if item.pen.is_some() {
                a.pen = item.pen;
            }
            if item.pattern.is_some() {
                a.fill_pattern = item.pattern;
            }
            if let Some(t) = self.tracking {
                let part = (parts > 1).then_some(n as u32);
                if let Some(tag) = t.tag(e, &cx.path, part) {
                    item.data.insert(provenance::KEY.into(), tag);
                }
            }
            let added = self.b.add(&e.kind, item.shape, layer, class, a, item.data);
            if let (true, Some(g)) = (added && !self.b.is_probe(), group) {
                let id = self.b.last_id();
                if is_pattern && !is_island {
                    outers.insert(g, id);
                    holes.push((id, Vec::new()));
                } else if let (true, Some(outer)) = (is_island, outers.get(&g))
                    && let Some(h) = holes.iter_mut().find(|h| h.0 == *outer)
                {
                    h.1.push(id);
                }
            }
        }
        // An outer loop knows its islands, so the export can write them as holes of one HATCH.
        for (outer, islands) in holes {
            if islands.is_empty() {
                continue;
            }
            self.b.push(kadakas_core::command::Command::SetData {
                id: kadakas_core::model::ObjectId(outer),
                set: [(
                    "hatch_islands".to_string(),
                    serde_json::to_value(&islands).unwrap_or_default(),
                )]
                .into(),
                remove: vec![],
            });
        }
    }

    /// Counts an entity as imported or skipped (the reason is in the report already).
    fn count(&mut self, imported: bool) {
        if imported {
            self.b.report.entities_imported += 1;
        } else {
            self.b.report.entities_skipped += 1;
        }
    }

    /// A DIMENSION (or table) is drawn from its anonymous block: it is inserted like a block
    /// reference at the origin. Without the block there is nothing to draw.
    fn block_proxy(&mut self, e: &Entity, cx: &Cx) -> bool {
        let name = e.get(2).unwrap_or("");
        if name.is_empty() || !self.dxf.blocks.contains_key(name) {
            self.b.report.skip(
                &e.kind,
                "has no geometry block in the file, so there is nothing to draw (the CAD program draws it from its data)",
            );
            return false;
        }
        let mut items: Vec<(i32, String)> = vec![
            (2, name.to_string()),
            (8, e.layer().to_string()),
            (10, "0".into()),
            (20, "0".into()),
            (30, "0".into()),
        ];
        for code in [5, 62, 420, 370] {
            if let Some(v) = e.get(code) {
                items.push((code, v.to_string()));
            }
        }
        let proxy = Entity {
            kind: "INSERT".into(),
            items,
            children: vec![],
        };
        self.insert(&proxy, cx, None)
    }

    /// Adds the merged 3D faces as mesh objects, one per DXF layer.
    fn flush_faces(&mut self) {
        let faces = std::mem::take(&mut self.faces);
        for (layer_name, m) in faces {
            let (layer, class, defaults) = self.target(&layer_name);
            let mesh = Mesh3d {
                name: String::new(),
                vertices: m.vertices,
                triangles: m.triangles,
                color: None,
                exact: None,
            };
            let attrs = Attributes {
                pen: defaults.pen,
                fill: defaults.fill,
                line_weight: defaults.line_weight,
                ..Default::default()
            };
            self.b.add(
                "3DFACE",
                Shape::Mesh3d(Box::new(mesh)),
                layer,
                class,
                attrs,
                BTreeMap::new(),
            );
        }
    }

    fn attrs(&self, e: &Entity, cx: &Cx, defaults: &Attributes) -> Attributes {
        let pen = if let Some(t) = e.int(420).and_then(|v| u32::try_from(v).ok()) {
            Some(true_color(t))
        } else {
            match e.int(62) {
                Some(0) => cx.pen.or(defaults.pen),
                Some(i) if i != 256 => u8::try_from(i.unsigned_abs())
                    .ok()
                    .and_then(aci_rgb)
                    .map(|[r, g, b]| Color([r, g, b, 255]))
                    .or(defaults.pen),
                _ => defaults.pen,
            }
        };
        let line_weight = match e.int(370) {
            Some(-2) => cx.weight.or(defaults.line_weight),
            Some(v) if v >= 0 => weight_mm(v),
            _ => defaults.line_weight,
        };
        Attributes {
            pen,
            fill: defaults.fill,
            line_weight,
            ..Default::default()
        }
    }

    /// Expands a block reference. False when nothing was inserted (the reason is in the report).
    fn insert(&mut self, e: &Entity, cx: &Cx, clip: Option<&clip::ClipRead>) -> bool {
        if !self.opts.explode_blocks {
            self.b
                .report
                .skip("INSERT", "blocks are not exploded (option)");
            return false;
        }
        let Some(name) = e.get(2) else {
            self.b.report.skip("INSERT", "no block name");
            return false;
        };
        let Some(block) = self.dxf.blocks.get(name) else {
            self.b.report.skip("INSERT", "block is not defined");
            return false;
        };
        if block.flags & 12 != 0 {
            // An external reference holds no geometry in this file: say which file it needs.
            let path = if block.path.is_empty() {
                "unknown path"
            } else {
                &block.path
            };
            self.b.report.skip(
                "INSERT",
                &format!("external reference (XREF) '{name}' to {path} is not loaded: import that file too"),
            );
            return false;
        }
        if cx.depth >= MAX_DEPTH {
            self.b.report.skip("INSERT", "blocks are nested too deep");
            return false;
        }
        let (flip, own_tilt) = match Plane::of(e) {
            Plane::Flat { flip } => (flip, None),
            Plane::Tilted(ocs) => {
                let Some(map) = tilt::map_of(&cx.xf, &ocs) else {
                    self.b.report.skip(
                        "INSERT",
                        "tilted extrusion direction: the block cannot be placed, because the transform around it cannot be inverted",
                    );
                    return false;
                };
                (false, Some(map))
            }
        };
        if own_tilt.is_some() {
            self.tally.tilted_blocks += 1;
        }
        let (sx, sy, sz) = (e.fd(41, 1.0), e.fd(42, 1.0), e.fd(43, 1.0));
        let rot = e.fd(50, 0.0).to_radians();
        let ins = DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0));
        let cols_n = e.int(70).unwrap_or(1).clamp(1, MAX_ARRAY);
        let rows_n = e.int(71).unwrap_or(1).clamp(1, MAX_ARRAY);
        if cols_n * rows_n > MAX_ARRAY {
            self.b.report.skip("INSERT", "array is too large");
            return false;
        }
        let (dx, dy) = (e.fd(44, 0.0), e.fd(45, 0.0));
        let (sin, cos) = rot.sin_cos();
        let rotm = |v: DVec2| DVec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
        let base = DVec2::new(block.base[0], block.base[1]);
        let m = [rotm(DVec2::new(sx, 0.0)), rotm(DVec2::new(0.0, sy))];
        let flipv = |v: DVec2| if flip { DVec2::new(-v.x, v.y) } else { v };
        let layer = match e.layer() {
            "0" => cx.layer.clone().or_else(|| Some("0".into())),
            l => Some(l.to_string()),
        };
        let own_pen = e
            .int(420)
            .and_then(|v| u32::try_from(v).ok())
            .map(true_color)
            .or_else(|| match e.int(62) {
                Some(i) if i > 0 => u8::try_from(i)
                    .ok()
                    .and_then(aci_rgb)
                    .map(|[r, g, b]| Color([r, g, b, 255])),
                _ => cx.pen,
            });
        for r in 0..rows_n {
            for c in 0..cols_n {
                let at = ins + rotm(DVec2::new(c as f64 * dx, r as f64 * dy));
                let local = Xf {
                    cols: [flipv(m[0]), flipv(m[1])],
                    t: flipv(at - (m[0] * base.x + m[1] * base.y)),
                    zs: sz,
                    zt: e.fd(30, 0.0) - sz * block.base[2],
                };
                let path = match self.tracking {
                    Some(t) => {
                        let copy = (cols_n * rows_n > 1).then_some((r * cols_n + c) as u32);
                        t.enter(&cx.path, e, name, block, copy)
                    }
                    None => Path::default(),
                };
                let xf = cx.xf.after(&local);
                // The clip of the reference joins the clips of the references around it.
                let mut clips = cx.clips.clone();
                if let Some(read) = clip {
                    clips.extend(self.clip_region(read, &xf, r == 0 && c == 0));
                }
                let inner = Cx {
                    xf,
                    layer: layer.clone(),
                    pen: own_pen,
                    weight: cx.weight,
                    depth: cx.depth + 1,
                    path,
                    clips,
                    tilt: match own_tilt {
                        Some(map) => tilt::chain(&cx.tilt, map),
                        None => cx.tilt.clone(),
                    },
                };
                for sub in &block.entities {
                    self.entity(sub, &inner);
                }
            }
        }
        true
    }

    fn convert(&mut self, e: &Entity, cx: &Cx, out: &mut Vec<Item>) -> Result<(), Skip> {
        let xf = &cx.xf;
        let tol = self.opts.tolerance;
        match e.kind.as_str() {
            "LINE" => {
                let (a, b) = (
                    (e.fd(10, 0.0), e.fd(20, 0.0), e.fd(30, 0.0)),
                    (e.fd(11, 0.0), e.fd(21, 0.0), e.fd(31, 0.0)),
                );
                if a.2 != 0.0 || b.2 != 0.0 {
                    let p = |t: (f64, f64, f64)| xf.apply(DVec2::new(t.0, t.1)).extend(xf.z(t.2));
                    out.push(Item::new(Shape::Polyline3d {
                        points: vec![p(a), p(b)],
                        closed: false,
                    }));
                } else {
                    out.push(Item::new(Shape::Line {
                        a: xf.apply(DVec2::new(a.0, a.1)),
                        b: xf.apply(DVec2::new(b.0, b.1)),
                    }));
                }
            }
            "LWPOLYLINE" => {
                let plane = Plane::of(e);
                let mut pts: Vec<(DVec2, f64)> = Vec::new();
                for (code, value) in &e.items {
                    let Some(v) = parse::parse_f64(value) else {
                        continue;
                    };
                    match code {
                        10 => pts.push((DVec2::new(v, 0.0), 0.0)),
                        20 => {
                            if let Some(p) = pts.last_mut() {
                                p.0.y = v;
                            }
                        }
                        42 => {
                            if let Some(p) = pts.last_mut() {
                                p.1 = v;
                            }
                        }
                        _ => {}
                    }
                }
                let mut closed = e.int(70).unwrap_or(0) & 1 != 0;
                if let Some(g) = close_repeated(&mut pts, &mut closed, |p| p.0.extend(0.0)) {
                    self.repeated(g * xf.length_scale());
                }
                let elevation = e.fd(38, 0.0);
                let flip = match plane {
                    Plane::Flat { flip } => flip,
                    Plane::Tilted(ocs) => {
                        if pts.len() < 2 {
                            return Err("fewer than 2 vertices");
                        }
                        let ring = tilted::polyline_points(
                            &ocs,
                            &pts,
                            closed,
                            elevation,
                            tol / xf.length_scale().max(1e-12),
                        );
                        self.tally.tilted += 1;
                        out.push(Item::new(Shape::Polyline3d {
                            points: ring.iter().map(|p| world3(xf, *p)).collect(),
                            closed,
                        }));
                        return Ok(());
                    }
                };
                out.push(Item::new(polyline_from(
                    &pts, closed, flip, elevation, xf, tol,
                )?));
            }
            "POLYLINE" => {
                let flags = e.int(70).unwrap_or(0);
                if flags & 64 != 0 {
                    out.push(Item::new(Shape::Mesh3d(Box::new(polyface_mesh(e, xf)?))));
                    return Ok(());
                }
                if flags & 16 != 0 {
                    out.push(Item::new(Shape::Mesh3d(Box::new(grid_mesh(e, xf)?))));
                    return Ok(());
                }
                let mut closed = flags & 1 != 0;
                let verts = e
                    .children
                    .iter()
                    .filter(|v| !(flags & 4 != 0 && v.int(70).unwrap_or(0) & 16 != 0));
                if flags & 8 != 0 {
                    let mut points: Vec<DVec3> = verts
                        .map(|v| {
                            xf.apply(DVec2::new(v.fd(10, 0.0), v.fd(20, 0.0)))
                                .extend(xf.z(v.fd(30, 0.0)))
                        })
                        .collect();
                    if let Some(g) = close_repeated(&mut points, &mut closed, |p| *p) {
                        self.repeated(g);
                    }
                    out.push(Item::new(Shape::Polyline3d { points, closed }));
                } else {
                    let mut pts: Vec<(DVec2, f64)> = verts
                        .map(|v| (DVec2::new(v.fd(10, 0.0), v.fd(20, 0.0)), v.fd(42, 0.0)))
                        .collect();
                    if let Some(g) = close_repeated(&mut pts, &mut closed, |p| p.0.extend(0.0)) {
                        self.repeated(g * xf.length_scale());
                    }
                    let flip = match Plane::of(e) {
                        Plane::Flat { flip } => flip,
                        Plane::Tilted(ocs) => {
                            if pts.len() < 2 {
                                return Err("fewer than 2 vertices");
                            }
                            let ring = tilted::polyline_points(
                                &ocs,
                                &pts,
                                closed,
                                e.fd(30, 0.0),
                                tol / xf.length_scale().max(1e-12),
                            );
                            self.tally.tilted += 1;
                            out.push(Item::new(Shape::Polyline3d {
                                points: ring.iter().map(|p| world3(xf, *p)).collect(),
                                closed,
                            }));
                            return Ok(());
                        }
                    };
                    out.push(Item::new(polyline_from(
                        &pts,
                        closed,
                        flip,
                        e.fd(30, 0.0),
                        xf,
                        tol,
                    )?));
                }
            }
            "CIRCLE" => {
                let (c, r) = (
                    DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0)),
                    e.f(40).ok_or("no radius")?,
                );
                let flip = match Plane::of(e) {
                    Plane::Flat { flip } => flip,
                    Plane::Tilted(ocs) => {
                        let ring = tilted::circle_points(
                            &ocs,
                            c.extend(e.fd(30, 0.0)),
                            r,
                            tol / xf.length_scale().max(1e-12),
                        );
                        self.tally.tilted += 1;
                        out.push(Item::new(Shape::Polyline3d {
                            points: ring.iter().map(|p| world3(xf, *p)).collect(),
                            closed: true,
                        }));
                        return Ok(());
                    }
                };
                if !xf.is_similarity() {
                    self.tally.scaled_arcs += 1;
                }
                if xf.is_similarity() {
                    out.push(Item::new(Shape::Circle {
                        center: xf.apply(flipx(c, flip)),
                        radius: r * xf.length_scale(),
                    }));
                } else {
                    let pts = ellipse_points(
                        c,
                        DVec2::new(r, 0.0),
                        1.0,
                        0.0,
                        TAU,
                        tol / xf.length_scale().max(1e-12),
                    );
                    let pts: Vec<DVec2> =
                        pts.into_iter().map(|p| xf.apply(flipx(p, flip))).collect();
                    out.push(Item::new(Shape::Polyline {
                        points: corners(&drop_closing(pts)),
                        closed: true,
                    }));
                }
            }
            "ARC" => {
                let (c, r) = (
                    DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0)),
                    e.f(40).ok_or("no radius")?,
                );
                let (a0, a1) = (e.fd(50, 0.0).to_radians(), e.fd(51, 360.0).to_radians());
                let mut sweep = (a1 - a0).rem_euclid(TAU);
                if sweep == 0.0 {
                    sweep = TAU;
                }
                let flip = match Plane::of(e) {
                    Plane::Flat { flip } => flip,
                    Plane::Tilted(ocs) => {
                        let pts = tilted::arc_points(
                            &ocs,
                            c.extend(e.fd(30, 0.0)),
                            r,
                            a0,
                            sweep,
                            tol / xf.length_scale().max(1e-12),
                        );
                        self.tally.tilted += 1;
                        out.push(Item::new(Shape::Polyline3d {
                            points: pts.iter().map(|p| world3(xf, *p)).collect(),
                            closed: false,
                        }));
                        return Ok(());
                    }
                };
                if !xf.is_similarity() {
                    self.tally.scaled_arcs += 1;
                }
                if xf.is_similarity() {
                    let at = |a: f64| xf.apply(flipx(c + DVec2::from_angle(a) * r, flip));
                    let center = xf.apply(flipx(c, flip));
                    let (s, en) = (at(a0) - center, at(a0 + sweep) - center);
                    let mirrored = (xf.det() < 0.0) != flip;
                    let (start, end) = if mirrored {
                        (en.to_angle(), s.to_angle())
                    } else {
                        (s.to_angle(), en.to_angle())
                    };
                    out.push(Item::new(Shape::Arc {
                        center,
                        radius: r * xf.length_scale(),
                        start,
                        end,
                    }));
                } else {
                    let n = arc_steps(r * xf.cols[0].length().max(xf.cols[1].length()), sweep, tol);
                    let pts = (0..=n)
                        .map(|i| {
                            xf.apply(flipx(
                                c + DVec2::from_angle(a0 + sweep * i as f64 / n as f64) * r,
                                flip,
                            ))
                        })
                        .collect::<Vec<_>>();
                    out.push(Item::new(Shape::Polyline {
                        points: corners(&pts),
                        closed: false,
                    }));
                }
            }
            "ELLIPSE" => {
                let c = DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0));
                let major = DVec2::new(e.fd(11, 0.0), e.fd(21, 0.0));
                let ratio = e.fd(40, 1.0);
                if major.length_squared() == 0.0 || ratio.is_nan() || ratio <= 0.0 {
                    return Err("degenerate ellipse");
                }
                let (t0, t1) = (e.fd(41, 0.0), e.fd(42, TAU));
                let mut sweep = (t1 - t0).rem_euclid(TAU);
                if sweep == 0.0 || (t1 - t0).abs() >= TAU - 1e-9 {
                    sweep = TAU;
                }
                self.tally.ellipses += 1;
                if let Plane::Tilted(ocs) = Plane::of(e) {
                    // The center and the major axis are world points; the minor axis lies in the
                    // plane of the extrusion direction.
                    let ring = tilted::ellipse_world(
                        DVec3::new(e.fd(10, 0.0), e.fd(20, 0.0), e.fd(30, 0.0)),
                        DVec3::new(e.fd(11, 0.0), e.fd(21, 0.0), e.fd(31, 0.0)),
                        ocs.z,
                        ratio,
                        t0,
                        sweep,
                        tol / xf.length_scale().max(1e-12),
                    );
                    let full = sweep >= TAU - 1e-9;
                    let mut points: Vec<DVec3> = ring.iter().map(|p| world3(xf, *p)).collect();
                    if full && points.len() > 1 {
                        points.pop();
                    }
                    self.tally.tilted += 1;
                    out.push(Item::new(Shape::Polyline3d {
                        points,
                        closed: full,
                    }));
                    return Ok(());
                }
                let nz = if e.fd(230, 1.0) < 0.0 { -1.0 } else { 1.0 };
                let pts = ellipse_points(
                    c,
                    major,
                    ratio * nz,
                    t0,
                    sweep,
                    tol / xf.length_scale().max(1e-12),
                );
                let full = sweep >= TAU - 1e-9;
                let pts: Vec<DVec2> = pts.into_iter().map(|p| xf.apply(p)).collect();
                let pts = if full { drop_closing(pts) } else { pts };
                out.push(Item::new(Shape::Polyline {
                    points: corners(&pts),
                    closed: full,
                }));
            }
            "SPLINE" => out.push(Item::new(spline(
                e,
                xf,
                tol / xf.length_scale().max(1e-12),
                &mut self.tally,
            )?)),
            "TEXT" | "MTEXT" | "ATTRIB" | "ATTDEF" => {
                // An attribute definition shows only when it is constant (flag 2): its value is
                // the text. Any other definition is the template of the attribute of an INSERT.
                if e.kind == "ATTDEF" && e.int(70).unwrap_or(0) & 2 == 0 {
                    return Err(extra::reason("ATTDEF").unwrap_or("attribute definition"));
                }
                let (flip, ocs) = match Plane::of(e) {
                    Plane::Flat { flip } => (flip, None),
                    Plane::Tilted(ocs) => (false, Some(ocs)),
                };
                if (e.kind == "ATTRIB" || e.kind == "ATTDEF") && e.int(70).unwrap_or(0) & 1 != 0 {
                    return Err("invisible attribute");
                }
                let mtext = e.kind == "MTEXT";
                let raw: String = if mtext {
                    e.items
                        .iter()
                        .filter(|(c, _)| *c == 3)
                        .map(|(_, v)| v.as_str())
                        .chain(e.get(1))
                        .collect()
                } else {
                    e.get(1).unwrap_or("").to_string()
                };
                let text = if mtext {
                    clean_mtext(&raw)
                } else {
                    decode_text(&raw)
                };
                if text.trim().is_empty() {
                    return Err("empty text");
                }
                let aligned = !mtext
                    && (e.int(72).unwrap_or(0) != 0 || e.int(73).unwrap_or(0) != 0)
                    && e.f(11).is_some();
                let p = if aligned {
                    DVec2::new(e.fd(11, 0.0), e.fd(21, 0.0))
                } else {
                    DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0))
                };
                let p = if mtext { p } else { flipx(p, flip) };
                // In a tilted plane the text is placed in the plane first and projected after (see `tilt`).
                // The point of a TEXT is in the OCS. The point and the direction of an MTEXT are in the drawing.
                let mut elevation = e.fd(30, 0.0);
                let mut p = p;
                if let (Some(o), true) = (&ocs, mtext) {
                    let w = DVec3::new(e.fd(10, 0.0), e.fd(20, 0.0), e.fd(30, 0.0));
                    p = DVec2::new(o.x.dot(w), o.y.dot(w));
                    elevation = o.z.dot(w);
                }
                let height = e
                    .f(40)
                    .filter(|h| *h > 0.0)
                    .map(|h| h * xf.length_scale())
                    .unwrap_or(self.opts.default_text_height);
                // Direction of the baseline: MTEXT has a vector (11, 21) or an angle in radians, TEXT degrees.
                let dir = if mtext {
                    match (e.f(11), e.f(21)) {
                        (Some(x), Some(y))
                            if x != 0.0 || y != 0.0 || (ocs.is_some() && e.fd(31, 0.0) != 0.0) =>
                        {
                            let w = DVec3::new(x, y, e.fd(31, 0.0));
                            match &ocs {
                                Some(o) => {
                                    DVec2::new(o.x.dot(w), o.y.dot(w)).normalize_or(DVec2::X)
                                }
                                None => DVec2::new(x, y).normalize(),
                            }
                        }
                        _ => DVec2::from_angle(e.fd(50, 0.0)),
                    }
                } else {
                    DVec2::from_angle(e.fd(50, 0.0).to_radians())
                };
                let dir = xf.cols[0] * flipx(dir, flip).x + xf.cols[1] * flipx(dir, flip).y;
                let mut item = Item::new(Shape::Text {
                    at: xf.apply(p),
                    text,
                    height,
                });
                if let Some(o) = &ocs {
                    item.tilt = Some(tilt::map_of(xf, o).ok_or(
                        "tilted extrusion direction: the text cannot be placed, because the transform around it cannot be inverted",
                    )?);
                    item.z = Some(xf.z(elevation));
                }
                let deg = dir.y.atan2(dir.x).to_degrees();
                if deg.is_finite() && deg.abs() > 1e-9 {
                    let deg = (deg * 1e6).round() / 1e6;
                    item.data.insert(
                        "rotation_deg".into(),
                        serde_json::Value::from(deg.rem_euclid(360.0)),
                    );
                }
                out.push(item);
            }
            "POINT" => {
                let z = e.fd(30, 0.0);
                if self.opts.points == PointMode::WithZ && z == 0.0 {
                    return Err("point without Z value (option)");
                }
                out.push(Item::new(Shape::Locus3d {
                    p: xf
                        .apply(DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0)))
                        .extend(xf.z(z)),
                }));
            }
            "3DFACE" => {
                let mut pts: Vec<DVec3> = Vec::new();
                for k in 0..4 {
                    let p = DVec3::new(e.fd(10 + k, 0.0), e.fd(20 + k, 0.0), e.fd(30 + k, 0.0));
                    let p = xf.apply(p.truncate()).extend(xf.z(p.z));
                    if pts.last() != Some(&p) && !(k == 3 && pts.first() == Some(&p)) {
                        pts.push(p);
                    }
                }
                if pts.len() < 3 {
                    return Err("degenerate face");
                }
                if self.opts.faces == crate::options::FaceMode::Mesh {
                    let layer_name = match (&cx.layer, e.layer()) {
                        (Some(l), "0") => l.clone(),
                        (_, l) => l.to_string(),
                    };
                    self.faces.entry(layer_name).or_default().face(&pts);
                    return Ok(());
                }
                out.push(Item::new(Shape::Polyline3d {
                    points: pts,
                    closed: true,
                }));
            }
            "HATCH" => {
                if !self.opts.hatches {
                    return Err("hatches are not imported (option)");
                }
                let plane = Plane::of(e);
                let flip = match plane {
                    Plane::Flat { flip } => flip,
                    Plane::Tilted(_) => false,
                };
                let (loops, dropped) = hatch_loops(e, tol / xf.length_scale().max(1e-12));
                for _ in 0..dropped {
                    self.b.report.skip(
                        "HATCH",
                        "boundary path that cannot be read (unsupported edge, edges that do not join, or too few points)",
                    );
                }
                if loops.is_empty() {
                    // Every path was reported above; a hatch without any says so once.
                    if dropped > 0 {
                        return Ok(());
                    }
                    return Err("hatch without usable boundary");
                }
                if let Plane::Tilted(ocs) = plane {
                    // A fill cannot lie in a tilted plane: the boundary paths are 3D polylines.
                    let elevation = e.fd(30, 0.0);
                    for l in loops {
                        let ring = kadakas_core::curve::flatten(
                            &l,
                            true,
                            tol / xf.length_scale().max(1e-12),
                        );
                        let mut points: Vec<DVec3> = ring
                            .iter()
                            .map(|p| world3(xf, ocs.world(p.extend(elevation))))
                            .collect();
                        if points.len() > 1 && points.first() == points.last() {
                            points.pop();
                        }
                        out.push(Item::new(Shape::Polyline3d {
                            points,
                            closed: true,
                        }));
                    }
                    self.tally.tilted_hatches += 1;
                    return Ok(());
                }
                let solid = e.int(70) == Some(1);
                let gradient = solid && e.int(450) == Some(1);
                // A gradient has two colors; the first one is the flat fill. A plain solid hatch
                // fills with its pen color.
                let fill = gradient
                    .then(|| {
                        self.tally.gradient_hatches += 1;
                        e.int(421)
                            .and_then(|v| u32::try_from(v).ok())
                            .map(true_color)
                            .or_else(|| {
                                e.int(63)
                                    .and_then(|i| u8::try_from(i).ok())
                                    .and_then(aci_rgb)
                                    .map(|[r, g, b]| Color([r, g, b, 255]))
                            })
                    })
                    .flatten();
                // The outlines with their curves cut into short chords, to test which loop lies in which.
                let corner_points: Vec<Vec<DVec2>> = loops
                    .iter()
                    .map(|l| {
                        kadakas_core::curve::flatten(
                            l,
                            true,
                            tol / 50.0 / xf.length_scale().max(1e-12),
                        )
                    })
                    .collect();
                // A pattern hatch becomes a hatch fill of the document (IOP-27).
                let pattern = if solid {
                    None
                } else if cx.tilt.is_some() {
                    // A fill cannot lie in a tilted plane, so the hatch stays an outline. No hatch
                    // resource is made for it.
                    self.tally.tilted_fills += 1;
                    None
                } else {
                    let extent = extra::extent(&corner_points)
                        * xf.cols[0].length().max(xf.cols[1].length());
                    self.pattern_fill(e, xf, flip, extent)
                };
                let fills = solid || pattern.is_some();
                let island = if fills && e.int(75).unwrap_or(0) != 2 {
                    extra::islands(&corner_points, tol / 10.0 / xf.length_scale().max(1e-12))
                } else {
                    vec![false; loops.len()]
                };
                // An outer loop is drawn before the islands inside it.
                let mut order: Vec<usize> = (0..loops.len()).collect();
                if fills {
                    let areas: Vec<f64> = corner_points.iter().map(|p| extra::area(p)).collect();
                    order.sort_by(|a, b| areas[*b].total_cmp(&areas[*a]));
                }
                // The outer loop that holds each island: the smallest loop (not an island) around it.
                let areas: Vec<f64> = corner_points.iter().map(|p| extra::area(p)).collect();
                let holder: Vec<Option<usize>> = (0..loops.len())
                    .map(|k| {
                        if !island[k] {
                            return None;
                        }
                        let at = *corner_points[k].first()?;
                        (0..loops.len())
                            .filter(|j| {
                                !island[*j]
                                    && areas[*j] > areas[k]
                                    && kadakas_core::geom::point_in_polygon(at, &corner_points[*j])
                            })
                            .min_by(|a, b| areas[*a].total_cmp(&areas[*b]))
                    })
                    .collect();
                let mut loops: Vec<Option<Vec<Vertex>>> = loops.into_iter().map(Some).collect();
                for k in order {
                    let (Some(l), is_island) = (loops[k].take(), island[k]) else {
                        continue;
                    };
                    let pts: Vec<Vertex> = l
                        .into_iter()
                        .map(|v| Vertex::new(xf.apply(flipx(v.p, flip)), v.kind))
                        .collect();
                    let mut item = Item::new(Shape::Polyline {
                        points: pts,
                        closed: true,
                    });
                    if fills {
                        item.group = Some(if is_island {
                            holder[k].unwrap_or(k) as u32
                        } else {
                            k as u32
                        });
                    }
                    if is_island {
                        // A filled shape has no hole: the island is the color of the paper.
                        item.fill = Some(extra::PAPER);
                        item.data.insert("hatch_island".into(), true.into());
                        self.tally.hatch_islands += 1;
                    } else if solid {
                        item.fill = fill;
                        item.fill_pen = fill.is_none();
                    } else {
                        item.pattern = pattern;
                    }
                    out.push(item);
                }
            }
            "WIPEOUT" => {
                out.push(extra::wipeout(e, xf)?);
                self.tally.wipeouts += 1;
            }
            "SOLID" | "TRACE" => {
                let plane = Plane::of(e);
                if let Plane::Tilted(_) = plane {
                    self.tally.tilted += 1;
                }
                out.push(extra::solid(e, xf, &plane)?);
            }
            "MESH" => {
                let (mesh, subdivided) = extra::mesh(e, xf)?;
                if subdivided {
                    self.tally.subdivided_meshes += 1;
                }
                out.push(Item::new(Shape::Mesh3d(Box::new(mesh))));
            }
            "LEADER" => {
                out.push(extra::leader(e, xf)?);
                self.tally.leaders += 1;
            }
            other => return Err(extra::reason(other).unwrap_or("unsupported entity type")),
        }
        Ok(())
    }
}

const REPEATED_NOTE: &str = "polylines whose closed flag is off but whose last point equals the first were imported as closed (the repeated point is dropped)";

/// CAD programs write a closed outline as an open polyline that ends where it starts. Such a
/// polyline (a circle is 2 points that repeat the first) becomes closed, and the repeated point
/// is dropped. "Equal" means a few units in the last place of the coordinates (16 times the
/// machine epsilon of their size): a real gap is a gap and the polyline stays open. Returns the
/// gap, in file units, of a polyline that it closed.
fn close_repeated<T>(pts: &mut Vec<T>, closed: &mut bool, at: impl Fn(&T) -> DVec3) -> Option<f64> {
    if *closed || pts.len() < 3 {
        return None;
    }
    let (a, b) = (at(&pts[0]), at(&pts[pts.len() - 1]));
    let gap = a.distance(b);
    let size = a.abs().max_element().max(b.abs().max_element());
    if gap > 16.0 * f64::EPSILON * size {
        return None;
    }
    pts.pop();
    *closed = true;
    Some(gap)
}

/// A world point through the transform of the block it is in: x and y by the 2D transform, z by the scale and move of z.
fn world3(xf: &Xf, p: DVec3) -> DVec3 {
    xf.apply(p.truncate()).extend(xf.z(p.z))
}

fn flipx(p: DVec2, flip: bool) -> DVec2 {
    if flip { DVec2::new(-p.x, p.y) } else { p }
}

fn drop_closing(mut pts: Vec<DVec2>) -> Vec<DVec2> {
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    pts
}

/// Number of chords for an arc of this radius and sweep within `tol`.
fn arc_steps(radius: f64, sweep: f64, tol: f64) -> usize {
    let r = radius.abs().max(1e-12);
    let step = if tol >= r {
        PI / 2.0
    } else {
        2.0 * (1.0 - tol / r).acos()
    };
    ((sweep.abs() / step.max(1e-6)).ceil() as usize).clamp(4, 4096)
}

/// Points of an ellipse from parameter `t0` over `sweep`. `ratio` is negative for a mirrored minor axis.
fn ellipse_points(c: DVec2, major: DVec2, ratio: f64, t0: f64, sweep: f64, tol: f64) -> Vec<DVec2> {
    let minor = DVec2::new(-major.y, major.x) * ratio;
    let n = arc_steps(major.length().max(minor.length()), sweep, tol);
    (0..=n)
        .map(|i| {
            let t = t0 + sweep * i as f64 / n as f64;
            c + major * t.cos() + minor * t.sin()
        })
        .collect()
}

/// Midpoint of the arc from `a` to `b` with this bulge.
fn bulge_mid(a: DVec2, b: DVec2, bulge: f64) -> DVec2 {
    let d = b - a;
    let mid = (a + b) / 2.0;
    // Right of the direction a to b is where a counterclockwise arc bulges.
    mid + DVec2::new(d.y, -d.x) * (bulge / 2.0)
}

/// Points along the arc from `a` to `b` with this bulge, `a` first and `b` last.
fn bulge_points(a: DVec2, b: DVec2, bulge: f64, tol: f64) -> Vec<DVec2> {
    let chord = a.distance(b);
    let sweep = 4.0 * bulge.atan();
    let half = sweep.abs() / 2.0;
    let r = chord / (2.0 * half.sin()).max(1e-300);
    let left = DVec2::new(-(b - a).y, (b - a).x) / chord;
    let center = (a + b) / 2.0 + left * (r * half.cos()) * bulge.signum();
    let n = arc_steps(r, sweep, tol);
    let a0 = (a - center).to_angle();
    (0..=n)
        .map(|i| {
            if i == 0 {
                a
            } else if i == n {
                b
            } else {
                center + DVec2::from_angle(a0 + sweep * i as f64 / n as f64) * r
            }
        })
        .collect()
}

/// A polyline from OCS points with bulges. Straight polylines with an elevation become 3D.
fn polyline_from(
    pts: &[(DVec2, f64)],
    closed: bool,
    flip: bool,
    elevation: f64,
    xf: &Xf,
    tol: f64,
) -> Result<Shape, Skip> {
    if pts.len() < 2 {
        return Err("fewer than 2 vertices");
    }
    let has_bulge = pts.iter().any(|p| p.1.abs() > 1e-9);
    if !has_bulge && elevation != 0.0 {
        let points = pts
            .iter()
            .map(|p| xf.apply(flipx(p.0, flip)).extend(xf.z(elevation)))
            .collect();
        return Ok(Shape::Polyline3d { points, closed });
    }
    let n = pts.len();
    let similar = xf.is_similarity();
    let tp = |p: DVec2| xf.apply(flipx(p, flip));
    let mut out: Vec<Vertex> = Vec::with_capacity(n * 2);
    for i in 0..n {
        out.push(Vertex::corner(tp(pts[i].0)));
        let bulge = pts[i].1;
        let last = i + 1 == n;
        if bulge.abs() <= 1e-9 || (last && !closed) {
            continue;
        }
        let (a, b) = (pts[i].0, pts[(i + 1) % n].0);
        if a.distance(b) <= 1e-12 {
            continue;
        }
        if similar {
            out.push(Vertex::new(
                tp(bulge_mid(a, b, bulge)),
                VertexKind::PointOnArc,
            ));
        } else {
            let seg = bulge_points(
                a,
                b,
                bulge,
                tol / xf.cols[0].length().max(xf.cols[1].length()).max(1e-12),
            );
            out.extend(seg[1..seg.len() - 1].iter().map(|p| Vertex::corner(tp(*p))));
        }
    }
    Ok(Shape::Polyline {
        points: out,
        closed,
    })
}

/// Boundary loops of a HATCH as vertices in OCS: polyline paths and line/arc edge paths.
fn hatch_loops(e: &Entity, tol: f64) -> (Vec<Vec<Vertex>>, usize) {
    let mut loops = Vec::new();
    let mut dropped = 0usize;
    let items = &e.items;
    let mut i = items
        .iter()
        .position(|(c, _)| *c == 91)
        .unwrap_or(items.len());
    let num = items
        .get(i)
        .and_then(|(_, v)| parse::parse_int(v))
        .unwrap_or(0)
        .clamp(0, 10_000);
    i += 1;
    let next = |i: &mut usize, code: i32| -> Option<f64> {
        while *i < items.len() {
            let (c, v) = &items[*i];
            if *c == code {
                *i += 1;
                return parse::parse_f64(v);
            }
            if *c == 92 {
                return None;
            }
            *i += 1;
        }
        None
    };
    for _ in 0..num {
        let Some(flags) = next(&mut i, 92) else { break };
        if flags as i64 & 2 != 0 {
            let has_bulge = next(&mut i, 72).unwrap_or(0.0) != 0.0;
            let _closed = next(&mut i, 73);
            let count = next(&mut i, 93).unwrap_or(0.0).clamp(0.0, 1.0e6) as usize;
            let mut pts = Vec::new();
            for _ in 0..count {
                let (Some(x), Some(y)) = (next(&mut i, 10), next(&mut i, 20)) else {
                    break;
                };
                let bulge = if has_bulge {
                    peek_bulge(items, &mut i)
                } else {
                    0.0
                };
                pts.push((DVec2::new(x, y), bulge));
            }
            match polyline_from(&pts, true, false, 0.0, &Xf::scale(1.0), tol) {
                Ok(Shape::Polyline { points, .. }) => loops.push(points),
                _ => dropped += 1,
            }
        } else {
            let n = next(&mut i, 93).unwrap_or(0.0).clamp(0.0, 1.0e6) as usize;
            let mut edges: Vec<Vec<Vertex>> = Vec::with_capacity(n.min(1024));
            let mut ok = true;
            for _ in 0..n {
                match hatch_edge(&mut i, &next, tol) {
                    Some(edge) => edges.push(edge),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            match ok.then(|| chain_edges(edges, tol.max(1e-9))).flatten() {
                Some(path) => loops.push(path),
                None => dropped += 1,
            }
        }
    }
    (loops, dropped)
}

/// Joins the edges of a hatch edge path into one closed ring. Edges follow each other, but one
/// may be written the other way round, and so may the first. A join or the closing gap wider
/// than `tol` makes the path unusable (`None`): a self-crossing outline would be worse.
fn chain_edges(mut edges: Vec<Vec<Vertex>>, tol: f64) -> Option<Vec<Vertex>> {
    if edges.is_empty() {
        return None;
    }
    let ends = |e: &Vec<Vertex>| (e[0].p, e[e.len() - 1].p);
    if edges.len() > 1 {
        // The first edge ends where the second begins, whichever way each is written.
        let (s2, e2) = ends(&edges[1]);
        let (s1, e1) = ends(&edges[0]);
        let near = |p: DVec2| p.distance(s2).min(p.distance(e2));
        if near(s1) < near(e1) {
            edges[0].reverse();
        }
    }
    let mut it = edges.into_iter();
    let mut path = it.next()?;
    for mut edge in it {
        let end = path[path.len() - 1].p;
        let (s, e) = ends(&edge);
        if s.distance(end) > e.distance(end) {
            edge.reverse();
        }
        if edge[0].p.distance(end) > tol {
            return None;
        }
        path.extend(edge.into_iter().skip(1));
    }
    if path.len() > 1 {
        if path[0].p.distance(path[path.len() - 1].p) > tol {
            return None;
        }
        path.pop();
    }
    (path.len() >= 3 || (path.len() == 2 && path.iter().any(|v| v.kind != VertexKind::Corner)))
        .then_some(path)
}

/// One edge of a HATCH edge path, as vertices from its start to its end (both are corners; an
/// arc has a point on it between them). Lines and arcs stay exact; ellipses and splines are cut
/// into lines within `tol`. `None` for an edge type that cannot be read.
fn hatch_edge(
    i: &mut usize,
    next: &dyn Fn(&mut usize, i32) -> Option<f64>,
    tol: f64,
) -> Option<Vec<Vertex>> {
    let corner = |p: DVec2| Vertex::corner(p);
    match next(i, 72)? as i64 {
        1 => {
            let (x0, y0, x1, y1) = (next(i, 10)?, next(i, 20)?, next(i, 11)?, next(i, 21)?);
            Some(vec![corner(DVec2::new(x0, y0)), corner(DVec2::new(x1, y1))])
        }
        2 => {
            let (cx, cy, r, a0, a1) = (
                next(i, 10)?,
                next(i, 20)?,
                next(i, 40)?,
                next(i, 50)?,
                next(i, 51)?,
            );
            let ccw = next(i, 73).unwrap_or(1.0) != 0.0;
            if !(r.is_finite() && r > 0.0) {
                return None;
            }
            let c = DVec2::new(cx, cy);
            // A clockwise edge stores the complements of its angles (360 less the angle) in
            // counterclockwise order: the arc runs counterclockwise from 360 - end to 360 - start,
            // and the edge is that arc read backwards.
            let (a0, a1) = if ccw {
                (a0, a1)
            } else {
                (360.0 - a1, 360.0 - a0)
            };
            let (a0, a1) = (a0.to_radians(), a1.to_radians());
            let mut sweep = (a1 - a0).rem_euclid(TAU);
            if sweep == 0.0 || (a1 - a0).abs() >= TAU - 1e-9 {
                sweep = TAU;
            }
            let at = |a: f64| c + DVec2::from_angle(a) * r;
            // A full circle is two half arcs; one arc cannot close on itself.
            let parts = if sweep > PI + 1e-9 { 2 } else { 1 };
            let part = sweep / parts as f64;
            let mut v = vec![corner(at(a0))];
            for k in 0..parts {
                let st = a0 + part * k as f64;
                v.push(Vertex::new(at(st + part / 2.0), VertexKind::PointOnArc));
                v.push(corner(at(st + part)));
            }
            if !ccw {
                v.reverse();
            }
            Some(v)
        }
        3 => {
            let (cx, cy, mx, my, ratio, a0, a1) = (
                next(i, 10)?,
                next(i, 20)?,
                next(i, 11)?,
                next(i, 21)?,
                next(i, 40)?,
                next(i, 50)?,
                next(i, 51)?,
            );
            let ccw = next(i, 73).unwrap_or(1.0) != 0.0;
            let major = DVec2::new(mx, my);
            if major.length_squared() == 0.0 || !(ratio.is_finite() && ratio > 0.0) {
                return None;
            }
            // Angles of an ellipse edge are real angles, not parameters; a clockwise edge stores
            // complements, as an arc does.
            let (a0, a1) = if ccw {
                (a0, a1)
            } else {
                (360.0 - a1, 360.0 - a0)
            };
            let param = |deg: f64| {
                let a = deg.to_radians();
                (a.sin() / ratio).atan2(a.cos())
            };
            let full = (a1 - a0).abs() >= 360.0 - 1e-9 || a1 == a0;
            let t0 = param(a0);
            let mut sweep = (param(a1) - t0).rem_euclid(TAU);
            if full || sweep == 0.0 {
                sweep = TAU;
            }
            let mut pts = ellipse_points(DVec2::new(cx, cy), major, ratio, t0, sweep, tol);
            if !ccw {
                pts.reverse();
            }
            Some(corners(&pts))
        }
        4 => {
            let degree = next(i, 94)?.clamp(1.0, 16.0) as usize;
            let rational = next(i, 73).unwrap_or(0.0) != 0.0;
            let _periodic = next(i, 74);
            let (nk, nc) = (next(i, 95)? as usize, next(i, 96)? as usize);
            if nk > 100_000 || nc > 100_000 {
                return None;
            }
            let knots: Vec<f64> = (0..nk).map(|_| next(i, 40)).collect::<Option<_>>()?;
            let mut ctrl = Vec::with_capacity(nc);
            let mut weights = Vec::new();
            for _ in 0..nc {
                ctrl.push(DVec3::new(next(i, 10)?, next(i, 20)?, 0.0));
                if rational {
                    weights.push(next(i, 42)?);
                }
            }
            let pts = bspline_points(degree, &ctrl, &knots, &weights, tol)?;
            Some(corners(
                &pts.iter().map(|p| p.truncate()).collect::<Vec<_>>(),
            ))
        }
        _ => None,
    }
}

/// Reads the optional bulge (42) that directly follows a hatch polyline vertex.
fn peek_bulge(items: &[(i32, String)], i: &mut usize) -> f64 {
    match items.get(*i) {
        Some((42, v)) => {
            *i += 1;
            parse::parse_f64(v).unwrap_or(0.0)
        }
        _ => 0.0,
    }
}

/// Cox-de Boor basis functions of degree `p` at `t` in knot span `span`.
fn basis(span: usize, t: f64, p: usize, knots: &[f64]) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    n[0] = 1.0;
    for j in 1..=p {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let denom = right[r + 1] + left[j - r];
            let temp = if denom == 0.0 { 0.0 } else { n[r] / denom };
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

/// Points on a (rational) B-spline from its control points, knots and weights. The curve is cut
/// until no chord is more than `tol` from the curve (in drawing units). `None` when the data do
/// not make a spline.
fn bspline_points(
    degree: usize,
    ctrl: &[DVec3],
    knots: &[f64],
    weights: &[f64],
    tol: f64,
) -> Option<Vec<DVec3>> {
    let n = ctrl.len();
    let valid = n > degree
        && knots.len() == n + degree + 1
        && knots.windows(2).all(|w| w[0] <= w[1])
        && knots[degree] < knots[n]
        && (weights.is_empty() || weights.len() == n);
    if !valid {
        return None;
    }
    let at = |t: f64| -> Option<DVec3> {
        // The last knot span that starts at or before t, but not past the end.
        let mut span = degree;
        while span + 1 < n && knots[span + 1] <= t {
            span += 1;
        }
        let nb = basis(span, t, degree, knots);
        let (mut acc, mut wsum) = (DVec3::ZERO, 0.0);
        for (j, nj) in nb.iter().enumerate() {
            let idx = span - degree + j;
            let w = weights.get(idx).copied().unwrap_or(1.0);
            acc += ctrl[idx] * (nj * w);
            wsum += nj * w;
        }
        (wsum.abs() > 1e-300).then(|| acc / wsum)
    };
    let (t0, t1) = (knots[degree], knots[n]);
    // A first cut into equal steps, then the middle of every piece is checked against its chord,
    // and a piece that is too far off is cut in two.
    let base = (n * 4).clamp(16, 1000);
    let mut pts: Vec<DVec3> = Vec::new();
    let mut prev: Option<(f64, DVec3)> = None;
    for k in 0..=base {
        let t = t0 + (t1 - t0) * k as f64 / base as f64;
        let Some(p) = at(t) else { continue };
        if let Some((tp, pp)) = prev {
            refine(&at, (tp, pp), (t, p), tol.max(1e-9), 0, &mut pts);
        }
        prev = Some((t, p));
    }
    if let Some((_, p)) = prev {
        pts.push(p);
    }
    (pts.len() >= 2).then_some(pts)
}

/// Adds the start of the piece `a` to `b` of a curve, after cutting it where it is farther than
/// `tol` from its chord.
fn refine(
    at: &impl Fn(f64) -> Option<DVec3>,
    a: (f64, DVec3),
    b: (f64, DVec3),
    tol: f64,
    depth: u32,
    out: &mut Vec<DVec3>,
) {
    if depth < 14
        && out.len() < 200_000
        && let Some(pm) = at((a.0 + b.0) / 2.0)
    {
        let chord = b.1 - a.1;
        let len2 = chord.length_squared();
        let off = if len2 < 1e-300 {
            (pm - a.1).length()
        } else {
            (pm - a.1 - chord * ((pm - a.1).dot(chord) / len2).clamp(0.0, 1.0)).length()
        };
        if off > tol {
            let m = ((a.0 + b.0) / 2.0, pm);
            refine(at, a, m, tol, depth + 1, out);
            refine(at, m, b, tol, depth + 1, out);
            return;
        }
    }
    out.push(a.1);
}

/// A clamped uniform knot vector for `n` control points of the given degree.
fn open_uniform_knots(n: usize, degree: usize) -> Vec<f64> {
    let spans = (n - degree) as f64;
    (0..n + degree + 1)
        .map(|i| {
            if i <= degree {
                0.0
            } else if i >= n {
                spans
            } else {
                (i - degree) as f64
            }
        })
        .collect()
}

fn spline(e: &Entity, xf: &Xf, tol: f64, tally: &mut Tally) -> Result<Shape, Skip> {
    let degree = e.int(71).unwrap_or(3).clamp(1, 16) as usize;
    let point_list = |cx: i32| -> Vec<DVec3> {
        let mut pts: Vec<DVec3> = Vec::new();
        for (code, value) in &e.items {
            let Some(v) = parse::parse_f64(value) else {
                continue;
            };
            if *code == cx {
                pts.push(DVec3::new(v, 0.0, 0.0));
            } else if *code == cx + 10 {
                if let Some(p) = pts.last_mut() {
                    p.y = v;
                }
            } else if *code == cx + 20
                && let Some(p) = pts.last_mut()
            {
                p.z = v;
            }
        }
        pts
    };
    let ctrl = point_list(10);
    let fit = point_list(11);
    let knots = e.all_f(40);
    let weights = e.all_f(41);
    let closed = e.int(70).unwrap_or(0) & 1 != 0;
    let to_world = |p: DVec3| xf.apply(p.truncate()).extend(xf.z(p.z));
    let make = |pts: Vec<DVec3>, closed: bool| -> Shape {
        if pts.iter().any(|p| p.z != 0.0) {
            Shape::Polyline3d {
                points: pts.into_iter().map(to_world).collect(),
                closed,
            }
        } else {
            let pts: Vec<DVec2> = pts.into_iter().map(|p| to_world(p).truncate()).collect();
            Shape::Polyline {
                points: corners(&pts),
                closed,
            }
        }
    };
    // Without usable knots a clamped uniform knot vector is made (as the ezdxf reader does), and a
    // degree that is too high for the points is lowered. A closed periodic spline whose control
    // points do not close by themselves gets its first points again at the end and uniform knots.
    let periodic = e.int(70).unwrap_or(0) & 3 == 3;
    let mut made = false;
    let mut spline_pts = bspline_points(degree, &ctrl, &knots, &weights, tol);
    if spline_pts.is_none() && ctrl.len() >= 2 {
        let used = degree.min(ctrl.len() - 1);
        let w = if weights.len() == ctrl.len() {
            &weights[..]
        } else {
            &[]
        };
        if periodic && ctrl.first() != ctrl.last() && ctrl.len() > used {
            let mut wrapped = ctrl.clone();
            wrapped.extend_from_slice(&ctrl[..used]);
            let ww: Vec<f64> = if w.is_empty() {
                vec![]
            } else {
                w.iter().chain(&w[..used]).copied().collect()
            };
            let uniform: Vec<f64> = (0..wrapped.len() + used + 1).map(|i| i as f64).collect();
            spline_pts = bspline_points(used, &wrapped, &uniform, &ww, tol);
        } else {
            spline_pts = bspline_points(used, &ctrl, &open_uniform_knots(ctrl.len(), used), w, tol);
        }
        made = spline_pts.is_some();
    }
    if let Some(mut pts) = spline_pts {
        tally.splines += 1;
        if made {
            tally.spline_knots_made += 1;
        }
        let end_closed = closed
            && pts
                .first()
                .zip(pts.last())
                .is_some_and(|(a, b)| a.distance(*b) < 1e-9);
        if end_closed {
            pts.pop();
        }
        return Ok(make(pts, end_closed));
    }
    if fit.len() >= 2 && fit.iter().all(|p| p.z == 0.0) {
        tally.spline_fit += 1;
        let last = fit.len() - 1;
        let points = fit
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let kind = if closed || (i != 0 && i != last) {
                    VertexKind::Cubic
                } else {
                    VertexKind::Corner
                };
                Vertex::new(to_world(*p).truncate(), kind)
            })
            .collect();
        return Ok(Shape::Polyline { points, closed });
    }
    let pts = if ctrl.len() >= 2 { ctrl } else { fit };
    if pts.len() < 2 {
        return Err("spline without points");
    }
    tally.spline_polygon += 1;
    Ok(make(pts, closed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Editor;
    use kadakas_core::model::{Document, ObjectId};

    /// Builds a DXF text from entity bodies.
    pub(crate) fn dxf_text(header: &str, tables: &str, blocks: &str, entities: &str) -> String {
        format!(
            "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n{tables}0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
        )
    }

    fn run(text: &str, opts: &ImportOptions) -> (Document, crate::Report) {
        let doc = Document::with_sequential_ids();
        let src = Source {
            name: "t".into(),
            data: text.as_bytes().to_vec(),
            extra: Default::default(),
        };
        let r = import(&src, &Context::of(&doc), opts, None).unwrap();
        let mut ed = Editor::new(doc);
        ed.execute(&r.command).unwrap();
        (ed.doc().clone(), r.report)
    }

    fn shapes(doc: &Document) -> Vec<&Shape> {
        doc.objects.values().map(|o| &o.shape).collect()
    }

    #[test]
    fn lines_circles_arcs_and_points() {
        let ents = "0\nLINE\n8\nWalls\n10\n0\n20\n0\n11\n1000\n21\n0\n\
0\nLINE\n8\nWalls\n10\n0\n20\n0\n30\n5\n11\n1000\n21\n0\n31\n9\n\
0\nCIRCLE\n8\nWalls\n10\n100\n20\n200\n40\n50\n\
0\nARC\n8\nTrees\n10\n0\n20\n0\n40\n10\n50\n90\n51\n180\n\
0\nPOINT\n10\n1\n20\n2\n30\n3\n\
0\nPOINT\n10\n1\n20\n2\n\
0\nDIMENSION\n8\nWalls\n";
        let (doc, report) = run(
            &dxf_text("9\n$INSUNITS\n70\n4\n", "", "", ents),
            &ImportOptions::default(),
        );
        assert_eq!(report.skipped_total(), 1);
        assert_eq!(report.skipped[0].entity, "DIMENSION");
        let s = shapes(&doc);
        assert_eq!(s.len(), 6);
        assert!(matches!(s[0], Shape::Line { b, .. } if *b == DVec2::new(1000.0, 0.0)));
        assert!(matches!(s[1], Shape::Polyline3d { points, .. } if points[1].z == 9.0));
        assert!(matches!(s[2], Shape::Circle { radius, .. } if *radius == 50.0));
        assert!(
            matches!(s[3], Shape::Arc { start, end, .. } if (*start - PI / 2.0).abs() < 1e-12 && (*end - PI).abs() < 1e-12)
        );
        assert!(matches!(s[4], Shape::Locus3d { p } if p.z == 3.0));
        assert!(matches!(s[5], Shape::Locus3d { p } if p.z == 0.0));
        // Layers become classes on one layer of their own, apart from the design work.
        assert_eq!(doc.layers.len(), 2);
        assert_eq!(doc.layers[1].name, "t");
        assert!(doc.objects.values().all(|o| o.layer == doc.layers[1].id));
        let names: Vec<_> = doc.classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["None", "Walls", "Trees", "0"]);
        let walls = doc.classes.iter().find(|c| c.name == "Walls").unwrap().id;
        assert_eq!(doc.objects.values().filter(|o| o.class == walls).count(), 3);
    }

    #[test]
    fn points_option_and_layers_mode() {
        let tables = "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nGreen\n62\n3\n370\n35\n0\nLAYER\n2\nOff\n62\n-1\n0\nLAYER\n2\nFrozen\n70\n1\n0\nENDTAB\n";
        let ents = "0\nPOINT\n8\nGreen\n10\n1\n20\n2\n\
0\nPOINT\n8\nGreen\n10\n1\n20\n2\n30\n4\n\
0\nLINE\n8\nOff\n10\n0\n20\n0\n11\n1\n21\n1\n\
0\nLINE\n8\nFrozen\n62\n1\n370\n70\n10\n0\n20\n0\n11\n1\n21\n1\n";
        let opts = ImportOptions {
            points: PointMode::WithZ,
            layer_mode: LayerMode::Layers,
            ..Default::default()
        };
        let (doc, report) = run(&dxf_text("", tables, "", ents), &opts);
        assert_eq!(doc.objects.len(), 3);
        assert_eq!(report.skipped_total(), 1);
        let names: Vec<_> = doc
            .layers
            .iter()
            .map(|l| (l.name.as_str(), l.visible))
            .collect();
        assert_eq!(
            names,
            [
                ("Layer 1", true),
                ("Green", true),
                ("Off", false),
                ("Frozen", false)
            ]
        );
        let p = doc.objects.values().next().unwrap();
        assert_eq!(p.attrs.pen, Some(Color([0, 255, 0, 255])));
        assert_eq!(p.attrs.line_weight, Some(0.35));
        let last = doc.objects.values().last().unwrap();
        assert_eq!(last.attrs.pen, Some(Color([255, 0, 0, 255])));
        assert_eq!(last.attrs.line_weight, Some(0.7));
        assert_eq!(doc.classes.len(), 1);
    }

    #[test]
    fn classes_get_layer_colors_and_visibility() {
        let tables = "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nGreen\n62\n3\n420\n16711935\n370\n50\n0\nLAYER\n2\nOff\n62\n-1\n0\nENDTAB\n";
        let ents = "0\nLINE\n8\nGreen\n10\n0\n20\n0\n11\n1\n21\n1\n0\nLINE\n8\nOff\n10\n0\n20\n0\n11\n1\n21\n1\n";
        let opts = ImportOptions {
            layer: Some("Imported".into()),
            ..Default::default()
        };
        let (doc, _) = run(&dxf_text("", tables, "", ents), &opts);
        let green = doc.classes.iter().find(|c| c.name == "Green").unwrap();
        assert_eq!(green.attrs.pen, Some(Color([255, 0, 255, 255])));
        assert_eq!(green.attrs.line_weight, Some(0.5));
        assert!(
            !doc.classes
                .iter()
                .find(|c| c.name == "Off")
                .unwrap()
                .visible
        );
        assert_eq!(doc.layers.len(), 2);
        assert_eq!(doc.layers[1].name, "Imported");
        assert!(doc.objects.values().all(|o| o.layer == doc.layers[1].id));
    }

    #[test]
    fn lwpolyline_bulges_become_point_on_arc() {
        // Half circle from (0,0) to (2000,0), bulge 1, then a straight closing edge.
        let ents = "0\nLWPOLYLINE\n90\n2\n70\n1\n10\n0\n20\n0\n42\n1\n10\n2000\n20\n0\n";
        let (doc, _) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let Shape::Polyline { points, closed } = shapes(&doc)[0] else {
            panic!()
        };
        assert!(closed);
        assert_eq!(points.len(), 3);
        assert_eq!(points[1].kind, VertexKind::PointOnArc);
        assert!(points[1].p.distance(DVec2::new(1000.0, -1000.0)) < 1e-9);
        // The curve is the exact half circle.
        let flat = kadakas_core::curve::flatten(points, true, 0.01);
        for p in flat.iter().filter(|p| p.y < -1e-6) {
            assert!((p.distance(DVec2::new(1000.0, 0.0)) - 1000.0).abs() < 0.02);
        }
        // A negative bulge goes the other way.
        let ents = "0\nLWPOLYLINE\n90\n2\n70\n0\n10\n0\n20\n0\n42\n-1\n10\n2000\n20\n0\n";
        let (doc, _) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let Shape::Polyline { points, .. } = shapes(&doc)[0] else {
            panic!()
        };
        assert!(points[1].p.distance(DVec2::new(1000.0, 1000.0)) < 1e-9);
        assert_eq!(points.len(), 3);
    }

    #[test]
    fn lwpolyline_elevation_flip_and_errors() {
        let ents = "0\nLWPOLYLINE\n70\n0\n38\n12\n10\n0\n20\n0\n10\n5\n20\n0\n\
0\nLWPOLYLINE\n10\n0\n20\n0\n\
0\nLWPOLYLINE\n70\n0\n210\n0\n220\n0\n230\n-1\n10\n1\n20\n0\n10\n3\n20\n0\n\
0\nCIRCLE\n210\n1\n220\n0\n230\n0\n10\n0\n20\n0\n40\n1\n\
0\nCIRCLE\n10\n0\n20\n0\n";
        let (doc, report) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let s = shapes(&doc);
        assert_eq!(s.len(), 3);
        assert!(matches!(s[0], Shape::Polyline3d { points, .. } if points[0].z == 12.0));
        let Shape::Polyline { points, .. } = s[1] else {
            panic!()
        };
        assert_eq!(points[0].p, DVec2::new(-1.0, 0.0));
        // The circle in the plane x = 0 stands up as a closed 3D polyline.
        let Shape::Polyline3d { points, closed } = s[2] else {
            panic!()
        };
        assert!(
            *closed
                && points
                    .iter()
                    .all(|p| p.x.abs() < 1e-9 && p.y.abs() <= 1.0 + 1e-9)
        );
        assert!(points.iter().any(|p| p.z.abs() > 0.99));
        assert_eq!(report.skipped_total(), 2);
    }

    #[test]
    fn polyline_2d_3d_and_mesh() {
        let ents = "0\nPOLYLINE\n70\n1\n0\nVERTEX\n10\n0\n20\n0\n42\n0.5\n0\nVERTEX\n10\n10\n20\n0\n0\nVERTEX\n10\n10\n20\n10\n0\nSEQEND\n\
0\nPOLYLINE\n70\n8\n0\nVERTEX\n10\n0\n20\n0\n30\n1\n70\n32\n0\nVERTEX\n10\n10\n20\n0\n30\n2\n70\n32\n0\nSEQEND\n\
0\nPOLYLINE\n70\n16\n0\nVERTEX\n10\n0\n20\n0\n0\nSEQEND\n\
0\nPOLYLINE\n70\n4\n0\nVERTEX\n10\n0\n20\n0\n0\nVERTEX\n10\n5\n20\n5\n70\n16\n0\nVERTEX\n10\n9\n20\n0\n0\nSEQEND\n";
        let (doc, report) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let s = shapes(&doc);
        assert_eq!(s.len(), 3);
        assert!(matches!(s[0], Shape::Polyline { points, closed: true } if points.len() == 4));
        assert!(matches!(s[1], Shape::Polyline3d { points, closed: false } if points[1].z == 2.0));
        assert!(matches!(s[2], Shape::Polyline { points, .. } if points.len() == 2));
        assert_eq!(report.skipped_total(), 1);
    }

    #[test]
    fn ellipse_and_spline() {
        let ents = "0\nELLIPSE\n10\n0\n20\n0\n11\n1000\n21\n0\n40\n0.5\n41\n0\n42\n6.283185307179586\n\
0\nELLIPSE\n10\n0\n20\n0\n11\n1000\n21\n0\n40\n0.5\n41\n0\n42\n1.5707963267948966\n\
0\nELLIPSE\n10\n0\n20\n0\n11\n0\n21\n0\n40\n0.5\n\
0\nSPLINE\n70\n8\n71\n2\n72\n6\n73\n3\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n10\n0\n20\n0\n10\n500\n20\n1000\n10\n1000\n20\n0\n\
0\nSPLINE\n70\n0\n71\n3\n11\n0\n21\n0\n11\n100\n21\n100\n11\n200\n21\n0\n\
0\nSPLINE\n70\n0\n10\n0\n20\n0\n10\n7\n20\n7\n30\n7\n\
0\nSPLINE\n70\n0\n\
0\nSPLINE\n70\n8\n71\n1\n72\n4\n73\n2\n40\n0\n40\n0\n40\n1\n40\n1\n10\n0\n20\n0\n10\n10\n20\n0\n41\n1\n41\n1\n";
        let (doc, report) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let s = shapes(&doc);
        assert_eq!(report.skipped_total(), 2, "{:?}", report.skipped);
        let Shape::Polyline { points, closed } = s[0] else {
            panic!()
        };
        assert!(closed);
        let flat = kadakas_core::curve::positions(points);
        for p in &flat {
            let v = (p.x / 1000.0).powi(2) + (p.y / 500.0).powi(2);
            assert!((v - 1.0).abs() < 0.01, "{p:?}");
        }
        assert!(matches!(s[1], Shape::Polyline { closed: false, .. }));
        // Quadratic B-spline through three control points: the midpoint is (500, 500).
        let Shape::Polyline { points, .. } = s[2] else {
            panic!()
        };
        let mid = points
            .iter()
            .map(|v| v.p)
            .min_by(|a, b| (a.x - 500.0).abs().total_cmp(&(b.x - 500.0).abs()))
            .unwrap();
        assert!((mid.y - 500.0).abs() < 5.0, "{mid:?}");
        let Shape::Polyline { points, .. } = s[3] else {
            panic!()
        };
        assert_eq!(points[1].kind, VertexKind::Cubic);
        assert_eq!(points[0].kind, VertexKind::Corner);
        assert!(matches!(s[4], Shape::Polyline3d { .. }));
        assert!(matches!(s[5], Shape::Polyline { points, .. } if points.len() > 2));
    }

    #[test]
    fn text_and_mtext() {
        let ents = "0\nTEXT\n8\nT\n10\n100\n20\n200\n40\n250\n50\n30\n1\n%%c10 %%p2 %%d \\U+00E4\n\
0\nTEXT\n10\n0\n20\n0\n40\n100\n72\n1\n11\n50\n21\n60\n1\nAligned\n\
0\nMTEXT\n10\n0\n20\n0\n40\n180\n3\n{\\fArial|b0;Hello}\\P\n1\nsecond \\S1/2; line\\~x\\L\\\\\n\
0\nTEXT\n1\n   \n0\nTEXT\n10\n0\n20\n0\n1\nNo height\n";
        let (doc, report) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let s = shapes(&doc);
        assert_eq!(report.skipped_total(), 1);
        assert!(
            matches!(s[0], Shape::Text { text, height, at } if text == "\u{d8}10 \u{b1}2 \u{b0} \u{e4}" && *height == 250.0 && *at == DVec2::new(100.0, 200.0))
        );
        assert!(matches!(s[1], Shape::Text { at, .. } if *at == DVec2::new(50.0, 60.0)));
        assert!(
            matches!(s[2], Shape::Text { text, height, .. } if text == "Hello\nsecond 1/2 line x\\" && *height == 180.0)
        );
        assert!(matches!(s[3], Shape::Text { height, .. } if *height == 250.0));
        let first = doc.objects.values().next().unwrap();
        assert_eq!(first.data["rotation_deg"], 30.0);
        assert_eq!(
            doc.objects
                .values()
                .filter(|o| o.data.contains_key("rotation_deg"))
                .count(),
            1
        );
        assert!(report.notes.iter().all(|n| !n.contains("rotation")));
    }

    #[test]
    fn insert_explodes_with_transform() {
        let blocks = "0\nBLOCK\n2\nB\n10\n10\n20\n0\n0\nLINE\n8\n0\n10\n10\n20\n0\n11\n20\n21\n0\n\
0\nCIRCLE\n8\n0\n10\n10\n20\n0\n40\n2\n0\nARC\n8\nFixed\n10\n10\n20\n0\n40\n3\n50\n0\n51\n90\n\
0\nINSERT\n2\nInner\n10\n0\n20\n0\n0\nENDBLK\n\
0\nBLOCK\n2\nInner\n0\nPOINT\n10\n0\n20\n0\n30\n1\n0\nENDBLK\n";
        let ents = "0\nINSERT\n8\nSite\n2\nB\n10\n100\n20\n100\n41\n2\n42\n2\n50\n90\n\
0\nINSERT\n8\nSite\n2\nB\n10\n0\n20\n0\n70\n2\n71\n2\n44\n1000\n45\n500\n\
0\nINSERT\n2\nMissing\n\
0\nINSERT\n2\nB\n210\n1\n220\n1\n230\n0\n\
0\nINSERT\n2\nB\n230\n-1\n10\n5\n20\n0\n";
        let (doc, report) = run(&dxf_text("", "", blocks, ents), &ImportOptions::default());
        // 3 shapes + inner point = 4 per insert: 1 + 4 + 1 (tilted) + 1 (flipped) = 7 inserts of 4.
        assert_eq!(doc.objects.len(), 4 * 7);
        assert_eq!(report.skipped_total(), 1);
        let first = &shapes(&doc)[0];
        // Line (10,0)-(20,0) about base (10,0), scale 2, rotate 90 deg, at (100,100).
        assert!(
            matches!(first, Shape::Line { a, b } if a.distance(DVec2::new(100.0, 100.0)) < 1e-9 && b.distance(DVec2::new(100.0, 120.0)) < 1e-9),
            "{first:?}"
        );
        let Shape::Circle { radius, .. } = shapes(&doc)[1] else {
            panic!()
        };
        assert_eq!(*radius, 4.0);
        let Shape::Arc { start, end, .. } = shapes(&doc)[2] else {
            panic!()
        };
        assert!((start - PI / 2.0).abs() < 1e-9 && (end - PI).abs() < 1e-9);
        // Layer "0" inside a block takes the layer of the insert, other layers stay.
        let site = doc.classes.iter().find(|c| c.name == "Site").unwrap().id;
        let fixed = doc.classes.iter().find(|c| c.name == "Fixed").unwrap().id;
        assert_eq!(doc.objects.values().filter(|o| o.class == fixed).count(), 7);
        assert!(doc.objects.values().filter(|o| o.class == site).count() >= 6);
        // The flipped insert mirrors X: the line starts at (5,0) and goes to (-5,0)+...
        let last_line = doc
            .objects
            .values()
            .rfind(|o| matches!(o.shape, Shape::Line { .. }))
            .unwrap();
        assert!(
            matches!(&last_line.shape, Shape::Line { a, b } if a.distance(DVec2::new(-5.0, 0.0)) < 1e-9 && b.distance(DVec2::new(-15.0, 0.0)) < 1e-9),
            "{:?}",
            last_line.shape
        );
        let opts = ImportOptions {
            explode_blocks: false,
            ..Default::default()
        };
        let (doc, report) = run(&dxf_text("", "", blocks, ents), &opts);
        assert_eq!(doc.objects.len(), 0);
        assert_eq!(report.skipped_total(), 5);
    }

    #[test]
    fn mirrored_arcs_and_non_uniform_scale() {
        let blocks = "0\nBLOCK\n2\nA\n0\nARC\n10\n0\n20\n0\n40\n10\n50\n0\n51\n90\n0\nCIRCLE\n10\n0\n20\n0\n40\n10\n\
0\nLWPOLYLINE\n70\n0\n10\n0\n20\n0\n42\n1\n10\n10\n20\n0\n0\nENDBLK\n";
        // Mirror in X (scale -1): the arc quadrant moves from the first to the second quadrant.
        let ents = "0\nINSERT\n2\nA\n41\n-1\n0\nINSERT\n2\nA\n41\n3\n";
        let (doc, _) = run(&dxf_text("", "", blocks, ents), &ImportOptions::default());
        let s = shapes(&doc);
        let Shape::Arc { start, end, .. } = s[0] else {
            panic!()
        };
        assert!(
            (start - PI / 2.0).abs() < 1e-9 && (end - PI).abs() < 1e-9,
            "{start} {end}"
        );
        // The mirrored bulge polyline still passes the same midpoint mirrored.
        let Shape::Polyline { points, .. } = s[2] else {
            panic!()
        };
        assert!(points[1].p.distance(DVec2::new(-5.0, -5.0)) < 1e-9);
        // Non-uniform scale: circle and arc become polylines.
        assert!(matches!(s[3], Shape::Polyline { closed: false, .. }));
        assert!(matches!(s[4], Shape::Polyline { closed: true, .. }));
        let Shape::Polyline { points, .. } = s[5] else {
            panic!()
        };
        assert!(points.len() > 3);
        assert!(points.iter().all(|v| v.kind == VertexKind::Corner));
    }

    #[test]
    fn hatches() {
        let ents = "0\nHATCH\n8\nH\n70\n1\n420\n255\n91\n2\n\
92\n7\n72\n0\n73\n1\n93\n3\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n97\n0\n\
92\n1\n93\n5\n72\n1\n10\n0\n20\n0\n11\n5\n21\n0\n72\n1\n10\n5\n20\n0\n11\n5\n21\n5\n72\n1\n10\n5\n20\n5\n11\n0\n21\n5\n72\n2\n10\n0\n20\n0\n40\n5\n50\n90\n51\n180\n73\n1\n72\n1\n10\n-5\n20\n0\n11\n0\n21\n0\n\
0\nHATCH\n70\n0\n91\n1\n92\n7\n72\n1\n73\n1\n93\n3\n10\n0\n20\n0\n42\n1\n10\n10\n20\n0\n42\n0\n10\n10\n20\n10\n\
0\nHATCH\n70\n0\n91\n1\n92\n1\n93\n1\n72\n3\n\
0\nHATCH\n70\n0\n";
        let (doc, report) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let s = shapes(&doc);
        // An unreadable path is reported once; a hatch with no path at all is reported once.
        assert_eq!(report.skipped_total(), 2, "{:?}", report.skipped);
        assert_eq!(s.len(), 3);
        let first = doc.objects.values().next().unwrap();
        assert_eq!(first.attrs.fill, Some(Color([0, 0, 255, 255])));
        assert!(matches!(s[0], Shape::Polyline { closed: true, .. }));
        let third = doc.objects.values().nth(2).unwrap();
        assert_eq!(third.attrs.fill, None);
        let opts = ImportOptions {
            hatches: false,
            ..Default::default()
        };
        let (doc, _) = run(&dxf_text("", "", "", ents), &opts);
        assert!(doc.objects.is_empty());
    }

    #[test]
    fn a_polyline_that_ends_where_it_starts_is_closed() {
        let ents = "0\nLWPOLYLINE\n70\n0\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n10\n0\n20\n0\n\
0\nPOLYLINE\n70\n0\n0\nVERTEX\n10\n0\n20\n0\n0\nVERTEX\n10\n10\n20\n0\n0\nVERTEX\n10\n10\n20\n10\n0\nVERTEX\n10\n0\n20\n0\n0\nSEQEND\n\
0\nPOLYLINE\n70\n8\n0\nVERTEX\n10\n0\n20\n0\n30\n1\n0\nVERTEX\n10\n10\n20\n0\n30\n1\n0\nVERTEX\n10\n10\n20\n10\n30\n1\n0\nVERTEX\n10\n0\n20\n0\n30\n1\n0\nSEQEND\n\
0\nPOLYLINE\n70\n8\n0\nVERTEX\n10\n0\n20\n0\n30\n1\n0\nVERTEX\n10\n10\n20\n0\n30\n1\n0\nVERTEX\n10\n10\n20\n10\n30\n1\n0\nVERTEX\n10\n0\n20\n0\n30\n5\n0\nSEQEND\n\
0\nLWPOLYLINE\n70\n0\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n";
        let (doc, report) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        let s = shapes(&doc);
        assert_eq!(s.len(), 5);
        assert!(matches!(s[0], Shape::Polyline { points, closed: true } if points.len() == 3));
        assert!(matches!(s[1], Shape::Polyline { points, closed: true } if points.len() == 3));
        assert!(matches!(s[2], Shape::Polyline3d { points, closed: true } if points.len() == 3));
        // The last point is higher: not the same point.
        assert!(matches!(s[3], Shape::Polyline3d { points, closed: false } if points.len() == 4));
        assert!(matches!(s[4], Shape::Polyline { closed: false, .. }));
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains(REPEATED_NOTE) && n.contains("largest gap")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn a_gap_is_not_closed_even_far_from_the_origin() {
        // Survey coordinates: 6,588,000 m north is 6.588e9 mm. A gap of 5 mm there is a gap, and
        // the same polyline with the very same end point is closed.
        let tri = |gap: f64| {
            format!(
                "0\nLWPOLYLINE\n70\n0\n10\n650000000\n20\n6588000000\n10\n650010000\n20\n6588000000\n10\n650010000\n20\n6588010000\n10\n650000000\n20\n{}\n",
                6588000000.0 + gap
            )
        };
        let (doc, report) = run(&dxf_text("", "", "", &tri(5.0)), &ImportOptions::default());
        assert!(
            matches!(shapes(&doc)[0], Shape::Polyline { closed: false, points } if points.len() == 4)
        );
        assert!(!report.notes.iter().any(|n| n.contains(REPEATED_NOTE)));
        let (doc, report) = run(&dxf_text("", "", "", &tri(0.0)), &ImportOptions::default());
        assert!(
            matches!(shapes(&doc)[0], Shape::Polyline { closed: true, points } if points.len() == 3)
        );
        assert!(report.notes.iter().any(|n| n.contains(REPEATED_NOTE)));
    }

    #[test]
    fn xrefs_and_paper_space_are_reported() {
        let blocks = "0\nBLOCK\n2\nXR\n70\n4\n1\nsite.dxf\n10\n0\n20\n0\n0\nENDBLK\n\
0\nBLOCK\n2\nXR2\n70\n8\n10\n0\n20\n0\n0\nENDBLK\n\
0\nBLOCK\n2\n*Paper_Space\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n";
        let ents = "0\nINSERT\n2\nXR\n8\n0\n10\n0\n20\n0\n0\nINSERT\n2\nXR2\n8\n0\n10\n0\n20\n0\n";
        // With the option off, paper space is counted and skipped.
        let off = ImportOptions {
            paper_space: false,
            ..ImportOptions::default()
        };
        let (doc, report) = run(&dxf_text("", "", blocks, ents), &off);
        assert!(doc.objects.is_empty());
        let why: Vec<&str> = report.skipped.iter().map(|s| s.reason.as_str()).collect();
        assert!(
            why.iter()
                .any(|w| w.contains("XREF") && w.contains("'XR'") && w.contains("site.dxf"))
        );
        assert!(
            why.iter()
                .any(|w| w.contains("'XR2'") && w.contains("unknown path"))
        );
        assert!(
            report
                .skipped
                .iter()
                .any(|s| s.entity == "LINE" && s.reason == "in paper space")
        );
    }

    #[test]
    fn hatch_edge_paths_keep_arcs_and_read_ellipses_and_splines() {
        let circle = "92\n1\n93\n1\n72\n2\n10\n0\n20\n0\n40\n5\n50\n0\n51\n360\n73\n1\n97\n0\n";
        let ellipse = "92\n1\n93\n1\n72\n3\n10\n0\n20\n0\n11\n1000\n21\n0\n40\n0.5\n50\n0\n51\n360\n73\n1\n97\n0\n";
        // A spline, and the line back along its chord written the other way round.
        let spline = "92\n1\n93\n2\n72\n4\n94\n3\n73\n0\n74\n0\n95\n8\n96\n4\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n\
10\n0\n20\n0\n10\n3\n20\n5\n10\n7\n20\n5\n10\n10\n20\n0\n72\n1\n10\n0\n20\n0\n11\n10\n21\n0\n97\n0\n";
        let rational = "92\n1\n93\n2\n72\n4\n94\n2\n73\n1\n74\n0\n95\n6\n96\n3\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n\
10\n0\n20\n0\n42\n1\n10\n5\n20\n5\n42\n2\n10\n10\n20\n0\n42\n1\n72\n1\n10\n10\n20\n0\n11\n0\n21\n0\n97\n0\n";
        let bad = "92\n1\n93\n1\n72\n9\n97\n0\n";
        let ents =
            format!("0\nHATCH\n8\nH\n70\n0\n91\n5\n{circle}{ellipse}{spline}{rational}{bad}");
        let (doc, report) = run(&dxf_text("", "", "", &ents), &ImportOptions::default());
        let polys: Vec<&Vec<Vertex>> = shapes(&doc)
            .into_iter()
            .map(|s| match s {
                Shape::Polyline {
                    points,
                    closed: true,
                } => points,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(polys.len(), 4, "{:?}", report.skipped);
        // The circle is two exact half arcs, not many chords.
        let arcs = |p: &Vec<Vertex>| {
            p.iter()
                .filter(|v| v.kind == VertexKind::PointOnArc)
                .count()
        };
        assert_eq!((polys[0].len(), arcs(polys[0])), (4, 2));
        assert!(polys[1].len() > 16 && arcs(polys[1]) == 0);
        assert!(polys[2].len() > 4 && polys[3].len() > 4);
        assert_eq!(report.skipped_total(), 1);
        assert!(report.skipped[0].reason.contains("boundary path"));
        // The ellipse of 1000 by 500 has the area pi * 500,000 (the chords are within 2 mm).
        let pts: Vec<DVec2> = polys[1].iter().map(|v| v.p).collect();
        let area = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| a.x * b.y - b.x * a.y)
            .sum::<f64>()
            .abs()
            / 2.0;
        assert!(
            (area / (std::f64::consts::PI * 500_000.0) - 1.0).abs() < 0.005,
            "{area}"
        );
    }

    #[test]
    fn hatch_arcs_that_run_clockwise_store_complementary_angles() {
        // A clockwise arc from 120 to 30 degrees (radius 5) is stored as 50 = 240, 51 = 330, and
        // closed by the chord. The arc passes 75 degrees.
        let cw = "92\n1\n93\n2\n72\n2\n10\n0\n20\n0\n40\n5\n50\n240\n51\n330\n73\n0\n\
72\n1\n10\n4.330127018922193\n20\n2.5\n11\n-2.5\n21\n4.330127018922193\n97\n0\n";
        // The same arc counterclockwise from 30 to 120 degrees, with the chord the other way.
        let ccw = "92\n1\n93\n2\n72\n2\n10\n0\n20\n0\n40\n5\n50\n30\n51\n120\n73\n1\n\
72\n1\n10\n-2.5\n20\n4.330127018922193\n11\n4.330127018922193\n21\n2.5\n97\n0\n";
        let ents = format!("0\nHATCH\n8\nH\n70\n0\n91\n1\n{cw}0\nHATCH\n8\nH\n70\n0\n91\n1\n{ccw}");
        let (doc, report) = run(&dxf_text("", "", "", &ents), &ImportOptions::default());
        assert_eq!(doc.objects.len(), 2, "{:?}", report.skipped);
        for s in shapes(&doc) {
            let Shape::Polyline {
                points,
                closed: true,
            } = s
            else {
                panic!()
            };
            let mid = points
                .iter()
                .find(|v| v.kind == VertexKind::PointOnArc)
                .unwrap();
            let want = DVec2::from_angle(75f64.to_radians()) * 5.0;
            assert!((mid.p - want).length() < 1e-9, "{:?}", mid.p);
            // Both give the same half moon: an arc of 90 degrees and its chord.
            let pts: Vec<DVec2> = points.iter().map(|v| v.p).collect();
            assert_eq!(pts.len(), 3);
        }
    }

    #[test]
    fn hatch_ellipse_edges_use_angles_and_complements() {
        // An ellipse of 10 by 5. The real angle 45 degrees is the point (4.472, 4.472), not the
        // parameter 45 degrees (7.07, 3.54). Clockwise from 45 to 0 is stored as 315 and 360.
        let quarter = |ccw: bool| {
            let (a, b) = if ccw { (0, 45) } else { (315, 360) };
            format!(
                "92\n1\n93\n3\n72\n3\n10\n0\n20\n0\n11\n10\n21\n0\n40\n0.5\n50\n{a}\n51\n{b}\n73\n{}\n\
72\n1\n10\n4.47213595499958\n20\n4.47213595499958\n11\n0\n21\n0\n\
72\n1\n10\n0\n20\n0\n11\n10\n21\n0\n97\n0\n",
                ccw as i32
            )
        };
        let ents = format!(
            "0\nHATCH\n8\nH\n70\n0\n91\n1\n{}0\nHATCH\n8\nH\n70\n0\n91\n1\n{}",
            quarter(true),
            quarter(false)
        );
        let opts = ImportOptions {
            tolerance: 0.01,
            ..Default::default()
        };
        let (doc, report) = run(&dxf_text("", "", "", &ents), &opts);
        assert_eq!(doc.objects.len(), 2, "{:?}", report.skipped);
        for s in shapes(&doc) {
            let Shape::Polyline {
                points,
                closed: true,
            } = s
            else {
                panic!()
            };
            let pts: Vec<DVec2> = points.iter().map(|v| v.p).collect();
            assert!(
                pts.iter()
                    .any(|p| (*p - DVec2::new(4.47213595499958, 4.47213595499958)).length() < 1e-6)
            );
            assert!(
                pts.iter()
                    .any(|p| (*p - DVec2::new(10.0, 0.0)).length() < 1e-6)
            );
            // Every point of the arc is on the ellipse.
            for p in &pts[..pts.len() - 2] {
                assert!(
                    ((p.x / 10.0).powi(2) + (p.y / 5.0).powi(2) - 1.0).abs() < 1e-9 || p.y == 0.0
                );
            }
        }
    }

    #[test]
    fn hatch_curves_are_cut_within_the_tolerance_in_the_units_of_the_file() {
        // A circle-like ellipse of radius 1 meter written in a file in meters. The tolerance is
        // 2 mm, so the chords must be much finer than 2 units.
        let ell = "92\n1\n93\n1\n72\n3\n10\n0\n20\n0\n11\n1\n21\n0\n40\n1\n50\n0\n51\n360\n73\n1\n97\n0\n";
        let ents = format!("0\nHATCH\n8\nH\n70\n0\n91\n1\n{ell}");
        let (doc, _) = run(
            &dxf_text("9\n$INSUNITS\n70\n6\n", "", "", &ents),
            &ImportOptions::default(),
        );
        let Shape::Polyline { points, .. } = shapes(&doc)[0] else {
            panic!()
        };
        // Radius 1000 mm with 2 mm sag: about 2 * acos(1 - 0.002) = 0.1267 rad per chord.
        assert!(points.len() >= 45, "{}", points.len());
    }

    #[test]
    fn hatch_paths_that_do_not_join_or_are_rejected_are_reported_once_each() {
        // Two lines that do not meet (a gap of 5), a polyline path with one point, and a good one.
        let gap = "92\n1\n93\n2\n72\n1\n10\n0\n20\n0\n11\n10\n21\n0\n72\n1\n10\n10\n20\n5\n11\n0\n21\n5\n97\n0\n";
        let one = "92\n7\n72\n0\n73\n1\n93\n1\n10\n0\n20\n0\n97\n0\n";
        let good =
            "92\n7\n72\n0\n73\n1\n93\n3\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n97\n0\n";
        let ents = format!(
            "0\nHATCH\n8\nH\n70\n0\n91\n3\n{gap}{one}{good}0\nHATCH\n8\nH\n70\n0\n91\n1\n{gap}"
        );
        let (doc, report) = run(&dxf_text("", "", "", &ents), &ImportOptions::default());
        assert_eq!(doc.objects.len(), 1);
        // 2 bad paths of the first hatch, 1 of the second: no extra line for the empty hatch.
        assert_eq!(report.skipped_total(), 3, "{:?}", report.skipped);
        // The first edge may be written the wrong way round.
        let rev = "92\n1\n93\n3\n72\n1\n10\n10\n20\n0\n11\n0\n21\n0\n72\n1\n10\n10\n20\n0\n11\n10\n21\n10\n72\n1\n10\n10\n20\n10\n11\n0\n21\n0\n97\n0\n";
        let ents = format!("0\nHATCH\n8\nH\n70\n0\n91\n1\n{rev}");
        let (doc, report) = run(&dxf_text("", "", "", &ents), &ImportOptions::default());
        assert_eq!(doc.objects.len(), 1, "{:?}", report.skipped);
        let Shape::Polyline {
            points,
            closed: true,
        } = shapes(&doc)[0]
        else {
            panic!()
        };
        assert_eq!(points.len(), 3);
    }

    #[test]
    fn colors_and_weights_on_entities() {
        let blocks =
            "0\nBLOCK\n2\nB\n0\nLINE\n62\n0\n370\n-2\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n";
        let ents = "0\nLINE\n62\n1\n370\n25\n10\n0\n20\n0\n11\n1\n21\n1\n\
0\nLINE\n420\n65793\n10\n0\n20\n0\n11\n1\n21\n1\n\
0\nLINE\n62\n256\n10\n0\n20\n0\n11\n1\n21\n1\n\
0\nINSERT\n2\nB\n62\n5\n\
0\nINSERT\n2\nB\n\
0\nLINE\n67\n1\n10\n0\n20\n0\n11\n1\n21\n1\n\
0\nLINE\n60\n1\n10\n0\n20\n0\n11\n1\n21\n1\n";
        let (doc, report) = run(&dxf_text("", "", blocks, ents), &ImportOptions::default());
        let o: Vec<_> = doc.objects.values().collect();
        assert_eq!(o[0].attrs.pen, Some(Color([255, 0, 0, 255])));
        assert_eq!(o[0].attrs.line_weight, Some(0.25));
        assert_eq!(o[1].attrs.pen, Some(Color([1, 1, 1, 255])));
        assert_eq!(o[2].attrs.pen, None);
        assert_eq!(o[3].attrs.pen, Some(Color([0, 0, 255, 255])));
        assert_eq!(o[4].attrs.pen, None);
        // The invisible line is skipped. The line in paper space is on a sheet layer.
        assert_eq!(report.skipped_total(), 1);
        assert_eq!(
            report
                .notes
                .iter()
                .filter(|n| n.contains("became a sheet layer"))
                .count(),
            1
        );
        let sheet = doc
            .layers
            .iter()
            .find(|l| l.kind == kadakas_core::model::LayerKind::Sheet);
        assert!(sheet.is_some_and(|l| doc.objects.values().any(|o| o.layer == l.id)));
    }

    #[test]
    fn units_and_notes() {
        let ents = "0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n";
        let (doc, report) = run(
            &dxf_text("9\n$INSUNITS\n70\n6\n", "", "", ents),
            &ImportOptions::default(),
        );
        assert!(matches!(shapes(&doc)[0], Shape::Line { b, .. } if b.x == 1000.0));
        assert!(report.notes.is_empty());
        // No header: no unit, and nothing says which drawing it is.
        let (_, report) = run(&dxf_text("", "", "", ents), &ImportOptions::default());
        assert_eq!(report.notes.len(), 2);
        let (_, report) = run(
            &dxf_text("9\n$INSUNITS\n70\n0\n", "", "", ents),
            &ImportOptions::default(),
        );
        assert_eq!(report.notes.len(), 1);
        let opts = ImportOptions {
            unit: Some(crate::options::Unit::Foot),
            ..Default::default()
        };
        let (doc, _) = run(&dxf_text("9\n$INSUNITS\n70\n6\n", "", "", ents), &opts);
        assert!(matches!(shapes(&doc)[0], Shape::Line { b, .. } if (b.x - 304.8).abs() < 1e-9));
        let cut = dxf_text("", "", "", ents);
        let (doc, report) = run(&cut[..cut.len() - 40], &ImportOptions::default());
        assert!(report.notes.iter().any(|n| n.contains("ends early")));
        assert!(doc.objects.len() <= 1);
    }

    #[test]
    fn self_referencing_block_stops() {
        let blocks = "0\nBLOCK\n2\nLOOP\n0\nINSERT\n2\nLOOP\n0\nINSERT\n2\nLOOP\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n";
        let (doc, report) = run(
            &dxf_text("", "", blocks, "0\nINSERT\n2\nLOOP\n"),
            &ImportOptions::default(),
        );
        assert!(doc.objects.len() < 10_000_000);
        assert!(report.skipped_total() > 0);
    }

    #[test]
    fn object_limit() {
        let ents = "0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n".repeat(5);
        let opts = ImportOptions {
            max_objects: 3,
            ..Default::default()
        };
        let (doc, report) = run(&dxf_text("", "", "", &ents), &opts);
        assert_eq!(doc.objects.len(), 3);
        assert!(report.notes.iter().any(|n| n.contains("limit")));
        let _ = ObjectId::from_num(0);
    }

    #[test]
    fn bulge_points_match_mid() {
        for b in [0.3, 1.0, -0.7, 2.5, -4.0] {
            let (a, c) = (DVec2::new(1.0, 2.0), DVec2::new(5.0, -1.0));
            let pts = bulge_points(a, c, b, 0.001);
            assert_eq!(pts[0], a);
            assert_eq!(*pts.last().unwrap(), c);
            let mid = bulge_mid(a, c, b);
            // All points share one circle with the midpoint.
            let center = {
                let (p, q, r) = (a, mid, c);
                let d = 2.0 * (p.x * (q.y - r.y) + q.x * (r.y - p.y) + r.x * (p.y - q.y));
                DVec2::new(
                    (p.length_squared() * (q.y - r.y)
                        + q.length_squared() * (r.y - p.y)
                        + r.length_squared() * (p.y - q.y))
                        / d,
                    (p.length_squared() * (r.x - q.x)
                        + q.length_squared() * (p.x - r.x)
                        + r.length_squared() * (q.x - p.x))
                        / d,
                )
            };
            let radius = center.distance(a);
            for p in &pts {
                assert!((center.distance(*p) - radius).abs() < 1e-6, "bulge {b}");
            }
        }
    }

    #[test]
    fn far_drawings_are_centered_and_the_shift_is_recorded() {
        // L-EST97-like survey coordinates in meters.
        let ents = "0\nLINE\n8\nA\n10\n6589000\n20\n540000\n11\n6589100\n21\n540050\n\
0\nCIRCLE\n8\nA\n10\n6589050\n20\n540025\n40\n2\n";
        let text = dxf_text("9\n$INSUNITS\n70\n6\n", "", "", ents);
        let (doc, report) = run(&text, &ImportOptions::default());
        assert_eq!(doc.objects.len(), 2, "{:?}", report.skipped);
        let g = doc.georef.clone().unwrap();
        assert_eq!(
            (g.crs.as_str(), g.origin),
            ("", DVec2::new(6_589_050.0, 540_025.0))
        );
        let Shape::Circle { center, radius } = shapes(&doc)[1] else {
            panic!()
        };
        assert_eq!((*center, *radius), (DVec2::ZERO, 2000.0));
        assert!(report.notes.iter().any(|n| n.contains("centered")));
        // Keep the file origin: objects are out of range and skipped.
        let keep = ImportOptions {
            origin: OriginMode::File,
            ..ImportOptions::default()
        };
        let (doc, report) = run(&text, &keep);
        assert_eq!(doc.objects.len(), 0);
        assert_eq!(report.skipped_total(), 2);
        assert!(doc.georef.is_none());
        // A document with a georeference keeps its own origin.
        let mut base = Document::with_sequential_ids();
        base.georef = Some(Georeference {
            crs: "EPSG:3301".into(),
            origin: DVec2::new(6_589_000.0, 540_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        });
        let src = Source {
            name: "t".into(),
            data: text.into_bytes(),
            extra: Default::default(),
        };
        let r = import(&src, &Context::of(&base), &ImportOptions::default(), None).unwrap();
        let mut ed = Editor::new(base.clone());
        ed.execute(&r.command).unwrap();
        assert_eq!(ed.doc().georef, base.georef);
        let Shape::Circle { center, .. } = shapes(ed.doc())[1] else {
            panic!()
        };
        assert_eq!(*center, DVec2::new(50_000.0, 25_000.0));
        assert!(r.report.georeferenced);
        // A drawing near the origin is not touched.
        let near = dxf_text("", "", "", "0\nLINE\n8\nA\n10\n0\n20\n0\n11\n10\n21\n0\n");
        let (doc, _) = run(&near, &ImportOptions::default());
        assert!(doc.georef.is_none());
    }

    #[test]
    fn nested_array_inserts_hit_the_exploded_limit() {
        // 100 x 100 array of block A, which holds a 100 x 100 array of block B: 100 million lines.
        let blocks = "0\nBLOCK\n2\nB\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n\
0\nBLOCK\n2\nA\n10\n0\n20\n0\n0\nINSERT\n2\nB\n70\n100\n71\n100\n44\n2\n45\n2\n0\nENDBLK\n";
        let ents = "0\nINSERT\n2\nA\n70\n100\n71\n100\n44\n300\n45\n300\n";
        let text = dxf_text("", "", blocks, ents);
        assert!(text.len() < 500);
        let doc = Document::with_sequential_ids();
        let src = Source {
            name: "t".into(),
            data: text.into_bytes(),
            extra: Default::default(),
        };
        let err = import(&src, &Context::of(&doc), &ImportOptions::default(), None).unwrap_err();
        assert!(err.to_string().contains("max_exploded"), "{err}");
        // A smaller drawing within the limit still works, and so does a higher limit.
        let small = dxf_text(
            "",
            "",
            "0\nBLOCK\n2\nB\n10\n0\n20\n0\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n",
            "0\nINSERT\n2\nB\n70\n10\n71\n10\n44\n5\n45\n5\n",
        );
        let (doc, _) = run(&small, &ImportOptions::default());
        assert_eq!(doc.objects.len(), 100);
        let opts = ImportOptions {
            max_exploded: 50,
            ..ImportOptions::default()
        };
        let src = Source {
            name: "t".into(),
            data: small.into_bytes(),
            extra: Default::default(),
        };
        assert!(
            import(
                &src,
                &Context::of(&Document::with_sequential_ids()),
                &opts,
                None
            )
            .is_err()
        );
    }
}
