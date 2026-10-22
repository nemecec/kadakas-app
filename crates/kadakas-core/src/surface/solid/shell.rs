//! Shells and the closed-solid check.
//!
//! A *shell* is the set of faces that a walk finds from a seed face (see the module `solid`). The
//! walk goes over each edge of a face to the face next to it in the material:
//!
//! - an edge used by exactly two faces leads to the other face;
//! - an edge used by three or more faces (two solids that share the edge after Split or Trim, or a
//!   fin) leads to the face that comes first when the face turns toward its back, around the edge
//!   (the radial order of a radial-edge kernel). A face that lies exactly in the half plane of the
//!   face (back to back with it) is no neighbour.
//!
//! A shell is closed when each of its edges is used twice by its own faces, in opposite directions.
//! Its volume is positive when its fronts look out. A shell whose faces look into a hollow (a
//! cavity) has a negative volume; it belongs to the solid that holds it.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::super::check::Problem;
use super::super::geom;
use super::super::topo::Topo;
use super::super::{EdgeId, FaceId, SurfaceModel, VertexId, key};
use super::locate::{Soup, Where, centroids};

/// Why an edge makes a shell open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenKind {
    /// Only one face of the shell uses the edge: the border of an open surface.
    Border,
    /// Three or more faces of the shell use the edge.
    NonManifold,
    /// Two faces use the edge in the same direction: they disagree about front and back.
    Reversed,
}

impl std::fmt::Display for OpenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OpenKind::Border => "only one face uses it",
            OpenKind::NonManifold => "three or more faces use it",
            OpenKind::Reversed => "two faces use it the same way round",
        })
    }
}

/// An edge that makes a shell open, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct OpenEdge {
    pub edge: EdgeId,
    pub kind: OpenKind,
}

/// What a shell is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellState {
    /// Closed, with a volume, and the fronts look out: a solid.
    Solid,
    /// It has open edges.
    Open,
    /// An edge of the shell passes through a face of the shell without an edge there.
    SelfCrossing,
    /// Closed, but the fronts look in (a negative volume). A cavity inside a solid looks like this.
    Inverted,
    /// Closed, but it holds no volume (two faces back to back, for example).
    Flat,
}

/// A set of faces and what the check found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Shell {
    /// The faces, in the order of their ids.
    pub faces: Vec<FaceId>,
    pub state: ShellState,
    /// The edges that make the shell open, in the order of their ids.
    pub open_edges: Vec<OpenEdge>,
    /// Edges of the shell that pass through a face of the shell with no edge there.
    pub crossings: Vec<(EdgeId, FaceId)>,
    /// The signed volume in mm³ (positive when the fronts look out).
    pub volume: f64,
    /// The sum of the face areas in mm².
    pub area: f64,
}

impl Shell {
    /// True if the shell is a solid: closed, with a volume, and the fronts look out.
    pub fn is_solid(&self) -> bool {
        self.state == ShellState::Solid
    }
}

/// The result of the closed-solid check of a whole model.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SolidReport {
    /// The shells, by the lowest face id of each. A face that two shells share is in both.
    pub shells: Vec<Shell>,
    /// Edges that no face uses.
    pub wire_edges: Vec<EdgeId>,
    /// The places where an edge of one shell passes through a face of another shell (or a line
    /// through a face) with no edge there. Intersect Faces makes the edges; two solids that
    /// cross are cut by the solid tools themselves.
    pub crossings: usize,
}

impl SolidReport {
    /// True for a model with faces in which every shell is a solid and there is no loose edge.
    pub fn is_closed(&self) -> bool {
        !self.shells.is_empty()
            && self.wire_edges.is_empty()
            && self.shells.iter().all(Shell::is_solid)
    }

    /// The edges that make the shells open, in the order of their ids (the edges that no face
    /// uses are in `wire_edges`).
    pub fn open_edges(&self) -> Vec<OpenEdge> {
        let mut out: Vec<OpenEdge> = self
            .shells
            .iter()
            .flat_map(|s| s.open_edges.iter().copied())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The solids of the model: the shells that are solids.
    pub fn solids(&self) -> impl Iterator<Item = &Shell> {
        self.shells.iter().filter(|s| s.is_solid())
    }
}

type Uses = BTreeMap<(VertexId, VertexId), Vec<(FaceId, VertexId, VertexId)>>;

/// Every use of an edge, by its vertex pair: the face and the direction of the use.
fn uses_of(m: &SurfaceModel) -> Uses {
    let mut uses: Uses = BTreeMap::new();
    for (&id, f) in &m.faces {
        for (a, b) in f.uses() {
            uses.entry(key(a, b)).or_default().push((id, a, b));
        }
    }
    uses
}

/// The face next to `f` at its use `a`→`b`, or `None` when it has none.
fn partner(
    m: &SurfaceModel,
    f: FaceId,
    (a, b): (VertexId, VertexId),
    list: &[(FaceId, VertexId, VertexId)],
) -> Option<FaceId> {
    let others: Vec<&(FaceId, VertexId, VertexId)> = list.iter().filter(|u| u.0 != f).collect();
    match others.as_slice() {
        [] => None,
        [one] => Some(one.0),
        _ => {
            let n = m.faces[&f].plane.normal;
            let t = (m.vertices[&b] - m.vertices[&a]).normalize_or_zero();
            // In the face, at a right angle to the edge, into the face.
            let e1 = n.cross(t);
            let e2 = t.cross(e1);
            // The material is behind the face.
            let s = (-n).dot(e2).signum();
            // The faces around the edge, in the order they come in when `f` turns toward its back.
            // A face bounds the material between itself and `f` only when its front looks away
            // from that wedge; a face whose front looks into the wedge belongs to another solid.
            let mut ahead: Vec<(f64, FaceId, bool)> = Vec::new();
            for &&(g, c, d) in &others {
                let ng = m.faces[&g].plane.normal;
                let td = (m.vertices[&d] - m.vertices[&c]).normalize_or_zero();
                let ug = ng.cross(td);
                let phi = (s * ug.dot(e2)).atan2(ug.dot(e1));
                let psi = phi.rem_euclid(std::f64::consts::TAU);
                // A face in the same half plane is back to back with `f`: no neighbour.
                if !(1e-7..=std::f64::consts::TAU - 1e-7).contains(&psi) {
                    continue;
                }
                // From the face back into the wedge.
                let back = psi.sin() * e1 - s * psi.cos() * e2;
                ahead.push((psi, g, ng.dot(back) < -1e-9));
            }
            let first = |only_facing: bool| {
                ahead
                    .iter()
                    .filter(|(_, _, ok)| *ok || !only_facing)
                    .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
                    .map(|(_, g, _)| *g)
            };
            return first(true).or_else(|| first(false));
        }
    }
}

/// The faces that the walk finds from `seed`.
fn walk(m: &SurfaceModel, uses: &Uses, seed: FaceId) -> BTreeSet<FaceId> {
    let mut set = BTreeSet::from([seed]);
    let mut queue = vec![seed];
    while let Some(f) = queue.pop() {
        for (a, b) in m.faces[&f].uses() {
            let Some(list) = uses.get(&key(a, b)) else {
                continue;
            };
            if let Some(g) = partner(m, f, (a, b), list)
                && set.insert(g)
            {
                queue.push(g);
            }
        }
    }
    set
}

/// The volume (mm³) and the area (mm²) of the faces, and the size of their coordinates.
fn measure(m: &SurfaceModel, set: &BTreeSet<FaceId>) -> (f64, f64, f64) {
    let Some(origin) = set
        .iter()
        .next()
        .and_then(|f| m.faces[f].outer.first())
        .map(|v| m.vertices[v])
    else {
        return (0.0, 0.0, 0.0);
    };
    let (mut volume, mut area, mut size) = (0.0, 0.0, 0.0f64);
    for id in set {
        let f = &m.faces[id];
        let a = geom::face_area(f, |v| m.vertices[&v]);
        let p0 = m.vertices[&f.outer[0]];
        volume += f.plane.normal.dot(p0 - origin) * a / 3.0;
        area += a;
        size = size.max(geom::size(p0));
    }
    (volume, area, size)
}

/// The shell of the faces `set`: its open edges, crossings, volume and state. `crossings` are the
/// `NotIntersected` pairs of the check of the whole model.
pub(super) fn state_with(
    m: &SurfaceModel,
    topo: &Topo,
    set: &BTreeSet<FaceId>,
    crossings: &[(EdgeId, FaceId)],
) -> Shell {
    let mut dirs: BTreeMap<(VertexId, VertexId), Vec<bool>> = BTreeMap::new();
    for id in set {
        for (a, b) in m.faces[id].uses() {
            dirs.entry(key(a, b)).or_default().push(a < b);
        }
    }
    let mut open_edges = Vec::new();
    for (k, d) in &dirs {
        let kind = match d.as_slice() {
            [_] => Some(OpenKind::Border),
            [x, y] if x != y => None,
            [_, _] => Some(OpenKind::Reversed),
            _ => Some(OpenKind::NonManifold),
        };
        if let (Some(kind), Some(edge)) = (kind, topo.edge(k.0, k.1)) {
            open_edges.push(OpenEdge { edge, kind });
        }
    }
    open_edges.sort();
    let own: Vec<(EdgeId, FaceId)> = crossings
        .iter()
        .copied()
        .filter(|(e, f)| set.contains(f) && topo.faces_of(*e).any(|g| set.contains(&g)))
        .collect();
    let (volume, area, size) = measure(m, set);
    let flat = geom::tolerance_at(size) * area / 2.0;
    let state = if !open_edges.is_empty() {
        ShellState::Open
    } else if !own.is_empty() {
        ShellState::SelfCrossing
    } else if volume.abs() <= flat {
        ShellState::Flat
    } else if volume < 0.0 {
        ShellState::Inverted
    } else {
        ShellState::Solid
    };
    Shell {
        faces: set.iter().copied().collect(),
        state,
        open_edges,
        crossings: own,
        volume,
        area,
    }
}

/// [`state_with`] for a set of faces, running the check of the model for the crossings.
pub(super) fn state_of(m: &SurfaceModel, set: &BTreeSet<FaceId>) -> Shell {
    let topo = Topo::of(m);
    let crossings = crossings_of(&m.check().warnings);
    state_with(m, &topo, set, &crossings)
}

/// The `NotIntersected` pairs among the warnings of a check.
pub(super) fn crossings_of(warnings: &[Problem]) -> Vec<(EdgeId, FaceId)> {
    warnings
        .iter()
        .filter_map(|p| match p {
            Problem::NotIntersected(e, f, _) => Some((*e, *f)),
            _ => None,
        })
        .collect()
}

/// The shells of a model by the walk, with the cavities put into the solids that hold them.
fn shells(m: &SurfaceModel, topo: &Topo, crossings: &[(EdgeId, FaceId)]) -> Vec<Shell> {
    let uses = uses_of(m);
    let mut covered: BTreeSet<FaceId> = BTreeSet::new();
    let mut shells: Vec<Shell> = Vec::new();
    for &seed in m.faces.keys() {
        if covered.contains(&seed) {
            continue;
        }
        let set = walk(m, &uses, seed);
        covered.extend(set.iter().copied());
        shells.push(state_with(m, topo, &set, crossings));
    }
    put_cavities_in_solids(m, topo, crossings, shells)
}

/// A closed shell that looks inward and lies inside a solid is a cavity of that solid (the
/// smallest one that holds it): its faces join the solid, whose volume is then less.
fn put_cavities_in_solids(
    m: &SurfaceModel,
    topo: &Topo,
    crossings: &[(EdgeId, FaceId)],
    mut shells: Vec<Shell>,
) -> Vec<Shell> {
    let hollow: Vec<usize> = (0..shells.len())
        .filter(|&i| shells[i].state == ShellState::Inverted)
        .collect();
    if hollow.is_empty() {
        return shells;
    }
    let hosts: Vec<usize> = (0..shells.len())
        .filter(|&i| shells[i].state == ShellState::Solid)
        .collect();
    let soups: BTreeMap<usize, Soup> = hosts
        .iter()
        .filter_map(|&i| Soup::of(m, shells[i].faces.iter().copied()).map(|s| (i, s)))
        .collect();
    let mut joined: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &c in &hollow {
        let Some(p) = centroids(m, shells[c].faces[0]).and_then(|v| v.first().copied()) else {
            continue;
        };
        let host = soups
            .iter()
            .filter(|(i, s)| {
                let (lo, hi) = s.bounds();
                !(p.cmplt(lo).any() || p.cmpgt(hi).any())
                    && matches!(s.locate(p), Some(Where::In))
                    && **i != c
            })
            .map(|(i, _)| *i)
            .min_by(|a, b| shells[*a].volume.total_cmp(&shells[*b].volume));
        if let Some(h) = host {
            joined.entry(h).or_default().push(c);
        }
    }
    let mut gone: BTreeSet<usize> = BTreeSet::new();
    for (h, cs) in joined {
        let mut set: BTreeSet<FaceId> = shells[h].faces.iter().copied().collect();
        for c in &cs {
            set.extend(shells[*c].faces.iter().copied());
            gone.insert(*c);
        }
        shells[h] = state_with(m, topo, &set, crossings);
    }
    shells
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !gone.contains(i))
        .map(|(_, s)| s)
        .collect()
}

/// The closed-solid check of a model.
pub(super) fn report(m: &SurfaceModel) -> SolidReport {
    let check = m.check();
    let topo = Topo::of(m);
    let crossings = crossings_of(&check.warnings);
    let shells = shells(m, &topo, &crossings);
    let crossings = check.not_intersected();
    SolidReport {
        shells,
        wire_edges: check.wire_edges,
        crossings,
    }
}

/// The faces of the solid that holds `face`, with its cavities.
pub(super) fn solid_of(m: &SurfaceModel, face: FaceId) -> Option<Vec<FaceId>> {
    if !m.faces.contains_key(&face) {
        return None;
    }
    let topo = Topo::of(m);
    let uses = uses_of(m);
    let set = walk(m, &uses, face);
    let mut shell = state_with(m, &topo, &set, &[]);
    if shell.state != ShellState::Solid {
        return Some(shell.faces);
    }
    // Cavities: the shells that look inward and lie inside this one.
    let all = shells(m, &topo, &[]);
    if let Some(s) = all.into_iter().find(|s| s.faces.contains(&face)) {
        shell = s;
    }
    Some(shell.faces)
}
