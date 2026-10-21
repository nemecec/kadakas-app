# The 3D modeler (`kadakas-modeler`)

`kadakas-modeler` is a second UI flavor of Kadakas (PLAT-11). It is a separate program. It has a large 3D view and the layout of familiar 3D modeling programs. It opens and saves the same `.kad` files as `kadakas-app`. This page describes MOD-01 (the program) and MOD-02 (navigation). The rows are in section 4.23 of `docs/feature-spec.md`.

## Start it

- The launcher lists it as a UI flavor: **Kadakas Modeler (SketchUp-like)**. The launcher finds the program `kadakas-modeler` in the version folder. It needs no `flavors.json`. The installers and the update archives bring the program.
- From a build folder: `cargo run -p kadakas-modeler -- model.kad`. The paths on the command line are files to open. The first one opens.
- The launcher passes its run id to the program. The program holds the lock of the run, writes the alive stamp every second and writes the clean-exit mark when it closes. A double click on a `.kad` file while the modeler runs reaches the running program: the launcher sends the file, and the modeler opens it like File > Open (twice a second at most). A crash leaves a crash file, like in the app (PLAT-20). The report names the program (`kadakas-modeler`). The next start of `kadakas-app` offers the report.
- On macOS the program is inside `Kadakas Engine.app`, next to `kadakas-app`.

## The window

| Part | What it has |
|---|---|
| Menu bar | File, Edit, Camera, Tools, View, Help. Each item shows its key. |
| Tool bar | The Getting Started set: Select, Eraser, Line, Rectangle, Circle, Arc, Push/Pull, Move, Rotate, Scale, Tape Measure, Paint Bucket, Orbit, Pan, Zoom, Zoom Extents. |
| 3D view | The scene, drawn by the same GPU renderer as the app. The drawing axes are red (X), green (Y) and blue (up). |
| Status bar | The instruction for the active tool, or the last message. At the right is the **Measurements** box. |
| Tray | **Entity Info**, **Materials** and **Scenes**. A click on a title bar collapses the panel. View > Tray hides the tray. |

Select, the drawing tools of MOD-08 (Eraser, Line, Rectangle, Circle, Arc, Push/Pull) and the navigation tools run. The other tools (Move, Rotate, Scale, Tape Measure, Paint Bucket) are in the tool bar, disabled. Their tooltip says "coming in a later version". Their keys show the same note in the status bar. The rows MOD-05 and the rest of MOD-08 turn them on.

**Measurements box.** It shows the size of the selection (length, width and height in the units of the person), the field of view while the Zoom tool is active, and the live value of a drawing tool that runs (length, width and height, radius, bulge, distance). You type into it without a click: digits and the marks of values start the text, letters follow once the text has started (`24s`, `500r`, `3'6"`), Backspace erases, Enter applies and Esc clears. See "Modeling" below.

**Entity Info.** It shows the type, the id, the layer, the class, the size and the lowest corner of the selected object. For several objects it shows the count and the size of the box around them. With no selection it shows the active layer and class. A surface model also shows its numbers of vertices, edges and faces and whether the faces close a volume. A selected face shows its area, normal, edges and holes. A selected edge shows its length, ends, curve and the number of faces on it. Several faces and edges show their counts. **Materials** and **Scenes** are placeholders. Paint comes with MOD-05 and scenes with MOD-10.

**Unsaved changes.** The session knows when the model has changes that nobody saved. File > New, File > Open, a file from the launcher, File > Quit and the close button then ask the same three questions as the app: Save and open, Don't save, or Cancel. Only one question is open at a time. A second request is dropped, and the status line says so. A drawing tool or an undo makes unsaved changes.

**Language and theme.** The program has en-US and Estonian. View > Language changes the language at once. View > Theme gives System, Light and Dark. Both are kept in `modeler.json` in the config folder (`KADAKAS_PREFS_DIR` names another folder). The units come from the `prefs.json` of the app. The modeler reads that file and never writes it.

## Navigation (MOD-02)

| Do | How |
|---|---|
| Orbit | Drag with the middle button, in every tool. Or choose the Orbit tool (`O`) and drag. The view turns about the point of the model under the pointer when the drag starts. |
| Pan | Shift and drag with the middle button. The point under the pointer follows the pointer exactly. Or choose the Pan tool (`H`) and drag. Or hold Shift with the Orbit tool. The arrow keys pan too. |
| Zoom | The wheel zooms to the pointer. In perspective the point of the model under the pointer stays under it, and the zoom goes on until the eye is a hundredth of a millimeter from that point. The Zoom tool (`Z`) zooms with a drag, up zooms in. Cmd/Ctrl+1 and Cmd/Ctrl+2 zoom in and out by a factor of 2. |
| Zoom Extents | `Shift+Z`, the last tool of the tool bar, or Camera > Zoom Extents. |
| Zoom Window | `Cmd/Ctrl+Shift+W` or Camera > Zoom Window, then drag a window. Or hold Cmd/Ctrl with the Zoom tool. |
| Standard views | Camera > Standard Views: Top, Bottom, Front, Back, Left, Right and Iso. Each view shows the whole model. |
| Projection | Camera > Perspective or Parallel Projection. The picture does not jump: the plane through the target keeps its size and its place. |
| Field of view | Camera > Field of View (1 to 120 degrees, 35 by default), or Shift and a drag with the Zoom tool. The view keeps its size at the target. The parallel projection has no field of view: the control is disabled and the drag does nothing. |
| Previous and next view | Camera > Previous and Next. A change counts as one view when the view has rested for 0.4 seconds, so a wheel turn is one entry. A new view after going back removes the views that were ahead. |
| Select | Space, or Esc. Click an object to select it. Shift adds or removes an object. |

The keys come from the **SketchUp-like** profile (`docs/keyboard-shortcuts.md`). The profile is in `crates/kadakas-ui-core/profiles/sketchup.json`. The modeler has no editor for the keys yet. A user file `keymap.json` does not apply to the modeler.

Walk, Look Around and Position Camera come in a later step of MOD-02. Two-point perspective is not planned.

## How it is built

The logic is in `kadakas-ui-core`. The program only reads the pointer and the keys, and draws.

| Part | Where | What |
|---|---|---|
| Camera math | `kadakas_ui_core::nav3d` | Orbit (also about a pivot), pan, zoom to the cursor, Zoom Extents, Zoom Window, standard views, projection, field of view, the history of views, and which drag does what (`drag_for`). Tested without a window. |
| Tool bar and Entity Info | `kadakas_ui_core::modeler` | The 16 tools, which of them run, the active tool, the text for the Measurements box and the data of Entity Info. |
| Keys | `kadakas_ui_core::keymap` and `profile` | New actions with no key in the Vectorworks-like profile (`Action::modeler_only`), and the profile `sketchup`. It is not in `Profile::builtins()`, so the app does not list it. |
| 3D drawing | `kadakas-view3d` | The GPU renderer, the vertex types and the view caches. They moved out of `kadakas-app` without a change of behavior. The app and the modeler use the same code. |
| Camera | `kadakas_render::Camera` | It got a field of view (`fov_y`). Its default is the old constant, so the app draws as before. |
| Files | `kadakas_api::files::file_call` | The call behind `file.open` and `file.save`, with the same questions to the person as the app: a big file, a file that another program changed, parts of the file that are missing. |
| The program | `crates/kadakas-modeler` | Window, menu, tool bar, tray, status bar, icons, settings and the handshake with the launcher. |

The tool bar icons are our own drawings from written ideas, in `assets/icons/modeler/`. They follow the style rules of `docs/icons.md`. Nothing is traced or copied.

## Modeling (MOD-08, first part)

The code that decides is in `kadakas_ui_core::modeling` (no window, no GPU, no clock). The modeler only feeds it the pointer and the keys, runs what it returns and draws. `crates/kadakas-modeler/src/edit3d.rs` is the glue.

### Look and selection

- A surface model is drawn by the scene builder of `kadakas-core`: the faces as triangles and every edge (not the ones with the attribute `hidden`) as a line. A face shows its **front** in a light color and its **back** in a bluish color (a flag on the vertices of the mesh, `Mesh::two_sided`; the GPU, the CPU renderer and so the exports all draw the back in the back color `SURFACE_BACK`. A face with a material shows the material on both sides.) Edges with the attribute `smooth` (the sides of a cylinder) are not drawn as lines.
- **Select** (Space): a click picks the face or edge under the pointer (an edge within a few pixels wins); Shift adds or removes. A double click on a face selects it with its edges, a double click on an edge selects it with its faces. A triple click selects everything connected to what was hit (through shared vertices, not the whole model). A click on nothing that is a part falls back to the selection of whole objects. A drag is a selection rectangle: left to right is a **window** (parts that are fully inside), right to left is a **crossing** (parts that it touches, drawn dashed). Selected edges are thick blue; a selected face shows blue hatch lines (they are depth tested). Delete erases the selected faces and then the selected edges in one step, or the selected objects.
- Faces and edges of a model are never selected together with whole objects.

### Tools

Each finished action is one `edit_surface` command and one undo step. A new model is made by the same step as its first edge. The geometry goes into the model under the first click, else the selected model, else the model that was drawn in last, else the first model of the document, else a new one.

| Tool (key) | Clicks | Typed in the box |
|---|---|---|
| Line (L) | start, then each next point; a click on the start closes the loop and ends; Esc ends | a length along the pointer, a point `[x,y,z]`, a relative point `<x,y,z>`; a negative length goes the other way |
| Rectangle (R) | two corners on the face under the first click, else on the horizontal plane; the sides follow the axes of the face | `width;height` (`;` with a decimal comma), `,4` keeps the width |
| Circle (C) | center, then a point | a radius; `24s` for the number of sides (the default is 24) |
| Arc (A), two-point arc | start and end of the chord, then the bulge | a chord length, then a bulge or a radius (`500r`); `12s` for the segments (default 12) |
| Eraser (E) | click an edge, or drag over edges: they turn red and go on release | nothing |
| Push/Pull (P) | click a face, move along its normal, click; a double click on a face repeats the last distance | a signed distance |

A circle or an arc is made by `edit_surface` with the action `add_curve`: a chain of edges that stays **one curve**, also after other edges cut it. The Eraser removes the whole curve. Pushing or pulling a circle copies the curve to the cap, and the new edges between the pieces get the attribute `smooth`.

**Typing after a tool ended.** Right after a Rectangle, Circle, Arc or Push/Pull a new value redoes it: the app takes back that step and makes the new one (still one undo step). This only works while nothing else changed the document.

**Push/Pull in the core** (`edit_surface` with `push_pull {face, distance}`) decides for each edge of the face. An edge shared with one wall at a right angle is stretched: the wall follows the edge (and gets a step where the face is split from a neighbor). "At a right angle" is a distance test: the moved edge must stay within the plane tolerance of the wall, so a wall that leans a little stretches for a short push and gets a new wall beside it for a long one. Any other edge gets a new wall. A lone face becomes a closed solid; a face on other faces (a square drawn on a wall, the half of a split top) becomes a bump, or a pocket for a push, and the opening stays. The face keeps its id and is the cap. When the new vertical edge would lie on an edge of the model (a top split from corner to corner and pushed down, so the vertical runs down the corner of the box), the two merge as in SketchUp: the corner edge is split at the new floor. A stretched edge that is replaced keeps its attributes and its place in a curve. The distance you type is in world mm; the tool turns it into the units of a scaled model. **Refused:** a push that reaches the opposite face of a slab (a hole through); a push into a wall that shares an edge of the face with another face (a fin: the new wall would lie on that wall); and any result in which new or moved geometry passes through, ends on or lies in another face (also a stretched wall that would cut a plate), or that has a model problem the model did not have before (compared as a set; the error names the new problem). These checks run on the touched geometry also when the model had a problem before. The error says why and nothing changes; the hole through is a later step. The model check (`check`) now also reports an edge that passes through a face and two coplanar faces that overlap. On a model of 50 000 edges a push takes about 150 ms and a full check about 250 ms (release build).

### Inference and the scene

The tools use `infer3d` (see `docs/modeler-inference.md`) with the last point of the tool as reference. Shift keeps the inference and the arrow keys lock an axis while a tool runs (so the arrow keys do not pan then). When the pointer rests, the point becomes the start of the dotted axis lines. Points are drawn as squares, lines and faces as dots, cue lines in their colors, and the label next to the pointer.

The inference scene (`modeling::SceneCache`) is built once per document revision, never every frame. It costs about 30 ms (debug build) for a model of 1,600 faces and 3,280 edges (`the_scene_build_cost_is_measured_and_stays_small`); one inference takes about 0.1 ms. There is one scene for each model (`Scene::from_surface_model`): a model whose revision and placement did not change keeps its scene, so an edit builds only the scene of the model it changed. The inference runs over all of them (`infer_scenes`) and skips the face or edges that the tool works on (`Modeling::excluded`), so a pulled face does not snap to itself. The scenes are built on the UI thread today (30 ms for 1,600 faces); `Scene` is `Send`, so a worker thread can take it over later.

When the model grows, the middle of the plan (the scene origin) moves. The window moves the camera target by the same amount, so the view does not jump.

## Surface models (MOD-06)

A surface model (`Shape::SurfaceModel`, MOD-06) is drawn by the same GPU renderer as the other objects. See `docs/feature-spec.md` for what the core does.

## Clean room

The layout, the tool names, the logic of the tools and the default keys follow public documentation of programs of this kind (`docs/research/sketchup-public-features.md`). No program of that kind was downloaded or run. No icon, image or text was copied. SketchUp is a trademark of Trimble Inc. The About window says that Kadakas Modeler is not affiliated with it.

## Not done yet

- A window of the web build. The program is for the desktop. The 3D view and the tray need a native window and a file dialog.
- The tools Move, Rotate, Scale, Tape Measure and Paint Bucket (MOD-05), and the rest of MOD-08: Polygon, Follow Me, Offset, Intersect, soft and smooth edges, and the hole through a slab for Push/Pull.
- Not tried in a real window: the drawing, the marks and the hatch on a GPU window were checked in headless frames and in one GPU test of the back color, not by hand.
- An editor for the keys of the modeler, and a review window for crash reports. A report stays in the queue until `kadakas-app` starts.
- Autosave and the agent server of the app.
