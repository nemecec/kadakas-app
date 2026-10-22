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
    pick_at(world, engine, cam, cursor).map(|(item, _)| item)
}

/// Like [`pick`], with the point of the face or edge that was hit.
pub fn pick_at(
    world: &World,
    engine: &mut Inferencer,
    cam: &Camera,
    cursor: DVec2,
) -> Option<(Item, DVec3)> {
    // A pick is its own question: nothing of an earlier pick (a hovered edge) may change it.
    engine.reset();
    let inf = world.infer(engine, cam, cursor, None, &super::Excluded::default());
    let model = world.scenes.object(inf.source)?;
    match inf.source.kind() {
        TagKind::Edge => Some((Item::Edge(model, EdgeId(inf.source.id())), inf.point)),
        TagKind::Face => Some((Item::Face(model, FaceId(inf.source.id())), inf.point)),
        _ => None,
    }
}

/// Like [`pick`], but an object in front of the face or edge (a tree in front of a wall) wins:
/// the result is `None` then, and the caller falls back to the pick of objects.
pub fn pick_visible(
    world: &World,
    engine: &mut Inferencer,
    cam: &Camera,
    cursor: DVec2,
    scene: &kadakas_core::mesh::Scene,
) -> Option<Item> {
    let (item, point) = pick_at(world, engine, cam, cursor)?;
    object_in_front(world.doc, scene, cam, cursor, point)
        .is_none()
        .then_some(item)
}

/// The guide under the pointer, if the guides are shown. A guide point or a guide line within the
/// pick radius of the pointer counts, whatever geometry is near.
pub fn pick_guide(
    world: &World,
    engine: &mut Inferencer,
    cam: &Camera,
    cursor: DVec2,
) -> Option<ObjectId> {
    engine.reset();
    let guides: Vec<&crate::infer3d::Scene> = world.scenes.guide_scene().into_iter().collect();
    let only_guides = |t: crate::infer3d::Tag| t.kind() != TagKind::Guide;
    let inf = engine.infer_scenes(&guides, cam, cursor, None, &only_guides);
    world.scenes.guide(inf.source)
}

/// The guides for a selection rectangle (pixels): a window takes the guide points and the finite
/// guide lines that lie fully inside, a crossing rectangle takes what it touches (also a guide
/// line without ends). Guides that are not shown are not in the scenes, so they are never taken.
pub fn rect_guides(
    world: &World,
    cam: &Camera,
    a: DVec2,
    b: DVec2,
    crossing: bool,
) -> Vec<ObjectId> {
    let (lo, hi) = (a.min(b), a.max(b));
    let mut out = Vec::new();
    for &id in world.scenes.guide_ids() {
        let Some(g) = world.doc.object(id).and_then(|o| match &o.shape {
            kadakas_core::model::Shape::Guide(g) => Some(&**g),
            _ => None,
        }) else {
            continue;
        };
        let z0 = super::scenes::elevation(world.doc, id);
        let at = g.at + DVec3::new(0.0, 0.0, z0);
        let hit = match (g.unit_dir(), g.length) {
            (None, _) => cam.project(at).is_some_and(|p| in_rect(p, lo, hi)),
            (Some(d), Some(l)) => match (cam.project(at), cam.project(at + d * l)) {
                (Some(p), Some(q)) if crossing => segment_hits_rect(p, q, lo, hi),
                (Some(p), Some(q)) => in_rect(p, lo, hi) && in_rect(q, lo, hi),
                _ => false,
            },
            (Some(d), None) if crossing => {
                // A line without ends: the part that the camera sees, as a long segment.
                let reach = crate::infer3d::GUIDE_REACH;
                match cam.screen_seg(at - d * reach, at + d * reach) {
                    Some(seg) => segment_hits_rect(seg.p[0], seg.p[1], lo, hi),
                    None => false,
                }
            }
            (Some(_), None) => false,
        };
        if hit {
            out.push(id);
        }
    }
    out
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

/// The faces that use an edge.
pub fn edge_faces(doc: &Document, model: ObjectId, edge: EdgeId) -> Vec<FaceId> {
    model_of(doc, model).map_or_else(Vec::new, |m| m.faces_of_edge(edge))
}

/// The items without the hidden faces and edges (Hide, MOD-05). A double click and a triple click
/// select neighbors; hidden ones stay out unless Hidden Geometry shows them.
pub fn without_hidden(doc: &Document, items: Vec<Item>) -> Vec<Item> {
    items
        .into_iter()
        .filter(|i| match i {
            Item::Face(m, f) => !model_of(doc, *m).is_some_and(|s| s.face_hidden(*f)),
            Item::Edge(m, e) => !model_of(doc, *m).is_some_and(|s| s.edge_hidden(*e)),
        })
        .collect()
}

/// A face with its edges, or an edge with its faces (a double click).
pub fn with_neighbors(doc: &Document, item: Item) -> Vec<Item> {
    match item {
        Item::Face(m, f) => std::iter::once(item)
            .chain(face_edges(doc, m, f).into_iter().map(|e| Item::Edge(m, e)))
            .collect(),
        Item::Edge(m, e) => std::iter::once(item)
            .chain(edge_faces(doc, m, e).into_iter().map(|f| Item::Face(m, f)))
            .collect(),
    }
}

/// All faces and edges that are connected to the item through shared vertices (a triple click).
/// Geometry in the same model that touches nothing of it is not included.
pub fn connected(doc: &Document, item: Item) -> Vec<Item> {
    let model = item.model();
    let Some(m) = model_of(doc, model) else {
        return Vec::new();
    };
    let mut verts: BTreeSet<kadakas_core::surface::VertexId> = BTreeSet::new();
    match item {
        Item::Edge(_, e) => {
            if let Some(x) = m.edge(e) {
                verts.extend(x.v);
            }
        }
        Item::Face(_, f) => {
            if let Some(f) = m.face(f) {
                verts.extend(f.loops().flatten().copied());
            }
        }
    }
    // Grow until no edge or face adds a vertex.
    loop {
        let before = verts.len();
        for (_, e) in m.edges() {
            if e.v.iter().any(|v| verts.contains(v)) {
                verts.extend(e.v);
            }
        }
        for (_, f) in m.faces() {
            if f.loops().flatten().any(|v| verts.contains(v)) {
                verts.extend(f.loops().flatten().copied());
            }
        }
        if verts.len() == before {
            break;
        }
    }
    let mut out: Vec<Item> = m
        .edges()
        .filter(|(_, e)| e.v.iter().any(|v| verts.contains(v)))
        .map(|(id, _)| Item::Edge(model, id))
        .collect();
    out.extend(
        m.faces()
            .filter(|(_, f)| f.outer.iter().any(|v| verts.contains(v)))
            .map(|(id, _)| Item::Face(model, id)),
    );
    out
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
            // Hidden edges are offered when Hidden Geometry shows them.
            if m.edge_hidden(eid) && !world.scenes.show.hidden_geometry {
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
            if m.face_hidden(fid) && !world.scenes.show.hidden_geometry {
                continue;
            }
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

/// The objects for a selection rectangle (pixels) that are no surface models: plants, hardscapes,
/// site models, instances and the like (MOD-05, round B). A window takes the objects whose drawn
/// geometry lies fully inside, a crossing rectangle the ones it touches. Surface models have
/// their faces and edges selected by [`rect_select`] instead.
pub fn rect_select_objects(
    doc: &Document,
    scene: &kadakas_core::mesh::Scene,
    cam: &Camera,
    a: DVec2,
    b: DVec2,
    crossing: bool,
) -> Vec<ObjectId> {
    let (lo, hi) = (a.min(b), a.max(b));
    let px = |p: glam::Vec3| cam.project(scene.to_world(p));
    let mut out = Vec::new();
    for id in scene.objects() {
        if model_of(doc, id).is_some() {
            continue;
        }
        let Some((tris, segs)) = scene.object_ranges(id) else {
            continue;
        };
        let any = std::cell::Cell::new(false);
        let all_inside = std::cell::Cell::new(true);
        let touches = std::cell::Cell::new(false);
        let see = |p: Option<DVec2>| match p {
            Some(p) => {
                any.set(true);
                if in_rect(p, lo, hi) {
                    touches.set(true);
                } else {
                    all_inside.set(false);
                }
            }
            None => all_inside.set(false),
        };
        for t in tris.clone() {
            for p in scene.mesh.triangle(t) {
                see(px(p));
            }
            if !crossing && !all_inside.get() {
                break;
            }
        }
        for s in segs.clone() {
            for p in scene.lines.segment(s) {
                see(px(p));
            }
            if !crossing && !all_inside.get() {
                break;
            }
        }
        let hit = if crossing {
            touches.get() || {
                // The rectangle may cross the geometry without holding a corner, or lie inside a
                // large face.
                let centre = (lo + hi) / 2.0;
                tris.clone().any(|t| {
                    let q = scene.mesh.triangle(t).map(px);
                    match q {
                        [Some(p0), Some(p1), Some(p2)] => {
                            segment_hits_rect(p0, p1, lo, hi)
                                || segment_hits_rect(p1, p2, lo, hi)
                                || segment_hits_rect(p2, p0, lo, hi)
                                || point_in_polygon(centre, &[p0, p1, p2])
                        }
                        _ => false,
                    }
                }) || segs.clone().any(|s| {
                    let [p, q] = scene.lines.segment(s).map(px);
                    matches!((p, q), (Some(p), Some(q)) if segment_hits_rect(p, q, lo, hi))
                })
            }
        } else {
            any.get() && all_inside.get()
        };
        if hit {
            out.push(id);
        }
    }
    out
}

/// The object that is in front of a face or an edge under the pointer: an object that is no
/// surface model (a plant, a slab, a site model) whose surface the pointer ray meets before it
/// meets `hit`, the point of the face or edge. A click on a tree in front of a wall is a click on
/// the tree. `None` when the face or edge is the nearest thing.
pub fn object_in_front(
    doc: &Document,
    scene: &kadakas_core::mesh::Scene,
    cam: &Camera,
    cursor: DVec2,
    hit: DVec3,
) -> Option<ObjectId> {
    let ray = cam.ray(cursor);
    // A parallel ray starts on a plane that may be inside the scene: step back out of it.
    let back = if matches!(
        cam.projection,
        crate::infer3d::Projection::Orthographic { .. }
    ) {
        let reach = scene.bounds.map_or(0.0, |(lo, hi)| {
            f64::from((hi - lo).length() + (lo + hi).length())
        });
        reach + (ray.origin - hit).length()
    } else {
        0.0
    };
    let origin = ray.origin - ray.dir * back;
    let local = (origin - scene.origin).as_vec3();
    let (id, t) = kadakas_render::pick::pick_ray(scene, local, ray.dir.as_vec3())?;
    let limit = (hit - origin).dot(ray.dir);
    (model_of(doc, id).is_none() && f64::from(t) < limit - 1.0).then_some(id)
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
