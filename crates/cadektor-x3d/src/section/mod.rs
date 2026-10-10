//! Sections and elevations: the cut of the 3D scene by a vertical plane.
//!
//! The plane runs along a section line. The view looks to one side of it. The drawing is in
//! section space: `x` is the distance along the line, counted so that it grows to the right in
//! the view, and `y` is the height above the model zero, both in mm. A cut gives:
//!
//! - faces: rings where the plane cuts a solid (walls, slabs, massing, trunks). The caller
//!   fills them with the hatch of the class.
//! - profiles: open lines where the plane cuts a surface (the terrain, flat areas).
//! - silhouettes: the cut through a plant, drawn as an outline.
//! - edges: the visible lines of the objects beyond the plane, with hidden lines removed.
//!
//! An elevation (mode `elevation`) cuts nothing. It draws the edges of everything between the
//! line and the far clip.

pub mod beyond;
pub mod cut;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use glam::{DVec2, DVec3, Vec3};
use serde::{Deserialize, Serialize};

use cadektor_core::mesh::Scene;
use cadektor_core::model::{Document, Object, ObjectId, Shape};
use cadektor_core::section::{self as settings, Look, Mode};

use beyond::{Region, Seg, Tri};

/// A section: the plane and what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionSpec {
    pub a: DVec2,
    pub b: DVec2,
    pub look: Look,
    pub mode: Mode,
    /// Far clip in mm, or `None`.
    pub depth: Option<f64>,
    /// Draw the objects beyond the cut (cut mode).
    pub beyond: bool,
}

impl SectionSpec {
    /// The spec of a section line object.
    pub fn from_line(obj: &Object) -> Option<SectionSpec> {
        let (a, b) = settings::ends(obj)?;
        let s = settings::data(obj)?;
        Some(SectionSpec {
            a,
            b,
            look: s.look,
            mode: s.mode,
            depth: s.depth,
            beyond: s.beyond,
        })
    }

    /// Length of the line in mm.
    pub fn length(&self) -> f64 {
        self.a.distance(self.b)
    }

    /// Why the spec cannot be drawn, or `None`.
    pub fn problem(&self) -> Option<String> {
        if !(self.a.is_finite() && self.b.is_finite()) {
            return Some("the section line needs finite points".into());
        }
        if self.length() < 1.0 {
            return Some("the section line is too short (less than 1 mm)".into());
        }
        if let Some(d) = self.depth
            && !(d.is_finite() && d > 0.0)
        {
            return Some("depth must be more than 0".into());
        }
        None
    }
}

/// The cut through a solid: rings, outer rings counterclockwise and holes clockwise.
#[derive(Clone, Debug, PartialEq)]
pub struct CutFace {
    pub object: ObjectId,
    pub rings: Vec<Vec<DVec2>>,
}

/// An open line where the plane cuts a surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub object: ObjectId,
    pub points: Vec<DVec2>,
}

/// A visible line beyond the plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub object: ObjectId,
    pub a: DVec2,
    pub b: DVec2,
    /// Distance from the plane in mm.
    pub depth: f64,
}

/// The drawing of a section.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionDrawing {
    /// Length of the section line in mm.
    pub length: f64,
    /// Cut faces of solids.
    pub faces: Vec<CutFace>,
    /// Cut through plants: outlines, not filled with a hatch.
    pub silhouettes: Vec<CutFace>,
    pub profiles: Vec<Profile>,
    pub edges: Vec<Edge>,
    /// Box around everything, or `None` for an empty drawing.
    pub bounds: Option<(DVec2, DVec2)>,
    /// Triangles that the plane cut.
    pub cut_triangles: usize,
    /// True if the edges hit the limit and some are missing.
    pub truncated: bool,
}

impl SectionDrawing {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
            && self.silhouettes.is_empty()
            && self.profiles.is_empty()
            && self.edges.is_empty()
    }

    /// Area of all cut faces, in mm2.
    pub fn cut_area(&self) -> f64 {
        self.faces
            .iter()
            .flat_map(|f| &f.rings)
            .map(|r| cadektor_core::geom::signed_area(r))
            .sum()
    }

    fn grow(&mut self) {
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        let mut add = |p: DVec2| {
            lo = lo.min(p);
            hi = hi.max(p);
        };
        for f in self.faces.iter().chain(&self.silhouettes) {
            f.rings.iter().flatten().for_each(|p| add(*p));
        }
        for p in &self.profiles {
            p.points.iter().for_each(|q| add(*q));
        }
        for e in &self.edges {
            add(e.a);
            add(e.b);
        }
        self.bounds = (lo.x <= hi.x && lo.y <= hi.y).then_some((lo, hi));
    }
}

/// The plane as a frame: origin, direction to the right, direction of the view.
struct Frame {
    o: DVec2,
    r: DVec2,
    n: DVec2,
    len: f64,
    scene_origin: DVec3,
}

impl Frame {
    fn new(spec: &SectionSpec, scene_origin: DVec3) -> Frame {
        let (o, e) = match spec.look {
            Look::Left => (spec.a, spec.b),
            Look::Right => (spec.b, spec.a),
        };
        let len = o.distance(e);
        let r = (e - o) / len;
        Frame {
            o,
            r,
            n: r.perp(),
            len,
            scene_origin,
        }
    }

    /// (u, height, w) of a scene position.
    fn uvw(&self, p: Vec3) -> DVec3 {
        let w = p.as_dvec3() + self.scene_origin;
        let d = w.truncate() - self.o;
        DVec3::new(d.dot(self.r), w.z, d.dot(self.n))
    }
}

/// The largest number of triangles that a drawing works on.
pub const MAX_TRIANGLES: usize = 3_000_000;

/// Draws the section of a scene. Objects that are section lines are left out. The scene must come
/// from the same document as `doc`.
pub fn compute(doc: &Document, scene: &Scene, spec: &SectionSpec) -> SectionDrawing {
    let mut out = SectionDrawing::default();
    if spec.problem().is_some() {
        return out;
    }
    let frame = Frame::new(spec, scene.origin);
    out.length = frame.len;
    let mesh = &scene.mesh;
    let plant_of = |id: ObjectId| {
        doc.object(id)
            .is_some_and(|o| matches!(o.shape, Shape::Plant { .. } | Shape::ExistingTree { .. }))
    };
    let skipped = |id: ObjectId| doc.object(id).is_none_or(settings::is_section_line);
    let mut tris: Vec<Tri> = Vec::with_capacity(mesh.triangle_count().min(MAX_TRIANGLES));
    for t in 0..mesh.triangle_count().min(MAX_TRIANGLES) {
        let object = mesh.tri_object[t];
        if skipped(object) {
            continue;
        }
        let idx = &mesh.indices[t * 3..t * 3 + 3];
        let pos = [
            mesh.positions[idx[0] as usize],
            mesh.positions[idx[1] as usize],
            mesh.positions[idx[2] as usize],
        ];
        if !pos.iter().all(|p| p.is_finite()) {
            continue;
        }
        let bits = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        tris.push(Tri {
            object,
            p: [frame.uvw(pos[0]), frame.uvw(pos[1]), frame.uvw(pos[2])],
            key: [bits(pos[0]), bits(pos[1]), bits(pos[2])],
            plant: plant_of(object),
            trunk: mesh.tri_part.get(t).copied() == Some(cadektor_core::present::PART_TRUNK),
        });
    }

    if spec.mode == Mode::Cut {
        cut_faces(&tris, &frame, &mut out, &plant_of);
    }
    if spec.mode == Mode::Elevation || spec.beyond {
        let mut segs = Vec::new();
        let with_tris: std::collections::BTreeSet<cadektor_core::uid::Uid> =
            tris.iter().map(|t| t.object.0).collect();
        let l = &scene.lines;
        for s in 0..l.segment_count() {
            let object = l.objects[s];
            if skipped(object) || with_tris.contains(&object.0) {
                continue;
            }
            let [a, b] = l.segment(s);
            if a.is_finite() && b.is_finite() {
                segs.push(Seg {
                    object,
                    a: frame.uvw(a),
                    b: frame.uvw(b),
                });
            }
        }
        let region = Region {
            len: frame.len,
            depth: spec.depth.unwrap_or(f64::INFINITY),
        };
        let covers: Vec<&CutFace> = out.faces.iter().collect();
        // Depths closer than this count as equal: a ten thousandth of the section, at least 1 mm.
        let tol = (frame.len * 1e-4).max(1.0);
        out.edges = beyond::edges(&tris, &segs, &region, &covers, tol, spec.mode == Mode::Cut);
        out.truncated = out.edges.len() >= beyond::MAX_EDGES;
    }
    out.grow();
    out
}

/// The cut of every object that the plane crosses.
fn cut_faces(
    tris: &[Tri],
    frame: &Frame,
    out: &mut SectionDrawing,
    plant_of: &dyn Fn(ObjectId) -> bool,
) {
    // The trunk of a plant is cut on its own: it is a solid, the canopy only gives a silhouette.
    let mut by_object: BTreeMap<(cadektor_core::uid::Uid, bool), Vec<(DVec2, DVec2)>> =
        BTreeMap::new();
    for t in tris {
        let w = [t.p[0].z, t.p[1].z, t.p[2].z];
        let side = [w[0] >= 0.0, w[1] >= 0.0, w[2] >= 0.0];
        if side[0] == side[1] && side[1] == side[2] {
            continue;
        }
        // The two edges that cross the plane. A vertex exactly on the plane counts as in front
        // of it, so every crossing is found once and the rings close.
        let mut pts = Vec::with_capacity(2);
        for k in 0..3 {
            let (i, j) = (k, (k + 1) % 3);
            if side[i] != side[j] {
                let f = w[i] / (w[i] - w[j]);
                let p = t.p[i].lerp(t.p[j], f);
                pts.push(DVec2::new(p.x, p.y));
            }
        }
        if let [a, b] = pts[..] {
            by_object
                .entry((t.object.0, t.trunk))
                .or_default()
                .push((a, b));
            out.cut_triangles += 1;
        }
    }
    for ((id, trunk), segs) in by_object {
        let object = ObjectId(id);
        let mut rings: Vec<Vec<DVec2>> = Vec::new();
        for c in cut::chain(&segs) {
            let pts = cut::simplify(&c.points, c.closed, cut::SNAP * 2.0);
            if c.closed {
                let clipped = cut::clip_ring(&pts, frame.len);
                if clipped.len() >= 3 {
                    rings.push(cut::simplify(&clipped, true, cut::SNAP * 2.0));
                }
            } else {
                for piece in cut::clip_line(&pts, frame.len) {
                    out.profiles.push(Profile {
                        object,
                        points: piece,
                    });
                }
            }
        }
        let rings = cut::orient(rings);
        if !rings.is_empty() {
            let face = CutFace { object, rings };
            if plant_of(object) && !trunk {
                out.silhouettes.push(face);
            } else {
                out.faces.push(face);
            }
        }
    }
}

/// A section cache that several callers share: the API, the sheets and the PDF export. The first
/// one that needs a drawing computes it, the others get the same one.
pub type SharedSections = Arc<Mutex<SectionCache>>;

/// Most sections that the cache keeps.
const MAX_SECTIONS: usize = 256;

/// Keeps the section of each key for one revision of the document. A viewer can ask every frame.
pub struct SectionCache {
    map: cadektor_core::cache::RevCache<u64, Arc<SectionDrawing>>,
    /// How often a section was computed.
    pub builds: u64,
}

impl Default for SectionCache {
    fn default() -> Self {
        SectionCache {
            map: cadektor_core::cache::RevCache::new(MAX_SECTIONS),
            builds: 0,
        }
    }
}

impl SectionCache {
    /// The drawing for `key` at `revision`. `make` runs only when the revision changed or the key is new.
    pub fn get(
        &mut self,
        revision: u64,
        key: u64,
        make: impl FnOnce() -> SectionDrawing,
    ) -> Arc<SectionDrawing> {
        if let Some(d) = self.map.get(revision, &key) {
            return d;
        }
        self.builds += 1;
        let d = Arc::new(make());
        self.map.insert(revision, key, d.clone());
        d
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests;
