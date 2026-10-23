//! The plan view of surface models (MOD-13): what the plan on the screen, the sheets and the SVG
//! draw for a surface model and for a component instance, and what picking and snapping use.
//!
//! The plan is the view from above. A face that looks up (or down) shows as its polygon, filled with
//! the colour of its material. A vertical face has no area in the plan and is left out. The faces
//! are drawn from the lowest to the highest, each followed by the edges that it is the top face of,
//! so a roof covers what is under it. Two faces whose footprints overlap are ordered by their
//! height at the overlap, not by their average height, so a slope runs over a plate where it is
//! above it and under it where it is below. An edge that no face seen from above uses (a loose
//! edge, the edge of a wall) is drawn in the order of its height, so a roof covers it. Hidden
//! faces and hidden edges are not drawn, and neither are soft edges (the edges of a curved
//! surface). One function makes the drawing, so the screen, the print and the SVG show the same
//! (AGR-01).
//!
//! A model with more than [`MAX_FACES`] faces, or more than [`MAX_EDGES`] edges, is drawn as its
//! outline (the border, the silhouette and the loose edges, [`Detail::Outline`]), and if even that
//! is more than [`MAX_OUTLINE_EDGES`] lines, as the box around it ([`Detail::Box`]). The limits
//! come from the measurements in `docs/modeler-limits.md`.

use std::collections::HashMap;

use glam::{DAffine3, DVec2, DVec3};

use crate::component::{self, Leaf, PlanMark};
use crate::model::{Document, Object, Shape};
use crate::present::{self, PaintResolver};
use crate::resource::ResourceId;
use crate::surface::{FaceId, SurfaceModel, VertexId};

/// Most faces that the plan draws one by one. More are drawn as the outline.
pub const MAX_FACES: usize = 20_000;
/// Most edges that the plan draws one by one. More are drawn as the outline.
pub const MAX_EDGES: usize = 30_000;
/// Most lines of an outline. More are drawn as the box.
pub const MAX_OUTLINE_EDGES: usize = 20_000;

/// The limits of the plan: the most faces and edges that it draws one by one, and the most lines
/// of an outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub faces: usize,
    pub edges: usize,
    pub outline: usize,
}

impl Limits {
    /// [`MAX_FACES`], [`MAX_EDGES`] and [`MAX_OUTLINE_EDGES`].
    pub const DEFAULT: Limits = Limits {
        faces: MAX_FACES,
        edges: MAX_EDGES,
        outline: MAX_OUTLINE_EDGES,
    };
}

/// How much of a model the plan draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// Every face and every edge that is shown.
    Full,
    /// The border, the silhouette and the loose edges. No fills.
    Outline,
    /// The box around the model.
    Box,
}

/// A face seen from above: the outer ring counterclockwise and the holes clockwise.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanFace {
    pub rings: Vec<Vec<DVec2>>,
    /// RGBA. Alpha 0 draws no fill.
    pub color: [u8; 4],
}

/// One thing of the plan, in the order of drawing.
#[derive(Clone, Debug, PartialEq)]
pub enum PlanItem {
    Face(PlanFace),
    Edge(DVec2, DVec2),
}

/// The plan of a model, a group of parts or an instance.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanDrawing {
    pub items: Vec<PlanItem>,
    pub detail: Detail,
    /// The faces and the edges of the parts, hidden ones not counted.
    pub faces_total: usize,
    pub edges_total: usize,
    /// The limits that decided the detail.
    pub limits: Limits,
}

impl PlanDrawing {
    /// True when the drawing is not the full one: the person sees an outline or a box.
    pub fn simplified(&self) -> bool {
        self.detail != Detail::Full
    }

    /// The faces drawn.
    pub fn faces(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, PlanItem::Face(_)))
            .count()
    }

    /// The edges drawn.
    pub fn edges(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, PlanItem::Edge(..)))
            .count()
    }

    /// The box around everything drawn, or `None` for an empty drawing.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        let mut add = |p: DVec2| {
            lo = lo.min(p);
            hi = hi.max(p);
        };
        for i in &self.items {
            match i {
                PlanItem::Face(f) => f.rings.iter().flatten().for_each(|p| add(*p)),
                PlanItem::Edge(a, b) => {
                    add(*a);
                    add(*b);
                }
            }
        }
        (lo.x <= hi.x && lo.y <= hi.y).then_some((lo, hi))
    }
}

/// A surface model placed in the drawing: the model, its transform to the drawing and the colour of
/// its faces that have no material of their own.
#[derive(Clone, Copy)]
pub struct Part<'a> {
    pub model: &'a SurfaceModel,
    pub xf: DAffine3,
    pub fill: Option<[u8; 4]>,
}

/// The colours of the materials of a document, for the faces that are painted.
pub struct Paint<'a> {
    doc: &'a Document,
    resolver: PaintResolver,
}

impl<'a> Paint<'a> {
    pub fn new(doc: &'a Document) -> Paint<'a> {
        Paint {
            doc,
            resolver: PaintResolver::new(doc),
        }
    }

    fn color(&self, id: ResourceId) -> Option<[u8; 4]> {
        present::material(self.doc, id).map(|m| m.base_color.0)
    }
}

/// What an object shows in the plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Drawing(PlanDrawing),
    /// An instance that cannot be drawn: the mark (a cross) at its origin, and the reason.
    Broken(DVec2, String),
}

/// The parts of an object that draws as surface models: the model itself, or the models of the
/// definition of an instance. `None` for any other object. `Err` for an instance that cannot be
/// expanded. A part of a definition whose class is switched off in the document is left out.
pub fn parts_of<'a>(
    doc: &'a Document,
    obj: &'a Object,
) -> Option<Result<Vec<Part<'a>>, (DVec2, String)>> {
    parts_of_with(doc, obj, &|c| doc.class(c).is_none_or(|k| k.visible))
}

/// Like [`parts_of`], with another view of the classes: `visible` says whether a class shows. A
/// viewport and an export have their own (the design view of the document is not theirs).
pub fn parts_of_with<'a>(
    doc: &'a Document,
    obj: &'a Object,
    visible: &dyn Fn(crate::model::ClassId) -> bool,
) -> Option<Result<Vec<Part<'a>>, (DVec2, String)>> {
    let fill = || {
        crate::hardscape::styled_attrs(doc, obj, doc.resolved_attrs(obj))
            .fill
            .filter(|c| c.0[3] > 0)
            .map(|c| c.0)
    };
    match &obj.shape {
        Shape::SurfaceModel(m) => Some(Ok(vec![Part {
            model: m,
            xf: m.transform,
            fill: fill(),
        }])),
        Shape::Component { symbol, transform } => {
            // A problem of the instance comes from the cached check of the plan mark.
            if let Some(PlanMark::Broken(p, why)) = component::plan_mark(doc, &obj.shape) {
                return Some(Err((p, why)));
            }
            let inst = component::affine(transform);
            let own = fill();
            let mut cache = component::ExpansionCache::new(&doc.resources);
            let leaves = match component::leaves_in(&mut cache, *symbol) {
                Ok(l) => l,
                Err(e) => {
                    return Some(Err((
                        DVec2::new(transform[9], transform[10]),
                        e.to_string(),
                    )));
                }
            };
            Some(Ok(leaves
                .into_iter()
                .filter(|l| component::leaf_shown_by(l, visible))
                .map(|l: Leaf<'a>| Part {
                    model: l.model,
                    xf: inst * l.transform,
                    fill: component::leaf_fill(doc, &l).or(own),
                })
                .collect()))
        }
        _ => None,
    }
}

/// The plan of an object, or `None` when the object is not a surface model or an instance.
pub fn plan_of_object(doc: &Document, obj: &Object) -> Option<Outcome> {
    plan_of_object_with(doc, obj, &|c| doc.class(c).is_none_or(|k| k.visible))
}

/// Like [`plan_of_object`], with the view of the classes of a viewport or an export.
pub fn plan_of_object_with(
    doc: &Document,
    obj: &Object,
    visible: &dyn Fn(crate::model::ClassId) -> bool,
) -> Option<Outcome> {
    Some(match parts_of_with(doc, obj, visible)? {
        Ok(parts) => Outcome::Drawing(drawing(&parts, Some(&Paint::new(doc)))),
        Err((p, why)) => Outcome::Broken(p, why),
    })
}

/// The corners of the box around the parts in the plan, counterclockwise.
fn box_ring(parts: &[Part]) -> Option<Vec<DVec2>> {
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    for p in parts {
        for (_, v) in p.model.vertices() {
            let w = p.xf.transform_point3(v).truncate();
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    (lo.x <= hi.x && lo.y <= hi.y)
        .then(|| vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)])
}

/// A face that is seen from above.
struct Seen {
    part: usize,
    id: FaceId,
    rings: Vec<Vec<DVec2>>,
    /// The average height, the first key to order the faces.
    z: f64,
    /// The plane of the face in the drawing: a normal that is not vertical, and `normal . p = d`.
    plane: (DVec3, f64),
    /// The box of the outer ring.
    lo: DVec2,
    hi: DVec2,
    /// True when the front looks up.
    up: bool,
}

impl Seen {
    /// The height of the plane of the face over a point of the plan.
    fn z_at(&self, p: DVec2) -> f64 {
        let (n, d) = self.plane;
        (d - n.x * p.x - n.y * p.y) / n.z
    }

    /// True if the point is inside the face: in the outer ring and in no hole.
    fn contains(&self, p: DVec2) -> bool {
        crate::geom::point_in_polygon(p, &self.rings[0])
            && !self.rings[1..]
                .iter()
                .any(|h| crate::geom::point_in_polygon(p, h))
    }
}

/// What the faces on the two sides of an edge look like from above.
#[derive(Clone, Copy, Default)]
struct Sides {
    up: bool,
    down: bool,
    flat: bool,
    uses: u32,
}

impl Sides {
    /// An edge that bounds the picture: a border, a loose edge, a vertical face on one side, or a
    /// front and a back side by side.
    fn silhouette(&self) -> bool {
        self.uses != 2 || self.flat || (self.up && self.down)
    }
}

fn key(a: VertexId, b: VertexId) -> (VertexId, VertexId) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Where the vertices of a part are in the drawing.
fn world(p: &Part) -> HashMap<VertexId, DVec3> {
    p.model
        .vertices()
        .map(|(id, v)| (id, p.xf.transform_point3(v)))
        .collect()
}

/// The area is too small to see in the plan, in comparison with the size of the face.
fn is_flat(area: f64, ring: &[DVec2]) -> bool {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in ring {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let size = (hi - lo).max_element();
    area.abs() <= 1e-9 * size * size
}

/// Every face of a part that is not hidden, seen from above. A vertical face is `None`.
fn seen_faces(index: usize, p: &Part, w: &HashMap<VertexId, DVec3>) -> Vec<(FaceId, Option<Seen>)> {
    let mirror = p.xf.matrix3.determinant() < 0.0;
    let mut out = Vec::new();
    for (id, f) in p.model.faces() {
        if p.model.face_hidden(id) {
            continue;
        }
        let ring = |l: &[VertexId]| -> Option<Vec<DVec2>> {
            l.iter().map(|v| w.get(v).map(|q| q.truncate())).collect()
        };
        let (Some(outer), Some(z)) = (
            ring(&f.outer),
            f.outer
                .iter()
                .map(|v| w.get(v).map(|q| q.z))
                .sum::<Option<f64>>(),
        ) else {
            out.push((id, None));
            continue;
        };
        let area = crate::geom::signed_area(&outer);
        if is_flat(area, &outer) {
            out.push((id, None));
            continue;
        }
        // The plane in the drawing, from the loop (Newell), for the order of overlapping faces.
        let pts3: Vec<DVec3> = f.outer.iter().filter_map(|v| w.get(v).copied()).collect();
        let mut normal = DVec3::ZERO;
        for k in 0..pts3.len() {
            let (a, b) = (pts3[k], pts3[(k + 1) % pts3.len()]);
            normal += DVec3::new(
                (a.y - b.y) * (a.z + b.z),
                (a.z - b.z) * (a.x + b.x),
                (a.x - b.x) * (a.y + b.y),
            );
        }
        let plane = (normal, normal.dot(pts3[0]));
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for q in &outer {
            lo = lo.min(*q);
            hi = hi.max(*q);
        }
        // The outer loop runs counterclockwise seen from the front, and a mirror turns that over.
        let up = (area > 0.0) != mirror;
        let mut rings = vec![outer];
        for h in &f.holes {
            if let Some(r) = ring(h) {
                rings.push(r);
            }
        }
        // The outer ring counterclockwise, the holes the other way round.
        if area < 0.0 {
            rings.iter_mut().for_each(|r| r.reverse());
        }
        out.push((
            id,
            Some(Seen {
                part: index,
                id,
                rings,
                z: z / f.outer.len().max(1) as f64,
                plane,
                lo,
                hi,
                up,
            }),
        ));
    }
    out
}

/// The colour of a face as seen from above.
fn face_color(p: &Part, id: FaceId, up: bool, paint: Option<&Paint>) -> [u8; 4] {
    let fallback = if up {
        p.fill.unwrap_or(crate::mesh::SURFACE_FRONT)
    } else {
        p.fill.unwrap_or(crate::mesh::SURFACE_BACK)
    };
    let Some(paint) = paint else { return fallback };
    let Some(face) = p.model.face(id) else {
        return fallback;
    };
    let (front, back) = paint.resolver.faces(&face.attrs);
    // A mirror swaps what is seen from above, which `up` already says.
    let side = if up { front } else { back };
    side.and_then(|r| paint.color(r)).unwrap_or(fallback)
}

/// How the plan draws the parts, without the drawing when it is the full one: the detail, the
/// faces and the edges that count. For a list of objects, which asks for each of them.
pub fn detail_of(parts: &[Part]) -> (Detail, usize, usize) {
    let limits = Limits::DEFAULT;
    let (faces, edges) = totals(parts);
    if faces <= limits.faces && edges <= limits.edges {
        return (Detail::Full, faces, edges);
    }
    (cached_drawing(parts, limits).detail, faces, edges)
}

/// The faces that are not hidden and the edges that are shown, in all the parts.
fn totals(parts: &[Part]) -> (usize, usize) {
    let faces = parts
        .iter()
        .map(|p| p.model.face_count() - p.model.hidden_faces().len())
        .sum();
    let edges = parts
        .iter()
        .map(|p| {
            p.model
                .edges()
                .filter(|(id, _)| p.model.edge_shown(*id))
                .count()
        })
        .sum();
    (faces, edges)
}

/// The order in which the faces are drawn. `seen` is sorted by the average height. Two faces whose
/// footprints overlap are ordered by their height at a point of the overlap: the lower one first.
/// The pairs come from a grid over the boxes. Where the heights cross (a slope through a plate)
/// the point decides, which is a known limit: the face is not cut. A cycle is broken at the
/// face that comes first by the average height. Returns the indices of `seen` in drawing order.
fn paint_order(seen: &[Seen]) -> Vec<usize> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = seen.len();
    if n < 2 {
        return (0..n).collect();
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    let mut mean = 0.0;
    for s in seen {
        lo = lo.min(s.lo);
        hi = hi.max(s.hi);
        mean += (s.hi - s.lo).max_element();
    }
    mean /= n as f64;
    let extent = (hi - lo).max_element().max(1e-9);
    let cell = (mean * 2.0).max(extent / 512.0).max(1e-9);
    let cols = (((hi.x - lo.x) / cell) as usize + 1).min(1024);
    let rows = (((hi.y - lo.y) / cell) as usize + 1).min(1024);
    let cell_of = |p: DVec2| {
        (
            (((p.x - lo.x) / cell) as usize).min(cols - 1),
            (((p.y - lo.y) / cell) as usize).min(rows - 1),
        )
    };
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); cols * rows];
    let mut big: Vec<u32> = Vec::new();
    for (i, s) in seen.iter().enumerate() {
        let ((x0, y0), (x1, y1)) = (cell_of(s.lo), cell_of(s.hi));
        if (x1 - x0 + 1) * (y1 - y0 + 1) > 64 {
            big.push(i as u32);
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                grid[y * cols + x].push(i as u32);
            }
        }
    }
    let mut must_follow: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut waiting = vec![0u32; n];
    // Tests the pair `a`, `b`. In the grid a pair is met in every cell that both reach, so it is
    // tested in one only: the cell of the lower left corner of the overlap of the two boxes.
    let mut test = |a: u32, b: u32, only_in: Option<(usize, usize)>| {
        let (a, b) = (a.min(b), a.max(b));
        if a == b {
            return;
        }
        let (fa, fb) = (&seen[a as usize], &seen[b as usize]);
        let (olo, ohi) = (fa.lo.max(fb.lo), fa.hi.min(fb.hi));
        if olo.x > ohi.x || olo.y > ohi.y {
            return;
        }
        if only_in.is_some_and(|c| c != cell_of(olo)) {
            return;
        }
        let mean_of = |s: &Seen| s.rings[0].iter().sum::<DVec2>() / s.rings[0].len() as f64;
        for p in [(olo + ohi) / 2.0, mean_of(fa), mean_of(fb)] {
            if fa.contains(p) && fb.contains(p) {
                let (za, zb) = (fa.z_at(p), fb.z_at(p));
                if (za - zb).abs() > 1e-6 * extent.max(1.0) {
                    let (first, second) = if za < zb { (a, b) } else { (b, a) };
                    must_follow[first as usize].push(second);
                    waiting[second as usize] += 1;
                    return;
                }
            }
        }
    };
    for (c, cellv) in grid.iter().enumerate() {
        let at = (c % cols, c / cols);
        for (k, a) in cellv.iter().enumerate() {
            for b in &cellv[k + 1..] {
                test(*a, *b, Some(at));
            }
        }
    }
    // The big faces are in no cell: each is tested with every face, once.
    for (k, a) in big.iter().enumerate() {
        for b in 0..n as u32 {
            if !big[..k].contains(&b) {
                test(*a, b, None);
            }
        }
    }
    // The faces that nothing has to come before go in the order of the average height. A cycle
    // is broken at the first face left.
    let mut ready: BinaryHeap<Reverse<u32>> = (0..n as u32)
        .filter(|i| waiting[*i as usize] == 0)
        .map(Reverse)
        .collect();
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut next_unplaced = 0usize;
    while order.len() < n {
        let i = match ready.pop() {
            Some(Reverse(i)) => i as usize,
            None => {
                while done[next_unplaced] {
                    next_unplaced += 1;
                }
                next_unplaced
            }
        };
        if done[i] {
            continue;
        }
        done[i] = true;
        order.push(i);
        for j in std::mem::take(&mut must_follow[i]) {
            let w = &mut waiting[j as usize];
            *w = w.saturating_sub(1);
            if *w == 0 && !done[j as usize] {
                ready.push(Reverse(j));
            }
        }
    }
    order
}

/// Draws the plan of the parts, within [`Limits::DEFAULT`].
pub fn drawing(parts: &[Part], paint: Option<&Paint>) -> PlanDrawing {
    drawing_with(parts, paint, Limits::DEFAULT)
}

/// Draws the plan of the parts within `limits` (the measurements of `docs/modeler-limits.md` use
/// other limits to time the full drawing of a big model).
pub fn drawing_with(parts: &[Part], paint: Option<&Paint>, limits: Limits) -> PlanDrawing {
    let worlds: Vec<HashMap<VertexId, DVec3>> = parts.iter().map(world).collect();
    let (faces_total, edges_total) = totals(parts);
    let mut out = PlanDrawing {
        items: Vec::new(),
        detail: Detail::Full,
        faces_total,
        edges_total,
        limits,
    };
    let full = faces_total <= limits.faces && edges_total <= limits.edges;
    // The faces seen from above, and for every edge the sides of the faces at it.
    let mut seen: Vec<Seen> = Vec::new();
    let mut sides: Vec<HashMap<(VertexId, VertexId), Sides>> = vec![HashMap::new(); parts.len()];
    for (i, p) in parts.iter().enumerate() {
        for (id, s) in seen_faces(i, p, &worlds[i]) {
            let Some(f) = p.model.face(id) else { continue };
            let (up, flat) = s.as_ref().map_or((false, true), |s| (s.up, false));
            for (a, b) in f.uses() {
                let e = sides[i].entry(key(a, b)).or_default();
                e.uses += 1;
                e.flat |= flat;
                e.up |= !flat && up;
                e.down |= !flat && !up;
            }
            if let Some(s) = s {
                seen.push(s);
            }
        }
    }
    let shown_edges = |i: usize| {
        let p = &parts[i];
        let w = &worlds[i];
        p.model.edges().filter_map(move |(id, e)| {
            if !p.model.edge_shown(id) {
                return None;
            }
            let (a3, b3) = (*w.get(&e.v[0])?, *w.get(&e.v[1])?);
            let (a, b) = (a3.truncate(), b3.truncate());
            // A vertical edge is a point in the plan.
            (a.distance_squared(b) > 1e-12).then_some((e.key(), a, b, a3.z.max(b3.z)))
        })
    };
    if full {
        // Lowest first by the average height; the same height keeps the order of the parts and
        // of the ids. Then faces that overlap are put in the order of their height at the overlap.
        seen.sort_by(|a, b| {
            a.z.total_cmp(&b.z)
                .then(a.part.cmp(&b.part))
                .then(a.id.cmp(&b.id))
        });
        let zs: Vec<f64> = seen.iter().map(|s| s.z).collect();
        let order = paint_order(&seen);
        let mut slot: Vec<Option<Seen>> = seen.into_iter().map(Some).collect();
        let seen: Vec<Seen> = order
            .iter()
            .map(|i| slot[*i].take().expect("once"))
            .collect();
        let mut place = vec![0usize; seen.len()];
        for (pos, base) in order.iter().enumerate() {
            place[*base] = pos;
        }
        // The edge goes with the highest face that uses it.
        let mut top: Vec<HashMap<(VertexId, VertexId), usize>> = vec![HashMap::new(); parts.len()];
        for (rank, s) in seen.iter().enumerate() {
            if let Some(f) = parts[s.part].model.face(s.id) {
                for (a, b) in f.uses() {
                    top[s.part].insert(key(a, b), rank);
                }
            }
        }
        // `after[r]` follows the face r. `before[r]` comes before it: the edges that no face seen
        // from above uses (loose edges, the edges of walls) in the order of their height, so a
        // roof over a wall covers the edges of the wall. `before[n]` is after every face.
        let n = seen.len();
        let mut after: Vec<Vec<(DVec2, DVec2)>> = vec![Vec::new(); n];
        let mut before: Vec<Vec<(DVec2, DVec2)>> = vec![Vec::new(); n + 1];
        for (i, tops) in top.iter().enumerate() {
            for (k, a, b, z) in shown_edges(i) {
                match tops.get(&k) {
                    Some(rank) => after[*rank].push((a, b)),
                    None => {
                        // The faces whose average height is not above the edge come first.
                        let base = zs.partition_point(|h| *h <= z);
                        let at = if base >= n { n } else { place[base] };
                        before[at].push((a, b));
                    }
                }
            }
        }
        for (rank, s) in seen.into_iter().enumerate() {
            for (a, b) in before[rank].drain(..) {
                out.items.push(PlanItem::Edge(a, b));
            }
            let color = face_color(&parts[s.part], s.id, s.up, paint);
            out.items.push(PlanItem::Face(PlanFace {
                rings: s.rings,
                color,
            }));
            for (a, b) in after[rank].drain(..) {
                out.items.push(PlanItem::Edge(a, b));
            }
        }
        for (a, b) in before[n].drain(..) {
            out.items.push(PlanItem::Edge(a, b));
        }
        return out;
    }
    // Too much to draw one by one: the outline.
    out.detail = Detail::Outline;
    for (i, part_sides) in sides.iter().enumerate() {
        for (k, a, b, _) in shown_edges(i) {
            if part_sides.get(&k).is_none_or(Sides::silhouette) {
                out.items.push(PlanItem::Edge(a, b));
            }
        }
    }
    if out.items.len() > limits.outline {
        out.items.clear();
        out.detail = Detail::Box;
    }
    if out.detail == Detail::Box || out.items.is_empty() {
        out.detail = Detail::Box;
        out.items.clear();
        if let Some(ring) = box_ring(parts) {
            let color = parts.iter().find_map(|p| p.fill).unwrap_or([0, 0, 0, 0]);
            for i in 0..4 {
                out.items.push(PlanItem::Edge(ring[i], ring[(i + 1) % 4]));
            }
            out.items.insert(
                0,
                PlanItem::Face(PlanFace {
                    rings: vec![ring],
                    color,
                }),
            );
        }
    }
    out
}

/// True if a point is on the plan of the parts: inside a face that is seen from above, or within
/// `tol` of an edge that is shown. The same faces and edges as the drawing.
pub fn hit(parts: &[Part], p: DVec2, tol: f64) -> bool {
    for part in parts {
        let w = world(part);
        // The box first: most clicks are far from the model.
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for q in w.values() {
            lo = lo.min(q.truncate());
            hi = hi.max(q.truncate());
        }
        if p.x < lo.x - tol || p.x > hi.x + tol || p.y < lo.y - tol || p.y > hi.y + tol {
            continue;
        }
        for (id, e) in part.model.edges() {
            if !part.model.edge_shown(id) {
                continue;
            }
            if let (Some(a), Some(b)) = (w.get(&e.v[0]), w.get(&e.v[1]))
                && dist_to_segment(p, a.truncate(), b.truncate()) <= tol
            {
                return true;
            }
        }
        for (_, s) in seen_faces(0, part, &w) {
            let Some(s) = s else { continue };
            if crate::geom::point_in_polygon(p, &s.rings[0])
                && !s.rings[1..]
                    .iter()
                    .any(|h| crate::geom::point_in_polygon(p, h))
            {
                return true;
            }
        }
    }
    false
}

fn dist_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// The points and edges of the parts that the pointer can snap to in the plan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapGeometry {
    /// The vertices.
    pub vertices: Vec<DVec2>,
    /// The middles of the edges that are shown.
    pub midpoints: Vec<DVec2>,
    /// The centres of the faces that are seen from above.
    pub centers: Vec<DVec2>,
    /// The edges that are shown.
    pub edges: Vec<[DVec2; 2]>,
}

/// The snap points of the parts, those within `radius` of `near` (center, radius) when it is given.
/// No part gives more than `cap` edges and `cap` centres. A model that the plan draws as an
/// outline or a box offers the points of that drawing only: the pointer is not offered a vertex
/// that is not on the screen.
pub fn snap_geometry(parts: &[Part], near: Option<(DVec2, f64)>, cap: usize) -> SnapGeometry {
    snap_geometry_with(parts, near, cap, Limits::DEFAULT)
}

/// Like [`snap_geometry`], within other limits of the plan.
pub fn snap_geometry_with(
    parts: &[Part],
    near: Option<(DVec2, f64)>,
    cap: usize,
    limits: Limits,
) -> SnapGeometry {
    let close = |p: DVec2| near.is_none_or(|(c, r)| p.distance(c) <= r);
    let near_seg = |a: DVec2, b: DVec2| near.is_none_or(|(c, r)| dist_to_segment(c, a, b) <= r);
    let mut g = SnapGeometry::default();
    let (faces, edges) = totals(parts);
    if faces > limits.faces || edges > limits.edges {
        let d = cached_drawing(parts, limits);
        let mut used: Vec<DVec2> = Vec::new();
        for item in &d.items {
            let PlanItem::Edge(a, b) = item else { continue };
            if g.edges.len() >= cap {
                break;
            }
            if !near_seg(*a, *b) {
                continue;
            }
            for q in [*a, *b] {
                if close(q) && !used.contains(&q) {
                    used.push(q);
                    g.vertices.push(q);
                }
            }
            g.edges.push([*a, *b]);
            let mid = (*a + *b) / 2.0;
            if close(mid) {
                g.midpoints.push(mid);
            }
        }
        return g;
    }
    for part in parts {
        let w = world(part);
        let mut used: std::collections::HashSet<VertexId> = std::collections::HashSet::new();
        let mut count = 0usize;
        for (id, e) in part.model.edges() {
            if count >= cap {
                break;
            }
            if !part.model.edge_shown(id) {
                continue;
            }
            let (Some(a), Some(b)) = (w.get(&e.v[0]), w.get(&e.v[1])) else {
                continue;
            };
            let (a, b) = (a.truncate(), b.truncate());
            if !near_seg(a, b) {
                continue;
            }
            count += 1;
            for (v, q) in [(e.v[0], a), (e.v[1], b)] {
                if close(q) && used.insert(v) {
                    g.vertices.push(q);
                }
            }
            // A vertical edge is a point in the plan: it has no line and no middle.
            if a.distance_squared(b) > 1e-12 {
                g.edges.push([a, b]);
                let mid = (a + b) / 2.0;
                if close(mid) {
                    g.midpoints.push(mid);
                }
            }
        }
        let mut centers = 0usize;
        for (_, s) in seen_faces(0, part, &w) {
            if centers >= cap {
                break;
            }
            let Some(s) = s else { continue };
            let c = centroid(&s.rings[0]);
            if close(c) {
                centers += 1;
                g.centers.push(c);
            }
        }
    }
    g
}

thread_local! {
    /// The outline drawings of big models that the snap asked for, by what they were made from.
    static OUTLINES: std::cell::RefCell<Vec<(u64, std::rc::Rc<PlanDrawing>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// What the parts are made of, as a number: their edit counts, sizes, placements and the
/// coordinates of their vertices. Two parts with the same number draw the same.
fn fingerprint(parts: &[Part], limits: Limits) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (limits.faces, limits.edges, limits.outline).hash(&mut h);
    for p in parts {
        (
            p.model.rev(),
            p.model.vertex_count(),
            p.model.edge_count(),
            p.model.face_count(),
            p.fill,
        )
            .hash(&mut h);
        for x in p.xf.to_cols_array() {
            x.to_bits().hash(&mut h);
        }
        for (id, v) in p.model.vertices() {
            (id.0, v.x.to_bits(), v.y.to_bits(), v.z.to_bits()).hash(&mut h);
        }
        for id in p.model.hidden_faces() {
            id.0.hash(&mut h);
        }
        for id in p.model.hidden_edges() {
            id.0.hash(&mut h);
        }
    }
    h.finish()
}

/// The drawing of the parts within the limits, kept for the last few models: the snap asks for it
/// at every move of the pointer.
fn cached_drawing(parts: &[Part], limits: Limits) -> std::rc::Rc<PlanDrawing> {
    let key = fingerprint(parts, limits);
    OUTLINES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((_, d)) = c.iter().find(|(k, _)| *k == key) {
            return d.clone();
        }
        let d = std::rc::Rc::new(drawing_with(parts, None, limits));
        if c.len() >= 8 {
            c.remove(0);
        }
        c.push((key, d.clone()));
        d
    })
}

/// The centre of mass of a ring.
fn centroid(ring: &[DVec2]) -> DVec2 {
    let o = ring[0];
    let (mut a2, mut c) = (0.0, DVec2::ZERO);
    for i in 0..ring.len() {
        let (p, q) = (ring[i] - o, ring[(i + 1) % ring.len()] - o);
        let cr = p.perp_dot(q);
        a2 += cr;
        c += (p + q) * cr;
    }
    if a2.abs() < 1e-12 {
        return ring.iter().sum::<DVec2>() / ring.len() as f64;
    }
    o + c / (3.0 * a2)
}
