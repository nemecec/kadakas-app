//! Turns closed shapes into hardscapes in one step, for example the outlines of a parking lot
//! that came in from a DXF file. A shape that lies inside another one becomes a hole of it.

use glam::DVec2;

use super::HardscapePatch;
use super::data::MAX_HOLES;
use crate::command::Command;
use crate::curve::{self, Vertex};
use crate::geom;
use crate::layout::object::outline_of;
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
    /// Delete the shapes that were used.
    pub delete_source: bool,
}

impl Default for FromShapesOptions {
    fn default() -> Self {
        FromShapesOptions {
            style: None,
            holes_from_inner: true,
            delete_source: true,
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

fn inside_or_on(ring: &[DVec2], p: DVec2) -> bool {
    let n = ring.len();
    geom::point_in_polygon(p, ring)
        || (0..n).any(|i| geom::dist_to_segment(p, ring[i], ring[(i + 1) % n]) < 1e-6)
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
        let ring = curve::flatten(&outer, true, TOLERANCE);
        let area = geom::polygon_area(&ring);
        if ring.len() < 3 || !(area > 0.0) {
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
    // The smallest shape that holds each shape, and how deep the shape lies.
    let n = items.len();
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut depth = vec![0usize; n];
    if opts.holes_from_inner {
        for i in 0..n {
            let mut best: Option<usize> = None;
            for j in 0..n {
                let (a, b) = (&items[i], &items[j]);
                if i == j
                    || b.area <= a.area
                    || a.lo.x < b.lo.x
                    || a.lo.y < b.lo.y
                    || a.hi.x > b.hi.x
                    || a.hi.y > b.hi.y
                    || !a.ring.iter().all(|p| inside_or_on(&b.ring, *p))
                {
                    continue;
                }
                depth[i] += 1;
                if best.is_none_or(|k| items[k].area > b.area) {
                    best = Some(j);
                }
            }
            parent[i] = best;
        }
    }
    let mut host_holes: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        if depth[i] % 2 == 1
            && let Some(p) = parent[i]
        {
            if host_holes[p].len() >= MAX_HOLES {
                made.skipped.push((
                    items[i].id,
                    format!("the hardscape has {MAX_HOLES} holes already"),
                ));
                continue;
            }
            host_holes[p].push(i);
        }
    }
    let mut commands = Vec::new();
    let mut next = doc.id_plan();
    let mut used: Vec<ObjectId> = Vec::new();
    for i in 0..n {
        let it = &items[i];
        if host_holes.iter().any(|h| h.contains(&i)) {
            continue;
        }
        let o = doc.object(it.id).expect("checked");
        let holes: Vec<Vec<Vertex>> = host_holes[i]
            .iter()
            .map(|&k| items[k].outer.clone())
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
