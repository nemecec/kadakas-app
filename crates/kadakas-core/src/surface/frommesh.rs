//! A triangle mesh into a surface model (MOD-12).
//!
//! The steps, all of them deterministic:
//!
//! 1. The mesh moves so that its center is near the local origin (the model transform puts it
//!    back). Vertices within the length tolerance of each other weld. Triangles with two equal
//!    corners, with no area, or that repeat an earlier one are dropped and counted.
//! 2. Triangles that share an edge and lie in one plane (within the plane tolerance of the
//!    edits) merge into one face, from the largest triangle outward. The boundary of a region is
//!    its outer loop and its holes. A region that touches itself at a vertex, or whose loops do
//!    not give one outer loop, stays as separate triangles.
//! 3. A vertex that has two edges only, in a straight line, goes (as the edits heal it).
//! 4. The counts are compared with the limits of a surface model. Over a limit the conversion is
//!    refused and the answer has every count.
//! 5. With an angle, the soften engine of MOD-08 softens the edges under the angle.
//!
//! Faces keep the winding of the triangles: the front is the side the triangles face.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};

use glam::DVec3;
use serde::Serialize;

use super::geom::{self, Plane, on_plane, plane_tolerance_at, tolerance_at};
use super::{
    Edge, EdgeId, Face, FaceId, MAX_EDGES, MAX_FACES, MAX_LOOP_VERTICES, MAX_VERTICES,
    SurfaceModel, VertexId, key,
};
use crate::model::MAX_COORD;

/// The most triangles that one conversion works on. A larger mesh is refused with its count.
pub const MAX_CONVERT_TRIANGLES: usize = 2_000_000;

/// What a conversion does.
#[derive(Clone, Debug, PartialEq)]
pub struct ConvertOptions {
    /// The weld distance in mm. `None`: the length tolerance of the model (0.01 mm).
    pub tolerance: Option<f64>,
    /// Soften the edges under this angle in degrees (0 to 180). `None`: leave every edge hard.
    pub soften_below_angle: Option<f64>,
    /// Merge coplanar triangles into faces. On by default; off keeps every triangle a face.
    pub merge: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            tolerance: None,
            soften_below_angle: None,
            merge: true,
        }
    }
}

/// One count that is over its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct OverLimit {
    pub what: &'static str,
    pub count: usize,
    pub limit: usize,
}

/// Why a mesh cannot become a surface model.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ConvertError {
    #[error("the mesh has no triangle with an area")]
    Empty,
    #[error("{0}")]
    Options(String),
    #[error("the mesh has {count} triangles, more than the {limit} that one conversion works on")]
    TooManyTriangles { count: usize, limit: usize },
    #[error("a vertex is not finite or out of range")]
    BadVertex,
    #[error("a triangle refers to vertex {index}, and there are {count}")]
    BadIndex { index: u32, count: usize },
    /// The result passes a limit of a surface model. `over` names each count that is too large,
    /// and `counts` is what the result would have.
    #[error("{}", over_text(.over, .counts))]
    OverLimit {
        over: Vec<OverLimit>,
        counts: Counts,
    },
    #[error("the surface model is not valid: {0}")]
    Invalid(String),
}

fn over_text(over: &[OverLimit], c: &Counts) -> String {
    let list: Vec<String> = over
        .iter()
        .map(|o| format!("{} {} (limit {})", o.count, o.what, o.limit))
        .collect();
    format!(
        "the mesh is too large for a surface model: {}. The result of the conversion would have {} vertices, {} edges and {} faces",
        list.join(", "),
        c.vertices,
        c.edges,
        c.faces
    )
}

/// The size of a surface model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
}

/// What a conversion did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ConvertReport {
    pub source_vertices: usize,
    pub source_triangles: usize,
    /// Vertices that welded into another.
    pub welded_vertices: usize,
    /// Triangles dropped because two corners became one vertex, or because they have no area and
    /// belong to no face.
    pub degenerate_triangles: usize,
    /// Triangles dropped because an earlier triangle has the same corners.
    pub duplicate_triangles: usize,
    /// Triangles with no area (corners in a line) that joined a face as the glue of its
    /// triangulation.
    pub joined_slivers: usize,
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
    /// Faces that have at least one hole.
    pub faces_with_holes: usize,
    /// Triangles that are now part of a larger face.
    pub merged_triangles: usize,
    /// Regions of coplanar triangles that could not become one face and stayed triangles.
    pub unmerged_regions: usize,
    /// Vertices that were taken away because they lay in a straight line between two edges.
    pub healed_vertices: usize,
    /// The weld distance that was used, in mm.
    pub tolerance_mm: f64,
    /// Edges that one face uses (the border of an open surface).
    pub border_edges: usize,
    /// Edges that three or more faces use, or that two faces use the same way.
    pub nonmanifold_edges: usize,
    /// True when every edge has two faces: the faces close a volume.
    pub closed: bool,
    pub area_mm2: f64,
    /// The volume in mm3 for a closed mesh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_mm3: Option<f64>,
    /// Edges that the angle test looked at (the softening angle was given).
    pub considered_edges: usize,
    /// Edges that are soft in the result.
    pub softened_edges: usize,
    /// The angle that was used, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soften_below_angle: Option<f64>,
}

/// The result of a conversion.
#[derive(Clone, Debug)]
pub struct Converted {
    pub model: SurfaceModel,
    /// The point (mm, same frame as the mesh) that the local origin of the model stands for.
    pub origin: DVec3,
    pub report: ConvertReport,
    /// What was left out or done in words.
    pub notes: Vec<String>,
}

/// A hasher for small integer keys: much faster than the default one, and the keys are not
/// chosen by an attacker (they come from the grid of a mesh, and a collision only costs time).
#[derive(Default)]
struct Mix(u64);

impl Hasher for Mix {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn write_i64(&mut self, v: i64) {
        self.0 = (self.0.rotate_left(21) ^ (v as u64)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(21) ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

type Grid = HashMap<[i64; 3], Vec<u32>, BuildHasherDefault<Mix>>;

const NONE: u32 = u32::MAX;

/// Converts the mesh. See the module documentation.
pub fn surface_from_triangles(
    vertices: &[DVec3],
    triangles: &[[u32; 3]],
    opts: &ConvertOptions,
) -> Result<Converted, ConvertError> {
    if let Some(a) = opts.soften_below_angle
        && !(a.is_finite() && (0.0..=180.0).contains(&a))
    {
        return Err(ConvertError::Options(
            "soften_below_angle must be from 0 to 180 degrees".into(),
        ));
    }
    if let Some(t) = opts.tolerance
        && !(t.is_finite() && (0.001..=1000.0).contains(&t))
    {
        return Err(ConvertError::Options(
            "tolerance must be from 0.001 to 1000 mm".into(),
        ));
    }
    if triangles.len() > MAX_CONVERT_TRIANGLES {
        return Err(ConvertError::TooManyTriangles {
            count: triangles.len(),
            limit: MAX_CONVERT_TRIANGLES,
        });
    }
    if vertices
        .iter()
        .any(|v| !v.is_finite() || v.abs().max_element() > MAX_COORD)
    {
        return Err(ConvertError::BadVertex);
    }
    if let Some(bad) = triangles
        .iter()
        .flatten()
        .find(|i| **i as usize >= vertices.len())
    {
        return Err(ConvertError::BadIndex {
            index: *bad,
            count: vertices.len(),
        });
    }
    let mut report = ConvertReport {
        source_vertices: vertices.len(),
        source_triangles: triangles.len(),
        ..Default::default()
    };
    let mut notes = Vec::new();
    // Only the vertices that triangles use count.
    let mut used = vec![false; vertices.len()];
    for t in triangles {
        for i in t {
            used[*i as usize] = true;
        }
    }
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for (v, u) in vertices.iter().zip(&used) {
        if *u {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
    }
    if !lo.is_finite() {
        return Err(ConvertError::Empty);
    }
    // The local origin: the center, in whole mm.
    let origin = ((lo + hi) / 2.0).round();
    let local = |p: DVec3| p - origin;
    let size = (lo - origin).abs().max((hi - origin).abs()).max_element();
    let tol = opts.tolerance.unwrap_or_else(|| tolerance_at(size));
    report.tolerance_mm = tol;

    // 1. Weld.
    let mut rep = vec![NONE; vertices.len()];
    let mut reps: Vec<DVec3> = Vec::new();
    let mut grid: Grid = Grid::default();
    let cell = |p: DVec3| -> [i64; 3] { [0, 1, 2].map(|k| (p[k] / tol).floor() as i64) };
    for (i, v) in vertices.iter().enumerate() {
        if !used[i] {
            continue;
        }
        let p = local(*v);
        let c = cell(p);
        let mut found = NONE;
        'near: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                        for r in list {
                            if reps[*r as usize].distance(p) <= tol {
                                found = *r;
                                break 'near;
                            }
                        }
                    }
                }
            }
        }
        if found == NONE {
            found = reps.len() as u32;
            reps.push(p);
            grid.entry(c).or_default().push(found);
        }
        rep[i] = found;
    }
    drop(grid);
    report.welded_vertices = used.iter().filter(|u| **u).count() - reps.len();

    // 2. Triangles: drop the collapsed and the repeated. A triangle with no area (its corners
    // lie in a line) has no plane, but it can be the glue of a triangulated face, so it is kept as
    // a sliver: it joins the region of a neighbour in whose plane it lies, else it is dropped.
    let ptol = |p: DVec3| plane_tolerance_at(geom::size(p));
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(triangles.len());
    let mut sliver: Vec<bool> = Vec::with_capacity(triangles.len());
    for t in triangles {
        let t = [rep[t[0] as usize], rep[t[1] as usize], rep[t[2] as usize]];
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            report.degenerate_triangles += 1;
            continue;
        }
        let (a, b, c) = (
            reps[t[0] as usize],
            reps[t[1] as usize],
            reps[t[2] as usize],
        );
        let cross = (b - a).cross(c - a).length();
        let longest = (b - a).length().max((c - b).length()).max((a - c).length());
        tris.push(t);
        sliver.push(cross <= longest * ptol(a).max(ptol(b)).max(ptol(c)));
    }
    {
        // Repeats: the same corners in the same cyclic order.
        let canon = |t: &[u32; 3]| -> [u32; 3] {
            let m = (0..3).min_by_key(|k| t[*k]).unwrap_or(0);
            [t[m], t[(m + 1) % 3], t[(m + 2) % 3]]
        };
        let mut order: Vec<([u32; 3], u32)> = tris
            .iter()
            .enumerate()
            .map(|(i, t)| (canon(t), i as u32))
            .collect();
        order.sort_unstable();
        let mut drop_it = vec![false; tris.len()];
        for w in order.windows(2) {
            if w[0].0 == w[1].0 {
                drop_it[w[1].1 as usize] = true;
            }
        }
        report.duplicate_triangles = drop_it.iter().filter(|d| **d).count();
        if report.duplicate_triangles > 0 {
            let mut i = 0;
            tris.retain(|_| {
                i += 1;
                !drop_it[i - 1]
            });
            let mut i = 0;
            sliver.retain(|_| {
                i += 1;
                !drop_it[i - 1]
            });
        }
    }
    if sliver.iter().all(|s| *s) {
        return Err(ConvertError::Empty);
    }
    if report.duplicate_triangles > 0 {
        notes.push(format!(
            "{} triangles repeated another triangle and were dropped",
            report.duplicate_triangles
        ));
    }

    // Planes of the triangles.
    let plane_of = |t: &[u32; 3]| -> Plane {
        let (a, b, c) = (
            reps[t[0] as usize],
            reps[t[1] as usize],
            reps[t[2] as usize],
        );
        Plane::new(a, (b - a).cross(c - a)).unwrap_or(Plane {
            normal: DVec3::Z,
            offset: a.z,
        })
    };

    // 3. Neighbours: edge k of a triangle runs from corner k to corner k + 1.
    let mut table: Vec<(u64, u32)> = Vec::with_capacity(tris.len() * 3);
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let forward = a < b;
            let (lo, hi) = if forward { (a, b) } else { (b, a) };
            table.push((
                ((lo as u64) << 32) | hi as u64,
                ((i as u32) << 3) | ((k as u32) << 1) | forward as u32,
            ));
        }
    }
    // The index fits: at most MAX_CONVERT_TRIANGLES << 3 < u32::MAX.
    table.sort_unstable();
    let mut nbr = vec![[NONE; 3]; tris.len()];
    let mut s = 0;
    while s < table.len() {
        let mut e = s + 1;
        while e < table.len() && table[e].0 == table[s].0 {
            e += 1;
        }
        let group = &table[s..e];
        // Two triangles that use the edge in opposite directions are neighbours. One triangle
        // alone, two that agree about the direction, or three or more are not.
        if let [a, b] = group
            && (a.1 & 1) != (b.1 & 1)
        {
            let (ta, ka) = ((a.1 >> 3) as usize, ((a.1 >> 1) & 3) as usize);
            let (tb, kb) = ((b.1 >> 3) as usize, ((b.1 >> 1) & 3) as usize);
            nbr[ta][ka] = tb as u32;
            nbr[tb][kb] = ta as u32;
        }
        s = e;
    }
    drop(table);

    // 4. Regions of coplanar triangles, from the largest triangle outward.
    let area2 = |t: &[u32; 3]| {
        let (a, b, c) = (
            reps[t[0] as usize],
            reps[t[1] as usize],
            reps[t[2] as usize],
        );
        (b - a).cross(c - a).length()
    };
    let mut order: Vec<u32> = (0..tris.len() as u32)
        .filter(|t| !sliver[*t as usize])
        .collect();
    if opts.merge {
        order.sort_by(|a, b| {
            area2(&tris[*b as usize])
                .total_cmp(&area2(&tris[*a as usize]))
                .then(a.cmp(b))
        });
    }
    let mut region = vec![NONE; tris.len()];
    let mut regions: Vec<(Plane, Vec<u32>)> = Vec::new();
    for &seed in &order {
        if region[seed as usize] != NONE {
            continue;
        }
        let plane = plane_of(&tris[seed as usize]);
        let id = regions.len() as u32;
        let mut members = vec![seed];
        region[seed as usize] = id;
        if opts.merge {
            let mut queue = VecDeque::from([seed]);
            while let Some(t) = queue.pop_front() {
                for n in nbr[t as usize] {
                    if n == NONE || region[n as usize] != NONE {
                        continue;
                    }
                    // A sliver has no plane of its own: its corners must lie in this one.
                    let ok = tris[n as usize]
                        .iter()
                        .all(|v| on_plane(&plane, reps[*v as usize]));
                    if ok {
                        region[n as usize] = id;
                        members.push(n);
                        queue.push_back(n);
                    }
                }
            }
        }
        regions.push((plane, members));
    }

    // The slivers that no region took are dropped.
    for (t, sl) in sliver.iter().enumerate() {
        if *sl && region[t] == NONE {
            report.degenerate_triangles += 1;
        } else if *sl {
            report.joined_slivers += 1;
        }
    }
    if report.degenerate_triangles > 0 {
        notes.push(format!(
            "{} triangles had no area and were dropped",
            report.degenerate_triangles
        ));
    }

    // 5. Faces: the loops of each region, else its triangles.
    let mut faces: Vec<Out> = Vec::new();
    for (rid, (plane, members)) in regions.iter().enumerate() {
        let single = |t: u32, faces: &mut Vec<Out>| {
            let tri = tris[t as usize];
            faces.push(Out {
                plane: plane_of(&tri),
                loops: vec![tri.to_vec()],
            });
        };
        if members.len() == 1 {
            single(members[0], &mut faces);
            continue;
        }
        match loops_of(members, &tris, &nbr, &region, rid as u32, plane, &reps) {
            Some(loops) => {
                report.merged_triangles += members.iter().filter(|t| !sliver[**t as usize]).count();
                faces.push(Out {
                    plane: *plane,
                    loops,
                })
            }
            None => {
                report.unmerged_regions += 1;
                for t in members {
                    if sliver[*t as usize] {
                        report.degenerate_triangles += 1;
                        report.joined_slivers = report.joined_slivers.saturating_sub(1);
                    } else {
                        single(*t, &mut faces);
                    }
                }
            }
        }
    }

    // 6. Heal the vertices that only continue a straight line.
    report.healed_vertices = heal(&mut faces, &reps);

    // 7. The model: ids for vertices, then edges, then faces.
    let mut used_v: BTreeSet<u32> = BTreeSet::new();
    let mut edge_set: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut loop_vertices = 0usize;
    for f in &faces {
        for l in &f.loops {
            loop_vertices += l.len();
            for i in 0..l.len() {
                used_v.insert(l[i]);
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                edge_set.insert(if a < b { (a, b) } else { (b, a) });
            }
        }
    }
    let counts = Counts {
        vertices: used_v.len(),
        edges: edge_set.len(),
        faces: faces.len(),
    };
    let mut over = Vec::new();
    for (what, count, limit) in [
        ("vertices", counts.vertices, MAX_VERTICES),
        ("edges", counts.edges, MAX_EDGES),
        ("faces", counts.faces, MAX_FACES),
        ("loop vertices", loop_vertices, MAX_LOOP_VERTICES),
    ] {
        if count > limit {
            over.push(OverLimit { what, count, limit });
        }
    }
    if !over.is_empty() {
        return Err(ConvertError::OverLimit { over, counts });
    }
    let vid: BTreeMap<u32, VertexId> = used_v
        .iter()
        .enumerate()
        .map(|(i, v)| (*v, VertexId(i as u32 + 1)))
        .collect();
    let first_edge = vid.len() as u32 + 1;
    let eid: BTreeMap<(u32, u32), EdgeId> = edge_set
        .iter()
        .enumerate()
        .map(|(i, e)| (*e, EdgeId(first_edge + i as u32)))
        .collect();
    let first_face = first_edge + eid.len() as u32;
    let mut vmap: BTreeMap<VertexId, DVec3> = BTreeMap::new();
    for (v, id) in &vid {
        vmap.insert(*id, reps[*v as usize]);
    }
    let mut emap: BTreeMap<EdgeId, Edge> = BTreeMap::new();
    for ((a, b), id) in &eid {
        emap.insert(
            *id,
            Edge {
                v: [vid[a], vid[b]],
            },
        );
    }
    let mut fmap: BTreeMap<FaceId, Face> = BTreeMap::new();
    for (i, f) in faces.iter().enumerate() {
        let conv = |l: &Vec<u32>| -> Vec<VertexId> {
            // The loop starts at its smallest vertex id, as saved loops do.
            let ids: Vec<VertexId> = l.iter().map(|v| vid[v]).collect();
            let m = (0..ids.len()).min_by_key(|k| ids[*k]).unwrap_or(0);
            (0..ids.len()).map(|k| ids[(m + k) % ids.len()]).collect()
        };
        fmap.insert(
            FaceId(first_face + i as u32),
            Face::new(
                f.plane,
                conv(&f.loops[0]),
                f.loops[1..].iter().map(conv).collect(),
            ),
        );
    }
    let next_id = first_face + fmap.len() as u32;
    let mut model = SurfaceModel::from_parts(vmap, emap, fmap, next_id);
    model.transform = glam::DAffine3::from_translation(origin);
    if let Some(p) = model.problem() {
        if opts.merge {
            // Try again with every triangle a face: the merge may have hit a shape it cannot hold.
            let mut again = opts.clone();
            again.merge = false;
            let mut r = surface_from_triangles(vertices, triangles, &again)?;
            r.notes.insert(
                0,
                format!("faces could not be merged ({p}): every triangle is a face"),
            );
            return Ok(r);
        }
        return Err(ConvertError::Invalid(p));
    }

    // 8. Report.
    report.vertices = model.vertex_count();
    report.edges = model.edge_count();
    report.faces = model.face_count();
    report.faces_with_holes = model.faces().filter(|(_, f)| !f.holes.is_empty()).count();
    report.area_mm2 = model.area();
    let mut uses: HashMap<(VertexId, VertexId), (u32, i32), BuildHasherDefault<Mix>> =
        HashMap::default();
    for (_, f) in model.faces() {
        for (a, b) in f.uses() {
            let e = uses.entry(key(a, b)).or_insert((0, 0));
            e.0 += 1;
            e.1 += if a < b { 1 } else { -1 };
        }
    }
    report.border_edges = uses.values().filter(|u| u.0 == 1).count();
    report.nonmanifold_edges = uses
        .values()
        .filter(|u| u.0 > 2 || (u.0 == 2 && u.1 != 0))
        .count();
    report.closed = report.border_edges == 0
        && report.nonmanifold_edges == 0
        && model.edge_count() > 0
        && uses.len() == model.edge_count();
    if report.closed {
        report.volume_mm3 = Some(volume_of(&model));
    }
    if let Some(angle) = opts.soften_below_angle {
        let all: Vec<EdgeId> = model.edges().map(|(id, _)| id).collect();
        super::edit::style::soften_edges(&mut model, &all, angle, true, true)
            .map_err(|e| ConvertError::Options(e.to_string()))?;
        let plan = super::edit::style::soften_plan(&model, &all, angle, true);
        report.considered_edges = plan.len();
        report.softened_edges = plan.iter().filter(|(_, soft)| *soft).count();
        report.soften_below_angle = Some(angle);
    }
    Ok(Converted {
        model,
        origin,
        report,
        notes,
    })
}

/// Counts the edges that the angle would soften in a model, without changing it. This is the
/// preview of the import dialog. Edges between coplanar faces count, as in the conversion.
pub fn softened_count(model: &SurfaceModel, angle: f64) -> usize {
    let all: Vec<EdgeId> = model.edges().map(|(id, _)| id).collect();
    super::edit::style::soften_plan(model, &all, angle, true)
        .iter()
        .filter(|(_, soft)| *soft)
        .count()
}

/// The signed volume of a closed model (the absolute value), from the triangles of its faces.
fn volume_of(m: &SurfaceModel) -> f64 {
    let Ok(mesh) = m.mesh() else { return 0.0 };
    let v: f64 = mesh
        .triangles
        .iter()
        .map(|t| {
            let p = |i: u32| mesh.positions[i as usize];
            p(t[0]).dot(p(t[1]).cross(p(t[2])))
        })
        .sum();
    (v / 6.0).abs()
}

/// A face before it has ids: the plane and the loops (the outer loop first).
struct Out {
    plane: Plane,
    loops: Vec<Vec<u32>>,
}

/// The loops of a region of triangles: one outer loop counterclockwise around the plane normal
/// and any number of holes. `None` when the boundary touches itself at a vertex, does not close,
/// or does not have exactly one outer loop.
fn loops_of(
    members: &[u32],
    tris: &[[u32; 3]],
    nbr: &[[u32; 3]],
    region: &[u32],
    rid: u32,
    plane: &Plane,
    reps: &[DVec3],
) -> Option<Vec<Vec<u32>>> {
    // The boundary uses: edges whose neighbour is not in the region.
    let mut uses: Vec<(u32, u32)> = Vec::new();
    for &t in members {
        let tri = tris[t as usize];
        for k in 0..3 {
            let n = nbr[t as usize][k];
            if n == NONE || region[n as usize] != rid {
                uses.push((tri[k], tri[(k + 1) % 3]));
            }
        }
    }
    uses.sort_unstable();
    // A vertex with two ways out is a pinch.
    if uses.windows(2).any(|w| w[0].0 == w[1].0) {
        return None;
    }
    let mut seen = vec![false; uses.len()];
    let mut loops: Vec<Vec<u32>> = Vec::new();
    for start in 0..uses.len() {
        if seen[start] {
            continue;
        }
        let mut l = Vec::new();
        let mut at = start;
        loop {
            if seen[at] {
                return None;
            }
            seen[at] = true;
            l.push(uses[at].0);
            let next = uses[at].1;
            if next == uses[start].0 {
                break;
            }
            at = uses.binary_search_by_key(&next, |u| u.0).ok()?;
            if l.len() > uses.len() {
                return None;
            }
        }
        if l.len() < 3 {
            return None;
        }
        loops.push(l);
    }
    let signed = |l: &Vec<u32>| -> f64 {
        let pts: Vec<DVec3> = l.iter().map(|v| reps[*v as usize]).collect();
        geom::newell(&pts).dot(plane.normal)
    };
    let areas: Vec<f64> = loops.iter().map(signed).collect();
    let outer: Vec<usize> = (0..loops.len()).filter(|i| areas[*i] > 0.0).collect();
    if outer.len() != 1 || areas.contains(&0.0) {
        return None;
    }
    let mut out = vec![loops[outer[0]].clone()];
    for (i, l) in loops.iter().enumerate() {
        if i != outer[0] {
            out.push(l.clone());
        }
    }
    Some(out)
}

/// Takes away the vertices that have two edges only, in a straight line, and returns how many.
fn heal(faces: &mut [Out], reps: &[DVec3]) -> usize {
    let mut adj: HashMap<u32, Vec<u32>, BuildHasherDefault<Mix>> = HashMap::default();
    let mut at: HashMap<u32, Vec<(usize, usize)>, BuildHasherDefault<Mix>> = HashMap::default();
    for (fi, f) in faces.iter().enumerate() {
        for (li, l) in f.loops.iter().enumerate() {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                for (x, y) in [(a, b), (b, a)] {
                    let v = adj.entry(x).or_default();
                    if !v.contains(&y) {
                        v.push(y);
                    }
                }
                let w = at.entry(a).or_default();
                if w.last() != Some(&(fi, li)) {
                    w.push((fi, li));
                }
            }
        }
    }
    let mut work: Vec<u32> = adj
        .iter()
        .filter(|(_, n)| n.len() == 2)
        .map(|(v, _)| *v)
        .collect();
    work.sort_unstable();
    work.reverse();
    let mut healed = 0usize;
    while let Some(v) = work.pop() {
        let Some(n) = adj.get(&v).filter(|n| n.len() == 2).cloned() else {
            continue;
        };
        let (a, b) = (n[0], n[1]);
        if adj.get(&a).is_some_and(|x| x.contains(&b)) {
            continue;
        }
        let (pv, pa, pb) = (reps[v as usize], reps[a as usize], reps[b as usize]);
        let ab = pb - pa;
        let t = (pv - pa).dot(ab);
        if !(t > 0.0 && t < ab.length_squared())
            || geom::point_line(pv, pa, pb) > plane_tolerance_at(geom::size(pv))
        {
            continue;
        }
        let places = at.get(&v).cloned().unwrap_or_default();
        if places.iter().any(|(f, l)| faces[*f].loops[*l].len() <= 3) {
            continue;
        }
        for (f, l) in &places {
            faces[*f].loops[*l].retain(|x| *x != v);
        }
        at.remove(&v);
        adj.remove(&v);
        for (x, y) in [(a, b), (b, a)] {
            if let Some(list) = adj.get_mut(&x) {
                list.retain(|e| *e != v);
                list.push(y);
            }
        }
        // The loops that held v now hold a and b next to each other: the places of a and b are the same.
        for (x, from) in [(a, &places), (b, &places)] {
            let w = at.entry(x).or_default();
            for p in from {
                if !w.contains(p) {
                    w.push(*p);
                }
            }
        }
        work.push(a);
        work.push(b);
        healed += 1;
    }
    healed
}
