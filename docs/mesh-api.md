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
| `exact_stale` | `mesh3d.retessellate`: the triangles were edited after the import, so the exact data no longer fits. | `object` |

`operand` is `first` or `second`. `open_kind` is `border`, `non_manifold` or `reversed`. In a batch the data also has `step`.

## Circles and arcs (MOD-15)

A curve is a chain of straight edges that stays one circle or arc. A curve also keeps the exact circle that its corners lie on.
The circle has a kind (`circle`, `arc` or `polygon`), a center, a normal, an axis, a radius and, for an arc, a start and an end angle.
The axis is the direction of the angle 0. Angles run counterclockwise about the normal. The number of segments is the number of edges.
Coordinates are local, in millimeters and radians.

`mesh.add_curve` takes the circle as `analytic`. The core keeps it only when every point lies on the circle within the length tolerance.
Else the curve is drawn without it and the notes say so.

`mesh.curve_info` reads the curves of a model, or one curve with `curve`. Each item has `edges`, `segments`, `closed` and `geometry`.
`geometry` is null for an old curve. An arc also has `start_deg`, `end_deg` and `radians`.

```json
{"method": "mesh.curve_info", "params": {"id": "<model>", "curve": 20}}
{"total": 1, "offset": 0, "items": [{"curve": 20, "edges": [21, 22], "segments": 24, "closed": true,
  "geometry": {"kind": "circle", "center": [0, 0, 0], "normal": [0, 0, 1], "axis": [1, 0, 0],
               "radius": 300, "length_mm": 1884.96}}]}
```

`mesh.set_segments {id, curve, segments}` rebuilds the chain with 3 to 1000 edges on the same circle. It is one undo step.
The ends of an arc stay. The first vertex of a circle stays. Faces in the plane of the circle follow, with the new vertices.
The edges keep the flags of the first edge of the chain (soft, smooth, faceted).

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
| Group, component, explode | The circle follows the chain. It is fitted again where the chain lands. |

The file keeps the circles in the model field `curve_geometry`, a list of `[curve id, {description}]`. A file without it loads as before.
An older program keeps the field it does not know. A row whose curve is gone or whose points are off the circle is left out on load.

## Reading in pages

`mesh.vertices`, `mesh.edges` and `mesh.faces` take `offset` and `limit`. The default `limit` is 100. The most is 1000. Give `ids` to read only some. Ids that do not exist are listed in `missing`.

## Questions about space

`mesh.near` takes a point in drawing coordinates and a radius. It returns faces, edges or vertices, nearest first, at most `limit` of each kind (200 at the most). It leaves out hidden faces and edges and soft edges unless `include_hidden` is true. It leaves out the models on a hidden layer or class unless you name the model with `id`. A model whose faces cannot be cut into triangles is listed in `skipped` and not searched.

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

After a change to a method, renew the Python client with `python python/tools/codegen.py`.
