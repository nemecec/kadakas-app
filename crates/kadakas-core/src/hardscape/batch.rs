//! Turns closed shapes into hardscapes in one step, for example the outlines of a parking lot
//! that came in from a DXF file. A shape that lies inside another one becomes a hole of it.
//!
//! "Inside" is a test of area, not of corners: a shape is inside another one when it leaves it by
//! less than a noise area (see `layout::ring_within`), so an island that touches the kerb line, or
//! sits 0.01 mm outside after a snap, is still a hole. A shape that overlaps another one only in
//! part is left out and named in `skipped`: the command never makes two hardscapes that overlap.

use glam::DVec2;

use super::HardscapePatch;
use super::data::MAX_HOLES;
use crate::command::Command;
use crate::curve::{self, Vertex};
use crate::geom;
use crate::layout::object::outline_of;
use crate::layout::{area_shared, ring_within};
use crate::model::{Document, ObjectId, Shape};
use crate::resource::ResourceId;

/// Flattening tolerance used to find which shape lies in which, in mm.
const TOLERANCE: f64 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct FromShapesOptions {
    /// The hardscape style of the new hardscapes.
    pub style: Option<ResourceId>,
    /// A shape inside another one becomes a hole of it (a shape inside a hole becomes a hardscape
    /// again, and so on). Without this, every shape becomes a hardscape of its own.
    pub holes_from_inner: bool,
    /// Delete the shapes that were used. Off by default: the outlines of a survey stay.
    pub delete_source: bool,
}

impl Default for FromShapesOptions {
    fn default() -> Self {
        FromShapesOptions {
            style: None,
            holes_from_inner: true,
            delete_source: false,
        }
    }
}

/// The plan of a conversion.
#[derive(Debug, Default)]
pub struct Made {
    /// One command, so one undo step. `None` when nothing can be made.
    pub command: Option<Command>,
    /// (source shape, new hardscape, number of holes).
    pub created: Vec<(ObjectId, ObjectId, usize)>,
    /// (shape that became a hole, the source shape of the hardscape it is a hole of).
    pub holes: Vec<(ObjectId, ObjectId)>,
    /// (shape, reason).
    pub skipped: Vec<(ObjectId, String)>,
}

struct Item {
    id: ObjectId,
    outer: Vec<Vertex>,
    ring: Vec<DVec2>,
    area: f64,
    lo: DVec2,
    hi: DVec2,
}

fn boxes_meet(a: &Item, b: &Item) -> bool {
    a.lo.x <= b.hi.x && b.lo.x <= a.hi.x && a.lo.y <= b.hi.y && b.lo.y <= a.hi.y
}

/// Noise area for "the shapes overlap", in mm2.
fn noise(a: f64) -> f64 {
    (1e-6 * a).max(1.0)
}

/// Plans the conversion of the given shapes. Shapes that are not closed outlines are skipped with
/// a reason.
pub fn from_shapes(doc: &Document, ids: &[ObjectId], opts: &FromShapesOptions) -> Made {
    let mut made = Made::default();
    let mut items: Vec<Item> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(o) = doc.object(id) else {
            made.skipped.push((id, "no such object".into()));
            continue;
        };
        if matches!(
            o.shape,
            Shape::Hardscape { .. } | Shape::LandscapeArea { .. }
        ) {
            made.skipped
                .push((id, format!("a {} is an area already", o.shape.kind())));
            continue;
        }
        let outer = match outline_of(o) {
            Ok((outer, _)) => outer,
            Err(why) => {
                made.skipped.push((id, why));
                continue;
            }
        };
        let ring = geom::open_ring(&curve::flatten(&outer, true, TOLERANCE), 1e-9);
        let area = geom::polygon_area(&ring);
        if ring.len() < 3 || area.is_nan() || area <= 0.0 {
            made.skipped.push((id, "the outline has no area".into()));
            continue;
        }
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in &ring {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        items.push(Item {
            id,
            outer,
            ring,
            area,
            lo,
            hi,
        });
    }
    let n = items.len();
    // For each pair: is i inside j, or do they overlap in part?
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut depth = vec![0usize; n];
    let mut dropped = vec![false; n];
    let overlaps = |a: &Item, b: &Item| {
        boxes_meet(a, b) && area_shared(&a.ring, &b.ring) > noise(a.area.min(b.area))
    };
    if opts.holes_from_inner {
        for i in 0..n {
            let mut best: Option<usize> = None;
            for j in 0..n {
                let (a, b) = (&items[i], &items[j]);
                if i == j || b.area <= a.area || !boxes_meet(a, b) {
                    continue;
                }
                if ring_within(&a.ring, &b.ring) {
                    depth[i] += 1;
                    if best.is_none_or(|k| items[k].area > b.area) {
                        best = Some(j);
                    }
                }
            }
            parent[i] = best;
        }
    }
    // Shapes that overlap in part (and are not inside each other) are not made. The smaller one goes.
    for i in 0..n {
        for j in 0..n {
            if i == j || dropped[i] || dropped[j] {
                continue;
            }
            let (a, b) = (&items[i], &items[j]);
            let nested = |x: usize, y: usize| {
                opts.holes_from_inner && ring_within(&items[x].ring, &items[y].ring)
            };
            if (a.area, i) < (b.area, j) && !nested(i, j) && !nested(j, i) && overlaps(a, b) {
                dropped[i] = true;
                made.skipped.push((
                    a.id,
                    format!(
                        "overlaps the shape {} in part, and would make two hardscapes that overlap. Fix the outlines or use holes_from_inner",
                        b.id
                    ),
                ));
            }
        }
    }
    let mut host_holes: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut is_hole = vec![false; n];
    for i in 0..n {
        if dropped[i] {
            continue;
        }
        if depth[i] % 2 == 1
            && let Some(p) = parent[i]
            && !dropped[p]
        {
            if host_holes[p].len() >= MAX_HOLES {
                made.skipped.push((
                    items[i].id,
                    format!("the hardscape has {MAX_HOLES} holes already"),
                ));
                continue;
            }
            host_holes[p].push(i);
            is_hole[i] = true;
        }
    }
    let mut commands = Vec::new();
    let mut next = doc.id_plan();
    let mut used: Vec<ObjectId> = Vec::new();
    for i in 0..n {
        if dropped[i] || is_hole[i] {
            continue;
        }
        let it = &items[i];
        let o = doc.object(it.id).expect("checked");
        // A hole that leaves the outline by noise is cut to it.
        let holes: Vec<Vec<Vertex>> = host_holes[i]
            .iter()
            .map(|&k| {
                let h = &items[k];
                if crate::layout::area_outside(&h.ring, &it.ring) <= 0.0 {
                    h.outer.clone()
                } else {
                    crate::layout::clip_ring_to(&h.ring, &it.ring)
                        .into_iter()
                        .next()
                        .map_or_else(|| h.outer.clone(), |r| curve::corners(&r))
                }
            })
            .collect();
        let patch = HardscapePatch {
            style: opts.style,
            holes: (!holes.is_empty()).then_some(holes),
            ..Default::default()
        };
        commands.push(Command::AddHardscape {
            boundary: Some(it.outer.clone()),
            patch: Box::new(patch),
            layer: Some(o.layer),
            class: Some(o.class),
            attrs: o.attrs.clone(),
        });
        let new_id = ObjectId(next.take());
        // The data of the shape (the properties of an imported entity) goes along.
        let mut data = o.data.clone();
        data.remove(super::data::KEY);
        if !data.is_empty() {
            commands.push(Command::SetData {
                id: new_id,
                set: data,
                remove: Vec::new(),
            });
        }
        made.created.push((it.id, new_id, host_holes[i].len()));
        used.push(it.id);
        for &k in &host_holes[i] {
            made.holes.push((items[k].id, it.id));
            used.push(items[k].id);
        }
    }
    if opts.delete_source && !used.is_empty() {
        commands.push(Command::DeleteObjects { ids: used });
    }
    if !commands.is_empty() {
        made.command = Some(Command::Batch { commands });
    }
    made
}
