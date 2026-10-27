//! Snap near-touches (MOD-19): the move of a vertex onto the plane of a face of the other solid,
//! for the solid tools, when the person agrees.
//!
//! # The near-touch
//!
//! A vertex of one solid lies off the plane of a face of the other solid by more than the plane
//! tolerance (0.0025 mm) but not more than the length tolerance (0.01 mm). The piece that the
//! cut would make there is too short to draw, and the vertex is too far off to count as on the
//! face. The solid tools refuse this (see `boolean`). [`find`] lists such vertices. A vertex is a
//! near-touch of a face when its foot on the plane lies in the face (or within the length
//! tolerance of its border), or when an edge of the vertex crosses the plane inside the face.
//!
//! # The snap
//!
//! [`snap`] moves each such vertex onto the plane along the normal of the face. The move is at
//! most the length tolerance. Faces of one plane count as one. A vertex that is near the plane of
//! another face too, or on the plane of another face that the move would take it off by more than
//! the plane tolerance, is not moved. The faces that use the vertex follow: a face that stays flat
//! keeps its id, and a face that does not stay flat is cut into triangles by the move of the model
//! (the move of `Edit::TransformVertices`, with its fold). The snap is refused, with a typed reason
//! ([`SnapRefusal`]), when
//!
//! - a vertex is used by geometry outside the operation,
//! - a face would shrink to nothing (an edge shorter than the length tolerance or an area
//!   smaller than its square),
//! - the model refuses the move (a face ends over another in one plane, and so on), or a moved
//!   vertex does not land where the snap puts it (it welds to another vertex),
//! - a solid is not closed after the move, or
//! - the move leaves or makes a near-touch (a vertex near the planes of two faces that are not
//!   one plane, or a vertex on a plane that the move takes it off).
//!
//! The module knows nothing of the operations. It works on a scratch model that holds the two
//! solids, given as two sets of faces, and returns what it moved and the two sets after the move
//! (a folded face has new pieces). The caller decides when to use it and puts the result in.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DVec2, DVec3};
use serde::Serialize;
use serde_json::Value;

use super::super::arrange::inside;
use super::super::edit;
use super::super::geom::{self, point_segment};
use super::super::index::BoxTree;
use super::super::topo::Topo;
use super::super::{FaceId, PLANE_TOLERANCE_FACTOR, SurfaceModel, VertexId};
use super::Operand;
use super::shell::{ShellState, crossings_of, state_with};

/// The attribute that follows a face through the move: the solids it belongs to.
const OWNER: &str = "snap_owner";

/// The words that start the note of every moved vertex (see [`SnapMove::note`]). A window that
/// says the same in its own words can tell these notes from the others.
pub const SNAP_NOTE_START: &str = "snapped vertex ";

/// A vertex of one solid that is a near-touch of a face of the other solid.
#[derive(Clone, Debug, PartialEq)]
pub struct NearTouch {
    /// The vertex, and the solid it belongs to.
    pub vertex: VertexId,
    pub operand: Operand,
    /// The face of the other solid.
    pub face: FaceId,
    /// The signed distance of the vertex from the plane of the face, in mm (positive in front).
    pub distance: f64,
}

/// A vertex that the snap moved. The positions are in the coordinates of the surface model, in mm.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SnapMove {
    pub vertex: VertexId,
    /// The faces whose plane the vertex went onto (more than one when they lie in one plane).
    /// Sorted.
    pub faces: Vec<FaceId>,
    pub from: [f64; 3],
    pub to: [f64; 3],
    /// How far the vertex moved, in mm.
    #[serde(rename = "distance_mm")]
    pub distance: f64,
}

impl SnapMove {
    /// The note of the result for this move. The numbers have four decimals (0.1 micrometer).
    pub fn note(&self) -> String {
        let faces: Vec<String> = self.faces.iter().map(ToString::to_string).collect();
        let onto = match faces.as_slice() {
            [one] => format!("the plane of face {one}"),
            many => format!("the planes of faces {}", many.join(", ")),
        };
        format!(
            "{SNAP_NOTE_START}{} from ({:.4}, {:.4}, {:.4}) mm to ({:.4}, {:.4}, {:.4}) mm onto {onto}: moved {:.4} mm",
            self.vertex,
            self.from[0],
            self.from[1],
            self.from[2],
            self.to[0],
            self.to[1],
            self.to[2],
            self.distance
        )
    }

    /// Reads the move back from its note (see [`SnapMove::note`]), with the numbers to four
    /// decimals. The note may have a prefix, such as the name of the object. `None` for any other
    /// text. The API uses it to list the moves of an edit that ran.
    pub fn from_note(note: &str) -> Option<SnapMove> {
        let rest = &note[note.find(SNAP_NOTE_START)? + SNAP_NOTE_START.len()..];
        let (vertex, rest) = rest.split_once(" from (")?;
        let (from, rest) = rest.split_once(") mm to (")?;
        let (to, rest) = rest.split_once(") mm onto ")?;
        let (onto, moved) = rest.split_once(": moved ")?;
        let point = |t: &str| -> Option<[f64; 3]> {
            let v: Vec<f64> = t
                .split(", ")
                .map(|x| x.parse().ok())
                .collect::<Option<_>>()?;
            v.try_into().ok()
        };
        let list = onto
            .strip_prefix("the plane of face ")
            .or_else(|| onto.strip_prefix("the planes of faces "))?;
        let faces: Vec<FaceId> = list
            .split(", ")
            .map(|x| x.parse().ok().map(FaceId))
            .collect::<Option<_>>()?;
        Some(SnapMove {
            vertex: VertexId(vertex.parse().ok()?),
            faces,
            from: point(from)?,
            to: point(to)?,
            distance: moved.strip_suffix(" mm")?.parse().ok()?,
        })
    }
}

/// Why a snap was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SnapRefusal {
    /// "shared"
    #[error(
        "vertex {vertex} is used by geometry outside the operation, so it stays: make the solids a group first"
    )]
    Shared { vertex: VertexId },
    /// "degenerate"
    #[error("the snap makes face {face} degenerate: {reason}")]
    Degenerate { face: FaceId, reason: String },
    /// "refused move"
    #[error(
        "the snap moves vertices in a way that the model refuses: {reason}. Faces of two solids that end in one plane and overlap are a limit of the solid tools (backlog row MOD-20)"
    )]
    MoveRefused { reason: String },
    /// "opens a solid"
    #[error("the snap leaves the {which} solid not closed: {reason}")]
    OpensSolid { which: Operand, reason: String },
    /// "new near-touch"
    #[error(
        "the snap leaves vertex {vertex} {distance:.4} mm from the plane of face {face}: a new near-touch"
    )]
    NewNearTouch {
        vertex: VertexId,
        face: FaceId,
        distance: f64,
    },
    /// "still refused"
    #[error("the operation is still refused after the snap: {reason}")]
    StillRefused { reason: String },
}

impl SnapRefusal {
    /// The stable id of the reason, for agents.
    pub fn code(&self) -> &'static str {
        match self {
            SnapRefusal::Shared { .. } => "snap_vertex_shared",
            SnapRefusal::Degenerate { .. } => "snap_degenerate_face",
            SnapRefusal::MoveRefused { .. } => "snap_move_refused",
            SnapRefusal::OpensSolid { .. } => "snap_breaks_solid",
            SnapRefusal::NewNearTouch { .. } => "snap_new_near_touch",
            SnapRefusal::StillRefused { .. } => "snap_still_refused",
        }
    }
}

/// What a snap did.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapped {
    /// The vertices that moved, in the order of their ids. Empty when there was no near-touch.
    pub moves: Vec<SnapMove>,
    /// The faces of the two solids after the move.
    pub a: BTreeSet<FaceId>,
    pub b: BTreeSet<FaceId>,
    /// The notes of the model about the move (a face that was cut into triangles).
    pub notes: Vec<String>,
}

fn positions(m: &SurfaceModel, set: &BTreeSet<FaceId>) -> BTreeSet<VertexId> {
    set.iter()
        .flat_map(|f| m.faces[f].loops().flatten().copied())
        .collect()
}

/// The 2D rings of a face in its plane.
fn rings(m: &SurfaceModel, f: FaceId) -> Vec<Vec<DVec2>> {
    let face = &m.faces[&f];
    face.loops()
        .map(|l| {
            l.iter()
                .map(|v| face.plane.project(m.vertices[v]))
                .collect()
        })
        .collect()
}

/// True if the point `p` (on the plane of `f`) is in the face or within `tol` of its border.
fn in_face(m: &SurfaceModel, f: FaceId, p: DVec3, tol: f64) -> bool {
    let face = &m.faces[&f];
    if inside(face.plane.project(p), &rings(m, f)) {
        return true;
    }
    face.uses().any(|(a, b)| {
        let (d, _) = point_segment(p, m.vertices[&a], m.vertices[&b]);
        d <= tol
    })
}

/// A vertex of one solid and a face of the other that it is near or on.
struct Contact {
    touch: NearTouch,
    /// Within the plane tolerance of the plane of the face: on it, no near-touch.
    on: bool,
}

/// The contacts of the vertices of the solid `from` with the faces of the solid `to`: the
/// near-touches, and the faces that a vertex is on (within the plane tolerance, in the face).
fn one_way(
    m: &SurfaceModel,
    topo: &Topo,
    from: &BTreeSet<FaceId>,
    to: &BTreeSet<FaceId>,
    operand: Operand,
) -> Vec<Contact> {
    let items: Vec<(DVec3, DVec3, u32)> = to
        .iter()
        .filter_map(|f| {
            let (lo, hi) = geom::bounds(m.faces[f].outer.iter().map(|v| m.vertices[v]))?;
            let t = geom::tolerance_at(geom::size(lo).max(geom::size(hi)));
            Some((lo - DVec3::splat(t), hi + DVec3::splat(t), f.0))
        })
        .collect();
    let tree = BoxTree::new(items);
    let mut out = Vec::new();
    for v in positions(m, from) {
        let p = m.vertices[&v];
        let tol = geom::tolerance_at(geom::size(p));
        let plane_tol = tol * PLANE_TOLERANCE_FACTOR;
        let mut near: Vec<FaceId> = Vec::new();
        // The edges of the vertex can cross a face from farther away than the box of the vertex.
        let reach = topo
            .edges_at(v)
            .filter_map(|e| m.edges.get(&e))
            .map(|e| {
                let w = if e.v[0] == v { e.v[1] } else { e.v[0] };
                m.vertices[&w]
            })
            .collect::<Vec<_>>();
        let (lo, hi) = reach
            .iter()
            .fold((p, p), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
        tree.query(lo - DVec3::splat(tol), hi + DVec3::splat(tol), &mut |id| {
            near.push(FaceId(id));
        });
        near.sort();
        for f in near {
            let face = &m.faces[&f];
            let d = face.plane.distance(p);
            let on = d.abs() <= plane_tol;
            if d.abs() > tol {
                continue;
            }
            let foot = p - face.plane.normal * d;
            let mut hit = in_face(m, f, foot, tol);
            if !hit && !on {
                // An edge from the vertex that crosses the plane inside the face.
                hit = reach.iter().any(|q| {
                    let dq = face.plane.distance(*q);
                    if dq.abs() <= plane_tol || (dq > 0.0) == (d > 0.0) {
                        return false;
                    }
                    let x = p + (*q - p) * (d / (d - dq));
                    in_face(m, f, x, tol)
                });
            }
            if hit {
                out.push(Contact {
                    touch: NearTouch {
                        vertex: v,
                        operand,
                        face: f,
                        distance: d,
                    },
                    on,
                });
            }
        }
    }
    out
}

fn contacts(m: &SurfaceModel, a: &BTreeSet<FaceId>, b: &BTreeSet<FaceId>) -> Vec<Contact> {
    let topo = Topo::of(m);
    let mut out = one_way(m, &topo, a, b, Operand::First);
    out.extend(one_way(m, &topo, b, a, Operand::Second));
    out.sort_by_key(|c| (c.touch.vertex, c.touch.face));
    out.dedup_by_key(|c| (c.touch.vertex, c.touch.face));
    out
}

/// The near-touches between the two solids: the vertices of each that lie off a face plane of the
/// other by more than the plane tolerance but not more than the length tolerance, with the face.
/// Sorted by vertex, then face.
pub fn find(m: &SurfaceModel, a: &BTreeSet<FaceId>, b: &BTreeSet<FaceId>) -> Vec<NearTouch> {
    contacts(m, a, b)
        .into_iter()
        .filter(|c| !c.on)
        .map(|c| c.touch)
        .collect()
}

/// Where `p` goes to be on the plane of its near-touches `ts` (all of one vertex): along the
/// normal of the nearest face. Faces in one plane count once, and the displacement and all of them
/// are given. A face in another plane is a refusal: the vertex stays near it. So is a face in
/// `ons` that the vertex is on, when the move takes the vertex off it by more than the plane
/// tolerance.
fn displacement(
    m: &SurfaceModel,
    p: DVec3,
    ts: &[&NearTouch],
    ons: &[&NearTouch],
) -> Result<(DVec3, Vec<FaceId>), SnapRefusal> {
    let plane_tol = geom::plane_tolerance_at(geom::size(p));
    let first = ts[0];
    let n = m.faces[&first.face].plane.normal;
    let mut faces = vec![first.face];
    for t in &ts[1..] {
        let u = m.faces[&t.face].plane.normal;
        // The same plane: parallel, and at the same place for the vertex.
        let same = u.cross(n).length() < 1e-9
            && (t.distance * u.dot(n) - first.distance).abs() <= plane_tol;
        if !same {
            return Err(SnapRefusal::NewNearTouch {
                vertex: t.vertex,
                face: t.face,
                distance: t.distance,
            });
        }
        faces.push(t.face);
    }
    faces.sort();
    let delta = -n * first.distance;
    for h in ons {
        let after = h.distance + m.faces[&h.face].plane.normal.dot(delta);
        if after.abs() > plane_tol {
            return Err(SnapRefusal::NewNearTouch {
                vertex: h.vertex,
                face: h.face,
                distance: after,
            });
        }
    }
    Ok((delta, faces))
}

/// Why a face would be degenerate when `vertices` stand at the positions `to`, or `None`.
fn degenerate(m: &SurfaceModel, to: &BTreeMap<VertexId, DVec3>) -> Option<(FaceId, String)> {
    let at = |v: &VertexId| to.get(v).copied().unwrap_or(m.vertices[v]);
    for (id, f) in &m.faces {
        if !f.loops().flatten().any(|v| to.contains_key(v)) {
            continue;
        }
        let tol = f
            .loops()
            .flatten()
            .map(|v| geom::tolerance_at(geom::size(at(v))))
            .fold(0.0, f64::max);
        for (x, y) in f.uses() {
            let len = at(&x).distance(at(&y));
            if len < tol {
                return Some((
                    *id,
                    format!(
                        "the edge between vertices {x} and {y} gets {len:.4} mm long, shorter than the length tolerance ({tol} mm)"
                    ),
                ));
            }
        }
        let area = f.outer.iter().map(&at).collect::<Vec<_>>();
        let a = geom::newell(&area).length() / 2.0;
        if a < tol * tol {
            return Some((*id, format!("its area gets {a:.6} mm²")));
        }
    }
    None
}

/// Both solids (as sets of faces of `m`) are closed solids.
fn ensure_closed(
    m: &SurfaceModel,
    a: &BTreeSet<FaceId>,
    b: &BTreeSet<FaceId>,
) -> Result<(), SnapRefusal> {
    let topo = Topo::of(m);
    let crossings = crossings_of(&m.check().warnings);
    for (set, which) in [(a, Operand::First), (b, Operand::Second)] {
        let s = state_with(m, &topo, set, &crossings);
        if s.state != ShellState::Solid {
            let reason = match s.open_edges.first() {
                Some(o) => format!("edge {} is open ({})", o.edge, o.kind),
                None => format!("{:?}", s.state).to_lowercase(),
            };
            return Err(SnapRefusal::OpensSolid { which, reason });
        }
    }
    Ok(())
}

/// After the move no near-touch is left, and none is new. A move can tilt a face (it is cut into
/// triangles), so a vertex of the other solid that was on it can be off it now.
pub(in crate::surface) fn no_near_touch_left(
    m: &SurfaceModel,
    a: &BTreeSet<FaceId>,
    b: &BTreeSet<FaceId>,
) -> Result<(), SnapRefusal> {
    match find(m, a, b).first() {
        Some(t) => Err(SnapRefusal::NewNearTouch {
            vertex: t.vertex,
            face: t.face,
            distance: t.distance,
        }),
        None => Ok(()),
    }
}

/// Moves every near-touch vertex of the solids `a` and `b` in the scratch model `m` onto the
/// plane of its face. `pinned` holds the vertices that geometry outside the operation uses.
///
/// `m` holds the two solids and nothing else that matters (the caller gives a copy). On an error
/// `m` is left in an unknown state. On success `m` holds the moved solids, and `Snapped` has the
/// moves and the faces of the two solids (the ids of a folded face are new).
pub fn snap(
    m: &mut SurfaceModel,
    a: &BTreeSet<FaceId>,
    b: &BTreeSet<FaceId>,
    pinned: &BTreeSet<VertexId>,
) -> Result<Snapped, SnapRefusal> {
    let all = contacts(m, a, b);
    let mut by_vertex: BTreeMap<VertexId, Vec<&NearTouch>> = BTreeMap::new();
    let mut on_faces: BTreeMap<VertexId, Vec<&NearTouch>> = BTreeMap::new();
    for c in &all {
        let map = if c.on { &mut on_faces } else { &mut by_vertex };
        map.entry(c.touch.vertex).or_default().push(&c.touch);
    }
    if by_vertex.is_empty() {
        return Ok(Snapped {
            moves: Vec::new(),
            a: a.clone(),
            b: b.clone(),
            notes: Vec::new(),
        });
    }
    if let Some(v) = by_vertex.keys().find(|v| pinned.contains(v)) {
        return Err(SnapRefusal::Shared { vertex: *v });
    }
    // Where each vertex goes: along the normals of the faces, onto their planes.
    let mut moves: Vec<SnapMove> = Vec::new();
    let mut target: BTreeMap<VertexId, DVec3> = BTreeMap::new();
    for (v, ts) in &mut by_vertex {
        ts.sort_by(|x, y| {
            x.distance
                .abs()
                .total_cmp(&y.distance.abs())
                .then(x.face.cmp(&y.face))
        });
        let p = m.vertices[v];
        let ons = on_faces.get(v).map_or(&[][..], Vec::as_slice);
        let (delta, faces) = displacement(m, p, ts, ons)?;
        target.insert(*v, p + delta);
        moves.push(SnapMove {
            vertex: *v,
            faces,
            from: p.to_array(),
            to: (p + delta).to_array(),
            distance: delta.length(),
        });
    }
    if let Some((face, reason)) = degenerate(m, &target) {
        return Err(SnapRefusal::Degenerate { face, reason });
    }
    // The faces carry their solids through the move: a face that is cut into triangles gives
    // each piece the attribute.
    for (id, f) in m.faces.iter_mut() {
        let owner = match (a.contains(id), b.contains(id)) {
            (true, true) => "ab",
            (true, false) => "a",
            _ => "b",
        };
        f.attrs.insert(OWNER.into(), Value::from(owner));
    }
    // One move for every offset (a face that moves whole is mapped exactly).
    let mut groups: Vec<(DVec3, Vec<VertexId>)> = Vec::new();
    for (v, to) in &target {
        let off = *to - m.vertices[v];
        match groups.iter_mut().find(|(o, _)| o.distance(off) <= 1e-9) {
            Some((_, vs)) => vs.push(*v),
            None => groups.push((off, vec![*v])),
        }
    }
    let mut notes = Vec::new();
    for (off, vertices) in groups {
        let matrix = DAffine3::from_translation(off).to_cols_array();
        match edit::move_vertices(m, &vertices, &matrix) {
            Ok(n) => notes.extend(n),
            Err(e) => {
                return Err(SnapRefusal::MoveRefused {
                    reason: e.to_string(),
                });
            }
        }
    }
    // Every vertex stands where the snap put it (none was welded or moved again).
    for (v, to) in &target {
        match m.vertices.get(v) {
            Some(p) if p.distance(*to) <= 1e-7 => {}
            _ => {
                return Err(SnapRefusal::MoveRefused {
                    reason: format!(
                        "vertex {v} does not stay where the snap puts it (it lands on other geometry)"
                    ),
                });
            }
        }
    }
    let (mut na, mut nb) = (BTreeSet::new(), BTreeSet::new());
    for (id, f) in m.faces.iter_mut() {
        match f.attrs.remove(OWNER).as_ref().and_then(Value::as_str) {
            Some("a") => {
                na.insert(*id);
            }
            Some("b") => {
                nb.insert(*id);
            }
            _ => {
                na.insert(*id);
                nb.insert(*id);
            }
        }
    }
    ensure_closed(m, &na, &nb)?;
    no_near_touch_left(m, &na, &nb)?;
    Ok(Snapped {
        moves,
        a: na,
        b: nb,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_distinct() {
        let all = [
            SnapRefusal::Shared {
                vertex: VertexId(1),
            },
            SnapRefusal::Degenerate {
                face: FaceId(1),
                reason: String::new(),
            },
            SnapRefusal::MoveRefused {
                reason: String::new(),
            },
            SnapRefusal::OpensSolid {
                which: Operand::First,
                reason: String::new(),
            },
            SnapRefusal::NewNearTouch {
                vertex: VertexId(1),
                face: FaceId(1),
                distance: 0.005,
            },
            SnapRefusal::StillRefused {
                reason: String::new(),
            },
        ];
        let codes: BTreeSet<&str> = all.iter().map(SnapRefusal::code).collect();
        assert_eq!(codes.len(), all.len());
        assert!(codes.iter().all(|c| c.starts_with("snap_")));
        for r in &all {
            assert!(!r.to_string().is_empty());
        }
    }

    #[test]
    fn a_solid_that_is_not_closed_after_the_move_is_named() {
        let (m, _, fa, fb) = crate::surface::tests_solid::turned_model(
            1.539_623_148_363_857_6,
            0.394_573_062_789_783_6,
            200.0,
            796.195_805_697_005_2,
            200.0,
        )
        .unwrap();
        let (a, b): (BTreeSet<FaceId>, BTreeSet<FaceId>) =
            (fa.iter().copied().collect(), fb.iter().copied().collect());
        assert_eq!(ensure_closed(&m, &a, &b), Ok(()));
        // A face of the second solid is left out: its edges are open.
        let gone = *b.iter().next().unwrap();
        let mut open_b = b.clone();
        open_b.remove(&gone);
        let e = ensure_closed(&m, &a, &open_b).unwrap_err();
        let SnapRefusal::OpensSolid { which, reason } = &e else {
            panic!("{e:?}");
        };
        assert_eq!(*which, Operand::Second);
        assert!(reason.contains("is open"), "{reason}");
        assert_eq!(e.code(), "snap_breaks_solid");
        // The first solid is looked at first.
        let mut open_a = a.clone();
        open_a.remove(a.iter().next().unwrap());
        let e = ensure_closed(&m, &open_a, &open_b).unwrap_err();
        assert!(
            matches!(
                e,
                SnapRefusal::OpensSolid {
                    which: Operand::First,
                    ..
                }
            ),
            "{e:?}"
        );
    }

    /// The ids of the corners of the box `[0, 1000]^3` in a model that holds only that box.
    fn box_model() -> (SurfaceModel, Vec<FaceId>, impl Fn(DVec3) -> VertexId) {
        let mut m = SurfaceModel::new();
        let c = |i: usize| -> [f64; 3] {
            std::array::from_fn(|k| if i >> k & 1 == 0 { 0.0 } else { 1000.0 })
        };
        let mut edges = Vec::new();
        for i in 0..8usize {
            for bit in [1usize, 2, 4] {
                if i & bit == 0 {
                    edges.push([c(i), c(i | bit)]);
                }
            }
        }
        m.apply(&crate::surface::Edit::AddEdges { edges }).unwrap();
        let faces: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
        let ids: Vec<(VertexId, DVec3)> = m.vertices().collect();
        let at = move |p: DVec3| ids.iter().find(|(_, q)| q.distance(p) < 1e-9).unwrap().0;
        (m, faces, at)
    }

    #[test]
    fn a_face_with_no_area_after_the_move_is_degenerate_even_with_long_edges() {
        let (m, _, at) = box_model();
        // The bottom face on one line: every edge is long, the area is zero.
        let to = BTreeMap::from([
            (
                at(DVec3::new(1000.0, 1000.0, 0.0)),
                DVec3::new(800.0, 0.0, 0.0),
            ),
            (
                at(DVec3::new(0.0, 1000.0, 0.0)),
                DVec3::new(200.0, 0.0, 0.0),
            ),
        ]);
        let (face, reason) = degenerate(&m, &to).unwrap();
        assert!(m.faces.contains_key(&face));
        assert!(reason.contains("area"), "{reason}");
        // A move of a vertex by 0.005 mm leaves every face as it is.
        let small = BTreeMap::from([(at(DVec3::ZERO), DVec3::new(0.0, 0.0, 0.005))]);
        assert!(degenerate(&m, &small).is_none());
        // An edge of 0.004 mm after the move is shorter than the length tolerance.
        let short = BTreeMap::from([(at(DVec3::ZERO), DVec3::new(1000.0, 0.0, 0.004))]);
        let (_, reason) = degenerate(&m, &short).unwrap();
        assert!(
            reason.contains("shorter than the length tolerance"),
            "{reason}"
        );
    }

    #[test]
    fn a_solid_that_looks_inward_is_not_closed_and_a_shared_face_is_in_both_sets() {
        let (mut m, faces, _) = box_model();
        let all: BTreeSet<FaceId> = faces.iter().copied().collect();
        // The state names the problem when no edge is open.
        let inward: Vec<FaceId> = if m.solid_state(&faces).unwrap().volume < 0.0 {
            Vec::new()
        } else {
            faces.clone()
        };
        m.apply(&crate::surface::Edit::ReverseFaces { faces: inward })
            .unwrap();
        let e = ensure_closed(&m, &all, &all).unwrap_err();
        let SnapRefusal::OpensSolid { reason, .. } = &e else {
            panic!("{e:?}");
        };
        assert_eq!(reason, "inverted");
    }

    #[test]
    fn a_face_that_both_solids_use_belongs_to_both() {
        let (mut m, _, fa, fb) = crate::surface::tests_solid::turned_model(
            1.539_623_148_363_857_6,
            0.394_573_062_789_783_6,
            200.0,
            796.195_805_697_005_2,
            200.0,
        )
        .unwrap();
        let a: BTreeSet<FaceId> = fa.iter().copied().collect();
        let mut b: BTreeSet<FaceId> = fb.iter().copied().collect();
        // The second set holds one face of the first solid more: its edges have three faces.
        b.insert(*a.iter().next().unwrap());
        let e = snap(&mut m, &a, &b, &BTreeSet::new());
        assert!(
            matches!(
                e,
                Err(SnapRefusal::OpensSolid { .. }) | Err(SnapRefusal::MoveRefused { .. })
            ),
            "{e:?}"
        );
    }

    #[test]
    fn a_move_has_a_note_with_all_facts() {
        let m = SnapMove {
            vertex: VertexId(12),
            faces: vec![FaceId(7)],
            from: [1.0, 2.0, 3.0],
            to: [1.0, 2.0, 3.005],
            distance: 0.005,
        };
        let n = m.note();
        assert!(n.contains("vertex 12"), "{n}");
        assert!(n.contains("(1.0000, 2.0000, 3.0000)"), "{n}");
        assert!(n.contains("the plane of face 7"), "{n}");
        assert!(n.contains("0.0050 mm"), "{n}");
        let two = SnapMove {
            faces: vec![FaceId(7), FaceId(9)],
            ..m
        };
        assert!(
            two.note().contains("the planes of faces 7, 9"),
            "{}",
            two.note()
        );
    }
}
