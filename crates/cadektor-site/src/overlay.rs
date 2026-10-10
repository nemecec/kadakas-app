//! An overlay of a TIN: the existing triangles are kept as they are, and rings are cut into them.
//!
//! All edges of the existing triangles are constraints, so the existing triangulation stays. A ring
//! is added as constraints that split the edges they cross, so every resulting triangle lies on
//! one side of every ring. The caller decides, for each triangle, which region it belongs to.

use std::collections::{HashMap, HashSet};

use glam::{DVec2, DVec3};
use spade::handles::{FixedDirectedEdgeHandle, FixedVertexHandle};
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};

use crate::SiteError;
use crate::tin::Tin;

/// Marks a vertex that is not a vertex of the existing TIN.
pub(crate) const NONE: u32 = u32::MAX;

/// A vertex of the overlay. `orig` is the index of the same vertex in the existing TIN.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OV {
    pub pos: Point2<f64>,
    pub z: f64,
    pub orig: u32,
}

impl HasPosition for OV {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.pos
    }
}

/// A corner of an overlay triangle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Corner {
    pub xy: DVec2,
    pub z: f64,
    pub orig: u32,
}

pub(crate) struct Overlay {
    cdt: ConstrainedDelaunayTriangulation<OV>,
    seen: HashMap<(u64, u64), FixedVertexHandle>,
}

fn key(p: DVec2) -> (u64, u64) {
    ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits())
}

impl Overlay {
    pub fn new() -> Self {
        Overlay {
            cdt: ConstrainedDelaunayTriangulation::new(),
            seen: HashMap::new(),
        }
    }

    /// The overlay of the given triangles of `tin`.
    pub fn of_triangles(
        tin: &Tin,
        triangles: impl Iterator<Item = usize>,
    ) -> Result<Self, SiteError> {
        let mut o = Overlay::new();
        let mut edges: HashSet<(usize, usize)> = HashSet::new();
        for t in triangles {
            let tri = tin.triangles[t];
            let mut h = [None; 3];
            for k in 0..3 {
                let v = tri[k];
                h[k] = Some(o.point(
                    tin.vertices[v as usize].truncate(),
                    tin.vertices[v as usize].z,
                    v,
                )?);
            }
            let h = h.map(Option::unwrap);
            for k in 0..3 {
                let (a, b) = (h[k], h[(k + 1) % 3]);
                if edges.insert((a.index().min(b.index()), a.index().max(b.index()))) {
                    o.edge(a, b);
                }
            }
        }
        Ok(o)
    }

    /// Adds a point. A second point at the same place gives the first one back.
    pub fn point(&mut self, p: DVec2, z: f64, orig: u32) -> Result<FixedVertexHandle, SiteError> {
        if !p.is_finite() || !z.is_finite() {
            return Err(SiteError::NotFinite);
        }
        if let Some(h) = self.seen.get(&key(p)) {
            return Ok(*h);
        }
        let h = self
            .cdt
            .insert(OV {
                pos: Point2::new(p.x, p.y),
                z,
                orig,
            })
            .map_err(|_| SiteError::NotFinite)?;
        self.seen.insert(key(p), h);
        Ok(h)
    }

    /// Keeps the segment between two points, if it crosses no other kept segment.
    pub fn edge(&mut self, a: FixedVertexHandle, b: FixedVertexHandle) -> bool {
        if a == b || !self.cdt.can_add_constraint(a, b) {
            return false;
        }
        self.cdt.add_constraint(a, b);
        true
    }

    /// Adds a closed ring. Where it crosses a kept segment, a point is added with the height `z`
    /// gives. Returns the points along the ring, in order, with their heights.
    pub fn ring(
        &mut self,
        ring: &[DVec2],
        z: &dyn Fn(DVec2) -> f64,
    ) -> Result<Vec<(DVec2, f64)>, SiteError> {
        let n = ring.len();
        let handles = ring
            .iter()
            .map(|p| self.point(*p, z(*p), NONE))
            .collect::<Result<Vec<_>, _>>()?;
        let mut chain: Vec<(DVec2, f64)> = Vec::new();
        for i in 0..n {
            let (a, b) = (handles[i], handles[(i + 1) % n]);
            if a == b {
                continue;
            }
            let edges = self.cdt.add_constraint_and_split(a, b, |p| OV {
                pos: p,
                z: z(DVec2::new(p.x, p.y)),
                orig: NONE,
            });
            self.push_chain(&edges, &mut chain);
        }
        Ok(chain)
    }

    /// Adds a segment like `ring` does and returns its points.
    pub fn segment(
        &mut self,
        a: DVec2,
        b: DVec2,
        z: &dyn Fn(DVec2) -> f64,
    ) -> Result<Vec<(DVec2, f64)>, SiteError> {
        let (ha, hb) = (self.point(a, z(a), NONE)?, self.point(b, z(b), NONE)?);
        let mut chain = Vec::new();
        if ha != hb {
            let edges = self.cdt.add_constraint_and_split(ha, hb, |p| OV {
                pos: p,
                z: z(DVec2::new(p.x, p.y)),
                orig: NONE,
            });
            self.push_chain(&edges, &mut chain);
        }
        Ok(chain)
    }

    fn push_chain(&self, edges: &[FixedDirectedEdgeHandle], chain: &mut Vec<(DVec2, f64)>) {
        for e in edges {
            let e = self.cdt.directed_edge(*e);
            let (f, t) = (e.from(), e.to());
            let (pf, pt) = (f.position(), t.position());
            if chain
                .last()
                .is_none_or(|(p, _)| *p != DVec2::new(pf.x, pf.y))
            {
                chain.push((DVec2::new(pf.x, pf.y), f.data().z));
            }
            chain.push((DVec2::new(pt.x, pt.y), t.data().z));
        }
    }

    /// The triangles of the overlay.
    pub fn faces(&self) -> Vec<[Corner; 3]> {
        self.cdt
            .inner_faces()
            .map(|f| {
                f.vertices().map(|v| {
                    let d = v.data();
                    Corner {
                        xy: DVec2::new(d.pos.x, d.pos.y),
                        z: d.z,
                        orig: d.orig,
                    }
                })
            })
            .collect()
    }
}

/// Collects the vertices and triangles of a new TIN. A vertex that is a vertex of the existing
/// TIN, with the same height, keeps its index. Vertices with the same place and height are one.
pub(crate) struct Out<'a> {
    existing: &'a Tin,
    pub vertices: Vec<DVec3>,
    map: HashMap<[u64; 3], u32>,
    pub triangles: Vec<[u32; 3]>,
}

impl<'a> Out<'a> {
    pub fn new(existing: &'a Tin) -> Self {
        Out {
            existing,
            vertices: existing.vertices.clone(),
            map: HashMap::new(),
            triangles: Vec::new(),
        }
    }

    pub fn vertex(&mut self, xy: DVec2, z: f64, orig: u32) -> u32 {
        if orig != NONE && self.existing.vertices[orig as usize].z.to_bits() == z.to_bits() {
            return orig;
        }
        let k = [
            (xy.x + 0.0).to_bits(),
            (xy.y + 0.0).to_bits(),
            (z + 0.0).to_bits(),
        ];
        let vertices = &mut self.vertices;
        *self.map.entry(k).or_insert_with(|| {
            vertices.push(xy.extend(z));
            (vertices.len() - 1) as u32
        })
    }

    /// Adds a triangle. `z` gives the height at each corner from its place and overlay height.
    pub fn triangle(&mut self, corners: &[Corner; 3], z: impl Fn(DVec2, f64) -> f64) {
        let t = corners.map(|c| self.vertex(c.xy, z(c.xy, c.z), c.orig));
        self.triangles.push(t);
    }

    /// Makes the TIN. Vertices that no triangle uses are left out.
    pub fn finish(self) -> Result<Tin, SiteError> {
        let mut new_index = vec![u32::MAX; self.vertices.len()];
        let mut vertices = Vec::new();
        let triangles: Vec<[u32; 3]> = self
            .triangles
            .iter()
            .map(|t| {
                t.map(|v| {
                    if new_index[v as usize] == u32::MAX {
                        new_index[v as usize] = vertices.len() as u32;
                        vertices.push(self.vertices[v as usize]);
                    }
                    new_index[v as usize]
                })
            })
            .collect();
        Tin::from_parts(vertices, triangles)
    }
}

/// Cuts a TIN to a ring: only the part inside the ring stays. The ring does not need to be convex.
pub fn crop(tin: &Tin, ring: &[DVec2]) -> Result<Tin, SiteError> {
    if ring.len() < 3 {
        return Ok(tin.clone());
    }
    let mut o = Overlay::of_triangles(tin, 0..tin.triangles.len())?;
    o.ring(ring, &|p| tin.elevation_at(p).unwrap_or(0.0))?;
    let mut out = Out::new(tin);
    for f in o.faces() {
        let c = (f[0].xy + f[1].xy + f[2].xy) / 3.0;
        if tin.locate(c).is_some() && crate::modifier::inside_or_on(ring, c) {
            out.triangle(&f, |_, z| z);
        }
    }
    out.finish()
}
