# Modeler: inference and the Measurements box (MOD-03, MOD-04)

This page describes the headless logic of two rows of the 3D modeler (section 4.23 of `docs/feature-spec.md`). The code is in `kadakas-ui-core`: `infer3d` (MOD-03) and `measure_box` (MOD-04). It has no window, no GPU and no clock. The modeler program (MOD-01) calls it. The general part of the modeler is in `docs/modeler.md`.

The behavior follows the public description of the inference of SketchUp-like modelers (`docs/research/sketchup-public-features.md`, section 4). The code, the colors and the texts are our own.

## 1. Inference in 3D (`kadakas_ui_core::infer3d`)

### How the modeler calls it

1. **Scene.** When the geometry changes, build a `Scene` with `SceneBuilder`: `add_point`, `add_edge`, `add_face` (outer loop and holes), `add_surface_model(&SurfaceModel, model_number)`. A surface model gives its edges (not the ones with the attribute `hidden: true`), its faces, and the centers of its circles and arcs (a curve whose corners lie on one circle, so also a regular polygon that is a curve). `finish()` builds the spatial index. A plain face (`add_face`) gives no center, so a rectangle or a square from the rectangle tool has none; `add_polygon_face` gives the center of a regular face for a caller that has no curve. `add_guide(&Guide, layer_elevation, index)` adds a guide (see "Guides" below). `add_surface_model_placed_with` can offer hidden edges and faces too.
2. **Camera.** Each frame make a `Camera` with `Camera::look_at(eye, target, up, Projection, viewport_px)`. Perspective (`fov_y`) and parallel (`height`) work.
3. **Engine.** The active tool keeps one `Inferencer`. On a pointer move call `infer(&scene, &camera, pointer_px, reference)`. `reference` is the last point of the tool, or `None` before the first click.
4. **Draw.** The `Inference` has `point`, `kind`, `screen` (the pixel of the point), `color`, `label` (en-US or et, from `kadakas-l10n`), `lines` (cue lines, some dotted), `locked`, `source` and `other` (tags of the edge, face or curve), and `constraint` (the point, line or plane that holds the point).
5. **Keys and pause.** Forward `Key::Right`, `Left`, `Up`, `Down`, `ShiftDown`, `ShiftUp` to `Inferencer::key` (it returns true when the lock changed). When the pointer rested for `PAUSE_SECONDS` (0.6 s), call `pause()`. Call `reset()` when the tool starts or ends. Call `release_locks()` after a click when the next segment starts fresh.

### Several models, skipped geometry and threads

- **One scene per model.** Keep one `Scene` for each model and call `infer_scenes(&[&scene_a, &scene_b], &camera, pointer_px, reference, &skip)`. An edit rebuilds only the scene of its model. `infer` is the same call with one scene and nothing skipped. The result does not depend on the order of the scenes. Occlusion works across scenes: a face of one model hides the points of another.
- **Skipping.** `skip` is a `&dyn Fn(Tag) -> bool`. It gets the tag of every point, edge and face. When it returns true the item is not offered and does not hide anything. A tool that moves or pushes geometry skips the tags of that geometry (for a surface model, the tags with its model number: `|t| t.model() == moving`). Pass `&|_| false` for nothing.
- **Hidden geometry.** A point, a midpoint, a center, a crossing or a point on an edge that is behind a face is not offered. The test is a ray through the pixel of the candidate: when the first face on that ray is nearer than the candidate, the candidate is dropped. An axis line, a lock and the free point are not tested. The "on face" result is the first face along the pointer ray.
- **Threads.** `Scene` and `SceneBuilder` are `Send + Sync` and own their data. Build a scene on a worker thread and send it to the UI thread: `std::thread::spawn(move || Scene::from_surface_model(model_snapshot, number))`, or fill a `SceneBuilder` there and call `finish()`. The UI thread keeps drawing with the old scene until the new one arrives.

A `Tag` is a number that the caller chooses. For a surface model the tag says the model number, the kind (edge, face, curve) and the id (`Tag::edge`, `Tag::face`, `Tag::curve`, then `kind()`, `model()`, `id()`).

### Kinds, labels and colors

| Kind | Label | Color | What |
|---|---|---|---|
| Endpoint | Endpoint | green | an end of an edge or a point |
| Center | Center | teal | a circle or arc center, a regular face center |
| Midpoint | Midpoint | cyan | the middle of an edge |
| Intersection | Intersection | near black | two edges cross, an edge crosses a face, an axis line crosses an edge |
| OnEdge | On Edge | red | on an edge |
| GuidePoint | Guide Point | amber | a guide point |
| OnGuide | On Guide | amber | on a guide line |
| OnAxis(X, Y, Z) | On Red / Green / Blue Axis | red, green, blue | on a line along an axis from the reference point |
| FromPoint(axis) | From Point | axis color, dotted | on a line along an axis from the point that you paused on; where such a line crosses an axis line from the reference point, the crossing |
| Parallel | Parallel to Edge | magenta | parallel to the edge that you hovered |
| Perpendicular | Perpendicular to Edge | magenta | perpendicular to that edge, in the plane of the reference point and the edge |
| ThroughPoint | Through Point | magenta | only as a Shift lock: on the line from the reference point through the endpoint, midpoint or center that you locked |
| OnFace | On Face | blue | on a face |
| Free | none | gray | the ground plane `z = 0`, else a point along the pointer ray |

The marks use fixed colors. A theme can change them. The kinds stay the same.

### Priority

A smaller number wins. A kind wins only when it is inside its own pick radius.

| Priority | Kinds |
|---|---|
| 0 | Endpoint, GuidePoint |
| 1 | Center |
| 2 | Midpoint |
| 3 | Intersection (also an axis line that crosses an edge) |
| 4 | OnEdge, OnGuide |
| 5 | OnAxis, FromPoint, Parallel, Perpendicular, ThroughPoint |
| 6 | OnFace |
| 7 | Free |

Among the lines, the one with the smallest angle wins. At an equal angle the order is X, Y, Z, Parallel, Perpendicular, FromPoint. A line from the reference point and a dotted line from the paused point that cross near the pointer give the crossing.

Pick radii are in pixels (`Options`): endpoint 10, center 10, midpoint 9, intersection 9, on edge 6. A line inference holds when the pointer is within 4 degrees (on the screen) of the line and at least 6 pixels from its origin. `ui_scale` multiplies the pixel values for high-density displays.

Ties are broken the same way every time: the kind priority, the pixel distance, the depth, the coordinates of the point, then the tags. The order in which the geometry was added does not change the result (there is a property test).

### Guides (MOD-05)

A guide (`kadakas_core::guide`) is construction geometry. The scene takes it with `SceneBuilder::add_guide`. The tag of a guide has the kind `TagKind::Guide` and the number `index`, made by `Tag::guide(index)`. The modeler keeps the guides of the document in one scene of their own, after the scenes of the models.

- A guide point is a point to snap to. It is the kind `GuidePoint` and has the priority of an endpoint. At the same distance an endpoint wins.
- A finite guide line is an edge of the scene. It gives only `OnGuide`. Its ends and its middle are no points. An edge wins over a guide line at the same distance.
- A line without ends is not in the index. It is in a short list. Every query tests the lines one by one, so no box of the index grows to kilometers. The cost is in `docs/modeler-limits.md`.
- A line without ends reaches ten times the size of the scene on each side of its point.
- The least reach is `GUIDE_REACH` (2,000,000 mm, 2 km). `SceneBuilder::set_reach` sets it (`reach_for(extent)`). The modeler uses the largest coordinate of the models and the guides.
- A guide that crosses an edge or another guide gives an `Intersection`. An axis line from the reference point that crosses a guide gives an `Intersection` too.
- `Constraint::Line` of an `OnGuide` result holds the line of the guide, so Shift keeps the point on the guide.
- A tool can skip all guides with a `skip` closure that tests `TagKind::Guide`. The Select tool does this the other way round to pick a guide (`modeling::select::pick_guide`).
- The colors are fixed, as for the other kinds. Amber (`infer3d::GUIDE`) is used by no axis, no geometry and no other inference.

### Locks

- **Shift** (`ShiftDown`) keeps the current inference until `ShiftUp`: an axis, parallel or perpendicular line keeps its direction (from the current reference point). An on-edge result keeps the line of the edge. An on-face result keeps the plane of the face. A point result (an endpoint, midpoint or center) with a reference point locks the direction from the reference point through that point (`Through Point`); the pointer then moves along that line. Without a reference point a point result keeps the point. Nothing happens on `Free`.
- **Right** locks the red axis, **Left** the green axis, **Up** the blue axis. The same key again unlocks. A lock follows the reference point, so after a click it holds the new point.
- **Down** locks the edge that you hovered: first parallel, then perpendicular, then no lock. When the pointer is on a perpendicular inference, the first press locks that one.
- Under a line lock (an arrow key or Shift on a line), an endpoint, midpoint or center under the pointer gives its place on the locked line: the end of the perpendicular from the point to the line. A dotted line shows the link from the point to the line. Under the blue lock a corner gives the point of the blue line at the height of the corner.
- The arrow keys before the first click belong to the tool, not to the inference: Circle and Freehand use them to lock the plane they draw in. The tool handles them and calls `key` only after the first click.
- The parallel and perpendicular inferences work after the pointer was on an edge (on edge, endpoint or midpoint). An edge along an axis gives the axis, not a second inference.

### Not in v1

Tangent at vertex, extend edge, a through-point inference without Shift, square and golden section, half circle, the Alt or Command cycle of linear inference states, magenta marks inside groups and components, origin points, and hiding of points behind back-facing faces only (all faces hide). Perpendicular to a face is not done.

### Performance

`cargo test -p kadakas-ui-core --release --lib -- --ignored --nocapture perf_` builds a surface model of 160 by 160 vertices on a ramp (50,880 edges, 25,281 faces) and asks 3000 questions with the pointer over the model. Apple silicon, release build, one thread:

| View | Build of the scene | p50 | p99 | max |
|---|---|---|---|---|
| overview (about 6 pixels per grid cell) | 54 ms | 0.06 ms | 0.11 ms | 1.5 ms |
| close (a few cells under the pointer) | 54 ms | 0.05 ms | 0.12 ms | 0.23 ms |
| inside (eye level in the model, half of it behind the camera) | 54 ms | 0.10 ms | 0.18 ms | 0.25 ms |

The test fails when the worst p99 is over 4 ms (a quarter of a 60 Hz frame). The index is a bounding volume hierarchy for points, edges and faces. A query visits the boxes that touch the area of the pick radius around the pointer (a box is cut at the near plane, and a box fully behind the camera is skipped), and every candidate costs one ray query for the hidden-geometry test, so the cost follows the number of things near the pointer.

## 2. The Measurements box (`kadakas_ui_core::measure_box`)

`parse(text, &Units, Tool, Phase)` gives a `Value` or an `InputError`. `Units` is the `kadakas_units::Units` of the document: the length unit (a bare number counts in it) and the number format (the decimal separator). Lengths come out in millimeters and angles in degrees. `InputError::message()` gives the text in the language of the UI.

| Text | Value | Notes |
|---|---|---|
| `2.5`, `2.5m`, `250cm`, `3'6"`, `5' 6 1/2"` | `Length` | units from `kadakas-units` |
| `3;4`, `3,4`, `,4`, `3;` | `Pair(Option, Option)` | `;` always; `,` only with a decimal point. An empty side keeps the old value |
| `45`, `45°`, `45deg`, `8:12` | `Angle` | `8:12` is rise to run |
| `2`, `-1`, `1.5` | `Factor` | a scale factor; the bare number for Scale |
| `1;1;2`, `1,2,3` | `Triple` | three values: Move (lengths, relative) and Scale (factors); a value may be empty |
| `5x`, `x5`, `5*`, `*5` | `Count` | 1 to 1000 copies, only after a copy |
| `/5`, `5/` | `Divide` | 2 to 1000 parts, only after a copy |
| `12s` | `Sides` | 3 to 1000. A bare number before the first click of Circle, Polygon and the arcs is the sides too (`24`) |
| `24r` | `Radius` | a length |
| `[1,2,3]` | `PointAbs` | `;` between the values with a decimal comma |
| `<1,2,3>` | `PointRel` | relative to the start |

With a decimal comma (Estonian) `2,5` is a length of two and a half, and two values are written `3;4`. With a decimal point, a tool that takes a length and no pair reads `2,5` as two and a half too. In a setting with a decimal point, a tool that takes a length and no pair refuses `1,000` (digits, one comma, three digits) with the error `AmbiguousComma`: it could be a decimal comma or a group of thousands, and the box does not guess. `2,5` and `1,50` still read as a decimal. Where a pair is accepted (Rectangle) `1,000` is the pair 1 and 0. A bare number is a length, an angle, the sides or the first value of a pair, as the table below says.

### Which tool takes which kind

`accepts(tool, phase)` returns this table. `Phase::Step(n)` is while the tool runs (n clicks so far); `Phase::Done` is just after the operation ended; `Phase::Copied` is just after it ended and it was a copy (Move or Rotate with the copy key). The tool tells the box which one it is: `5x` and `/5` are only accepted after a copy. For the other tools `Copied` is the same as `Done`.

| Tool | While it runs | After it ended | Bare number | Negative |
|---|---|---|---|---|
| Line | length, point (absolute, relative) | the same | length | yes |
| Rectangle | pair (one length is the first value) | the same | first value | yes |
| Circle, Polygon | step 0 (no click): sides; later: length (radius), sides | the same as later | step 0: sides, later: length | no |
| Arc | step 0: sides; step 1 (center clicked): radius (length), sides; step 2 (start clicked): angle, sides | sides, radius | sides, then length, then angle; none after | angle yes |
| Two-point arc | step 0: sides; step 1 (start clicked): length (chord); step 2 (end clicked): length (bulge), radius, sides | length, radius, sides | sides, then length | no at step 1 |
| Move | length, three values (`x;y;z`), point | the same (after a copy: count, divide) | length | yes |
| Rotate | angle | angle (after a copy: count, divide) | angle | yes |
| Scale | factor, three factors (`1;1;2`), length | the same | factor | yes |
| Push/Pull, Offset, Tape measure | length | length | length | yes |
| Protractor | angle | angle | angle | yes |
| Select, Eraser, Freehand, Follow Me, Orbit, Pan, Zoom, Paint | nothing | nothing | none | none |

A valid value of a kind that the tool does not take gives `NotAccepted` with the kind and the list of kinds that the tool takes. A bare number is a factor for Scale (`2`, `-1`, `1.5`; a decimal comma that was typed by habit is a factor too); a number with a unit is a length (`2m`: the new size along the grip). Three values with a separator are `Triple`: the three lengths of a Move or the three factors of a Scale; a value may be empty (`;;5`). A tool that takes no triple answers three values with `TooManyValues`. Not in v1: the rotated rectangle.
