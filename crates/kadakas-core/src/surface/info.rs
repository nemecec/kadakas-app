//! What Object Info and the agent API tell about a surface model (MOD-13): the counts, the
//! closed-solid state with the volume and the area, the box, and the materials. The same numbers
//! for a surface model, for a group of them and for an instance of a component (all the models of
//! its definition, through the transform of the instance). Everything is in the drawing: the
//! transform of a model is applied to the volume and the area.

use std::collections::BTreeMap;

use glam::{DMat3, DVec3};
use serde::Serialize;

use crate::model::{Document, Object, Shape};
use crate::present::PaintResolver;
use crate::surface::plan::{self, Part};

/// The faces that one answer checks for a closed solid by itself (the check takes about a second at
/// the limits of a model, see `docs/modeler-limits.md`). The budget counts the faces of all the
/// models of the answer, and a model that was checked before and has not changed costs nothing.
pub const AUTO_CHECK_FACES: usize = 20_000;

/// The faces that a list of objects (`objects.list`) checks in one answer.
pub const LIST_CHECK_FACES: usize = 60_000;

/// How many faces may still be checked for a closed solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget(pub usize);

impl Budget {
    /// [`AUTO_CHECK_FACES`].
    pub fn auto() -> Budget {
        Budget(AUTO_CHECK_FACES)
    }

    /// No check.
    pub fn never() -> Budget {
        Budget(0)
    }

    /// Every model.
    pub fn always() -> Budget {
        Budget(usize::MAX)
    }
}

/// When the closed-solid check runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// Never: a model with faces is `Unchecked`.
    Never,
    /// For a model with at most [`AUTO_CHECK_FACES`] faces.
    Auto,
    /// For every model.
    Always,
}

/// Whether the models close a volume.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SolidState {
    /// The check was not run (the model is large).
    Unchecked,
    /// No faces.
    Empty,
    /// Every shell is a closed solid and no edge is loose. The volume is in mm3.
    Closed { solids: usize, volume: f64 },
    /// At least one shell has open edges, or an edge is loose. `open_edges` counts the edges that
    /// make a shell open and the loose edges.
    Open { open_edges: usize },
    /// Closed, but an edge of a shell passes through a face of it with no edge there.
    Crossing { crossings: usize },
    /// Closed, but the fronts of the faces look in (a negative volume).
    Inward,
    /// Closed, but it holds no volume (two faces back to back).
    Flat,
}

/// The numbers of a surface model, a group of them or an instance.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SurfaceInfo {
    /// The surface models counted.
    pub models: usize,
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
    pub hidden_faces: usize,
    pub hidden_edges: usize,
    pub soft_edges: usize,
    pub solid: SolidState,
    /// The area of all faces in mm2.
    pub area: f64,
    /// The box around the models in the drawing.
    pub bounds: Option<([f64; 3], [f64; 3])>,
    /// The materials that paint the faces (front or back) and the number of faces of each, by name.
    pub materials: Vec<MaterialCount>,
    /// The faces with no material on either side.
    pub unpainted_faces: usize,
}

/// A material and the faces that use it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MaterialCount {
    pub name: String,
    pub faces: usize,
}

/// The factor by which a transform scales the area of a plane with the unit normal `n`.
fn area_factor(m: &DMat3, n: DVec3) -> f64 {
    let det = m.determinant();
    if det.abs() < 1e-18 {
        return 0.0;
    }
    (m.inverse().transpose() * n).length() * det.abs()
}

/// A cheap test that the faces of a model that are not hidden close a volume: every edge is used
/// by exactly two of them, once in each direction. It takes time in proportion to the number of
/// corners, far less than the closed-solid check. A model without faces is not watertight. The
/// section drawing uses it to tell a cut with an inside from a cut of a surface.
pub fn is_watertight(m: &crate::surface::SurfaceModel) -> bool {
    use std::collections::HashMap;
    let mut uses: HashMap<(u32, u32), (u8, u8)> = HashMap::new();
    let mut any = false;
    for (id, f) in m.faces() {
        if m.face_hidden(id) {
            continue;
        }
        any = true;
        for (a, b) in f.uses() {
            let e = uses.entry((a.0.min(b.0), a.0.max(b.0))).or_default();
            if a.0 <= b.0 {
                e.0 = e.0.saturating_add(1);
            } else {
                e.1 = e.1.saturating_add(1);
            }
        }
    }
    any && uses.values().all(|u| *u == (1, 1))
}

/// What the closed-solid check found for one model, in the local coordinates of the model.
#[derive(Clone, Copy, Debug)]
enum Verdict {
    Closed { solids: usize, volume: f64 },
    Open { edges: usize },
    Crossing { crossings: usize },
    Inward,
    Flat,
}

thread_local! {
    /// The verdicts of the models that were checked, by what the model is made of. A model that
    /// has not changed is not checked again, whatever else changed in the document.
    static VERDICTS: std::cell::RefCell<std::collections::HashMap<u64, Verdict>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Most verdicts that are kept.
const MAX_VERDICTS: usize = 512;

/// What the model is made of, as a number: its vertices, edges and faces (not its placement, which
/// does not change a solid). It takes time in proportion to the size of the model, far less than
/// the check.
fn content_key(m: &crate::surface::SurfaceModel) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (id, v) in m.vertices() {
        (id.0, v.x.to_bits(), v.y.to_bits(), v.z.to_bits()).hash(&mut h);
    }
    for (id, e) in m.edges() {
        (id.0, e.v[0].0, e.v[1].0).hash(&mut h);
    }
    for (id, f) in m.faces() {
        id.0.hash(&mut h);
        for l in f.loops() {
            l.len().hash(&mut h);
            for v in l {
                v.0.hash(&mut h);
            }
        }
    }
    h.finish()
}

fn verdict_of(m: &crate::surface::SurfaceModel) -> Verdict {
    use crate::surface::ShellState;
    let report = m.solid_report();
    if report.is_closed() {
        return Verdict::Closed {
            solids: report.solids().count(),
            volume: report.solids().map(|s| s.volume).sum(),
        };
    }
    let open = report.open_edges().len() + report.wire_edges.len();
    if open > 0 {
        return Verdict::Open { edges: open };
    }
    let has = |st: ShellState| report.shells.iter().any(|s| s.state == st);
    if has(ShellState::SelfCrossing) {
        Verdict::Crossing {
            crossings: report.crossings.max(1),
        }
    } else if has(ShellState::Flat) {
        Verdict::Flat
    } else {
        Verdict::Inward
    }
}

/// The verdict for a model, from the cache or by the check. `None` when the budget does not allow
/// the check.
fn verdict(m: &crate::surface::SurfaceModel, budget: &mut Budget) -> Option<Verdict> {
    let key = content_key(m);
    if let Some(v) = VERDICTS.with(|c| c.borrow().get(&key).copied()) {
        return Some(v);
    }
    if m.face_count() > budget.0 {
        return None;
    }
    budget.0 = budget.0.saturating_sub(m.face_count());
    let v = verdict_of(m);
    VERDICTS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= MAX_VERDICTS {
            c.clear();
        }
        c.insert(key, v);
    });
    Some(v)
}

/// The numbers of the parts. `check` says when the closed-solid check runs.
pub fn of_parts(doc: &Document, parts: &[Part], check: Check) -> SurfaceInfo {
    let mut budget = match check {
        Check::Never => Budget::never(),
        Check::Auto => Budget::auto(),
        Check::Always => Budget::always(),
    };
    of_parts_with(doc, parts, &mut budget)
}

/// Like [`of_parts`], with a budget of faces for the check that several answers share.
pub fn of_parts_with(doc: &Document, parts: &[Part], budget: &mut Budget) -> SurfaceInfo {
    let resolver = PaintResolver::new(doc);
    let mut info = SurfaceInfo {
        models: parts.len(),
        vertices: 0,
        edges: 0,
        faces: 0,
        hidden_faces: 0,
        hidden_edges: 0,
        soft_edges: 0,
        solid: SolidState::Empty,
        area: 0.0,
        bounds: None,
        materials: Vec::new(),
        unpainted_faces: 0,
    };
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    let (mut solids, mut volume) = (0usize, 0.0f64);
    let (mut open, mut crossings, mut flat, mut inward) = (0usize, 0usize, false, false);
    let mut unchecked = false;
    let mut bounds: Option<(DVec3, DVec3)> = None;
    for p in parts {
        let m = p.model;
        info.vertices += m.vertex_count();
        info.edges += m.edge_count();
        info.faces += m.face_count();
        info.hidden_faces += m.hidden_faces().len();
        info.hidden_edges += m.hidden_edges().len();
        info.soft_edges += m.edges().filter(|(id, _)| m.edge_soft(*id)).count();
        let mx = p.xf.matrix3;
        for (id, f) in m.faces() {
            info.area += m.face_area(id).unwrap_or(0.0) * area_factor(&mx, f.plane.normal);
            let (front, back) = resolver.faces(&f.attrs);
            // A material that paints both sides of a face counts the face once.
            let mut named: Vec<crate::resource::ResourceId> = Vec::new();
            for r in [front, back].into_iter().flatten() {
                if doc.resources.get(r).is_some() && !named.contains(&r) {
                    named.push(r);
                }
            }
            for r in &named {
                if let Some(res) = doc.resources.get(*r) {
                    *by_name.entry(res.name.clone()).or_default() += 1;
                }
            }
            if named.is_empty() {
                info.unpainted_faces += 1;
            }
        }
        if let Some((lo, hi)) = m.local_bounds() {
            for i in 0..8 {
                let c = DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                );
                let w = p.xf.transform_point3(c);
                bounds = Some(bounds.map_or((w, w), |(a, b)| (a.min(w), b.max(w))));
            }
        }
        if m.face_count() == 0 {
            continue;
        }
        let Some(v) = verdict(m, budget) else {
            unchecked = true;
            continue;
        };
        let det = mx.determinant().abs();
        match v {
            Verdict::Closed {
                solids: n,
                volume: v,
            } => {
                solids += n;
                volume += v * det;
            }
            Verdict::Open { edges } => open += edges,
            Verdict::Crossing { crossings: n } => crossings += n,
            Verdict::Flat => flat = true,
            Verdict::Inward => inward = true,
        }
    }
    info.bounds = bounds.map(|(a, b)| (a.to_array(), b.to_array()));
    info.materials = by_name
        .into_iter()
        .map(|(name, faces)| MaterialCount { name, faces })
        .collect();
    info.solid = if info.faces == 0 {
        SolidState::Empty
    } else if open > 0 {
        SolidState::Open { open_edges: open }
    } else if crossings > 0 {
        SolidState::Crossing { crossings }
    } else if flat {
        SolidState::Flat
    } else if inward {
        SolidState::Inward
    } else if unchecked {
        SolidState::Unchecked
    } else {
        // The volumes of the solids of a group are added. Solids that overlap count twice.
        SolidState::Closed { solids, volume }
    };
    info
}

/// The numbers of an object: a surface model, an instance of a component, or a group (all of its
/// members that draw as surface models). `None` for any other object, and for a group with no
/// such member.
pub fn of_object(doc: &Document, obj: &Object, check: Check) -> Option<SurfaceInfo> {
    let mut budget = match check {
        Check::Never => Budget::never(),
        Check::Auto => Budget::auto(),
        Check::Always => Budget::always(),
    };
    of_object_with(doc, obj, &mut budget)
}

/// Like [`of_object`], with a budget of faces for the check that several objects share (a list).
pub fn of_object_with(doc: &Document, obj: &Object, budget: &mut Budget) -> Option<SurfaceInfo> {
    let ids = match obj.shape {
        Shape::Group { .. } => doc.leaves(&[obj.id]),
        Shape::SurfaceModel(_) | Shape::Component { .. } => vec![obj.id],
        _ => return None,
    };
    let mut parts: Vec<Part> = Vec::new();
    for id in ids {
        let o = doc.object(id)?;
        if let Some(Ok(p)) = plan::parts_of(doc, o) {
            parts.extend(p);
        }
    }
    (!parts.is_empty()).then(|| of_parts_with(doc, &parts, budget))
}
