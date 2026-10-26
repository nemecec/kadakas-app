//! Geometry of IFC representation items: profiles, curves, extruded solids, face sets, breps,
//! mapped items and simple clipping. Everything is in the units of the file. The importer scales
//! to millimeters when it emits objects.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::rc::Rc;

use glam::{DAffine2, DAffine3, DVec2, DVec3};
use kadakas_core::curve::{self, Vertex, VertexKind};

pub(crate) mod solids;

use super::csg;
use super::ctx::{Ctx, attr, enum_of, flag, int, items, num, numbers};
use crate::step::Value;

/// One cached advanced brep: its mesh (if it has one) and the sentences of the report.
pub type BrepRead = Rc<(Option<MeshData>, Vec<String>)>;

/// The most bytes of meshes that the cache of advanced breps keeps (the least recently used
/// ones are dropped; a dropped solid that is used again is read again).
pub const BREP_CACHE_BYTES: usize = 256 * 1024 * 1024;

/// The meshes of advanced breps that were read, by item, with a limit of bytes.
#[derive(Default)]
pub struct BrepCache {
    inner: std::cell::RefCell<BrepCacheInner>,
    /// Test hook: another limit than `BREP_CACHE_BYTES` (0 means the default).
    limit: std::cell::Cell<usize>,
}

#[derive(Default)]
struct BrepCacheInner {
    /// By item: the read, its size in bytes and the tick of its last use.
    map: HashMap<u64, (BrepRead, usize, u64)>,
    /// The items by the tick of their last use: the first one is the least recently used.
    order: std::collections::BTreeMap<u64, u64>,
    bytes: usize,
    tick: u64,
}

impl BrepCacheInner {
    fn remove(&mut self, id: u64) {
        if let Some((_, size, tick)) = self.map.remove(&id) {
            self.bytes -= size;
            self.order.remove(&tick);
        }
    }
}

/// The bytes that a read takes: the triangles, the exact surfaces (a structural estimate from
/// the counts, not a serialization) and the sentences.
fn brep_read_bytes(read: &BrepRead) -> usize {
    let mesh = read.0.as_ref().map_or(0, |m| {
        m.v.len() * 24 + m.t.len() * 12 + m.exact.as_ref().map_or(0, |x| x.approx_bytes())
    });
    mesh + read.1.iter().map(|s| s.len() + 24).sum::<usize>() + 64
}

impl BrepCache {
    /// A cache that keeps `limit` bytes.
    pub fn with_limit(limit: usize) -> Self {
        let c = Self::default();
        c.limit.set(limit);
        c
    }

    /// The read of this item, now the most recently used.
    pub fn get(&self, id: u64) -> Option<BrepRead> {
        let mut i = self.inner.borrow_mut();
        i.tick += 1;
        let tick = i.tick;
        let (read, old) = {
            let (r, _, t) = i.map.get_mut(&id)?;
            let old = std::mem::replace(t, tick);
            (r.clone(), old)
        };
        i.order.remove(&old);
        i.order.insert(tick, id);
        Some(read)
    }

    /// Keeps a read. The least recently used reads go while the total is over the limit; one
    /// read that is over the limit by itself is not kept.
    pub fn insert(&self, id: u64, read: BrepRead) {
        let size = brep_read_bytes(&read);
        let limit = match self.limit.get() {
            0 => BREP_CACHE_BYTES,
            n => n,
        };
        if size > limit {
            return;
        }
        let mut i = self.inner.borrow_mut();
        i.remove(id);
        while i.bytes + size > limit {
            let Some((&tick, &oldest)) = i.order.iter().next() else {
                break;
            };
            i.order.remove(&tick);
            if let Some((_, old, _)) = i.map.remove(&oldest) {
                i.bytes -= old;
            }
        }
        i.tick += 1;
        let tick = i.tick;
        i.order.insert(tick, id);
        i.map.insert(id, (read, size, tick));
        i.bytes += size;
    }

    /// The bytes and the number of reads that the cache holds.
    pub fn held(&self) -> (usize, usize) {
        let i = self.inner.borrow();
        (i.bytes, i.map.len())
    }
}

/// An indexed triangle mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub v: Vec<DVec3>,
    pub t: Vec<[u32; 3]>,
    /// The exact surfaces of the solid (an `IfcAdvancedBrep`), in the units and the frame of the
    /// vertices. It follows a move, a turn and a uniform scale; any other change of the mesh (a cut,
    /// a join with another body) drops it.
    pub exact: Option<Box<kadakas_core::exact::ExactSolid>>,
}

impl MeshData {
    pub fn append(&mut self, o: &MeshData) {
        self.exact = match (self.v.is_empty(), self.exact.take(), &o.exact) {
            (true, _, e) => e.clone(),
            (false, Some(mut a), Some(b)) => {
                a.merge(b);
                Some(a)
            }
            _ => None,
        };
        let base = self.v.len() as u32;
        self.v.extend_from_slice(&o.v);
        self.t
            .extend(o.t.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }

    pub fn transform(&mut self, xf: &DAffine3) {
        for p in &mut self.v {
            *p = xf.transform_point3(*p);
        }
        if let Some(e) = self.exact.take() {
            self.exact = e.transformed(xf).ok().map(Box::new);
        }
        if xf.matrix3.determinant() < 0.0 {
            self.flip();
        }
    }

    pub fn flip(&mut self) {
        for t in &mut self.t {
            t.swap(1, 2);
        }
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        if self.v.is_empty() {
            return None;
        }
        Some(self.v.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        ))
    }

    /// Drops vertices that no triangle uses (for example points that a `PnIndex` leaves out).
    pub fn compact(&mut self) {
        let mut remap = vec![u32::MAX; self.v.len()];
        let mut out = Vec::new();
        for t in &mut self.t {
            for i in t.iter_mut() {
                let r = &mut remap[*i as usize];
                if *r == u32::MAX {
                    out.push(self.v[*i as usize]);
                    *r = (out.len() - 1) as u32;
                }
                *i = *r;
            }
        }
        self.v = out;
    }

    /// Merges vertices that are closer than `tol` (on a grid) and drops triangles that collapse.
    pub fn weld(&mut self, tol: f64) {
        let q = |p: &DVec3| {
            (
                (p.x / tol).round() as i64,
                (p.y / tol).round() as i64,
                (p.z / tol).round() as i64,
            )
        };
        let mut map: HashMap<(i64, i64, i64), u32> = HashMap::with_capacity(self.v.len());
        let mut remap = Vec::with_capacity(self.v.len());
        let mut out: Vec<DVec3> = Vec::new();
        for p in &self.v {
            let id = *map.entry(q(p)).or_insert_with(|| {
                out.push(*p);
                (out.len() - 1) as u32
            });
            remap.push(id);
        }
        self.t = self
            .t
            .iter()
            .map(|t| t.map(|i| remap[i as usize]))
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect();
        self.v = out;
    }

    /// An axis-aligned box with 12 triangles facing outward.
    pub fn cuboid(lo: DVec3, hi: DVec3) -> MeshData {
        let c = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        let v = vec![
            c(false, false, false),
            c(true, false, false),
            c(true, true, false),
            c(false, true, false),
            c(false, false, true),
            c(true, false, true),
            c(true, true, true),
            c(false, true, true),
        ];
        let t = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        MeshData { v, t, exact: None }
    }
}

/// One closed area: an outer outline and holes. Outlines are curved vertices (see `curve`).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Vec<Vertex>,
    pub holes: Vec<Vec<Vertex>>,
}

/// A profile pushed along a direction. `dir` is in the local frame and has the length of the depth.
#[derive(Clone, Debug, PartialEq)]
pub struct Extrusion {
    pub regions: Vec<Region>,
    pub dir: DVec3,
    pub xf: DAffine3,
    /// Largest distance between a curve and its flat segments, in file units.
    pub tol: f64,
}

impl Extrusion {
    /// True if the profile plane is horizontal and the extrusion goes straight up or down.
    pub fn is_upright(&self) -> bool {
        let m = self.xf.matrix3;
        let up = m.z_axis.normalize_or_zero();
        let flat = |v: DVec3| v.x.abs() < 1e-9 && v.y.abs() < 1e-9;
        flat(up)
            && up.z.abs() > 0.0
            && self.dir.x.abs() < 1e-9 * self.dir.length().max(1.0)
            && self.dir.y.abs() < 1e-9 * self.dir.length().max(1.0)
    }

    /// Height in world units (absolute, along Z) for an upright extrusion.
    pub fn height(&self) -> f64 {
        (self.xf.matrix3 * self.dir).z.abs()
    }

    /// The closed triangle mesh of the solid, in world coordinates. The caps and the walls use the
    /// same points of the outline and the holes, so the mesh is closed with no point between.
    pub fn mesh(&self) -> MeshData {
        let mut out = MeshData::default();
        for r in &self.regions {
            let ring = |vs: &[Vertex]| curve::flatten(vs, true, self.tol.max(1e-9));
            let outer = clean(&ring(&r.outer), true);
            if outer.len() < 3 {
                continue;
            }
            // The holes run clockwise.
            let holes: Vec<Vec<DVec2>> = r
                .holes
                .iter()
                .map(|h| clean(&ring(h), true))
                .filter(|h| h.len() >= 3)
                .map(|h| h.into_iter().rev().collect())
                .collect();
            let mut rings: Vec<Vec<DVec2>> = vec![outer];
            rings.extend(holes);
            let caps = if rings.len() == 1 {
                Some(
                    kadakas_core::mesh::triangulate_edge_exact(&rings[0])
                        .into_iter()
                        .map(|[a, b, c]| [a as u32, b as u32, c as u32])
                        .collect::<Vec<_>>(),
                )
            } else {
                kadakas_core::solid3d::cap_triangles(&rings)
            };
            let Some(caps) = caps else {
                out.append(&self.mesh_split(&rings));
                continue;
            };
            let total: usize = rings.iter().map(Vec::len).sum();
            let mut m = MeshData::default();
            m.v.extend(rings.iter().flatten().map(|p| p.extend(0.0)));
            m.v.extend(rings.iter().flatten().map(|p| p.extend(0.0) + self.dir));
            let top = total as u32;
            for [a, b, c] in caps {
                // Bottom faces down (clockwise from above), top faces up.
                m.t.push([a, c, b]);
                m.t.push([top + a, top + b, top + c]);
            }
            // Walls. The outline runs counterclockwise and the holes clockwise.
            let mut at = 0u32;
            for ring in &rings {
                let n = ring.len() as u32;
                for i in 0..n {
                    let j = (i + 1) % n;
                    m.t.push([at + i, at + j, top + at + j]);
                    m.t.push([at + i, top + at + j, top + at + i]);
                }
                at += n;
            }
            // An extrusion against the profile normal turns the whole solid inside out.
            if self.dir.z < 0.0 {
                m.flip();
            }
            out.append(&m);
        }
        out.transform(&self.xf);
        out
    }

    /// The mesh of a region whose cap has no bridges (a rare outline): the area is cut into pieces
    /// without holes. The pieces have points of their own, so the mesh may have cracks.
    fn mesh_split(&self, rings: &[Vec<DVec2>]) -> MeshData {
        let holes: Vec<Vec<DVec2>> = rings[1..]
            .iter()
            .map(|h| h.iter().rev().copied().collect())
            .collect();
        let mut m = MeshData::default();
        for piece in &split_counted(&rings[0], &holes) {
            let tris = kadakas_core::mesh::triangulate(piece);
            let base = m.v.len() as u32;
            m.v.extend(piece.iter().map(|p| p.extend(0.0)));
            let top = m.v.len() as u32;
            m.v.extend(piece.iter().map(|p| p.extend(0.0) + self.dir));
            for [a, b, c] in tris {
                m.t.push([base + a as u32, base + c as u32, base + b as u32]);
                m.t.push([top + a as u32, top + b as u32, top + c as u32]);
            }
        }
        for ring in rings {
            let base = m.v.len() as u32;
            let n = ring.len() as u32;
            m.v.extend(ring.iter().map(|p| p.extend(0.0)));
            m.v.extend(ring.iter().map(|p| p.extend(0.0) + self.dir));
            for i in 0..n {
                let j = (i + 1) % n;
                m.t.push([base + i, base + j, base + n + j]);
                m.t.push([base + i, base + n + j, base + n + i]);
            }
        }
        if self.dir.z < 0.0 {
            m.flip();
        }
        m
    }
}

/// Removes repeated points and the closing point, and sets the winding. `None` winding is kept.
fn clean(points: &[DVec2], ccw: bool) -> Vec<DVec2> {
    let mut ring: Vec<DVec2> = Vec::with_capacity(points.len());
    for p in points {
        if !p.is_finite() {
            return Vec::new();
        }
        if ring.last().is_none_or(|l| l.distance_squared(*p) > 1e-18) {
            ring.push(*p);
        }
    }
    while ring.len() > 1 && ring[0].distance_squared(ring[ring.len() - 1]) <= 1e-18 {
        ring.pop();
    }
    if ring.len() < 3 {
        return Vec::new();
    }
    let area = crate::geom::signed_area(&ring);
    if area == 0.0 {
        return Vec::new();
    }
    if (area < 0.0) == ccw {
        ring.reverse();
    }
    ring
}

/// A curve in the world, for annotations. `exact` holds curved vertices when the curve is flat and
/// horizontal.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveData {
    pub exact: Option<(Vec<Vertex>, f64)>,
    pub pts: Vec<DVec3>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Mesh(MeshData),
    Extrusion(Extrusion),
    /// A mapped item that becomes a symbol instance. `xf` takes the source frame to the world.
    Instance {
        source: u64,
        xf: DAffine3,
    },
    Curve(CurveData),
    Point(DVec3),
}

impl Piece {
    /// A rough number of triangles, for the budget.
    pub fn cost(&self) -> usize {
        match self {
            Piece::Mesh(m) => m.t.len(),
            Piece::Extrusion(e) => e.regions.iter().map(|r| r.outer.len() * 4 + 2).sum(),
            _ => 1,
        }
    }

    /// The piece moved by `t`.
    pub fn transformed(&self, t: &DAffine3) -> Piece {
        match self {
            Piece::Mesh(m) => {
                let mut m = m.clone();
                m.transform(t);
                Piece::Mesh(m)
            }
            Piece::Extrusion(e) => Piece::Extrusion(Extrusion {
                xf: *t * e.xf,
                ..e.clone()
            }),
            Piece::Instance { source, xf } => Piece::Instance {
                source: *source,
                xf: *t * *xf,
            },
            Piece::Point(p) => Piece::Point(t.transform_point3(*p)),
            Piece::Curve(c) => {
                let m = t.matrix3;
                let similar = m.z_axis.normalize_or_zero().distance(DVec3::Z) < 1e-9
                    && m.x_axis.z.abs() < 1e-9
                    && m.y_axis.z.abs() < 1e-9
                    && (m.x_axis.length() - m.y_axis.length()).abs() < 1e-9
                    && m.x_axis.dot(m.y_axis).abs() < 1e-9;
                let exact = c.exact.as_ref().filter(|_| similar).map(|(vs, z)| {
                    let mapped: Vec<Vertex> = vs
                        .iter()
                        .map(|v| {
                            let w = t.transform_point3(v.p.extend(*z));
                            let kind = match v.kind {
                                VertexKind::Arc { radius } => VertexKind::Arc {
                                    radius: radius * m.x_axis.length(),
                                },
                                k => k,
                            };
                            Vertex::new(w.truncate(), kind)
                        })
                        .collect();
                    (mapped, t.transform_point3(DVec3::new(0.0, 0.0, *z)).z)
                });
                Piece::Curve(CurveData {
                    exact,
                    pts: c.pts.iter().map(|p| t.transform_point3(*p)).collect(),
                    closed: c.closed,
                })
            }
        }
    }
}

/// Evaluates representation items.
pub struct Eval<'a> {
    pub cx: &'a Ctx<'a>,
    pub pieces: Vec<Piece>,
    pub issues: Vec<String>,
    /// True when a bounding box stands in for geometry that is not supported.
    pub approximate: bool,
    /// Triangles of irregular networks that are holes or voids (flag below zero) and were left out.
    pub voids: usize,
    /// The work budget of the whole import for booleans. Without one an evaluation has its own.
    pub fuel: Option<&'a csg::Fuel>,
    /// Advanced breps that were read already, by the number of the item (see `advanced_brep`).
    pub brep_cache: Option<&'a BrepCache>,
    pub tris: usize,
    pub max_tris: usize,
    /// Tells if a mapped item may become a symbol instance.
    pub instancing: Option<&'a dyn Fn(u64) -> bool>,
    /// Curve tolerance in file units.
    pub tol: f64,
    /// Items visited and pieces copied so far. A file cannot make the work grow without a limit.
    visits: usize,
    /// Pieces of each mapped source, in the frame of the source. `None` while the source is being read.
    cache: HashMap<u64, Option<Rc<Vec<Piece>>>>,
}

/// The most triangles of the two operands of an `IfcBooleanResult` (the work of the tree grows
/// faster than the number of triangles).
const MAX_BOOLEAN_TRIANGLES: usize = 20_000;
const MAX_DEPTH: usize = 16;
/// Most items that one evaluation visits (and pieces it copies from mapped sources).
const MAX_VISITS: usize = 100_000;

impl<'a> Eval<'a> {
    pub fn new(cx: &'a Ctx<'a>, max_tris: usize) -> Eval<'a> {
        Eval {
            cx,
            pieces: Vec::new(),
            issues: Vec::new(),
            approximate: false,
            voids: 0,
            fuel: None,
            brep_cache: None,
            tris: 0,
            max_tris,
            instancing: None,
            tol: 2.0 / cx.units.length_mm,
            visits: 0,
            cache: HashMap::new(),
        }
    }

    fn issue(&mut self, text: impl Into<String>) {
        let t = text.into();
        if !self.issues.contains(&t) {
            self.issues.push(t);
        }
    }

    fn add_mesh(&mut self, m: MeshData) {
        if m.t.is_empty() {
            return;
        }
        self.tris += m.t.len();
        if self.tris > self.max_tris {
            self.issue("the triangle limit was reached: geometry was dropped");
            return;
        }
        self.pieces.push(Piece::Mesh(m));
    }

    /// Evaluates one representation item with the transform `xf` (item frame to world).
    pub fn item(&mut self, id: u64, xf: &DAffine3, depth: usize) {
        if depth > MAX_DEPTH {
            self.issue("representation items nest too deep");
            return;
        }
        if self.tris > self.max_tris {
            return;
        }
        self.visits += 1;
        if self.visits > MAX_VISITS {
            self.issue("the file has too many nested items: the rest is dropped");
            return;
        }
        let Some(e) = self.cx.ent(id) else {
            self.issue(format!("missing representation item #{id}"));
            return;
        };
        match e.name.as_str() {
            "IFCEXTRUDEDAREASOLID" => self.extruded(&e, xf),
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" => {
                self.triangulated(&e, xf)
            }
            "IFCPOLYGONALFACESET" => self.polygonal(&e, xf),
            _ if matches!(
                e.name.as_str(),
                "IFCFACETEDBREP"
                    | "IFCFACETEDBREPWITHVOIDS"
                    | "IFCSHELLBASEDSURFACEMODEL"
                    | "IFCFACEBASEDSURFACEMODEL"
                    | "IFCCLOSEDSHELL"
                    | "IFCOPENSHELL"
                    | "IFCCONNECTEDFACESET"
            ) && self.has_curved_faces(&e) =>
            {
                // Faces on curved surfaces, or with edge loops: the STEP reader cuts them.
                self.advanced_brep(&e, xf)
            }
            "IFCFACETEDBREP" => self.shell(attr(&e, 0), xf),
            "IFCFACETEDBREPWITHVOIDS" => self.shell(attr(&e, 0), xf),
            "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                let mut m = MeshData::default();
                for s in items(attr(&e, 0)) {
                    if let Some(sh) = self.cx.get(s) {
                        self.shell_into(&sh, xf, &mut m);
                    }
                }
                m.weld(1e-9);
                self.add_mesh(m);
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" => {
                let mut m = MeshData::default();
                self.shell_into(&e, xf, &mut m);
                m.weld(1e-9);
                self.add_mesh(m);
            }
            "IFCMAPPEDITEM" => self.mapped(&e, xf, depth),
            "IFCBOOLEANRESULT" | "IFCBOOLEANCLIPPINGRESULT" => self.boolean(&e, xf, depth),
            "IFCREVOLVEDAREASOLID" => self.revolved(&e, xf),
            "IFCSWEPTDISKSOLID" | "IFCSWEPTDISKSOLIDPOLYGONAL" => self.swept_disk(&e, xf),
            "IFCCSGSOLID" => self.csg_solid(&e, xf, depth),
            "IFCBLOCK"
            | "IFCSPHERE"
            | "IFCRIGHTCIRCULARCYLINDER"
            | "IFCRIGHTCIRCULARCONE"
            | "IFCRECTANGULARPYRAMID" => self.primitive(&e, xf),
            "IFCADVANCEDBREP" | "IFCADVANCEDBREPWITHVOIDS" => self.advanced_brep(&e, xf),
            "IFCBOUNDINGBOX" => self.bounding_box(&e, xf),
            "IFCGEOMETRICSET" | "IFCGEOMETRICCURVESET" => {
                for v in items(attr(&e, 0)) {
                    if let Some(i) = v.as_ref_id() {
                        self.item(i, xf, depth + 1);
                    }
                }
            }
            "IFCPOLYLINE"
            | "IFCINDEXEDPOLYCURVE"
            | "IFCCOMPOSITECURVE"
            | "IFCTRIMMEDCURVE"
            | "IFCCIRCLE" => self.curve_piece(id, xf),
            "IFCCARTESIANPOINT" => {
                if let Some(p) = super::ctx::point_of(&e) {
                    self.pieces.push(Piece::Point(xf.transform_point3(p)));
                }
            }
            other => self.issue(format!("unsupported geometry {other}")),
        }
    }

    // ------------------------------------------------------------ mapped items

    fn mapped(&mut self, e: &crate::step::Entity, xf: &DAffine3, depth: usize) {
        let Some(src_id) = attr(e, 0).as_ref_id() else {
            return;
        };
        let Some(src) = self.cx.ent(src_id) else {
            self.issue("a mapped item has no source");
            return;
        };
        let origin = self
            .cx
            .get(attr(&src, 0))
            .and_then(|o| self.cx.axis3(&o))
            .unwrap_or(DAffine3::IDENTITY);
        let target = self.cx.operator(attr(e, 1)).unwrap_or(DAffine3::IDENTITY);
        if origin.matrix3.determinant().abs() < 1e-18 || target.matrix3.determinant().abs() < 1e-18
        {
            self.issue("a mapped item has a flat transform");
            return;
        }
        // A point of the source goes through the origin of the map, then through the target.
        let source_to_world = *xf * target * origin;
        if let Some(ok) = self.instancing
            && ok(src_id)
        {
            // The definition lives in the frame of the source.
            self.pieces.push(Piece::Instance {
                source: src_id,
                xf: source_to_world,
            });
            return;
        }
        let Some(cached) = self.source_pieces(src_id, &src, depth) else {
            return;
        };
        for p in cached.iter() {
            self.visits += 1;
            self.tris += p.cost();
            if self.visits > MAX_VISITS || self.tris > self.max_tris {
                self.issue("the triangle or item limit was reached: geometry was dropped");
                return;
            }
            self.pieces.push(p.transformed(&source_to_world));
        }
    }

    /// The pieces of a representation map in the frame of the source. They are made once per source.
    fn source_pieces(
        &mut self,
        src_id: u64,
        src: &crate::step::Entity,
        depth: usize,
    ) -> Option<Rc<Vec<Piece>>> {
        match self.cache.get(&src_id) {
            Some(Some(hit)) => return Some(hit.clone()),
            Some(None) => {
                self.issue("a mapped item contains itself");
                return None;
            }
            None => {}
        }
        let Some(rep) = self.cx.get(attr(src, 1)) else {
            self.issue("a mapped item has no representation");
            return None;
        };
        self.cache.insert(src_id, None);
        let saved = std::mem::take(&mut self.pieces);
        for i in items(attr(&rep, 3)) {
            if let Some(id) = i.as_ref_id() {
                self.item(id, &DAffine3::IDENTITY, depth + 1);
            }
        }
        let made = Rc::new(std::mem::replace(&mut self.pieces, saved));
        self.cache.insert(src_id, Some(made.clone()));
        Some(made)
    }

    // ------------------------------------------------------------ boolean clipping

    fn boolean(&mut self, e: &crate::step::Entity, xf: &DAffine3, depth: usize) {
        let op = enum_of(attr(e, 0)).unwrap_or("");
        let Some(first) = attr(e, 1).as_ref_id() else {
            return;
        };
        let start = self.pieces.len();
        // The first operand is evaluated in place; the clip works on what it added.
        self.item(first, xf, depth + 1);
        let second = self.cx.get(attr(e, 2));
        match (op, second) {
            ("DIFFERENCE", Some(s)) if s.name == "IFCHALFSPACESOLID" => {
                let Some(plane) = self.cx.get(attr(&s, 0)).filter(|p| p.name == "IFCPLANE") else {
                    self.issue("a half space is not bounded by a plane: not clipped");
                    return;
                };
                let Some(pos) = self.cx.get(attr(&plane, 0)).and_then(|p| self.cx.axis3(&p)) else {
                    return;
                };
                let world = *xf * pos;
                let n = world.matrix3.z_axis.normalize_or_zero();
                let o = world.translation;
                // With the agreement flag TRUE the normal points out of the removed half space.
                let keep = if flag(attr(&s, 1)).unwrap_or(true) {
                    n
                } else {
                    -n
                };
                // Convert the pieces of the first operand to meshes, then clip them.
                let tail: Vec<Piece> = self.pieces.drain(start..).collect();
                for p in tail {
                    match piece_mesh(&p) {
                        Some(m) => {
                            let clipped = clip_mesh(&m, o, keep);
                            if !clipped.t.is_empty() {
                                self.pieces.push(Piece::Mesh(clipped));
                            }
                        }
                        None => self.pieces.push(p),
                    }
                }
            }
            ("DIFFERENCE", Some(s)) if s.name == "IFCPOLYGONALBOUNDEDHALFSPACE" => {
                // The removed volume is a prism of the polygon, cut by the plane.
                let tail: Vec<Piece> = self.pieces.drain(start..).collect();
                let Some(host) = solid_mesh(&tail) else {
                    self.issue(
                        "a bounded half space is not applied: the solid is not a closed mesh",
                    );
                    self.pieces.extend(tail);
                    return;
                };
                match self.bounded_half_space(&s, xf, &host) {
                    Some(cutter) => self.combine(csg::Op::Difference, host, cutter, tail),
                    None => {
                        self.issue("a bounded half space is not applied: its boundary or plane cannot be read");
                        self.pieces.extend(tail);
                    }
                }
            }
            (o @ ("DIFFERENCE" | "UNION" | "INTERSECTION"), Some(s)) => {
                // The second operand is a solid of its own.
                let mid = self.pieces.len();
                self.item(s.id, xf, depth + 1);
                let second: Vec<Piece> = self.pieces.drain(mid..).collect();
                let first: Vec<Piece> = self.pieces.drain(start..).collect();
                let kind = match o {
                    "DIFFERENCE" => csg::Op::Difference,
                    "UNION" => csg::Op::Union,
                    _ => csg::Op::Intersection,
                };
                match (solid_mesh(&first), solid_mesh(&second)) {
                    (Some(a), Some(b)) => {
                        let mut all = first;
                        all.extend(second);
                        self.combine(kind, a, b, all)
                    }
                    _ => {
                        self.issue(format!(
                            "a boolean {o} with {} is not applied: only the first operand is used",
                            s.name
                        ));
                        self.pieces.extend(first);
                        if kind == csg::Op::Union {
                            self.pieces.extend(second);
                        }
                    }
                }
            }
            (_, Some(s)) => self.issue(format!(
                "a boolean {op} with {} is not applied: only the first operand is used",
                s.name
            )),
            _ => {}
        }
    }

    /// Applies a boolean operation to two meshes. When it is refused, the `fallback` pieces are kept
    /// and an issue says why.
    fn combine(&mut self, op: csg::Op, a: MeshData, b: MeshData, fallback: Vec<Piece>) {
        // Counted before the work: the operands must fit in the memory of the boolean.
        if a.t.len() + b.t.len() > MAX_BOOLEAN_TRIANGLES {
            self.issue(format!(
                "a boolean is not applied: the operands have {} triangles, more than {}: the operands are kept as they are",
                a.t.len() + b.t.len(),
                MAX_BOOLEAN_TRIANGLES
            ));
            self.pieces.extend(fallback);
            return;
        }
        let result = match self.fuel {
            Some(f) => f.scoped(csg::PER_CALL_FUEL, |own| csg::boolean(&a, &b, op, own)),
            None => csg::boolean(&a, &b, op, &csg::Fuel::new(csg::PER_CALL_FUEL)),
        };
        match result {
            Ok(m) => {
                if !m.t.is_empty() {
                    self.add_mesh(m);
                }
            }
            Err(why) => {
                self.issue(format!(
                    "a boolean {} is not applied ({why}): the operands are kept as they are",
                    match op {
                        csg::Op::Difference => "DIFFERENCE",
                        csg::Op::Union => "UNION",
                        csg::Op::Intersection => "INTERSECTION",
                    }
                ));
                self.pieces.extend(fallback);
            }
        }
    }

    /// The volume that an `IfcPolygonalBoundedHalfSpace` removes, as a closed mesh: the prism of its
    /// polygon along the Z axis of its position, cut by the base plane. `host` sets the length of the prism.
    fn bounded_half_space(
        &mut self,
        s: &crate::step::Entity,
        xf: &DAffine3,
        host: &MeshData,
    ) -> Option<MeshData> {
        let plane = self.cx.get(attr(s, 0)).filter(|p| p.name == "IFCPLANE")?;
        let plane_pos = self
            .cx
            .get(attr(&plane, 0))
            .and_then(|p| self.cx.axis3(&p))?;
        let pos = self
            .cx
            .get(attr(s, 2))
            .and_then(|p| self.cx.axis3(&p))
            .unwrap_or(DAffine3::IDENTITY);
        let ring = self.ring(attr(s, 3))?;
        let (lo, hi) = host.bounds()?;
        let reach = ((hi - lo).length() + 1.0) * 4.0
            + (xf.translation - lo).length()
            + pos.translation.length();
        let prism = Extrusion {
            regions: vec![Region {
                outer: ring,
                holes: Vec::new(),
            }],
            dir: DVec3::new(0.0, 0.0, 2.0 * reach),
            xf: *xf * pos * DAffine3::from_translation(DVec3::new(0.0, 0.0, -reach)),
            tol: self.tol,
        }
        .mesh();
        let world = *xf * plane_pos;
        let n = world.matrix3.z_axis.normalize_or_zero();
        // With the agreement flag TRUE the normal points out of the removed half space.
        let removed = if flag(attr(s, 1)).unwrap_or(true) {
            -n
        } else {
            n
        };
        let cutter = clip_mesh(&prism, world.translation, removed);
        (!cutter.t.is_empty()).then_some(cutter)
    }

    // ------------------------------------------------------------ bounding box

    fn bounding_box(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(c) = self.cx.point(attr(e, 0)) else {
            return;
        };
        let d = DVec3::new(
            num(attr(e, 1)).unwrap_or(0.0),
            num(attr(e, 2)).unwrap_or(0.0),
            num(attr(e, 3)).unwrap_or(0.0),
        );
        if !(d.is_finite() && d.min_element() > 0.0) {
            return;
        }
        let mut m = MeshData::cuboid(c, c + d);
        m.transform(xf);
        self.add_mesh(m);
    }

    // ------------------------------------------------------------ extruded solid

    fn extruded(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(regions) = self.profile(attr(e, 0).as_ref_id().unwrap_or(0)) else {
            return;
        };
        let pos = self
            .cx
            .get(attr(e, 1))
            .and_then(|p| self.cx.axis3(&p))
            .unwrap_or(DAffine3::IDENTITY);
        let dir = self
            .cx
            .direction(attr(e, 2))
            .and_then(|d| d.try_normalize())
            .unwrap_or(DVec3::Z);
        let depth = num(attr(e, 3)).unwrap_or(0.0);
        if !(depth.is_finite() && depth > 0.0) {
            self.issue("an extruded solid has no depth");
            return;
        }
        let ex = Extrusion {
            regions,
            dir: dir * depth,
            xf: *xf * pos,
            tol: self.tol,
        };
        self.tris += ex
            .regions
            .iter()
            .map(|r| r.outer.len() * 4 + 2)
            .sum::<usize>();
        if self.tris > self.max_tris {
            self.issue("the triangle limit was reached: geometry was dropped");
            return;
        }
        self.pieces.push(Piece::Extrusion(ex));
    }

    fn pos2(&self, v: &Value) -> DAffine2 {
        self.cx
            .get(v)
            .and_then(|p| self.cx.axis2(&p))
            .unwrap_or(DAffine2::IDENTITY)
    }

    /// The regions of a profile definition.
    pub fn profile(&mut self, id: u64) -> Option<Vec<Region>> {
        let regions = self.profile_inner(id)?;
        // Every number is checked before an overlay or a mesh sees it: a profile of 1E308 would
        // overflow the integer grid of the overlay. The bound is the one of the overlay, in file
        // units, so projected coordinates in a profile (with the placements at the origin) pass;
        // the range test of the builder comes later and the file is shifted when it fails.
        let limit = kadakas_core::geom::MAX_OVERLAY_COORD;
        let fine = |v: &Vertex| {
            v.p.is_finite()
                && v.p.abs().max_element() <= limit
                && match v.kind {
                    VertexKind::Arc { radius } => radius.is_finite() && radius.abs() <= limit,
                    _ => true,
                }
        };
        if regions
            .iter()
            .all(|r| r.outer.iter().chain(r.holes.iter().flatten()).all(fine))
        {
            Some(regions)
        } else {
            self.issue("a profile has numbers that are too large (more than 1e12) or not a number");
            None
        }
    }

    fn profile_inner(&mut self, id: u64) -> Option<Vec<Region>> {
        let e = self.cx.ent(id)?;
        let corner = |p: DVec2| Vertex::corner(p);
        let place = |a: &DAffine2, v: Vec<Vertex>| -> Vec<Vertex> {
            v.into_iter()
                .map(|x| Vertex::new(a.transform_point2(x.p), x.kind))
                .collect()
        };
        let rect = |x: f64, y: f64, r: f64| -> Vec<Vertex> {
            let (hx, hy) = (x / 2.0, y / 2.0);
            let kind = if r > 0.0 {
                VertexKind::Arc { radius: r }
            } else {
                VertexKind::Corner
            };
            [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
                .iter()
                .map(|(a, b)| Vertex::new(DVec2::new(*a, *b), kind))
                .collect()
        };
        let circle = |r: f64| -> Vec<Vertex> {
            (0..4)
                .map(|k| {
                    let a = PI / 2.0 * k as f64;
                    Vertex::new(DVec2::from_angle(a) * r, VertexKind::PointOnArc)
                })
                .collect()
        };
        let regions = match e.name.as_str() {
            "IFCRECTANGLEPROFILEDEF" | "IFCROUNDEDRECTANGLEPROFILEDEF" => {
                let (x, y) = (num(attr(&e, 3))?, num(attr(&e, 4))?);
                let r = if e.name == "IFCROUNDEDRECTANGLEPROFILEDEF" {
                    num(attr(&e, 5)).unwrap_or(0.0)
                } else {
                    0.0
                };
                if !(x > 0.0 && y > 0.0) {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                vec![Region {
                    outer: place(&a, rect(x, y, r.min(x.min(y) / 2.0))),
                    holes: vec![],
                }]
            }
            "IFCRECTANGLEHOLLOWPROFILEDEF" => {
                let (x, y, t) = (num(attr(&e, 3))?, num(attr(&e, 4))?, num(attr(&e, 5))?);
                if !(x > 0.0 && y > 0.0 && t > 0.0 && 2.0 * t < x.min(y)) {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                vec![Region {
                    outer: place(&a, rect(x, y, 0.0)),
                    holes: vec![place(&a, rect(x - 2.0 * t, y - 2.0 * t, 0.0))],
                }]
            }
            "IFCCIRCLEPROFILEDEF" | "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let r = num(attr(&e, 3))?;
                if r <= 0.0 {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                let holes = if e.name == "IFCCIRCLEHOLLOWPROFILEDEF" {
                    let t = num(attr(&e, 4))?;
                    if t <= 0.0 || t >= r {
                        return None;
                    }
                    vec![place(&a, circle(r - t))]
                } else {
                    vec![]
                };
                vec![Region {
                    outer: place(&a, circle(r)),
                    holes,
                }]
            }
            "IFCELLIPSEPROFILEDEF" => {
                let (rx, ry) = (num(attr(&e, 3))?, num(attr(&e, 4))?);
                if !(rx > 0.0 && ry > 0.0) {
                    return None;
                }
                let a = self.pos2(attr(&e, 2));
                let pts = (0..48)
                    .map(|k| {
                        let t = TAU * k as f64 / 48.0;
                        corner(DVec2::new(rx * t.cos(), ry * t.sin()))
                    })
                    .collect();
                vec![Region {
                    outer: place(&a, pts),
                    holes: vec![],
                }]
            }
            "IFCISHAPEPROFILEDEF" => {
                let (w, d, tw, tf) = (
                    num(attr(&e, 3))?,
                    num(attr(&e, 4))?,
                    num(attr(&e, 5))?,
                    num(attr(&e, 6))?,
                );
                if !(w > 0.0 && d > 0.0 && tw > 0.0 && tf > 0.0 && 2.0 * tf < d && tw < w) {
                    return None;
                }
                let (hw, hd, ht) = (w / 2.0, d / 2.0, tw / 2.0);
                let a = self.pos2(attr(&e, 2));
                let pts = [
                    (-hw, -hd),
                    (hw, -hd),
                    (hw, -hd + tf),
                    (ht, -hd + tf),
                    (ht, hd - tf),
                    (hw, hd - tf),
                    (hw, hd),
                    (-hw, hd),
                    (-hw, hd - tf),
                    (-ht, hd - tf),
                    (-ht, -hd + tf),
                    (-hw, -hd + tf),
                ]
                .iter()
                .enumerate()
                .map(|(i, (x, y))| {
                    // The four inner corners, between the web and the flanges, have the fillet.
                    let r = num(attr(&e, 7)).unwrap_or(0.0).min(hw - ht).min(hd - tf);
                    let kind = if r > 0.0 && matches!(i, 3 | 4 | 9 | 10) {
                        VertexKind::Arc { radius: r }
                    } else {
                        VertexKind::Corner
                    };
                    Vertex::new(DVec2::new(*x, *y), kind)
                })
                .collect();
                vec![Region {
                    outer: place(&a, pts),
                    holes: vec![],
                }]
            }
            "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                let outer = self.ring(attr(&e, 2))?;
                let mut holes = Vec::new();
                if e.name == "IFCARBITRARYPROFILEDEFWITHVOIDS" {
                    for h in items(attr(&e, 3)) {
                        if let Some(r) = self.ring(h) {
                            holes.push(r);
                        }
                    }
                }
                vec![Region { outer, holes }]
            }
            "IFCCOMPOSITEPROFILEDEF" => {
                let mut all = Vec::new();
                for p in items(attr(&e, 2)) {
                    if let Some(i) = p.as_ref_id()
                        && let Some(mut r) = self.profile(i)
                    {
                        all.append(&mut r);
                    }
                }
                all
            }
            other => {
                self.issue(format!("unsupported profile {other}"));
                return None;
            }
        };
        if regions.is_empty() {
            None
        } else {
            Some(regions)
        }
    }

    /// A closed outline from a curve reference.
    fn ring(&mut self, v: &Value) -> Option<Vec<Vertex>> {
        let id = v.as_ref_id()?;
        let mut pts = Vec::new();
        let mut zs = Vec::new();
        if !self.curve_vertices(id, &mut pts, &mut zs, 0) {
            self.issue("a profile outline is not supported");
            return None;
        }
        // Drop the closing point.
        while pts.len() > 1 && pts[0].p.distance_squared(pts[pts.len() - 1].p) < 1e-18 {
            pts.pop();
        }
        (pts.len() >= 3).then_some(pts)
    }

    // ------------------------------------------------------------ curves

    fn curve_piece(&mut self, id: u64, xf: &DAffine3) {
        let mut pts = Vec::new();
        let mut zs = Vec::new();
        if !self.curve_vertices(id, &mut pts, &mut zs, 0) || pts.len() < 2 {
            self.issue("a curve is not supported");
            return;
        }
        let closed = pts.len() > 2 && pts[0].p.distance_squared(pts[pts.len() - 1].p) < 1e-12;
        if closed {
            pts.pop();
            zs.pop();
        }
        let up = xf.matrix3.z_axis.normalize_or_zero();
        let horizontal = up.x.abs() < 1e-9 && up.y.abs() < 1e-9 && up.z > 0.0;
        let flat_z = zs.iter().all(|z| (z - zs[0]).abs() < 1e-9);
        let exact = (horizontal && flat_z && (xf.matrix3.x_axis.z.abs() < 1e-9)).then(|| {
            let mapped: Vec<Vertex> = pts
                .iter()
                .map(|v| {
                    let w = xf.transform_point3(v.p.extend(0.0));
                    Vertex::new(w.truncate(), v.kind)
                })
                .collect();
            let z = xf.transform_point3(DVec3::new(0.0, 0.0, zs[0])).z;
            (mapped, z)
        });
        // The flat points: curved vertices are flattened for the 3D case.
        let flat = curve::flatten(&pts, closed, self.tol.max(1e-9));
        let z0 = zs.first().copied().unwrap_or(0.0);
        let world: Vec<DVec3> = if flat.len() == pts.len() {
            flat.iter()
                .zip(&zs)
                .map(|(p, z)| xf.transform_point3(p.extend(*z)))
                .collect()
        } else {
            flat.iter()
                .map(|p| xf.transform_point3(p.extend(z0)))
                .collect()
        };
        self.pieces.push(Piece::Curve(CurveData {
            exact,
            pts: world,
            closed,
        }));
    }

    /// Appends the vertices of a curve. Curved segments become `PointOnArc` vertices, so the arcs stay exact.
    pub fn curve_vertices(
        &mut self,
        id: u64,
        out: &mut Vec<Vertex>,
        zs: &mut Vec<f64>,
        depth: usize,
    ) -> bool {
        if depth > 4 {
            return false;
        }
        let Some(e) = self.cx.ent(id) else {
            return false;
        };
        let push = |out: &mut Vec<Vertex>, zs: &mut Vec<f64>, p: DVec3, kind: VertexKind| {
            if let Some(last) = out.last()
                && kind == VertexKind::Corner
                && last.kind == VertexKind::Corner
                && last.p.distance_squared(p.truncate()) < 1e-18
                && (zs.last().copied().unwrap_or(0.0) - p.z).abs() < 1e-12
            {
                return;
            }
            out.push(Vertex::new(p.truncate(), kind));
            zs.push(p.z);
        };
        match e.name.as_str() {
            "IFCPOLYLINE" => {
                let mut any = false;
                for v in items(attr(&e, 0)) {
                    if let Some(p) = self.cx.point(v) {
                        push(out, zs, p, VertexKind::Corner);
                        any = true;
                    }
                }
                any
            }
            "IFCINDEXEDPOLYCURVE" => {
                let Some(list) = self.cx.get(attr(&e, 0)) else {
                    return false;
                };
                let coords = point_list(&list);
                if coords.is_empty() {
                    return false;
                }
                let at = |i: i64| -> Option<DVec3> {
                    usize::try_from(i - 1)
                        .ok()
                        .and_then(|k| coords.get(k))
                        .copied()
                };
                let segs = items(attr(&e, 1));
                if segs.is_empty() {
                    for p in &coords {
                        push(out, zs, *p, VertexKind::Corner);
                    }
                    return true;
                }
                for s in segs {
                    let (name, idx) = match s {
                        Value::Typed(n, inner) => (n.as_str(), items(inner)),
                        other => ("IFCLINEINDEX", items(other)),
                    };
                    let ids: Vec<i64> = idx.iter().filter_map(int).collect();
                    if name == "IFCARCINDEX" && ids.len() == 3 {
                        let (Some(a), Some(b), Some(c)) = (at(ids[0]), at(ids[1]), at(ids[2]))
                        else {
                            return false;
                        };
                        push(out, zs, a, VertexKind::Corner);
                        push(out, zs, b, VertexKind::PointOnArc);
                        push(out, zs, c, VertexKind::Corner);
                    } else {
                        for i in ids {
                            let Some(p) = at(i) else { return false };
                            push(out, zs, p, VertexKind::Corner);
                        }
                    }
                }
                true
            }
            "IFCCOMPOSITECURVE" => {
                let mut any = false;
                for seg in self.cx.get_all(attr(&e, 0)) {
                    let same = flag(attr(&seg, 1)).unwrap_or(true);
                    let Some(parent) = attr(&seg, 2).as_ref_id() else {
                        continue;
                    };
                    let (mut p2, mut z2) = (Vec::new(), Vec::new());
                    if !self.curve_vertices(parent, &mut p2, &mut z2, depth + 1) {
                        return false;
                    }
                    if !same {
                        p2.reverse();
                        z2.reverse();
                    }
                    for (v, z) in p2.into_iter().zip(z2) {
                        push(out, zs, v.p.extend(z), v.kind);
                    }
                    any = true;
                }
                any
            }
            "IFCTRIMMEDCURVE" => {
                let Some(basis) = self.cx.get(attr(&e, 0)) else {
                    return false;
                };
                let sense = flag(attr(&e, 3)).unwrap_or(true);
                match basis.name.as_str() {
                    "IFCCIRCLE" => {
                        let Some(path) = self.circle_path(&basis, attr(&e, 1), attr(&e, 2), sense)
                        else {
                            return false;
                        };
                        for (p, k) in path {
                            push(out, zs, p, k);
                        }
                        true
                    }
                    "IFCLINE" => {
                        let pt = |v: &Value| items(v).iter().find_map(|x| self.cx.point(x));
                        match (pt(attr(&e, 1)), pt(attr(&e, 2))) {
                            (Some(a), Some(b)) => {
                                push(out, zs, a, VertexKind::Corner);
                                push(out, zs, b, VertexKind::Corner);
                                true
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }
            "IFCCIRCLE" => {
                let Some(path) =
                    self.circle_path(&e, &Value::List(vec![]), &Value::List(vec![]), true)
                else {
                    return false;
                };
                for (p, k) in path {
                    push(out, zs, p, k);
                }
                true
            }
            _ => false,
        }
    }

    /// The vertices of an arc of a circle, in the frame of the curve. Arcs over 180 degrees are split,
    /// so that three points always fix them.
    fn circle_path(
        &self,
        circle: &crate::step::Entity,
        t1: &Value,
        t2: &Value,
        sense: bool,
    ) -> Option<Vec<(DVec3, VertexKind)>> {
        let pos = self
            .cx
            .get(attr(circle, 0))
            .and_then(|p| self.cx.axis3(&p))?;
        let r = num(attr(circle, 1))?;
        if !(r.is_finite() && r > 0.0) {
            return None;
        }
        let inv = pos.inverse();
        let angle = |trim: &Value| -> Option<f64> {
            for v in items(trim) {
                if let Some(p) = self.cx.point(v) {
                    let l = inv.transform_point3(p);
                    return Some(l.y.atan2(l.x));
                }
            }
            for v in items(trim) {
                if let Some(n) = num(v) {
                    return Some(n * self.cx.units.angle_rad);
                }
            }
            None
        };
        let (a0, a1) = match (angle(t1), angle(t2)) {
            (Some(a), Some(b)) => (a, b),
            _ => (0.0, TAU),
        };
        let mut sweep = if sense {
            (a1 - a0).rem_euclid(TAU)
        } else {
            -(a0 - a1).rem_euclid(TAU)
        };
        if sweep.abs() < 1e-9 {
            sweep = if sense { TAU } else { -TAU };
        }
        let n = if sweep.abs() > PI * 0.99 { 2 } else { 1 };
        let at = |a: f64| pos.transform_point3(DVec3::new(r * a.cos(), r * a.sin(), 0.0));
        let mut out = Vec::new();
        for k in 0..n {
            let s = a0 + sweep * k as f64 / n as f64;
            let m = a0 + sweep * (k as f64 + 0.5) / n as f64;
            out.push((at(s), VertexKind::Corner));
            out.push((at(m), VertexKind::PointOnArc));
        }
        out.push((at(a0 + sweep), VertexKind::Corner));
        Some(out)
    }

    // ------------------------------------------------------------ face sets and breps

    fn triangulated(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(list) = self.cx.get(attr(e, 0)) else {
            return;
        };
        let coords = point_list(&list);
        // IfcTriangulatedIrregularNetwork adds the flags of the triangles: the sixth attribute in
        // IFC4X3, the fifth in the release candidates of IFC4X3, which have no PnIndex.
        let network = e.name == "IFCTRIANGULATEDIRREGULARNETWORK";
        let (pn_at, flags_at) = if network && e.attrs.len() < 6 {
            (None, Some(4))
        } else {
            (Some(4), network.then_some(5))
        };
        let pn: Vec<i64> = pn_at
            .map(|i| items(attr(e, i)).iter().filter_map(int).collect())
            .unwrap_or_default();
        let flags: Vec<i64> = flags_at
            .map(|i| items(attr(e, i)).iter().filter_map(int).collect())
            .unwrap_or_default();
        let idx = |v: &Value| -> Option<u32> {
            let mut i = int(v)?;
            if !pn.is_empty() {
                i = *pn.get(usize::try_from(i - 1).ok()?)?;
            }
            let k = usize::try_from(i - 1).ok()?;
            (k < coords.len()).then_some(k as u32)
        };
        let mut m = MeshData {
            v: coords.iter().map(|p| xf.transform_point3(*p)).collect(),
            t: Vec::new(),
            exact: None,
        };
        let (mut holes, mut voids, mut breaklines) = (0usize, 0usize, 0usize);
        for (n, t) in items(attr(e, 3)).iter().enumerate() {
            let l = items(t);
            if l.len() != 3 {
                continue;
            }
            // -2 is a void, -1 a hole: neither is part of the surface. Positive flags mark breaklines.
            match flags.get(n).copied().unwrap_or(0) {
                -1 => {
                    holes += 1;
                    continue;
                }
                f if f < -1 => {
                    voids += 1;
                    continue;
                }
                f if f > 0 => breaklines += 1,
                _ => {}
            }
            if let (Some(a), Some(b), Some(c)) = (idx(&l[0]), idx(&l[1]), idx(&l[2])) {
                m.t.push([a, b, c]);
            }
        }
        if holes + voids > 0 {
            self.voids += holes + voids;
            self.issue(format!(
                "{holes} holes and {voids} voids of an irregular network (flags -1 and -2) stay open"
            ));
        }
        if breaklines > 0 {
            self.issue(format!(
                "{breaklines} triangles of an irregular network have breakline flags: the breaklines are not kept as lines"
            ));
        }
        if xf.matrix3.determinant() < 0.0 {
            m.flip();
        }
        m.compact();
        self.add_mesh(m);
    }

    fn polygonal(&mut self, e: &crate::step::Entity, xf: &DAffine3) {
        let Some(list) = self.cx.get(attr(e, 0)) else {
            return;
        };
        let coords: Vec<DVec3> = point_list(&list)
            .iter()
            .map(|p| xf.transform_point3(*p))
            .collect();
        let pn: Vec<i64> = items(attr(e, 3)).iter().filter_map(int).collect();
        let ring = |v: &Value| -> Option<Vec<DVec3>> {
            items(v)
                .iter()
                .map(|i| {
                    let mut i = int(i)?;
                    if !pn.is_empty() {
                        i = *pn.get(usize::try_from(i - 1).ok()?)?;
                    }
                    coords.get(usize::try_from(i - 1).ok()?).copied()
                })
                .collect()
        };
        let mut m = MeshData::default();
        for f in self.cx.get_all(attr(e, 2)) {
            let Some(outer) = ring(attr(&f, 0)) else {
                continue;
            };
            let mut rings = vec![outer];
            if f.name == "IFCINDEXEDPOLYGONALFACEWITHVOIDS" {
                rings.extend(items(attr(&f, 1)).iter().filter_map(&ring));
            }
            add_polygon(&mut m, &rings);
        }
        if xf.matrix3.determinant() < 0.0 {
            m.flip();
        }
        m.weld(1e-9);
        self.add_mesh(m);
    }

    fn shell(&mut self, outer: &Value, xf: &DAffine3) {
        let Some(sh) = self.cx.get(outer) else {
            return;
        };
        let mut m = MeshData::default();
        self.shell_into(&sh, xf, &mut m);
        m.weld(1e-9);
        self.add_mesh(m);
    }

    fn shell_into(&mut self, shell: &crate::step::Entity, xf: &DAffine3, m: &mut MeshData) {
        let first_tri = m.t.len();
        for f in self.cx.get_all(attr(shell, 0)) {
            if f.name != "IFCFACE" && f.name != "IFCADVANCEDFACE" {
                continue;
            }
            let mut rings: Vec<Vec<DVec3>> = Vec::new();
            for b in self.cx.get_all(attr(&f, 0)) {
                let Some(l) = self.cx.get(attr(&b, 0)) else {
                    continue;
                };
                if l.name != "IFCPOLYLOOP" {
                    self.issue(format!("unsupported face bound {}", l.name));
                    continue;
                }
                let mut pts: Vec<DVec3> = items(attr(&l, 0))
                    .iter()
                    .filter_map(|p| self.cx.point(p))
                    .map(|p| xf.transform_point3(p))
                    .collect();
                if flag(attr(&b, 1)) == Some(false) {
                    pts.reverse();
                }
                // The outer bound comes first.
                if b.name == "IFCFACEOUTERBOUND" {
                    rings.insert(0, pts);
                } else {
                    rings.push(pts);
                }
            }
            add_polygon(m, &rings);
        }
        if xf.matrix3.determinant() < 0.0 {
            for t in &mut m.t[first_tri..] {
                t.swap(1, 2);
            }
        }
    }
}

thread_local! {
    /// Faces whose numbers were beyond the range of the overlay since the last `take_range_drops`.
    static RANGE_DROPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`kadakas_core::geom::split_with_holes`] that counts a face with numbers out of range (the
/// overlay refuses them): the product reports it (see `take_range_drops`), the face is not lost
/// without a word.
pub(crate) fn split_counted(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Vec<Vec<DVec2>> {
    if !kadakas_core::geom::rings_in_range(outer, holes) {
        RANGE_DROPS.with(|c| c.set(c.get() + 1));
        return Vec::new();
    }
    crate::geom::split_with_holes(outer, holes)
}

/// The number of faces that `split_counted` refused since the last call; it starts again at zero.
pub(crate) fn take_range_drops() -> usize {
    RANGE_DROPS.with(|c| c.replace(0))
}

/// The coordinates of an `IfcCartesianPointList2D` or `3D`.
fn point_list(e: &crate::step::Entity) -> Vec<DVec3> {
    items(attr(e, 0))
        .iter()
        .filter_map(|row| {
            let n = numbers(row);
            (n.len() >= 2).then(|| DVec3::new(n[0], n[1], n.get(2).copied().unwrap_or(0.0)))
        })
        .collect()
}

/// Triangulates a planar polygon in 3D with optional holes. The first ring is the outline.
pub fn add_polygon(m: &mut MeshData, rings: &[Vec<DVec3>]) {
    let Some(outer) = rings.first() else { return };
    if outer.len() < 3 {
        return;
    }
    // Newell's normal.
    let mut n = DVec3::ZERO;
    for i in 0..outer.len() {
        let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
        n += DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let Some(n) = n.try_normalize() else { return };
    let basis = super::ctx::frame(n, None);
    let (u, w) = (basis.x_axis, basis.y_axis);
    let o = outer[0];
    let flat = |r: &[DVec3]| -> Vec<DVec2> {
        r.iter()
            .map(|p| DVec2::new((*p - o).dot(u), (*p - o).dot(w)))
            .collect()
    };
    if rings.len() == 1 {
        let pts = flat(outer);
        let base = m.v.len() as u32;
        m.v.extend_from_slice(outer);
        for [a, b, c] in kadakas_core::mesh::triangulate(&pts) {
            m.t.push([base + a as u32, base + b as u32, base + c as u32]);
        }
        return;
    }
    let outer2 = clean(&flat(outer), true);
    let holes: Vec<Vec<DVec2>> = rings[1..]
        .iter()
        .map(|r| clean(&flat(r), true))
        .filter(|r| r.len() >= 3)
        .collect();
    if outer2.len() < 3 {
        return;
    }
    for piece in split_counted(&outer2, &holes) {
        let base = m.v.len() as u32;
        m.v.extend(piece.iter().map(|p| o + u * p.x + w * p.y));
        for [a, b, c] in kadakas_core::mesh::triangulate(&piece) {
            m.t.push([base + a as u32, base + b as u32, base + c as u32]);
        }
    }
}

/// One mesh of all pieces, if every piece is solid geometry. `None` for an empty list.
pub fn solid_mesh(pieces: &[Piece]) -> Option<MeshData> {
    let mut out = MeshData::default();
    for p in pieces {
        out.append(&piece_mesh(p)?);
    }
    (!out.t.is_empty()).then_some(out)
}

/// The mesh of a piece, if it is solid geometry.
pub fn piece_mesh(p: &Piece) -> Option<MeshData> {
    match p {
        Piece::Mesh(m) => Some(m.clone()),
        Piece::Extrusion(e) => Some(e.mesh()),
        _ => None,
    }
}

/// Keeps the part of the mesh on the side of `normal` (where `(p - origin) . normal >= 0`) and closes
/// the cut with flat caps. A cut with nested loops (a hole in the section) is closed with its hole.
pub fn clip_mesh(m: &MeshData, origin: DVec3, normal: DVec3) -> MeshData {
    let Some(n) = normal.try_normalize() else {
        return m.clone();
    };
    let d = |p: DVec3| (p - origin).dot(n);
    let mut out = MeshData::default();
    let mut index: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let key = |p: DVec3| {
        (
            (p.x * 1e6).round() as i64,
            (p.y * 1e6).round() as i64,
            (p.z * 1e6).round() as i64,
        )
    };
    let mut vid = |out: &mut MeshData, p: DVec3| -> u32 {
        *index.entry(key(p)).or_insert_with(|| {
            out.v.push(p);
            (out.v.len() - 1) as u32
        })
    };
    let mut cut: Vec<(DVec3, DVec3)> = Vec::new();
    for t in &m.t {
        let p = t.map(|i| m.v[i as usize]);
        let ds = p.map(d);
        let eps = 1e-9;
        if ds.iter().all(|x| *x >= -eps) {
            let ids = p.map(|q| vid(&mut out, q));
            if ds.iter().all(|x| x.abs() <= eps) {
                continue;
            }
            out.t.push(ids);
            // An edge lying in the plane is part of the cut.
            for k in 0..3 {
                let (a, b) = (k, (k + 1) % 3);
                if ds[a].abs() <= eps && ds[b].abs() <= eps {
                    cut.push((p[a], p[b]));
                }
            }
            continue;
        }
        if ds.iter().all(|x| *x <= eps) {
            continue;
        }
        // The triangle crosses the plane: clip the polygon.
        let mut poly: Vec<DVec3> = Vec::new();
        for k in 0..3 {
            let (a, b) = (k, (k + 1) % 3);
            if ds[a] >= 0.0 {
                poly.push(p[a]);
            }
            if (ds[a] > 0.0 && ds[b] < 0.0) || (ds[a] < 0.0 && ds[b] > 0.0) {
                let s = ds[a] / (ds[a] - ds[b]);
                poly.push(p[a].lerp(p[b], s));
            }
        }
        if poly.len() < 3 {
            continue;
        }
        // Points of the new polygon that lie on the plane form the cut edge.
        let on: Vec<DVec3> = poly
            .iter()
            .copied()
            .filter(|q| d(*q).abs() <= 1e-7)
            .collect();
        if on.len() == 2 {
            cut.push((on[0], on[1]));
        }
        let ids: Vec<u32> = poly.iter().map(|q| vid(&mut out, *q)).collect();
        for k in 1..ids.len() - 1 {
            out.t.push([ids[0], ids[k], ids[k + 1]]);
        }
    }
    cap(&mut out, &cut, origin, n);
    out
}

/// Closes the cut of a clipped mesh. The caps face away from `keep`.
fn cap(out: &mut MeshData, cut: &[(DVec3, DVec3)], _origin: DVec3, keep: DVec3) {
    if cut.is_empty() {
        return;
    }
    let key = |p: DVec3| {
        (
            (p.x * 1e5).round() as i64,
            (p.y * 1e5).round() as i64,
            (p.z * 1e5).round() as i64,
        )
    };
    // Chain the segments into loops.
    let mut next: HashMap<(i64, i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (i, (a, b)) in cut.iter().enumerate() {
        next.entry(key(*a)).or_default().push((i, true));
        next.entry(key(*b)).or_default().push((i, false));
    }
    let mut used = vec![false; cut.len()];
    let mut loops: Vec<Vec<DVec3>> = Vec::new();
    for s in 0..cut.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let mut pts = vec![cut[s].0];
        let mut end = cut[s].1;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > cut.len() + 2 {
                break;
            }
            if key(end) == key(pts[0]) {
                break;
            }
            pts.push(end);
            let Some(cands) = next.get(&key(end)) else {
                break;
            };
            let Some(&(i, fwd)) = cands.iter().find(|(i, _)| !used[*i]) else {
                break;
            };
            used[i] = true;
            end = if fwd { cut[i].1 } else { cut[i].0 };
        }
        if pts.len() >= 3 && key(end) == key(pts[0]) {
            loops.push(pts);
        }
    }
    if loops.is_empty() {
        return;
    }
    // Plane basis: the cap faces -keep.
    let out_n = -keep;
    let basis = super::ctx::frame(out_n, None);
    let (u, w) = (basis.x_axis, basis.y_axis);
    let o = loops[0][0];
    let flat = |r: &[DVec3]| -> Vec<DVec2> {
        r.iter()
            .map(|p| DVec2::new((*p - o).dot(u), (*p - o).dot(w)))
            .collect()
    };
    let mut rings: Vec<(Vec<DVec2>, f64)> = loops
        .iter()
        .map(|l| {
            let f = flat(l);
            let a = crate::geom::signed_area(&f).abs();
            (f, a)
        })
        .collect();
    rings.sort_by(|a, b| b.1.total_cmp(&a.1));
    // A loop inside an earlier loop is a hole.
    let mut groups: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>)> = Vec::new();
    for (r, _) in rings {
        let c = clean(&r, true);
        if c.len() < 3 {
            continue;
        }
        let mut placed = false;
        for g in groups.iter_mut() {
            if inside(&g.0, c[0]) {
                g.1.push(c.clone());
                placed = true;
                break;
            }
        }
        if !placed {
            groups.push((c, Vec::new()));
        }
    }
    for (outer, holes) in groups {
        for piece in split_counted(&outer, &holes) {
            let base = out.v.len() as u32;
            out.v.extend(piece.iter().map(|p| o + u * p.x + w * p.y));
            for [a, b, c] in kadakas_core::mesh::triangulate(&piece) {
                out.t
                    .push([base + a as u32, base + b as u32, base + c as u32]);
            }
        }
    }
}

fn inside(poly: &[DVec2], p: DVec2) -> bool {
    kadakas_core::geom::point_in_polygon(p, poly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{Limits, StepIndex};

    fn index(data: &str) -> StepIndex {
        let text = format!("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{data}\nENDSEC;\n");
        StepIndex::from_bytes(text.into_bytes(), Limits::default()).unwrap()
    }

    fn volume(m: &MeshData) -> f64 {
        m.t.iter()
            .map(|t| {
                let (a, b, c) = (m.v[t[0] as usize], m.v[t[1] as usize], m.v[t[2] as usize]);
                a.dot(b.cross(c))
            })
            .sum::<f64>()
            / 6.0
    }

    fn run(data: &str, id: u64) -> (Vec<Piece>, Vec<String>) {
        let x = index(data);
        let cx = Ctx::new(&x);
        let mut ev = Eval::new(&cx, 1_000_000);
        ev.item(id, &DAffine3::IDENTITY, 0);
        (ev.pieces, ev.issues)
    }

    const BASE: &str = "#1=IFCCARTESIANPOINT((0.,0.,0.));\n#2=IFCAXIS2PLACEMENT3D(#1,$,$);\n#3=IFCDIRECTION((0.,0.,1.));\n";

    #[test]
    fn rectangle_extrusion_is_a_closed_solid_of_the_right_volume() {
        let (p, issues) = run(
            &format!(
                "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,4.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,3.);"
            ),
            5,
        );
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert!(ex.is_upright());
        assert!((ex.height() - 3.0).abs() < 1e-12);
        let m = ex.mesh();
        assert!((volume(&m) - 24.0).abs() < 1e-9, "{}", volume(&m));
    }

    #[test]
    fn circle_and_voids_and_downward_extrusions() {
        let (p, _) = run(
            &format!(
                "{BASE}#4=IFCCIRCLEPROFILEDEF(.AREA.,$,$,1.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,2.);"
            ),
            5,
        );
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        // The circle is exact in the profile (four points on the arc) and flat in the mesh.
        assert!(
            ex.regions[0]
                .outer
                .iter()
                .all(|v| v.kind == VertexKind::PointOnArc)
        );
        let v = volume(&ex.mesh());
        assert!((v - PI * 2.0).abs() < 0.05 * PI * 2.0, "{v}");
        // A profile with a square void.
        let (p, _) = run(
            &format!(
                "{BASE}#10=IFCCARTESIANPOINT((0.,0.));\n#11=IFCCARTESIANPOINT((10.,0.));\n#12=IFCCARTESIANPOINT((10.,10.));\n#13=IFCCARTESIANPOINT((0.,10.));\n#14=IFCPOLYLINE((#10,#11,#12,#13,#10));\n#15=IFCCARTESIANPOINT((4.,4.));\n#16=IFCCARTESIANPOINT((6.,4.));\n#17=IFCCARTESIANPOINT((6.,6.));\n#18=IFCCARTESIANPOINT((4.,6.));\n#19=IFCPOLYLINE((#15,#16,#17,#18,#15));\n#20=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#14,(#19));\n#21=IFCEXTRUDEDAREASOLID(#20,#2,#3,1.);\n#22=IFCDIRECTION((0.,0.,-1.));\n#23=IFCEXTRUDEDAREASOLID(#20,#2,#22,1.);"
            ),
            21,
        );
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert_eq!(ex.regions[0].holes.len(), 1);
        assert!(
            (volume(&ex.mesh()) - 96.0).abs() < 1e-6,
            "{}",
            volume(&ex.mesh())
        );
        let (p, _) = run(
            &format!(
                "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,1.);\n#22=IFCDIRECTION((0.,0.,-1.));\n#23=IFCEXTRUDEDAREASOLID(#4,#2,#22,2.);"
            ),
            23,
        );
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert!(ex.is_upright());
        // A solid that goes down has positive volume too, and lies below.
        assert!((volume(&ex.mesh()) - 2.0).abs() < 1e-9);
        assert!(ex.mesh().bounds().unwrap().1.z < 1e-9);
    }

    #[test]
    fn composite_curve_with_arc_stays_exact() {
        // A half disc: a line along the x axis and a semicircle above it.
        let data = format!(
            "{BASE}#10=IFCCARTESIANPOINT((-1.,0.));\n#11=IFCCARTESIANPOINT((1.,0.));\n#12=IFCPOLYLINE((#10,#11));\n#13=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#12);\n#14=IFCCARTESIANPOINT((0.,0.));\n#15=IFCAXIS2PLACEMENT2D(#14,$);\n#16=IFCCIRCLE(#15,1.);\n#17=IFCTRIMMEDCURVE(#16,(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(180.)),.T.,.PARAMETER.);\n#18=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#17);\n#19=IFCCOMPOSITECURVE((#13,#18),.F.);\n#20=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#19);\n#21=IFCEXTRUDEDAREASOLID(#20,#2,#3,1.);\n#30=IFCPROJECT('g',$,'p',$,$,$,$,$,#31);\n#31=IFCUNITASSIGNMENT((#32,#33));\n#32=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#33=IFCCONVERSIONBASEDUNIT($,.PLANEANGLEUNIT.,'DEGREE',#34);\n#34=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.0174532925199433),#35);\n#35=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);"
        );
        let (p, issues) = run(&data, 21);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        let kinds: Vec<_> = ex.regions[0].outer.iter().map(|v| v.kind).collect();
        assert!(kinds.contains(&VertexKind::PointOnArc), "{kinds:?}");
        // Half disc area: pi/2 (the flattened mesh is slightly smaller).
        let v = volume(&ex.mesh());
        assert!((v - PI / 2.0).abs() < 0.01, "{v}");
    }

    #[test]
    fn indexed_poly_curve_with_an_arc_index() {
        let data = format!(
            "{BASE}#10=IFCCARTESIANPOINTLIST2D(((0.,0.),(2.,0.),(1.,1.),(0.,2.)));\n#11=IFCINDEXEDPOLYCURVE(#10,(IFCLINEINDEX((1,2)),IFCARCINDEX((2,3,4)),IFCLINEINDEX((4,1))),.F.);\n#12=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#11);\n#13=IFCEXTRUDEDAREASOLID(#12,#2,#3,1.);"
        );
        let (p, issues) = run(&data, 13);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        assert_eq!(ex.regions[0].outer.len(), 4);
        assert_eq!(ex.regions[0].outer[2].kind, VertexKind::PointOnArc);
    }

    #[test]
    fn face_sets_and_breps() {
        // A tetrahedron as a triangulated face set.
        let tet = "#10=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(6.,0.,0.),(0.,6.,0.),(0.,0.,6.)));\n#11=IFCTRIANGULATEDFACESET(#10,$,.T.,((1,3,2),(1,2,4),(2,3,4),(1,4,3)),$);";
        let (p, issues) = run(tet, 11);
        assert!(issues.is_empty());
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 36.0).abs() < 1e-9);
        // The same as a polygonal face set with one quad face and a hole-free triangle.
        let cube = "#10=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1.,0.,0.),(1.,1.,0.),(0.,1.,0.),(0.,0.,1.),(1.,0.,1.),(1.,1.,1.),(0.,1.,1.)));\n#11=IFCINDEXEDPOLYGONALFACE((1,4,3,2));\n#12=IFCINDEXEDPOLYGONALFACE((5,6,7,8));\n#13=IFCINDEXEDPOLYGONALFACE((1,2,6,5));\n#14=IFCINDEXEDPOLYGONALFACE((2,3,7,6));\n#15=IFCINDEXEDPOLYGONALFACE((3,4,8,7));\n#16=IFCINDEXEDPOLYGONALFACE((4,1,5,8));\n#17=IFCPOLYGONALFACESET(#10,.T.,(#11,#12,#13,#14,#15,#16),$);";
        let (p, issues) = run(cube, 17);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert_eq!(m.v.len(), 8);
        assert!((volume(m) - 1.0).abs() < 1e-9);
        // A faceted brep of the same cube (one face has a hole-free loop).
        let mut brep = String::new();
        let pts = [
            (0, 0, 0),
            (1, 0, 0),
            (1, 1, 0),
            (0, 1, 0),
            (0, 0, 1),
            (1, 0, 1),
            (1, 1, 1),
            (0, 1, 1),
        ];
        for (i, (x, y, z)) in pts.iter().enumerate() {
            brep += &format!("#{}=IFCCARTESIANPOINT(({x}.,{y}.,{z}.));\n", 100 + i);
        }
        let faces = [
            [1, 4, 3, 2],
            [5, 6, 7, 8],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 4, 8, 7],
            [4, 1, 5, 8],
        ];
        for (k, f) in faces.iter().enumerate() {
            let refs: Vec<String> = f.iter().map(|i| format!("#{}", 99 + i)).collect();
            brep += &format!(
                "#{}=IFCPOLYLOOP(({}));\n#{}=IFCFACEOUTERBOUND(#{},.T.);\n#{}=IFCFACE((#{}));\n",
                200 + k,
                refs.join(","),
                300 + k,
                200 + k,
                400 + k,
                300 + k
            );
        }
        brep += "#500=IFCCLOSEDSHELL((#400,#401,#402,#403,#404,#405));\n#501=IFCFACETEDBREP(#500);\n#502=IFCSHELLBASEDSURFACEMODEL((#500));";
        let (p, issues) = run(&brep, 501);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 1.0).abs() < 1e-9);
        let (p, _) = run(&brep, 502);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 1.0).abs() < 1e-9);
        // Reversed bound orientation turns the face around.
        // (The top face carries a third of the signed volume; the bottom face none.)
        let flipped = brep.replace(
            "#301=IFCFACEOUTERBOUND(#201,.T.)",
            "#301=IFCFACEOUTERBOUND(#201,.F.)",
        );
        let (p, _) = run(&flipped, 501);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 1.0 / 3.0).abs() < 1e-9, "{}", volume(m));
    }

    #[test]
    fn polygon_with_a_hole_in_3d() {
        let mut m = MeshData::default();
        let sq = |a: f64, z: f64| {
            vec![
                DVec3::new(-a, -a, z),
                DVec3::new(a, -a, z),
                DVec3::new(a, a, z),
                DVec3::new(-a, a, z),
            ]
        };
        let mut hole = sq(1.0, 0.0);
        hole.reverse();
        add_polygon(&mut m, &[sq(3.0, 0.0), hole]);
        let area: f64 =
            m.t.iter()
                .map(|t| {
                    let (a, b, c) = (m.v[t[0] as usize], m.v[t[1] as usize], m.v[t[2] as usize]);
                    (b - a).cross(c - a).length() / 2.0
                })
                .sum();
        assert!((area - 32.0).abs() < 1e-6, "{area}");
        // Degenerate input does nothing.
        let mut e = MeshData::default();
        add_polygon(&mut e, &[vec![DVec3::ZERO, DVec3::X]]);
        add_polygon(&mut e, &[vec![DVec3::ZERO, DVec3::X, DVec3::X * 2.0]]);
        add_polygon(&mut e, &[]);
        assert!(e.t.is_empty());
    }

    #[test]
    fn clipping_cuts_and_caps_a_box() {
        let cube = MeshData::cuboid(DVec3::ZERO, DVec3::new(2.0, 2.0, 2.0));
        assert!((volume(&cube) - 8.0).abs() < 1e-9);
        let c = clip_mesh(&cube, DVec3::new(0.0, 0.0, 1.0), -DVec3::Z);
        assert!((volume(&c) - 4.0).abs() < 1e-9, "{}", volume(&c));
        // A sloped cut.
        let c = clip_mesh(
            &cube,
            DVec3::new(1.0, 1.0, 1.0),
            DVec3::new(-1.0, 0.0, -1.0),
        );
        let v = volume(&c);
        assert!(v > 1.0 && v < 7.0, "{v}");
        // Nothing to remove, everything removed.
        assert!(
            (volume(&clip_mesh(&cube, DVec3::new(0.0, 0.0, 5.0), -DVec3::Z)) - 8.0).abs() < 1e-9
        );
        assert!(
            clip_mesh(&cube, DVec3::new(0.0, 0.0, -5.0), -DVec3::Z)
                .t
                .is_empty()
        );
        assert_eq!(clip_mesh(&cube, DVec3::ZERO, DVec3::ZERO), cube);
    }

    #[test]
    fn boolean_clipping_result_with_a_half_space() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,2.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,4.);\n#6=IFCCARTESIANPOINT((0.,0.,3.));\n#7=IFCAXIS2PLACEMENT3D(#6,$,$);\n#8=IFCPLANE(#7);\n#9=IFCHALFSPACESOLID(#8,.F.);\n#10=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#5,#9);\n#11=IFCHALFSPACESOLID(#8,.T.);\n#12=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#5,#11);\n#13=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#10,#11);"
        );
        // Flag FALSE: the half space is above the plane, so the part above is removed. 2x2x3 remains.
        let (p, issues) = run(&data, 10);
        assert!(issues.is_empty(), "{issues:?}");
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 12.0).abs() < 1e-6, "{}", volume(m));
        // Flag TRUE: the half space is below the plane: 2x2x1 remains.
        let (p, _) = run(&data, 12);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 4.0).abs() < 1e-6);
        // Nested clipping removes everything.
        let (p, _) = run(&data, 13);
        assert!(p.is_empty());
    }

    #[test]
    fn booleans_with_a_solid_second_operand_and_a_bounded_half_space() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,2.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,4.);\n\
#14=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,1.);\n#15=IFCEXTRUDEDAREASOLID(#14,#2,#3,4.);\n\
#16=IFCBOOLEANRESULT(.DIFFERENCE.,#5,#15);\n#17=IFCBOOLEANRESULT(.INTERSECTION.,#5,#15);\n\
#18=IFCCARTESIANPOINT((10.,0.,0.));\n#19=IFCAXIS2PLACEMENT3D(#18,$,$);\n#20=IFCEXTRUDEDAREASOLID(#14,#19,#3,4.);\n\
#21=IFCBOOLEANRESULT(.UNION.,#5,#20);\n#22=IFCBOOLEANRESULT(.DIFFERENCE.,#16,#20);\n\
#30=IFCCARTESIANPOINT((0.,0.,2.));\n#31=IFCAXIS2PLACEMENT3D(#30,$,$);\n#32=IFCPLANE(#31);\n\
#33=IFCCARTESIANPOINT((-0.5,-0.5));\n#34=IFCCARTESIANPOINT((0.5,-0.5));\n#35=IFCCARTESIANPOINT((0.5,0.5));\n#36=IFCCARTESIANPOINT((-0.5,0.5));\n\
#37=IFCPOLYLINE((#33,#34,#35,#36,#33));\n#38=IFCPOLYGONALBOUNDEDHALFSPACE(#32,.F.,#2,#37);\n#39=IFCBOOLEANCLIPPINGRESULT(.DIFFERENCE.,#5,#38);"
        );
        let vol = |id: u64| {
            let (p, issues) = run(&data, id);
            assert!(issues.is_empty(), "{issues:?}");
            p.iter()
                .map(|p| volume(&piece_mesh(p).unwrap()))
                .sum::<f64>()
        };
        assert!((vol(16) - 12.0).abs() < 1e-6);
        assert!((vol(17) - 4.0).abs() < 1e-6);
        assert!((vol(21) - 20.0).abs() < 1e-6);
        // A second cutter that misses the solid changes nothing.
        assert!((vol(22) - 12.0).abs() < 1e-6);
        // The flag FALSE: the part above the plane z = 2 inside the polygon goes: 1 x 1 x 2.
        assert!((vol(39) - 14.0).abs() < 1e-6);
    }

    #[test]
    fn a_boolean_with_an_open_operand_keeps_the_first_one_and_says_so() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,2.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,4.);\n\
#40=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));\n#41=IFCTRIANGULATEDFACESET(#40,$,.F.,((1,2,3)),$);\n\
#42=IFCBOOLEANRESULT(.DIFFERENCE.,#5,#41);"
        );
        let (p, issues) = run(&data, 42);
        assert!(
            issues.iter().any(|i| i.contains("not applied")),
            "{issues:?}"
        );
        assert!(
            (p.iter()
                .map(|p| volume(&piece_mesh(p).unwrap()))
                .sum::<f64>()
                - 16.0)
                .abs()
                < 1e-6
                || p.len() == 2
        );
    }

    #[test]
    fn the_fillets_of_an_i_shape_add_their_area() {
        // IPE 600: 220 wide, 600 deep, web 12, flange 19, fillet 24 (the file is in millimeters here).
        let data = format!(
            "{BASE}#4=IFCISHAPEPROFILEDEF(.AREA.,$,$,220.,600.,12.,19.,24.,$,$);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,1000.);"
        );
        let (p, issues) = run(&data, 5);
        assert!(issues.is_empty(), "{issues:?}");
        let area = 2.0 * 220.0 * 19.0 + (600.0 - 38.0) * 12.0 + (4.0 - PI) * 24.0 * 24.0;
        let v = volume(&piece_mesh(&p[0]).unwrap());
        assert!(
            (v / 1000.0 - area).abs() < area * 0.003,
            "{} {area}",
            v / 1000.0
        );
    }

    #[test]
    fn the_exact_surfaces_of_a_read_count_in_its_size() {
        let plain = Rc::new((Some(MeshData::default()), vec![]));
        let m = MeshData {
            exact: Some(Box::new(kadakas_core::exact::ExactSolid {
                version: 1,
                product: String::new(),
                unit_mm: 1.0,
                tolerance_mm: 0.1,
                angle_deg: 15.0,
                vertices: vec![DVec3::ZERO; 1000],
                edges: vec![],
                shells: vec![],
                faces: vec![],
                mesh_hash: 0,
                extra: Default::default(),
            })),
            ..Default::default()
        };
        let with = Rc::new((Some(m), vec![]));
        assert!(brep_read_bytes(&with) >= brep_read_bytes(&plain) + 50_000);
    }

    #[test]
    fn the_cache_of_advanced_breps_has_a_byte_limit_and_drops_the_least_recently_used() {
        let mesh = |n: usize| {
            Rc::new((
                Some(MeshData {
                    v: vec![DVec3::ZERO; n],
                    ..Default::default()
                }),
                vec![],
            ))
        };
        // One read takes 100 * 24 + 64 = 2,464 bytes: the limit holds three of them.
        let c = BrepCache::with_limit(3 * 2_464 + 10);
        for id in 1..=3 {
            c.insert(id, mesh(100));
        }
        assert_eq!(c.held().1, 3);
        // Item 1 is used, so item 2 is the least recently used one and goes.
        assert!(c.get(1).is_some());
        c.insert(4, mesh(100));
        assert_eq!(c.held().1, 3);
        assert!(
            c.get(2).is_none() && c.get(1).is_some() && c.get(3).is_some() && c.get(4).is_some()
        );
        assert!(c.held().0 <= 3 * 2_464 + 10);
        // A read that is over the limit alone is not kept, and nothing else goes for it.
        c.insert(5, mesh(10_000));
        assert!(c.get(5).is_none());
        assert_eq!(c.held().1, 3);
        // The same item again replaces the old read (the bytes do not add up).
        c.insert(4, mesh(100));
        assert_eq!(c.held().1, 3);
        assert_eq!(c.held().0, 3 * 2_464);
    }

    #[test]
    fn extrusions_of_profiles_with_voids_are_closed_solids() {
        let hollow = |profile: &str| {
            let data = format!("{BASE}#4={profile};\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,3.);");
            let (p, issues) = run(&data, 5);
            assert!(issues.is_empty(), "{issues:?}");
            piece_mesh(&p[0]).unwrap()
        };
        for profile in [
            "IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,$,5.,0.2)",
            "IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,$,4.,2.,0.2)",
        ] {
            let m = hollow(profile);
            assert!(csg::is_closed(&m), "{profile} is open");
            assert!(volume(&m) > 0.0, "{profile}");
        }
        // The rectangle is exact.
        let m = hollow("IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,$,4.,2.,0.2)");
        assert!((volume(&m) - (8.0 - 3.6 * 1.6) * 3.0).abs() < 1e-9);
        // A profile with two voids.
        let two = format!(
            "{BASE}#10=IFCCARTESIANPOINT((-4.,-2.));#11=IFCCARTESIANPOINT((4.,-2.));#12=IFCCARTESIANPOINT((4.,2.));#13=IFCCARTESIANPOINT((-4.,2.));\n#14=IFCPOLYLINE((#10,#11,#12,#13,#10));\n\
#20=IFCCARTESIANPOINT((-3.,-1.));#21=IFCCARTESIANPOINT((-1.,-1.));#22=IFCCARTESIANPOINT((-1.,1.));#23=IFCCARTESIANPOINT((-3.,1.));\n#24=IFCPOLYLINE((#20,#21,#22,#23,#20));\n\
#30=IFCCARTESIANPOINT((1.,-1.5));#31=IFCCARTESIANPOINT((3.,-1.5));#32=IFCCARTESIANPOINT((2.,1.5));\n#34=IFCPOLYLINE((#30,#31,#32,#30));\n\
#40=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#14,(#24,#34));\n#41=IFCEXTRUDEDAREASOLID(#40,#2,#3,2.);"
        );
        let (p, _) = run(&two, 41);
        let m = piece_mesh(&p[0]).unwrap();
        assert!(csg::is_closed(&m));
        assert!(
            (volume(&m) - (32.0 - 4.0 - 3.0) * 2.0).abs() < 1e-9,
            "{}",
            volume(&m)
        );
    }

    #[test]
    fn unsupported_operands_and_items_are_reported() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,2.,2.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,4.);\n#6=IFCPOLYGONALBOUNDEDHALFSPACE($,$,$,$);\n#7=IFCBOOLEANRESULT(.DIFFERENCE.,#5,#6);\n#8=IFCBOOLEANRESULT(.INTERSECTION.,#5,#4);\n#9=IFCADVANCEDBREP($);\n#10=IFCISHAPEPROFILEDEF(.AREA.,$,$,2.,4.,0.5,0.5,0.);\n#11=IFCEXTRUDEDAREASOLID(#10,#2,#3,4.);\n#12=IFCCSHAPEPROFILEDEF(.AREA.,$,$,2.);\n#13=IFCEXTRUDEDAREASOLID(#12,#2,#3,4.);"
        );
        let (p, issues) = run(&data, 7);
        assert_eq!(p.len(), 1);
        assert!(issues[0].contains("bounded half space"), "{issues:?}");
        let (_, issues) = run(&data, 8);
        assert!(issues[0].contains("IFCRECTANGLEPROFILEDEF"), "{issues:?}");
        let (_, issues) = run(&data, 9);
        assert!(
            issues[0].starts_with("unsupported geometry IFCADVANCEDBREP"),
            "{issues:?}"
        );
        let (p, _) = run(&data, 11);
        assert!(
            (volume(&piece_mesh(&p[0]).unwrap()) - (2.0 * 0.5 * 2.0 + 3.0 * 0.5) * 4.0 / 1.0 * 1.0)
                .abs()
                < 2.1
        );
        let (p, issues) = run(&data, 13);
        assert!(p.is_empty());
        assert!(issues.iter().any(|i| i.contains("IFCCSHAPEPROFILEDEF")));
        let (_, issues) = run(&data, 99);
        assert!(issues[0].contains("missing"));
    }

    #[test]
    fn mapped_items_expand_or_become_instances() {
        let data = format!(
            "{BASE}#4=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,1.);\n#5=IFCEXTRUDEDAREASOLID(#4,#2,#3,1.);\n#6=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#5));\n#7=IFCREPRESENTATIONMAP(#2,#6);\n#8=IFCCARTESIANPOINT((10.,0.,0.));\n#9=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#8,2.,$);\n#10=IFCMAPPEDITEM(#7,#9);"
        );
        let (p, _) = run(&data, 10);
        let Piece::Extrusion(ex) = &p[0] else {
            panic!()
        };
        // Scale 2 at x = 10: a 2 x 2 x 2 cube.
        assert!((volume(&ex.mesh()) - 8.0).abs() < 1e-9);
        let x = index(&data);
        let cx = Ctx::new(&x);
        let mut ev = Eval::new(&cx, 1_000_000);
        let yes = |id: u64| id == 7;
        ev.instancing = Some(&yes);
        ev.item(10, &DAffine3::IDENTITY, 0);
        let Piece::Instance { source, xf } = &ev.pieces[0] else {
            panic!()
        };
        assert_eq!(*source, 7);
        assert!((xf.transform_point3(DVec3::ZERO) - DVec3::new(10.0, 0.0, 0.0)).length() < 1e-9);
    }

    #[test]
    fn bounding_boxes_and_curves() {
        let data = format!(
            "{BASE}#4=IFCBOUNDINGBOX(#1,2.,3.,4.);\n#5=IFCBOUNDINGBOX(#1,0.,3.,4.);\n#6=IFCCARTESIANPOINT((1.,1.));\n#7=IFCCARTESIANPOINT((4.,1.));\n#8=IFCPOLYLINE((#6,#7));\n#9=IFCGEOMETRICCURVESET((#8,#1));"
        );
        let (p, _) = run(&data, 4);
        let Piece::Mesh(m) = &p[0] else { panic!() };
        assert!((volume(m) - 24.0).abs() < 1e-9);
        assert!(run(&data, 5).0.is_empty());
        let (p, _) = run(&data, 9);
        assert_eq!(p.len(), 2);
        let Piece::Curve(c) = &p[0] else { panic!() };
        assert_eq!(c.pts.len(), 2);
        assert!(c.exact.is_some() && !c.closed);
        assert!(matches!(p[1], Piece::Point(_)));
    }

    #[test]
    fn mesh_helpers() {
        let mut m = MeshData::cuboid(DVec3::ZERO, DVec3::ONE);
        m.transform(&DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        // A mirror keeps the solid right side out.
        assert!((volume(&m) - 1.0).abs() < 1e-9);
        let mut w = MeshData::cuboid(DVec3::ZERO, DVec3::ONE);
        w.append(&MeshData::cuboid(DVec3::ZERO, DVec3::ONE));
        assert_eq!(w.v.len(), 16);
        w.weld(1e-6);
        assert_eq!(w.v.len(), 8);
        assert!(MeshData::default().bounds().is_none());
    }
}
