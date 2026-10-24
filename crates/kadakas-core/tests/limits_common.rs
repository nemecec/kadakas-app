//! Helpers shared by the limit measurements (`limits_measure.rs` in kadakas-core, and the tests
//! of the same name in kadakas-ui-core and kadakas-props, which include this file with `#[path]`).
//! Measurement code only: nothing here is used by the product.
#![allow(dead_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// A global allocator that counts the bytes in use and the peak. Install it in the test binary
/// with `#[global_allocator] static A: limits_common::Counting = limits_common::Counting;`.
pub struct Counting;

static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let now = CUR.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        CUR.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            CUR.fetch_sub(l.size(), Ordering::Relaxed);
            grow(new);
        }
        q
    }
}

/// Bytes on the heap now.
pub fn heap() -> usize {
    CUR.load(Ordering::Relaxed)
}
/// Starts a new peak: the peak is the heap now.
pub fn reset_peak() {
    PEAK.store(CUR.load(Ordering::Relaxed), Ordering::Relaxed);
}
/// The highest heap since `reset_peak`.
pub fn peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}

#[cfg(unix)]
unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut [i64; 18]) -> i32;
}

/// User plus system CPU seconds of this process so far. Where there is no `getrusage` (Windows),
/// the wall time since the first call stands in for it.
pub fn cpu() -> f64 {
    #[cfg(unix)]
    {
        let mut b = [0i64; 18];
        unsafe { getrusage(0, &mut b) };
        let t = |s: i64, us: i64| s as f64 + (us as i32) as f64 / 1e6;
        t(b[0], b[1]) + t(b[2], b[3])
    }
    #[cfg(not(unix))]
    {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64()
    }
}

/// The resident set size now, in bytes (from `ps`).
pub fn rss() -> usize {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<usize>()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    out * 1024
}

/// The highest resident set size of this process so far, in bytes.
pub fn max_rss() -> usize {
    #[cfg(unix)]
    {
        let mut b = [0i64; 18];
        unsafe { getrusage(0, &mut b) };
        b[4] as usize
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// Wall time, CPU time and the growth of the heap peak of a piece of work.
pub struct Meas {
    t: Instant,
    c: f64,
    h0: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Took {
    pub wall: f64,
    pub cpu: f64,
    /// Peak heap during the work, above the heap at its start.
    pub peak: usize,
    /// Heap after the work minus heap before.
    pub kept: i64,
}

impl Meas {
    pub fn start() -> Meas {
        reset_peak();
        Meas {
            t: Instant::now(),
            c: cpu(),
            h0: heap(),
        }
    }
    pub fn stop(self) -> Took {
        Took {
            wall: self.t.elapsed().as_secs_f64(),
            cpu: cpu() - self.c,
            peak: peak().saturating_sub(self.h0),
            kept: heap() as i64 - self.h0 as i64,
        }
    }
}

/// Times a closure.
pub fn timed<T>(f: impl FnOnce() -> T) -> (T, Took) {
    let m = Meas::start();
    let r = f();
    (r, m.stop())
}

pub fn fmt_took(t: &Took) -> String {
    format!(
        "wall_s={:.4} cpu_s={:.4} peak_heap_mb={:.1} kept_heap_mb={:.1}",
        t.wall,
        t.cpu,
        t.peak as f64 / 1048576.0,
        t.kept as f64 / 1048576.0
    )
}

/// Prints one result line that the driver script collects.
pub fn result(name: &str, fields: &str) {
    println!("RESULT {name} {fields}");
}

pub fn mb(n: usize) -> String {
    format!("{:.2}", n as f64 / 1048576.0)
}

// ---------------------------------------------------------------------------
// Shapes. A `Mesh` is a list of points and of faces (loops of point indices, counterclockwise
// seen from the front). `segs_of` gives its edges for `Edit::AddEdges` (the model is drawn), and
// `construct` builds the model straight from its parts (`SurfaceModel::from_parts`), which is
// instant: it shows what the limits allow without the time that drawing takes.

use kadakas_core::surface::{Edge, EdgeId, Face, FaceId, Plane, SurfaceModel, VertexId};
use std::collections::{BTreeMap, BTreeSet};

pub type Seg = [[f64; 3]; 2];

pub struct Mesh {
    pub v: Vec<[f64; 3]>,
    pub f: Vec<Vec<usize>>,
}

impl Mesh {
    /// The edges, each once, in the order of the faces.
    pub fn segs(&self) -> Vec<Seg> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for f in &self.f {
            for i in 0..f.len() {
                let (a, b) = (f[i], f[(i + 1) % f.len()]);
                if seen.insert((a.min(b), a.max(b))) {
                    out.push([self.v[a], self.v[b]]);
                }
            }
        }
        out
    }
}

pub fn chunks(segs: Vec<Seg>, n: usize) -> Vec<Vec<Seg>> {
    segs.chunks(n).map(<[Seg]>::to_vec).collect()
}

fn hash01(a: u64, b: u64) -> f64 {
    let mut h = a.wrapping_mul(0x9E3779B97F4A7C15) ^ b.wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 32;
    (h % 1_000_000) as f64 / 1_000_000.0
}

fn newell(v: &[[f64; 3]], f: &[usize]) -> glam::DVec3 {
    let mut n = glam::DVec3::ZERO;
    for i in 0..f.len() {
        let a = glam::DVec3::from(v[f[i]]);
        let b = glam::DVec3::from(v[f[(i + 1) % f.len()]]);
        n += glam::DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    n
}

/// Turns the loop around if its normal points to the center `c`.
fn outward(v: &[[f64; 3]], f: &mut [usize], c: glam::DVec3) {
    let mid = f
        .iter()
        .map(|&i| glam::DVec3::from(v[i]))
        .sum::<glam::DVec3>()
        / f.len() as f64;
    if newell(v, f).dot(mid - c) < 0.0 {
        f.reverse();
    }
}

/// n x n squares of 1 m in one plane.
pub fn grid_mesh(n: usize, ox: f64, oy: f64, z: f64) -> Mesh {
    let idx = |i: usize, j: usize| j * (n + 1) + i;
    let mut v = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            v.push([ox + i as f64 * 1000.0, oy + j as f64 * 1000.0, z]);
        }
    }
    let mut f = Vec::new();
    for j in 0..n {
        for i in 0..n {
            f.push(vec![
                idx(i, j),
                idx(i + 1, j),
                idx(i + 1, j + 1),
                idx(i, j + 1),
            ]);
        }
    }
    Mesh { v, f }
}

/// A terrain patch of n x n cells, each cut by a diagonal into two triangles. The heights are a
/// smooth wave plus up to `noise` mm of random height at every vertex.
pub fn terrain_mesh(n: usize, ox: f64, oy: f64, noise: f64) -> Mesh {
    let zf = |i: usize, j: usize| {
        800.0 * (0.11 * i as f64).sin() * (0.17 * j as f64).cos()
            + noise * (hash01(i as u64 + 1, j as u64 + 7) - 0.5)
    };
    let idx = |i: usize, j: usize| j * (n + 1) + i;
    let mut v = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            v.push([ox + i as f64 * 1000.0, oy + j as f64 * 1000.0, zf(i, j)]);
        }
    }
    let mut f = Vec::new();
    for j in 0..n {
        for i in 0..n {
            f.push(vec![idx(i, j), idx(i + 1, j), idx(i + 1, j + 1)]);
            f.push(vec![idx(i, j), idx(i + 1, j + 1), idx(i, j + 1)]);
        }
    }
    Mesh { v, f }
}

/// A triangulated UV sphere of radius 10 m with r rings and 2r segments. Every vertex moves
/// radially by up to `noise` times the radius (a rough scan has 0.001; a spiky mesh 0.3, and
/// then every 37th vertex goes out to 2.5 times the radius).
pub fn sphere_mesh(r: usize, noise: f64) -> Mesh {
    let s = 2 * r;
    let rad = 10_000.0;
    let mut v = vec![[0.0, 0.0, rad]];
    for k in 1..r {
        for i in 0..s {
            let th = std::f64::consts::PI * k as f64 / r as f64;
            let ph = 2.0 * std::f64::consts::PI * i as f64 / s as f64;
            let mut q = 1.0 + noise * (hash01(k as u64, i as u64) - 0.5) * 2.0;
            if noise >= 0.1 && (k * s + i).is_multiple_of(37) {
                q = 2.5;
            }
            v.push([
                rad * q * th.sin() * ph.cos(),
                rad * q * th.sin() * ph.sin(),
                rad * q * th.cos(),
            ]);
        }
    }
    v.push([0.0, 0.0, -rad]);
    let south = v.len() - 1;
    let ring = |k: usize, i: usize| 1 + (k - 1) * s + i % s;
    let mut f = Vec::new();
    for i in 0..s {
        f.push(vec![0, ring(1, i), ring(1, i + 1)]);
    }
    for k in 1..r - 1 {
        for i in 0..s {
            f.push(vec![ring(k, i), ring(k + 1, i), ring(k + 1, i + 1)]);
            f.push(vec![ring(k, i), ring(k + 1, i + 1), ring(k, i + 1)]);
        }
    }
    for i in 0..s {
        f.push(vec![ring(r - 1, i), south, ring(r - 1, i + 1)]);
    }
    for face in &mut f {
        outward(&v, face, glam::DVec3::ZERO);
    }
    Mesh { v, f }
}

/// `count` separate cubes of 1 m (8 vertices, 12 edges, 6 faces each), in a grid.
pub fn boxes_mesh(count: usize) -> Mesh {
    let mut v = Vec::new();
    let mut f = Vec::new();
    for k in 0..count {
        let (x, y) = ((k % 100) as f64 * 2000.0, (k / 100) as f64 * 2000.0);
        let base = v.len();
        for i in 0..8usize {
            v.push([
                x + if i & 1 == 1 { 1000.0 } else { 0.0 },
                y + if i & 2 == 2 { 1000.0 } else { 0.0 },
                if i & 4 == 4 { 1000.0 } else { 0.0 },
            ]);
        }
        let c = glam::DVec3::new(x + 500.0, y + 500.0, 500.0);
        for q in [
            [0, 1, 3, 2],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [0, 2, 6, 4],
            [1, 3, 7, 5],
        ] {
            let mut face: Vec<usize> = q.iter().map(|i| base + i).collect();
            outward(&v, &mut face, c);
            f.push(face);
        }
    }
    Mesh { v, f }
}

pub fn mesh_of(shape: &str, p: usize) -> Mesh {
    match shape {
        "grid" => grid_mesh(p, 0.0, 0.0, 0.0),
        "terrain" => terrain_mesh(p, 0.0, 0.0, 0.0),
        "noisy" => terrain_mesh(p, 0.0, 0.0, 3000.0),
        "sphere" => sphere_mesh(p, 0.001),
        "spiky" => sphere_mesh(p, 0.3),
        "boxes" => boxes_mesh(p),
        other => panic!("unknown shape {other}"),
    }
}

/// The model of a mesh, built from its parts. Nothing is drawn, nothing is checked.
pub fn construct(mesh: &Mesh) -> SurfaceModel {
    let mut vertices = BTreeMap::new();
    for (i, p) in mesh.v.iter().enumerate() {
        vertices.insert(VertexId(i as u32 + 1), glam::DVec3::from(*p));
    }
    let mut next = mesh.v.len() as u32 + 1;
    let mut edge_ids = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for f in &mesh.f {
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            let key = (a.min(b), a.max(b));
            edge_ids.entry(key).or_insert_with(|| {
                let id = EdgeId(next);
                next += 1;
                edges.insert(
                    id,
                    Edge {
                        v: [VertexId(a as u32 + 1), VertexId(b as u32 + 1)],
                    },
                );
                id
            });
        }
    }
    let mut faces = BTreeMap::new();
    for f in &mesh.f {
        let n = newell(&mesh.v, f);
        let plane = Plane::new(glam::DVec3::from(mesh.v[f[0]]), n).expect("a face with an area");
        let outer: Vec<VertexId> = f.iter().map(|i| VertexId(*i as u32 + 1)).collect();
        faces.insert(FaceId(next), Face::new(plane, outer, vec![]));
        next += 1;
    }
    SurfaceModel::from_parts(vertices, edges, faces, next)
}

/// Edges of the grid as drawn (see `grid_mesh`), for patches.
pub fn grid_segs(n: usize, ox: f64, oy: f64, z: f64) -> Vec<Seg> {
    grid_mesh(n, ox, oy, z).segs()
}
pub fn terrain_segs(n: usize, ox: f64, oy: f64, noise: f64) -> Vec<Seg> {
    terrain_mesh(n, ox, oy, noise).segs()
}

/// The 12 edges of a cube of side s at (x, y, 0).
pub fn cube_segs(x: f64, y: f64, s: f64) -> Vec<Seg> {
    let c = |i: usize| {
        [
            x + if i & 1 == 1 { s } else { 0.0 },
            y + if i & 2 == 2 { s } else { 0.0 },
            if i & 4 == 4 { s } else { 0.0 },
        ]
    };
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push([c(i), c(i | bit)]);
            }
        }
    }
    edges
}

/// The 4 edges of a square of side s at (x, y, 0).
pub fn square_segs(x: f64, y: f64, s: f64) -> Vec<Seg> {
    let p = [
        [x, y, 0.0],
        [x + s, y, 0.0],
        [x + s, y + s, 0.0],
        [x, y + s, 0.0],
    ];
    (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect()
}

// ---------------------------------------------------------------------------
// Worlds with definitions and instances.

use glam::{DAffine3, DVec3};
use kadakas_core::command::Editor;
use kadakas_core::component::{self, Part};
use kadakas_core::graphics::{GraphicResource, SymbolDef};
use kadakas_core::model::*;
use kadakas_core::resource::{Resource, ResourceBody, ResourceId};
use kadakas_core::surface::Edit;

pub fn model_of(kind: &str) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    let edges = match kind {
        "cube" => cube_segs(0.0, 0.0, 1000.0),
        "sq" => square_segs(0.0, 0.0, 1000.0),
        other => panic!("unknown part kind {other}"),
    };
    m.apply(&Edit::AddEdges { edges }).unwrap();
    m
}

pub fn part_at(m: &SurfaceModel, k: usize) -> Part {
    let mut m = m.clone();
    m.transform = DAffine3::from_translation(DVec3::new(
        (k % 100) as f64 * 2000.0,
        (k / 100) as f64 * 2000.0,
        0.0,
    ));
    Part::new(Shape::SurfaceModel(Box::new(m)))
}

pub fn instance_shape(symbol: ResourceId, k: usize) -> Shape {
    Shape::Component {
        symbol,
        transform: component::raw(&DAffine3::from_translation(DVec3::new(
            (k % 300) as f64 * 40_000.0,
            (k / 300) as f64 * 40_000.0,
            0.0,
        ))),
    }
}

pub fn push_def(doc: &mut Document, name: &str, parts: Vec<Part>) -> ResourceId {
    let id = ResourceId(doc.alloc_id());
    doc.resources.push(Resource {
        id,
        name: name.into(),
        body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
            parts,
            ..SymbolDef::default()
        }))),
    });
    id
}

pub fn push_instances(doc: &mut Document, symbol: ResourceId, count: usize) -> Vec<ObjectId> {
    let mut ids = Vec::with_capacity(count);
    for k in 0..count {
        let id = ObjectId(doc.alloc_id());
        doc.objects.insert(
            id,
            Object::new(id, FIRST_LAYER, NONE_CLASS, instance_shape(symbol, k)),
        );
        ids.push(id);
    }
    ids
}

pub fn def_parts(kind: &str, parts: usize) -> Vec<Part> {
    // "empty": parts that are groups with no model (they add no leaf to the expansion).
    if kind == "empty" {
        return (0..parts)
            .map(|_| Part::new(Shape::Group { name: "g".into() }))
            .collect();
    }
    let m = model_of(kind);
    (0..parts).map(|k| part_at(&m, k)).collect()
}

pub fn world(kind: &str, parts: usize, instances: usize) -> (Editor, ResourceId, Vec<ObjectId>) {
    let mut doc = Document::with_sequential_ids();
    let sym = push_def(&mut doc, "Def", def_parts(kind, parts));
    let ids = push_instances(&mut doc, sym, instances);
    (Editor::new(doc), sym, ids)
}

pub fn save_text(doc: &Document) -> String {
    let mut v = Vec::new();
    doc.write_compact(&mut v).unwrap();
    String::from_utf8(v).unwrap()
}
