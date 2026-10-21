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

**Entity Info.** It shows the type, the id, the layer, the class, the size and the lowest corner of the selected object. For several objects it shows the count and the size of the box around them. With no selection it shows the active layer and class. A surface model also shows its numbers of vertices, edges and faces and whether the faces close a volume. A selected face shows its area, normal, edges and holes. After a merge (Erase, or a flush Push/Pull), the face that stays has the lower id and its own attributes and material. It also has the attributes that it did not have from the merged faces. The status line names what was taken or dropped. A selected edge shows its length, ends, curve and the number of faces on it. Several faces and edges show their counts. **Materials** and **Scenes** are placeholders. Paint comes with MOD-05 and scenes with MOD-10.

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

**Push/Pull in the core** (`edit_surface` with `push_pull {face, distance}`) decides for each edge of the face. An edge shared with one wall at a right angle is stretched: the wall follows the edge (and gets a step where the face is split from a neighbor). "At a right angle" is a distance test: the moved edge must stay within the plane tolerance of the wall, so a wall that leans a little stretches for a short push and gets a new wall beside it for a long one. Any other edge gets a new wall. A lone face becomes a closed solid; a face on other faces (a square drawn on a wall, the half of a split top) becomes a bump, or a pocket for a push, and the opening stays. The face keeps its id and is the cap. When the new vertical edge would lie on an edge of the model (a top split from corner to corner and pushed down, so the vertical runs down the corner of the box), the two merge as in SketchUp: the corner edge is split at the new floor. A stretched edge that is replaced keeps its attributes and its place in a curve. **Flush:** when the face lands on the geometry along its normal (a bump pushed back down, a pocket floor pulled back up to the top, the second half of a split top pushed to the level of the first), the walls between collapse, the vertices that meet are welded, edges that fall together become one (the older id stays; edge attributes that the weld drops give a note), and the face merges with the face of the same plane that it now touches, by the rules of Erase (the face with the lower id stays, so a pocket pulled back up gives the original top again). A cap vertex (or the copy of a vertex that stays) also welds when it lands on a vertex or inside an edge of a face in the plane of the cap. The edge is split there first (a welded T-junction). Example: a square at the foot of a step, pulled up to the high side, merges with the high side, and the part of the step wall beside it goes. A wall whose middle collapses is cut into the parts that keep an area. A push that lands within the length tolerance (0.01 mm) of such geometry goes exactly there, so the cap stays in one plane. A push that ends farther away leaves a thin wall (a wall of 0.02 mm is valid). Which ids stay: the vertex that was there stays, and the moving vertex or copy goes. Of two edges that fall together, the lower id stays. Of two faces that merge, the lower id stays. A circle that lands on its own ring gives its place in a curve to the ring (an edge is never in two curves). The ring then goes with the edges that the merge erases. **Attributes of a merge** (Erase and a flush): the id that stays is the lowest. The look is that of the merged face with the largest area (a tie goes to the lower id): its attributes, a material too, become those of the new face, whole. Nothing is taken from the smaller faces. So a small face never changes how a large one looks: a bump with a material, pushed flat, does not paint the top, and the material of the bump is dropped. Each value of a smaller face that the larger face does not have, or has with another value, is dropped with a note in the result of the command (`notes`, in the JSON of `execute`). The modeler shows the note in the status line. So the edit never drops a material silently. It is close to the rule for faces that land on one face when models merge (MOD-07), where the larger area also decides. SketchUp has no documented rule for this (forum posts disagree), so this one is our own choice. A face that collapses with attributes, and an edge attribute that a weld drops, give a note too. A push that passes a vertex along the normal (the top pushed past a line across a wall) goes in steps: to the line, then on. A face that merged on the way stops there, and a push past it is refused ("a push past that is a later step"). This is by design. Example: a pocket floor with a bump on it, pulled to the top, is refused at the height of the bump. The person pulls to the bump (the two faces merge), then on to the top. The distance you type is in world mm; the tool turns it into the units of a scaled model. **Refused:** a push that reaches the opposite face of a slab (a hole through: the face would become flush with a face that looks the other way, beside it or under it, or a copy would end on it); a push into a wall that shares an edge of the face with another face (a fin: the new wall would lie on that wall); and any result in which new or moved geometry passes through, ends on or lies in another face (also a stretched wall that would cut a plate), or that has a model problem the model did not have before (the error names the new problem). A contact or a problem counts as old only when the model had one of the same kind at the same place, so an old contact on a split-off part counts as old, and an old problem that moved counts as new. These checks run on the touched geometry also when the model had a problem before. The error says why and nothing changes; the hole through is a later step. The model check (`check`) now also reports an edge that passes through a face, an edge of a face that lies inside a coplanar face, and two coplanar faces that overlap (also in part; faces that only touch along an edge or at a vertex do not). The sides count. Two faces of one plane that share an area and look the same way overlap. That is an error (also in the fast check, at a shared edge). Two faces that look opposite ways lie back to back (a box that stands on a slab, with its own vertices). That is valid. It goes to the warnings of the report (`BackToBack`), and the solid tools (MOD-09) can use it as an inner face. A hole of a face is not an area of the face. A face inside the hole overlaps nothing. A face that covers the hole and the material around it overlaps, and its edges show it. A face is tested through an index of its sides, so a face with thousands of holes costs `O(log n)` per test. On a model of 50 000 edges a push takes about 150 to 650 ms and a full check about 250 to 400 ms (release build; the high end is a slab top with 4,200 bumps).

### Faces that cross, and Intersect Faces

**What is accepted.** Drawing does not cut faces that cross in other planes. An edge that passes through a face in another plane is accepted and is not split there. Other modelers of this kind work the same way (public documentation: edges and faces that cross are not split automatically). A sketch can hold such places.

**The warning.** The model check (`check`) reports each such place as the warning `NotIntersected` in `warnings`, not as an error. The warning names the edge, the face and the point (in mm, to the µm). The fast checks do not look at it, so the two checks agree. Two faces that cross always have an edge that passes through the other face. So one kind of warning covers both cases. A closed solid must not hold such a place: while one is left, `is_closed` is false (the solid tools of MOD-09 use it). Faces of one plane that overlap stay an error. Entity Info shows the count ("Not intersected: 6 places, use Intersect Faces").

**The tool.** Edit > Intersect Faces has two items. The context menu of the 3D view (the secondary button) has the same items.

- **With Model**: the whole model. It works on the models of the selected faces and edges, or on the selected models. With nothing selected, it works on every surface model of the open group or level.
- **With Selection**: the selected faces against the rest of their model (new edges where they cross anything), and the selected edges where they pass through a face.

In the core the tool is `edit_surface` with `intersect_faces {scope: "model" | "selection", faces, edges}`. It is one undo step for all models. The steps:

1. A box tree of the faces finds the pairs of faces in two planes whose boxes meet.
2. For each pair, the sides of both faces cut the line where the two planes meet. A large face uses a box tree of its sides.
3. The pieces of the line that lie in both faces become new edges. A hole is no part of its face. A border counts.
4. The pieces are drawn as `add_edges` draws them, with the same tolerances. An end on an edge splits the edge. The pieces weld to vertices and split at crossings. Both planes are solved again, so both faces split. Unlike drawing, a region in no face gets no new face.

A piece is left out when an edge that both faces use already covers it. So a second run changes nothing, and the same model always gives the same ids. An edge along the line that only one face uses, or no face (a line, a guide), does not cover the piece: the piece is drawn, and the other face splits there. A line with no face that passes through a face is split at that point. Its two parts stand on the face, and a note says so. An edge of a face is split only by the end of a new piece on it. The parts of a split face keep the attributes of the face. Split edges keep their attributes and their place in a curve, and the curve stays one chain. Each run gives a note for the status line ("Intersect Faces: 12 new edges, 6 new faces"). A piece that would be shorter than the length tolerance next to other geometry is left out whole: before anything is drawn, a test without changes (`segment_fits`) decides. So no edge is split for it, the crossing stays a `NotIntersected` warning, and notes name the piece and the place.

Measurement (release build, `surface::tests_intersect::intersect_timing`): 840 boxes stand through one slab. The model has 5,046 faces and 6,720 places where an edge passes through a face. The check takes 0.05 s. One run on the whole model would make 13,440 new edges, more than the limit, so it is refused. Three runs With Selection (the walls of a third of the boxes each) take 0.72 s together and make 13,440 edges and 8,400 faces. No warning is left. A run on the whole model after them takes 0.28 s and changes nothing.

**Push/Pull stays strict.** When new or moved geometry passes through, ends on or lies in a face, Push/Pull refuses the push. This includes a new `NotIntersected` place. A place that was there before the push, at the same place, does not stop it.

**Not done yet.**

- With Context. The geometry of two surface models (for example, one in a group and one outside it) is not intersected. Each model is intersected alone. A later step can merge the models into the raw model first.
- Coplanar faces that overlap. They are an error of the model, and the tool leaves them.
- More than 10,000 new edges in one run (`MAX_EDIT_EDGES`), the edges of the splits included. Such a run is refused and changes nothing. With Selection can do the work in parts.

**Curves after Erase.** When an erase takes edges out of a curve, the rest is cut into chains. A circle that loses one edge stays one arc. When it loses two edges that do not meet, two arcs stay.

### Inference and the scene

The tools use `infer3d` (see `docs/modeler-inference.md`) with the last point of the tool as reference. Shift keeps the inference and the arrow keys lock an axis while a tool runs (so the arrow keys do not pan then). When the pointer rests, the point becomes the start of the dotted axis lines. Points are drawn as squares, lines and faces as dots, cue lines in their colors, and the label next to the pointer.

The inference scene (`modeling::SceneCache`) is built once per document revision, never every frame. It costs about 30 ms (debug build) for a model of 1,600 faces and 3,280 edges (`the_scene_build_cost_is_measured_and_stays_small`); one inference takes about 0.1 ms. There is one scene for each model (`Scene::from_surface_model`): a model whose revision and placement did not change keeps its scene, so an edit builds only the scene of the model it changed. The inference runs over all of them (`infer_scenes`) and skips the face or edges that the tool works on (`Modeling::excluded`), so a pulled face does not snap to itself. The scenes are built on the UI thread today (30 ms for 1,600 faces); `Scene` is `Send`, so a worker thread can take it over later.

When the model grows, the middle of the plan (the scene origin) moves. The window moves the camera target by the same amount, so the view does not jump.

## Surface models (MOD-06)

A surface model (`Shape::SurfaceModel`, MOD-06) is drawn by the same GPU renderer as the other objects. See `docs/feature-spec.md` for what the core does.

### Performance of add_edges

Before, each new edge was compared with every vertex, edge and face of the model. So an edit of `k` edges in a model of `E` edges took about `1.4e-7 × k × E` seconds.

Now an edit uses a spatial index (`surface/edit/near.rs`). It keeps box trees of the vertices, the edges and the faces for the time of one edit. A tree is built when the edit first needs it, and it grows when the edit adds or changes an element. Other parts:

- The planes to solve come from the faces grouped by their plane and from a tree of the ends of the new pieces.
- The pieces on one straight run share the walk along the run.
- The walks along a cycle in one plane are kept and read again.
- Erase uses the same index for its merges.

An edit now costs about `O(E log E + k log E)`: a build of the index once, then a short query for each new edge.

The index only finds candidates. Every test is the same as before, on the same numbers, in the same order. So the result is the same to the bit. The old scans stay in the code (`surface/edit/reference.rs` lets a test choose them). The test `surface::tests_index` compares both ways on random edits: grid and free points, welds within the tolerance, near misses, crossings, holes, gaps closed last, curves, erase, Push/Pull, turned planes and survey coordinates (650 km from the origin). It passed 12,000 cases in a release build.

Measurement (`crates/kadakas-core/tests/limits_measure.rs`, `limits_measure.py`; release build, Apple M1 Pro, one run each). The numbers are CPU time of one `edit_surface` command through the editor. The machine was shared; the load average is given.

| Scenario | Before | After |
|---|---|---|
| 1,000 edges, empty model | 0.04 s | 0.011 s |
| 5,000 edges, empty model | 1.4 s | 0.08 s |
| 10,000 edges, empty model (grid / terrain / lines) | 5.8 / 15.3 / 12.2 s | 0.19 / 0.44 / 0.06 s |
| 20,000 / 50,000 edges, empty model, in edits of 10,000 | 23 / 140 s | 0.45 / 1.6 s |
| 1,000 edges, 80k-edge model (same plane / other plane / terrain / lines) | 9.5 / 10.6 / 15.3 / 23.2 s | 0.24 / 0.20 / 0.28 / 0.17 s |
| 2,000 / 5,000 / 10,000 edges, 80k-edge model | 23 / 59 / 120 s | 0.27 / 0.37 / 0.57 s |
| 1 edge, 80k-edge model (command / `apply` only) | 0.174 / 0.066 s | 0.167 / 0.055 s |
| No change, 80k-edge model (command / `apply` only) | 0.149 / 0.045 s | 0.140 / 0.034 s |
| Terrain of 75k edges, drawn in 8 edits | 773 s | 8.7 s |
| 6,250 boxes, one edit each | 759 s | 212 s |
| Erase every 7th edge of the 80k model (11,486 edges) | 158 s | 1.3 s (wall, test build) |
| Load average during the runs | 5 to 17 | 5 to 13 |

The results are the same: every scenario ends with the same counts of vertices, edges, faces and loop vertices.

What is left:

- Each command on an 80k-edge model still costs about 0.14 s. Most of it is the check of each command (`SurfaceModel::problem`, about 0.1 s), two copies of the model and the topology index (`Topo`). The topology index is now built from sorted lists.
- Each edit builds the index again (`O(E log E)`). So many small edits in a row still add up: 6,250 boxes take 212 s, about 30 ms for each box at the end. To remove this, the index has to stay from one edit to the next. That is a larger change.
- A long straight run shared by many pieces in many planes still makes the work grow faster than linear in the length of the run.

## Groups and components (MOD-07)

Code: `kadakas_core::component` (rules, parts, walks, the edit context), `command/component_ops.rs` (the commands), `surface/part.rs` (take a part out of a surface model, draw a model into another one), `kadakas_ui_core::infer3d` (scenes of instances). Public behavior only, as in `docs/research/sketchup-public-features.md`.

**Group.** An object `Shape::Group`. Its members name it as `parent`: surface models, groups and component instances. The group has no transform; each surface model keeps one transform that goes to the drawing, so moving a group moves its members. Geometry in a group is a surface model of its own, so it does not weld to the geometry outside.

**Component.** A definition and instances. The definition is a symbol resource (`GraphicResource::Symbol`) with the new list `parts` (surface models, groups, instances of other components). An instance is the shape `component`: the symbol and a `transform` of 12 numbers (three columns of the matrix, then the move). Nobody copies the definition: every user reads it when it needs it.

| Command (`op`) | What it does |
|---|---|
| `make_group` `{model?, faces?, edges?, objects?, name?}` | The chosen faces and edges of `model` and the chosen `objects` go into a new group. The new surface model keeps the ids and the placement. A curve goes whole or not at all. Returns the group, then the new model. |
| `make_component` `{name, model?, faces?, edges?, objects?, replace?}` | The same choice becomes the parts of a new definition. With `replace` (default) one instance takes its place. Returns the definition, then the instance. |
| `explode` `{ids}` | A group or an instance goes. Its surface models merge into the raw model of the level; faces form again by the rules of MOD-06 and keep their side, attributes and, where the edge is found whole, their curve. Groups and instances inside move up one level. |
| `make_unique` `{ids}` | A copy of the definition for the instances (`Tile#1`). Refused with a message if nobody else uses the definition. |
| `edit_component` `{id}` and `close_component` `{group, resolve?}` | Edit in place. See below. |

Each command is one undo step. An error changes nothing.

**Edit in place.** `edit_component` turns the parts of the definition into objects in an edit group at the instance (placed by the instance transform) and hides the instance. Every command then works on those objects: draw, make a group, make a component, explode, nest. `close_component` writes the objects back as the parts and removes the edit group; all instances show the new definition at once. While the edit is open, the other instances still show the old definition (a live update is a later step of the UI). A definition is open in one place at a time, and an outer edit cannot close while an inner one is open. The edit group keeps the version of the definition (`definition_version`, a hash of its parts). If another user changed the definition meanwhile, or the edit group has no saved version (it came from an older build), `close_component` fails with a message that starts with `conflict:` and changes nothing; the person answers with `resolve`: `keep_as_new` keeps the edit as a new component (`Name#1`, language-neutral, the instance uses it, the other version stays), `overwrite` replaces the definition with the edit on purpose, `discard` throws the edit away (this works with or without a conflict). A component that the edit would leave empty is refused for every answer except `discard`, so no instance becomes a placeholder. The modeler has an Edit menu entry for each answer. Known limit: two closes on two collaborating replicas at the same moment are still last-writer-wins (each replica sees the old version when it checks); the check protects against a change that arrived before the close.

**Raw model and edit context.** The raw geometry of a level (the root of the drawing, a group, an edit group) lives in its surface model objects. The *raw model* is the first plain one of them in z-order (no attributes, the class of the level): a model with a look of its own is never the raw model, so drawn or exploded geometry does not take its colour. Drawing in a context goes there, and an exploded group merges into it. `EditContext` (headless, in the core) keeps the path of ids that a person has entered, enters a group (no change of the document) or a component (`edit_component`), leaves it (`close_component`), and answers what the UI needs: `container`, `contains`, `pick`, `dimmed` (the flag `dim_rest` says whether the rest is drawn dimmed), `raw_model`, `to_local` and `to_world`. `apply_world` takes an edit with points in the drawing, converts them into the local coordinates of the raw model through its transform, and, if the level has no model yet, makes one in the same undo step.

**Limits.** 32 groups deep, 16 components deep, 20,000 surface models in the expansion of one definition, 10,000 parts per definition. A command or a walk over a limit fails with a message (`ComponentError`) and changes nothing; a component that would contain itself is refused. `add_resource` and `modify_resource` of a symbol with parts check the expansion of the definition and of every definition that uses it (a nested definition that grows can fail an outer one; one pass over the graph with counted models, so 5,000 definitions on one base cost one pass), and the 2D `group`, `make_group`, `edit_component` and `explode` keep 32 groups. A file with a definition over a limit loads, and the load notes say which; its instances are drawn as a marked cross. The same note is added when a version is restored or other users' document is applied (`apply_remote`); a local edit of a base that an over-limit definition uses then says "which is over a limit" and names it. The measurement is `perf_limits` in `crates/kadakas-core/tests/components_files.rs` (debug build: making 32 nested groups 5 ms, moving them 2 ms, walking a chain of 16 definitions 2 microseconds, walking 20,000 models 4 ms).

**Inference.** `kadakas_ui_core::infer3d::scene_of_instances(doc, instances, first_model)` makes one `Scene` of any number of instances. A definition is turned into a `DefinitionScene` once (in the coordinates of the definition, nested components placed) and kept in a `DefinitionCache`; each instance adds a placed copy of the points, edges and faces. The surface models of the definition are never copied. The model number of a tag is `first_model` plus the ordinal of the model in the definition; `InstanceScenes::instance_of(tag)` turns it back into the instance. The cache keeps the content version of a definition with its scene (`component::version`: revisions of the surface models, placements, nested definitions), so an undo, a change by a peer and a change of a nested component are seen without any call; `forget` only frees memory.

**Collaboration and files.** Groups and instances are objects and definitions are resources, so the commands need no new rule. The surface models are changed with `SurfaceModel::apply`, so `rev` and the log of MOD-06 stay true. In the file a definition has `parts` in its symbol, an instance is `{"kind": "component", "symbol": ..., "transform": [...]}`.

**Broken instances.** An instance whose definition is missing, is not a component, contains itself or is over a limit is not dropped and not hidden: the plan, the SVG, the print and the 3D scene draw a cross in the warning colour at its origin, `component::findings` and `document.summary.broken_components` give the reason, and the load notes list it. A component instance may only name a definition with 3D parts; a symbol instance may not name a definition that has only parts (a mixed symbol with 2D items and parts serves both).

**Modeler.** The Edit menu has Make Group, Make Component, Explode, Make Unique, Edit Group or Component and Close Group or Component. They work on the selected objects (face and edge selection comes with MOD-05) and show the message of the core when a command is refused. A new component is named `Component 1`, `Component 2`, ... (a name box comes later).

**Not done.** Glue To, Cut Opening, Reload and Save As, Always Face Camera, Live and Dynamic Components, Reset Scale and Skew, Change Axes, lock, a live update of the other instances while an edit is open, and the plan drawing of components (MOD-13: the plan shows the box around an instance, the same on the screen, in the SVG and in the print; the box is the box of the definition placed by the instance transform, kept per definition and content version, and read once per definition in a `plan_frame`).

## Clean room

The layout, the tool names, the logic of the tools and the default keys follow public documentation of programs of this kind (`docs/research/sketchup-public-features.md`). No program of that kind was downloaded or run. No icon, image or text was copied. SketchUp is a trademark of Trimble Inc. The About window says that Kadakas Modeler is not affiliated with it.

## Not done yet

- A window of the web build. The program is for the desktop. The 3D view and the tray need a native window and a file dialog.
- The tools Move, Rotate, Scale, Tape Measure and Paint Bucket (MOD-05), and the rest of MOD-08: Polygon, Follow Me, Offset, Intersect Faces With Context, soft and smooth edges, and the hole through a slab for Push/Pull.
- Not tried in a real window: the drawing, the marks and the hatch on a GPU window were checked in headless frames and in one GPU test of the back color, not by hand.
- An editor for the keys of the modeler, and a review window for crash reports. A report stays in the queue until `kadakas-app` starts.
- Autosave and the agent server of the app.
