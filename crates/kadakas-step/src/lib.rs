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
mod brep;
mod ctx;
mod curve;
mod geom;
mod mesh2d;
mod nurbs;
pub mod part21;
mod surface;
mod units;

use std::collections::BTreeMap;

use glam::DAffine3;
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
}

impl Default for StepLimits {
    fn default() -> Self {
        StepLimits {
            max_entities: 5_000_000,
            max_control_points: 262_144,
            max_curve_points: 200_000,
            max_face_triangles: 200_000,
            max_assembly_depth: 32,
            max_instances: 20_000,
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
            "welded_vertices": self.welded_vertices,
        })
    }
}

/// The result of a read.
#[derive(Debug)]
pub struct StepImport {
    pub set: MeshSet,
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

/// Why an entity type was not used.
fn unused_reason(name: &str) -> &'static str {
    const GEOMETRY: &[&str] = &[
        "SURFACE", "CURVE", "POINT", "DIRECTION", "AXIS", "EDGE", "LOOP", "FACE", "VERTEX",
        "SHELL", "BREP", "SOLID", "LINE", "CIRCLE", "VECTOR", "TRIANGULATED", "TESSELLATED",
        "COORDINATES", "POLYLINE",
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
    if bytes.len() as u64 > opts.limits.max_bytes {
        return Err(MeshError::FileTooLarge {
            bytes: bytes.len() as u64,
            limit: opts.limits.max_bytes,
        });
    }
    let text = String::from_utf8_lossy(bytes);
    if !text.trim_start_matches('\u{feff}').trim_start().starts_with("ISO-10303-21") {
        return Err(MeshError::Malformed(
            "this is not a STEP file: it does not start with ISO-10303-21".into(),
        ));
    }
    let file = part21::parse(&text).map_err(|m| {
        if m.contains("end of file") || m.contains("not closed") || m.contains("no DATA") {
            MeshError::Truncated {
                what: "the STEP file".into(),
                have: bytes.len() as u64,
                need: bytes.len() as u64 + 1,
            }
        } else {
            MeshError::Malformed(format!("STEP syntax: {m}"))
        }
    })?;
    drop(text);
    if file.entities.len() > opts.step.max_entities {
        return Err(MeshError::TooManyParts {
            count: file.entities.len() as u64,
            limit: opts.step.max_entities as u64,
        });
    }
    let mut c = Ctx::new(&file, opts, &opts.step);
    let schemas = file.schemas();
    if !schemas.is_empty()
        && !schemas
            .iter()
            .any(|s| KNOWN_SCHEMAS.iter().any(|k| s.to_ascii_uppercase().contains(k)))
    {
        c.note(format!(
            "the schema {} is not one of AP203, AP214 or AP242: the file is read as if it were",
            schemas.join(", ")
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
    let tol_for_weld = c.tol.max(1e-9);
    let weld_eps = (opts.tolerance_mm.unwrap_or(0.05) * 1e-3).max(1e-6).min(tol_for_weld * 1e6);
    for inst in &structure.instances {
        let moved = if inst.placement == DAffine3::IDENTITY {
            None
        } else {
            Some(inst.geom.transformed(&inst.placement))
        };
        let geom = moved.as_ref().unwrap_or(&inst.geom);
        // Bodies: one group for a product, or each body on its own.
        let mut groups: Vec<(String, BTreeMap<Option<[u8; 4]>, (Vec<glam::DVec3>, Vec<[u32; 3]>)>)> =
            Vec::new();
        for b in &geom.bodies {
            let name = if opts.per_product || b.name.is_empty() {
                inst.name.clone()
            } else {
                b.name.clone()
            };
            let name = if name.is_empty() { b.name.clone() } else { name };
            let idx = if opts.per_product {
                groups.iter().position(|g| g.0 == name).filter(|_| groups.len() == 1)
            } else {
                None
            };
            let g = match idx {
                Some(i) => &mut groups[i].1,
                None => {
                    groups.push((name, BTreeMap::new()));
                    &mut groups.last_mut().expect("just pushed").1
                }
            };
            for (color, tris) in &b.groups {
                let e = g.entry(*color).or_default();
                let base = e.0.len() as u32;
                e.0.extend_from_slice(&b.positions);
                e.1.extend(tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
            }
        }
        for (name, by_color) in groups {
            for (color, (pos, tris)) in by_color {
                let (positions, triangles, w, d) = weld(&pos, &tris, weld_eps);
                welded += w;
                degenerate += d;
                if triangles.is_empty() {
                    continue;
                }
                parts.push(Part {
                    name: if name.is_empty() { "STEP".into() } else { name.clone() },
                    material: String::new(),
                    color,
                    positions,
                    triangles,
                });
            }
        }
        for cv in &geom.curves {
            curves.push(CurvePart {
                name: if inst.name.is_empty() { cv.name.clone() } else { inst.name.clone() },
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
        tolerance_mm: Some(c.tol * c.unit_scales.first().copied().unwrap_or(1.0)),
        angle_deg: opts.angle_deg,
        welded_vertices: welded,
    };
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
    Ok(StepImport { set, curves, report })
}

/// Joins vertices that lie within `eps` of each other, drops triangles with a repeated corner
/// and vertices that no triangle uses. Returns the vertices, the triangles, the number of
/// vertices joined and of triangles dropped.
fn weld(
    pos: &[glam::DVec3],
    tris: &[[u32; 3]],
    eps: f64,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>, u64, u64) {
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
                    if let Some(&i) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz])
                        && kept[i as usize].distance(*p) <= eps
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
    let mut dropped = 0u64;
    let mut used = vec![u32::MAX; kept.len()];
    let mut out_pos: Vec<[f64; 3]> = Vec::new();
    for t in tris {
        let m = [remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]];
        if m[0] == m[1] || m[1] == m[2] || m[0] == m[2] {
            dropped += 1;
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
        let even = (t[0] == key[0] && t[1] == key[1]) || (t[1] == key[0] && t[2] == key[1]) || (t[2] == key[0] && t[0] == key[1]);
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
            dropped += 2;
        }
        for extra in plus.into_iter().skip(1).chain(minus.into_iter().skip(1)) {
            remove[extra] = true;
            dropped += 1;
        }
    }
    let mut k = 0;
    out_tris.retain(|_| {
        k += 1;
        !remove[k - 1]
    });
    (out_pos, out_tris, joined, dropped)
}
