//! Agent API for surface models and the modeler (MOD-11).
//!
//! Methods: `mesh.summary|vertices|edges|faces|solid_report|bounds|near|pick` read a surface model.
//! `mesh.create` makes one. The edits (`mesh.add_edges`, `mesh.push_pull`, `mesh.solid` and the
//! others) are thin wrappers over the command `edit_surface` and the group and component commands
//! of the core. `mesh.batch` runs many edits as one undo step. `mesh.render` shows the result.
//!
//! The geometry is the code of `kadakas_core::surface`. This module checks the call, builds the
//! commands, runs them in one undo step and describes what changed. A refusal gets a stable id.
//! An agent edit is marked as an agent change by the session (see `agent_activity_api`), like
//! every other method that changes the document.

use serde_json::{Value, json};

use kadakas_core::command::Command;
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::sheets::ViewCamera;
use kadakas_core::surface::SurfaceModel;
use kadakas_render::Camera;
use kadakas_render::Projection;

use crate::errors::bad;
use crate::params::mesh::*;
use crate::{RpcError, Session};

mod edit;
mod query;
mod read;

pub const MESH_SUMMARY: (&str, &str) = (
    "mesh.summary",
    "The surface models of the document. params: {id?} (one model, else all). Returns {models: [{id, name, layer, class, parent?, vertices, edges, faces, curves, revision, problem (null or the first problem of the model), hidden_faces, hidden_edges, soft_edges, transform (12 numbers), bounds: {local, world} ({min, max} in mm or null for an empty model)}]}. Coordinates in the other mesh methods are local to the model. Drawing coordinates are the local ones moved by the transform.",
);
pub const MESH_VERTICES: (&str, &str) = (
    "mesh.vertices",
    "Vertices of a surface model, in id order. params: {id, ids? (only these), offset? (default 0), limit? (default 100, at most 1000)}. Returns {total, offset, items: [{id, p: [x, y, z] (local mm)}], missing? (ids that do not exist)}.",
);
pub const MESH_EDGES: (&str, &str) = (
    "mesh.edges",
    "Edges of a surface model, in id order. params: {id, ids?, offset?, limit?} as in mesh.vertices. Returns {total, offset, items: [{id, v: [vertex, vertex], faces: [face ids], soft, smooth, hidden, curve (id or null), length_mm}], missing?}.",
);
pub const MESH_FACES: (&str, &str) = (
    "mesh.faces",
    "Faces of a surface model, in id order. params: {id, ids?, offset?, limit?, with_points? (add the coordinates of the loops)}. Returns {total, offset, items: [{id, outer: [vertex ids], holes: [[vertex ids]], normal: [x, y, z] (points to the front), area_mm2, hidden, material (front, or null), back_material, points?: {outer: [[x, y, z]], holes}}], missing?}.",
);
pub const MESH_SOLID_REPORT: (&str, &str) = (
    "mesh.solid_report",
    "Is the model made of closed solids? params: {id}. Returns {id, closed (true when it has faces, every shell is a solid and no edge is loose), shells: [{faces, state (solid | open | self_crossing | inverted | flat), volume_mm3, area_mm2, open_edges: [{edge, kind (border | non_manifold | reversed)}], ...}], wire_edges, crossings}. The open edges say what to fix before a solid operation.",
);
pub const MESH_BOUNDS: (&str, &str) = (
    "mesh.bounds",
    "The box around a surface model. params: {id}. Returns {id, empty, local: {min, max, size} | null, world: {min, max, size} | null} in mm.",
);
pub const MESH_NEAR: (&str, &str) = (
    "mesh.near",
    "Faces, edges or vertices near a point. params: {id?, point: [x, y, z] (drawing coordinates, mm), radius (mm), kinds? (faces | edges | vertices, default [faces]), limit? (per kind, default 20, at most 200), include_hidden?}. Returns {point, radius, faces?: [{model, id, distance, closest: [x, y, z] (drawing coordinates)}], edges?, vertices?}, nearest first.",
);
pub const MESH_PICK: (&str, &str) = (
    "mesh.pick",
    "Shoots a ray at the surface models. params: {id?, origin: [x, y, z], direction: [x, y, z] (drawing coordinates), all? (every face the ray meets, nearest first, at most 200), include_hidden?}. Returns {hit: null | {model, face, distance, point: [x, y, z] (drawing coordinates), local: [x, y, z], side (front | back: the side of the face that the ray meets), normal: [x, y, z] (drawing coordinates)}, hits? (with all)}.",
);
pub const MESH_CREATE: (&str, &str) = (
    "mesh.create",
    "Make an empty surface model in one undo step. params: {name?, origin? ([x, y, z] in drawing coordinates, where the local origin is, default [0, 0, 0]), layer?, class?}. Returns {id}. The model is drawn with the edits below.",
);
pub const MESH_ADD_EDGES: (&str, &str) = (
    "mesh.add_edges",
    "Draw straight edges. params: {id, edges: [[[x, y, z], [x, y, z]]]} (local mm, at most 10000). Ends weld to vertices and edges, crossings split, closed coplanar edges make faces, and a new edge splits a face. Returns the change (see describe.mesh.results).",
);
pub const MESH_ADD_CURVE: (&str, &str) = (
    "mesh.add_curve",
    "Draw a chain of edges that stays one curve (a circle or an arc) when other edges cut it. params: {id, points: [[x, y, z]], closed? (the last point joins the first), faceted? (a polygon: its corners stay creases)}. Returns the change.",
);
pub const MESH_ERASE: (&str, &str) = (
    "mesh.erase",
    "Erase edges and faces in one undo step. params: {id, edges?, faces?} (at least one). Erased edges merge coplanar faces on their sides and remove other faces of the edge. Erased faces leave their edges. Returns the change.",
);
pub const MESH_REVERSE_FACES: (&str, &str) = (
    "mesh.reverse_faces",
    "Turn faces over: front and back change places. params: {id, faces}. Returns the change.",
);
pub const MESH_PUSH_PULL: (&str, &str) = (
    "mesh.push_pull",
    "Push or pull a planar face along its normal. params: {id, face, distance (mm, positive toward the front)}. A lone face becomes a solid, a face of a prism moves, and a push can make a hole. Returns the change. The face keeps its id and ends as the cap.",
);
pub const MESH_FOLLOW_ME: (&str, &str) = (
    "mesh.follow_me",
    "Sweep a face along a chain of edges. params: {id, face, path: [edge ids]}. An open path gives a solid, a closed path a ring. Returns the change.",
);
pub const MESH_OFFSET: (&str, &str) = (
    "mesh.offset",
    "Offset the loops of a face, or a chain of edges. params: {id, face?, edges?, distance (mm), outward? (face), toward? ([x, y, z] local, required with edges)}. Give face or edges. Returns the change.",
);
pub const MESH_INTERSECT_FACES: (&str, &str) = (
    "mesh.intersect_faces",
    "Make edges where faces in different planes cross, and split both faces. params: {id, scope? (model | selection, default model), faces?, edges?}. With selection, only the given faces and edges are tested against the model. Returns the change.",
);
pub const MESH_TRANSFORM: (&str, &str) = (
    "mesh.transform",
    "Move, rotate or scale vertices, faces and edges. params: {id, vertices?, faces?, edges?, then matrix? (12 numbers) or any of translate? ([x, y, z]), rotate? ({axis, degrees, about?}), scale? ({factor | factors, about?})}. The order is scale, then rotate, then translate. A face that is no longer flat is folded into triangles with soft edges. Returns the change.",
);
pub const MESH_COPY: (&str, &str) = (
    "mesh.copy",
    "Copy faces and edges inside the model. params: {id, faces?, edges?, count? (default 1), and the map as in mesh.transform}. The first copy is placed by the map, the next by the map applied twice, and so on. Returns the change.",
);
pub const MESH_PAINT: (&str, &str) = (
    "mesh.paint",
    "Put a material on faces. params: {id, faces, side? (front | back | both, default front), material? (ID or name of a material of the document, without it the paint is cleared)}. Returns the change.",
);
pub const MESH_HIDE: (&str, &str) = (
    "mesh.hide",
    "Hide or show faces and edges. params: {id, faces?, edges?, hidden? (default true)}. Returns the change.",
);
pub const MESH_SOFTEN: (&str, &str) = (
    "mesh.soften",
    "Set the flags of edges. params: {id, edges, soft?, smooth?} (at least one flag). A soft edge is not drawn or picked. The faces on the sides of a smooth edge shade as one surface. A flag that is not given stays as it is. Returns the change.",
);
pub const MESH_SOFTEN_EDGES: (&str, &str) = (
    "mesh.soften_edges",
    "Soften every edge under an angle. params: {id, below_angle (degrees, 0 to 180), edges? (default all edges of the model), smooth? (default true), coplanar? (also edges between faces in one plane, default false)}. An edge between two faces at an angle below the limit becomes soft. The other edges of the set become hard. Edges without exactly two faces are not touched. Returns the change and {changed (how many edges changed their flags), softened (edges that are soft now), considered (edges that were looked at)}.",
);
pub const MESH_SOLID: (&str, &str) = (
    "mesh.solid",
    "A boolean operation of two closed solids of the model. params: {id, op (union | subtract | intersect | trim | split), first?, second? (the faces of each solid) or first_face?, second_face? (one face of each solid: the solid that holds it)}. Subtract and trim take the second from the first. Trim keeps the second, cut along the first. Split makes three solids. A solid that is not closed is refused, with the edge to fix (see mesh.solid_report). Returns the change.",
);
pub const MESH_MAKE_GROUP: (&str, &str) = (
    "mesh.make_group",
    "Make a group of faces and edges (and other objects). params: {id (the surface model), faces?, edges?, objects?, name?}. The geometry moves into a surface model of its own, so it no longer welds to the rest. Returns the change and {group, created_objects}.",
);
pub const MESH_MAKE_COMPONENT: (&str, &str) = (
    "mesh.make_component",
    "Make a component definition of faces and edges (and other objects). params: {id, name, faces?, edges?, objects?, replace? (default true: one instance takes their place)}. Returns the change and {created_objects, created_resources}.",
);
pub const MESH_EXPLODE: (&str, &str) = (
    "mesh.explode",
    "Explode groups and component instances into the surface model of their level. params: {ids}. The geometry welds to its neighbours and faces form again. Returns {created_objects, removed_objects, notes}.",
);
pub const MESH_BATCH: (&str, &str) = (
    "mesh.batch",
    "Run many edits as one undo step. All succeed or none does. params: {id? (the model of the edits that give none), edits: [{method, ...params of mesh.<method>}]} with 1 to 50 edits and at most 10000 edges in all add_edges edits. method is one of add_edges, add_curve, erase, reverse_faces, push_pull, follow_me, offset, intersect_faces, transform, copy, paint, hide, soften, soften_edges, solid, make_group, make_component, explode. Every edit is checked before the first one runs. An edit that fails undoes all, and the error names its index. Ids that an earlier edit makes are not known before the batch runs: read them with a first call, or use another call. Returns {count, edits: [the change of each edit], notes}.",
);
pub const MESH_RENDER: (&str, &str) = (
    "mesh.render",
    "A PNG of the model so that you see the result. params: {id? (frame the camera on this surface model), view?, camera? ({target, yaw, pitch, distance, fov, projection}, wins over view), width?, height?, projection?, mode? (shaded | wireframe | hidden_line | dashed_hidden_line), render_style? (a style of the document or a built-in one such as Realistic), samples?}. It is render.png with the camera and the framing of a model. Returns {png_base64, width, height, view, projection, mode, camera, render_style?}. MCP gives the image itself.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "model": "A surface model (shape kind 'surface_model') is made of vertices, edges and faces. Coordinates are local, in mm, and the transform of the model places them in the drawing. Vertices, edges and faces have whole-number ids that come from one counter of the model and are never reused. Faces are planar and may have holes. Closed coplanar edges make a face, a new edge splits a face, and geometry of one model sticks together. A group or a component keeps its geometry apart.",
        "workflow": "Make a model with mesh.create. Draw edges with mesh.add_edges and read the ids of the new faces from the answer. Pull a face with mesh.push_pull. Check with mesh.solid_report. Look at it with mesh.render. Use mesh.batch for a long run of edits that must succeed together.",
        "results": "An edit answers {op, id, counts: {vertices, edges, faces}, revision, created: {vertices, edges, faces}, removed: {vertices, edges, faces}, truncated?, notes?}. The lists in created and removed have at most 1000 ids each. Notes tell what the edit changed that you did not ask for, such as a material that a merged face dropped.",
        "undo": "Every edit method is one undo step, also mesh.erase with edges and faces, and mesh.batch with all its edits. A refused edit changes nothing and makes no step. The person sees the steps of an agent in the activity panel and can revert them.",
        "errors": "A refusal is a validation error (code 3, kind validation). A missing object, vertex, edge or face is not_found (2). A size limit is limit (6). The data of the error has error_id, a stable name, and the ids that matter (object, edge, face, vertex, operand, step). The message is English text for a person. Ids: no_such_object, not_a_surface_model, no_such_vertex, no_such_edge, no_such_face, bad_point, edge_too_short, edge_too_close, model_limit, faces_unsolved, edit_refused, model_invalid, solid_empty, solid_open, solid_self_crossing, solid_inverted, solid_flat, solid_attached, solid_no_result, solid_cut, solid_result_open, solid_result_volume, solid_result_invalid, batch_too_large, batch_edges_too_many, command_refused. The data of an edit in a batch has step, the index of the edit.",
        "limits": {
            "batch_edits": edit::BATCH_EDITS_MAX,
            "batch_edges": edit::BATCH_EDGES_MAX,
            "edges_per_edit": 10000,
            "page": 1000,
            "why": "docs/modeler-limits.md: one command on a model of 80,000 edges costs about 0.125 s, and an edit of 10,000 edges costs 0.2 to 0.75 s. 50 edits and 10,000 new edges in all keep a batch near 7 s in the worst case."
        },
        "agent_changes": "Edits are marked as agent changes. They show in agent.activity and in the Activity panel of the person, who can undo or revert them.",
        "examples": [
            {"method": "mesh.create", "params": {"name": "Shed", "origin": [0, 0, 0]}},
            {"method": "mesh.summary", "params": {}},
            {"method": "mesh.add_edges", "params": {"id": "00000000-0000-0000-0000-000000000002", "edges": [[[0, 0, 0], [4000, 0, 0]], [[4000, 0, 0], [4000, 3000, 0]], [[4000, 3000, 0], [0, 3000, 0]], [[0, 3000, 0], [0, 0, 0]]]}},
            {"method": "mesh.push_pull", "params": {"id": "00000000-0000-0000-0000-000000000002", "face": 1, "distance": 2500}},
            {"method": "mesh.faces", "params": {"id": "00000000-0000-0000-0000-000000000002", "limit": 10, "with_points": true}},
            {"method": "mesh.solid_report", "params": {"id": "00000000-0000-0000-0000-000000000002"}},
            {"method": "mesh.transform", "params": {"id": "00000000-0000-0000-0000-000000000002", "faces": [1], "rotate": {"axis": [0, 0, 1], "degrees": 15, "about": [0, 0, 0]}, "translate": [100, 0, 0]}},
            {"method": "mesh.soften_edges", "params": {"id": "00000000-0000-0000-0000-000000000002", "below_angle": 20}},
            {"method": "mesh.solid", "params": {"id": "00000000-0000-0000-0000-000000000002", "op": "subtract", "first_face": 1, "second_face": 9}},
            {"method": "mesh.near", "params": {"point": [100, 100, 0], "radius": 500, "kinds": ["faces", "edges"]}},
            {"method": "mesh.pick", "params": {"origin": [2000, 1500, 9000], "direction": [0, 0, -1]}},
            {"method": "mesh.batch", "params": {"id": "00000000-0000-0000-0000-000000000002", "edits": [
                {"method": "add_edges", "edges": [[[0, 0, 3000], [4000, 0, 3000]]]},
                {"method": "paint", "faces": [1], "material": "Brick"},
                {"method": "hide", "edges": [1], "hidden": true}
            ]}},
            {"method": "mesh.render", "params": {"id": "00000000-0000-0000-0000-000000000002", "camera": {"target": [2000, 1500, 1000], "yaw": 0.8, "pitch": 0.5, "distance": 12000, "fov": 45, "projection": "perspective"}, "width": 800, "height": 600}}
        ]
    })
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    let op = method.strip_prefix("mesh.")?;
    let p = if p.is_null() { json!({}) } else { p };
    Some(match op {
        "summary" | "vertices" | "edges" | "faces" | "solid_report" | "bounds" => {
            read::handle(s.editor.doc(), op, p)
        }
        "near" | "pick" => query::handle(s.editor.doc(), op, p),
        "render" => render(s, p),
        "batch" => edit::batch(s, p),
        _ if edit::is_edit(op) => edit::single(s, op, p),
        _ => Err(RpcError::method_not_found(method)),
    })
}

/// The id of an object from the text of a parameter.
pub(crate) fn object_id(text: &str) -> Result<ObjectId, RpcError> {
    serde_json::from_value(Value::String(text.to_string()))
        .map_err(|_| bad(format!("'{text}' is not an object id")))
}

/// Makes an error typed: a stable `error_id` in the data, with the facts given.
pub(crate) fn typed(e: RpcError, error_id: &'static str, facts: Value) -> RpcError {
    let mut d = match facts {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    d.insert("error_id".into(), json!(error_id));
    e.with_reason(error_id).with_details(Value::Object(d))
}

/// The surface model of an object, or the typed error.
pub(crate) fn model_of(doc: &Document, id: ObjectId) -> Result<&SurfaceModel, RpcError> {
    match doc.object(id).map(|o| &o.shape) {
        Some(Shape::SurfaceModel(m)) => Ok(m),
        Some(other) => Err(typed(
            RpcError::validation(format!(
                "object {id} is not a surface model (it is a {})",
                other.kind()
            )),
            "not_a_surface_model",
            json!({"object": id, "kind": other.kind()}),
        )),
        None => Err(typed(
            RpcError::not_found(format!("object {id} does not exist")),
            "no_such_object",
            json!({"object": id}),
        )),
    }
}

/// The ids and surface models of the document, in the order of the ids. With `only`, that one.
pub(crate) fn models<'a>(
    doc: &'a Document,
    only: Option<&str>,
) -> Result<Vec<(ObjectId, &'a SurfaceModel)>, RpcError> {
    if let Some(text) = only {
        let id = object_id(text)?;
        return Ok(vec![(id, model_of(doc, id)?)]);
    }
    let mut out: Vec<(ObjectId, &SurfaceModel)> = doc
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::SurfaceModel(m) => Some((o.id, m.as_ref())),
            _ => None,
        })
        .collect();
    out.sort_by_key(|(id, _)| *id);
    Ok(out)
}

/// The orbit camera of a saved camera. The camera is checked first.
pub(crate) fn camera_of(c: &ViewCamera) -> Result<Camera, RpcError> {
    if !c.is_valid() {
        return Err(bad(
            "camera: finite numbers, distance above 0, fov 1 to 170 degrees, projection perspective or ortho",
        ));
    }
    Ok(Camera {
        target: glam::Vec3::new(c.target[0] as f32, c.target[1] as f32, c.target[2] as f32),
        yaw: c.yaw as f32,
        pitch: c.pitch as f32,
        distance: c.distance as f32,
        projection: Projection::from_name(&c.projection).unwrap_or(Projection::Perspective),
        fov_y: c.fov.to_radians() as f32,
    })
}

/// A camera as JSON, in the form that `camera` takes.
pub(crate) fn camera_json(c: &Camera) -> Value {
    json!({
        "target": [c.target.x, c.target.y, c.target.z],
        "yaw": c.yaw,
        "pitch": c.pitch,
        "distance": c.distance,
        "fov": f64::from(c.fov_y).to_degrees(),
        "projection": c.projection.id(),
    })
}

/// `mesh.render`: render.png with a model to frame.
fn render(s: &mut Session, p: Value) -> Result<Value, RpcError> {
    let q: MeshRenderParams = serde_json::from_value(p).map_err(bad)?;
    let mut png = json!({});
    for (k, v) in [
        ("view", q.view.map(Value::from)),
        ("width", q.width.map(|n| json!(n.0))),
        ("height", q.height.map(|n| json!(n.0))),
        ("samples", q.samples.map(|n| json!(n.0))),
        (
            "projection",
            q.projection
                .map(|v| serde_json::to_value(v).unwrap_or_default()),
        ),
        (
            "mode",
            q.mode.map(|v| serde_json::to_value(v).unwrap_or_default()),
        ),
        ("render_style", q.render_style.map(|r| Value::from(r.0))),
        (
            "camera",
            q.camera
                .map(|c| serde_json::to_value(c).unwrap_or_default()),
        ),
    ] {
        if let Some(v) = v {
            png[k] = v;
        }
    }
    if let Some(r) = &q.id {
        let id = object_id(&r.0)?;
        let m = model_of(s.editor.doc(), id)?;
        // The camera frames the box of the model (over the height of the whole scene).
        if let Some((lo, hi)) = m.bounds() {
            png["region"] = json!([lo.x, lo.y, hi.x, hi.y]);
        }
    }
    // The answer always says which camera was used, so that a later call can repeat it.
    png["with_camera"] = json!(true);
    s.dispatch("render.png", png)
}

/// The command that adds the empty model of `mesh.create`.
pub(crate) fn create_command(
    name: Option<String>,
    origin: Option<[f64; 3]>,
    layer: Option<kadakas_core::model::LayerId>,
    class: Option<kadakas_core::model::ClassId>,
) -> Command {
    let mut m = SurfaceModel::new();
    m.name = name.unwrap_or_default();
    if let Some(o) = origin {
        m.transform = glam::DAffine3::from_translation(glam::DVec3::from(o));
    }
    Command::AddObject {
        shape: Shape::SurfaceModel(Box::new(m)),
        layer,
        class,
        attrs: Default::default(),
    }
}
