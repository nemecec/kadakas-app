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

/// A model with more faces than this is not checked for a closed solid unless the caller asks (the
/// check takes about a second at the limits of a model, see `docs/modeler-limits.md`).
pub const AUTO_CHECK_FACES: usize = 20_000;

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
    /// At least one shell is open, crosses itself, looks inward or has no volume, or an edge is
    /// loose. `open_edges` counts the edges that make a shell open and the loose edges.
    Open { open_edges: usize },
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

/// The numbers of the parts. `check` says when the closed-solid check runs.
pub fn of_parts(doc: &Document, parts: &[Part], check: Check) -> SurfaceInfo {
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
    let (mut solids, mut volume, mut open) = (0usize, 0.0f64, 0usize);
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
            let mut named = false;
            for r in [front, back].into_iter().flatten() {
                if let Some(res) = doc.resources.get(r) {
                    *by_name.entry(res.name.clone()).or_default() += 1;
                    named = true;
                }
            }
            if !named {
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
        let skip = match check {
            Check::Never => true,
            Check::Auto => m.face_count() > AUTO_CHECK_FACES,
            Check::Always => false,
        };
        if skip {
            unchecked = true;
            continue;
        }
        let report = m.solid_report();
        let det = mx.determinant().abs();
        if report.is_closed() {
            solids += report.solids().count();
            volume += report.solids().map(|s| s.volume).sum::<f64>() * det;
        } else {
            open += report.open_edges().len() + report.wire_edges.len();
            // A shell that is open for another reason (it crosses itself, looks inward or is
            // flat) counts as one.
            open = open.max(1);
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
    } else if unchecked {
        SolidState::Unchecked
    } else {
        SolidState::Closed { solids, volume }
    };
    info
}

/// The numbers of an object: a surface model, an instance of a component, or a group (all of its
/// members that draw as surface models). `None` for any other object, and for a group with no
/// such member.
pub fn of_object(doc: &Document, obj: &Object, check: Check) -> Option<SurfaceInfo> {
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
    (!parts.is_empty()).then(|| of_parts(doc, &parts, check))
}
