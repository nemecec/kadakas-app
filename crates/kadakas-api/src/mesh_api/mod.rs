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

use crate::errors::bad;
use crate::params::mesh::*;
use crate::{RpcError, Session};

mod convert;
mod edit;
mod query;
mod read;

pub(crate) use query::QueryCache;

pub const MESH_SUMMARY: (&str, &str) = (
    "mesh.summary",
    "The surface models of the document. params: {id?} (one model, else all). Returns {component_instances (how many component instances the document has: their geometry is not listed here), models: [{id, name, layer, class, parent?, vertices, edges, faces, curves, revision, problem (null or the first problem of the model), hidden_faces, hidden_edges, soft_edges, transform (12 numbers), bounds: {local, world} ({min, max} in mm or null for an empty model)}]}. Coordinates in the other mesh methods are local to the model. Drawing coordinates are the local ones moved by the transform.",
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
    "Faces, edges or vertices near a point. params: {id?, point: [x, y, z] (drawing coordinates, mm), radius (mm), kinds? (faces | edges | vertices, default [faces]), limit? (per kind, default 20, at most 200), include_hidden?}. Returns {point, radius, faces?: [{model, id, distance, closest: [x, y, z] (drawing coordinates)}], edges?, vertices?, skipped?}, nearest first. Hidden faces and edges and soft edges are left out unless include_hidden. Models on a hidden layer or class are left out unless id names the model. skipped lists a model whose faces cannot be cut into triangles. Component instances are not searched.",
);
pub const MESH_PICK: (&str, &str) = (
    "mesh.pick",
    "Shoots a ray at the surface models. params: {id?, origin: [x, y, z], direction: [x, y, z] (drawing coordinates), all? (every face the ray meets, nearest first, at most 200), include_hidden?}. Returns {hit: null | {model, face, distance, point: [x, y, z] (drawing coordinates), local: [x, y, z], side (front | back: the side of the face that the ray meets), normal: [x, y, z] (drawing coordinates)}, hits? (with all, at most 200, truncated is true when more faces were met), skipped?}. It has the rules of mesh.near for hidden items, layers, classes and instances.",
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
    "Soften every edge under an angle. params: {id, below_angle (degrees, 0 to 180), edges? (default all edges of the model), smooth? (default true), coplanar? (also edges between faces in one plane, default false)}. An edge between two faces at an angle below the limit becomes soft. The other edges of the set become hard. Edges without exactly two faces are not touched. Returns the change and three numbers. considered is how many edges the angle test looked at. changed is how many of them got other flags than they had (a second call with the same angle changes 0). softened is how many edges are soft in the whole model after the call.",
);
pub const MESH_TO_SURFACE: (&str, &str) = (
    "mesh.to_surface",
    "Turn a mesh object (shape kind mesh3d, for example from import.file or an IFC import) into a surface model, in place and in one undo step. params: {id, tolerance? (mm, 0.001 to 1000, default 0.01: vertices closer than this weld), soften_below_angle? (degrees, 0 to 180, default off: edges between faces at an angle below it become soft and smooth, with the soften engine of mesh.soften_edges), merge? (default true: triangles in one plane merge into faces with holes; false keeps every triangle a face), dry_run? (convert nothing and answer with the report)}. The model must fit the limits of a surface model: 55,000 vertices, 150,000 edges, 100,000 faces and 300,000 loop vertices, counted after the merge. A mesh over a limit is refused with the counts (error_id mesh_too_large_for_surface, data over and counts) and nothing changes. Returns {id, kind: surface_model, softened_edges (0 when no angle was given), report: {source_vertices, source_triangles, welded_vertices, degenerate_triangles, duplicate_triangles, joined_slivers, vertices, edges, faces, faces_with_holes, merged_triangles, unmerged_regions, healed_vertices, tolerance_mm, border_edges, nonmanifold_edges, closed, area_mm2, volume_mm3?, considered_edges, softened_edges, soften_below_angle?}, notes?}. The color of the mesh becomes the fill of the object. The sides of the faces follow the winding of the triangles.",
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
    "Run many edits as one undo step. All succeed or none does. params: {id? (the model of the edits that give none), edits: [{method, ...params of mesh.<method>}]} with 1 to 50 edits. The add_edges edits draw at most 10000 edges in all, and the edits of any kind make at most 10000 new edges in all (checked after each edit, the batch is undone when it is over). method is one of add_edges, add_curve, erase, reverse_faces, push_pull, follow_me, offset, intersect_faces, transform, copy, paint, hide, soften, soften_edges, solid, make_group, make_component, explode. Every edit is checked before the first one runs. An edit that fails undoes all, and the error names its index. Ids that an earlier edit makes are not known before the batch runs: read them with a first call, or use another call. Returns {count, edits: [the change of each edit], notes}.",
);
pub const MESH_RENDER: (&str, &str) = (
    "mesh.render",
    "A PNG of the model so that you see the result. params: {id? (frame the camera on this surface model), view?, camera? ({target (drawing coordinates, mm), yaw, pitch, distance, fov, projection}, wins over view), width?, height?, projection?, mode? (shaded | wireframe | hidden_line | dashed_hidden_line), render_style? (a style of the document or a built-in one such as Realistic), samples?}. It is render.png with the camera and the framing of a model. Returns {png_base64, width, height, view, projection, mode, camera (in drawing coordinates, give it back as camera to repeat the picture), render_style?}. MCP gives the image itself.",
);

/// The stable ids of refusals (`error_id`). `describe` and `docs/mesh-api.md` list the same names
/// (a test compares them).
pub const ERROR_IDS: &[&str] = &[
    "no_such_object",
    "not_a_surface_model",
    "no_such_vertex",
    "no_such_edge",
    "no_such_face",
    "layer_locked",
    "layer_grey",
    "class_grey",
    "no_such_layer",
    "bad_point",
    "edge_too_short",
    "edge_too_close",
    "model_limit",
    "faces_unsolved",
    "edit_refused",
    "model_invalid",
    "solid_empty",
    "solid_open",
    "solid_self_crossing",
    "solid_inverted",
    "solid_flat",
    "solid_attached",
    "solid_no_result",
    "solid_cut",
    "solid_result_open",
    "solid_result_volume",
    "solid_result_invalid",
    "batch_too_large",
    "batch_edges_too_many",
    "command_refused",
    "not_a_mesh",
    "mesh_too_large_for_surface",
    "mesh_not_convertible",
    "no_such_class",
    "no_such_resource",
];

/// The id of the first model in a fresh session with sequential ids, for the examples.
const EXAMPLE_MODEL: &str = "00000000-0000-0000-0000-000000000002";

/// Most objects of one answer that get the closed-solid check in their `surface` summary
/// (`objects.list`). The check of a model takes up to a second at the limits of a model, so a long
/// list does not run it for every object.
pub(crate) const SUMMARY_CHECKS: usize = 100;

/// What `objects.list` and `objects.get` tell about a surface model, a group of them and a
/// component instance (MOD-13): the object gets a field `surface` with the counts, the
/// closed-solid state with volume and area, the box and the materials (the numbers of Object Info).
/// With `elide` the geometry of a surface model is left out of the answer (a list of many models
/// would be huge); `mesh.vertices`, `mesh.edges`, `mesh.faces` and `objects.get` read it.
pub(crate) fn annotate_object(
    doc: &Document,
    o: &kadakas_core::model::Object,
    v: &mut Value,
    elide: bool,
    checks: &mut usize,
) {
    use kadakas_core::surface::info::{Check, SolidState};
    let check = if *checks > 0 {
        Check::Auto
    } else {
        Check::Never
    };
    let Some(i) = kadakas_core::surface::info::of_object(doc, o, check) else {
        return;
    };
    if check == Check::Auto && i.faces > 0 {
        *checks = checks.saturating_sub(1);
    }
    let solid = match &i.solid {
        SolidState::Empty => json!({"state": "empty"}),
        SolidState::Unchecked => json!({"state": "unchecked"}),
        SolidState::Closed { solids, volume } => {
            json!({"state": "closed", "solids": solids, "volume_mm3": volume})
        }
        SolidState::Open { open_edges } => json!({"state": "open", "open_edges": open_edges}),
    };
    let bounds = i.bounds.map(|(lo, hi)| {
        json!({"min": lo, "max": hi, "size": [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]})
    });
    v["surface"] = json!({
        "kind": o.shape.kind(),
        "models": i.models,
        "vertices": i.vertices,
        "edges": i.edges,
        "faces": i.faces,
        "hidden_faces": i.hidden_faces,
        "hidden_edges": i.hidden_edges,
        "soft_edges": i.soft_edges,
        "solid": solid,
        "area_mm2": i.area,
        "bounds": bounds,
        "materials": i.materials.iter().map(|m| json!({"name": m.name, "faces": m.faces})).collect::<Vec<_>>(),
        "unpainted_faces": i.unpainted_faces,
    });
    if elide && let Some(Value::Object(shape)) = v.get_mut("shape") {
        shape.retain(|k, _| matches!(k.as_str(), "kind" | "name" | "transform" | "v"));
        shape.insert(
            "elided".into(),
            json!("vertices, edges and faces: read them with mesh.vertices, mesh.edges, mesh.faces or objects.get"),
        );
    }
}

/// Extra text for `describe`.
pub fn describe() -> Value {
    let m = EXAMPLE_MODEL;
    json!({
        "model": "A surface model (shape kind 'surface_model') is made of vertices, edges and faces. Coordinates are local, in mm, and the transform of the model places them in the drawing. Vertices, edges and faces have whole-number ids that come from one counter of the model and are never reused. Faces are planar and may have holes. Closed coplanar edges make a face, a new edge splits a face, and geometry of one model sticks together. A group or a component keeps its geometry apart.",
        "in_the_app": "The app shows surface models, groups of them and component instances like other objects (MOD-13): in the plan with their faces in the colours of their materials and the edges that are shown (not the hidden ones and not the soft ones), in 3D and in renders, on sheets (plan, top, elevation, 3D and section viewports) and in the exports. They are selected, moved, turned, copied and deleted with the same methods as other objects (execute move_objects, rotate_objects and the others, transform_objects3d), and visibility by layer and class works. objects.list and objects.get give each of them a field surface with the counts of vertices, edges and faces, the closed-solid state with volume_mm3, area_mm2, bounds and the materials; objects.list leaves the geometry of a surface model out (shape.elided), objects.get has it. objects.query reads Kind = \"surface_model\" or Kind = \"component\". Object Info of the app shows the same numbers, and the plan of a large model is simplified: over 20,000 faces or 30,000 edges only the outline is drawn, and over 20,000 outline lines only the box. render.svg says which one in data-detail (full, outline or box). A hidden-line drawing of a model with more than 50,000 lines uses the silhouettes, creases and borders of its mesh instead of every edge.",
        "scope": "The mesh methods read and edit the surface model objects of the document, also the ones inside groups. Component instances show their definition in renders, but their geometry is not listed by the reads: edit the definition with mesh.explode or work on the model before mesh.make_component. mesh.summary counts the instances (component_instances). mesh.near and mesh.pick leave out the models on a hidden layer or class unless you name the model with id.",
        "workflow": "Make a model with mesh.create. Draw edges with mesh.add_edges and read the ids of the new faces from the answer. Pull a face with mesh.push_pull. Check with mesh.solid_report. Look at it with mesh.render. Use mesh.batch for a long run of edits that must succeed together.",
        "results": "An edit answers {op, id, counts: {vertices, edges, faces}, revision, created: {vertices, edges, faces}, removed: {vertices, edges, faces}, truncated?, notes?}. The lists in created and removed have at most 1000 ids each. Notes tell what the edit changed that you did not ask for, such as a material that a merged face dropped. model_gone is true when the edit moved the whole model away (mesh.make_group of every face), so there are no counts.",
        "soften_edges": "mesh.soften_edges answers with three numbers. considered is how many edges the angle test looked at (edges with exactly two faces, and edges between coplanar faces only with coplanar). changed is how many edges got other flags than they had. softened is how many edges are soft in the whole model after the edit. A second call with the same angle changes 0.",
        "to_surface": "mesh.to_surface turns a mesh3d object into a surface model (MOD-12). Imports of OBJ, STL and glTF files make mesh3d objects, which hold millions of triangles and have no edges and faces to edit. The conversion welds the vertices, merges coplanar triangles into faces with holes, and softens the edges under an angle if you pass soften_below_angle. It reports counts: the answer has softened_edges, and report has the rest. A mesh over the limits of a surface model is refused with the counts and stays a mesh. import.file does the same in one step with the options to_surface and soften_below_angle.",
        "undo": "Every edit method is one undo step, also mesh.erase with edges and faces, and mesh.batch with all its edits. A refused edit changes nothing and makes no step. The person sees the steps of an agent in the activity panel and can revert them.",
        "camera": "The camera of mesh.render and render.png is in drawing coordinates: target is a point of the drawing in mm. The answer of mesh.render has the camera that was used. Give it back as camera and the picture is the same, also after other models were added elsewhere.",
        "errors": format!("A refusal is a validation error (code 3, kind validation). A missing object, layer, vertex, edge or face is not_found (2). A locked layer is a conflict (5). A size limit is limit (6). A parameter of the wrong name or type is invalid_params (-32602) and has no error_id. The data of the error has error_id, a stable name, and the ids that matter (object, layer, edge, face, vertex, operand, step). The message is English text for a person. Ids: {}. The data of an edit in a batch has step, the index of the edit.", ERROR_IDS.join(", ")),
        "limits": {
            "batch_edits": edit::BATCH_EDITS_MAX,
            "batch_edges": edit::BATCH_EDGES_MAX,
            "edges_per_edit": 10000,
            "page": read::PAGE_MAX,
            "why": "docs/modeler-limits.md: one command on a model of 80,000 edges costs about 0.125 s, and an edit of 10,000 edges costs 0.2 to 0.75 s. Measured through mesh.batch on a release build with 72,580 edges: 50 hides took 5.25 s and 48 push_pulls took 15.3 s. A batch stops when the edges that its edits made, of any kind, pass 10,000."
        },
        "agent_changes": "Edits are marked as agent changes. They show in agent.activity and in the Activity panel of the person, who can undo or revert them.",
        "examples": [
            {"method": "mesh.create", "params": {"name": "Shed", "origin": [0, 0, 0]}},
            {"method": "mesh.summary", "params": {}},
            {"method": "mesh.add_edges", "params": {"id": m, "edges": [[[0, 0, 0], [4000, 0, 0]], [[4000, 0, 0], [4000, 3000, 0]], [[4000, 3000, 0], [0, 3000, 0]], [[0, 3000, 0], [0, 0, 0]]]}},
            {"method": "mesh.push_pull", "params": {"id": m, "face": 9, "distance": 2500}},
            {"method": "mesh.faces", "params": {"id": m, "limit": 10, "with_points": true}},
            {"method": "mesh.solid_report", "params": {"id": m}},
            {"method": "mesh.transform", "params": {"id": m, "faces": [9], "scale": {"factor": 0.5, "about": [2000, 1500, 2500]}}},
            {"method": "mesh.soften_edges", "params": {"id": m, "below_angle": 20}},
            {"method": "mesh.near", "params": {"point": [100, 100, 0], "radius": 500, "kinds": ["faces", "edges"]}},
            {"method": "mesh.pick", "params": {"origin": [2000, 1500, 9000], "direction": [0, 0, -1]}},
            {"method": "material.create", "params": {"name": "Brick", "base_color": [180, 80, 60]}},
            {"method": "mesh.batch", "params": {"id": m, "edits": [
                {"method": "add_edges", "edges": [[[6000, 0, 0], [7000, 0, 0]]]},
                {"method": "paint", "faces": [9], "material": "Brick"},
                {"method": "hide", "faces": [9], "hidden": false}
            ]}},
            {"method": "mesh.render", "params": {"id": m, "camera": {"target": [2000, 1500, 1000], "yaw": 0.8, "pitch": 0.5, "distance": 12000, "fov": 45, "projection": "perspective"}, "width": 400, "height": 300}}
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
        "near" | "pick" => query::handle(s, op, p),
        "render" => render(s, p),
        "batch" => edit::batch(s, p),
        "to_surface" => convert::to_surface(s, p),
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

/// Like [`models`], without the models on a hidden layer or class when no model is named.
pub(crate) fn visible_models<'a>(
    doc: &'a Document,
    only: Option<&str>,
) -> Result<Vec<(ObjectId, &'a SurfaceModel)>, RpcError> {
    let mut list = models(doc, only)?;
    if only.is_none() {
        list.retain(|(id, _)| {
            doc.object(*id).is_some_and(|o| {
                doc.layer(o.layer).is_none_or(|l| l.visible)
                    && doc.class(o.class).is_none_or(|c| c.visible)
            })
        });
    }
    Ok(list)
}

/// The orbit camera of a camera in drawing coordinates, for a scene with this origin. The
/// camera is checked first.
pub(crate) fn camera_of(c: &ViewCamera, origin: glam::DVec3) -> Result<Camera, RpcError> {
    if !c.is_valid() {
        return Err(bad(
            "camera: finite numbers, distance above 0, fov 1 to 170 degrees, projection perspective or ortho",
        ));
    }
    Ok(Camera::from_saved(c, origin))
}

/// A camera as JSON in drawing coordinates, in the form that `camera` takes.
pub(crate) fn camera_json(c: &Camera, origin: glam::DVec3) -> Value {
    serde_json::to_value(c.to_saved(origin)).unwrap_or(Value::Null)
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
