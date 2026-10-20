//! Selecting faces and edges of surface models: the selection, the pick under the pointer, the
//! window and crossing selection, and the lines that show a selected face.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{EdgeId, FaceId};

use super::plane::Plane3;
use super::scenes::{World, frame_of, model_of};
use crate::infer3d::{Camera, Inferencer, TagKind};

/// A selected face or edge of a surface model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item {
    Face(ObjectId, FaceId),
    Edge(ObjectId, EdgeId),
}

impl Item {
    pub fn model(self) -> ObjectId {
        match self {
            Item::Face(m, _) | Item::Edge(m, _) => m,
        }
    }
}

/// The selected faces and edges. Whole objects are in `Session::selection`; the two selections
/// are never both filled.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SubSelection {
    items: BTreeSet<Item>,
}

impl SubSelection {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        self.items.iter().copied()
    }

    pub fn contains(&self, i: Item) -> bool {
        self.items.contains(&i)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn set(&mut self, items: impl IntoIterator<Item = Item>) {
        self.items = items.into_iter().collect();
    }

    pub fn add(&mut self, items: impl IntoIterator<Item = Item>) {
        self.items.extend(items);
    }

    /// A click: replaces the selection with the item, or with Shift adds it or takes it out.
    /// A click on nothing clears it (not with Shift).
    pub fn click(&mut self, hit: Option<Item>, shift: bool) {
        match (hit, shift) {
            (Some(i), false) => self.set([i]),
            (Some(i), true) => {
                if !self.items.remove(&i) {
                    self.items.insert(i);
                }
            }
            (None, false) => self.items.clear(),
            (None, true) => {}
        }
    }

    pub fn faces(&self) -> Vec<(ObjectId, FaceId)> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Face(m, f) => Some((*m, *f)),
                Item::Edge(..) => None,
            })
            .collect()
    }

    pub fn edges(&self) -> Vec<(ObjectId, EdgeId)> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Edge(m, e) => Some((*m, *e)),
                Item::Face(..) => None,
            })
            .collect()
    }

    /// Takes out what is not in the document any more (after an undo).
    pub fn retain_existing(&mut self, doc: &Document) {
        self.items.retain(|i| match i {
            Item::Face(m, f) => model_of(doc, *m).is_some_and(|s| s.face(*f).is_some()),
            Item::Edge(m, e) => model_of(doc, *m).is_some_and(|s| s.edge(*e).is_some()),
        });
    }
}

/// The face or edge under the pointer. `engine` is an inferencer of its own, so that the locks
/// of a tool do not change what a click selects.
pub fn pick(world: &World, engine: &mut Inferencer, cam: &Camera, cursor: DVec2) -> Option<Item> {
    let inf = engine.infer(world.scenes.scene(), cam, cursor, None);
    let model = world.scenes.object(inf.source)?;
    match inf.source.kind() {
        TagKind::Edge => Some(Item::Edge(model, EdgeId(inf.source.id()))),
        TagKind::Face => Some(Item::Face(model, FaceId(inf.source.id()))),
        _ => None,
    }
}

/// The edges that bound a face.
pub fn face_edges(doc: &Document, model: ObjectId, face: FaceId) -> Vec<EdgeId> {
    let Some(m) = model_of(doc, model) else {
        return Vec::new();
    };
    let Some(f) = m.face(face) else {
        return Vec::new();
    };
    let mut out: BTreeSet<EdgeId> = BTreeSet::new();
    for (a, b) in f.uses() {
        if let Some(e) = m.edge_between(a, b) {
            out.insert(e);
        }
    }
    out.into_iter().collect()
}

/// Does the segment touch the rectangle? Both in pixels.
fn segment_hits_rect(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> bool {
    // Liang-Barsky.
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = b - a;
    for (p, q) in [
        (-d.x, a.x - lo.x),
        (d.x, hi.x - a.x),
        (-d.y, a.y - lo.y),
        (d.y, hi.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn in_rect(p: DVec2, lo: DVec2, hi: DVec2) -> bool {
    p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
}

fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// The faces and edges for a selection rectangle (pixels). A window (`crossing` false) takes what
/// lies fully inside; a crossing rectangle takes what it touches. Hidden edges are not offered.
pub fn rect_select(world: &World, cam: &Camera, a: DVec2, b: DVec2, crossing: bool) -> Vec<Item> {
    let (lo, hi) = (a.min(b), a.max(b));
    let mut out = Vec::new();
    for &id in world.scenes.objects() {
        let Some(m) = model_of(world.doc, id) else {
            continue;
        };
        let fr = frame_of(world.doc, id);
        let px = |v| {
            m.vertex(v)
                .and_then(|p| cam.project(fr.transform_point3(p)))
        };
        for (eid, e) in m.edges() {
            let hidden = m
                .edge_attrs(eid)
                .and_then(|x| x.get("hidden"))
                .is_some_and(|v| v == &serde_json::Value::Bool(true));
            if hidden {
                continue;
            }
            let (Some(p), Some(q)) = (px(e.v[0]), px(e.v[1])) else {
                continue;
            };
            let hit = if crossing {
                segment_hits_rect(p, q, lo, hi)
            } else {
                in_rect(p, lo, hi) && in_rect(q, lo, hi)
            };
            if hit {
                out.push(Item::Edge(id, eid));
            }
        }
        for (fid, f) in m.faces() {
            let pts: Option<Vec<DVec2>> = f.outer.iter().map(|v| px(*v)).collect();
            let Some(pts) = pts else { continue };
            let hit = if crossing {
                let n = pts.len();
                pts.iter().any(|p| in_rect(*p, lo, hi))
                    || (0..n).any(|i| segment_hits_rect(pts[i], pts[(i + 1) % n], lo, hi))
                    || point_in_polygon((lo + hi) / 2.0, &pts)
            } else {
                pts.iter().all(|p| in_rect(*p, lo, hi))
            };
            if hit {
                out.push(Item::Face(id, fid));
            }
        }
    }
    out
}

/// Lines across a face that show it is selected: parallel lines in the plane of the face, clipped
/// to the face (holes stay open). `loops` are in world coordinates, the outer loop first.
pub fn face_hatch(loops: &[Vec<DVec3>], lines: usize) -> Vec<[DVec3; 2]> {
    let Some(outer) = loops.first() else {
        return Vec::new();
    };
    if outer.len() < 3 || lines == 0 {
        return Vec::new();
    }
    let n = outer
        .iter()
        .enumerate()
        .map(|(i, p)| (*p - outer[0]).cross(outer[(i + 1) % outer.len()] - outer[0]))
        .sum::<DVec3>();
    let Some(plane) = Plane3::through(outer[0], n) else {
        return Vec::new();
    };
    let flat: Vec<Vec<DVec2>> = loops
        .iter()
        .map(|l| l.iter().map(|p| plane.coords(*p)).collect())
        .collect();
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in flat.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if !(hi.y - lo.y).is_finite() || hi.y - lo.y <= 0.0 {
        return Vec::new();
    }
    let step = (hi.y - lo.y) / (lines as f64 + 1.0);
    let mut out = Vec::new();
    for k in 1..=lines {
        let y = lo.y + step * k as f64 + step * 0.013;
        let mut xs: Vec<f64> = Vec::new();
        for l in &flat {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let p = plane.origin + plane.u * pair[0] + plane.v * y;
            let q = plane.origin + plane.u * pair[1] + plane.v * y;
            out.push([p, q]);
        }
    }
    out
}
