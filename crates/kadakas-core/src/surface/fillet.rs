//! Fillet and chamfer (MOD-21): the plan of the new geometry, without the model.
//!
//! The planner takes a closed polyhedral surface around the edges ([`Shape`]), the edges and a
//! [`Profile`] (a round of a radius with a number of segments, or a flat cut at a distance). It
//! returns a [`Plan`]: the new points, the new loops of the faces that change, the new faces and
//! the edges to trim. The model edit (`edit::fillet`) applies the plan and checks the result. So
//! the planner is pure geometry and has its own unit tests.
//!
//! # The strip of an edge
//!
//! An edge between face A (it runs from `p` to `q` in the loop of A) and face B gets a strip. The
//! strip touches A and B along two lines (the rails) at the setback `s` from the edge: `r tan(φ/2)`
//! for a round of radius `r` (φ is the angle between the two face normals) and the distance for a
//! cut. The cross section of a round is an arc about a centre at distance `r` from both planes:
//! inside the solid for a convex edge, outside for a concave edge. The arc is cut into `n` equal
//! segments, so the strip is `n` faces, each between two lines parallel to the edge. A face with
//! two sides parallel to one direction is always flat, whatever the ends of the strip are.
//!
//! # The corners
//!
//! Each end of an edge is a vertex where exactly three faces meet. The planner refuses a vertex of
//! more faces. With `m` of the three edges at the vertex chosen:
//!
//! - `m = 1`: the strip runs to the plane of the third face. That face gets the end of the strip
//!   in its outline (a box with one round edge: the end faces get the quarter circle).
//! - `m = 2`: the two strips end on one plane: the plane through the third edge (it stays sharp)
//!   and the corner of the two rails in the face that the two edges share. When the two edges meet
//!   their faces at the same angle, the ends are the same points and the strips meet as a miter.
//!   Otherwise a flat piece in that plane fills the gap between the two ends.
//! - `m = 3`: for a round, the strips end at the cross section through the centre of the ball
//!   that touches all three planes. A patch of triangles on that ball closes the corner. Its points
//!   are rows between two of the arcs, so the third arc is the last row. For a cut, the corner is
//!   the triangle of the three rail corners.
//!
//! A vertex with chosen convex and concave edges is refused.
//!
//! # What does not fit
//!
//! A strip must keep a length along its edge, each changed face must keep its sides in their
//! directions, its loops must not cross and its holes must stay inside. A corner piece must be a
//! simple polygon that agrees with its neighbours about front and back. Each failure is
//! [`FilletError::DoesNotFit`]; the model edit then finds the largest size that fits by halving.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3};

use super::geom::{self, Plane};
use super::{EdgeId, FaceId, VertexId, key};

mod corner;
mod fit;
#[cfg(test)]
mod tests;

/// The most segments of a round.
pub const MAX_SEGMENTS: u32 = 64;
/// The default segments of a round are at most this many.
pub const MAX_DEFAULT_SEGMENTS: u32 = 24;
/// The angle in degrees below which `faces` and `all` leave an edge out (an edge of a curved
/// surface).
pub const DEFAULT_MIN_ANGLE: f64 = 5.0;

/// The cross section of the new strips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Profile {
    /// A round (fillet) of `radius` mm in `segments` flat faces.
    Round { radius: f64, segments: u32 },
    /// A flat cut (chamfer) at `distance` mm from the edge on both faces.
    Flat { distance: f64 },
}

impl Profile {
    /// The radius or the distance.
    pub fn size(&self) -> f64 {
        match self {
            Profile::Round { radius, .. } => *radius,
            Profile::Flat { distance } => *distance,
        }
    }

    /// The same profile with another radius or distance.
    pub fn resized(&self, x: f64) -> Profile {
        match self {
            Profile::Round { segments, .. } => Profile::Round {
                radius: x,
                segments: *segments,
            },
            Profile::Flat { .. } => Profile::Flat { distance: x },
        }
    }

    /// "radius" or "distance", for the messages.
    pub fn noun(&self) -> &'static str {
        match self {
            Profile::Round { .. } => "radius",
            Profile::Flat { .. } => "distance",
        }
    }
}

/// The default number of segments of a round of `radius` mm whose edges turn by at most
/// `angle` radians: at most 15 degrees per segment, and less for a large radius, so the arc
/// leaves the true circle by at most 0.5 mm. A small radius gets fewer, so that a segment stays
/// four length tolerances long. At least 1 and at most [`MAX_DEFAULT_SEGMENTS`].
pub fn default_segments(radius: f64, angle: f64) -> u32 {
    let mut step = 15f64.to_radians();
    if radius > 0.5 {
        step = step.min(2.0 * (1.0 - 0.5 / radius).acos());
    }
    let n = (angle / step).ceil();
    // A small radius gets fewer segments, so a segment stays four length tolerances long.
    let fits = (radius * angle / (4.0 * super::LENGTH_TOLERANCE)).floor();
    let n = n.min(fits);
    if n.is_finite() && n >= 1.0 {
        (n as u32).clamp(1, MAX_DEFAULT_SEGMENTS)
    } else {
        1
    }
}

/// The angle between the normals of the two faces of an edge (radians, 0 for one plane) and
/// whether the edge is convex. `a` is the normal of the face whose loop runs along `d`, `b` the
/// normal of the other face.
pub fn edge_kind(a: DVec3, b: DVec3, d: DVec3) -> (f64, bool) {
    let c = a.cross(b);
    let phi = c.length().atan2(a.dot(b));
    (phi, c.dot(d) > 0.0)
}

/// A face of the [`Shape`]: its plane and its loops (outer first), as in a surface model.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeFace {
    pub plane: Plane,
    pub loops: Vec<Vec<VertexId>>,
}

/// The faces around the edges to round, with the positions of their vertices. It holds at least
/// every face that uses an end of a chosen edge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub points: BTreeMap<VertexId, DVec3>,
    pub faces: BTreeMap<FaceId, ShapeFace>,
}

/// A point of the plan: a vertex that stays, or a new point (an index into [`Plan::points`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pt {
    Old(VertexId),
    New(usize),
}

/// What a new face is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Segment `j` of the strip of an edge.
    Strip { edge: EdgeId, segment: usize },
    /// A piece of the corner at a vertex.
    Corner { vertex: VertexId },
}

/// A new face of the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct NewFace {
    /// One loop, counterclockwise seen from the front.
    pub points: Vec<Pt>,
    /// The front normal.
    pub normal: DVec3,
    pub role: Role,
    /// The faces whose look the new face can take (the model edit takes the largest).
    pub looks: Vec<FaceId>,
}

/// An edge that stays but gets a new end: the edge `{old, far}` runs from `far` to `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trim {
    pub far: VertexId,
    pub old: VertexId,
    pub to: Pt,
}

/// The new geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// The new points.
    pub points: Vec<DVec3>,
    /// The faces that change: all their new loops (outer first). They keep their planes.
    pub faces: BTreeMap<FaceId, Vec<Vec<Pt>>>,
    /// The strips (edge by edge) and the corner pieces.
    pub new_faces: Vec<NewFace>,
    /// The edges that stay with a new end.
    pub trims: Vec<Trim>,
    /// The vertices that go (the ends of the chosen edges).
    pub gone: BTreeSet<VertexId>,
    /// Edges inside the round (between two segments): soft and smooth with `smooth`.
    pub blend: BTreeSet<(Pt, Pt)>,
    /// Edges where the round meets a face along a rail: soft with `smooth`.
    pub rails: BTreeSet<(Pt, Pt)>,
    /// Edges that cut a strip face into triangles where it took a point of another end, so that
    /// every face stays flat: always soft.
    pub seams: BTreeSet<(Pt, Pt)>,
    /// An upper bound of the volume that the convex edges take away (mm³).
    pub convex_bound: f64,
    /// An upper bound of the volume that the concave edges add (mm³).
    pub concave_bound: f64,
}

/// The pair of points as an edge key.
pub fn pair(a: Pt, b: Pt) -> (Pt, Pt) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Why a fillet or a chamfer was refused. The texts are English for agents and logs; the window
/// shows its own text for each kind.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum FilletError {
    #[error("no edges to round or cut: give edges, faces or all")]
    NoEdges,
    #[error(
        "edge {edge} is not between exactly two faces of a solid, so it has no corner to round or cut"
    )]
    NotSolidEdge { edge: EdgeId },
    #[error(
        "edge {edge} lies between faces in one plane, or between faces folded onto each other: \
         there is no corner to round or cut"
    )]
    FlatEdge { edge: EdgeId },
    #[error("the corner at vertex {vertex} is not supported: {why}")]
    Corner { vertex: VertexId, why: String },
    #[error("the {noun} of {asked} mm does not fit: {why}; {}", fits_text(*.most))]
    DoesNotFit {
        noun: &'static str,
        asked: f64,
        why: String,
        /// The largest size that fits (mm), when it was looked for; 0 when nothing fits.
        most: Option<f64>,
    },
    #[error(
        "the {noun} of {asked} mm is too small for {segments} segments at edge {edge}: a segment \
         would be shorter than the length tolerance"
    )]
    TooSmall {
        noun: &'static str,
        asked: f64,
        segments: u32,
        edge: EdgeId,
    },
    #[error(
        "the edit would make about {count} new edges, more than the {max} of one edit; {}",
        match most_segments {
            Some(k) => format!("use fewer segments: at most about {k}"),
            None => "choose fewer edges, or round them in more than one step".to_string(),
        }
    )]
    TooManyEdges {
        count: usize,
        max: usize,
        most_segments: Option<u32>,
    },
    #[error("segments must be from 1 to {MAX_SEGMENTS}, not {segments}")]
    Segments { segments: u32 },
    #[error("the {noun} must be a number larger than the length tolerance (0.01 mm), not {asked}")]
    BadSize { noun: &'static str, asked: f64 },
    #[error("the faces at edge {edge} are not a closed solid: {why}")]
    NotSolid { edge: EdgeId, why: String },
    #[error("the result would not be a valid solid: {0}")]
    Result(String),
}

fn fits_text(most: Option<f64>) -> String {
    match most {
        Some(x) if x > 0.0 => format!("at most about {x} mm fits"),
        Some(_) => "there is no room for it there".to_string(),
        None => "a smaller size fits".to_string(),
    }
}

impl FilletError {
    /// The stable id of the refusal (for agents).
    pub fn code(&self) -> &'static str {
        match self {
            FilletError::NoEdges => "fillet_no_edges",
            FilletError::NotSolidEdge { .. } => "fillet_edge_not_solid",
            FilletError::FlatEdge { .. } => "fillet_edge_flat",
            FilletError::Corner { .. } => "fillet_corner_unsupported",
            FilletError::DoesNotFit { .. } => "fillet_does_not_fit",
            FilletError::TooSmall { .. } => "fillet_too_small",
            FilletError::Segments { .. } => "fillet_segments_out_of_range",
            FilletError::TooManyEdges { .. } => "fillet_too_many_edges",
            FilletError::BadSize { .. } => "fillet_bad_size",
            FilletError::NotSolid { .. } => "fillet_not_solid",
            FilletError::Result(_) => "fillet_result_invalid",
        }
    }
}

/// The uses of each edge (by its vertex pair): the face and the direction.
type Uses = BTreeMap<(VertexId, VertexId), Vec<(FaceId, VertexId, VertexId)>>;

/// The strip of one chosen edge.
#[derive(Clone, Debug)]
struct Strip {
    edge: EdgeId,
    p: VertexId,
    q: VertexId,
    /// The position of `p`.
    at: DVec3,
    len: f64,
    /// The unit direction from `p` to `q`.
    d: DVec3,
    /// Face A (its loop runs from `p` to `q`) and face B.
    faces: [FaceId; 2],
    normals: [DVec3; 2],
    /// The unit directions into A and into B, at a right angle to the edge.
    inward: [DVec3; 2],
    convex: bool,
    phi: f64,
    /// The setback of the rails from the edge.
    s: f64,
    /// The cross section relative to `p`: point 0 on the rail in A, the last on the rail in B.
    prof: Vec<DVec3>,
    /// The ends at `p` and at `q`.
    ends: [Option<End>; 2],
}

/// One end of a strip: its points (in the order of the profile) and where each lies along the
/// edge (from `p`).
#[derive(Clone, Debug)]
struct End {
    pts: Vec<Pt>,
    t: Vec<f64>,
    /// Points put into each side of the end (side `j` runs from point `j` to `j + 1`), where the
    /// end of another strip crosses or touches it.
    between: Vec<Vec<Pt>>,
}

impl Strip {
    /// 0 for face A, 1 for face B.
    fn side_of(&self, f: FaceId) -> Option<usize> {
        self.faces.iter().position(|g| *g == f)
    }

    /// 0 for the end at `p`, 1 for the end at `q`.
    fn end_of(&self, v: VertexId) -> usize {
        usize::from(v != self.p)
    }

    /// Point `j` of the cross section at `t` along the edge from `p`.
    fn point(&self, j: usize, t: f64) -> DVec3 {
        self.at + self.prof[j] + self.d * t
    }

    /// Where line `j` of the strip meets a plane (`t` from `p`), or `None` when it runs along it.
    fn t_on(&self, j: usize, plane: &Plane) -> Option<f64> {
        let nd = plane.normal.dot(self.d);
        if nd.abs() < 1e-9 {
            return None;
        }
        Some(-plane.distance(self.at + self.prof[j]) / nd)
    }

    /// The last index of the profile.
    fn n(&self) -> usize {
        self.prof.len() - 1
    }
}

/// The faces around a vertex in the order of a walk: face `i` runs from the vertex to `next[i]`,
/// and the edge `{v, next[i]}` lies between face `i` and face `i + 1`.
#[derive(Clone, Debug)]
struct Around {
    faces: Vec<FaceId>,
    next: Vec<VertexId>,
}

/// The work of one plan.
struct Builder<'a> {
    shape: &'a Shape,
    tol: f64,
    points: Vec<DVec3>,
    /// For each changed face, the points that replace each of its corners (in loop order).
    subs: BTreeMap<FaceId, BTreeMap<VertexId, Vec<Pt>>>,
    trims: Vec<Trim>,
    corners: Vec<NewFace>,
    /// Corner pieces whose side is found from their neighbours (index into `corners`).
    to_orient: Vec<usize>,
    blend: BTreeSet<(Pt, Pt)>,
    rails: BTreeSet<(Pt, Pt)>,
}

impl Builder<'_> {
    fn add(&mut self, p: DVec3) -> Pt {
        self.points.push(p);
        Pt::New(self.points.len() - 1)
    }

    fn pos(&self, p: Pt) -> DVec3 {
        match p {
            Pt::Old(v) => self.shape.points[&v],
            Pt::New(i) => self.points[i],
        }
    }

    fn sub(&mut self, face: FaceId, v: VertexId, pts: Vec<Pt>) {
        self.subs.entry(face).or_default().insert(v, pts);
    }
}

/// The plan of a fillet or a chamfer of `edges` (each with its two vertices) on `shape`.
pub fn plan(
    shape: &Shape,
    edges: &[(EdgeId, VertexId, VertexId)],
    profile: &Profile,
) -> Result<Plan, FilletError> {
    if edges.is_empty() {
        return Err(FilletError::NoEdges);
    }
    let noun = profile.noun();
    let size = profile.size();
    let all: Vec<DVec3> = shape.points.values().copied().collect();
    let tol = geom::tol(&all);
    if !size.is_finite() || size <= tol {
        return Err(FilletError::BadSize { noun, asked: size });
    }
    if let Profile::Round { segments, .. } = profile
        && !(1..=MAX_SEGMENTS).contains(segments)
    {
        return Err(FilletError::Segments {
            segments: *segments,
        });
    }
    // The uses of each edge: (face, from, to).
    let mut uses: Uses = BTreeMap::new();
    for (id, f) in &shape.faces {
        for l in &f.loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                uses.entry(key(a, b)).or_default().push((*id, a, b));
            }
        }
    }
    let mut strips: Vec<Strip> = Vec::with_capacity(edges.len());
    let mut by_key: BTreeMap<(VertexId, VertexId), usize> = BTreeMap::new();
    for &(edge, a, b) in edges {
        if by_key.contains_key(&key(a, b)) {
            continue;
        }
        let s = strip(shape, &uses, edge, a, b, profile, tol)?;
        by_key.insert(key(a, b), strips.len());
        strips.push(s);
    }
    let mut b = Builder {
        shape,
        tol,
        points: Vec::new(),
        subs: BTreeMap::new(),
        trims: Vec::new(),
        corners: Vec::new(),
        to_orient: Vec::new(),
        blend: BTreeSet::new(),
        rails: BTreeSet::new(),
    };
    let ends: BTreeSet<VertexId> = strips.iter().flat_map(|s| [s.p, s.q]).collect();
    // The faces at each end, from one pass over the faces.
    let mut at_vertex: BTreeMap<VertexId, Vec<(FaceId, VertexId, VertexId)>> = BTreeMap::new();
    for (id, f) in &shape.faces {
        for l in &f.loops {
            let k = l.len();
            for i in 0..k {
                if ends.contains(&l[i]) {
                    at_vertex.entry(l[i]).or_default().push((
                        *id,
                        l[(i + k - 1) % k],
                        l[(i + 1) % k],
                    ));
                }
            }
        }
    }
    for v in &ends {
        let at = around(*v, at_vertex.remove(v).unwrap_or_default())?;
        let sel: Vec<Option<usize>> = at
            .next
            .iter()
            .map(|w| by_key.get(&key(*v, *w)).copied())
            .collect();
        corner::corner(&mut b, &mut strips, *v, &at, &sel, profile)?;
    }
    assemble(b, strips, profile, ends)
}

/// The strip of one edge, without its ends.
fn strip(
    shape: &Shape,
    uses: &Uses,
    edge: EdgeId,
    a: VertexId,
    b: VertexId,
    profile: &Profile,
    tol: f64,
) -> Result<Strip, FilletError> {
    let not_solid = FilletError::NotSolidEdge { edge };
    let list = uses.get(&key(a, b)).ok_or(not_solid.clone())?;
    let [(fa, p, q), (fb, p2, q2)] = list.as_slice() else {
        return Err(not_solid);
    };
    if fa == fb || p2 != q || q2 != p {
        return Err(not_solid);
    }
    let (fa, fb, p, q) = (*fa, *fb, *p, *q);
    let (pp, qp) = (shape.points[&p], shape.points[&q]);
    let len = pp.distance(qp);
    let d = (qp - pp) / len;
    let (na, nb) = (shape.faces[&fa].plane.normal, shape.faces[&fb].plane.normal);
    let (phi, convex) = edge_kind(na, nb, d);
    if phi < 1e-6 || phi > std::f64::consts::PI - 1e-6 || len <= tol {
        return Err(FilletError::FlatEdge { edge });
    }
    let ua = na.cross(d).normalize();
    let ub = d.cross(nb).normalize();
    let (s, prof) = match profile {
        Profile::Round { radius, segments } => {
            let r = *radius;
            let s = r * (phi / 2.0).tan();
            let sign = if convex { -1.0 } else { 1.0 };
            let c = ua * s + na * (sign * r);
            let (wa, wb) = if convex { (na, nb) } else { (-na, -nb) };
            let n = *segments as usize;
            let prof: Vec<DVec3> = (0..=n)
                .map(|j| c + slerp(wa, wb, j as f64 / n as f64) * r)
                .collect();
            let chord = prof[0].distance(prof[1]);
            if chord <= tol {
                return Err(FilletError::TooSmall {
                    noun: profile.noun(),
                    asked: r,
                    segments: *segments,
                    edge,
                });
            }
            (s, prof)
        }
        Profile::Flat { distance } => (*distance, vec![ua * *distance, ub * *distance]),
    };
    Ok(Strip {
        edge,
        p,
        q,
        at: pp,
        len,
        d,
        faces: [fa, fb],
        normals: [na, nb],
        inward: [ua, ub],
        convex,
        phi,
        s,
        prof,
        ends: [None, None],
    })
}

/// The unit vector at the share `t` of the turn from `a` to `b` (both unit length).
pub(crate) fn slerp(a: DVec3, b: DVec3, t: f64) -> DVec3 {
    let c = a.cross(b);
    let ang = c.length().atan2(a.dot(b));
    if ang < 1e-12 {
        return a;
    }
    let s = ang.sin();
    (a * ((1.0 - t) * ang).sin() + b * (t * ang).sin()) / s
}

/// The faces around `v` as one walk (`items`: each use of `v` in a loop as face, previous and next
/// vertex), or the refusal when they are not one fan of three faces.
fn around(v: VertexId, items: Vec<(FaceId, VertexId, VertexId)>) -> Result<Around, FilletError> {
    let corner = |why: String| FilletError::Corner { vertex: v, why };
    let mut seen: BTreeSet<FaceId> = BTreeSet::new();
    for it in &items {
        if !seen.insert(it.0) {
            return Err(corner(format!(
                "face {} passes through the vertex more than once",
                it.0
            )));
        }
    }
    let mut by_prev: BTreeMap<VertexId, usize> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        if by_prev.insert(it.1, i).is_some() {
            return Err(corner(
                "more than two faces meet at an edge there, so it is no simple corner of a solid"
                    .into(),
            ));
        }
    }
    let mut order = Vec::with_capacity(items.len());
    let mut cur = 0usize;
    for _ in 0..items.len() {
        order.push(cur);
        match by_prev.get(&items[cur].2) {
            Some(&n) => cur = n,
            None => {
                return Err(corner(
                    "the faces there do not close around the vertex (an open edge)".into(),
                ));
            }
        }
    }
    if cur != 0 || order.iter().collect::<BTreeSet<_>>().len() != items.len() {
        return Err(corner(
            "the faces there make more than one fan around the vertex".into(),
        ));
    }
    if items.len() != 3 {
        return Err(corner(format!(
            "{} faces meet there; a fillet or a chamfer works at corners of exactly three faces. \
             A corner that an earlier fillet made has more faces: round all edges of a corner in \
             one step, or round the side edges first and the top after them",
            items.len()
        )));
    }
    Ok(Around {
        faces: order.iter().map(|i| items[*i].0).collect(),
        next: order.iter().map(|i| items[*i].2).collect(),
    })
}

/// The strip faces, the new loops of the changed faces and the checks.
fn assemble(
    mut b: Builder<'_>,
    strips: Vec<Strip>,
    profile: &Profile,
    gone: BTreeSet<VertexId>,
) -> Result<Plan, FilletError> {
    let noun = profile.noun();
    let asked = profile.size();
    let no_fit = |why: String| FilletError::DoesNotFit {
        noun,
        asked,
        why,
        most: None,
    };
    let mut new_faces: Vec<NewFace> = Vec::new();
    let mut seams: BTreeSet<(Pt, Pt)> = BTreeSet::new();
    // A face is flat when no point is farther from its plane than this.
    let flat_eps = b.tol * 1e-7;
    let (mut convex_bound, mut concave_bound) = (0.0, 0.0);
    for s in &strips {
        let (Some(e0), Some(e1)) = (&s.ends[0], &s.ends[1]) else {
            unreachable!("both ends of a strip are planned");
        };
        let n = s.n();
        for j in 0..=n {
            if e1.t[j] - e0.t[j] <= b.tol {
                return Err(no_fit(format!(
                    "edge {} ({:.2} mm) is too short for the rounds at its ends",
                    s.edge, s.len
                )));
            }
        }
        let expect = match profile {
            Profile::Round { .. } => None,
            Profile::Flat { .. } => Some(s.normals[0] + s.normals[1]),
        };
        // An end whose points left their lines (a point of another end was put in) gets a short
        // row of its own near the end, cut into triangles, so the rest of the strip stays flat
        // faces with long sides that never come near another end.
        let shortest = (0..=n)
            .map(|j| e1.t[j] - e0.t[j])
            .fold(f64::INFINITY, f64::min);
        let delta = (50.0 * b.tol).min(0.3 * shortest);
        let bent = |b: &Builder<'_>, e: &End| {
            e.between.iter().any(|w| !w.is_empty())
                || (0..=n).any(|j| b.pos(e.pts[j]).distance(s.point(j, e.t[j])) > flat_eps)
        };
        let cut = |b: &mut Builder<'_>, e: &End, sign: f64| End {
            pts: (0..=n)
                .map(|j| b.add(s.point(j, e.t[j] + sign * delta)))
                .collect(),
            t: (0..=n).map(|j| e.t[j] + sign * delta).collect(),
            between: vec![Vec::new(); n],
        };
        let c0 = if bent(&b, e0) {
            Some(cut(&mut b, e0, 1.0))
        } else {
            None
        };
        let c1 = if bent(&b, e1) {
            Some(cut(&mut b, e1, -1.0))
        } else {
            None
        };
        // The rails pass through the cuts: the faces next to the strip get the points.
        let (fa, fb) = (s.faces[0], s.faces[1]);
        if let Some(c) = &c0 {
            if let Some(x) = b.subs.get_mut(&fa).and_then(|m| m.get_mut(&s.p)) {
                x.push(c.pts[0]);
            }
            if let Some(x) = b.subs.get_mut(&fb).and_then(|m| m.get_mut(&s.p)) {
                x.insert(0, c.pts[n]);
            }
        }
        if let Some(c) = &c1 {
            if let Some(x) = b.subs.get_mut(&fa).and_then(|m| m.get_mut(&s.q)) {
                x.insert(0, c.pts[0]);
            }
            if let Some(x) = b.subs.get_mut(&fb).and_then(|m| m.get_mut(&s.q)) {
                x.push(c.pts[n]);
            }
        }
        let mut rows: Vec<(&End, &End)> = Vec::new();
        if let Some(c) = &c0 {
            rows.push((e0, c));
        }
        rows.push((c0.as_ref().unwrap_or(e0), c1.as_ref().unwrap_or(e1)));
        if let Some(c) = &c1 {
            rows.push((c, e1));
        }
        for (e0, e1) in rows {
            for j in 0..n {
                let mut l = vec![e0.pts[j]];
                l.extend_from_slice(&e0.between[j]);
                l.push(e0.pts[j + 1]);
                l.push(e1.pts[j + 1]);
                l.extend(e1.between[j].iter().rev());
                l.push(e1.pts[j]);
                l.dedup();
                let pts: Vec<DVec3> = l.iter().map(|p| b.pos(*p)).collect();
                let normal = geom::newell(&pts).normalize_or_zero();
                let want = expect.unwrap_or_else(|| {
                    slerp(s.normals[0], s.normals[1], (j as f64 + 0.5) / n as f64)
                });
                if normal.dot(want) <= 0.0 {
                    return Err(no_fit(format!(
                        "the strip of edge {} would turn inside out",
                        s.edge
                    )));
                }
                let role = Role::Strip {
                    edge: s.edge,
                    segment: j,
                };
                // A strip side that took a point of another end is not flat any more: triangles.
                let mid = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
                let off = pts
                    .iter()
                    .map(|p| (*p - mid).dot(normal).abs())
                    .fold(0.0, f64::max);
                if off <= flat_eps {
                    new_faces.push(NewFace {
                        points: l,
                        normal,
                        role,
                        looks: s.faces.to_vec(),
                    });
                    continue;
                }
                let plane = Plane::new(mid, normal).expect("a normal");
                let ring: Vec<DVec2> = pts.iter().map(|p| plane.project(*p)).collect();
                let tris = geom::triangulate_rings(&[ring]).ok_or_else(|| {
                    no_fit(format!(
                        "the strip of edge {} would fold at its end",
                        s.edge
                    ))
                })?;
                let boundary: BTreeSet<(Pt, Pt)> = (0..l.len())
                    .map(|i| pair(l[i], l[(i + 1) % l.len()]))
                    .collect();
                for t in tris {
                    let tri: Vec<Pt> = t.iter().map(|i| l[*i as usize]).collect();
                    let tp: Vec<DVec3> = tri.iter().map(|p| b.pos(*p)).collect();
                    let tn = geom::newell(&tp).normalize_or_zero();
                    for i in 0..3 {
                        let e = pair(tri[i], tri[(i + 1) % 3]);
                        if !boundary.contains(&e) {
                            seams.insert(e);
                        }
                    }
                    new_faces.push(NewFace {
                        points: tri,
                        normal: tn,
                        role,
                        looks: s.faces.to_vec(),
                    });
                }
            }
            for j in 0..=n {
                let e = pair(e0.pts[j], e1.pts[j]);
                if j == 0 || j == n {
                    // A round of one segment is a flat facet: its rails stay drawn.
                    if n > 1 {
                        b.rails.insert(e);
                    }
                } else {
                    b.blend.insert(e);
                }
            }
        }
        // The wedge between the edge and the rails, a little longer than the edge.
        let wedge = 0.5 * s.s * s.s * s.phi.sin() * (s.len + 4.0 * s.s);
        if s.convex {
            convex_bound += wedge;
        } else {
            concave_bound += wedge;
        }
    }
    // The new loops of the changed faces.
    let mut faces: BTreeMap<FaceId, Vec<Vec<Pt>>> = BTreeMap::new();
    for (id, subs) in &b.subs {
        let f = &b.shape.faces[id];
        let loops: Vec<Vec<Pt>> = f
            .loops
            .iter()
            .map(|l| {
                l.iter()
                    .flat_map(|v| match subs.get(v) {
                        Some(s) => s.clone(),
                        None => vec![Pt::Old(*v)],
                    })
                    .collect()
            })
            .collect();
        fit::face(&b, *id, f, subs, &loops).map_err(no_fit)?;
        faces.insert(*id, loops);
    }
    // The corner pieces that take their side from their neighbours.
    let mut used: BTreeMap<(Pt, Pt), Vec<bool>> = BTreeMap::new();
    let note = |l: &[Pt], used: &mut BTreeMap<(Pt, Pt), Vec<bool>>| {
        for i in 0..l.len() {
            let (x, y) = (l[i], l[(i + 1) % l.len()]);
            used.entry(pair(x, y)).or_default().push(x < y);
        }
    };
    for f in &new_faces {
        note(&f.points, &mut used);
    }
    for l in faces.values().flatten() {
        note(l, &mut used);
    }
    for (i, c) in b.corners.iter().enumerate() {
        if !b.to_orient.contains(&i) {
            note(&c.points, &mut used);
        }
    }
    let orient = b.to_orient.clone();
    for i in orient {
        let c = &b.corners[i];
        let Role::Corner { vertex } = c.role else {
            continue;
        };
        let (mut keep, mut flip) = (0, 0);
        for k in 0..c.points.len() {
            let (x, y) = (c.points[k], c.points[(k + 1) % c.points.len()]);
            if let Some(ds) = used.get(&pair(x, y)) {
                for d in ds {
                    if *d == (x < y) {
                        flip += 1;
                    } else {
                        keep += 1;
                    }
                }
            }
        }
        if keep > 0 && flip > 0 || keep + flip == 0 {
            return Err(no_fit(format!(
                "the corner piece at vertex {vertex} would fold over its neighbours"
            )));
        }
        let mut pts = c.points.clone();
        if flip > 0 {
            pts.reverse();
        }
        let pos: Vec<DVec3> = pts.iter().map(|p| b.pos(*p)).collect();
        let normal = geom::newell(&pos).normalize_or_zero();
        let c = &mut b.corners[i];
        c.points = pts;
        c.normal = normal;
    }
    for c in &b.corners {
        let pos: Vec<DVec3> = c.points.iter().map(|p| b.pos(*p)).collect();
        if let Role::Corner { vertex } = c.role {
            fit::piece(&pos, c.normal, b.tol)
                .map_err(|why| no_fit(format!("the corner piece at vertex {vertex} {why}")))?;
        }
    }
    new_faces.extend(b.corners);
    Ok(Plan {
        points: b.points,
        faces,
        new_faces,
        trims: b.trims,
        gone,
        blend: b.blend,
        rails: b.rails,
        seams,
        convex_bound,
        concave_bound,
    })
}

/// The largest size up to `upto` with which the plan works (0 when none), found by halving down
/// to a sixteenth of the length tolerance.
pub fn largest_fit(
    shape: &Shape,
    edges: &[(EdgeId, VertexId, VertexId)],
    profile: &Profile,
    upto: f64,
) -> f64 {
    let all: Vec<DVec3> = shape.points.values().copied().collect();
    let tol = geom::tol(&all);
    let (mut lo, mut hi) = (0.0, upto);
    let steps = ((upto / tol).log2().ceil() + 4.0).clamp(1.0, 48.0) as usize;
    for _ in 0..steps {
        let mid = (lo + hi) / 2.0;
        if plan(shape, edges, &profile.resized(mid)).is_ok() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// A point of a plane in its 2D axes.
fn flat(plane: &Plane, p: DVec3) -> DVec2 {
    plane.project(p)
}
