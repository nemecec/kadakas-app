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

Edits. Each one is one undo step.

| Method | What it does |
|---|---|
| `mesh.create` | An empty model. |
| `mesh.add_edges` | Straight edges. Faces form and split by themselves. |
| `mesh.add_curve` | A chain that stays one curve, such as a circle. |
| `mesh.erase` | Erase edges and faces. |
| `mesh.reverse_faces` | Turn faces over. |
| `mesh.push_pull` | Push or pull a face. |
| `mesh.follow_me` | Sweep a face along edges. |
| `mesh.offset` | Offset a face or a chain of edges. |
| `mesh.intersect_faces` | Cut faces where they cross. |
| `mesh.transform` | Move, rotate or scale. |
| `mesh.copy` | Copy faces and edges in the model. |
| `mesh.paint` | Put a material on faces. |
| `mesh.hide` | Hide or show faces and edges. |
| `mesh.soften` | Set the soft and smooth flags of edges. |
| `mesh.soften_edges` | Soften every edge under an angle. It counts the edges that changed. |
| `mesh.solid` | Union, subtract, intersect, trim or split two solids. |
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
  "created": {"vertices": [], "edges": [], "faces": []},
  "removed": {"vertices": [], "edges": [], "faces": []},
  "notes": []
}
```

`created` and `removed` list at most 1000 ids each. If a list was cut, `truncated` is true. `notes` tell what the edit changed that you did not ask for. An example is a material that a merged face dropped.

`mesh.soften_edges` adds `changed`, `softened` and `considered`. `changed` is the number of edges that got other flags. `softened` is the number of soft edges in the model now. `considered` is the number of edges that the angle test looked at. Edges that do not have exactly two faces are not touched.

`mesh.make_group`, `mesh.make_component` and `mesh.explode` add `created_objects`, `removed_objects` and `created_resources`.

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

The caps come from `docs/modeler-limits.md`. A command on a model of 80,000 edges costs about 0.125 s. Fifty edits cost about 6 s. An edit of 10,000 edges costs 0.2 to 0.75 s. So a batch stays near 7 s in the worst case.

## Undo and review

Every edit method is one undo step. This includes `mesh.erase` with edges and faces, and `mesh.batch` with all its edits. An edit that is refused changes nothing and makes no step.

Edits of an agent are marked as agent changes. They show in `agent.activity` and in the Activity panel. The person undoes or reverts a step there. There is no review window of its own.

## Errors

A refusal is a validation error (code 3). A missing object, vertex, edge or face is `not_found` (code 2). A size limit is `limit` (code 6). A wrong parameter is `invalid_params` (code -32602).

The data of the error has `error_id`, a stable name. It also has the ids that matter. The message is English text for a person. A tool reads `error_id`, not the message.

| `error_id` | Meaning | Facts |
|---|---|---|
| `no_such_object` | The object does not exist. | `object` |
| `not_a_surface_model` | The object is another kind. | `object`, `kind` |
| `no_such_vertex`, `no_such_edge`, `no_such_face` | The id is not in the model. | `object`, `vertex`, `edge` or `face` |
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
| `solid_result_open` | The result is not a closed solid. | |
| `solid_result_volume` | The volume of the result does not fit. | |
| `solid_result_invalid` | The result makes the model invalid. | |
| `batch_too_large` | More than 50 edits. | `max`, `count` |
| `batch_edges_too_many` | More than 10,000 edges in all. | `max`, `count` |
| `command_refused` | A group or component command was refused. | `op` |

`operand` is `first` or `second`. `open_kind` is `border`, `non_manifold` or `reversed`. In a batch the data also has `step`.

## Reading in pages

`mesh.vertices`, `mesh.edges` and `mesh.faces` take `offset` and `limit`. The default `limit` is 100. The most is 1000. Give `ids` to read only some. Ids that do not exist are listed in `missing`.

## Questions about space

`mesh.near` takes a point in drawing coordinates and a radius. It returns faces, edges or vertices, nearest first. It leaves out hidden items unless `include_hidden` is true.

`mesh.pick` shoots a ray. The answer says which face was hit, the distance, the point, and the `side` that was met (`front` or `back`). With `all` it lists every face on the ray.

## Pictures

`mesh.render` is `render.png` with a camera and a model to frame. It takes `view`, `camera`, `width`, `height`, `projection`, `mode`, `render_style` and `samples`. The camera is the same as in a scene (MOD-10): `target`, `yaw`, `pitch`, `distance`, `fov` and `projection`. `render.png` takes `camera` too. The answer has the camera that was used, so a later call repeats it.

Over MCP the picture comes as an image block.

## Limits

Models have the limits of `docs/modeler-limits.md`: 50,000 vertices, 100,000 edges, 50,000 faces and 10,000 edges in one edit. A refused edit gives `model_limit`.

## Where the code is

| Part | Place |
|---|---|
| Methods and `describe` text | `crates/kadakas-api/src/mesh_api/mod.rs` |
| Reads | `crates/kadakas-api/src/mesh_api/read.rs` |
| Near and pick | `crates/kadakas-api/src/mesh_api/query.rs` |
| Edits, batch and refusals | `crates/kadakas-api/src/mesh_api/edit.rs` |
| Parameter structs | `crates/kadakas-api/src/params/mesh.rs` |
| Tests | `crates/kadakas-api/tests/mesh.rs`, `python/tests/test_mesh.py` |

After a change to a method, renew the Python client with `python python/tools/codegen.py`.
