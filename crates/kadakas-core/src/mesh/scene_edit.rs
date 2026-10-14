//! Changes to a finished scene: recolor the objects, leave some out, add the geometry of another
//! scene. The compare overlay (REV-03) uses them to tint objects and to add ghosts, without
//! building the scene again.

use glam::Vec3;

use super::{ObjectRange, Scene};
use crate::model::ObjectId;

impl Scene {
    /// Changes the colors of the geometry of the objects. `f` gets the object and the color of a
    /// vertex or segment and gives the new color, or `None` to keep it. It is called once for each
    /// vertex (the first triangle that uses it counts) and once for each segment.
    pub fn recolor(&mut self, f: &dyn Fn(ObjectId, [u8; 4]) -> Option<[u8; 4]>) {
        let mut seen = vec![false; self.mesh.positions.len()];
        for (id, r) in &self.ranges {
            for t in r.tris.clone() {
                for k in 0..3 {
                    let v = self.mesh.indices[t * 3 + k] as usize;
                    if std::mem::replace(&mut seen[v], true) {
                        continue;
                    }
                    if let Some(c) = f(*id, self.mesh.colors[v]) {
                        self.mesh.colors[v] = c;
                    }
                }
            }
            for s in r.segs.clone() {
                if let Some(c) = f(*id, self.lines.colors[s]) {
                    self.lines.colors[s] = c;
                }
            }
        }
    }

    /// A copy of the scene with only the objects for which `keep` is true. Geometry that belongs
    /// to no object of the scene is left out.
    pub fn retain(&self, keep: &dyn Fn(ObjectId) -> bool) -> Scene {
        let mut out = Scene {
            origin: self.origin,
            ..Scene::default()
        };
        // The new index of a vertex, or `u32::MAX`.
        let mut map = vec![u32::MAX; self.mesh.positions.len()];
        for (id, r) in &self.ranges {
            if !keep(*id) {
                continue;
            }
            let (t0, s0) = (out.mesh.triangle_count(), out.lines.segment_count());
            for t in r.tris.clone() {
                for k in 0..3 {
                    let v = self.mesh.indices[t * 3 + k] as usize;
                    if map[v] == u32::MAX {
                        map[v] = out.mesh.positions.len() as u32;
                        out.mesh.positions.push(self.mesh.positions[v]);
                        out.mesh.normals.push(self.mesh.normals[v]);
                        out.mesh.colors.push(self.mesh.colors[v]);
                    }
                    out.mesh.indices.push(map[v]);
                }
                out.mesh.tri_object.push(self.mesh.tri_object[t]);
                out.mesh
                    .tri_part
                    .push(self.mesh.tri_part.get(t).copied().unwrap_or(0));
            }
            for s in r.segs.clone() {
                out.lines.positions.extend_from_slice(&[
                    self.lines.positions[2 * s],
                    self.lines.positions[2 * s + 1],
                ]);
                out.lines.colors.push(self.lines.colors[s]);
                out.lines.objects.push(self.lines.objects[s]);
            }
            out.ranges.insert(
                *id,
                ObjectRange {
                    tris: t0..out.mesh.triangle_count(),
                    segs: s0..out.lines.segment_count(),
                },
            );
        }
        out.refit();
        out
    }

    /// Adds the geometry of `other` to this scene. The positions of `other` are moved if its
    /// origin is another one. An object that this scene has already keeps its place in the list
    /// of objects: the added geometry is drawn but not listed under it.
    pub fn append(&mut self, other: &Scene) {
        let shift = (other.origin - self.origin).as_vec3();
        let base = self.mesh.positions.len() as u32;
        let (t0, s0) = (self.mesh.triangle_count(), self.lines.segment_count());
        let moved = |p: &Vec3| *p + shift;
        self.mesh
            .positions
            .extend(other.mesh.positions.iter().map(moved));
        self.mesh.normals.extend_from_slice(&other.mesh.normals);
        self.mesh.colors.extend_from_slice(&other.mesh.colors);
        self.mesh
            .indices
            .extend(other.mesh.indices.iter().map(|i| i + base));
        self.mesh
            .tri_object
            .extend_from_slice(&other.mesh.tri_object);
        self.mesh.tri_part.extend(
            (0..other.mesh.triangle_count())
                .map(|t| other.mesh.tri_part.get(t).copied().unwrap_or(0)),
        );
        self.lines
            .positions
            .extend(other.lines.positions.iter().map(moved));
        self.lines.colors.extend_from_slice(&other.lines.colors);
        self.lines.objects.extend_from_slice(&other.lines.objects);
        for (id, r) in &other.ranges {
            self.ranges.entry(*id).or_insert(ObjectRange {
                tris: t0 + r.tris.start..t0 + r.tris.end,
                segs: s0 + r.segs.start..s0 + r.segs.end,
            });
        }
        self.refit();
    }

    /// Computes the box around the geometry again.
    fn refit(&mut self) {
        let mut acc = super::BoundsAcc::default();
        self.mesh.positions.iter().for_each(|p| acc.add(*p));
        self.lines.positions.iter().for_each(|p| acc.add(*p));
        self.bounds = acc.get();
    }
}

#[cfg(test)]
mod tests;
