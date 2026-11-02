# Agent API for surface models (`mesh.*`)

This page is the guide for MOD-11. An agent draws and edits surface models with the same tools as the 3D modeler. It works over JSON-RPC, over MCP and in Python. The methods are thin wrappers. The geometry code is `kadakas_core::surface`. The API module is `crates/kadakas-api/src/mesh_api`.

The wording follows public help texts only. No other program was used as a source.

## The model

A surface model has vertices, edges and faces. Coordinates are local and in millimeters. The transform of the model places them in the drawing. Faces are flat. Some faces have holes.

The ids of vertices, edges and faces are whole numbers. They come from one counter in each model. An id is never used twice. So a vertex and a face never have the same id.

A closed set of coplanar edges makes a face. A new edge splits a face. Geometry in one model sticks together. A group or a component keeps its geometry apart.

`describe` has a section `mesh` with the model, the limits and examples. `describe {method: "mesh.push_pull"}` gives the JSON Schema of one method.

## Methods

Reads. They change nothing.

| Method | What it does |
|---|---|
| `mesh.summary` | The surface models of the document, with counts, problem, bounds and transform. |
| `mesh.vertices` | Vertices, in pages or by id. |
| `mesh.edges` | Edges with their faces, flags, curve and length. |
| `mesh.faces` | Faces with loops, holes, normal, area, paint and hidden state. |
| `mesh.solid_report` | The closed-solid check of MOD-09, with the open edges. |
| `mesh.bounds` | The box around a model, local and in the drawing. |
| `mesh.near` | Faces, edges or vertices near a point. |
| `mesh.pick` | The faces that a ray meets. |
| `mesh.curve_info` | The curves of a model with their exact circle, radius, center and segments (MOD-15). |

Edits. Each one is one undo step.

| Method | What it does |
|---|---|
| `mesh.create` | An empty model. |
| `mesh.add_edges` | Straight edges. Faces form and split by themselves. |
| `mesh.add_curve` | A chain that stays one curve, such as a circle. It takes the exact circle as `analytic`. |
| `mesh.set_segments` | Rebuild a circle or an arc with another number of segments (MOD-15). |
| `mesh.erase` | Erase edges and faces. |
| `mesh.reverse_faces` | Turn faces over. |
| `mesh.push_pull` | Push or pull a face. |
| `mesh.follow_me` | Sweep a face along edges. |
| `mesh.offset` | Offset a face or a chain of edges. |
| `mesh.fillet` | Round edges of a closed solid (MOD-21). |
| `mesh.chamfer` | Cut edges of a closed solid flat (MOD-21). |
| `mesh.intersect_faces` | Cut faces where they cross. |
| `mesh.transform` | Move, rotate or scale. |
| `mesh.copy` | Copy faces and edges in the model. |
| `mesh.paint` | Put a material on faces. |
| `mesh.hide` | Hide or show faces and edges. |
| `mesh.soften` | Set the soft and smooth flags of edges. |
| `mesh.soften_edges` | Soften every edge under an angle. It counts the edges that changed. |
| `mesh.to_surface` | Turn a mesh object (from `import.file`) into a surface model. It can soften the edges under an angle. See `docs/interop.md`. |
| `mesh3d.exact_info` | The true radii, axes and normals that a mesh from a STEP import keeps. See `docs/exact-solids.md`. |
| `mesh3d.retessellate` | Cut the triangles of such a mesh again at another tolerance. One undo step. |
| `mesh.solid` | Union, subtract, intersect, trim or split two solids. With `snap_near` it also snaps near-touches. |
| `mesh.make_group` | Make a group. |
| `mesh.make_component` | Make a component. |
| `mesh.explode` | Explode groups and component instances. |

Batch and picture.

| Method | What it does |
|---|---|
| `mesh.batch` | Many edits, one undo step, all or nothing. |
| `mesh.render` | A PNG of the model. |

## A first run

```json
{"method": "mesh.create", "params": {"name": "Shed"}}
```

The answer has the id of the model. Draw a rectangle with four edges.

```json
{"method": "mesh.add_edges", "params": {"id": "<model>", "edges": [
  [[0, 0, 0], [4000, 0, 0]], [[4000, 0, 0], [4000, 3000, 0]],
  [[4000, 3000, 0], [0, 3000, 0]], [[0, 3000, 0], [0, 0, 0]]]}}
```

The answer lists the new ids. The new face is in `created.faces`. Pull it up.

```json
{"method": "mesh.push_pull", "params": {"id": "<model>", "face": 9, "distance": 2500}}
```

Check the result and look at it.

```json
{"method": "mesh.solid_report", "params": {"id": "<model>"}}
{"method": "mesh.render", "params": {"id": "<model>", "width": 800, "height": 600}}
```

In Python the same calls are typed methods of `doc.api`.

```python
import kadakas

doc = kadakas.open()
model = doc.api.mesh_create(name="Shed")["id"]
drawn = doc.api.mesh_add_edges(id=model, edges=[
    [[0, 0, 0], [4000, 0, 0]], [[4000, 0, 0], [4000, 3000, 0]],
    [[4000, 3000, 0], [0, 3000, 0]], [[0, 3000, 0], [0, 0, 0]]])
face = drawn["created"]["faces"][0]
doc.api.mesh_push_pull(id=model, face=face, distance=2500)
assert doc.api.mesh_solid_report(id=model)["closed"]
```

## The answer of an edit

An edit answers with the change it made.

```json
{
  "op": "push_pull",
  "id": "<model>",
  "counts": {"vertices": 8, "edges": 12, "faces": 6},
  "revision": 2,
  "created": {"vertices": [10, 11, 12, 13], "edges": [14, 15, 16, 17, 18, 19, 20, 21], "faces": [22, 23, 24, 25, 26]},
  "removed": {"vertices": [], "edges": [], "faces": []}
}
```

The answer above is the push of the face of a rectangle of 4000 by 3000 mm. `created` and `removed` list at most 1000 ids each. If a list was cut, `truncated` is true. `notes` is in the answer only when the edit changed something that you did not ask for. An example is a material that a merged face dropped.

`mesh.soften_edges` adds `changed`, `softened` and `considered`. Take a box with 12 edges that all meet at 90 degrees and call it with an angle of 20. `considered` is 12, the edges that the test looked at. `changed` is 0, because no edge got other flags than it had. `softened` is 0, the number of soft edges in the whole model after the call. Call it with 100 and `changed` is 12 and `softened` is 12. Call it again with 100 and `changed` is 0 and `softened` stays 12. Edges that do not have exactly two faces are not looked at.

`mesh.make_group`, `mesh.make_component` and `mesh.explode` add `created_objects`, `removed_objects` and `created_resources`. `mesh.make_group` also names the new group object in `group`. When the group or component takes every face of the model, the model moves away with them. Then the answer has `model_gone: true` and no counts.

## Transform

`mesh.transform` and `mesh.copy` take a `matrix` of 12 numbers. They also take any of `translate`, `rotate` and `scale`. The order is scale, then rotate, then translate.

```json
{"method": "mesh.transform", "params": {"id": "<model>", "faces": [9],
  "rotate": {"axis": [0, 0, 1], "degrees": 15, "about": [0, 0, 0]},
  "translate": [100, 0, 0]}}
```

## Soften by angle

```json
{"method": "mesh.soften_edges", "params": {"id": "<model>", "below_angle": 20}}
```

An edge between two faces at an angle below 20 degrees becomes soft and smooth. The other edges in the set become hard. Give `edges` to limit the set. Set `smooth` to false to make soft edges that do not smooth the shading. Edges between faces in one plane are left out unless `coplanar` is true.

## Solid tools

`mesh.solid` takes `op` (`union`, `subtract`, `intersect`, `trim` or `split`). Name the solids with face lists in `first` and `second`. Or give one face of each in `first_face` and `second_face`. The solid that holds the face is used.

A solid that is not closed is refused. The refusal names the edge to fix. Use `mesh.solid_report` to see all open edges first.

### Snap near-touches

A vertex of one solid can lie off a face plane of the other solid. The distance is above 0.0025 mm and at most 0.01 mm. The cut is refused then. The tool never moves the vertex on its own.

Limit of the solid tools: they do not cut overlapping faces of two solids in one plane (backlog row MOD-20). A snap that leads there is refused with `snap_move_refused` or `snap_still_refused`.

When a snap resolves the refusal, the error id is `near_touch_snap_available`. The data has `vertices` and `flag`. Each vertex has `vertex`, `faces`, `from`, `to` and `distance_mm`. `flag` is `snap_near`. `max_distance_mm` is the largest move. Nothing changes. Before this change the refusal had the id `solid_cut`. An agent that matches `solid_cut` must match `near_touch_snap_available` too.

Repeat the call with `snap_near: true`. The tool moves the vertices onto the planes along the face normals. Then it runs the operation. The move and the operation are one undo step.

The answer lists each moved vertex in `snapped` and in `notes`. The positions have four decimals. `snapped` is an empty list when nothing was moved. A call without `snap_near` gives the same geometry as before. Only the id of a refusal that a snap resolves is new.

```json
{"method": "mesh.solid", "params": {"id": "<model>", "op": "union", "first_face": 25, "second_face": 41, "snap_near": true}}
```

The tool refuses a snap that fails, with its own id. The model is unchanged. See the table of error ids.

Cost. Measured once (release build, one run each, other load not controlled, `perf_near_touch_in_a_big_model` in `surface/tests_solid.rs`, run with `cargo test --release -- --ignored`). The model has two balls of 20,480 triangles each (40,960 faces in all) and a pair of boxes. The tool times include the full check of the model.

| What | Time |
|---|---|
| Union of a pair without a near-touch | 0.83 s |
| Union refused for a near-touch, no flag (includes the test of the snap) | 0.39 s |
| Union with `snap_near` | 0.82 s |
| Split of a pair without a near-touch | 0.81 s |
| Split refused for a near-touch, no flag | 0.38 s |
| Split with `snap_near` | 0.82 s |

So a refusal is cheaper than a successful operation, and the snap adds no measurable time to it. The check of the model dominates. This is one measurement on one model, not a campaign.

## Fillet and chamfer (MOD-21)

`mesh.fillet` rounds edges of a closed solid. `mesh.chamfer` cuts them flat. Each call is one undo step.

```json
{"method": "mesh.fillet", "params": {"id": "<model>", "faces": [9], "radius": 50}}
{"method": "mesh.chamfer", "params": {"id": "<model>", "all": true, "distance": 20}}
```

Choose the edges in three ways. They add up.

- `edges`: these edge ids, taken as given.
- `faces`: the sharp edges of these faces.
- `all`: every sharp edge of the model.

A sharp edge lies between two faces at an angle above `min_angle` (5 degrees by default). It is not soft or smooth, so the facets of a cylinder stay. It is convex, or concave with `concave: true`.

`mesh.fillet` takes `radius`, `segments` (1 to 64) and `smooth` (default true). The default number of segments turns at most 15 degrees per segment. With `smooth` the edges inside the round become soft and smooth, and the rails become soft. `mesh.chamfer` takes `distance`, the distance from the edge on both faces.

Faces stay flat and the result is a closed solid. The faces next to the edges keep their ids. The new faces and edges are in `created`, and the removed edges in `removed`.

Corners: exactly three faces meet at each end of a chosen edge. One chosen edge runs to the third face. Two chosen edges meet on a plane through the sharp third edge. Three meet on a ball patch, or on a triangle for a chamfer. A corner of four or more faces is refused. A corner with chosen convex and concave edges is refused too. See `docs/modeler.md`, section "Fillet and chamfer", for a description of the corners and the limits.

A size that does not fit is refused with `fillet_does_not_fit`. The message ends with "at most about X mm fits", and `max_mm` holds X. X is the largest size that the whole edit accepts, rounded down to two significant digits, so a second call with X works. More than 10,000 new edges is refused with `fillet_too_many_edges`. Then `max_segments` holds the most segments that fit. Both work in `mesh.batch` as `fillet` and `chamfer`.

## Batch

`mesh.batch` runs many edits as one undo step. All edits work or none does.

```json
{"method": "mesh.batch", "params": {"id": "<model>", "edits": [
  {"method": "add_edges", "edges": [[[0, 0, 3000], [4000, 0, 3000]]]},
  {"method": "paint", "faces": [9], "material": "Brick"},
  {"method": "hide", "edges": [3], "hidden": true}
]}}
```

Each edit has a `method` (without `mesh.`) and the parameters of that method. An edit gives its own `id` if it needs one. Without it the `id` of the batch is used.

Every edit is checked before the first one runs. A wrong parameter is an error that names the index, for example `edits[2]`. If an edit is refused while it runs, all earlier edits are undone. The error has `step`, the index of the edit.

The ids that an edit makes are not known before the batch runs. Say an edit needs the id of a new face. Run the first part and read the answer. Then run the second part.

The answer has `count` and `edits`, the change of each edit.

### The cap

A batch has at most 50 edits. The `add_edges` edits of a batch draw at most 10,000 edges in all. A call over a cap is refused at once with the id `batch_too_large` or `batch_edges_too_many`.

A batch also stops when its edits made more than 10,000 edges in all, of any kind: copies, Follow Me, Offset, Intersect Faces and the solid tools count too. The check runs after each edit. The error `batch_edges_too_many` names the edit (`step`) and all edits are undone.

The caps come from `docs/modeler-limits.md`. A command on a model of 80,000 edges costs about 0.125 s. An edit of 10,000 edges costs 0.2 to 0.75 s. I measured a batch through the API on a release build with a model of 72,580 edges, 36,481 vertices and 36,100 faces. Fifty `hide` edits took 5.25 s (0.105 s each). Forty-eight `push_pull` edits took 15.3 s (0.32 s each). A batch with one `add_edges` of 5,000 edges took 0.33 s. So a batch of 50 stays near 5 s for cheap edits and near 16 s for Push/Pull on a model of that size. A debug build is slower. The cost of a batch is the sum of its edits, so a call that must stay short uses fewer edits.

## Undo and review

Every edit method is one undo step. This includes `mesh.erase` with edges and faces, and `mesh.batch` with all its edits. An edit that is refused changes nothing and makes no step.

Edits of an agent are marked as agent changes. They show in `agent.activity` and in the Activity panel. The person undoes or reverts a step there. There is no review window of its own.

## Delete, selection and the camera (AGR-09)

An agent that builds a model needs three more things. They are not `mesh.*` methods, but a model builder uses them all the time.

### Delete objects

`objects.delete {ids}` deletes whole objects, for example a surface model or a group. It runs the command `delete_objects`, so it has the same checks. A locked layer, a grey layer or a grey class refuses the call. A group goes with its members. The call is one undo step.

An unknown id is the error `no_such_object`. Nothing is deleted then. An empty `ids` is the error `ids_empty`. The answer is `{count, deleted: [id]}`. `deleted` is in id order and also lists the members of a deleted group. The deleted objects leave the selection, and an undo does not bring the selection back.

To delete objects together with other edits in one step, use the command in a batch: `execute {command: {op: "batch", commands: [..., {op: "delete_objects", ids}]}}`. `mesh.batch` takes edits of one model and has no delete. To delete a face or an edge, use `mesh.erase`.

In MCP the method is in the tool `document`, which has the destructive hint. The tool `objects` only reads. In Python it is `doc.api.objects_delete(ids=[...])`.

### An unknown method

The answer to a method that does not exist names listed methods, up to three. `data.closest` has the same list. A name with a near spelling, such as `mesh.facez`, gets "Closest listed methods". A name that only shares a word gets "Listed methods with a word in common".

A name like `object.delete`, `objects.remove` or `delete_objects` also gets a hint: call `objects.delete {ids}` or the command form `execute {command: {op: "delete_objects", ids}}`. A name with a near spelling or a delete hint gets no feedback draft. The answer already points to a method.

A name longer than 64 characters is not searched. The message shows its first 64 characters and an ellipsis, and the answer has no `closest`.

### The selection with faces and edges

`selection.get` returns `{ids, faces, edges}`. `ids` are whole objects. `faces` is `{model id: [face id]}` and `edges` is `{model id: [edge id]}`. The face and edge ids are the ones of `mesh.faces` and `mesh.edges`. Both maps are empty when no part is selected. Whole objects and parts are never selected together. A selection of objects ends the selection of parts. The parts do not come back when the objects are deselected.

`selection.set` takes the same shape: `{ids?, faces?, edges?}`. What is left out is empty. A model that is not a surface model is the error `not_a_surface_model`. A face or edge that does not exist is `no_such_face` or `no_such_edge`. A model on a grey layer or in a grey class is refused, as it is for `ids`. The call refuses `ids` together with `faces` or `edges`. A refused call leaves the selection as it was. An undo or a delete removes the parts that are gone. An undo does not bring a selection back.

What is done and what is not:

- Done: the answer shape and the store in the session (`Session::parts`). Also done: the mirror of the picks of the 3D modeler into the session, and `selection.set` of parts.
- Not done: no host connects a face pick of the person to an agent yet. The modeler has an editor with face picking, but no agent connection. The app has an agent connection, but no face picking, so it refuses faces and edges in `selection.set` as `parts_not_supported`. Until the modeler has an agent transport, only `selection.set` fills the maps, for example in `kadakas serve`. The backlog rows AGR-10 and the modeler agent transport close this gap.

`view.get` in the app also has `selection_parts` with the same two maps.

```json
{"method": "selection.get"}
{"ids": [], "faces": {"0190f1c2-...": [14, 15]}, "edges": {}}
```

### The camera of the person

`view.get` (app only) answers in 3D with `camera`. It has the keys of the camera of `render.png` and `mesh.render`. These are `target` (drawing mm), `yaw` and `pitch` (radians), `distance` (mm), `fov` (degrees) and `projection`. The older keys stay: `view`, `render_mode`, `eye`, `up`, `fov_deg` and `visible_box`. `aspect` is the width of the canvas over its height. `canvas` in the answer has the size in points.

Give that object back as it is. `view.camera`, `render.png` and `mesh.render` use one rule for the position:

- Give `eye` and `target`, or `target`, `yaw`, `pitch` and `distance`. One form is used as it is.
- With both forms, they must give the same eye. A gap up to 1e-3 of the distance is fine. A larger gap is refused as `camera_conflict`, and the message names the keys.
- `yaw`, `pitch` and `distance` go together. `pitch` is from -pi/2 to pi/2. The same range holds in all three calls.
- The orbit form is used as it is, never through an eye. Top and Bottom keep their turn. An eye straight above or below the target has no turn. `view.camera` then keeps the turn of the view. `render.png` uses the turn of the Top view.
- `render.png` and `mesh.render` default `fov` to 45 and `projection` to perspective.

The render calls and `view.camera` ignore some keys by design: `view`, `render_mode`, `up`, `fov_deg`, `aspect` and `visible_box`. So the camera of `view.get` goes in whole. A key that no call knows is an error that names the nearest key. `saved_view.save` and the other write calls take the strict camera `{target, yaw, pitch, distance, fov, projection}` and refuse every other key.

- `render.png {camera: <view.get camera>, width, height}` draws the view of the person. Choose a width and height with the ratio `aspect`, and the picture has the same frame.
- `view.camera <view.get camera>` moves the view of the person to that camera.
- `mesh.render {camera: <view.get camera>}` draws the same view with a model to look at.

The camera that `render.png` or `mesh.render` returns has the same six keys. It goes back into any of these calls. The app tests check the round trip `view.get`, `view.camera`, `render.png` for Top, Bottom, far targets, isometric and random views.

## The pointer of the person (AGR-10)

People say "this" and "here" while they point at the view. `view.get` (app only) has the field `pointer`. It tells where the pointer is and what is under it.

```json
{"method": "view.get"}
{"mode": "3d", "canvas": {"width": 1180, "height": 760}, "pointer": {
  "over_canvas": true, "canvas": [412.5, 301.0], "age_ms": 340, "navigating": false,
  "focused": true, "mode": "3d",
  "ray": {"origin": [2000, -7000, 6000], "direction": [0.0, 0.62, -0.78]},
  "hit": {"model": "0190f1c2-...", "face": 14, "edge": 31, "distance": 8120.4,
          "point": [2000, 727.3, 1000], "local": [2000, 727.3, 1000], "side": "front", "normal": [0, 0, 1]}}}
```

The fields:

- `pointer` is `null` when the pointer is not over the drawing area. This holds when it is outside the window, over a menu or a palette, or under a dialog. It also holds when another tab covers the drawing area or the window is minimised. Then `pointer_reason` is `not_over_canvas`.
- `pointer` is also `null` when the person turned off "Agents may see my pointer" in Preferences. Then `pointer_reason` is `disabled_by_user`. The setting is on by default.
- `canvas` is `[x, y]` in points from the top left corner of the drawing area. `canvas` in the rest of `view.get` has the size of that area, so the two share one frame.
- `age_ms` is the time since the pointer last moved. The session counts it when you ask. A resting pointer has a growing age. It counts the movement of the pointer only. A camera that moves under a still pointer does not reset it.
- `navigating` is true while the person moves the view, with a drag, the wheel or a key.
- `focused` is true when the window has the focus of the system.
- `mode` is `plan` or `3d`, as in `view.get`.

In the plan, `pointer` adds `point` and `ids`. `point` is `[x, y]` in drawing mm, not snapped. `ids` has the object that a click at the cursor selects, as a list with one id. The cursor is snapped, as it is for a click. The list is `[]` when no object is there. A group is named, not its member, until the person enters the group.

In 3D, `pointer` adds `ray`, `hit` and, sometimes, `nearest_object`:

- `ray` is `{origin, direction}` in drawing mm. `direction` is a unit vector. A parallel view starts the ray in front of the models. Give both to `mesh.pick` as they are.
- `hit` is the answer of `mesh.pick` for that ray, the key `hit` of it. It has `model`, `face`, `distance`, `point`, `local`, `side` and `normal`. It is `null` when no surface model is hit. A group member is named by its model, so the group of the plan is not in `hit`.
- `hit.edge` is added when the point is within 6 points of the screen from a shown edge of the face. It is the id of `mesh.edges`. It is left out otherwise. `mesh.pick` itself never has it.
- Only surface models that are displayed count. These are not met: a model hidden with Hide, a model hidden by a phase rule, and a model on a hidden layer, a hidden class or a sheet layer. `mesh.pick` has the same rule. A model on a grey layer or class is skipped by the pointer, because the person cannot pick it. `mesh.pick` still reads it. A model on a locked layer is met, so the agent can say what the person points at.
- Objects that are not surface models are components, slabs, site models, plants, imported meshes and the like. A ray meets them too. When one is nearer than every surface model, `hit` is `null` and `nearest_object` is `{id, kind, distance, point}`. `mesh.pick` knows no such object, so the test compares with the same ray: if `nearest_object` is set, the ray meets that object first. The test uses the triangles that the 3D view draws, after a box test per object.

A test checks that `mesh.pick` with the ray gives the same `hit`, apart from `edge` and apart from grey models.

### Who delivers the pointer

| Host | Pointer data |
|---|---|
| The app (`kadakas app --agent-port`) | Yes. The plan and the 3D view show the pointer to the session in each frame. `view.get` answers from it. |
| `kadakas serve --connect` and `kadakas mcp --connect` | Yes. They forward every request to the live app, so `view.get` is the app's. |
| `kadakas serve` alone | No. It has no window, so `view.get` is the error that names the app. |
| The 3D modeler program | No. It has no agent connection, and its status bar has no X and Y box. The mirror that `Session::parts` has (`parts_sync.rs`) is not built for the pointer. |

The session keeps the sample (`kadakas_api::Session::set_pointer`, code in `crates/kadakas-api/src/pointer_api.rs`). A host with an agent connection calls it once per frame. It calls `pointer_new_frame` before it draws the area. A sample that no frame renewed is old, and the answer is `null`. The app also clears the sample when the window is hidden or minimised.

### A window that is not in front

For the main use case the person types "this" in a terminal while the pointer is over the Kadakas window. The Kadakas window then has no focus. On macOS the system does not always tell a window without focus where the pointer moves. So the position can be old. `focused` is `false` in that case. `age_ms` then shows the time since the last move that the app saw, and it keeps growing. An agent that reads a large `age_ms` together with `focused: false` asks the person.

### Privacy and consent

The position of the pointer stays in the app. Only a `view.get` call of the agent reads it. It is not saved, not in the document, not in the undo history and not sent to collaborators. The cursor that collaborators see (PLAT-07) is a separate, shared position and is not touched.

The person controls the reading:

- The setting "Agents may see my pointer" in Preferences is on by default. Off: `pointer` is `null` and `pointer_reason` is `disabled_by_user`. Nothing else changes. The status bar of the person keeps working.
- An eye in the status bar shows for 10 seconds after an agent read the pointer. Its tooltip says "An agent reads your pointer."
- The Agent Activity panel lists the reads as "Read pointer, N s ago". It lists at most one line a minute. A read is not a change of the drawing, so it is not a step of the activity log. The panel shows these lines apart, and they are not saved.
- A client gets at most 60 pointer answers in a second. After that it gets its last answer again, not an error.
- The lock "Agent may move my view" is about moving the view. It does not touch the pointer. `view.get` works while the view is locked.

### The status bar

The X and Y numbers in the status bar use the same data. They show the point under the pointer and nothing otherwise:

- In the plan they show the cursor while the pointer is over the drawing area.
- In 3D they show the point of `hit`. They show nothing in three cases. The view moves, no surface model is under the pointer, or another object is nearer.
- Outside the drawing area they show nothing. The text keeps its width, so the bar does not jump.
- The map readout (N and E, or lat and lon) keeps its place in the same way. It is blank while the pointer is away. A click on it still switches between the two forms. Its slot is 34 characters wide.

The setting of the consent does not change the status bar.

### Cost

A pointer move in 3D runs one ray against the surface models. A box around each model keeps most models from being tried. A model is cut into triangles again only when it changed. Take a drawing of 200 surface models with about 2,000 triangles each. In a debug build a pointer move took 27.3 ms before this and 0.23 ms after. A release build is faster. The next pick after an edit of one model took 297 ms before (with the edit) and 2.3 ms after. Run the measure by hand: `cargo test -p kadakas-api --test pointer_cost -- --ignored --nocapture`.

### Not done

The deliberate gesture is not built: Alt+click or a Point tool that sends `view.pointed {model, face, point, ids}` as an event. Events carry document changes only (PLAT-02). Selection events are still open (API-17). The gesture needs a second kind of event first. Until then the agent reads `pointer` when the person says "this".

## Errors

A refusal is a validation error (code 3). A missing object, layer, class, vertex, edge or face is `not_found` (code 2). A locked layer is a `conflict` (code 5). A size limit is `limit` (code 6). A wrong parameter is `invalid_params` (code -32602) and has no `error_id`.

The data of the error has `error_id`, a stable name. It also has the ids that matter. The message is English text for a person. A tool reads `error_id`, not the message.

| `error_id` | Meaning | Facts |
|---|---|---|
| `no_such_object` | The object does not exist. | `object` |
| `not_a_surface_model` | The object is another kind. | `object`, `kind` |
| `no_such_vertex`, `no_such_edge`, `no_such_face` | The id is not in the model. | `object`, `vertex`, `edge` or `face` |
| `layer_locked` | The layer of the model is locked. | `object`, `layer` |
| `layer_grey`, `class_grey` | The layer or class of the model is grey. Make it visible first. | `object`, `layer` or `class` |
| `no_such_layer`, `no_such_class`, `no_such_resource` | A command named a layer, class or resource that is not there. | `layer`, `class` or `resource` |
| `bad_point` | A point is not finite or is out of range. | `object` |
| `edge_too_short` | An edge is shorter than the length tolerance. | `tolerance_mm` |
| `edge_too_close` | An edge passes too close to other geometry. | `tolerance_mm` |
| `model_limit` | The edit breaks a size limit of the model. | `what`, `max`, `count` |
| `faces_unsolved` | The faces of a plane are not solved. | |
| `edit_refused` | The edit makes a face collapse or a loop cross itself. | |
| `model_invalid` | The result is an invalid model. | |
| `solid_empty` | A solid has no faces. | `operand` |
| `solid_open` | A solid is not closed. | `operand`, `edge`, `open_kind`, `open_edges` |
| `solid_self_crossing` | A solid passes through itself. | `operand`, `edge`, `face` |
| `solid_inverted` | A solid looks inward. Use `mesh.reverse_faces`. | `operand` |
| `solid_flat` | A solid has no volume. | `operand` |
| `solid_attached` | A solid is stuck to other geometry. Make a group first. | `operand` |
| `solid_no_result` | The result is empty. | `solid_op` |
| `solid_cut` | The solids are not cut cleanly. | |
| `near_touch_snap_available` | A near-touch refused the cut. A snap resolves it. Repeat with `snap_near`. | `vertices`, `flag`, `max_distance_mm` |
| `snap_vertex_shared` | A vertex to snap is used by geometry outside the operation. | `vertex` |
| `snap_degenerate_face` | The snap leaves a face with a very short edge or no area. | `face` |
| `snap_move_refused` | The model refuses the move of the vertices. | |
| `snap_breaks_solid` | A solid is not closed after the snap. | `operand` |
| `snap_new_near_touch` | A vertex stays near another plane, or the move takes it off a plane it is on. | `vertex`, `face`, `distance_mm` |
| `snap_still_refused` | The operation is still refused after the snap. | |
| `solid_result_open` | The result is not a closed solid. | |
| `solid_result_volume` | The volume of the result does not fit. | |
| `solid_result_invalid` | The result makes the model invalid. | |
| `batch_too_large` | More than 50 edits. | `max`, `count` |
| `batch_edges_too_many` | More than 10,000 edges in all. | `max`, `count` |
| `command_refused` | A group or component command was refused. | `op` |
| `not_a_mesh` | `mesh.to_surface` was called on an object that is not a mesh (`mesh3d`). | `object`, `kind` |
| `mesh_too_large_for_surface` | The mesh is over a limit of a surface model. Nothing changes. | `over` (each count with its limit), `counts` |
| `mesh_not_convertible` | The mesh gives no valid surface model (no triangle with an area, or a point out of range). | |
| `no_exact_data` | `mesh3d.exact_info` or `mesh3d.retessellate` was called on a mesh that keeps no exact data (only a STEP import with `keep_exact` does). | `object` |
| `no_such_curve` | The curve is not in the model. | `object`, `curve` |
| `curve_without_description` | The curve has no exact circle. Old curves and curves that an edit made inexact have none. | `curve` |
| `segments_out_of_range` | The number of segments is not from 3 to 1000. | `segments`, `min`, `max` |
| `curve_vertex_shared` | Another edge meets the chain inside it. | `vertex` |
| `curve_borders_other_plane` | A face on another plane uses the curve, for example the wall of a pushed circle. | `face` |
| `curve_loop_unclear` | A loop of a face does not run once along the whole chain. | `face` |
| `curve_result_invalid` | The new chain makes the model invalid, for example edges that are too short. | `reason` |
| `curve_unknown_kind` | The circle has a kind from a newer program. It is read only. | `curve` |
| `fillet_no_edges` | `mesh.fillet` or `mesh.chamfer` found no edge to change. | |
| `fillet_edge_not_solid` | An edge is not between exactly two faces. | `edge` |
| `fillet_not_solid` | The faces of the edge are not a closed solid. | `edge`, `reason` |
| `fillet_edge_flat` | The faces of an edge lie in one plane. | `edge` |
| `fillet_corner_unsupported` | A corner of four or more faces, or of convex and concave edges. | `vertex`, `reason` |
| `fillet_does_not_fit` | The radius or distance does not fit. | `what`, `asked_mm`, `max_mm`, `reason` |
| `fillet_too_small` | A segment is shorter than the length tolerance. | `edge`, `segments` |
| `fillet_segments_out_of_range` | The segments are not 1 to 64. | `segments`, `min`, `max` |
| `fillet_too_many_edges` | The edit makes more than 10,000 new edges. `max_segments` is the most segments that fit, or null. | `count`, `max`, `max_segments` |
| `fillet_bad_size` | The size is not a number above 0.01 mm. | `asked_mm` |
| `fillet_result_invalid` | The result is not a valid closed solid. | `reason` |
| `exact_stale` | `mesh3d.retessellate`: the triangles were edited after the import, so the exact data no longer fits. | `object` |

`operand` is `first` or `second`. `open_kind` is `border`, `non_manifold` or `reversed`. In a batch the data also has `step`.

## Circles and arcs (MOD-15)

A curve is a chain of straight edges that stays one circle or arc. A curve also keeps the exact circle that its corners lie on.
The circle has a kind (`circle`, `arc` or `polygon`), a center, a normal, an axis, a radius and, for an arc, a start and an end angle.
The axis is the direction of the angle 0. Angles run counterclockwise about the normal. The number of segments is the number of edges.
Coordinates are local, in millimeters and radians.

`mesh.add_curve` takes the circle as `analytic`. The kind is `circle`, `arc` or `polygon`. Unknown fields are refused. The core keeps it only when every point lies on the circle within the length tolerance.
Else the curve is drawn without it and the notes say so.

`mesh.curve_info` reads the curves of a model, or one curve with `curve`. Each item has `edges`, `segments`, `closed`, `length_mm` and `geometry`.
`length_mm` is the circumference for a circle and the length of the chain for an arc or a polygon.
`geometry` is null for an old curve. An arc also has `start_deg`, `end_deg` and `radians`.

```json
{"method": "mesh.curve_info", "params": {"id": "<model>", "curve": 20}}
{"total": 1, "offset": 0, "items": [{"curve": 20, "edges": [21, 22], "segments": 24, "closed": true,
  "length_mm": 1884.96,
  "geometry": {"kind": "circle", "center": [0, 0, 0], "normal": [0, 0, 1], "axis": [1, 0, 0],
               "radius": 300}}]}
```

`mesh.set_segments {id, curve, segments}` rebuilds the chain with 3 to 1000 edges on the same circle. It is one undo step.
The ends of an arc stay. The first vertex of a circle stays. Faces in the plane of the circle follow, with the new vertices.
Edges that stay keep their own flags (soft, smooth, faceted). New edges take the flags of the first edge of the chain.

The call is refused, with an `error_id`, in these cases.
- The curve has no circle: `curve_without_description`.
- Another edge meets the chain inside: `curve_vertex_shared`.
- A face on another plane uses the chain: `curve_borders_other_plane`. A circle under the wall of a pushed cylinder is an example.
- The new edges are too short or cross other geometry: `curve_result_invalid`.

What the other edits do with the circle.

| Edit | Result |
|---|---|
| Move, rotate, mirror, uniform scale | The circle moves with the chain. The radius scales. |
| Scale with other factors, move of single points | The circle is dropped. The notes say so. The chain stays a curve. |
| Erase that cuts a circle | The pieces are arcs on the same circle, with new angles. |
| Erase at the ends of an arc | The arc is shorter. The angles are updated. |
| Edge across the curve | The chain gets a vertex on the chord. The circle stays. |
| Push/Pull, Follow Me, copy | The copy gets its own circle, at the new place. |
| Offset | The new curve gets a circle with the radius of its corners. |
| Group, component, explode, copy | The circle is fitted again where the chain lands. If the points are not on a circle (a group scaled unequally), the circle is dropped and the notes say so. |

The file keeps the circles in the model field `curve_geometry`, a list of `[curve id, {description}]`. A file without it loads as before.
An older program keeps the field it does not know. A row whose curve is gone or whose points are off the circle is left out on load.

## Reading in pages

`mesh.vertices`, `mesh.edges` and `mesh.faces` take `offset` and `limit`. The default `limit` is 100. The most is 1000. Give `ids` to read only some. Ids that do not exist are listed in `missing`.

## Questions about space

`mesh.near` takes a point in drawing coordinates and a radius. It returns faces, edges or vertices, nearest first, at most `limit` of each kind (200 at the most). It leaves out hidden faces and edges and soft edges unless `include_hidden` is true. It leaves out the models that are not displayed unless you name the model with `id`. A model whose faces cannot be cut into triangles is listed in `skipped` and not searched.

`mesh.pick` shoots a ray. The answer says which face was hit, the distance, the point, and the `side` that was met (`front` or `back`). With `all` it lists every face on the ray, nearest first. It lists at most 200 and then `truncated` is true. It has the same rules for hidden items, layers, classes and `skipped` as `mesh.near`. Both methods keep the triangles of a model for a revision of the document, so many calls in a row cut the faces once.

## Pictures

`mesh.render` is `render.png` with a camera and a model to frame. It takes `view`, `camera`, `width`, `height`, `projection`, `mode`, `render_style` and `samples`. The camera has `target`, `yaw`, `pitch`, `distance`, `fov` and `projection`. The `target` is a point of the drawing in millimeters (drawing coordinates), the same frame as every other coordinate of the API and as the cameras of scenes (MOD-10). It does not depend on which other objects are in the drawing. Give the camera of an answer back as `camera` and the picture is the same, also after you add models far away. `render.png` takes `camera` too. The answer has the camera that was used.

Over MCP the picture comes as an image block.

## Limits

Models have the limits of `docs/modeler-limits.md`: 55,000 vertices, 150,000 edges, 100,000 faces and 10,000 edges in one edit. A refused edit gives `model_limit`.

## What the reads see

The reads and edits work on the surface model objects of the document, also the ones inside groups. A component instance shows its definition in renders, but the reads do not list its geometry. `mesh.summary` counts the instances in `component_instances`. To read or edit a component, explode the instance with `mesh.explode`. When `mesh.make_component` takes every face of a model, the geometry lives in the definition and the model is gone (`model_gone`).

## The objects in the app (MOD-13)

The Vectorworks-like app shows a surface model, a group of models and a component instance as an object. An agent finds them with `objects.list` and `objects.get`. Both give the object the field `surface`: the counts of vertices, edges and faces, `solid`, `area_mm2`, `bounds` (`min`, `max`, `size` in the drawing), the materials with the number of faces and `unpainted_faces`. These are the numbers that Object Info of the app shows.

`solid.state` is `closed` (with `solids` and `volume_mm3`), `open` (with `open_edges`), `crossing` (faces pass through each other, with `crossings`), `inward` (closed, but the fronts look in), `flat` (closed, but no volume), `empty` or `unchecked`. The check takes up to 3 s for a model at the limits, so one answer checks at most 20,000 faces (a list 60,000) and says `unchecked` for the rest. A model that was checked once is not checked again until its vertices, edges or faces change, so a second call is cheap and gives the rest.

`objects.list` leaves the geometry out of a surface model and says so in `shape.elided`, because a list of big models would be huge. It changes nothing else: an instance keeps its `symbol` and `transform`, and a group keeps its shape. `objects.get` has the geometry too. `objects.query` finds them with `Kind = "surface_model"` or `Kind = "component"`.

The plan simplifies a big model: over 20,000 faces or 30,000 edges it is the outline, and an outline of more than 20,000 lines is the box. Then `surface.plan` is `{detail: outline | box, faces, edges, limit_faces, limit_edges, limit_outline, note}`, with the same text as the note on the canvas of the app, in English. A model drawn in full has no `plan`. `render.svg` gives `notes` (the texts) and `simplified` (the objects with the same fields) when something was simplified. `render.png` in a hidden-line mode gives `notes` when an object with more than 50,000 edges was drawn by its outline, creases and borders only.

The usual methods work on these objects: `selection.set`, and the commands `move_objects`, `rotate_objects`, `copy_objects`, `delete_objects`, `transform_objects` and `transform_objects3d` of `execute`. Each is one undo step. Visibility by layer and class (`visibility.set`) applies to them, and a grey one cannot be selected. `render.svg` draws their plan. `snap.query` finds their vertices, edge middles and face centers.

## Where the code is

| Part | Place |
|---|---|
| Methods and `describe` text | `crates/kadakas-api/src/mesh_api/mod.rs` |
| Reads | `crates/kadakas-api/src/mesh_api/read.rs` |
| Near and pick | `crates/kadakas-api/src/mesh_api/query.rs` |
| Edits, batch and refusals | `crates/kadakas-api/src/mesh_api/edit.rs` |
| Parameter structs | `crates/kadakas-api/src/params/mesh.rs` |
| Tests | `crates/kadakas-api/tests/mesh.rs`, `python/tests/test_mesh.py` |
| Delete, selection parts, unknown methods (AGR-09) | `crates/kadakas-api/src/objects_api.rs`, `selection_api.rs`, `method_hints.rs`, `tests/agent_gaps.rs`, `tests/behavior/test_agent_gaps.py` |
| Selected parts of the 3D editor | `crates/kadakas-modeler/src/parts_sync.rs` |
| Camera round trip | `crates/kadakas-app/src/agent_view.rs`, `crates/kadakas-app/src/app/agent_view_tests.rs` |
| The pointer of the person (AGR-10) | `crates/kadakas-api/src/pointer_api.rs`, `crates/kadakas-app/src/agent_pointer.rs`, `crates/kadakas-api/tests/pointer.rs`, `crates/kadakas-api/tests/pointer_cost.rs`, `crates/kadakas-app/src/app/agent_view_tests.rs`, `tests/behavior/test_agent_view.py` |

After a change to a method, renew the Python client with `python python/tools/codegen.py`.
