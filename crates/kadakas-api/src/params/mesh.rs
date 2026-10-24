//! Parameters of the methods of the domain `mesh` (MOD-11).
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::sheets::{RenderPngMode, RenderPngProjection};
use super::*;

param_enum! {
    MeshKind {
        Faces = "faces" | "face",
        Edges = "edges" | "edge",
        Vertices = "vertices" | "vertex",
    }
}

param_enum! {
    MeshScope {
        Model = "model",
        Selection = "selection",
    }
}

param_enum! {
    MeshSide {
        Front = "front",
        Back = "back",
        Both = "both",
    }
}

param_enum! {
    MeshSolidOp {
        Union = "union",
        Subtract = "subtract",
        Intersect = "intersect",
        Trim = "trim",
        Split = "split",
    }
}

/// A turn about an axis.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshRotate {
    /// the direction of the axis, [x, y, z], not zero
    pub axis: [f64; 3],
    /// degrees, counterclockwise seen from the tip of the axis
    pub degrees: f64,
    /// a point on the axis (default the origin of the model)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<[f64; 3]>,
}

/// A scale about a point.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshScale {
    /// one factor for all three axes, not zero
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<f64>,
    /// one factor for each axis [x, y, z], none of them zero
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factors: Option<[f64; 3]>,
    /// the point that stays (default the origin of the model)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<[f64; 3]>,
}

// Parameters of `mesh.summary`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshSummaryParams {
    /// one surface model; without it all surface models of the document
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
}

// Parameters of `mesh.vertices`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshVerticesParams {
    pub id: ObjectRef,
    /// only these vertex IDs (in the order given)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u32>>,
    /// how many to skip, default 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// at most this many, default 100, at most 1000
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

// Parameters of `mesh.edges`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshEdgesParams {
    pub id: ObjectRef,
    /// only these edge IDs (in the order given)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u32>>,
    /// how many to skip, default 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// at most this many, default 100, at most 1000
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

// Parameters of `mesh.faces`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshFacesParams {
    pub id: ObjectRef,
    /// only these face IDs (in the order given)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u32>>,
    /// how many to skip, default 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// at most this many, default 100, at most 1000
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// true: add the coordinates of the loops (local mm), default false
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with_points: Option<bool>,
}

// Parameters of `mesh.solid_report`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshSolidReportParams {
    pub id: ObjectRef,
}

// Parameters of `mesh.bounds`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshBoundsParams {
    pub id: ObjectRef,
}

// Parameters of `mesh.near`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshNearParams {
    /// one surface model; without it all surface models of the document
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    /// [x, y, z] in drawing coordinates (mm)
    pub point: [f64; 3],
    /// mm: the farthest distance from the point
    pub radius: f64,
    /// what to look for: faces, edges, vertices (default faces)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<MeshKind>>,
    /// at most this many of each kind, nearest first, default 20, at most 200
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// true: also look at hidden faces and edges, default false
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_hidden: Option<bool>,
}

// Parameters of `mesh.pick`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshPickParams {
    /// one surface model; without it all surface models of the document
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    /// the start of the ray, [x, y, z] in drawing coordinates (mm)
    pub origin: [f64; 3],
    /// the direction of the ray, [x, y, z], not zero
    pub direction: [f64; 3],
    /// true: give every face the ray meets, nearest first (at most 200), default false (the first)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    /// true: also hit hidden faces, default false
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_hidden: Option<bool>,
}

// Parameters of `mesh.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshCreateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// where the origin of the model is, [x, y, z] in drawing coordinates (mm), default [0, 0, 0]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `mesh.add_edges`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshAddEdgesParams {
    /// the surface model
    pub id: ObjectRef,
    /// straight edges, each [[x, y, z], [x, y, z]] in local mm; at most 10000
    pub edges: Vec<[[f64; 3]; 2]>,
}

// Parameters of `mesh.add_curve`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshAddCurveParams {
    pub id: ObjectRef,
    /// the points of the chain, [x, y, z] in local mm
    pub points: Vec<[f64; 3]>,
    /// true: the last point joins the first (a circle), default false
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// true: a polygon, the edges between its sides stay creases, default false
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faceted: Option<bool>,
}

// Parameters of `mesh.erase`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshEraseParams {
    pub id: ObjectRef,
    /// edges to erase: coplanar faces on their sides merge, other faces of the edges go
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    /// faces to erase; their edges stay
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<u32>>,
}

// Parameters of `mesh.reverse_faces`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshReverseFacesParams {
    pub id: ObjectRef,
    pub faces: Vec<u32>,
}

// Parameters of `mesh.push_pull`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshPushPullParams {
    pub id: ObjectRef,
    pub face: u32,
    /// mm along the front normal: positive toward the front, negative toward the back
    pub distance: f64,
}

// Parameters of `mesh.follow_me`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshFollowMeParams {
    pub id: ObjectRef,
    /// the profile face
    pub face: u32,
    /// the chain of edges to follow
    pub path: Vec<u32>,
}

// Parameters of `mesh.offset`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshOffsetParams {
    pub id: ObjectRef,
    /// offset all loops of this face (give face or edges)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<u32>,
    /// offset this chain of edges in one plane (give face or edges)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    /// mm, more than the length tolerance
    pub distance: f64,
    /// face: true draws outside the face, default false (inside)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outward: Option<bool>,
    /// edges: the side to draw on, a point [x, y, z] in local mm (required with edges)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toward: Option<[f64; 3]>,
}

// Parameters of `mesh.intersect_faces`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshIntersectFacesParams {
    pub id: ObjectRef,
    /// model (all faces and edges, default) or selection (the given faces and edges against the model)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MeshScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
}

// Parameters of `mesh.transform`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshTransformParams {
    pub id: ObjectRef,
    /// vertices to move
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertices: Option<Vec<u32>>,
    /// faces to move (all their vertices)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<u32>>,
    /// edges to move (their ends)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    /// 12 numbers: the three columns of the 3x3 part, then the translation (local mm). Instead of translate, rotate and scale
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<[f64; 12]>,
    /// [x, y, z] in local mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<MeshRotate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<MeshScale>,
}

// Parameters of `mesh.copy`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshCopyParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    /// as in mesh.transform
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<[f64; 12]>,
    /// [x, y, z] in local mm: the step of the copies
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<MeshRotate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<MeshScale>,
    /// how many copies, default 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

// Parameters of `mesh.paint`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshPaintParams {
    pub id: ObjectRef,
    pub faces: Vec<u32>,
    /// front (default), back or both
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<MeshSide>,
    /// a material of the document (ID or name); without it the paint is cleared
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
}

// Parameters of `mesh.hide`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshHideParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    /// true (default) hides, false shows again
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

// Parameters of `mesh.soften`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshSoftenParams {
    pub id: ObjectRef,
    pub edges: Vec<u32>,
    /// true makes the edges soft (not drawn), false makes them hard; not given: stays as it is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft: Option<bool>,
    /// true makes the faces on the sides shade as one surface; not given: stays as it is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smooth: Option<bool>,
}

// Parameters of `mesh.soften_edges`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshSoftenEdgesParams {
    pub id: ObjectRef,
    /// degrees, 0 to 180: edges between two faces at an angle below it become soft, the others hard
    pub below_angle: f64,
    /// only these edges; default all edges of the model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    /// true (default): soft edges are smooth too
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smooth: Option<bool>,
    /// true: edges between coplanar faces count too, default false
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coplanar: Option<bool>,
}

// Parameters of `mesh.to_surface`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshToSurfaceParams {
    /// a mesh object (shape kind mesh3d), for example from import.file
    pub id: ObjectRef,
    /// mm, 0.001 to 1000: vertices closer than this weld. Default 0.01
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
    /// degrees, 0 to 180, default off: edges between faces at an angle below it become soft and smooth
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soften_below_angle: Option<f64>,
    /// true (default): coplanar triangles merge into faces with holes. false: every triangle is a face
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<bool>,
    /// true: convert nothing, answer with the report (the counts and the edges that would soften)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `mesh.solid`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshSolidParams {
    pub id: ObjectRef,
    /// union, subtract, intersect, trim or split
    pub op: MeshSolidOp,
    /// the faces of the first solid; or give first_face
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<Vec<u32>>,
    /// the faces of the second solid; or give second_face
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<Vec<u32>>,
    /// one face of the first solid: the solid that holds it is used
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_face: Option<u32>,
    /// one face of the second solid: the solid that holds it is used
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_face: Option<u32>,
}

// Parameters of `mesh.make_group`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshMakeGroupParams {
    /// the surface model that holds the faces and edges
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    /// other objects of the same layer and group that join the group
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `mesh.make_component`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshMakeComponentParams {
    /// the surface model that holds the faces and edges
    pub id: ObjectRef,
    /// the name of the component definition
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<ObjectRef>>,
    /// true (default): one instance takes the place of the geometry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
}

// Parameters of `mesh.explode`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshExplodeParams {
    /// groups and component instances
    pub ids: Vec<ObjectRef>,
}

// Parameters of `mesh.batch`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshBatchParams {
    /// the surface model of the edits that give no id of their own
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    /// 1 to 50 edits, run in this order. Each is {method, ...params of mesh.<method>} where method
    /// is one of add_edges, add_curve, erase, reverse_faces, push_pull, follow_me, offset,
    /// intersect_faces, transform, copy, paint, hide, soften, soften_edges, solid, make_group,
    /// make_component, explode. All succeed or none does
    pub edits: Vec<Obj>,
}

// Parameters of `mesh.render`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshRenderParams {
    /// frame the camera on this surface model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    /// a name from view.list; default the current view
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// {target [x, y, z], yaw, pitch (radians), distance (mm), fov (degrees), projection}; wins over view
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<super::layers::SavedViewCamera>,
    /// pixels, 16 to 2048, default 800
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Int>,
    /// pixels, 16 to 2048, default 600
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<RenderPngProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RenderPngMode>,
    /// a render style of the document (ID or name) or a built-in one such as Realistic
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_style: Option<ResourceRef>,
    /// 1 to 3 (anti-aliasing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<Int>,
}

// Parameters of `mesh3d.exact_info`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mesh3dExactInfoParams {
    /// a mesh object (shape kind mesh3d) from a STEP import
    pub id: ObjectRef,
    /// most faces listed, 1 to 5000, default 200
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
}

// Parameters of `mesh3d.retessellate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mesh3dRetessellateParams {
    /// a mesh object (shape kind mesh3d) from a STEP import
    pub id: ObjectRef,
    /// mm, 0.001 to 1000: the largest distance of a triangle from its surface
    pub tolerance: f64,
    /// degrees, 1 to 90, default 15: the largest turn of the normal between neighbours
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
}

entries! {
    "mesh3d.exact_info" => Mesh3dExactInfoParams,
    "mesh3d.retessellate" => Mesh3dRetessellateParams,
    "mesh.summary" => MeshSummaryParams,
    "mesh.vertices" => MeshVerticesParams,
    "mesh.edges" => MeshEdgesParams,
    "mesh.faces" => MeshFacesParams,
    "mesh.solid_report" => MeshSolidReportParams,
    "mesh.bounds" => MeshBoundsParams,
    "mesh.near" => MeshNearParams,
    "mesh.pick" => MeshPickParams,
    "mesh.create" => MeshCreateParams,
    "mesh.add_edges" => MeshAddEdgesParams,
    "mesh.add_curve" => MeshAddCurveParams,
    "mesh.erase" => MeshEraseParams,
    "mesh.reverse_faces" => MeshReverseFacesParams,
    "mesh.push_pull" => MeshPushPullParams,
    "mesh.follow_me" => MeshFollowMeParams,
    "mesh.offset" => MeshOffsetParams,
    "mesh.intersect_faces" => MeshIntersectFacesParams,
    "mesh.transform" => MeshTransformParams,
    "mesh.copy" => MeshCopyParams,
    "mesh.paint" => MeshPaintParams,
    "mesh.hide" => MeshHideParams,
    "mesh.soften" => MeshSoftenParams,
    "mesh.soften_edges" => MeshSoftenEdgesParams,
    "mesh.to_surface" => MeshToSurfaceParams,
    "mesh.solid" => MeshSolidParams,
    "mesh.make_group" => MeshMakeGroupParams,
    "mesh.make_component" => MeshMakeComponentParams,
    "mesh.explode" => MeshExplodeParams,
    "mesh.batch" => MeshBatchParams,
    "mesh.render" => MeshRenderParams,
}
