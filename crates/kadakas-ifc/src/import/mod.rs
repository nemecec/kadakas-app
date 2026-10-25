//! IFC import: IFC2x3, IFC4 and IFC4x3 files become one `Command::Batch` and a report.
//!
//! The reader ([`crate::reader`]) indexes the file once and parses entities when they are needed.
//! The importer walks the products, evaluates their geometry ([`geom`]) and maps them to objects:
//!
//! | IFC | Kadakas |
//! |---|---|
//! | `IfcGeographicElement` TERRAIN, an `IfcSite` with a face set | a site model (the vertices are its points) or, with the option `terrain: "mesh"`, a 3D mesh |
//! | `IfcGeographicElement` vegetation with a thin flat extrusion | a landscape area (exact arcs, holes) |
//! | `IfcGeographicElement` vegetation with a size | a plant (the size is the box around the geometry) |
//! | `IfcSlab`, `IfcCovering`, walls and all other elements with geometry | a generic 3D mesh object (`Shape::Mesh3d`). The IFC class is in the data under `ifc_class`. Nothing is turned into a hardscape. |
//! | `IfcMappedItem` used twice or more by upright extrusions | a symbol with 3D components, and symbol instances |
//! | `IfcAnnotation` curves and points | polylines (arcs stay exact) and survey points |
//! | `IfcBuildingStorey` | a design layer with the storey elevation |
//! | `IfcPresentationLayerAssignment` | a class |
//! | property sets and quantity sets | records (one record format per set) or object data |
//!
//! Geometry that is not supported is listed in the report. If the product has an `IfcBoundingBox`
//! representation, the box stands in for it. Openings (`IfcRelVoidsElement`) are not cut.
//! `IfcMapConversion` sets the georeference of an empty document, or moves the geometry into the
//! georeference of the document when both use the same CRS.

pub mod advanced;
pub mod csg;
pub mod ctx;
pub mod geom;
pub mod model;
mod props;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_more;
#[cfg(test)]
mod tests_openings;
#[cfg(test)]
mod tests_real;
#[cfg(test)]
mod tests_swept;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use glam::{DAffine3, DVec2, DVec3};
use kadakas_core::command::Command;
use kadakas_core::curve::{self, Vertex};
use kadakas_core::georef::Georeference;
use kadakas_core::graphics::{GraphicResource, Solid3d, SymbolDef, SymbolItem};
use kadakas_core::model::{Attributes, ClassId, Color, Document, LayerId, ObjectId, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_io::builder::Builder;
use kadakas_io::{Context, ImportResult};
use serde_json::{Value as Json, json};

use crate::container::{self, Container};
use crate::reader::{Limits, StepError, StepIndex};
use crate::step::Entity;
use ctx::{Ctx, attr, enum_of, items, num, text};
use geom::{Eval, MeshData, Piece};
use model::Model;

/// How property sets are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyMode {
    /// One record format per property set, attached to the objects.
    Records,
    /// Plain object data under `ifc_psets`.
    Data,
    None,
}

/// What a terrain becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainMode {
    SiteModel,
    Mesh,
}

#[derive(Clone, Debug)]
pub struct IfcImportOptions {
    /// One design layer per storey, with its elevation. Elements outside a storey go to the layer "Site".
    pub layers_from_storeys: bool,
    /// One class per presentation layer.
    pub classes_from_layers: bool,
    pub property_sets: PropertyMode,
    pub terrain: TerrainMode,
    /// Turn repeated mapped items into symbols.
    pub instances: bool,
    /// Turn vegetation with a size into plants and thin areas into landscape areas.
    pub vegetation: bool,
    /// Use `IfcMapConversion`.
    pub georef: bool,
    pub max_objects: usize,
    pub max_triangles: usize,
    /// Work (split tests) that all booleans of one import may use: openings, `IfcBooleanResult`.
    /// When it is used up, the remaining openings stay as objects and the report counts them.
    pub boolean_work: u64,
    pub limits: Limits,
}

impl Default for IfcImportOptions {
    fn default() -> Self {
        IfcImportOptions {
            layers_from_storeys: true,
            classes_from_layers: true,
            property_sets: PropertyMode::Records,
            terrain: TerrainMode::SiteModel,
            instances: true,
            vegetation: true,
            georef: true,
            max_objects: 500_000,
            max_triangles: 20_000_000,
            boolean_work: csg::IMPORT_FUEL,
            limits: Limits::default(),
        }
    }
}

impl IfcImportOptions {
    /// Reads the options of `import.file`. Unknown names are an error.
    pub fn from_json(v: &Json) -> Result<IfcImportOptions, IfcImportError> {
        let mut o = IfcImportOptions::default();
        let Some(map) = v.as_object() else {
            return if v.is_null() {
                Ok(o)
            } else {
                Err(IfcImportError::Option("options must be an object".into()))
            };
        };
        let bad =
            |k: &str, want: &str| IfcImportError::Option(format!("option '{k}' needs {want}"));
        for (k, val) in map {
            let flag = || val.as_bool().ok_or_else(|| bad(k, "true or false"));
            let count = |min: u64| {
                val.as_u64()
                    .filter(|n| *n >= min)
                    .map(|n| n as usize)
                    .ok_or_else(|| bad(k, &format!("a whole number of at least {min}")))
            };
            match k.as_str() {
                "layers_from_storeys" => o.layers_from_storeys = flag()?,
                "classes_from_layers" => o.classes_from_layers = flag()?,
                "instances" => o.instances = flag()?,
                "vegetation" => o.vegetation = flag()?,
                "georef" => o.georef = flag()?,
                "max_objects" => o.max_objects = count(1)?,
                "max_triangles" => o.max_triangles = count(1)?,
                "boolean_work" => o.boolean_work = count(0)? as u64,
                "max_file_mb" => o.limits.max_bytes = count(1)?.saturating_mul(1 << 20),
                "property_sets" => {
                    o.property_sets = match val.as_str() {
                        Some("records") => PropertyMode::Records,
                        Some("data") => PropertyMode::Data,
                        Some("none") => PropertyMode::None,
                        _ => return Err(bad(k, "\"records\", \"data\" or \"none\"")),
                    }
                }
                "terrain" => {
                    o.terrain = match val.as_str() {
                        Some("site_model") => TerrainMode::SiteModel,
                        Some("mesh") => TerrainMode::Mesh,
                        _ => return Err(bad(k, "\"site_model\" or \"mesh\"")),
                    }
                }
                other => {
                    return Err(IfcImportError::Option(format!(
                        "unknown option '{other}'. Options: layers_from_storeys, classes_from_layers, property_sets, terrain, instances, vegetation, georef, max_objects, max_triangles, boolean_work, max_file_mb"
                    )));
                }
            }
        }
        Ok(o)
    }
}

/// Why an import failed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IfcImportError {
    #[error("{0}")]
    Option(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Format(String),
    /// A bug met a file nobody foresaw: the import was stopped and nothing was changed.
    #[error("the import failed on a damaged file (internal error)")]
    Internal,
}

impl From<StepError> for IfcImportError {
    fn from(e: StepError) -> Self {
        match e {
            StepError::Io(m) => IfcImportError::Io(m),
            StepError::TooLarge(n) => IfcImportError::Format(format!(
                "the file is larger than the limit of {} MB (option max_file_mb, at most 4096): it needs more memory than this session allows",
                n >> 20
            )),
            e => IfcImportError::Format(e.to_string()),
        }
    }
}

/// Runs an import and turns a panic into [`IfcImportError::Internal`]: a crafted file must never
/// end the program.
fn guarded<T>(f: impl FnOnce() -> Result<T, IfcImportError>) -> Result<T, IfcImportError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or(Err(IfcImportError::Internal))
}

/// Reads an IFC file (STEP or ifcZIP, found by the first bytes, not by the name) and plans the
/// import into `doc`. IFC XML and IFC JSON are refused with a message. A panic inside is caught.
pub fn import_file(
    path: &Path,
    doc: &Document,
    opts: &IfcImportOptions,
) -> Result<ImportResult, IfcImportError> {
    guarded(|| import_file_unguarded(path, doc, opts))
}

/// Plans the import of IFC bytes (STEP or ifcZIP). A panic inside is caught.
pub fn import_bytes(
    data: Vec<u8>,
    doc: &Document,
    opts: &IfcImportOptions,
) -> Result<ImportResult, IfcImportError> {
    guarded(|| import_bytes_unguarded(data, doc, opts))
}

/// [`import_file`] without the catch of panics, for the tests that look for them.
#[doc(hidden)]
pub fn import_file_unguarded(
    path: &Path,
    doc: &Document,
    opts: &IfcImportOptions,
) -> Result<ImportResult, IfcImportError> {
    use std::io::{Read, Seek};
    let mut file = std::fs::File::open(path)
        .map_err(|e| IfcImportError::Io(format!("cannot read {}: {e}", path.display())))?;
    let mut head = [0u8; 64];
    let n = file.read(&mut head).unwrap_or(0);
    match container::sniff(&head[..n]) {
        Container::Step => {
            drop(file);
            let index = StepIndex::from_file(path, opts.limits.clone())?;
            import_index(index, doc, opts, Vec::new())
        }
        Container::Zip => {
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if len > opts.limits.max_bytes as u64 {
                return Err(StepError::TooLarge(opts.limits.max_bytes).into());
            }
            file.rewind()
                .map_err(|e| IfcImportError::Io(e.to_string()))?;
            import_zip(std::io::BufReader::new(file), doc, opts)
        }
        other => Err(container::refusal(other)),
    }
}

fn import_zip<R: std::io::Read + std::io::Seek>(
    r: R,
    doc: &Document,
    opts: &IfcImportOptions,
) -> Result<ImportResult, IfcImportError> {
    let unpacked = container::unpack(r, opts.limits.max_bytes)?;
    let index = StepIndex::from_bytes(unpacked.data, opts.limits.clone())?;
    import_index(index, doc, opts, unpacked.notes)
}

/// [`import_bytes`] without the catch of panics, for the tests that look for them.
#[doc(hidden)]
pub fn import_bytes_unguarded(
    data: Vec<u8>,
    doc: &Document,
    opts: &IfcImportOptions,
) -> Result<ImportResult, IfcImportError> {
    match container::sniff(&data[..data.len().min(64)]) {
        Container::Step => {
            let index = StepIndex::from_bytes(data, opts.limits.clone())?;
            import_index(index, doc, opts, Vec::new())
        }
        Container::Zip => import_zip(std::io::Cursor::new(data), doc, opts),
        other => Err(container::refusal(other)),
    }
}

fn import_index(
    index: StepIndex,
    doc: &Document,
    opts: &IfcImportOptions,
    notes: Vec<String>,
) -> Result<ImportResult, IfcImportError> {
    let schemas = index.schemas();
    if !schemas.is_empty()
        && !schemas
            .iter()
            .any(|s| s.to_ascii_uppercase().starts_with("IFC"))
    {
        return Err(IfcImportError::Format(format!(
            "the STEP file is not an IFC file (schema {})",
            schemas.join(", ")
        )));
    }
    if index.ids_of("IFCPROJECT").is_empty() && index.is_empty() {
        return Err(IfcImportError::Format("the file has no IFC data".into()));
    }
    let ctx = Context::of(doc);
    let schema = schemas.first().cloned().unwrap_or_default();
    let make = || {
        let cx = Ctx::new(&index);
        let multi_site = cx.x.ids_of("IFCSITE").len() > 1;
        Importer {
            b: Builder::new(&ctx, "ifc", opts.max_objects),
            model: Model::load(&cx),
            cx,
            opts,
            doc,
            schema: schema.clone(),
            frame: Frame::default(),
            shift: DVec2::ZERO,
            multi_site,
            layer_cache: HashMap::new(),
            class_cache: HashMap::new(),
            symbols: HashMap::new(),
            symbol_names: HashSet::new(),
            sym_src: RefCell::new(HashMap::new()),
            rows: Vec::new(),
            triangles: 0,
            geometry_notes: BTreeMap::new(),
            z_dropped: 0,
            fuel: csg::Fuel::new(opts.boolean_work),
            brep_cache: Default::default(),
            open_cuts: HashSet::new(),
            opening_failures: BTreeMap::new(),
            cut: HashSet::new(),
            uncut: HashSet::new(),
            bodyless: HashSet::new(),
        }
    };
    let mut imp = make();
    // Geometry far from the origin (projected coordinates used as model coordinates) is moved to
    // the origin when the document has no georeference of its own; the shift goes into the
    // georeference. The place is found from the placements and a bounded sample of points, with no
    // geometry made, so a file that is not far costs nothing more.
    if opts.georef && doc.georef.is_none() {
        imp.shift = imp.far_shift().unwrap_or(DVec2::ZERO);
        // The sample read placements; the counts of the import start again.
        imp.cx.bad_placements.set(0);
        imp.cx.other_placements.set(0);
    }
    for n in notes.iter().chain(&imp.cx.x.notes) {
        imp.b.report.note(n);
    }
    imp.run();
    // Last resort: the coordinates were far, but the samples did not show it (products out of range
    // were skipped). The geometry itself says where the file is: a pass that only measures.
    let out_of_range = imp
        .b
        .report
        .skipped
        .iter()
        .any(|s| s.reason.contains("out of range"));
    if out_of_range && opts.georef && doc.georef.is_none() && imp.shift == DVec2::ZERO {
        let mut probe = make();
        probe.b.set_probe();
        probe.run();
        if let Some((lo, hi)) = probe.b.bounds() {
            let c = (lo + hi) / 2.0;
            if c.is_finite() && c.abs().max_element() < 1.0e12 {
                let mut again = make();
                again.shift = (c / 1000.0).round() * 1000.0;
                for n in notes.iter().chain(&again.cx.x.notes) {
                    again.b.report.note(n);
                }
                again.b.report.note("the geometry was far from the origin and out of range: the file was read again with a shift");
                again.run();
                return Ok(again.b.finish());
            }
        }
    }
    Ok(imp.b.finish())
}

/// Hosts with openings that were not all cut, for one reason.
#[derive(Default)]
struct OpeningTally {
    hosts: usize,
    openings: usize,
    /// The first hosts, with the number of their openings.
    names: Vec<(String, usize)>,
}

/// A rigid motion in the plane (millimeters), for placing the file into the georeference of the document.
#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    rotation: f64,
    shift: DVec2,
}

impl Frame {
    fn point(&self, p: DVec3) -> DVec3 {
        if self.rotation == 0.0 && self.shift == DVec2::ZERO {
            return p;
        }
        let q = DVec2::from_angle(self.rotation).rotate(p.truncate()) + self.shift;
        q.extend(p.z)
    }

    fn point2(&self, p: DVec2) -> DVec2 {
        self.point(p.extend(0.0)).truncate()
    }
}

struct SymSrc {
    /// Outline, base and height of each solid, in the frame of the map origin, in file units.
    solids: Vec<(Vec<DVec2>, f64, f64)>,
}

struct Importer<'a> {
    cx: Ctx<'a>,
    model: Model,
    b: Builder,
    opts: &'a IfcImportOptions,
    doc: &'a Document,
    schema: String,
    frame: Frame,
    /// Shift (mm) that moves far geometry to the origin; it is part of the georeference.
    shift: DVec2,
    /// The file has more than one `IfcSite` (a complex site with partial sites): each gets a layer.
    multi_site: bool,
    layer_cache: HashMap<Option<u64>, (LayerId, f64)>,
    class_cache: HashMap<String, ClassId>,
    symbols: HashMap<(u64, i64), ResourceId>,
    symbol_names: HashSet<String>,
    sym_src: RefCell<HashMap<u64, Option<Rc<SymSrc>>>>,
    rows: Vec<(ObjectId, Vec<model::PropSet>)>,
    triangles: usize,
    geometry_notes: BTreeMap<String, usize>,
    z_dropped: usize,
    /// Openings that were cut from their host, and openings whose cut failed.
    /// The work budget for the booleans of the whole import.
    fuel: csg::Fuel,
    /// Advanced breps that were read already.
    brep_cache: geom::BrepCache,
    /// Hosts whose mesh after the cut is not closed (data `ifc.mesh_closed` is false).
    open_cuts: HashSet<u64>,
    /// Hosts whose openings could not all be cut, by reason and whether the cut was partial.
    opening_failures: BTreeMap<(String, bool), OpeningTally>,
    cut: HashSet<u64>,
    uncut: HashSet<u64>,
    /// Openings that have no Body representation (a Reference representation is not subtracted).
    bodyless: HashSet<u64>,
}

/// Wall-clock-free helpers for the object data.
fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl<'a> Importer<'a> {
    /// The shift (mm, whole meters) that brings the file to the origin, if it is more than 100 km
    /// away. Samples: the placements of the sites, buildings and the first products, the first
    /// points of the first point lists, and the first points of the file.
    fn far_shift(&self) -> Option<DVec2> {
        const FAR_MM: f64 = 1.0e8;
        const MAX_PRODUCTS: usize = 3000;
        let mm = self.cx.units.length_mm;
        // Two kinds of sample: where the products are placed, and the coordinates of point lists.
        // They are in different frames (a point list is in the frame of its product), so the
        // place comes from the placements when they are far, else from the points.
        struct Spread {
            lo: DVec2,
            hi: DVec2,
            far: bool,
        }
        impl Spread {
            fn new() -> Spread {
                Spread {
                    lo: DVec2::splat(f64::INFINITY),
                    hi: DVec2::splat(f64::NEG_INFINITY),
                    far: false,
                }
            }
            fn add(&mut self, p: DVec2) {
                if p.is_finite() {
                    self.far |= p.abs().max_element() > FAR_MM;
                    self.lo = self.lo.min(p);
                    self.hi = self.hi.max(p);
                }
            }
        }
        let (mut placed, mut points) = (Spread::new(), Spread::new());
        let products: HashSet<String> = model::product_names().collect();
        let mut seen = 0usize;
        for (id, n) in self.cx.x.iter() {
            let spatial = model::is_spatial(n);
            if !(spatial || products.contains(n)) {
                continue;
            }
            seen += 1;
            if seen > MAX_PRODUCTS && !spatial {
                continue;
            }
            if let Some(e) = self.cx.ent(id)
                && let Some(p) = attr(&e, 5).as_ref_id()
            {
                let t = self.cx.placement(p).translation;
                placed.add(DVec2::new(t.x, t.y) * mm);
            }
        }
        // Points: a stride sample of up to 100,000 of the cartesian points and of the first points
        // of the point lists. Only the far ones count for the place, because a point near the origin
        // is a point of a profile or of a local frame.
        const MAX_POINTS: usize = 100_000;
        for (name, per_entity) in [
            ("IFCCARTESIANPOINT", 1),
            ("IFCCARTESIANPOINTLIST3D", 4),
            ("IFCCARTESIANPOINTLIST2D", 4),
        ] {
            let ids = self.cx.x.ids_of(name);
            let stride = ids.len().div_ceil(MAX_POINTS).max(1);
            for id in ids.into_iter().step_by(stride) {
                let Some(e) = self.cx.ent(id) else { continue };
                let rows: Vec<Vec<f64>> = if name == "IFCCARTESIANPOINT" {
                    vec![ctx::numbers(attr(&e, 0))]
                } else {
                    items(attr(&e, 0))
                        .iter()
                        .take(per_entity)
                        .map(ctx::numbers)
                        .collect()
                };
                for v in rows {
                    if v.len() >= 2 {
                        let p = DVec2::new(v[0], v[1]) * mm;
                        if p.is_finite() && p.abs().max_element() > FAR_MM {
                            points.add(p);
                        }
                    }
                }
            }
        }
        let s = if placed.far {
            placed
        } else if points.far {
            points
        } else {
            return None;
        };
        let c = (s.lo + s.hi) / 2.0;
        (c.is_finite() && c.abs().max_element() < 1.0e12).then(|| (c / 1000.0).round() * 1000.0)
    }

    fn run(&mut self) {
        self.place_file();
        let mut cands: BTreeSet<u64> = BTreeSet::new();
        let names: HashSet<String> = model::product_names().collect();
        for (id, n) in self.cx.x.iter() {
            if names.contains(n) {
                cands.insert(id);
            }
        }
        for id in &self.model.related {
            if self.cx.x.name_of(*id).is_some_and(|n| n.starts_with("IFC")) {
                cands.insert(*id);
            }
        }
        // Openings come last: a host has cut them (or failed to) before they are looked at.
        let is_opening = |x: &StepIndex, id: u64| {
            matches!(
                x.name_of(id),
                Some("IFCOPENINGELEMENT" | "IFCOPENINGSTANDARDCASE")
            )
        };
        let (late, mut cands): (Vec<u64>, Vec<u64>) =
            cands.into_iter().partition(|id| is_opening(self.cx.x, *id));
        cands.extend(late);
        let mut seen_classes: BTreeMap<String, usize> = BTreeMap::new();
        let mut global_ids: HashSet<String> = HashSet::new();
        let mut repeated_ids = 0usize;
        for id in cands {
            if self.b.is_full() {
                break;
            }
            let Some(e) = self.cx.ent(id) else { continue };
            // Some exporters repeat a GlobalId. Both objects are kept; the count goes in the report.
            if let Some(g) = text(attr(&e, 0)).filter(|g| !g.is_empty())
                && !global_ids.insert(g.to_string())
            {
                repeated_ids += 1;
            }
            // A product has a placement (5) and a representation (6).
            let Some(rep) = self.cx.get(attr(&e, 6)) else {
                continue;
            };
            if rep.name != "IFCPRODUCTDEFINITIONSHAPE" {
                continue;
            }
            let class = model::class_name(&e.name);
            *seen_classes.entry(class.clone()).or_default() += 1;
            // Every product with a representation is read; it either gives objects or has a line
            // in `skipped`, so the accounting is exact (`entities_read` = imported + skipped).
            let (objects, skipped) = (self.b.report.total(), self.b.report.skipped_total());
            self.product(&e, &rep);
            self.b.report.entities_read += 1;
            if self.b.report.total() > objects {
                self.b.report.entities_imported += 1;
            } else {
                if self.b.report.skipped_total() == skipped {
                    self.b
                        .report
                        .skip(&class, "no object could be made from it");
                }
                self.b.report.entities_skipped += 1;
            }
            // Parsed entities of a big file would fill the memory.
            if id % 4096 == 0 {
                self.cx.x.clear_cache();
            }
        }
        self.finish_records();
        if repeated_ids > 0 {
            self.b.report.note(&format!(
                "{repeated_ids} products repeat the GlobalId of another product: all are imported"
            ));
        }
        self.finish_opening_notes();
        if !self.open_cuts.is_empty() {
            self.b.report.note(&format!(
                "{} meshes are not closed after the cut of their openings (the volume is right, but a solid tool may refuse them): their data ifc.mesh_closed is false",
                self.open_cuts.len()
            ));
        }
        if !self.cut.is_empty() {
            self.b.report.note(&format!(
                "{} openings (IfcRelVoidsElement) were cut from their walls and slabs",
                self.cut.len()
            ));
        }
        for (what, n) in std::mem::take(&mut self.geometry_notes) {
            self.b.report.note(&format!("{n} x {what}"));
        }
        if self.cx.bad_placements.get() > 0 {
            self.b.report.note(&format!(
                "{} placement chains loop or have more than 256 levels: they stop there",
                self.cx.bad_placements.get()
            ));
        }
        if self.cx.other_placements.get() > 0 {
            self.b.report.note(&format!(
                "{} placements (IfcLinearPlacement, or an IfcGridPlacement that cannot be read) are not supported: the product keeps the place of its parent",
                self.cx.other_placements.get()
            ));
        }
        if self.z_dropped > 0 {
            self.b.report.note(&format!(
                "{} landscape areas or plants keep the elevation of their layer: their own height is not stored",
                self.z_dropped
            ));
        }
        if self.b.report.total() == 0 {
            self.b
                .report
                .note("the file has no geometry that could be imported");
        }
    }

    // ------------------------------------------------------------ georeference

    fn place_file(&mut self) {
        if !self.opts.georef {
            return;
        }
        if self.shift != DVec2::ZERO {
            self.b.set_offset(self.shift);
            self.b.report.note(&format!(
                "the geometry is far from the origin: it was moved by ({}, {}) m, and the shift is in the georeference",
                self.shift.x / 1000.0,
                self.shift.y / 1000.0
            ));
        }
        let Some(g) = model::file_georef(&self.cx) else {
            if self.shift != DVec2::ZERO {
                self.b.set_georef(Georeference {
                    crs: String::new(),
                    origin: self.shift / 1000.0,
                    rotation: 0.0,
                    user_origin: DVec2::ZERO,
                });
                self.b.report.georeferenced = false;
                return;
            }
            if self.schema.to_ascii_uppercase().starts_with("IFC2X3") {
                self.b.report.note(
                    "the file has no georeference (IFC2x3 has no IfcMapConversion, and the property set ePSet_MapConversion is missing): the geometry keeps its local coordinates",
                );
            }
            return;
        };
        if (g.meters_per_mm / 0.001 - 1.0).abs() > 0.01 {
            self.b.report.note(&format!(
                "the map conversion has a scale of {} m per mm: the scale is not applied",
                g.meters_per_mm
            ));
        }
        if (g.factors - DVec3::ONE).abs().max_element() > 1e-9 {
            self.b.report.note(&format!(
                "the map conversion has grid factors ({}, {}, {}) (IfcMapConversionScaled): the factors are not applied",
                g.factors.x, g.factors.y, g.factors.z
            ));
        }
        if g.height.abs() > 1e-9 {
            self.b.report.note(&format!(
                "the map conversion has an orthogonal height of {}: the height is not applied",
                g.height
            ));
        }
        let crs_ok = kadakas_crs::Crs::parse(&g.crs).is_ok();
        match &self.doc.georef {
            None => {
                let mut geo = model::to_georef(&g);
                geo.origin += DVec2::from_angle(g.rotation).rotate(self.shift) / 1000.0;
                if !crs_ok {
                    self.b.report.note(&format!(
                        "the CRS '{}' of the file is not known: the georeference holds the shift of the origin only",
                        g.crs
                    ));
                    geo.crs.clear();
                }
                self.b.set_georef(geo);
            }
            Some(d) if !d.crs.is_empty() && crs_ok && same_crs(&g.crs, &d.crs) => {
                self.frame = Frame {
                    rotation: g.rotation - d.rotation,
                    shift: DVec2::from_angle(-d.rotation).rotate(g.origin - d.origin) * 1000.0,
                };
                self.b.report.georeferenced = true;
                self.b.report.note(&format!(
                    "the geometry was moved into the georeference of the document ({})",
                    d.crs
                ));
            }
            Some(d) => {
                self.b.report.note(&format!(
                    "the CRS of the file ('{}') differs from the CRS of the document ('{}'): the geometry keeps its local coordinates",
                    g.crs, d.crs
                ));
            }
        }
    }

    // ------------------------------------------------------------ layers and classes

    fn layer_of(&mut self, product: u64) -> (LayerId, f64) {
        if !self.opts.layers_from_storeys {
            let id = self.b.default_layer;
            let z = self.doc.layer(id).map_or(0.0, |l| l.elevation);
            return (id, z);
        }
        let storey = self.model.storey_of(&self.cx, product);
        // Without a storey, a file with partial sites puts each site on a layer of its own.
        let site = if storey.is_none() && self.multi_site {
            self.model.site_of(&self.cx, product)
        } else {
            None
        };
        let key = storey.or(site);
        if let Some(hit) = self.layer_cache.get(&key) {
            return *hit;
        }
        let (name, elevation) = match key.and_then(|s| self.cx.ent(s)) {
            Some(s) if s.name == "IFCSITE" => {
                let name = text(attr(&s, 2))
                    .map(|n| clean_text(n.trim(), 100))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("Site {}", s.id));
                (name, 0.0)
            }
            Some(s) => {
                let name = text(attr(&s, 2))
                    .map(|n| clean_text(n.trim(), 100))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("Storey {}", s.id));
                let z = num(attr(&s, 9))
                    .or_else(|| {
                        attr(&s, 5)
                            .as_ref_id()
                            .map(|p| self.cx.placement(p).translation.z)
                    })
                    .unwrap_or(0.0)
                    * self.cx.units.length_mm;
                (
                    name,
                    if z.is_finite() && z.abs() < 1e8 {
                        z
                    } else {
                        0.0
                    },
                )
            }
            None => ("Site".to_string(), 0.0),
        };
        let before = self.b.report.layers_created.len();
        let id = self.b.layer(&name, true);
        let result = if self.b.report.layers_created.len() > before {
            self.b.set_elevation(id, elevation);
            (id, elevation)
        } else {
            (id, self.doc.layer(id).map_or(0.0, |l| l.elevation))
        };
        self.layer_cache.insert(key, result);
        result
    }

    fn class_of(&mut self, ids: &[u64]) -> ClassId {
        if !self.opts.classes_from_layers {
            return self.b.default_class;
        }
        let Some((name, on)) = ids.iter().find_map(|i| self.model.layers.get(i)).cloned() else {
            return self.b.default_class;
        };
        if let Some(c) = self.class_cache.get(&name) {
            return *c;
        }
        let c = self
            .b
            .class(&clean_text(&name, 100), &Attributes::default(), on);
        self.class_cache.insert(name, c);
        c
    }

    // ------------------------------------------------------------ products

    fn product(&mut self, e: &Entity, rep: &Entity) {
        let class = model::class_name(&e.name);
        match e.name.as_str() {
            "IFCOPENINGELEMENT" | "IFCOPENINGSTANDARDCASE" => {
                if self.cut.contains(&e.id) {
                    self.b
                        .report
                        .skip(&class, "an opening: it was cut from its host element");
                    return;
                }
                if self.bodyless.contains(&e.id) {
                    self.b.report.skip(
                        &class,
                        "an opening without a Body representation: a Reference representation is not subtracted",
                    );
                    return;
                }
                if !self.uncut.contains(&e.id) {
                    self.b.report.skip(
                        &class,
                        "an opening whose host element was not imported or has no body",
                    );
                    return;
                }
                // The cut failed: the opening stays as an object of its own (see `cut_openings`).
            }
            "IFCSPACE" | "IFCEXTERNALSPATIALELEMENT" => {
                self.b
                    .report
                    .skip(&class, "a space: it has no material and is not imported");
                return;
            }
            "IFCGRID" | "IFCVIRTUALELEMENT" => {
                self.b
                    .report
                    .skip(&class, "a helper element without a body");
                return;
            }
            _ => {}
        }
        // Representation items, for the class and the color.
        let reps = self.cx.get_all(attr(rep, 2));
        let mut rep_ids: Vec<u64> = reps.iter().map(|r| r.id).collect();
        let mut item_ids: Vec<u64> = Vec::new();
        for r in &reps {
            for i in items(attr(r, 3)) {
                if let Some(i) = i.as_ref_id() {
                    item_ids.push(i);
                }
            }
        }
        rep_ids.extend(item_ids.iter().copied());
        let class_id = self.class_of(&rep_ids);
        let predefined = self.predefined(e);
        let object_type = text(attr(e, 4)).unwrap_or("").to_string();

        // Annotations are curves and points.
        if e.name == "IFCANNOTATION" {
            self.annotation(e, &reps, &class, class_id);
            return;
        }

        let (terrain, vegetation) =
            classify_geographic(e, &predefined, &object_type, || self.has_face_set(&reps));

        let mut out = self.eval_product(e, &reps, true);
        if out
            .pieces
            .iter()
            .any(|p| matches!(p, Piece::Instance { .. }))
            && !self.instances_ok(&out.pieces, e)
        {
            out = self.eval_product(e, &reps, false);
        }
        let mut notes = std::mem::take(&mut out.issues);
        if !out.pieces.is_empty() && self.model.openings_of.contains_key(&e.id) {
            self.cut_openings(e, &reps, &mut out, &class);
        }
        if out.pieces.is_empty() {
            // The bounding box stands in for geometry that is not supported.
            if let Some(b) = self.box_piece(e, &reps) {
                out.pieces.push(b);
                out.approximate = true;
            }
        }
        if out.pieces.is_empty() {
            let why = notes
                .first()
                .cloned()
                .unwrap_or_else(|| "no body geometry".into());
            self.b.report.skip(&class, &why);
            return;
        }
        if out.approximate {
            let why = notes.first().cloned().unwrap_or_default();
            self.b.report.skip(
                &format!("{class} (geometry)"),
                &format!("{why}: imported as a bounding box"),
            );
        } else {
            for n in notes.drain(..) {
                *self
                    .geometry_notes
                    .entry(format!("{class}: {n}"))
                    .or_default() += 1;
            }
        }

        let (layer, elevation) = self.layer_of(e.id);
        let color = self.color_of(&item_ids);
        let name = text(attr(e, 2))
            .map(|n| clean_text(n.trim(), 200))
            .unwrap_or_default();
        let psets = if self.opts.property_sets == PropertyMode::None {
            Vec::new()
        } else {
            self.model.property_sets(&self.cx, e.id)
        };
        let data = self.data_of(e, &class, &name, &predefined, &object_type, &psets);

        // Instances.
        if out
            .pieces
            .iter()
            .all(|p| matches!(p, Piece::Instance { .. }))
        {
            self.symbols_out(
                &out.pieces,
                (layer, elevation),
                class_id,
                data,
                &psets,
                &class,
            );
            return;
        }

        if terrain {
            self.terrain(
                e,
                &out.pieces,
                (layer, elevation),
                class_id,
                data,
                &psets,
                &class,
                color,
                &name,
                out.voids,
            );
            return;
        }
        if vegetation && self.opts.vegetation {
            if self.landscape_area(
                e,
                &out.pieces,
                (layer, elevation),
                class_id,
                &data,
                &psets,
                &name,
            ) {
                return;
            }
            if self.plant(
                e,
                &out.pieces,
                (layer, elevation),
                class_id,
                &data,
                &psets,
                &name,
            ) {
                return;
            }
        }
        self.mesh_object(
            e,
            &out.pieces,
            class_id,
            layer,
            data,
            &psets,
            &class,
            color,
            &name,
        );
    }

    /// Subtracts the openings of a host element (`IfcRelVoidsElement`) from its solid pieces. An
    /// opening that was cut from every piece it reaches is remembered in `cut`. When a cut fails (an
    /// open mesh, a body that is no solid, a geometry too complex, the work budget), the piece keeps
    /// its gross shape and the openings that reached it stay as objects of their own (`uncut`). An
    /// opening that was cut from one piece and not from another counts as cut, and the report says
    /// that it was cut in part. The report counts both cases per host (see `finish_opening_notes`).
    fn cut_openings(&mut self, e: &Entity, reps: &[Rc<Entity>], out: &mut EvalOut, class: &str) {
        let Some(list) = self.model.openings_of.get(&e.id).cloned() else {
            return;
        };
        // Instances cannot be cut: evaluate again without them.
        if out
            .pieces
            .iter()
            .any(|p| matches!(p, Piece::Instance { .. }))
        {
            let again = self.eval_product(e, reps, false);
            out.pieces = again.pieces;
        }
        // The cutters, in the units of the file (the host is in the same units).
        let mut cutters: Vec<(u64, geom::MeshData)> = Vec::new();
        for id in &list {
            let Some(op) = self.cx.ent(*id) else { continue };
            let Some(shape) = self.cx.get(attr(&op, 6)) else {
                continue;
            };
            if shape.name != "IFCPRODUCTDEFINITIONSHAPE" {
                continue;
            }
            // Only a Body representation is subtracted, not a Reference one.
            let op_reps: Vec<Rc<Entity>> = self
                .cx
                .get_all(attr(&shape, 2))
                .into_iter()
                .filter(|r| {
                    text(attr(r, 1)).is_none_or(|i| i.is_empty() || i.eq_ignore_ascii_case("Body"))
                })
                .collect();
            let got = self.eval_product(&op, &op_reps, false);
            let mut m = geom::MeshData::default();
            for p in &got.pieces {
                if let Some(pm) = geom::piece_mesh(p) {
                    m.append(&pm);
                }
            }
            if m.t.is_empty() {
                self.bodyless.insert(*id);
                continue;
            }
            cutters.push((*id, m));
        }
        if cutters.is_empty() {
            return;
        }
        // Per opening: in how many pieces it was cut and in how many the cut failed.
        let mut cut_in: HashMap<u64, usize> = HashMap::new();
        let mut failed_in: HashMap<u64, usize> = HashMap::new();
        let mut reason: Option<String> = None;
        let mut pieces = Vec::with_capacity(out.pieces.len());
        for p in std::mem::take(&mut out.pieces) {
            let Some(host) = geom::piece_mesh(&p) else {
                pieces.push(p);
                continue;
            };
            let Some((lo, hi)) = host.bounds() else {
                continue;
            };
            // Only the openings that reach the piece.
            let near: Vec<&(u64, geom::MeshData)> = cutters
                .iter()
                .filter(|(_, c)| {
                    c.bounds().is_some_and(|(a, b)| {
                        a.cmple(hi + DVec3::splat(csg::EPS)).all()
                            && b.cmpge(lo - DVec3::splat(csg::EPS)).all()
                    })
                })
                .collect();
            if near.is_empty() {
                pieces.push(p);
                continue;
            }
            let meshes: Vec<geom::MeshData> = near.iter().map(|(_, c)| c.clone()).collect();
            let result = if self.fuel.left() == 0 {
                Err(csg::CsgError::OutOfFuel)
            } else {
                self.fuel.scoped(csg::PER_CALL_FUEL, |own| {
                    csg::subtract_all(&host, &meshes, own)
                })
            };
            match result {
                Ok(m) => {
                    // The cut is exact in volume. A mesh that is not closed is told, not hidden.
                    if !csg::is_closed(&m) {
                        self.open_cuts.insert(e.id);
                    }
                    for (id, _) in &near {
                        *cut_in.entry(*id).or_default() += 1;
                    }
                    pieces.push(Piece::Mesh(m));
                }
                Err(why) => {
                    for (id, _) in &near {
                        *failed_in.entry(*id).or_default() += 1;
                    }
                    reason.get_or_insert_with(|| why.to_string());
                    pieces.push(p);
                }
            }
        }
        out.pieces = pieces;
        let (mut kept, mut partly) = (0usize, 0usize);
        for (id, _) in &cutters {
            match (cut_in.contains_key(id), failed_in.contains_key(id)) {
                (_, false) => {
                    self.cut.insert(*id);
                }
                (true, true) => {
                    self.cut.insert(*id);
                    partly += 1;
                }
                (false, true) => {
                    self.uncut.insert(*id);
                    kept += 1;
                }
            }
        }
        let why = reason.unwrap_or_default();
        let who = {
            let name = text(attr(e, 2))
                .map(|n| clean_text(n.trim(), 60))
                .unwrap_or_default();
            if name.is_empty() {
                format!("{class} #{}", e.id)
            } else {
                format!("{class} '{name}'")
            }
        };
        for (n, partial) in [(kept, false), (partly, true)] {
            if n > 0 {
                let t = self
                    .opening_failures
                    .entry((why.clone(), partial))
                    .or_default();
                t.hosts += 1;
                t.openings += n;
                if t.names.len() < 3 {
                    t.names.push((who.clone(), n));
                }
            }
        }
    }

    /// The notes about openings that could not be cut: one line for each host when there are few,
    /// else one line for all hosts with the same reason.
    fn finish_opening_notes(&mut self) {
        for ((why, partial), t) in std::mem::take(&mut self.opening_failures) {
            let what = if partial {
                "cut from some parts only (the other parts keep their gross shape)"
            } else {
                "kept as objects of their own (the host keeps its gross shape)"
            };
            if t.hosts <= 3 {
                for (who, n) in &t.names {
                    self.b
                        .report
                        .note(&format!("{who}: {n} openings {what}: {why}"));
                }
            } else {
                let names: Vec<&str> = t.names.iter().map(|(w, _)| w.as_str()).collect();
                self.b.report.note(&format!(
                    "{} hosts ({}, ...): {} openings {what}: {why}",
                    t.hosts,
                    names.join(", "),
                    t.openings
                ));
            }
        }
    }

    fn predefined(&self, e: &Entity) -> String {
        if model::is_spatial(&e.name) {
            return String::new();
        }
        // Elements have the predefined type as their last attribute (position 8 in IFC4 elements).
        e.attrs
            .iter()
            .skip(7)
            .filter_map(enum_of)
            .next()
            .unwrap_or("")
            .to_string()
    }

    fn has_face_set(&self, reps: &[Rc<Entity>]) -> bool {
        reps.iter().any(|r| {
            items(attr(r, 3)).iter().any(|i| {
                matches!(
                    self.cx.x.name_of(i.as_ref_id().unwrap_or(0)),
                    Some(
                        "IFCTRIANGULATEDFACESET"
                            | "IFCTRIANGULATEDIRREGULARNETWORK"
                            | "IFCPOLYGONALFACESET"
                    )
                )
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn data_of(
        &self,
        e: &Entity,
        class: &str,
        name: &str,
        predefined: &str,
        object_type: &str,
        psets: &[model::PropSet],
    ) -> BTreeMap<String, Json> {
        let mut info = serde_json::Map::new();
        info.insert("class".into(), json!(class));
        if let Some(g) = text(attr(e, 0)) {
            info.insert("global_id".into(), json!(clean_text(g, 64)));
        }
        if !name.is_empty() {
            info.insert("name".into(), json!(name));
        }
        if let Some(d) = text(attr(e, 3)).map(str::trim).filter(|d| !d.is_empty()) {
            info.insert("description".into(), json!(clean_text(d, 500)));
        }
        if !predefined.is_empty() {
            info.insert("predefined_type".into(), json!(predefined));
        }
        if !object_type.is_empty() {
            info.insert("object_type".into(), json!(clean_text(object_type, 200)));
        }
        if let Some(t) = text(attr(e, 7)) {
            info.insert("tag".into(), json!(clean_text(t, 100)));
        }
        if self.open_cuts.contains(&e.id) {
            info.insert("mesh_closed".into(), json!(false));
        }
        info.insert("schema".into(), json!(self.schema));
        let mut data = BTreeMap::new();
        data.insert("ifc_class".to_string(), json!(class));
        data.insert("ifc".to_string(), Json::Object(info));
        if self.opts.property_sets == PropertyMode::Data && !psets.is_empty() {
            let mut sets = serde_json::Map::new();
            for s in psets {
                let mut m = serde_json::Map::new();
                for p in &s.props {
                    m.insert(p.name.clone(), p.value.clone());
                }
                sets.insert(s.name.clone(), Json::Object(m));
            }
            data.insert("ifc_psets".to_string(), Json::Object(sets));
        }
        data
    }

    fn color_of(&self, items: &[u64]) -> Option<kadakas_core::model::Color> {
        let find = |ids: &[u64]| ids.iter().find_map(|i| self.model.colors.get(i)).copied();
        let c = find(items).or_else(|| {
            // Mapped items: the style is often on the items of the source.
            items.iter().find_map(|i| {
                let e = self.cx.ent(*i)?;
                if e.name != "IFCMAPPEDITEM" {
                    return None;
                }
                let src = self.cx.get(attr(&e, 0))?;
                let rep = self.cx.get(attr(&src, 1))?;
                let ids: Vec<u64> = ctx::items(attr(&rep, 3))
                    .iter()
                    .filter_map(|v| v.as_ref_id())
                    .collect();
                find(&ids)
            })
        })?;
        Some(Color(c))
    }

    // ------------------------------------------------------------ geometry

    /// The body pieces of a product, in the world of the file (file units).
    fn eval_product(&self, e: &Entity, reps: &[Rc<Entity>], instancing: bool) -> EvalOut {
        let placement = attr(e, 5)
            .as_ref_id()
            .map_or(DAffine3::IDENTITY, |p| self.cx.placement(p));
        let ok = |src: u64| self.sym_source(src).is_some();
        let mut ev = Eval::new(
            &self.cx,
            self.opts.max_triangles.saturating_sub(self.triangles),
        );
        ev.fuel = Some(&self.fuel);
        ev.brep_cache = Some(&self.brep_cache);
        if instancing && self.opts.instances {
            ev.instancing = Some(&ok);
        }
        let annotation = e.name == "IFCANNOTATION";
        for r in reps {
            if !self.wanted_rep(r, annotation) {
                continue;
            }
            for i in items(attr(r, 3)) {
                if let Some(i) = i.as_ref_id() {
                    ev.item(i, &placement, 0);
                }
            }
        }
        EvalOut {
            pieces: ev.pieces,
            issues: ev.issues,
            approximate: ev.approximate,
            voids: ev.voids,
            tris: ev.tris,
        }
    }

    /// Which representations give the body (or, for annotations, the curves).
    fn wanted_rep(&self, r: &Entity, annotation: bool) -> bool {
        let ident = text(attr(r, 1)).unwrap_or("");
        let rtype = enum_of_or_text(attr(r, 2));
        if rtype == "BOUNDINGBOX" || ident.eq_ignore_ascii_case("Box") {
            return false;
        }
        if annotation {
            return true;
        }
        ident.eq_ignore_ascii_case("Body")
            || matches!(
                rtype.as_str(),
                "SWEPTSOLID"
                    | "BREP"
                    | "TESSELLATION"
                    | "MAPPEDREPRESENTATION"
                    | "CLIPPING"
                    | "SURFACEMODEL"
                    | "CSG"
                    | "ADVANCEDBREP"
                    | "ADVANCEDSWEPTSOLID"
                    | "SBSM"
            )
    }

    /// The `IfcBoundingBox` of a product, if it has a box representation.
    fn box_piece(&self, e: &Entity, reps: &[Rc<Entity>]) -> Option<Piece> {
        let placement = attr(e, 5)
            .as_ref_id()
            .map_or(DAffine3::IDENTITY, |p| self.cx.placement(p));
        let mut ev = Eval::new(&self.cx, 1000);
        for r in reps {
            let ident = text(attr(r, 1)).unwrap_or("");
            if ident.eq_ignore_ascii_case("Box") || enum_of_or_text(attr(r, 2)) == "BOUNDINGBOX" {
                for i in items(attr(r, 3)) {
                    if let Some(i) = i.as_ref_id() {
                        ev.item(i, &placement, 0);
                    }
                }
            }
        }
        ev.pieces.into_iter().next()
    }

    fn instances_ok(&self, pieces: &[Piece], _e: &Entity) -> bool {
        pieces.iter().all(|p| match p {
            Piece::Instance { xf, .. } => upright_plan(xf).is_some(),
            _ => false,
        })
    }

    /// The symbol source of a representation map: only upright extrusions without holes, and used twice or more.
    fn sym_source(&self, src_id: u64) -> Option<Rc<SymSrc>> {
        if let Some(hit) = self.sym_src.borrow().get(&src_id) {
            return hit.clone();
        }
        let result = self.build_sym_source(src_id).map(Rc::new);
        self.sym_src.borrow_mut().insert(src_id, result.clone());
        result
    }

    fn build_sym_source(&self, src_id: u64) -> Option<SymSrc> {
        if self.model.map_uses.get(&src_id).copied().unwrap_or(0) < 2 {
            return None;
        }
        let src = self.cx.ent(src_id)?;
        let rep = self.cx.get(attr(&src, 1))?;
        let mut ev = Eval::new(&self.cx, 200_000);
        for i in items(attr(&rep, 3)) {
            ev.item(i.as_ref_id()?, &DAffine3::IDENTITY, 0);
        }
        if !ev.issues.is_empty() || ev.pieces.is_empty() || ev.pieces.len() > 40 {
            return None;
        }
        let mut solids = Vec::new();
        for p in &ev.pieces {
            let Piece::Extrusion(x) = p else { return None };
            if !x.is_upright() || x.regions.len() != 1 || !x.regions[0].holes.is_empty() {
                return None;
            }
            let ring = curve::flatten(&x.regions[0].outer, true, x.tol.max(1e-9));
            let outline: Vec<DVec2> = ring
                .iter()
                .map(|p| x.xf.transform_point3(p.extend(0.0)).truncate())
                .collect();
            let (lo, hi) = x.mesh().bounds()?;
            if outline.len() < 3 || hi.z - lo.z <= 0.0 {
                return None;
            }
            solids.push((outline, lo.z, hi.z - lo.z));
        }
        Some(SymSrc { solids })
    }

    // ------------------------------------------------------------ emit: symbols

    #[allow(clippy::too_many_arguments)]
    fn symbols_out(
        &mut self,
        pieces: &[Piece],
        layer: (LayerId, f64),
        class: ClassId,
        data: BTreeMap<String, Json>,
        psets: &[model::PropSet],
        class_name: &str,
    ) {
        let mm = self.cx.units.length_mm;
        for p in pieces {
            let Piece::Instance { source, xf } = p else {
                continue;
            };
            let Some((_, rotation)) = upright_plan(xf) else {
                continue;
            };
            let Some(src) = self.sym_source(*source) else {
                continue;
            };
            let t = self.frame.point(xf.translation * mm);
            let dz = t.z - layer.1;
            let limit = kadakas_core::model::MAX_COORD / 10.0;
            let fits = dz.abs() < limit
                && src.solids.iter().all(|(o, base, h)| {
                    (base * mm + dz).abs() < limit
                        && (h * mm) > 0.0
                        && h * mm < limit
                        && o.iter().all(|p| p.abs().max_element() * mm < limit)
                })
                && center_ok(
                    self.frame.point2(xf.translation.truncate() * mm) - self.shift,
                    limit,
                );
            if !fits {
                self.b.report.skip(
                    class_name,
                    "a symbol instance is out of range or has no height",
                );
                continue;
            }
            let key = (*source, (dz * 10.0).round() as i64);
            let sym = match self.symbols.get(&key) {
                Some(s) => *s,
                None => {
                    let def = SymbolDef {
                        insertion: DVec2::ZERO,
                        items: src
                            .solids
                            .iter()
                            .map(|(o, _, _)| SymbolItem {
                                shape: Shape::Polyline {
                                    points: curve::corners(
                                        &o.iter().map(|p| *p * mm).collect::<Vec<_>>(),
                                    ),
                                    closed: true,
                                },
                                attrs: Attributes::default(),
                                class: None,
                            })
                            .collect(),
                        solids: src
                            .solids
                            .iter()
                            .map(|(o, base, h)| Solid3d::Extrusion {
                                outline: o.iter().map(|p| *p * mm).collect(),
                                base: base * mm + dz,
                                height: h * mm,
                                color: None,
                            })
                            .collect(),
                        ..SymbolDef::default()
                    };
                    let name = self.symbol_name(*source);
                    let id = self.b.resource(
                        &name,
                        ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def))),
                    );
                    self.symbols.insert(key, id);
                    id
                }
            };
            let center = self.frame.point2(xf.translation.truncate() * mm);
            let shape = Shape::Symbol {
                symbol: sym,
                at: center,
                rotation: rotation + self.frame.rotation,
                scale: 1.0,
                scale_y: None,
            };
            if self.b.add(
                class_name,
                shape,
                layer.0,
                class,
                Attributes::default(),
                data.clone(),
            ) {
                self.rows.push((ObjectId(self.b.last_id()), psets.to_vec()));
            }
        }
    }

    fn symbol_name(&mut self, source: u64) -> String {
        let taken: HashSet<String> = self
            .doc
            .resources
            .iter()
            .map(|r| r.name.to_ascii_lowercase())
            .collect();
        let base = format!("IFC mapped shape {source}");
        let mut name = base.clone();
        let mut n = 1;
        while taken.contains(&name.to_ascii_lowercase()) || self.symbol_names.contains(&name) {
            n += 1;
            name = format!("{base} ({n})");
        }
        self.symbol_names.insert(name.clone());
        name
    }

    // ------------------------------------------------------------ emit: terrain, areas, plants, meshes

    /// All solid pieces as one mesh in millimeters, placed by the frame.
    fn world_mesh(&self, pieces: &[Piece]) -> MeshData {
        let mm = self.cx.units.length_mm;
        let mut m = MeshData::default();
        for p in pieces {
            if let Some(pm) = geom::piece_mesh(p) {
                m.append(&pm);
            }
        }
        for v in &mut m.v {
            *v = self.frame.point(*v * mm);
        }
        if let Some(e) = m.exact.take() {
            let map = DAffine3::from_translation(self.frame.shift.extend(0.0))
                * DAffine3::from_rotation_z(self.frame.rotation)
                * DAffine3::from_scale(DVec3::splat(mm));
            m.exact = e.transformed(&map).ok().map(Box::new);
        }
        m.weld(0.01);
        m
    }

    #[allow(clippy::too_many_arguments)]
    fn terrain(
        &mut self,
        e: &Entity,
        pieces: &[Piece],
        layer: (LayerId, f64),
        class: ClassId,
        data: BTreeMap<String, Json>,
        psets: &[model::PropSet],
        class_name: &str,
        color: Option<Color>,
        name: &str,
        voids: usize,
    ) {
        let m = self.world_mesh(pieces);
        if m.t.is_empty() {
            self.b
                .report
                .skip(class_name, "the terrain has no triangles");
            return;
        }
        const MAX_POINTS: usize = 250_000;
        // A site model is made again from its points, so it would close the holes of the network.
        if self.opts.terrain == TerrainMode::SiteModel && voids > 0 {
            self.b.report.note(&format!(
                "a terrain has {voids} triangles that are holes or voids: it was imported as a 3D mesh so that they stay open"
            ));
        } else if self.opts.terrain == TerrainMode::SiteModel && m.v.len() <= MAX_POINTS {
            let points: Vec<DVec3> =
                m.v.iter()
                    .map(|p| DVec3::new(p.x, p.y, p.z - layer.1))
                    .collect();
            let shape = Shape::SiteModel {
                points,
                lines: Vec::new(),
                settings: kadakas_core::site::SiteSettings::default(),
            };
            if self.b.add(
                class_name,
                shape,
                layer.0,
                class,
                Attributes::default(),
                data,
            ) {
                self.rows.push((ObjectId(self.b.last_id()), psets.to_vec()));
            }
            return;
        }
        if self.opts.terrain == TerrainMode::SiteModel && voids == 0 {
            self.b.report.note(&format!(
                "the terrain has {} points, more than {MAX_POINTS}: it was imported as a 3D mesh",
                m.v.len()
            ));
        }
        self.add_mesh(e, m, class, layer.0, data, psets, class_name, color, name);
    }

    #[allow(clippy::too_many_arguments)]
    fn mesh_object(
        &mut self,
        e: &Entity,
        pieces: &[Piece],
        class: ClassId,
        layer: LayerId,
        data: BTreeMap<String, Json>,
        psets: &[model::PropSet],
        class_name: &str,
        color: Option<Color>,
        name: &str,
    ) {
        let m = self.world_mesh(pieces);
        if m.t.is_empty() {
            self.b
                .report
                .skip(class_name, "the geometry has no triangles");
            return;
        }
        self.add_mesh(e, m, class, layer, data, psets, class_name, color, name);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_mesh(
        &mut self,
        _e: &Entity,
        m: MeshData,
        class: ClassId,
        layer: LayerId,
        data: BTreeMap<String, Json>,
        psets: &[model::PropSet],
        class_name: &str,
        color: Option<Color>,
        name: &str,
    ) {
        self.triangles += m.t.len();
        let mut mesh = kadakas_core::mesh3d::Mesh3d {
            name: name.to_string(),
            vertices: m.v,
            triangles: m.t,
            color,
            exact: None,
            exact_unusable: None,
        };
        if let Some(e) = m.exact {
            mesh.set_exact(*e);
        }
        let shape = Shape::Mesh3d(Box::new(mesh));
        if self
            .b
            .add(class_name, shape, layer, class, Attributes::default(), data)
        {
            self.rows.push((ObjectId(self.b.last_id()), psets.to_vec()));
        }
    }

    /// A thin, flat, upright extrusion of vegetation is a landscape area.
    #[allow(clippy::too_many_arguments)]
    fn landscape_area(
        &mut self,
        _e: &Entity,
        pieces: &[Piece],
        layer: (LayerId, f64),
        class: ClassId,
        data: &BTreeMap<String, Json>,
        psets: &[model::PropSet],
        name: &str,
    ) -> bool {
        let [Piece::Extrusion(x)] = pieces else {
            return false;
        };
        let mm = self.cx.units.length_mm;
        if !x.is_upright() || x.regions.len() != 1 || x.height() * mm > 500.0 {
            return false;
        }
        let map = |vs: &[Vertex]| -> Vec<Vertex> {
            let mut out: Vec<Vertex> = vs
                .iter()
                .map(|v| {
                    let w = x.xf.transform_point3(v.p.extend(0.0)) * mm;
                    Vertex::new(self.frame.point2(w.truncate()), v.kind)
                })
                .collect();
            // A mirrored frame turns the winding; the area does not care.
            if out.len() > 1 && out[0].p == out[out.len() - 1].p {
                out.pop();
            }
            out
        };
        let boundary = map(&x.regions[0].outer);
        let holes: Vec<Vec<Vertex>> = x.regions[0].holes.iter().map(|h| map(h)).collect();
        if boundary.len() < 3 || curve::area(&boundary) < 0.5e6 {
            return false;
        }
        // Holes that do not lie inside the outline would fail the command: they are left out.
        let before = holes.len();
        let holes: Vec<Vec<Vertex>> = holes
            .into_iter()
            .filter(|h| {
                h.len() >= 3
                    && kadakas_core::landscape::holes_fit(&boundary, std::slice::from_ref(h))
            })
            .take(kadakas_core::landscape::MAX_HOLES)
            .collect();
        if holes.len() < before {
            self.b.report.note(
                "some holes of a landscape area were left out: they are not inside its outline",
            );
        }
        if (x.xf.translation.z * mm - layer.1).abs() > 1.0 {
            self.z_dropped += 1;
        }
        let shape = Shape::LandscapeArea {
            boundary,
            name: name.to_string(),
        };
        if self.b.add(
            "IfcGeographicElement",
            shape,
            layer.0,
            class,
            Attributes::default(),
            data.clone(),
        ) {
            let id = ObjectId(self.b.last_id());
            if !holes.is_empty() {
                // The builder moved the outline; the holes must move with it.
                let off = self.b.offset();
                let holes: Vec<Vec<Vertex>> = holes
                    .into_iter()
                    .map(|h| {
                        h.into_iter()
                            .map(|v| Vertex::new(v.p - off, v.kind))
                            .collect()
                    })
                    .collect();
                self.b.push(Command::SetLandscapeArea {
                    ids: vec![id],
                    patch: kadakas_core::landscape::LandscapePatch {
                        holes: Some(holes),
                        ..Default::default()
                    },
                });
            }
            self.rows.push((id, psets.to_vec()));
        }
        true
    }

    /// Vegetation with a size is a plant. The size is the box around the geometry.
    #[allow(clippy::too_many_arguments)]
    fn plant(
        &mut self,
        _e: &Entity,
        pieces: &[Piece],
        layer: (LayerId, f64),
        class: ClassId,
        data: &BTreeMap<String, Json>,
        psets: &[model::PropSet],
        name: &str,
    ) -> bool {
        let m = self.world_mesh(pieces);
        let Some((lo, hi)) = m.bounds() else {
            return false;
        };
        let d = hi - lo;
        let spread = d.x.max(d.y);
        let height = d.z;
        // A flat, wide shape is not a plant (for example an area draped over the terrain).
        if !(spread > 10.0
            && height >= 200.0
            && height >= spread * 0.15
            && spread < 80_000.0
            && height < 80_000.0)
        {
            return false;
        }
        let at = DVec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        // The botanical name from the property sets (our own export writes it), else the name of the element.
        let botanical = psets
            .iter()
            .flat_map(|s| &s.props)
            .find(|p| {
                ["species", "botanicalname", "botanical name"]
                    .contains(&p.name.to_ascii_lowercase().as_str())
            })
            .and_then(|p| p.value.as_str())
            .map(|t| clean_text(t.trim(), 200))
            .filter(|t| !t.is_empty());
        let species = botanical.unwrap_or_else(|| {
            if name.is_empty() {
                "IFC vegetation".to_string()
            } else {
                name.to_string()
            }
        });
        let shape = Shape::Plant {
            at,
            species,
            spread,
            height,
        };
        if self.b.add(
            "IfcGeographicElement",
            shape,
            layer.0,
            class,
            Attributes::default(),
            data.clone(),
        ) {
            self.rows.push((ObjectId(self.b.last_id()), psets.to_vec()));
        }
        true
    }

    // ------------------------------------------------------------ annotations

    fn annotation(&mut self, e: &Entity, reps: &[Rc<Entity>], class_name: &str, class: ClassId) {
        let out = self.eval_product(e, reps, false);
        let (layer, elevation) = self.layer_of(e.id);
        let name = text(attr(e, 2))
            .map(|n| clean_text(n, 200))
            .unwrap_or_default();
        let data = self.data_of(e, class_name, &name, "", "", &[]);
        let mm = self.cx.units.length_mm;
        let mut any = false;
        for p in &out.pieces {
            let shape = match p {
                Piece::Curve(c) => match &c.exact {
                    Some((verts, z)) if (z * mm - elevation).abs() < 1.0 => {
                        let points: Vec<Vertex> = verts
                            .iter()
                            .map(|v| Vertex::new(self.frame.point2(v.p * mm), v.kind))
                            .collect();
                        Shape::Polyline {
                            points,
                            closed: c.closed,
                        }
                    }
                    _ => Shape::Polyline3d {
                        points: c
                            .pts
                            .iter()
                            .map(|p| {
                                let w = self.frame.point(*p * mm);
                                DVec3::new(w.x, w.y, w.z - elevation)
                            })
                            .collect(),
                        closed: c.closed,
                    },
                },
                Piece::Point(p) => {
                    let w = self.frame.point(*p * mm);
                    Shape::Locus3d {
                        p: DVec3::new(w.x, w.y, w.z - elevation),
                    }
                }
                _ => continue,
            };
            any |= self.b.add(
                class_name,
                shape,
                layer,
                class,
                Attributes::default(),
                data.clone(),
            );
        }
        if !any {
            let why = out
                .issues
                .first()
                .cloned()
                .unwrap_or_else(|| "no curves or points".into());
            self.b.report.skip(class_name, &why);
        }
    }

    // ------------------------------------------------------------ records

    fn finish_records(&mut self) {
        if self.opts.property_sets != PropertyMode::Records || self.rows.is_empty() {
            return;
        }
        let rows = std::mem::take(&mut self.rows);
        props::write_records(&mut self.b, self.doc, rows);
    }
}

struct EvalOut {
    pieces: Vec<Piece>,
    issues: Vec<String>,
    approximate: bool,
    /// Triangles of an irregular network that were left out as holes or voids.
    voids: usize,
    #[allow(dead_code)]
    tris: usize,
}

fn enum_of_or_text(v: &crate::step::Value) -> String {
    enum_of(v)
        .or_else(|| text(v))
        .unwrap_or("")
        .to_ascii_uppercase()
}

/// The plan position and rotation of an instance whose frame is upright, without scale and mirror.
fn upright_plan(xf: &DAffine3) -> Option<(DVec2, f64)> {
    let m = xf.matrix3;
    let tol = 1e-6;
    let ok = (m.z_axis - DVec3::Z).length() < tol
        && (m.x_axis.length() - 1.0).abs() < tol
        && (m.y_axis.length() - 1.0).abs() < tol
        && m.x_axis.z.abs() < tol
        && m.y_axis.z.abs() < tol
        && m.x_axis.truncate().perp_dot(m.y_axis.truncate()) > 0.0
        && m.x_axis.dot(m.y_axis).abs() < tol;
    ok.then(|| (xf.translation.truncate(), m.x_axis.y.atan2(m.x_axis.x)))
}

fn center_ok(p: DVec2, limit: f64) -> bool {
    p.is_finite() && p.abs().max_element() < limit
}

fn same_crs(a: &str, b: &str) -> bool {
    use kadakas_crs::Crs;
    if a.trim().eq_ignore_ascii_case(b.trim()) {
        return true;
    }
    match (Crs::parse(a), Crs::parse(b)) {
        (Ok(x), Ok(y)) => x.epsg().is_some() && x.epsg() == y.epsg(),
        _ => false,
    }
}

/// Words in the object type that name vegetation.
const VEGETATION_WORDS: [&str; 9] = [
    "VEGETATION",
    "PLANT",
    "PLANTS",
    "TREE",
    "TREES",
    "SHRUB",
    "SHRUBS",
    "HEDGE",
    "BUSH",
];

/// Whole words of a text, in upper case: "Street tree" gives STREET and TREE.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_uppercase)
        .collect()
}

/// Terrain and vegetation among geographic elements and sites. The predefined type decides when it says
/// something (TERRAIN, VEGETATION, SOIL_BORING_POINT). The object type is read as whole words, and only
/// when the predefined type is USERDEFINED, NOTDEFINED or missing.
fn classify_geographic(
    e: &Entity,
    predefined: &str,
    object_type: &str,
    site_has_faces: impl FnOnce() -> bool,
) -> (bool, bool) {
    if e.name == "IFCSITE" {
        return (site_has_faces(), false);
    }
    if e.name != "IFCGEOGRAPHICELEMENT" {
        return (false, false);
    }
    let open = matches!(predefined, "" | "USERDEFINED" | "NOTDEFINED");
    if predefined == "TERRAIN" {
        return (true, false);
    }
    if predefined == "VEGETATION" {
        return (false, true);
    }
    if !open {
        return (false, false);
    }
    let w = words(object_type);
    let terrain = w.iter().any(|x| x == "TIN" || x == "TERRAIN");
    let vegetation = !terrain && w.iter().any(|x| VEGETATION_WORDS.contains(&x.as_str()));
    (terrain, vegetation)
}
