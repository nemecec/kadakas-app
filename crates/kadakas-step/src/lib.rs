//! Kadakas STEP (IOP-11): STEP files (ISO 10303-21) for Kadakas.
//!
//! - [`part21`] is the reader and writer of the file syntax. IFC uses it too.
//! - [`read`] imports the geometry of a STEP file (AP203, AP214, AP242): solids, surface models,
//!   tessellated items and curve sets. Faces on planes, cylinders, cones, spheres, tori,
//!   surfaces of revolution and of extrusion and B-spline surfaces are cut into triangles
//!   within a tolerance. Edges are cut once, so neighbouring faces share their points and a
//!   closed solid stays closed. Assemblies keep their names and placements, colors are kept.
//!
//! The reader never trusts the file: there are limits on entities, nesting, degrees, control
//! points and triangles, and a file cannot make it panic. It reads bytes and knows no files,
//! so it builds for wasm. The formats come from public knowledge of the standard and open
//! documentation. The text of the standard is not used.

mod assembly;
mod body;
mod ctx;
mod geom;
mod mesh2d;
pub mod part21;
mod read_brep;
mod tess;
mod units;

use std::collections::BTreeMap;

use glam::DAffine3;
use kadakas_core::exact::{self, ExactSolid};
use kadakas_meshio::{Format, Frame, Limits, MeshError, MeshSet, Part, Unit, Up};
use serde_json::{Value as Json, json};

use ctx::{Ctx, Fail};

/// What a read may use besides the limits of a mesh read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepLimits {
    pub max_entities: usize,
    pub max_control_points: usize,
    pub max_curve_points: usize,
    pub max_face_triangles: usize,
    pub max_assembly_depth: usize,
    pub max_instances: usize,
    /// Steps of work that a read may take whatever it makes (see `Ctx::work`). One step is
    /// about 65 ns of a current computer: a surface evaluation takes a few steps and a
    /// triangle of a curved face about 50.
    pub max_work: u64,
    /// More steps that each triangle made adds to the limit. A file that makes many
    /// triangles has earned the time; a file that makes none has not.
    pub work_per_triangle: u64,
    /// The longest read in milliseconds by the wall clock: a guard for slow computers and for
    /// builds without optimisation. The browser build has no clock and does not check it.
    pub max_millis: u64,
    /// The largest STEP file in bytes. The parsed file takes 9 to 14 times its size in memory
    /// (measured, see `docs/interop.md`), so this is below the general file limit.
    pub max_bytes: u64,
    /// The most memory, estimated, that the parsed entities may take.
    pub max_parse_memory: u64,
}

/// The largest STEP file that is read by default: 128 MiB, and 64 MiB in the browser, which
/// has less memory to give. A parsed file takes 9 to 14 times its size in memory.
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_MAX_BYTES: u64 = 128 * 1024 * 1024;
#[cfg(target_arch = "wasm32")]
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;

impl Default for StepLimits {
    fn default() -> Self {
        StepLimits {
            max_entities: 5_000_000,
            max_control_points: 262_144,
            max_curve_points: 200_000,
            max_face_triangles: 200_000,
            max_assembly_depth: 32,
            max_instances: 20_000,
            max_work: 60_000_000,
            work_per_triangle: 100,
            max_millis: 60_000,
            max_bytes: DEFAULT_MAX_BYTES,
            max_parse_memory: 3 * 1024 * 1024 * 1024,
        }
    }
}

/// The options of a read.
#[derive(Clone, Debug)]
pub struct StepOptions {
    /// The length unit of the numbers, instead of the unit that the file names.
    pub unit: Option<Unit>,
    /// The largest distance of a triangle from the surface, in millimeters. The default comes
    /// from the size of the file: 1/2000 of the diagonal, between 0.01 and 0.5.
    pub tolerance_mm: Option<f64>,
    /// The largest turn of the normal between neighbouring points, in degrees.
    pub angle_deg: f64,
    /// One part for each product and color (true) or for each body and color (false).
    pub per_product: bool,
    /// Keep the exact surfaces of each part next to its triangles (the default).
    pub keep_exact: bool,
    pub limits: Limits,
    pub step: StepLimits,
}

impl Default for StepOptions {
    fn default() -> Self {
        StepOptions {
            unit: None,
            tolerance_mm: None,
            angle_deg: 15.0,
            per_product: true,
            keep_exact: true,
            limits: Limits::default(),
            step: StepLimits::default(),
        }
    }
}

/// A line or polyline of the file, in millimeters.
#[derive(Clone, Debug, PartialEq)]
pub struct CurvePart {
    pub name: String,
    pub points: Vec<[f64; 3]>,
    pub closed: bool,
    pub color: Option<[u8; 4]>,
}

/// The accounting of a read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepReport {
    pub schemas: Vec<String>,
    pub entities_read: u64,
    pub entities_used: u64,
    pub entities_skipped: u64,
    /// Entity types that were read, with counts.
    pub used: BTreeMap<String, u64>,
    /// Entity types that were not read: (type, reason, count).
    pub skipped: Vec<(String, String, u64)>,
    /// Faces cut into triangles, by kind of surface: (kind, faces, triangles).
    pub tessellated: Vec<(String, u64, u64)>,
    pub products: usize,
    pub instances: usize,
    pub unit_mm: Vec<f64>,
    pub unit_from_file: bool,
    pub tolerance_mm: Option<f64>,
    pub angle_deg: f64,
    pub welded_vertices: u64,
    /// Steps of work that the read took (see `StepLimits::max_work`).
    pub work_units: u64,
    /// Parts that keep their exact surfaces, and the size of that data.
    pub exact_kept: usize,
    pub exact_bytes: usize,
    /// Parts without exact data, with the reason: (part, reason).
    pub exact_dropped: Vec<(String, String)>,
}

impl StepReport {
    pub fn to_json(&self) -> Json {
        json!({
            "schemas": self.schemas,
            "entities_read": self.entities_read,
            "entities_used": self.entities_used,
            "entities_skipped": self.entities_skipped,
            "used": self.used.iter().map(|(k, v)| json!({"entity": k, "count": v})).collect::<Vec<_>>(),
            "skipped": self.skipped.iter().map(|(e, r, c)| json!({"entity": e, "reason": r, "count": c})).collect::<Vec<_>>(),
            "tessellated": self.tessellated.iter().map(|(k, f, t)| json!({"surface": k, "faces": f, "triangles": t})).collect::<Vec<_>>(),
            "products": self.products,
            "instances": self.instances,
            "length_unit_mm": self.unit_mm,
            "unit_from_file": self.unit_from_file,
            "tolerance_mm": self.tolerance_mm,
            "angle_deg": self.angle_deg,
            "work_units": self.work_units,
            "welded_vertices": self.welded_vertices,
            "exact": {
                "kept_parts": self.exact_kept,
                "approx_bytes": self.exact_bytes,
                "without": self.exact_dropped.iter().map(|(p, r)| json!({"part": p, "reason": r})).collect::<Vec<_>>(),
            },
        })
    }
}

/// The result of a read.
#[derive(Debug)]
pub struct StepImport {
    pub set: MeshSet,
    /// The exact surfaces of each part of `set` (same order), in millimeters and in the frame
    /// of the part. `None` for a part that has none: tessellated items, an option that turned
    /// them off, or a payload over the cap (the report says which).
    pub exact: Vec<Option<ExactSolid>>,
    pub curves: Vec<CurvePart>,
    pub report: StepReport,
}

const KNOWN_SCHEMAS: &[&str] = &[
    "AUTOMOTIVE_DESIGN",
    "CONFIG_CONTROL_DESIGN",
    "AP203",
    "AP214",
    "AP242",
    "AP209",
    "MANAGED_MODEL_BASED_3D_ENGINEERING",
    "STRUCTURAL_FRAME_SCHEMA",
    "GEOMETRY_SCHEMA",
];

/// Vertices and triangles of one color of a product.
type Soup = (Vec<glam::DVec3>, Vec<[u32; 3]>);

/// The parts of one product while they are collected.
struct Acc {
    name: String,
    by_color: BTreeMap<Option<[u8; 4]>, Soup>,
    exact: Option<ExactSolid>,
    exact_ok: bool,
    /// Why a body of the part has no exact data, when it is more than "given as triangles".
    lost: Option<&'static str>,
}

/// Why an entity type was not used.
fn unused_reason(name: &str) -> &'static str {
    const GEOMETRY: &[&str] = &[
        "SURFACE",
        "CURVE",
        "POINT",
        "DIRECTION",
        "AXIS",
        "EDGE",
        "LOOP",
        "FACE",
        "VERTEX",
        "SHELL",
        "BREP",
        "SOLID",
        "LINE",
        "CIRCLE",
        "VECTOR",
        "TRIANGULATED",
        "TESSELLATED",
        "COORDINATES",
        "POLYLINE",
    ];
    if GEOMETRY.iter().any(|g| name.contains(g)) {
        "geometry that no product or item refers to"
    } else if name.contains("STYLE") || name.contains("COLOUR") || name.contains("PRESENTATION") {
        "presentation data that no imported item uses"
    } else {
        "product data, units or other information without geometry"
    }
}

/// Reads a STEP file into parts in millimeters, Z up.
pub fn read(bytes: &[u8], opts: &StepOptions) -> Result<StepImport, MeshError> {
    let max_bytes = opts.limits.max_bytes.min(opts.step.max_bytes);
    if bytes.len() as u64 > max_bytes {
        return Err(MeshError::FileTooLarge {
            bytes: bytes.len() as u64,
            limit: max_bytes,
        });
    }
    let text = String::from_utf8_lossy(bytes);
    if !text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with("ISO-10303-21")
    {
        return Err(MeshError::Malformed(
            "this is not a STEP file: it does not start with ISO-10303-21".into(),
        ));
    }
    let file = part21::parse_limited(&text, opts.step.max_entities, opts.step.max_parse_memory)
        .map_err(|e| match e {
            part21::ParseError::TooManyEntities { limit } => MeshError::TooManyEntities {
                limit: limit as u64,
            },
            part21::ParseError::TooMuchMemory { estimate, limit } => MeshError::TooMuchMemory {
                bytes: estimate,
                limit,
            },
            part21::ParseError::Syntax(m) => {
                if m.contains("end of file") || m.contains("not closed") || m.contains("no DATA") {
                    MeshError::Truncated {
                        what: "the STEP file".into(),
                        have: bytes.len() as u64,
                        need: bytes.len() as u64 + 1,
                    }
                } else {
                    MeshError::Malformed(format!("STEP syntax: {m}"))
                }
            }
        })?;
    drop(text);
    if file.entities.len() > opts.step.max_entities {
        return Err(MeshError::TooManyEntities {
            limit: opts.step.max_entities as u64,
        });
    }
    let mut c = Ctx::new(&file, opts, &opts.step);
    let schemas = file.schemas();
    if !schemas.is_empty()
        && !schemas.iter().any(|s| {
            KNOWN_SCHEMAS
                .iter()
                .any(|k| s.to_ascii_uppercase().contains(k))
        })
    {
        c.note(format!(
            "the schema {} is not one of AP203, AP214 or AP242: the file is read as if it were",
            schemas.join(", ")
        ));
    }
    if !file.duplicates.is_empty() {
        let shown: Vec<String> = file
            .duplicates
            .iter()
            .take(5)
            .map(|n| format!("#{n}"))
            .collect();
        c.note(format!(
            "{} entity numbers are defined more than once ({}{}): the first definition is used",
            file.duplicates.len(),
            shown.join(", "),
            if file.duplicates.len() > 5 {
                ", ..."
            } else {
                ""
            }
        ));
    }
    c.read_colors();
    let structure = match c.structure() {
        Ok(s) => s,
        Err(Fail::Abort(e)) => return Err(e),
        Err(Fail::Skip(why)) => return Err(MeshError::Unsupported(why)),
    };
    let mut parts: Vec<Part> = Vec::new();
    let mut curves: Vec<CurvePart> = Vec::new();
    let mut welded = 0u64;
    let mut degenerate = 0u64;
    let mut cancelled = 0u64;
    let mut exacts: Vec<Option<ExactSolid>> = Vec::new();
    let mut exact_bytes = 0usize;
    let mut exact_dropped: Vec<(String, String)> = Vec::new();
    // The tolerance of the first representation, in millimeters: the report and the weld use
    // the same number, whatever units other representations have.
    let tol_mm = c.tol_mm.unwrap_or(c.tol);
    let weld_eps = (tol_mm * 1e-3).max(1e-3);
    for inst in &structure.instances {
        let moved = if inst.placement == DAffine3::IDENTITY {
            None
        } else {
            Some(inst.geom.transformed(&inst.placement))
        };
        let geom = moved.as_ref().unwrap_or(&inst.geom);
        // Bodies: one group for a product, or each body on its own.
        let mut groups: Vec<Acc> = Vec::new();
        for b in &geom.bodies {
            let name = if opts.per_product || b.name.is_empty() {
                inst.name.clone()
            } else {
                b.name.clone()
            };
            let name = if name.is_empty() {
                b.name.clone()
            } else {
                name
            };
            let idx = if opts.per_product {
                groups.iter().position(|g| g.name == name)
            } else {
                None
            };
            let g = match idx {
                Some(i) => &mut groups[i],
                None => {
                    groups.push(Acc {
                        name,
                        by_color: BTreeMap::new(),
                        exact: None,
                        exact_ok: true,
                        lost: None,
                    });
                    groups.last_mut().expect("just pushed")
                }
            };
            for (color, tris) in &b.groups {
                let e = g.by_color.entry(*color).or_default();
                let base = e.0.len() as u32;
                e.0.extend_from_slice(&b.positions);
                e.1.extend(tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
            }
            match &b.exact {
                Some(x) => match &mut g.exact {
                    Some(have) => have.merge(x),
                    None => g.exact = Some(x.clone()),
                },
                None => {
                    g.exact_ok = false;
                    g.lost = g.lost.or(b.exact_lost);
                }
            }
        }
        for g in groups {
            for (color, (pos, tris)) in g.by_color {
                // A number out of range (a huge unit) is refused like in the other readers.
                let far = pos.iter().fold(0.0f64, |m, p| m.max(p.abs().max_element()));
                if !far.is_finite() || far > opts.limits.max_coord_mm {
                    return Err(MeshError::OutOfRange {
                        what: format!("a vertex of '{}'", g.name),
                        value: far,
                        limit: opts.limits.max_coord_mm,
                    });
                }
                // The weld hashes every vertex and every triangle of every copy.
                match c.charge(pos.len() as u64 * 3 + tris.len() as u64 * 2) {
                    Ok(()) => {}
                    Err(Fail::Abort(e)) => return Err(e),
                    Err(Fail::Skip(why)) => return Err(MeshError::Unsupported(why)),
                }
                let (positions, triangles, w, d, x) = weld(&pos, &tris, weld_eps);
                welded += w;
                degenerate += d;
                cancelled += x;
                if triangles.is_empty() {
                    continue;
                }
                let name = if g.name.is_empty() {
                    "STEP".to_string()
                } else {
                    g.name.clone()
                };
                // The exact data of this color.
                let payload = match (&g.exact, g.exact_ok, opts.keep_exact) {
                    (_, _, false) => Err("the option keep_exact is off".to_string()),
                    (None, _, _) | (_, false, _) => Err(g.lost.map_or_else(
                        || {
                            "the part has triangles that were given, not cut from surfaces"
                                .to_string()
                        },
                        str::to_string,
                    )),
                    (Some(x), true, true) => {
                        let mut x = x.clone();
                        x.retain_faces(|f| f.color == color);
                        if x.faces.is_empty() {
                            Err("no faces are left for this color".to_string())
                        } else if x.approx_bytes() > exact::MAX_OBJECT_BYTES {
                            Err(format!(
                                "the exact data is about {} bytes, over the limit of {} bytes for one object: only the triangles are kept",
                                x.approx_bytes(),
                                exact::MAX_OBJECT_BYTES
                            ))
                        } else {
                            Ok(x)
                        }
                    }
                };
                match payload {
                    Ok(x) => {
                        exact_bytes += x.approx_bytes();
                        exacts.push(Some(x));
                    }
                    Err(why) => {
                        if opts.keep_exact {
                            exact_dropped.push((name.clone(), why));
                        }
                        exacts.push(None);
                    }
                }
                parts.push(Part {
                    name,
                    material: String::new(),
                    color,
                    positions,
                    triangles,
                });
            }
        }
        for cv in &geom.curves {
            curves.push(CurvePart {
                name: if inst.name.is_empty() {
                    cv.name.clone()
                } else {
                    inst.name.clone()
                },
                points: cv.points.iter().map(|p| p.to_array()).collect(),
                closed: cv.closed,
                color: cv.color,
            });
        }
    }
    let (used_n, used, unseen) = c.accounting();
    let mut skipped: Vec<(String, String, u64)> = c
        .skips
        .iter()
        .map(|((e, r), n)| (e.clone(), r.clone(), *n))
        .collect();
    for (name, n) in unseen {
        let reason = unused_reason(&name);
        skipped.push((name, reason.to_string(), n));
    }
    let skipped_n: u64 = skipped.iter().map(|s| s.2).sum();
    let report = StepReport {
        schemas,
        entities_read: file.entities.len() as u64,
        entities_used: used_n,
        entities_skipped: skipped_n,
        used,
        skipped: skipped.clone(),
        tessellated: c
            .tessellated
            .iter()
            .map(|(k, s)| (k.clone(), s.faces, s.triangles))
            .collect(),
        products: structure.products,
        instances: structure.instances.len(),
        unit_mm: c.unit_scales.clone(),
        unit_from_file: c.unit_from_file && opts.unit.is_none(),
        tolerance_mm: Some(tol_mm),
        angle_deg: opts.angle_deg,
        welded_vertices: welded,
        work_units: c.work,
        exact_kept: exacts.iter().flatten().count(),
        exact_bytes,
        exact_dropped,
    };
    if cancelled > 0 {
        c.note(format!(
            "{cancelled} triangles that repeat or cancel each other were removed"
        ));
    }
    let notes = c.notes.clone();
    let frame = Frame {
        unit: Unit::Millimetre,
        up: Up::Z,
    };
    if parts.is_empty() && curves.is_empty() {
        let mut why: Vec<String> = skipped
            .iter()
            .filter(|s| s.1.contains("not") || s.1.contains("cannot") || s.1.contains("face"))
            .take(3)
            .map(|s| format!("{} x {} ({})", s.2, s.0, s.1))
            .collect();
        if why.is_empty() {
            return Err(MeshError::Empty);
        }
        why.insert(0, "the file has no geometry that could be read".into());
        return Err(MeshError::Unsupported(why.join("; ")));
    }
    let set = if parts.is_empty() {
        MeshSet {
            format: Format::Step,
            frame,
            parts,
            totals: Default::default(),
            notes,
            origin_mm: None,
        }
    } else {
        MeshSet::finish(Format::Step, frame, parts, degenerate, notes, &opts.limits)?
    };
    // A set that was joined by color has parts of other faces than the payloads.
    let mut report = report;
    let exact = if set.parts.len() == exacts.len() {
        exacts
    } else {
        report.exact_kept = 0;
        report.exact_dropped.push((
            "all".into(),
            "the parts were joined by color, so the exact data does not fit them".into(),
        ));
        vec![None; set.parts.len()]
    };
    Ok(StepImport {
        set,
        exact,
        curves,
        report,
    })
}

/// Joins vertices that lie within `eps` of each other, drops triangles with a repeated corner
/// and vertices that no triangle uses. Returns the vertices, the triangles, the number of
/// vertices joined and of triangles dropped.
fn weld(
    pos: &[glam::DVec3],
    tris: &[[u32; 3]],
    eps: f64,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>, u64, u64, u64) {
    use std::collections::HashMap;
    let inv = 1.0 / eps;
    let cell = |p: &glam::DVec3| -> [i64; 3] {
        [
            (p.x * inv).round() as i64,
            (p.y * inv).round() as i64,
            (p.z * inv).round() as i64,
        ]
    };
    let mut grid: HashMap<[i64; 3], u32> = HashMap::with_capacity(pos.len());
    let mut remap: Vec<u32> = Vec::with_capacity(pos.len());
    let mut kept: Vec<glam::DVec3> = Vec::new();
    let mut joined = 0u64;
    for p in pos {
        let c = cell(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(&i) = grid.get(&[
                        c[0].saturating_add(dx),
                        c[1].saturating_add(dy),
                        c[2].saturating_add(dz),
                    ]) && kept[i as usize].distance(*p) <= eps
                    {
                        found = Some(i);
                        break 'search;
                    }
                }
            }
        }
        match found {
            Some(i) => {
                joined += 1;
                remap.push(i);
            }
            None => {
                let i = kept.len() as u32;
                kept.push(*p);
                grid.insert(c, i);
                remap.push(i);
            }
        }
    }
    let mut out_tris = Vec::with_capacity(tris.len());
    let mut degenerate = 0u64;
    let mut cancelled = 0u64;
    let mut used = vec![u32::MAX; kept.len()];
    let mut out_pos: Vec<[f64; 3]> = Vec::new();
    for t in tris {
        let m = [
            remap[t[0] as usize],
            remap[t[1] as usize],
            remap[t[2] as usize],
        ];
        if m[0] == m[1] || m[1] == m[2] || m[0] == m[2] {
            degenerate += 1;
            continue;
        }
        let mut n = [0u32; 3];
        for k in 0..3 {
            if used[m[k] as usize] == u32::MAX {
                used[m[k] as usize] = out_pos.len() as u32;
                out_pos.push(kept[m[k] as usize].to_array());
            }
            n[k] = used[m[k] as usize];
        }
        out_tris.push(n);
    }
    // Two triangles with the same corners cancel when they face opposite ways (the sliver at
    // a pole that two sides of a seam both make) and one of them is enough when they face
    // the same way.
    let mut seen: HashMap<[u32; 3], Vec<(usize, bool)>> = HashMap::new();
    for (i, t) in out_tris.iter().enumerate() {
        let mut key = *t;
        key.sort_unstable();
        // The winding: whether the corners are an even permutation of the sorted ones.
        let even = (t[0] == key[0] && t[1] == key[1])
            || (t[1] == key[0] && t[2] == key[1])
            || (t[2] == key[0] && t[0] == key[1]);
        seen.entry(key).or_default().push((i, even));
    }
    let mut remove = vec![false; out_tris.len()];
    for list in seen.values() {
        if list.len() < 2 {
            continue;
        }
        let mut plus: Vec<usize> = list.iter().filter(|x| x.1).map(|x| x.0).collect();
        let mut minus: Vec<usize> = list.iter().filter(|x| !x.1).map(|x| x.0).collect();
        while let (Some(a), Some(b)) = (plus.pop(), minus.pop()) {
            remove[a] = true;
            remove[b] = true;
            cancelled += 2;
        }
        for extra in plus.into_iter().skip(1).chain(minus.into_iter().skip(1)) {
            remove[extra] = true;
            cancelled += 1;
        }
    }
    let mut k = 0;
    out_tris.retain(|_| {
        k += 1;
        !remove[k - 1]
    });
    (out_pos, out_tris, joined, degenerate, cancelled)
}

/// The triangles of a payload, cut again at another tolerance.
#[derive(Debug)]
pub struct Retessellation {
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub notes: Vec<String>,
    /// Faces that could not be cut (their number in the payload and the reason).
    pub skipped: Vec<(usize, String)>,
    /// Faces and triangles by kind of surface.
    pub kinds: Vec<(String, u64, u64)>,
}

/// Cuts the faces of an exact solid into triangles again, with a chord `tolerance_mm` and a
/// largest turn of the normal `angle_deg`. The payload is checked first, because it may come
/// from a file. Nothing is read from a STEP file.
pub fn retessellate(
    solid: &ExactSolid,
    tolerance_mm: f64,
    angle_deg: f64,
    limits: &Limits,
) -> Result<Retessellation, MeshError> {
    solid
        .validate()
        .map_err(|why| MeshError::Malformed(format!("the exact data is not valid: {why}")))?;
    if !(tolerance_mm.is_finite() && tolerance_mm > 0.0 && angle_deg.is_finite() && angle_deg > 0.0)
    {
        return Err(MeshError::Malformed(
            "the tolerance and the angle must be positive numbers".into(),
        ));
    }
    let params = tess::TessParams {
        tol: tolerance_mm,
        angle: angle_deg.to_radians().clamp(0.02, 1.5),
        max_face_triangles: StepLimits::default().max_face_triangles,
        triangles_left: limits.max_triangles as usize,
    };
    let (mut vertices, mut triangles) = (0u64, 0u64);
    let (max_work, per_triangle) = {
        let d = StepLimits::default();
        (d.max_work, d.work_per_triangle)
    };
    let max_millis = StepLimits::default().max_millis;
    let mut clock = ctx::Clock::default();
    let mut work = 0u64;
    let mut spend = |c: tess::Cost| -> Result<(), MeshError> {
        let (v, t) = match c {
            tess::Cost::Output(v, t) => (v, t),
            tess::Cost::Work(n) => {
                work = work.saturating_add(n);
                let allowed = max_work.saturating_add(per_triangle.saturating_mul(triangles));
                if work > allowed {
                    return Err(MeshError::TooMuchWork {
                        units: work,
                        limit: allowed,
                    });
                }
                if let Some((millis, limit)) = clock.check(work, max_millis) {
                    return Err(MeshError::TooMuchTime { millis, limit });
                }
                return Ok(());
            }
        };
        vertices += v;
        triangles += t;
        if triangles > limits.max_triangles {
            return Err(MeshError::TooManyTriangles {
                count: triangles,
                limit: limits.max_triangles,
            });
        }
        if vertices > limits.max_vertices {
            return Err(MeshError::TooManyVertices {
                count: vertices,
                limit: limits.max_vertices,
            });
        }
        let need = Limits::memory_of(vertices, triangles);
        if need > limits.max_memory {
            return Err(MeshError::TooMuchMemory {
                bytes: need,
                limit: limits.max_memory,
            });
        }
        Ok(())
    };
    let t = tess::tessellate(solid, &params, &mut spend)?;
    // A face of a closed shell that cannot be cut now would open the solid: the old mesh stays.
    let opened: Vec<&(usize, String)> = t
        .skipped
        .iter()
        .filter(|(f, _)| {
            solid
                .faces
                .get(*f)
                .and_then(|face| solid.shells.get(face.shell as usize))
                .is_some_and(|sh| sh.closed)
        })
        .collect();
    if let Some((face, why)) = opened.first() {
        return Err(MeshError::Unsupported(format!(
            "{} faces of a closed solid cannot be cut at this tolerance (face {face}: {why}), so the solid would open: the old mesh is kept",
            opened.len()
        )));
    }
    let mut pos = t.positions;
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for (_, g) in t.groups {
        tris.extend(g);
    }
    if tris.is_empty() {
        return Err(MeshError::Empty);
    }
    let (positions, triangles, _, _, _) = {
        let eps = (tolerance_mm * 1e-3).max(1e-3);
        weld(&std::mem::take(&mut pos), &tris, eps)
    };
    Ok(Retessellation {
        positions,
        triangles,
        notes: t.notes,
        skipped: t.skipped,
        kinds: t.kinds.into_iter().map(|(k, (f, t))| (k, f, t)).collect(),
    })
}
