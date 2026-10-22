//! The hole through a slab (MOD-08): a face drawn on one side of a slab, pushed exactly to the
//! opposite side, cuts a hole through.
//!
//! The push is a pocket whose floor (the cap) reaches the plane of a face `G` that looks the
//! other way (the bottom of the slab under its top). When the cap lies fully inside `G` (inside
//! its outer loop, farther than the length tolerance from its border, and no hole of `G` meets
//! it), the cap goes: `G` gets the outer loop of the cap as a new hole, the walls of the pocket
//! are the walls of the hole, and each hole of the pushed face (a ring pushed through) leaves an
//! island face in `G` (a column stands in the hole). The result is closed when the slab was.
//!
//! Refused with a reason: a cap that reaches the plane of such a face and overlaps it, but is not
//! fully inside it (it touches or crosses its border or one of its holes, or it lies across two
//! faces of the other side: erase the edge between them first). A push past the opposite face is
//! refused by the usual checks (it cuts through other faces).
//!
//! Only a push to the back of the face (a negative distance, into the solid) of a face that has
//! neighbours (a pocket) cuts a hole. A distance within the length tolerance of the opposite face
//! goes exactly there.

use std::collections::BTreeSet;

use glam::{DVec2, DVec3};

use super::super::super::clash::Shape;
use super::super::super::geom::{self, on_plane};
use super::super::super::{FaceId, SurfaceError, SurfaceModel, VertexId, key};

/// The face that a push of `face` by `d` cuts a hole through, and the exact distance to it, or
/// `None` when the push is no hole through. An error when the cap reaches the opposite face but is
/// not fully inside one face.
pub(super) fn target(
    m: &SurfaceModel,
    face: FaceId,
    d: f64,
) -> Result<Option<(FaceId, f64)>, SurfaceError> {
    let Some(f) = m.faces.get(&face) else {
        return Ok(None);
    };
    if !d.is_finite() || d >= 0.0 {
        return Ok(None);
    }
    let n = f.plane.normal;
    let p0 = m.vertices[&f.outer[0]];
    // A face with no neighbour is a lone sketch: it becomes a solid, never a hole.
    let mine: BTreeSet<(VertexId, VertexId)> = f.uses().map(|(a, b)| key(a, b)).collect();
    let attached = m
        .faces
        .iter()
        .any(|(id, g)| *id != face && g.uses().any(|(a, b)| mine.contains(&key(a, b))));
    if !attached {
        return Ok(None);
    }
    let tol = geom::tol(&[p0, p0 + n * d]);
    let mut found: Option<(FaceId, f64)> = None;
    for (&gid, g) in &m.faces {
        if gid == face || g.plane.normal.dot(n) > -(1.0 - 1e-9) {
            continue;
        }
        let s = (m.vertices[&g.outer[0]] - p0).dot(n);
        if (s - d).abs() > tol || s >= 0.0 {
            continue;
        }
        // The cap at the exact distance, in the axes of `G`.
        let lift = |v: &VertexId| g.plane.project(m.vertices[v] + n * s);
        if !f
            .loops()
            .flatten()
            .all(|v| on_plane(&g.plane, m.vertices[v] + n * s))
        {
            continue;
        }
        let cap: Vec<DVec2> = f.outer.iter().map(lift).collect();
        let shape = Shape::new(m, g);
        let cap3: Vec<DVec3> = f.outer.iter().map(|v| m.vertices[v] + n * s).collect();
        let margin = geom::tol(&cap3);
        match fit(&cap, &shape, margin) {
            Fit::Apart => {}
            Fit::Inside => {
                if found.is_none() {
                    found = Some((gid, s));
                }
            }
            Fit::Overlaps => {
                return Err(SurfaceError::Unsolved(format!(
                    "face {face} reaches the opposite face {gid} but is not fully inside it (it \
                     touches or crosses its border or a hole, or lies across two faces): a hole \
                     through needs the whole face inside one face of the other side, away from \
                     its edges"
                )));
            }
        }
    }
    // A face back to back with the face of the other side under the hole (a solid that stands
    // under the slab): the walls would end on it.
    if let Some((gid, s)) = found {
        let g = &m.faces[&gid];
        let cap3: Vec<DVec3> = f.outer.iter().map(|v| m.vertices[v] + n * s).collect();
        let cap: Vec<DVec2> = cap3.iter().map(|p| g.plane.project(*p)).collect();
        let margin = geom::tol(&cap3);
        for (&hid, h) in &m.faces {
            if hid == gid
                || hid == face
                || h.plane.normal.dot(g.plane.normal) > -(1.0 - 1e-9)
                || !on_plane(&g.plane, m.vertices[&h.outer[0]])
            {
                continue;
            }
            if fit(&cap, &Shape::new(m, h), margin) != Fit::Apart {
                return Err(SurfaceError::Unsolved(format!(
                    "face {face} cannot be pushed through: face {hid} lies back to back under the \
                     hole (another solid stands under the slab there)"
                )));
            }
        }
    }
    Ok(found)
}

/// How the ring of the cap lies on a face of the other side.
#[derive(Debug, PartialEq)]
enum Fit {
    Apart,
    Inside,
    Overlaps,
}

fn fit(cap: &[DVec2], g: &Shape, margin: f64) -> Fit {
    let n = cap.len();
    let sides: Vec<(DVec2, DVec2)> = (0..n).map(|i| (cap[i], cap[(i + 1) % n])).collect();
    let crosses = sides.iter().any(|&(a, b)| {
        let pad = DVec2::splat(margin);
        let mut hit = false;
        g.sides_in(a.min(b) - pad, a.max(b) + pad, &mut |c, e| {
            hit |= segments_meet(a, b, c, e, margin);
        });
        hit
    });
    let ring = [cap.to_vec()];
    let all_in = cap.iter().all(|p| g.strictly_inside(*p, margin));
    // A loop of `G` (its outer loop or a hole) inside the cap.
    let holds_loop = g.rings.iter().any(|r| {
        r.first()
            .is_some_and(|p| super::super::arrange::inside(*p, &ring))
    });
    if all_in && !crosses && !holds_loop {
        return Fit::Inside;
    }
    let any_in = cap.iter().any(|p| g.in_closure(*p, margin));
    if any_in || crosses || holds_loop {
        Fit::Overlaps
    } else {
        Fit::Apart
    }
}

/// True if the segments `a`–`b` and `c`–`e` cross or come within `margin` of each other.
fn segments_meet(a: DVec2, b: DVec2, c: DVec2, e: DVec2, margin: f64) -> bool {
    let z = |p: DVec2| DVec3::new(p.x, p.y, 0.0);
    match geom::segment_segment(z(a), z(b), z(c), z(e)) {
        Some((_, _, dist)) => dist <= margin,
        None => [
            geom::point_segment(z(a), z(c), z(e)).0,
            geom::point_segment(z(b), z(c), z(e)).0,
            geom::point_segment(z(c), z(a), z(b)).0,
            geom::point_segment(z(e), z(a), z(b)).0,
        ]
        .into_iter()
        .any(|x| x <= margin),
    }
}
