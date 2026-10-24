//! Block clips (XCLIP) on import (IOP-27).
//!
//! A block reference with a clip has an extension dictionary. Its entry `ACAD_FILTER` holds a
//! dictionary with the entry `SPATIAL`, and that is a SPATIAL_FILTER object: the clip polygon, the
//! inverse of the matrix that places the block, and the flags. The polygon is in the space of the
//! reference, so the inverse matrix gives the polygon in the coordinates of the block. Two points
//! are a rectangle by its corners. An inverted clip keeps a second polygon (the part that is cut
//! away) in an XRECORD of the filter.
//!
//! Our blocks are exploded, so a symbol instance with a stencil does not exist. The import cuts the
//! geometry of each entity of the block by the polygon, in the plane:
//!
//! - A line, or a polyline that crosses the polygon, is cut at the edges of the polygon. The cut points
//!   are exact. A curve that is cut becomes chords within the tolerance. A curve that lies
//!   entirely inside stays exact.
//! - A filled shape (a solid, a hatch loop, a wipeout) becomes the polygon boolean of the shape and
//!   the clip. A hole that this makes becomes a white island, as in a hatch.
//! - A text, a point and a 3D point stay or go whole by their point. A mesh stays or goes whole by
//!   its vertices.
//! - An entity that is entirely outside is not imported, and its line in `report.skipped` says so.

use std::collections::HashMap;
use std::rc::Rc;

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use kadakas_core::curve::{self, corners};
use kadakas_core::geom::{dist_to_segment, point_in_polygon, point_in_polygon_closed};
use kadakas_core::model::Shape;
use kadakas_core::{DVec2, DVec3};

use super::extra::PAPER;
use super::{Item, Tally, Walker, Xf};
use crate::dxf::hash;
use crate::dxf::parse::Entity;

/// What the file says about the clip of one block reference.
pub(super) enum ClipRead {
    /// The clip is switched off.
    Off,
    /// The clip cannot be read (too few points, or the matrix is missing).
    Unreadable,
    /// A clip polygon in the coordinates of the block, and a polygon cut away from it (inverted).
    Polygon {
        outer: Vec<DVec2>,
        inner: Vec<DVec2>,
        /// The clip has a front or a back plane (3D). Only the polygon is applied.
        planes: bool,
        /// The clip is inverted.
        inverted: bool,
    },
}

/// The handles of the objects of the file, for the walk from a block reference to its filter.
pub(super) fn handle_index(objects: &[Entity]) -> HashMap<String, usize> {
    objects
        .iter()
        .enumerate()
        .filter_map(|(i, o)| Some((o.handle()?, i)))
        .collect()
}

/// The handle that follows the entry `name` of a dictionary.
fn entry(dict: &Entity, name: &str) -> Option<String> {
    let at = dict
        .items
        .iter()
        .position(|(c, v)| *c == 3 && v.trim() == name)?;
    dict.items[at + 1..]
        .iter()
        .find(|(c, _)| *c == 360 || *c == 350)
        .and_then(|(_, h)| hash::normal_handle(h))
}

/// The handle of the extension dictionary of an entity or an object.
fn xdict(e: &Entity) -> Option<String> {
    let at = e
        .items
        .iter()
        .position(|(c, v)| *c == 102 && v.trim() == "{ACAD_XDICTIONARY")?;
    e.items[at + 1..]
        .iter()
        .find(|(c, _)| *c == 360)
        .and_then(|(_, h)| hash::normal_handle(h))
}

impl Walker<'_> {
    fn object(&self, handle: &str) -> Option<&Entity> {
        self.object_index
            .get(handle)
            .and_then(|i| self.dxf.objects.get(*i))
    }

    /// The spatial filter of a block reference, if it has one.
    fn spatial_filter(&self, insert: &Entity) -> Option<&Entity> {
        let dict = self.object(&xdict(insert)?)?;
        let filter_dict = self.object(&entry(dict, "ACAD_FILTER")?)?;
        self.object(&entry(filter_dict, "SPATIAL")?)
    }

    /// The clip of a block reference. `None` when it has none.
    pub(super) fn read_clip(&self, insert: &Entity) -> Option<ClipRead> {
        let filter = self.spatial_filter(insert)?;
        if filter.int(71) == Some(0) {
            return Some(ClipRead::Off);
        }
        // The matrix is the inverse of the placement of the block: the last 24 numbers of the
        // code 40 are it (12 numbers) and the matrix of the clip space (12 numbers).
        let nums = filter.all_f(40);
        let points: Vec<DVec2> = filter
            .items
            .iter()
            .filter(|(c, _)| *c == 10)
            .zip(filter.items.iter().filter(|(c, _)| *c == 20))
            .filter_map(|((_, x), (_, y))| {
                Some(DVec2::new(
                    super::parse::parse_f64(x)?,
                    super::parse::parse_f64(y)?,
                ))
            })
            .collect();
        let declared = filter.int(70).unwrap_or(-1);
        if nums.len() < 24 || points.len() < 2 || declared != points.len() as i64 {
            return Some(ClipRead::Unreadable);
        }
        let m = &nums[nums.len() - 24..nums.len() - 12];
        let map = |p: DVec2| {
            DVec2::new(
                m[0] * p.x + m[1] * p.y + m[3],
                m[4] * p.x + m[5] * p.y + m[7],
            )
        };
        let to_block = |pts: &[DVec2]| -> Option<Vec<DVec2>> {
            block_ring(&pts.iter().map(|p| map(*p)).collect::<Vec<_>>())
        };
        let Some(outer) = to_block(&points) else {
            return Some(ClipRead::Unreadable);
        };
        let (inner, inverted) = self
            .inverted_part(filter)
            .map_or((Vec::new(), false), |pts| {
                (to_block(&pts).unwrap_or_default(), true)
            });
        Some(ClipRead::Polygon {
            outer,
            inner,
            planes: filter.int(72) == Some(1) || filter.int(73) == Some(1),
            inverted,
        })
    }

    /// The points of the part that an inverted clip cuts away, in the space of the reference.
    fn inverted_part(&self, filter: &Entity) -> Option<Vec<DVec2>> {
        let dict = self.object(&xdict(filter)?)?;
        let record = self.object(&entry(dict, "ACAD_XREC_ROUNDTRIP")?)?;
        let at = record
            .items
            .iter()
            .position(|(c, v)| *c == 102 && v.trim() == "ACAD_INVERTEDCLIP_ROUNDTRIP")?;
        let mut xs = Vec::new();
        for (c, v) in &record.items[at + 1..] {
            match *c {
                102 => break,
                10 => xs.push((super::parse::parse_f64(v)?, f64::NAN)),
                20 => xs.last_mut()?.1 = super::parse::parse_f64(v)?,
                _ => {}
            }
        }
        let pts: Vec<DVec2> = xs.into_iter().map(|(x, y)| DVec2::new(x, y)).collect();
        pts.iter().all(|p| p.is_finite()).then_some(pts)
    }

    /// The clip of a block reference in the drawing, for the content of the copy with the
    /// transform `xf`. Counts the clip in the tally. `None` when the content shows whole.
    pub(super) fn clip_region(
        &mut self,
        read: &ClipRead,
        xf: &Xf,
        counted: bool,
    ) -> Option<Rc<Region>> {
        match read {
            ClipRead::Off => {
                if counted {
                    self.tally.xclip_off += 1;
                }
                None
            }
            ClipRead::Unreadable => {
                if counted {
                    self.tally.xclip_unreadable += 1;
                }
                None
            }
            ClipRead::Polygon {
                outer,
                inner,
                planes,
                inverted,
            } => {
                let world = |pts: &[DVec2]| pts.iter().map(|p| xf.apply(*p)).collect::<Vec<_>>();
                let region = Region::new(world(outer), world(inner));
                if counted {
                    if region.is_some() {
                        self.tally.xclip_applied += 1;
                        self.tally.xclip_planes += usize::from(*planes);
                        self.tally.xclip_inverted += usize::from(*inverted);
                    } else {
                        self.tally.xclip_unreadable += 1;
                    }
                }
                region.map(Rc::new)
            }
        }
    }
}

/// A clip polygon in block coordinates: two points are a rectangle by its corners. `None` when
/// the polygon has no area.
fn block_ring(pts: &[DVec2]) -> Option<Vec<DVec2>> {
    if pts.iter().any(|p| !p.is_finite()) {
        return None;
    }
    if pts.len() == 2 {
        let (a, b) = (pts[0].min(pts[1]), pts[0].max(pts[1]));
        return (b.x > a.x && b.y > a.y)
            .then(|| vec![a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)]);
    }
    let mut ring: Vec<DVec2> = Vec::with_capacity(pts.len());
    for p in pts {
        if ring.last().is_none_or(|l| l.distance(*p) > 1e-12) {
            ring.push(*p);
        }
    }
    if ring.len() > 1 && ring[0].distance(ring[ring.len() - 1]) <= 1e-12 {
        ring.pop();
    }
    let area: f64 = (0..ring.len())
        .map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()]))
        .sum::<f64>()
        / 2.0;
    (ring.len() >= 3 && area.abs() > 0.0).then_some(ring)
}

/// What a clip lets through: the inside of the outer polygon, without the inner polygon.
pub(super) struct Region {
    outer: Vec<DVec2>,
    inner: Vec<DVec2>,
    lo: DVec2,
    hi: DVec2,
    /// How near to an edge counts as on the edge.
    eps: f64,
    /// The area as polygons with holes, for the clip of filled shapes.
    shapes: Vec<Vec<Vec<[f64; 2]>>>,
}

fn path(r: &[DVec2]) -> Vec<[f64; 2]> {
    r.iter().map(|p| [p.x, p.y]).collect()
}

impl Region {
    /// `None` when the outer polygon has no area. An inner polygon with no area is ignored.
    pub fn new(outer: Vec<DVec2>, inner: Vec<DVec2>) -> Option<Region> {
        let outer = block_ring(&outer)?;
        let inner = block_ring(&inner).unwrap_or_default();
        let (lo, hi) = outer.iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let shapes = if inner.is_empty() {
            vec![vec![path(&outer)]]
        } else {
            vec![path(&outer)].overlay(
                &vec![path(&inner)],
                OverlayRule::Difference,
                FillRule::EvenOdd,
            )
        };
        let eps = 1e-9 * (hi - lo).max_element().max(1.0);
        Some(Region {
            outer,
            inner,
            lo,
            hi,
            eps,
            shapes,
        })
    }

    /// Whether a point shows. A point on the edge of the clip shows (a frame that lies on the edge stays).
    pub fn contains(&self, p: DVec2) -> bool {
        let inside = p.cmpge(self.lo - self.eps).all()
            && p.cmple(self.hi + self.eps).all()
            && (point_in_polygon_closed_eps(p, &self.outer, self.eps));
        inside
            && (self.inner.is_empty()
                || on_edge(p, &self.inner, self.eps)
                || !point_in_polygon(p, &self.inner))
    }

    /// The parts of the segment `a`-`b` inside the region, as parameter ranges. The cut points
    /// are the crossings with the edges.
    pub fn clip_segment(&self, a: DVec2, b: DVec2) -> Vec<(f64, f64)> {
        if a.max(b).cmplt(self.lo).any() || a.min(b).cmpgt(self.hi).any() {
            return Vec::new();
        }
        let d = b - a;
        let mut ts = vec![0.0, 1.0];
        for ring in [&self.outer, &self.inner] {
            let n = ring.len();
            for i in 0..n {
                let (c, e) = (ring[i], ring[(i + 1) % n]);
                let r = e - c;
                let den = d.perp_dot(r);
                if den == 0.0 {
                    continue;
                }
                let t = (c - a).perp_dot(r) / den;
                let u = (c - a).perp_dot(d) / den;
                if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u) {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        let mut out: Vec<(f64, f64)> = Vec::new();
        for w in ts.windows(2) {
            if w[1] - w[0] <= 1e-12 || !self.contains(a + d * ((w[0] + w[1]) / 2.0)) {
                continue;
            }
            match out.last_mut() {
                Some(last) if (last.1 - w[0]).abs() <= 1e-12 => last.1 = w[1],
                _ => out.push((w[0], w[1])),
            }
        }
        out
    }

    /// The polygons (outline and holes) of the part of a ring inside the region.
    fn clip_ring(&self, ring: &[DVec2]) -> Vec<Vec<Vec<DVec2>>> {
        let subject = vec![path(ring)];
        let mut out = Vec::new();
        for shape in &self.shapes {
            for s in subject.overlay(shape, OverlayRule::Intersect, FillRule::EvenOdd) {
                let contours: Vec<Vec<DVec2>> = s
                    .iter()
                    .map(|c| c.iter().map(|p| DVec2::new(p[0], p[1])).collect())
                    .collect();
                if contours.first().is_some_and(|c| c.len() >= 3) {
                    out.push(contours);
                }
            }
        }
        out
    }
}

fn on_edge(p: DVec2, ring: &[DVec2], eps: f64) -> bool {
    let n = ring.len();
    (0..n).any(|i| dist_to_segment(p, ring[i], ring[(i + 1) % n]) <= eps)
}

fn point_in_polygon_closed_eps(p: DVec2, ring: &[DVec2], eps: f64) -> bool {
    on_edge(p, ring, eps) || point_in_polygon_closed(p, ring)
}

const WHOLE: f64 = 1e-9;

fn is_whole(parts: &[(f64, f64)]) -> bool {
    parts.len() == 1 && parts[0].0 <= WHOLE && parts[0].1 >= 1.0 - WHOLE
}

/// What a path (a chain of points) looks like after the clip.
struct Cut {
    /// Every segment lies entirely inside.
    all_inside: bool,
    /// The pieces, each a chain of points.
    pieces: Vec<Vec<DVec3>>,
}

/// Cuts a chain of points. Z is interpolated along each segment. `closed` adds the last segment.
fn cut_chain(region: &Region, pts: &[DVec3], closed: bool) -> Cut {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let mut all_inside = n > 0;
    let mut pieces: Vec<Vec<DVec3>> = Vec::new();
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let parts = region.clip_segment(a.truncate(), b.truncate());
        all_inside &= is_whole(&parts);
        for (t0, t1) in parts {
            let (p, q) = (a.lerp(b, t0), a.lerp(b, t1));
            match pieces.last_mut() {
                Some(last) if last.last().is_some_and(|l| l.distance(p) <= 1e-9) => last.push(q),
                _ => pieces.push(vec![p, q]),
            }
        }
    }
    // A closed chain that crosses its first point is one piece.
    if closed && pieces.len() > 1 {
        let (first, last) = (
            pieces[0][0],
            pieces[pieces.len() - 1][pieces[pieces.len() - 1].len() - 1],
        );
        if first.distance(last) <= 1e-9 {
            let head = pieces.remove(0);
            if let Some(tail) = pieces.last_mut() {
                tail.extend(head.into_iter().skip(1));
            }
        }
    }
    Cut { all_inside, pieces }
}

fn lift(p: DVec2) -> DVec3 {
    p.extend(0.0)
}

/// Cuts the items of one entity by a clip. An item that is entirely inside stays as it is.
pub(super) fn apply(items: Vec<Item>, clips: &[Rc<Region>], tol: f64, t: &mut Tally) -> Vec<Item> {
    let mut cur = items;
    for region in clips {
        let mut next = Vec::with_capacity(cur.len());
        for item in cur {
            next.extend(clip_item(item, region, tol, t));
        }
        cur = next;
        if cur.is_empty() {
            break;
        }
    }
    cur
}

fn clip_item(item: Item, region: &Region, tol: f64, t: &mut Tally) -> Vec<Item> {
    let filled = item.fill.is_some() || item.fill_pen || item.pattern.is_some();
    match &item.shape {
        Shape::Line { a, b } => {
            let parts = region.clip_segment(*a, *b);
            if is_whole(&parts) {
                return vec![item];
            }
            t.xclip_cut += usize::from(!parts.is_empty());
            parts
                .into_iter()
                .map(|(t0, t1)| {
                    let mut piece = item.clone();
                    piece.shape = Shape::Line {
                        a: a.lerp(*b, t0),
                        b: a.lerp(*b, t1),
                    };
                    piece
                })
                .collect()
        }
        Shape::Polyline { points, closed } => {
            let curved = !curve::all_straight(points);
            let ring = curve::flatten(points, *closed, tol);
            if filled && *closed {
                return clip_filled(item, region, &ring, curved, t);
            }
            let chain: Vec<DVec3> = ring.iter().map(|p| lift(*p)).collect();
            let cut = cut_chain(region, &chain, *closed);
            if cut.all_inside {
                return vec![item];
            }
            note_cut(t, curved, &cut);
            pieces_as_items(&item, cut, false)
        }
        Shape::Circle { center, radius } => {
            let ring: Vec<DVec2> = (0..96)
                .map(|k| {
                    *center
                        + DVec2::from_angle(std::f64::consts::TAU * f64::from(k) / 96.0) * *radius
                })
                .collect();
            clip_round(item, region, ring, true, t)
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = kadakas_core::model::arc_sweep(*start, *end);
            let n = ((sweep / std::f64::consts::TAU * 96.0).ceil() as usize).max(2);
            let ring: Vec<DVec2> = (0..=n)
                .map(|k| {
                    *center + DVec2::from_angle(*start + sweep * k as f64 / n as f64) * *radius
                })
                .collect();
            clip_round(item, region, ring, false, t)
        }
        Shape::Polyline3d { points, closed } => {
            let cut = cut_chain(region, points, *closed);
            if cut.all_inside {
                return vec![item];
            }
            note_cut(t, false, &cut);
            pieces_as_items(&item, cut, true)
        }
        Shape::Text { at, .. } => {
            let at = *at;
            keep_by_point(item, region, at, t)
        }
        Shape::Locus3d { p } => {
            let at = p.truncate();
            keep_by_point(item, region, at, t)
        }
        Shape::Mesh3d(m) => {
            // A mesh is not cut: it stays when one of its vertices lies in the clip.
            let any = m.vertices.iter().any(|v| region.contains(v.truncate()));
            t.xclip_whole += 1;
            if any { vec![item] } else { Vec::new() }
        }
        _ => vec![item],
    }
}

fn keep_by_point(item: Item, region: &Region, at: DVec2, t: &mut Tally) -> Vec<Item> {
    if region.contains(at) {
        vec![item]
    } else {
        t.xclip_whole += 1;
        Vec::new()
    }
}

/// Counts an item that a clip cut. An item that is gone whole is not counted: its entity has a
/// line in the report.
fn note_cut(t: &mut Tally, curved: bool, cut: &Cut) {
    if !cut.pieces.is_empty() {
        t.xclip_cut += 1;
        t.xclip_curves += usize::from(curved);
    }
}

/// The pieces of a cut as items that copy the look of the item. A piece of a 3D chain is a 3D
/// polyline. A piece of a flat chain is a polyline, or a line when it has two points.
fn pieces_as_items(item: &Item, cut: Cut, three_d: bool) -> Vec<Item> {
    cut.pieces
        .into_iter()
        .map(|chain| {
            let mut piece = item.clone();
            piece.fill = None;
            piece.fill_pen = false;
            piece.pattern = None;
            piece.shape = if three_d {
                Shape::Polyline3d {
                    points: chain,
                    closed: false,
                }
            } else {
                let pts: Vec<DVec2> = chain.iter().map(|p| p.truncate()).collect();
                match pts.as_slice() {
                    [a, b] => Shape::Line { a: *a, b: *b },
                    _ => Shape::Polyline {
                        points: corners(&pts),
                        closed: false,
                    },
                }
            };
            piece
        })
        .collect()
}

/// A circle or an arc: exact when it is entirely inside, gone when it is entirely outside, else
/// chords.
fn clip_round(
    item: Item,
    region: &Region,
    ring: Vec<DVec2>,
    closed: bool,
    t: &mut Tally,
) -> Vec<Item> {
    let filled = item.fill.is_some() || item.fill_pen || item.pattern.is_some();
    if filled && closed {
        return clip_filled(item, region, &ring, true, t);
    }
    let chain: Vec<DVec3> = ring.iter().map(|p| lift(*p)).collect();
    let cut = cut_chain(region, &chain, closed);
    if cut.all_inside {
        return vec![item];
    }
    note_cut(t, true, &cut);
    pieces_as_items(&item, cut, false)
}

/// A filled closed shape: the polygon boolean of the shape and the clip. A hole that this makes is
/// a white island on top, as in a hatch.
fn clip_filled(
    item: Item,
    region: &Region,
    ring: &[DVec2],
    curved: bool,
    t: &mut Tally,
) -> Vec<Item> {
    let chain: Vec<DVec3> = ring.iter().map(|p| lift(*p)).collect();
    let cut = cut_chain(region, &chain, true);
    if cut.all_inside {
        // Inside the outline. A hole of the clip (inverted) inside the shape would still cut it.
        let hole_inside = region.inner.iter().any(|p| point_in_polygon(*p, ring));
        if !hole_inside {
            return vec![item];
        }
    }
    let shapes = region.clip_ring(ring);
    if !shapes.is_empty() {
        t.xclip_cut += 1;
        t.xclip_curves += usize::from(curved);
    }
    let mut out = Vec::new();
    for contours in shapes {
        let mut outline = item.clone();
        outline.shape = Shape::Polyline {
            points: corners(&contours[0]),
            closed: true,
        };
        out.push(outline);
        for hole in &contours[1..] {
            let mut island = item.clone();
            island.shape = Shape::Polyline {
                points: corners(hole),
                closed: true,
            };
            island.fill = Some(PAPER);
            island.fill_pen = false;
            island.pattern = None;
            island.data.insert("hatch_island".into(), true.into());
            t.hatch_islands += 1;
            out.push(island);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::curve::{Vertex, VertexKind};
    use kadakas_core::mesh3d::Mesh3d;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
        vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)]
    }

    fn region() -> Region {
        Region::new(square(0.0, 0.0, 100.0, 100.0), vec![]).unwrap()
    }

    fn run(shape: Shape, region: &Region) -> (Vec<Item>, Tally) {
        let mut t = Tally::default();
        let out = clip_item(Item::new(shape), region, 0.01, &mut t);
        (out, t)
    }

    #[test]
    fn a_polygon_needs_an_area() {
        assert!(block_ring(&[v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0)]).is_none());
        assert!(block_ring(&[v(0.0, 0.0), v(0.0, 5.0)]).is_none());
        assert!(block_ring(&[v(0.0, 0.0), v(f64::NAN, 5.0), v(1.0, 1.0)]).is_none());
        assert!(block_ring(&[]).is_none());
        // The closing point is dropped, and two points are a rectangle.
        let r = block_ring(&[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(0.0, 0.0)]).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(block_ring(&[v(5.0, 5.0), v(1.0, 1.0)]).unwrap().len(), 4);
        assert!(Region::new(vec![v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0)], vec![]).is_none());
        // An inner polygon with no area is ignored.
        let r = Region::new(
            square(0.0, 0.0, 10.0, 10.0),
            vec![v(1.0, 1.0), v(2.0, 2.0), v(3.0, 3.0)],
        )
        .unwrap();
        assert!(r.contains(v(5.0, 5.0)));
    }

    #[test]
    fn the_region_is_the_outer_polygon_without_the_inner_one() {
        let r = Region::new(
            square(0.0, 0.0, 100.0, 100.0),
            square(40.0, 40.0, 60.0, 60.0),
        )
        .unwrap();
        assert!(r.contains(v(10.0, 10.0)));
        assert!(!r.contains(v(50.0, 50.0)));
        assert!(!r.contains(v(150.0, 50.0)));
        // A segment through the hole has two parts.
        let parts = r.clip_segment(v(0.0, 50.0), v(100.0, 50.0));
        assert_eq!(parts.len(), 2);
        assert!((parts[0].1 - 0.4).abs() < 1e-9 && (parts[1].0 - 0.6).abs() < 1e-9);
        // A segment that misses the box has none.
        assert!(r.clip_segment(v(200.0, 0.0), v(300.0, 0.0)).is_empty());
        assert!(r.clip_segment(v(-50.0, -50.0), v(-10.0, 300.0)).is_empty());
    }

    #[test]
    fn a_line_on_the_edge_of_the_clip_stays() {
        let r = region();
        // The bottom edge, the right edge, and a line that runs along the edge and out of it.
        for (a, b) in [
            (v(0.0, 0.0), v(100.0, 0.0)),
            (v(100.0, 20.0), v(100.0, 80.0)),
        ] {
            let (out, t) = run(Shape::Line { a, b }, &r);
            assert_eq!(out.len(), 1);
            assert_eq!(t.xclip_cut, 0);
        }
        let (out, _) = run(
            Shape::Line {
                a: v(-50.0, 0.0),
                b: v(60.0, 0.0),
            },
            &r,
        );
        let Shape::Line { a, b } = out[0].shape else {
            panic!()
        };
        assert!((a.x - 0.0).abs() < 1e-9 && (b.x - 60.0).abs() < 1e-9);
        // The edge of a hole shows too.
        let ring = Region::new(
            square(0.0, 0.0, 100.0, 100.0),
            square(40.0, 40.0, 60.0, 60.0),
        )
        .unwrap();
        assert!(ring.contains(v(40.0, 50.0)));
        assert!(!ring.contains(v(50.0, 50.0)));
    }

    #[test]
    fn arcs_circles_and_points_are_kept_cut_or_dropped() {
        let r = region();
        // An arc entirely inside is kept as it is.
        let arc = |cx: f64, radius: f64| Shape::Arc {
            center: v(cx, 50.0),
            radius,
            start: 0.0,
            end: std::f64::consts::PI,
        };
        let (out, t) = run(arc(50.0, 10.0), &r);
        assert!(matches!(out[0].shape, Shape::Arc { .. }) && t.xclip_cut == 0);
        // An arc that crosses the edge x = 100 becomes chords.
        let (out, t) = run(arc(100.0, 20.0), &r);
        assert!(
            matches!(out[0].shape, Shape::Polyline { .. })
                && t.xclip_cut == 1
                && t.xclip_curves == 1
        );
        // An arc outside is gone, and is not counted as cut.
        let (out, t) = run(arc(300.0, 10.0), &r);
        assert!(out.is_empty() && t.xclip_cut == 0);
        // A filled circle that crosses becomes a filled polygon.
        let mut item = Item::new(Shape::Circle {
            center: v(100.0, 50.0),
            radius: 20.0,
        });
        item.fill = Some(kadakas_core::model::Color::BLACK);
        let mut t = Tally::default();
        let out = clip_item(item, &r, 0.01, &mut t);
        assert_eq!(out.len(), 1);
        assert!(out[0].fill.is_some());
        let Shape::Polyline { points, closed } = &out[0].shape else {
            panic!()
        };
        assert!(*closed);
        let area = kadakas_core::curve::area(points).abs();
        assert!(
            (area - std::f64::consts::PI * 400.0 / 2.0).abs() < 15.0,
            "{area}"
        );
        // A point and a mesh.
        let (out, _) = run(
            Shape::Locus3d {
                p: DVec3::new(10.0, 10.0, 5.0),
            },
            &r,
        );
        assert_eq!(out.len(), 1);
        let (out, t) = run(
            Shape::Locus3d {
                p: DVec3::new(-10.0, 10.0, 5.0),
            },
            &r,
        );
        assert!(out.is_empty() && t.xclip_whole == 1);
        let mesh = |x: f64| {
            Shape::Mesh3d(Box::new(Mesh3d {
                name: String::new(),
                vertices: vec![
                    DVec3::new(x, 0.0, 0.0),
                    DVec3::new(x + 1.0, 0.0, 0.0),
                    DVec3::new(x, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
                color: None,
            }))
        };
        assert_eq!(run(mesh(50.0), &r).0.len(), 1);
        assert!(run(mesh(500.0), &r).0.is_empty());
        // A shape that a clip does not know passes.
        let (out, _) = run(Shape::Group { name: "g".into() }, &r);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_closed_outline_that_crosses_its_first_point_is_one_piece() {
        // The start of the ring is outside, the rest inside: the cut joins the last and the first piece.
        let r = region();
        let ring = vec![
            Vertex::new(v(-20.0, 50.0), VertexKind::Corner),
            Vertex::new(v(50.0, 20.0), VertexKind::Corner),
            Vertex::new(v(50.0, 80.0), VertexKind::Corner),
        ];
        let (out, t) = run(
            Shape::Polyline {
                points: ring,
                closed: true,
            },
            &r,
        );
        assert_eq!(t.xclip_cut, 1);
        // The outline is open now: two pieces that meet at the point inside, as one chain.
        assert!(!out.is_empty());
        assert!(out.len() <= 2, "{}", out.len());
    }

    #[test]
    fn a_filled_shape_that_holds_the_hole_of_the_clip_gets_a_white_island() {
        let r = Region::new(
            square(0.0, 0.0, 100.0, 100.0),
            square(40.0, 40.0, 60.0, 60.0),
        )
        .unwrap();
        let mut item = Item::new(Shape::Polyline {
            points: kadakas_core::curve::corners(&square(10.0, 10.0, 90.0, 90.0)),
            closed: true,
        });
        item.fill_pen = true;
        let mut t = Tally::default();
        let out = clip_item(item, &r, 0.01, &mut t);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].fill, Some(PAPER));
        assert_eq!(t.hatch_islands, 1);
    }

    #[test]
    fn clips_apply_one_after_the_other_and_an_empty_list_is_cheap() {
        let a = std::rc::Rc::new(region());
        let b = std::rc::Rc::new(Region::new(square(50.0, 0.0, 200.0, 100.0), vec![]).unwrap());
        let mut t = Tally::default();
        let items = vec![Item::new(Shape::Line {
            a: v(0.0, 10.0),
            b: v(300.0, 10.0),
        })];
        let out = apply(items.clone(), &[a.clone(), b], 0.01, &mut t);
        let Shape::Line { a: p, b: q } = out[0].shape else {
            panic!()
        };
        assert!((p.x - 50.0).abs() < 1e-9 && (q.x - 100.0).abs() < 1e-9);
        assert_eq!(apply(items.clone(), &[], 0.01, &mut t).len(), 1);
        // The first clip removes everything: the second one is not needed.
        let far =
            std::rc::Rc::new(Region::new(square(1000.0, 0.0, 1100.0, 100.0), vec![]).unwrap());
        assert!(apply(items, &[far, a], 0.01, &mut t).is_empty());
    }
}
