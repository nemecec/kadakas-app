//! The edits of `mesh.*`: thin wrappers over `edit_surface` and the group and component commands.
//!
//! A call is checked, turned into the commands of the core, and run in one undo step with
//! `Editor::execute_stepwise`. `mesh.batch` runs the commands of many edits in the same way, so
//! it is one undo step and all or nothing. This file builds commands and describes the change. The
//! geometry is the code of `kadakas_core::surface`.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DAffine3, DQuat, DVec3};
use serde_json::{Value, json};

use kadakas_core::command::{Command, CommandError};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::resource::ResourceId;
use kadakas_core::surface::{
    CurveError, CurveId, EdgeId, Edit, FaceId, IntersectScope, PaintSide, SolidError, SolidOp,
    SurfaceError, SurfaceModel, VertexId, soften_plan,
};

use super::{create_command, model_of, object_id, typed};
use crate::errors::bad;
use crate::params::mesh::*;
use crate::params::{self, ObjectRef};
use crate::{RpcError, Session};

/// The most edits in one `mesh.batch`.
///
/// Measured through the API (release build, a model of 72,580 edges): 50 hides took 5.25 s and 48
/// Push/Pulls took 15.3 s, a command costing 0.1 to 0.3 s. `docs/modeler-limits.md` has the base:
/// about 0.125 s for a command on a model of 80,000 edges. So one batch stays a request that an
/// agent can wait for.
pub const BATCH_EDITS_MAX: usize = 50;
/// The most new edges in all `add_edges` edits of one `mesh.batch`. It is the limit of the edges
/// in one edit of the core (10,000), so a batch does not do more edge work than one big edit
/// (0.2 to 0.75 s in the same document).
pub const BATCH_EDGES_MAX: usize = 10_000;
/// The longest list of ids in the answer of one edit.
const LIST_MAX: usize = 1000;

/// The edits that `mesh.batch` takes.
const BATCH_OPS: [&str; 19] = [
    "add_edges",
    "add_curve",
    "set_segments",
    "erase",
    "reverse_faces",
    "push_pull",
    "follow_me",
    "offset",
    "intersect_faces",
    "transform",
    "copy",
    "paint",
    "hide",
    "soften",
    "soften_edges",
    "solid",
    "make_group",
    "make_component",
    "explode",
];

/// True for the name of an edit method (after `mesh.`), also `create`.
pub fn is_edit(op: &str) -> bool {
    op == "create" || BATCH_OPS.contains(&op)
}

fn faces(v: Option<Vec<u32>>) -> Vec<FaceId> {
    v.unwrap_or_default().into_iter().map(FaceId).collect()
}

fn edges(v: Option<Vec<u32>>) -> Vec<EdgeId> {
    v.unwrap_or_default().into_iter().map(EdgeId).collect()
}

/// Which faces a solid has: given, or the solid that holds one face (found when the edit runs).
#[derive(Clone, Debug)]
enum Pick {
    Faces(Vec<FaceId>),
    Holding(FaceId),
}

/// A command that is made when its turn comes, from the document as the edits before it left it.
#[derive(Clone, Debug)]
enum Step {
    Fixed(Box<Command>),
    /// Soften Edges on the given edges, else on all edges of the model.
    SoftenAll {
        id: ObjectId,
        edges: Option<Vec<EdgeId>>,
        angle: f64,
        smooth: bool,
        coplanar: bool,
    },
    Solid {
        id: ObjectId,
        op: SolidOp,
        first: Pick,
        second: Pick,
    },
}

impl Step {
    fn build(&self, doc: &Document) -> Result<Command, RpcError> {
        Ok(match self {
            Step::Fixed(c) => (**c).clone(),
            Step::SoftenAll {
                id,
                edges,
                angle,
                smooth,
                coplanar,
            } => {
                let m = model_of(doc, *id)?;
                Command::EditSurface {
                    id: *id,
                    edit: Edit::SoftenEdges {
                        edges: edges
                            .clone()
                            .unwrap_or_else(|| m.edges().map(|(e, _)| e).collect()),
                        angle: *angle,
                        smooth: *smooth,
                        coplanar: *coplanar,
                    },
                }
            }
            Step::Solid {
                id,
                op,
                first,
                second,
            } => {
                let m = model_of(doc, *id)?;
                let pick = |p: &Pick| -> Result<Vec<FaceId>, RpcError> {
                    match p {
                        Pick::Faces(f) => Ok(f.clone()),
                        Pick::Holding(f) => m.solid_of(*f).ok_or_else(|| no_face(*id, *f)),
                    }
                };
                Command::EditSurface {
                    id: *id,
                    edit: Edit::Solid {
                        op: *op,
                        first: pick(first)?,
                        second: pick(second)?,
                    },
                }
            }
        })
    }
}

fn fixed(c: Command) -> Step {
    Step::Fixed(Box::new(c))
}

fn no_face(object: ObjectId, f: FaceId) -> RpcError {
    typed(
        RpcError::not_found(format!("face {f} does not exist")),
        "no_such_face",
        json!({"object": object, "face": f}),
    )
}

/// One edit of the call: the steps to run and what to report.
struct Planned {
    op: &'static str,
    /// The surface model that the answer counts.
    model: Option<ObjectId>,
    steps: Vec<Step>,
    /// Soften Edges by angle: report the edges that changed.
    soften: bool,
    /// Report the objects and resources that appeared or went.
    objects: bool,
}

fn static_op(op: &str) -> &'static str {
    BATCH_OPS
        .iter()
        .copied()
        .chain(["create"])
        .find(|o| *o == op)
        .unwrap_or("edit")
}

/// The map of Move, Rotate and Scale as 12 numbers: scale, then rotate, then translate.
fn map_of(
    matrix: Option<[f64; 12]>,
    translate: Option<[f64; 3]>,
    rotate: Option<&MeshRotate>,
    scale: Option<&MeshScale>,
) -> Result<[f64; 12], RpcError> {
    if let Some(m) = matrix {
        if translate.is_some() || rotate.is_some() || scale.is_some() {
            return Err(bad(
                "give matrix, or any of translate, rotate and scale, not both",
            ));
        }
        if !m.iter().all(|v| v.is_finite()) {
            return Err(bad("matrix: 12 finite numbers"));
        }
        return Ok(m);
    }
    if translate.is_none() && rotate.is_none() && scale.is_none() {
        return Err(bad("give matrix, translate, rotate or scale"));
    }
    let finite = |p: [f64; 3], what: &str| -> Result<DVec3, RpcError> {
        let v = DVec3::from(p);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(bad(format!("{what}: three finite numbers")))
        }
    };
    let mut m = DAffine3::IDENTITY;
    if let Some(s) = scale {
        let about = finite(s.about.unwrap_or([0.0; 3]), "scale.about")?;
        let f = match (s.factor, s.factors) {
            (Some(f), None) => DVec3::splat(f),
            (None, Some(f)) => DVec3::from(f),
            _ => return Err(bad("scale: give factor or factors")),
        };
        if !f.is_finite() || f.cmpeq(DVec3::ZERO).any() {
            return Err(bad("scale: factors are finite and not zero"));
        }
        m = DAffine3::from_translation(about)
            * DAffine3::from_scale(f)
            * DAffine3::from_translation(-about)
            * m;
    }
    if let Some(r) = rotate {
        let about = finite(r.about.unwrap_or([0.0; 3]), "rotate.about")?;
        let axis = finite(r.axis, "rotate.axis")?
            .try_normalize()
            .ok_or_else(|| bad("rotate.axis: not zero"))?;
        if !r.degrees.is_finite() {
            return Err(bad("rotate.degrees: a finite number"));
        }
        let q = DQuat::from_axis_angle(axis, r.degrees.to_radians());
        m = DAffine3::from_translation(about)
            * DAffine3::from_quat(q)
            * DAffine3::from_translation(-about)
            * m;
    }
    if let Some(t) = translate {
        m = DAffine3::from_translation(finite(t, "translate")?) * m;
    }
    Ok(m.to_cols_array())
}

/// The material of a paint: the text of the ID, from an ID or a name.
fn material_id(doc: &Document, text: &str) -> Result<String, RpcError> {
    let by_id = kadakas_core::uid::Uid::parse(text.trim())
        .map(ResourceId)
        .filter(|id| kadakas_core::present::material(doc, *id).is_some());
    if let Some(id) = by_id {
        return Ok(id.0.to_string());
    }
    let mut hits = doc.resources.iter().filter(|r| {
        r.name.eq_ignore_ascii_case(text.trim())
            && kadakas_core::present::material(doc, r.id).is_some()
    });
    let first = hits.next().map(|r| r.id);
    if first.is_some() && hits.next().is_some() {
        return Err(bad(format!(
            "several materials are named '{text}'. Give the ID"
        )));
    }
    first
        .map(|id| id.0.to_string())
        .ok_or_else(|| bad(format!("there is no material '{text}'")))
}

fn parse<T: serde::de::DeserializeOwned>(p: Value) -> Result<T, RpcError> {
    serde_json::from_value(p).map_err(bad)
}

fn surface(id: ObjectId, edit: Edit) -> Step {
    fixed(Command::EditSurface { id, edit })
}

/// Builds the edit `op` from checked parameters. The models and faces are checked when the edit
/// runs, where the refusal of the core carries a typed id.
fn plan(s: &Session, op: &str, p: Value) -> Result<Planned, RpcError> {
    let doc = s.editor.doc();
    let mut out = Planned {
        op: static_op(op),
        model: None,
        steps: Vec::new(),
        soften: false,
        objects: false,
    };
    // The model that an edit works on is checked first: a missing one is the first thing to say.
    macro_rules! with_model {
        ($q:ident : $ty:ty) => {{
            let $q: $ty = parse(p)?;
            let id = object_id(&$q.id.0)?;
            editable_model(doc, id)?;
            out.model = Some(id);
            ($q, id)
        }};
    }
    match op {
        "create" => {
            let q: MeshCreateParams = parse(p)?;
            let (layer, class) = layer_class(&q)?;
            let cmd = create_command(q.name, q.origin, layer, class);
            out.steps.push(fixed(s.in_context(cmd)));
            out.objects = true;
        }
        "add_edges" => {
            let (q, id) = with_model!(q: MeshAddEdgesParams);
            out.steps
                .push(surface(id, Edit::AddEdges { edges: q.edges }));
        }
        "add_curve" => {
            let (q, id) = with_model!(q: MeshAddCurveParams);
            out.steps.push(surface(
                id,
                Edit::AddCurve {
                    points: q.points,
                    closed: q.closed.unwrap_or(false),
                    faceted: q.faceted.unwrap_or(false),
                    analytic: q.analytic,
                },
            ));
        }
        "set_segments" => {
            let (q, id) = with_model!(q: MeshSetSegmentsParams);
            out.steps.push(surface(
                id,
                Edit::SetCurveSegments {
                    curve: CurveId(q.curve),
                    segments: q.segments,
                },
            ));
        }
        "erase" => {
            let (q, id) = with_model!(q: MeshEraseParams);
            if q.edges.is_none() && q.faces.is_none() {
                return Err(bad("give edges or faces"));
            }
            // Faces first: their edges stay, so the edge ids are still good.
            if q.faces.is_some() {
                out.steps.push(surface(
                    id,
                    Edit::EraseFaces {
                        faces: faces(q.faces),
                    },
                ));
            }
            if q.edges.is_some() {
                out.steps.push(surface(
                    id,
                    Edit::EraseEdges {
                        edges: edges(q.edges),
                    },
                ));
            }
        }
        "reverse_faces" => {
            let (q, id) = with_model!(q: MeshReverseFacesParams);
            out.steps.push(surface(
                id,
                Edit::ReverseFaces {
                    faces: faces(Some(q.faces)),
                },
            ));
        }
        "push_pull" => {
            let (q, id) = with_model!(q: MeshPushPullParams);
            if !q.distance.is_finite() {
                return Err(bad("distance: a finite number"));
            }
            out.steps.push(surface(
                id,
                Edit::PushPull {
                    face: FaceId(q.face),
                    distance: q.distance,
                },
            ));
        }
        "follow_me" => {
            let (q, id) = with_model!(q: MeshFollowMeParams);
            out.steps.push(surface(
                id,
                Edit::FollowMe {
                    face: FaceId(q.face),
                    path: edges(Some(q.path)),
                },
            ));
        }
        "offset" => {
            let (q, id) = with_model!(q: MeshOffsetParams);
            if !q.distance.is_finite() {
                return Err(bad("distance: a finite number"));
            }
            let edit = match (q.face, q.edges) {
                (Some(f), None) => Edit::OffsetFace {
                    face: FaceId(f),
                    distance: q.distance,
                    outward: q.outward.unwrap_or(false),
                },
                (None, Some(e)) => Edit::OffsetEdges {
                    edges: edges(Some(e)),
                    distance: q.distance,
                    toward: q
                        .toward
                        .ok_or_else(|| bad("give toward, the side of the edges to draw on"))?,
                },
                _ => return Err(bad("give face or edges, not both")),
            };
            out.steps.push(surface(id, edit));
        }
        "intersect_faces" => {
            let (q, id) = with_model!(q: MeshIntersectFacesParams);
            let scope = match q.scope {
                Some(MeshScope::Selection) => IntersectScope::Selection,
                _ => IntersectScope::Model,
            };
            out.steps.push(surface(
                id,
                Edit::IntersectFaces {
                    scope,
                    faces: faces(q.faces),
                    edges: edges(q.edges),
                },
            ));
        }
        "transform" => {
            let (q, id) = with_model!(q: MeshTransformParams);
            if q.vertices.is_none() && q.faces.is_none() && q.edges.is_none() {
                return Err(bad("give vertices, faces or edges"));
            }
            let matrix = map_of(q.matrix, q.translate, q.rotate.as_ref(), q.scale.as_ref())?;
            out.steps.push(surface(
                id,
                Edit::TransformVertices {
                    vertices: q
                        .vertices
                        .unwrap_or_default()
                        .into_iter()
                        .map(VertexId)
                        .collect(),
                    faces: faces(q.faces),
                    edges: edges(q.edges),
                    matrix,
                },
            ));
        }
        "copy" => {
            let (q, id) = with_model!(q: MeshCopyParams);
            if q.faces.is_none() && q.edges.is_none() {
                return Err(bad("give faces or edges"));
            }
            let count = q.count.unwrap_or(1);
            if count == 0 {
                return Err(bad("count: 1 or more"));
            }
            let matrix = map_of(q.matrix, q.translate, q.rotate.as_ref(), q.scale.as_ref())?;
            out.steps.push(surface(
                id,
                Edit::CopyGeometry {
                    faces: faces(q.faces),
                    edges: edges(q.edges),
                    matrix,
                    count,
                },
            ));
        }
        "paint" => {
            let (q, id) = with_model!(q: MeshPaintParams);
            let material = q
                .material
                .as_deref()
                .map(|m| material_id(doc, m))
                .transpose()?;
            out.steps.push(surface(
                id,
                Edit::SetFaceMaterial {
                    faces: faces(Some(q.faces)),
                    side: match q.side {
                        Some(MeshSide::Back) => PaintSide::Back,
                        Some(MeshSide::Both) => PaintSide::Both,
                        _ => PaintSide::Front,
                    },
                    material,
                },
            ));
        }
        "hide" => {
            let (q, id) = with_model!(q: MeshHideParams);
            if q.faces.is_none() && q.edges.is_none() {
                return Err(bad("give faces or edges"));
            }
            out.steps.push(surface(
                id,
                Edit::SetHidden {
                    faces: faces(q.faces),
                    edges: edges(q.edges),
                    hidden: q.hidden.unwrap_or(true),
                },
            ));
        }
        "soften" => {
            let (q, id) = with_model!(q: MeshSoftenParams);
            if q.soft.is_none() && q.smooth.is_none() {
                return Err(bad("give soft or smooth"));
            }
            out.steps.push(surface(
                id,
                Edit::SetEdgeStyle {
                    edges: edges(Some(q.edges)),
                    soft: q.soft,
                    smooth: q.smooth,
                },
            ));
        }
        "soften_edges" => {
            let (q, id) = with_model!(q: MeshSoftenEdgesParams);
            if !(q.below_angle.is_finite() && (0.0..=180.0).contains(&q.below_angle)) {
                return Err(bad("below_angle: degrees, 0 to 180"));
            }
            out.soften = true;
            out.steps.push(Step::SoftenAll {
                id,
                edges: q.edges.map(|e| edges(Some(e))),
                angle: q.below_angle,
                smooth: q.smooth.unwrap_or(true),
                coplanar: q.coplanar.unwrap_or(false),
            });
        }
        "solid" => {
            let (q, id) = with_model!(q: MeshSolidParams);
            let side = |list: Option<Vec<u32>>, one: Option<u32>, name: &str| match (list, one) {
                (Some(l), None) => Ok(Pick::Faces(faces(Some(l)))),
                (None, Some(f)) => Ok(Pick::Holding(FaceId(f))),
                (None, None) => Err(bad(format!("give {name} or {name}_face"))),
                (Some(_), Some(_)) => Err(bad(format!("give {name} or {name}_face, not both"))),
            };
            out.steps.push(Step::Solid {
                id,
                op: match q.op {
                    MeshSolidOp::Union => SolidOp::Union,
                    MeshSolidOp::Subtract => SolidOp::Subtract,
                    MeshSolidOp::Intersect => SolidOp::Intersect,
                    MeshSolidOp::Trim => SolidOp::Trim,
                    MeshSolidOp::Split => SolidOp::Split,
                },
                first: side(q.first, q.first_face, "first")?,
                second: side(q.second, q.second_face, "second")?,
            });
        }
        "make_group" => {
            let (q, id) = with_model!(q: MeshMakeGroupParams);
            out.objects = true;
            out.steps.push(fixed(Command::MakeGroup {
                model: Some(id),
                faces: faces(q.faces),
                edges: edges(q.edges),
                objects: object_ids(q.objects)?,
                name: q.name,
            }));
        }
        "make_component" => {
            let (q, id) = with_model!(q: MeshMakeComponentParams);
            out.objects = true;
            out.steps.push(fixed(Command::MakeComponent {
                name: q.name,
                model: Some(id),
                faces: faces(q.faces),
                edges: edges(q.edges),
                objects: object_ids(q.objects)?,
                replace: q.replace.unwrap_or(true),
            }));
        }
        "explode" => {
            let q: MeshExplodeParams = parse(p)?;
            out.objects = true;
            out.steps.push(fixed(Command::Explode {
                ids: object_ids(Some(q.ids.clone()))?,
            }));
        }
        other => return Err(RpcError::method_not_found(&format!("mesh.{other}"))),
    }
    Ok(out)
}

fn object_ids(v: Option<Vec<ObjectRef>>) -> Result<Vec<ObjectId>, RpcError> {
    v.unwrap_or_default()
        .iter()
        .map(|r| object_id(&r.0))
        .collect()
}

/// The layer and class of `mesh.create` (already IDs after the check of the parameters).
fn layer_class(
    q: &MeshCreateParams,
) -> Result<
    (
        Option<kadakas_core::model::LayerId>,
        Option<kadakas_core::model::ClassId>,
    ),
    RpcError,
> {
    let layer = q
        .layer
        .as_ref()
        .map(|l| serde_json::from_value(Value::String(l.0.clone())))
        .transpose()
        .map_err(bad)?;
    let class = q
        .class
        .as_ref()
        .map(|c| serde_json::from_value(Value::String(c.0.clone())))
        .transpose()
        .map_err(bad)?;
    Ok((layer, class))
}

/// What a model was before an edit, for the change that the answer reports.
#[derive(Default)]
struct Snap {
    vertices: BTreeSet<u32>,
    edges: BTreeSet<u32>,
    faces: BTreeSet<u32>,
    /// (soft, smooth) of each edge, for Soften Edges by angle.
    flags: BTreeMap<u32, (bool, bool)>,
    considered: usize,
    objects: BTreeSet<ObjectId>,
    resources: BTreeSet<ResourceId>,
}

impl Snap {
    fn of(doc: &Document, item: &Planned) -> Snap {
        let mut s = Snap::default();
        if let Some(m) = item.model.and_then(|id| model_of(doc, id).ok()) {
            s.vertices = m.vertices().map(|(v, _)| v.0).collect();
            s.edges = m.edges().map(|(e, _)| e.0).collect();
            s.faces = m.faces().map(|(f, _)| f.0).collect();
            if item.soften {
                s.flags = flags(m);
            }
        }
        if item.objects {
            s.objects = doc.objects.values().map(|o| o.id).collect();
            s.resources = doc.resources.iter().map(|r| r.id).collect();
        }
        s
    }
}

fn flags(m: &SurfaceModel) -> BTreeMap<u32, (bool, bool)> {
    m.edges()
        .map(|(e, _)| (e.0, (m.edge_soft(e), m.edge_smooth(e))))
        .collect()
}

fn list(ids: impl Iterator<Item = u32>, cut: &mut bool) -> Value {
    let all: Vec<u32> = ids.collect();
    if all.len() > LIST_MAX {
        *cut = true;
    }
    json!(all.into_iter().take(LIST_MAX).collect::<Vec<_>>())
}

/// The change of one edit, as JSON.
fn item_json(doc: &Document, item: &Planned, before: &Snap) -> Value {
    let mut v = json!({"op": item.op});
    let mut cut = false;
    let mut created_objects: Vec<ObjectId> = Vec::new();
    if item.objects {
        created_objects = doc
            .objects
            .values()
            .map(|o| o.id)
            .filter(|id| !before.objects.contains(id))
            .collect();
        created_objects.sort();
        let removed: Vec<ObjectId> = before
            .objects
            .iter()
            .copied()
            .filter(|id| doc.object(*id).is_none())
            .collect();
        v["created_objects"] = json!(created_objects);
        v["removed_objects"] = json!(removed);
        let mut resources: Vec<ResourceId> = doc
            .resources
            .iter()
            .map(|r| r.id)
            .filter(|id| !before.resources.contains(id))
            .collect();
        resources.sort();
        v["created_resources"] = json!(resources);
    }
    let model = item.model.or_else(|| {
        (item.op == "create")
            .then(|| created_objects.first().copied())
            .flatten()
    });
    if item.op == "create" {
        v["id"] = json!(created_objects.first());
    } else if let Some(id) = item.model {
        v["id"] = json!(id);
    }
    if item.op == "make_group" {
        v["group"] = json!(created_objects.iter().find(|id| doc.is_group(**id)));
    }
    let Some(m) = model.and_then(|id| model_of(doc, id).ok()) else {
        if item.model.is_some() {
            v["model_gone"] = json!(true);
        }
        return v;
    };
    v["counts"] =
        json!({"vertices": m.vertex_count(), "edges": m.edge_count(), "faces": m.face_count()});
    v["revision"] = json!(m.rev());
    let (nv, ne, nf): (BTreeSet<u32>, BTreeSet<u32>, BTreeSet<u32>) = (
        m.vertices().map(|(x, _)| x.0).collect(),
        m.edges().map(|(x, _)| x.0).collect(),
        m.faces().map(|(x, _)| x.0).collect(),
    );
    v["created"] = json!({
        "vertices": list(nv.difference(&before.vertices).copied(), &mut cut),
        "edges": list(ne.difference(&before.edges).copied(), &mut cut),
        "faces": list(nf.difference(&before.faces).copied(), &mut cut),
    });
    v["removed"] = json!({
        "vertices": list(before.vertices.difference(&nv).copied(), &mut cut),
        "edges": list(before.edges.difference(&ne).copied(), &mut cut),
        "faces": list(before.faces.difference(&nf).copied(), &mut cut),
    });
    if cut {
        v["truncated"] = json!(true);
    }
    if item.soften {
        let after = flags(m);
        v["changed"] = json!(
            after
                .iter()
                .filter(|(e, f)| before.flags.get(e).is_some_and(|b| b != *f))
                .count()
        );
        v["softened"] = json!(after.values().filter(|(soft, _)| *soft).count());
        v["considered"] = json!(before.considered);
    }
    v
}

/// The typed error of a refused step. The core says why with a typed error, so the edit is not
/// run again.
fn refusal(cmd: &Command, e: &CommandError, step: Option<usize>) -> RpcError {
    let msg = e.to_string();
    let mut facts = json!({});
    if let Some(i) = step {
        facts["step"] = json!(i);
    }
    match cmd {
        Command::EditSurface { id, .. } => facts["object"] = json!(id),
        other => facts["op"] = json!(op_of(other)),
    }
    let with = |mut f: Value, k: &str, v: Value| {
        f[k] = v;
        f
    };
    match e {
        CommandError::Surface(se) => {
            let (kind, error_id, more) = classify(se);
            if let (Value::Object(f), Value::Object(m)) = (&mut facts, more) {
                f.extend(m);
            }
            let text = se.to_string();
            let err = match kind {
                Kind::NotFound => RpcError::not_found(text),
                Kind::Limit => RpcError::limit(text),
                Kind::Validation => RpcError::validation(text),
            };
            typed(err, error_id, facts)
        }
        CommandError::NoObject(o) => typed(
            RpcError::not_found(msg),
            "no_such_object",
            with(facts, "object", json!(o)),
        ),
        CommandError::NoLayer(l) => typed(
            RpcError::not_found(msg),
            "no_such_layer",
            with(facts, "layer", json!(l)),
        ),
        CommandError::NoClass(c) => typed(
            RpcError::not_found(msg),
            "no_such_class",
            with(facts, "class", json!(c)),
        ),
        CommandError::NoResource(r) => typed(
            RpcError::not_found(msg),
            "no_such_resource",
            with(facts, "resource", json!(r)),
        ),
        CommandError::LayerLocked(l) => typed(
            RpcError::conflict(msg),
            "layer_locked",
            with(facts, "layer", json!(l)),
        ),
        CommandError::LayerNotEmpty(_) | CommandError::ResourceInUse(..) => {
            typed(RpcError::conflict(msg), "command_refused", facts)
        }
        // The edit works, but the model is invalid after it.
        CommandError::Invalid(m) if matches!(cmd, Command::EditSurface { .. }) => {
            typed(RpcError::validation(m.clone()), "model_invalid", facts)
        }
        _ => typed(RpcError::validation(msg), "command_refused", facts),
    }
}

/// The surface model of an object that the person could edit now: the layer is not locked and
/// neither the layer nor the class is grey. The refusal is typed before anything runs.
fn editable_model(doc: &Document, id: ObjectId) -> Result<&SurfaceModel, RpcError> {
    let m = model_of(doc, id)?;
    let o = doc.object(id).expect("checked by model_of");
    if doc.layer(o.layer).is_some_and(|l| l.locked) {
        return Err(typed(
            RpcError::conflict(format!("layer {} is locked", o.layer)),
            "layer_locked",
            json!({"object": id, "layer": o.layer}),
        ));
    }
    if doc.drawing.grey.layers.contains(&o.layer) {
        return Err(typed(
            RpcError::validation(format!(
                "layer {} is grey: its objects cannot be edited. Make it visible first",
                o.layer
            )),
            "layer_grey",
            json!({"object": id, "layer": o.layer}),
        ));
    }
    if doc.drawing.grey.classes.contains(&o.class) {
        return Err(typed(
            RpcError::validation(format!(
                "class {} is grey: its objects cannot be edited. Make it visible first",
                o.class
            )),
            "class_grey",
            json!({"object": id, "class": o.class}),
        ));
    }
    Ok(m)
}

fn op_of(cmd: &Command) -> &'static str {
    match cmd {
        Command::MakeGroup { .. } => "make_group",
        Command::MakeComponent { .. } => "make_component",
        Command::Explode { .. } => "explode",
        Command::AddObject { .. } => "create",
        _ => "edit",
    }
}

enum Kind {
    NotFound,
    Limit,
    Validation,
}

/// The stable id, the kind and the facts of a refusal of the core.
fn classify(e: &SurfaceError) -> (Kind, &'static str, Value) {
    use Kind::*;
    match e {
        SurfaceError::BadPoint => (Validation, "bad_point", json!({})),
        SurfaceError::TooShort { tol } => {
            (Validation, "edge_too_short", json!({"tolerance_mm": tol}))
        }
        SurfaceError::TooClose { tol } => {
            (Validation, "edge_too_close", json!({"tolerance_mm": tol}))
        }
        SurfaceError::NoEdge(x) => (NotFound, "no_such_edge", json!({"edge": x})),
        SurfaceError::NoFace(x) => (NotFound, "no_such_face", json!({"face": x})),
        SurfaceError::NoVertex(x) => (NotFound, "no_such_vertex", json!({"vertex": x})),
        SurfaceError::Limit { what, max, count } => (
            Limit,
            "model_limit",
            json!({"what": what, "max": max, "count": count}),
        ),
        SurfaceError::Unsolved(_) => (Validation, "faces_unsolved", json!({})),
        SurfaceError::Refused(_) => (Validation, "edit_refused", json!({})),
        SurfaceError::Curve(c) => {
            let facts = match c {
                CurveError::NoCurve(x)
                | CurveError::NoDescription(x)
                | CurveError::UnknownKind(x) => {
                    json!({"curve": x})
                }
                CurveError::Segments { segments, min, max } => {
                    json!({"segments": segments, "min": min, "max": max})
                }
                CurveError::VertexShared { vertex } => json!({"vertex": vertex}),
                CurveError::BordersOtherPlane { face } | CurveError::LoopUnclear { face } => {
                    json!({"face": face})
                }
                CurveError::Invalid { reason } => json!({"reason": reason}),
            };
            let kind = match c {
                CurveError::NoCurve(_) => NotFound,
                _ => Validation,
            };
            (kind, c.code(), facts)
        }
        SurfaceError::Solid(s) => {
            let (id, facts) = match s {
                SolidError::Empty(w) => ("solid_empty", json!({"operand": w})),
                SolidError::Open {
                    which,
                    count,
                    first,
                    kind,
                } => (
                    "solid_open",
                    json!({"operand": which, "open_edges": count, "edge": first, "open_kind": kind}),
                ),
                SolidError::SelfCrossing { which, edge, face } => (
                    "solid_self_crossing",
                    json!({"operand": which, "edge": edge, "face": face}),
                ),
                SolidError::Inverted(w) => ("solid_inverted", json!({"operand": w})),
                SolidError::Flat(w) => ("solid_flat", json!({"operand": w})),
                SolidError::Attached(w) => ("solid_attached", json!({"operand": w})),
                SolidError::NoResult(op) => ("solid_no_result", json!({"solid_op": op})),
                SolidError::Cut(_) => ("solid_cut", json!({})),
                SolidError::ResultOpen(_) => ("solid_result_open", json!({})),
                SolidError::ResultVolume => ("solid_result_volume", json!({})),
                SolidError::ResultInvalid(_) => ("solid_result_invalid", json!({})),
            };
            (Validation, id, facts)
        }
    }
}

/// Runs the planned edits in one undo step. Returns the change of each edit and the notes.
fn run(
    s: &mut Session,
    items: &[Planned],
    batch: bool,
) -> Result<(Vec<Value>, Vec<String>), RpcError> {
    let mut failure: Option<RpcError> = None;
    let mut results: Vec<Value> = Vec::with_capacity(items.len());
    let mut made = 0usize;
    let outcome = s.editor.execute_stepwise(|step| {
        for (i, item) in items.iter().enumerate() {
            let mut before = Snap::of(step.doc(), item);
            for st in &item.steps {
                let index = batch.then_some(i);
                let cmd = match st.build(step.doc()) {
                    Ok(c) => c,
                    Err(e) => {
                        let message = e.message.clone();
                        failure = Some(with_step(e, index));
                        return Err(CommandError::Invalid(message));
                    }
                };
                if item.soften
                    && let Command::EditSurface {
                        edit:
                            Edit::SoftenEdges {
                                edges,
                                angle,
                                coplanar,
                                ..
                            },
                        id,
                    } = &cmd
                    && let Ok(m) = model_of(step.doc(), *id)
                {
                    before.considered = soften_plan(m, edges, *angle, *coplanar).len();
                }
                if let Err(e) = step.run(&cmd) {
                    failure = Some(refusal(&cmd, &e, index));
                    return Err(e);
                }
            }
            // The work of a batch is bounded by the edges that its edits make, of any kind.
            if batch {
                made += new_edges(step.doc(), item, &before);
                if made > BATCH_EDGES_MAX {
                    failure = Some(too_many_edges(made, i));
                    return Err(CommandError::Invalid("too many edges".into()));
                }
            }
            results.push(item_json(step.doc(), item, &before));
        }
        Ok(())
    });
    match (outcome, failure) {
        (Ok(out), _) => {
            s.prune_selection();
            Ok((results, out.notes))
        }
        (Err(_), Some(e)) => Err(e),
        (Err(e), None) => Err(RpcError::from(e)),
    }
}

/// How many edges the item made in its model.
fn new_edges(doc: &Document, item: &Planned, before: &Snap) -> usize {
    item.model
        .and_then(|id| model_of(doc, id).ok())
        .map_or(0, |m| {
            m.edges()
                .filter(|(e, _)| !before.edges.contains(&e.0))
                .count()
        })
}

fn too_many_edges_drawn(count: usize) -> RpcError {
    typed(
        RpcError::limit(format!(
            "the add_edges edits of a batch draw at most {BATCH_EDGES_MAX} edges in all and this batch draws {count}"
        )),
        "batch_edges_too_many",
        json!({"max": BATCH_EDGES_MAX, "count": count}),
    )
}

fn too_many_edges(count: usize, step: usize) -> RpcError {
    typed(
        RpcError::limit(format!(
            "the edits of a batch make at most {BATCH_EDGES_MAX} edges in all and the edits up to edit {step} made {count}"
        )),
        "batch_edges_too_many",
        json!({"max": BATCH_EDGES_MAX, "count": count, "step": step}),
    )
}

/// Adds the index of the edit in a batch to an error.
fn with_step(mut e: RpcError, index: Option<usize>) -> RpcError {
    if let Some(i) = index {
        match &mut e.details {
            Some(Value::Object(d)) => {
                d.insert("step".into(), json!(i));
            }
            _ => e.details = Some(json!({"step": i})),
        }
    }
    e
}

/// A single edit method.
pub(super) fn single(s: &mut Session, op: &str, p: Value) -> Result<Value, RpcError> {
    let item = plan(s, op, p)?;
    let (mut results, notes) = run(s, std::slice::from_ref(&item), false)?;
    let mut v = results.pop().unwrap_or_else(|| json!({}));
    if !notes.is_empty() {
        v["notes"] = json!(notes);
    }
    Ok(v)
}

/// `mesh.batch`.
pub(super) fn batch(s: &mut Session, p: Value) -> Result<Value, RpcError> {
    let q: MeshBatchParams = parse(p)?;
    if q.edits.is_empty() {
        return Err(bad("edits: at least one edit"));
    }
    if q.edits.len() > BATCH_EDITS_MAX {
        return Err(typed(
            RpcError::limit(format!(
                "a batch has at most {BATCH_EDITS_MAX} edits and this one has {}",
                q.edits.len()
            )),
            "batch_too_large",
            json!({"max": BATCH_EDITS_MAX, "count": q.edits.len()}),
        ));
    }
    let mut planned: Vec<Planned> = Vec::with_capacity(q.edits.len());
    let mut new_edges = 0usize;
    for (i, mut item) in q.edits.into_iter().enumerate() {
        let at = |e: RpcError| {
            let mut e = e;
            e.message = format!("edits[{i}]: {}", e.message);
            with_step(e, Some(i))
        };
        let op = match item.remove("method") {
            Some(Value::String(m)) => m.strip_prefix("mesh.").unwrap_or(&m).to_string(),
            _ => {
                return Err(at(bad(
                    "give method, the name of an edit such as push_pull",
                )));
            }
        };
        if !BATCH_OPS.contains(&op.as_str()) {
            return Err(at(bad(format!(
                "method '{op}' is not an edit of a batch. Edits: {}",
                BATCH_OPS.join(", ")
            ))));
        }
        if op != "explode"
            && !item.contains_key("id")
            && let Some(id) = &q.id
        {
            item.insert("id".into(), json!(id.0));
        }
        let checked = params::prepare(s.editor.doc(), &format!("mesh.{op}"), Value::Object(item))
            .map_err(at)?;
        if op == "add_edges" {
            new_edges += checked["edges"].as_array().map_or(0, Vec::len);
        }
        planned.push(plan(s, &op, checked).map_err(at)?);
    }
    if new_edges > BATCH_EDGES_MAX {
        return Err(too_many_edges_drawn(new_edges));
    }
    let (results, notes) = run(s, &planned, true)?;
    let mut v = json!({"count": results.len(), "edits": results});
    if !notes.is_empty() {
        v["notes"] = json!(notes);
    }
    Ok(v)
}
