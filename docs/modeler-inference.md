# Modeler: inference and the Measurements box (MOD-03, MOD-04)

This page describes the headless logic of two rows of the 3D modeler (section 4.23 of `docs/feature-spec.md`). The code is in `kadakas-ui-core`: `infer3d` (MOD-03) and `measure_box` (MOD-04). It has no window, no GPU and no clock. The modeler program (MOD-01) calls it. The general part of the modeler is in `docs/modeler.md`.

The behavior follows the public description of the inference of SketchUp-like modelers (`docs/research/sketchup-public-features.md`, section 4). The code, the colors and the texts are our own.

## 1. Inference in 3D (`kadakas_ui_core::infer3d`)

### How the modeler calls it

1. **Scene.** When the geometry changes, build a `Scene` with `SceneBuilder`: `add_point`, `add_edge`, `add_face` (outer loop and holes), `add_surface_model(&SurfaceModel, model_number)`. A surface model gives its edges (not the ones with the attribute `hidden: true`), its faces, the centers of its circles and arcs (a curve whose corners lie on one circle) and the centers of its regular faces. `finish()` builds the spatial index.
2. **Camera.** Each frame make a `Camera` with `Camera::look_at(eye, target, up, Projection, viewport_px)`. Perspective (`fov_y`) and parallel (`height`) work.
3. **Engine.** The active tool keeps one `Inferencer`. On a pointer move call `infer(&scene, &camera, pointer_px, reference)`. `reference` is the last point of the tool, or `None` before the first click.
4. **Draw.** The `Inference` has `point`, `kind`, `screen` (the pixel of the point), `color`, `label` (en-US or et, from `kadakas-l10n`), `lines` (cue lines, some dotted), `locked`, `source` and `other` (tags of the edge, face or curve), and `constraint` (the point, line or plane that holds the point).
5. **Keys and pause.** Forward `Key::Right`, `Left`, `Up`, `Down`, `ShiftDown`, `ShiftUp` to `Inferencer::key` (it returns true when the lock changed). When the pointer rested for `PAUSE_SECONDS` (0.6 s), call `pause()`. Call `reset()` when the tool starts or ends. Call `release_locks()` after a click when the next segment starts fresh.

A `Tag` is a number that the caller chooses. For a surface model the tag says the model number, the kind (edge, face, curve) and the id (`Tag::edge`, `Tag::face`, `Tag::curve`, then `kind()`, `model()`, `id()`).

### Kinds, labels and colors

| Kind | Label | Color | What |
|---|---|---|---|
| Endpoint | Endpoint | green | an end of an edge or a point |
| Center | Center | teal | a circle or arc center, a regular face center |
| Midpoint | Midpoint | cyan | the middle of an edge |
| Intersection | Intersection | near black | two edges cross, an edge crosses a face, an axis line crosses an edge |
| OnEdge | On Edge | red | on an edge |
| OnAxis(X, Y, Z) | On Red / Green / Blue Axis | red, green, blue | on a line along an axis from the reference point |
| FromPoint(axis) | From Point | axis color, dotted | on a line along an axis from the point that you paused on; where such a line crosses an axis line from the reference point, the crossing |
| Parallel | Parallel to Edge | magenta | parallel to the edge that you hovered |
| Perpendicular | Perpendicular to Edge | magenta | perpendicular to that edge, in the plane of the reference point and the edge |
| OnFace | On Face | blue | on a face |
| Free | none | gray | the ground plane `z = 0`, else a point along the pointer ray |

The marks use fixed colors. A theme can change them. The kinds stay the same.

### Priority

A smaller number wins. A kind wins only when it is inside its own pick radius.

| Priority | Kinds |
|---|---|
| 0 | Endpoint |
| 1 | Center |
| 2 | Midpoint |
| 3 | Intersection (also an axis line that crosses an edge) |
| 4 | OnEdge |
| 5 | OnAxis, FromPoint, Parallel, Perpendicular |
| 6 | OnFace |
| 7 | Free |

Among the lines, the one with the smallest angle wins. At an equal angle the order is X, Y, Z, Parallel, Perpendicular, FromPoint. A line from the reference point and a dotted line from the paused point that cross near the pointer give the crossing.

Pick radii are in pixels (`Options`): endpoint 10, center 10, midpoint 9, intersection 9, on edge 6. A line inference holds when the pointer is within 4 degrees (on the screen) of the line and at least 6 pixels from its origin. `ui_scale` multiplies the pixel values for high-density displays.

Ties are broken the same way every time: the kind priority, the pixel distance, the depth, the coordinates of the point, then the tags. The order in which the geometry was added does not change the result (there is a property test).

### Locks

- **Shift** (`ShiftDown`) keeps the current inference until `ShiftUp`: an axis, parallel or perpendicular line keeps its direction (from the current reference point). An on-edge result keeps the line of the edge. An on-face result keeps the plane of the face. A point result keeps the point. Nothing happens on `Free`.
- **Right** locks the red axis, **Left** the green axis, **Up** the blue axis. The same key again unlocks. A lock follows the reference point, so after a click it holds the new point.
- **Down** locks the edge that you hovered: first parallel, then perpendicular, then no lock. When the pointer is on a perpendicular inference, the first press locks that one.
- The parallel and perpendicular inferences work after the pointer was on an edge (on edge, endpoint or midpoint). An edge along an axis gives the axis, not a second inference.

### Not in v1

Tangent at vertex, extend edge, through point, square and golden section, half circle, the Alt or Command cycle of linear inference states, magenta marks inside groups and components, origin points, guides, and hiding of points behind faces (the modeler can pass only visible geometry). Perpendicular to a face is not done.

### Performance

`cargo test -p kadakas-ui-core --release --lib -- --ignored --nocapture perf_` builds a surface model of 160 by 160 vertices on a ramp (50,880 edges, 25,281 faces) and asks 3000 questions with the pointer over the model. Apple silicon, release build, one thread:

| View | Build of the scene | p50 | p99 | max |
|---|---|---|---|---|
| overview (about 6 pixels per grid cell) | 65 ms | 0.04 ms | 0.09 ms | 1.3 ms |
| close (a few cells under the pointer) | 65 ms | 0.43 ms | 0.72 ms | 3.3 ms |

The test fails when the worst p99 is over 4 ms (a quarter of a 60 Hz frame). The index is a bounding volume hierarchy for points, edges and faces. A query visits the boxes that touch the area of the pick radius around the pointer, so the cost follows the number of things near the pointer.

## 2. The Measurements box (`kadakas_ui_core::measure_box`)

`parse(text, &Units, Tool, Phase)` gives a `Value` or an `InputError`. `Units` is the `kadakas_units::Units` of the document: the length unit (a bare number counts in it) and the number format (the decimal separator). Lengths come out in millimeters and angles in degrees. `InputError::message()` gives the text in the language of the UI.

| Text | Value | Notes |
|---|---|---|
| `2.5`, `2.5m`, `250cm`, `3'6"`, `5' 6 1/2"` | `Length` | units from `kadakas-units` |
| `3;4`, `3,4`, `,4`, `3;` | `Pair(Option, Option)` | `;` always; `,` only with a decimal point. An empty side keeps the old value |
| `45`, `45°`, `45deg`, `8:12` | `Angle` | `8:12` is rise to run |
| `5x`, `x5`, `5*`, `*5` | `Count` | 1 to 1000 copies |
| `/5`, `5/` | `Divide` | 2 to 1000 parts |
| `12s` | `Sides` | 3 to 1000 |
| `24r` | `Radius` | a length |
| `[1,2,3]` | `PointAbs` | `;` between the values with a decimal comma |
| `<1,2,3>` | `PointRel` | relative to the start |

With a decimal comma (Estonian) `2,5` is a length of two and a half, and two values are written `3;4`. With a decimal point, a tool that takes a length and no pair reads `2,5` as two and a half too. A bare number is a length, an angle or the first value of a pair, as the table below says.

### Which tool takes which kind

`accepts(tool, phase)` returns this table. `Phase::Step(n)` is while the tool runs (n clicks so far); `Phase::Done` is just after the operation ended.

| Tool | While it runs | After it ended | Bare number | Negative |
|---|---|---|---|---|
| Line | length, point (absolute, relative) | the same | length | yes |
| Rectangle | pair (one length is the first value) | the same | first value | yes |
| Circle, Polygon | length (radius), sides | the same | length | no |
| Arc | step 0: radius, sides; later: angle, sides | sides, radius | length, then angle | angle yes |
| Two-point arc | step 0: length; later: length (bulge), radius, sides | length, radius, sides | length | no at step 0 |
| Move | length, point | length, point, count, divide | length | yes |
| Rotate | angle | angle, count, divide | angle | yes |
| Push/Pull, Offset, Tape measure | length | length | length | yes |
| Protractor | angle | angle | angle | yes |
| Select, Eraser, Freehand, Follow Me, Orbit, Pan, Zoom, Paint | nothing | nothing | none | none |

A valid value of a kind that the tool does not take gives `NotAccepted` with the kind and the list of kinds that the tool takes. Not in v1: a scale factor and three values for Scale, the rotated rectangle.
