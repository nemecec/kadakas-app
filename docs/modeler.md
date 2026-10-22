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
| Tray | **Entity Info**, **Tags**, **Materials** and **Scenes**. A click on a title bar collapses the panel. View > Tray hides the tray. |

Select, the drawing tools of MOD-08, Move, Rotate, Scale, the Paint Bucket, Tape Measure and the navigation tools run.

**Measurements box.** It shows the size of the selection (length, width and height in the units of the person), the field of view while the Zoom tool is active, and the live value of a drawing tool that runs (length, width and height, radius, bulge, distance). You type into it without a click: digits and the marks of values start the text, letters follow once the text has started (`24s`, `500r`, `3'6"`), Backspace erases, Enter applies and Esc clears. See "Modeling" below.

**Entity Info.** It shows the type, the id, the layer, the class, the size and the lowest corner of the selected object. For several objects it shows the count and the size of the box around them. With no selection it shows the active layer and class. A surface model also shows its numbers of vertices, edges and faces and whether the faces close a volume. A selected face shows its area, normal, edges and holes. After a merge (Erase, or a flush Push/Pull), the face that stays has the lower id and its own attributes and material. It also has the attributes that it did not have from the merged faces. The status line names what was taken or dropped. A selected edge shows its length, ends, curve and the number of faces on it. Several faces and edges show their counts. **Materials** is the list of the materials of the model (see "Paint Bucket and Materials" below). **Scenes** is a placeholder; scenes come with MOD-10.

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

Move, Rotate and Scale are in the section after this one.

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

## Move, Rotate and Scale (MOD-05, first half)

Move (`M`), Rotate (`Q`) and Scale (`S`) work on the selection: whole objects (surface models, groups, component instances, and the landscape objects), or faces and edges of surface models. The code that decides is `kadakas_ui_core::modeling::xform` (headless). Each finished action is one undo step, and a typed value after the action redoes it: the app takes back that step and makes the new one. Esc ends a tool that is running; a second Esc goes back to Select.

| Tool | Clicks | Typed in the box |
|---|---|---|
| Move (M) | a base point (the inference snaps; the arrow keys and Shift lock a direction), then the end point | a length along the current direction, three lengths `x;y;z`, a relative point `<x,y,z>`, an absolute point `[x,y,z]`; after a copy `5x` (five copies at the same step) and `/5` (five copies that divide the distance) |
| Rotate (Q) | the center, the start direction, then the angle; the protractor has a tick every 15 degrees and the angle snaps to a tick within 3 degrees | an angle (`45`, `-30`, `8:12`); after a copy `5x` (each copy turned one step further) and `/5` (the angle divided) |
| Scale (S) | a grip on the box around the selection, then the end | a factor `2`, three factors `1;1;2` (a value may be empty and stays 1), or a length `2m` (the box gets that length along the grip) |

Ctrl (Option on macOS) toggles **copy** in Move and Rotate (Ctrl pressed alone and let go without another key or button; Ctrl+Z and the like do not toggle; a plus shows at the pointer). In Scale the held key scales **about the center** (as Shift is held); Shift toggles **uniform** scaling. With nothing selected, a click picks the entity under the pointer first (a face or an edge of a surface model, else an object), as Select does, and starts the tool there.

- **Move** shows the selection moved while the pointer moves. A selected face moves all its vertices, a selected edge its two ends. The faces that share a moved vertex are refitted (see below).
- **Rotate.** The plane of the protractor is the plane of the face under the pointer, else the horizontal plane, and its normal is turned to the viewer (also on the back of a face); the arrow keys lock it before the center is clicked (Up blue, Left green, Right red, Down free) and Shift keeps the plane the pointer showed. **Direction:** a typed angle goes the way the pointer was dragged (a drag clockwise as the viewer sees it, then `30` turns 30 degrees clockwise) and a minus reverses it (`-30`); with no drag a positive angle is counterclockwise as the viewer sees it. (The research doc does not verify a rule for the sign; this is the familiar behaviour of such programs.) A value that is not an angle, typed in the angle step, gives a message and the protractor stays. Whole objects turn about any axis through their transform; a plan object (a plant, a hardscape, a site model, a symbol) turns only about a vertical axis (the command says so and changes nothing otherwise).
- **Scale grips.** 8 corners (uniform), 12 edges (two axes, each its own factor) and 6 faces (one axis). Grips on an axis along which the box has no size are not offered (a flat face has no grips for its thickness). Where grips lie behind each other the corner wins over the edge and the edge over the face, then the grip nearest to the eye. The factor is found from the pointer ray: the closest point on the line from the anchor through the grip (uniform, or one axis), or the point where the ray meets the plane of the grip (two axes), or the plane facing the camera (a corner with Shift). The anchor is the opposite side, or the center with Ctrl. A factor is never smaller than 0.001 in size (the sign flips the object). **The box:** for one surface model or one component instance (the box of its definition placed by the instance transform; a turned one) the grips are on the box in its own axes, so a non-uniform scale keeps the angles of the model; for several objects, for faces and edges, and for other objects the box is aligned to the world axes (a non-uniform world scale of a turned object skews it; Entity Info then says "skewed"). A typed length is measured along the axes of the grip, also with Shift.
- **Copies.** A copy of an object is a new object with its group members; a copy of faces and edges is new geometry in the same model, welded where it lands on the model. The first copy is placed by the map, the next by the map applied twice, and so on. After a copy the copies are the selection, so the next Move moves the copy (for geometry: the faces and edges of the last copy).
- **Entity Info** shows the position, the rotation (about x, y, z, in degrees) and the scale of the selected object (a surface model, a component instance, a plant). For faces and edges it shows what it showed (read only).

### The core edits

`edit_surface` has two new actions (`crates/kadakas-core/src/surface/edit/transform.rs`), and there is one new object command.

| Command | What it does |
|---|---|
| `edit_surface` `transform_vertices {vertices?, faces?, edges?, matrix}` | Moves the vertices, all vertices of the faces and the ends of the edges by the affine map `matrix` (12 numbers: the three columns of the 3x3 part, then the translation; local coordinates of the model). One edit, one undo step. Move is the translation case; rotate and scale are affine. |
| `edit_surface` `copy_geometry {faces?, edges?, matrix, count}` | `count` copies of the faces and edges (with the edges of the faces and whole curves), the first placed by `matrix`, the next by the matrix applied twice, and so on. New ids; a copy welds to the geometry it lands on, by the rules of `absorb` (MOD-07). At most `MAX_EDIT_EDGES` edges over all copies. |
| `transform_objects3d {ids, matrix, copy?, count?}` | Move, turn, scale or copy objects in 3D. The map is in world coordinates (the layer elevation is part of the world). Surface models and component instances take any affine map through their transform; plan objects take a move (the height is not changed, with a note), a turn about a vertical axis and a scale in the plan, by the rules of `transform_objects`. A group moves as one unit. With `copy`, `count` copies are made (an array along a vector, or around an axis). |

**What `transform_vertices` does**, in this order:

1. **Snap.** When the moved geometry lands within the length tolerance of geometry that did not move (a moved vertex on a vertex or on the inside of an edge, a moved edge over a vertex), and all these landings agree on one offset within the plane tolerance, all moved vertices are shifted by that offset, so they land exactly. A moved vertex on the inside of an edge that did not move is then put onto the edge. So an error below the tolerance never tilts a face out of its plane and never folds it.
2. **Weld.** A moved vertex that lands within the length tolerance of another vertex is welded into it: the vertex that was there stays (of two that moved, the lower id). Edges that fall together become one (the lower id stays), loops and faces that lose their area go (with notes when they had attributes). This is the weld of Push/Pull. A vertex that lands on the inside of an edge, and an edge that is moved over a vertex, are split there (a welded T-junction), so the model has no vertex on an edge. When an edge is slid along another, the parts that then join the same two vertices become one edge (the lower id stays and takes the attributes of the other; a value that differs is dropped with a note). A vertex that would land on an edge of its own face is refused with that reason (the face would touch itself).
3. **Planes.** A face whose vertices all moved is mapped exactly: an affine map keeps planes (the new normal is the old one times the inverse transpose of the matrix, and a mirror turns the loops around), so such a face is never folded. Any other face that has a moved vertex keeps its plane if all its vertices still lie on it, else it is refitted (Newell normal of the outer loop, mean offset) and is flat if every vertex lies within the plane tolerance of the fit.
4. **Fold.** A face that is not flat any more is cut into triangles, so every face stays planar (`surface/edit/transform/fold.rs`). **The cut depends on the geometry only**, never on the ids or on the vertex where a loop starts: the loops are first written from their lowest point (by x, then y, then z), and every tie is broken by coordinates. A **quad** is cut along the diagonal that bends the least (the smaller angle between the normals of the two triangles); equal bends: the shorter diagonal; still equal: the diagonal whose ends are lower by coordinates. **Any other face** (also with holes; a hole that touches the outer loop at a vertex is joined there) is cut into ears and then the diagonals are flipped toward the constrained Delaunay triangulation in the best-fit plane (a diagonal flips when the far corner of the other triangle lies inside the circle of the first, or when a triangle has no area), which keeps the triangles well shaped; the loop sides stay. The new edges between the triangles have the attributes `soft: true` and `smooth: true`, so they are not drawn as lines and the surface shades round. They also have the fold mark `fold: {"face": id, "corners": [c, d]}`: the id of the face, and the third corners of the two pieces at the edge. An edge that was there already is used as it is; then the fold has no marks and never unfolds. **Ids:** the pieces are ordered by area, largest first (equal areas: by their sorted vertex ids); **the first piece keeps the id of the face, which is the lowest id of all pieces; the others get new ids in that order**; new edges get ids in the order of the pieces. Each piece keeps the attributes and the unknown fields of the face (so its material).
5. **Unfold.** The pieces of a fold that are flat again (in one plane within the plane tolerance, looking the same way) join into the face they came from. The fold edges go. Only exact pieces join: the two faces of each fold edge must still be the two triangles that its mark names. If you drew on a piece, split it or added an edge to it, the fold stays and its edges stay soft. The lowest id stays (that of the largest piece, so the face gets its id back). The look of the larger piece wins (merge rule C) and each dropped value gives a note. So a corner moved up and back gives the model before, with its ids.
6. **No other merge.** A move never merges faces because they became coplanar or adjacent. Faces that were apart stay apart, the edges between them stay, and no vertex heals away. Only Erase merges faces. Example: on a floor drawn as a grid of cells, lift a vertex and move it back. The four cells at the vertex fold and unfold, and the floor keeps all its lines (paving and grading need them). A user soft edge is no fold edge, so it stays too. A landing still joins geometry: vertices weld, a vertex splits an edge, and edges that fall together become one edge. The faces at the landing stay separate faces.

**Refused**, with the model unchanged and a reason that names the face: a loop of a face that would cross or touch itself, a vertex that would land on an edge of its own face, a face that would collapse to zero area without a weld, a map that flattens the model (the ratio of its smallest to its largest singular value is below the relative tolerance 1e-9: a squash that makes any face thinner than its own tolerance; a uniform scale of any size, such as 1e-5, is applied), faces that would overlap in one plane (an edge of a face inside a coplanar face that looks the same way, or one face over another: the coplanar overlaps of `check`, which Push/Pull refuses too), a result outside the range of coordinates, and a result with a model problem that the model did not have before (the error names it; a hole inside another hole of its face is the problem `HoleInHole`). **Accepted with a warning:** geometry that is moved through faces in other planes. An edge that passes through a face in another plane is the warning `NotIntersected` of `check`, as for drawing; Intersect Faces makes the edges there. Curves keep their identity: a curve is a chain of edge ids, and a move keeps the ids; a curve whose vertices move apart stays one curve if it is still a chain, and a weld that cuts it cuts it into chains.

**Copies.** A copy that would move no vertex by the length tolerance lies on the original and is refused. An edit that changes nothing (the identity, or a step that welds back onto itself) leaves the model as it was: no revision, no log entry and no undo step.

**Limits and time.** The fold and the T-junctions together may make at most `MAX_EDIT_EDGES` (10,000) new edges in one edit; the usual `MAX_VERTICES`, `MAX_EDGES`, `MAX_FACES` and `MAX_LOOP_VERTICES` hold for the result. The edit looks only at the part of the model around what moved: box trees of the vertices and edges in that region, a topology index of the faces at the moved vertices and their neighbours, and checks of what changed (the structure checks of the faces and edges around the changes, the distances and the coplanar overlaps of the changed items). Only when that look finds a problem do the full checks of the model before and after decide, so a problem that the model had before stays allowed. The passes that are left over the whole model are simple scans with position tables. A test runs every edit both ways (this part only, and the whole model) and compares the models (`the_local_work_gives_the_same_results_as_the_whole_model`). Measured in a release build on a grid of 200 x 200 faces (40,401 vertices, 80,400 edges; `surface::tests_transform::perf_move`, `--ignored`; Apple M1 Pro, load average about 23):

| Edit (whole `apply`) | Before | After |
|---|---|---|
| Lift 1 vertex (4 faces fold) | 163 ms | 15 ms |
| Move it back (the faces unfold) | 207 ms | 45 ms, 15 ms without the merge |
| Slide 1 vertex in its plane | 157 ms | 13 ms |
| Lift 1,000 vertices (the faces fold) | 173 ms | 40 ms |
| Lift them back (the faces unfold) | 229 ms | 76 ms, 40 ms without the merge |
| Lift a block of whole faces | 170 ms | 37 ms |

The fast check that every command runs takes about 0.09 s more. Since a move no longer merges faces, it does not build the spatial index of Erase for the whole model. A move back now costs about what the move costs.

**Not done.** Auto-fold (the modifier of SketchUp that releases the constraint on Move), moving a vertex alone (the selection holds faces and edges; the core takes vertices), a rubber-band preview of the folds while the pointer moves (the preview shows the outline of the selection with the map applied), grips on a rotated box (the box is axis aligned in the world), Flip Along and mirror as a tool, and the guides and the Tape Measure (round B).

## Tape Measure, guides, tags and Hide (MOD-05, round B part 2)

The logic is in `kadakas_ui_core::modeling::tape`, `kadakas_ui_core::tags` and the guide code of `infer3d`. The window code is in `edit3d.rs`, `tags_panel.rs` and `app.rs`. Tests cover all of it without a window. Most go through the real input path.

### Guides

A guide is an object of the kind `guide` (`kadakas_core::guide`). It makes no geometry. It is a **guide line** or a **guide point**. A line has a point, a direction and an optional length. Without a length it has no ends.

- A guide lives on a layer and in a class, like any object. The visibility of its tag and its layer applies to it.
- The numbers are in the coordinates of the layer, so `z` is measured from the layer.
- A document holds at most 10,000 guides. A command that would add more is refused with a clear message and changes nothing.
- A guide is a thin dashed amber line. A guide point is a small cross. A line without ends is cut to the view.
- A selected guide is blue. A hidden guide that is shown is faint.
- **View > Guides** shows or hides all guides. A hidden guide is not drawn and not offered by the inference. It is not picked. Select All and a rectangle do not take it.
- **Edit > Delete Guides** removes all guides in one undo step. With more than three guides it asks first.
- The **Eraser** removes a guide on a click or a drag. **Select** picks a guide on a click.
- A window takes guide points and guide lines with ends. A crossing rectangle takes lines without ends too.
- Move, Rotate and Scale treat a selected guide like any object. The inference snaps to guides (`docs/modeler-inference.md`, "Guides").
- The Vectorworks-like app keeps guides and does not draw them in the plan. Exports leave them out and say how many (see "Exports" below).
- In the plan app a guide cannot be picked, and Select All leaves it out. Edit > Delete Guides is there too and asks above three. A 2D move or scale of a selection carries a guide along.
- The agent API leaves guides out of `objects.list` unless the call asks for `kind: "guide"`. `document.summary` counts them in `guide_count` and not in `object_count`.
- The counts of the Tags panel leave guides out. The question before a tag is deleted counts them apart.

### Tape Measure (T)

| Clicks | What it does |
|---|---|
| start, move, end (Ctrl first: measure only) | The distance shows live in the box and in the status line. It stays after the second click. |
| an edge or a guide line, then a point away from it | A parallel guide line through the second click. The live value is the offset. |
| a point, a face or the air, then a direction | A finite guide line from the first point to the second click, and a guide point at the second click. |
| two clicks on the same point | A guide point. |

- The inference works as in the drawing tools. The arrow keys and Shift lock a direction.
- The guide of a parallel line passes through the second click. Its offset is the distance from the line to that point, measured perpendicular to the line in 3D.
- The inference decides the second click. On a face it is on the face. On an edge it is on the edge. In the air it is on the ground plane.
- One pair of clicks is one undo step, also when it makes a line and a point.
- **Typed distance.** With a guide from an edge, the number is the offset. It goes to the side of the pointer. A negative number takes the other side.
- With a guide from a point, the number is the length of a finite guide line. It has a guide point at its end.
- The line runs along the pointer direction. A negative number goes the other way.
- **Ctrl** (Option on macOS) pressed alone toggles between making guides and measuring only. The tool keeps the choice until it is chosen again.
- Esc cancels the measure in progress. A second Esc goes back to Select. Hovering an edge shows its length.
- Each guide is one undo step.
- **Resize the model.** Measure, then type a new length. Ctrl is not needed: a pair of clicks that made guides is a measure too. A question asks "Resize the model?".
- A measure lasts until the document changes in another way (an edit, an undo, a new tool). Esc ends it too.
- Before the question, the window finds what will happen. It lists how many objects scale and how many plan objects keep their height.
- If an object is on a locked layer or on a grey tag, nothing is asked. The status line names the locked layers or counts the grey objects, because the command would refuse the whole step.
- On Yes, all objects of the open level are scaled about the **first measured point**. Guides scale too. The factor is the typed length over the measured length.
- The first point stays where it is. A scale about the origin would move the whole model away from what the person looks at. Which point other modelers use was not checked against a manual. Move the model afterwards if needed.
- It is one undo step through `transform_objects3d`. Heights of plan objects come from their layers and stay. The status line says so.
- A factor above 1000 or below 1/1000 is refused. On No nothing changes.

### Tags (the Tags panel)

Tags are classes. The panel is in the tray under Entity Info.

- Each tag has an **eye** (shown or hidden) and a **color swatch**. It has a **mark** for the active tag, a name and a count of objects.
- The swatch shows the pen color of the class, else its fill, else an empty frame. New geometry goes to the active tag.
- **Untagged** is the default class. It cannot be renamed or deleted.
- **New** makes the tag `Tag 1`, `Tag 2` and so on. **Rename** (or a double click on the name) edits the name inline. A name must not be empty or the same as another tag, ignoring case.
- **Delete** asks first when anything uses the tag. Everything moves to Untagged in the same undo step. Nothing moves without the answer. A tag that nothing uses goes at once.
- The core command is `delete_class {id, move_to}`. It moves every reference (`kadakas_core::classes`).
- The references are objects, parts and components of definitions, insert classes of symbols, viewport overrides and annotations, saved views, and material slots.
- A material slot of the tag is not moved to Untagged, since that would paint every Untagged object. Each moved object gets a slot of its own, so it looks the same.
- The question says how many of each there are. One undo step brings all of it back.
- **Show all** shows every hidden tag and layer in one undo step.
- Below the tags are the layers with an eye and a mark for the active layer. They use the commands that exist (`modify_layer`, `set_active`).
- **Entity Info** has a Tag list for the selected objects. A group gives the tag to its members.
- The tag belongs to the object. With faces or edges selected the list is off and a hint says so. Faces keep the tag of their model.
- The panel makes its rows once for each revision of the document, not for each frame. Its commands run as one batch: one undo step, and a failure applies nothing.
- A hidden tag hides its objects as a hidden layer does. They leave the 3D view, picking, the inference and exports.
- The scene builders already leave out objects whose class is not visible. Nothing repeats that rule.
- Not done: tag folders, the Tag tool, Color by Tag and Purge.

### Hide and Unhide

- **Edit > Hide** hides the selected objects or the selected faces and edges of surface models. A group hides its members.
- Hide is also in the context menu of the 3D view. It is one undo step.
- **Edit > Unhide** brings back the selected hidden items. They are selectable only while View > Hidden Geometry (faces and edges) or View > Hidden Objects (objects) is on. **Edit > Unhide All** brings back everything in the document.
- An object is hidden by the key `kadakas.hidden` in its `data` (`kadakas_core::visibility`). The command `set_hidden {ids, hidden}` writes it, a group passes it to its members.
- Keys that start with `kadakas.` are reserved. The agent API refuses `set_data` for them with a clear error. Reading them is free.
- A face or an edge is hidden by the attribute `hidden: true`. `edit_surface` sets it with the action `set_hidden {faces, edges, hidden}`.
- Both are plain data. They are saved and undone. They sync in collaboration like other data.
- The action is logged as an intent that finds faces and edges by position.
- A hidden **edge** does not merge faces, unlike a soft edge. The faces stay as they are.
- A hidden **face** stays in the model and keeps its edges.
- The renderers, the picking and the inference leave hidden items out.
- With **View > Hidden Geometry** or **Hidden Objects** on, hidden items are drawn ghosted. The inference and the picks offer them.
- A ghost is a dotted gray line, and a few lines across a hidden face. The 3D renderer never draws a hidden item itself.
- A hidden object that is not a surface model, such as a plant, is not ghosted. Hidden Objects does not show it.
- The plan app has no Hide. It shows "N hidden objects" in the status line. A click unhides them, and Edit > Unhide All does the same. Sheets and print follow hidden objects as they always followed hidden layers and tags.
- Ghosts cost time only when a revision changes. At most 20,000 lines are drawn in a frame, solid above 2,000.

### Exports

An export never drops construction geometry silently. `export.file` runs on a copy of the document. The copy has no guides, no hidden objects and no hidden faces. This holds for every format (DXF, IFC, glTF, GLB, OBJ, SVG, PDF, PNG).

The answer has `left_out` with the numbers: `guides`, `hidden_objects`, `hidden_faces` and `hidden_edges`. It has `notes` too, and the same lines are in `warnings`. A line reads "2 guides left out: construction geometry". The DXF and IFC reports also count a guide as skipped, with the reason.

Only what really left is counted. Hidden faces that could not be taken out of the copy stay in the export. They are counted apart, with a warning. A hidden edge that no face uses goes. One that borders a shown face stays in the geometry and is not drawn as a line. The note says so.

Objects on a hidden layer or tag are not counted. The exports leave them out by the rules of layers and classes, as before. The code is `kadakas_core::leftout`.

The copy costs time and memory in proportion to the size of the document. It is made only when something is left out. A document with nothing hidden and no guides is exported as it is.

### Not done

- A tape mode of its own for guide points. A double click on one spot makes one.
- Angled guides from the Protractor.
- Tag folders, the Tag tool, Color by Tag, Purge and the Outliner.
- A ghost for hidden objects that are not surface models.
- Not tried by hand in a window: the eye, the swatch, the dashes, the ghosts and the question windows.
- Only headless frames checked these.

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

## Paint Bucket and Materials (MOD-05, round B1)

**Paint Bucket (`B`).** The tool puts a material of the model (3D-06: color, texture, mapping, roughness, metallic, opacity) on faces and objects. Choose the material in the Materials panel first (the bucket shows its color at the pointer), then click.

| Click | What it paints |
|---|---|
| Click on a face | The side of the face that faces the viewer (the front or the back), only that face |
| Ctrl, click | The faces that are connected to it through shared edges and have the same material on that side (the sides follow the direction of the shared edge, so a box paints all its outer faces from one click) |
| Shift, click | All faces of the document with the same material on that side, in every surface model that can be edited (models on a locked or grey layer or in a grey class are left out and the status line counts the faces) |
| Alt (Cmd on a Mac), click | Nothing is painted: the material under the pointer goes into the bucket. An unpainted side loads "Default", and the next click then clears the paint |
| Click with objects selected | The selected objects as a whole, when the click is on one of them (or in a group of them) |
| Click on an object that is no face | A group or a component instance as a whole |

The modifiers follow the public habit of programs of this kind. Only Alt (Cmd) for the sample is on an official help page; Ctrl for the connected fill and Shift for the same-material fill are general knowledge and not checked against a manual (`docs/research/sketchup-public-features.md`). The color of the bucket is drawn at the pointer. Esc leaves the tool and keeps the material in the bucket. Each click is one undo step and the status line says what was painted.

A click goes to the nearest thing under the pointer: a tree in front of a wall is the click on the tree (refused), not on the wall behind it; Select does the same. **Plants, hardscapes, site models and the other objects of the Vectorworks-like app keep the material of their style.** A click on one of them does nothing and the status line says why ("... Change the style in the Kadakas app."). In a selection of several objects the paintable ones are painted and the status line counts the ones that were left alone.

**Where the paint is stored.**

- A face holds the material of its front in the attribute `material` and of its back in `back_material`, as the text form of the id of a material resource (a face value that is the exact name of a material works too, for files made by hand). The core edit is `edit_surface` `set_face_material {faces, side: front | back | both, material | null}`: one undo step, the ids are the ids of the faces, nothing random, and the edit is a replayable intent for collaboration (`paint_at`: the faces by a point inside each). The command refuses a material that is no material of the document. An edit that changes nothing is no step. The attributes pass through the fold of Move, merge rule C and the split of a face like every attribute of a face. **Reverse Faces swaps the two**, so the paint stays on the side it was on. So does Push/Pull when it turns a face over (the new bottom of a free face, the cap of a face pushed the other way), and the walls that Push/Pull makes get the paint of the face, front and back. The Paint Bucket stores the id of the material, never its name (a name can be given twice or changed); a name in a file made by hand still shows its material. A face that names a material that is gone (another user deleted it) draws the default look and counts as no use.
- An object as a whole has a slot of the document (`material.assign` with `object`, no part). A material on a group goes to the members that the Paint Bucket paints (surface models, groups, component instances) and that have no material of their own; the deepest group wins. Plants and hardscapes in the group keep the material of their style. The order is: the face, the part of the object, the object (and its group), the style, the class. **Explode** of a painted group or instance keeps the paint: a surface model gets the material on the sides of its faces that have none, and other members that can be painted get it as their own material.

**How it is drawn.** The scene builder (`kadakas_core::mesh`) groups the triangles of a surface model by the materials of the front and the back of their face. A face with one material on both sides is one set of triangles. Otherwise the front and the back are two sets, each drawn from its side only (`Mesh::tri_side`), and a side without a material has the default front color or the back color. The look of the renderers (`kadakas_render::look`) takes the material of a triangle from its face first (`Mesh::tri_paint`), then from the slots. The GPU view, the PBR and the basic CPU renderers, `render.png` and `render.svg` all follow, with the texture mapping of 3D-06 (planar, box, cylindrical). The vertex color of a painted face is the color of its material, so even a view without textures shows it. The glTF and OBJ files have one material for each face: they carry the front material and leave out the back-only triangles.

**Materials panel.** It lists the materials of the model with a small thumbnail (the color, or one repeat of the generated or stored texture over a checker where the material is clear). The first line is "Default (no material)", an empty bucket. A click puts a material in the bucket; a double click or **Edit** opens the editor under the list. Buttons: **New** (a gray material, opened in the editor), **Add built-in** (Grass, Gravel, Concrete pavers, Bark mulch, Water, Brick, Timber, the ones the model lacks; the same as `material.add_defaults`), **Purge unused**, **Duplicate** and **Delete**. The editor has the name, the color (R, G, B), opacity, roughness, metallic, the generated pattern or none, the mapping (planar, box, cylindrical), the size of one repeat in mm, the rotation and the offset. A field is applied when the person lets go of the slider or leaves the box, and each is one undo step; a value out of range is refused with the reason. **A used material asks first**: Delete shows "{name} is in use ({count}). If you delete it, those faces and objects return to the default look." with **Delete anyway** and **Keep**; the answer yes clears the faces and deletes the material in one undo step. The command itself refuses to delete a material that paints faces (`ResourceInUse`), so nothing is dropped silently. The uses are counted in one pass over the document (`present::material_use_map`, once per revision for the list): slots of objects, styles and classes, the textures of site models and of texture beds, the faces of surface models and the faces of the parts of component definitions (the confirmed delete clears those too, in the same step). Purge keeps every material that is in use. The logic is `kadakas_ui_core::materials` (no window); the panel is `tray.rs`.

**Entity Info.** A selected face shows the material of its front and of its back, each in a box that changes it (one undo step). An object shows its material as a whole, if it has one.

**Not done.** Texture positioning on a face (pins), importing an image into a material from the panel (the API has `material.import_texture`), the "Default" material of the model with a name, a preview of the faces a fill will paint, materials on the faces inside the definition of a component (edit the component in place), and Both sides in the tool (the core has it, Entity Info sets the sides one by one).

## Landscape objects in the modeler (MOD-05, round B1)

A document made in the Vectorworks-like app holds plants, existing trees, hardscapes, landscape areas, site models, symbols and the like. The modeler **shows** them: it draws the same scene as the app (`Session::scene`: the scene builder with the site model extension), at the elevation of their layer. Plants have a trunk and a canopy, hardscapes are slabs, site models are the terrain mesh, symbols draw their parts. Nothing here is a copy: it is the code of `kadakas-core` and `kadakas-site`.

| Kind | Click | Window and crossing | Entity Info says | Move | Rotate | Scale |
|---|---|---|---|---|---|---|
| Plant, existing tree | yes | yes | species, style, height, spread (tree: trunk diameter) | yes | yes (about the pivot, the plant's own turn changes) | the spread; the height stays, the status line says so |
| Hardscape | yes | yes | area, style, surface material | yes (the height note) | refused by the core ("cannot be turned") | refused |
| Landscape area | yes | yes | name, area, style | yes | refused | refused |
| Site model | yes | yes | number of survey points and lines, extent (width, depth, height), texture repeat | yes | refused | refused |
| Symbol, component instance, group | yes | yes | definition or members | yes | yes | yes |

**Select.** A click picks the object under the pointer when no face or edge of a surface model is there. A window (drag to the right) takes the objects whose drawn geometry lies fully inside, a crossing (drag to the left) the ones it touches, also a rectangle inside a large face (`select::rect_select_objects`). A rectangle that holds faces or edges of surface models selects those; objects that are no surface models are taken when it holds no part. Shift adds. Faces and edges of a model are never selected together with whole objects.

**Move, Rotate, Scale** are the tools of round A (`transform_objects3d`). What the core allows for each kind is in the table; a refusal shows the reason of the core in the status line and changes nothing.

**What needs the Kadakas app.** The modeler does not edit the data of these objects (the plant style, the hardscape pattern, the site model points). Entity Info says so and has the button **Open in the Kadakas app**; so does File > **Open in the Kadakas App**. The model must be saved first: with unsaved changes the same three questions as before a New or an Open come (Save and open, Don't save, Cancel); a model with no file gets Save As. Then the main app is started with the file.

*How it starts.* There is no request in the launcher for "open this file in flavor X" (the open requests of the launcher go to a running app of a run). So the modeler starts the program `kadakas-app` that is next to it in the version folder, with the file after `--` (`kadakas_update::launch::sibling_app_command`; on macOS the main executable of `Kadakas Engine.app`). **If a main app runs already, no second one starts:** every run that has an alive stamp also writes `runs/<id>.program` with its program name, `handshake::running_program` finds a fresh `kadakas-app` run, and the file goes to it as the open request that the launcher uses for a double click (`handshake::send_open`). In all cases the status line says that the file is now open in two programs and that the last save wins; the modeler then watches the file once a second: a model with no unsaved changes is loaded again when the other program saved, with unsaved changes the status line warns and a save asks before it replaces the file (the consent check of `files::save`). The new app is not run by the launcher: it has no run record, so no crash mark and no restart for an update, and `KADAKAS_VIA_LAUNCHER` is set so that it does not hand over to the launcher (which would start the flavor of the person's choice again). If the app is not next to the modeler, the status line says so.

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

**Limits.** 32 groups deep, 16 components deep, 20,000 surface models in the expansion of one definition, 10,000 parts per definition. A command or a walk over a limit fails with a message (`ComponentError`) and changes nothing; a component that would contain itself is refused. `add_resource` and `modify_resource` of a symbol with parts check the expansion of the definition and of every definition that uses it (a nested definition that grows can fail an outer one; one pass over the graph with counted models, so 5,000 definitions on one base cost one pass), and the 2D `group`, `make_group`, `edit_component` and `explode` keep 32 groups. A file with a definition over a limit loads, and the load notes say which; its instances are drawn as a marked cross. The same holds for the total of models: the instances beyond it are drawn as a marked cross in the 3D scene. The same note is added when a version is restored or other users' document is applied (`apply_remote`); a local edit of a base that an over-limit definition uses then says "which is over a limit" and names it. The measurement is `perf_limits` in `crates/kadakas-core/tests/components_files.rs` (debug build: making 32 nested groups 5 ms, moving them 2 ms, walking a chain of 16 definitions 2 microseconds, walking 20,000 models 4 ms).

**Guards on cost.** Five more limits keep a walk or a scene from taking all the time or memory. Each refuses with a plain message and the numbers. The model stays as it was. The measurements and the reasons are in `docs/modeler-limits.md`.

- Visited parts, `MAX_VISITED` = 1,000,000. One expansion visits at most this many parts, groups and instances, with a nested instance counted each time. A chain of definitions with no model and 8 instances per level has 8 to the power of the levels paths. The old walk took 19 s at 10 levels. Now `component::ExpansionCache` finds the models, the visited parts and the depth of each definition once. The work is the size of the graph and not the number of paths. This covers `leaves`, `definition_depth`, `check_expansion`, `version`, the plan marks, the 3D scene, the file load and the notes of a restore. The error names the definition. A test compares the memo with the plain walk on random graphs.
- Total models, `MAX_TOTAL_MODELS` = 250,000. All instances together hold at most this many models, and each instance counts its whole expansion. Instances on hidden layers count too, because they still use memory. The mesh and the inference scene keep a copy per instance, about 4.7 KB per model, so the limit is about 1.2 GB. The editor checks it after every command that adds or duplicates instances or changes a definition: add, copy, array, paste, make component, close edit, add and modify resource. The message is "total models would be N, the limit is 250,000" and it names the component with the most models. A command that does not raise the total is always allowed. The editor keeps the total from step to step, so adding many instances one by one stays cheap. `scene_of_instances` refuses a list of instances over the limit.
- Content that arrives. A file, a restored version or a remote document can be over the limit. Nothing is cut. The load notes and `component::findings` list the instances beyond the limit, in the order of the document. The 3D scene draws the cross in the warning colour for them. The plan, the SVG and the print draw the box of the definition as before.
- Size of a definition, `MAX_DEFINITION_EDGES` = 100,000 and `MAX_DEFINITION_BYTES` = 16 MiB. The edges are counted over all surface models of the parts, and the bytes are the saved text of the parts. `add_resource`, `modify_resource` and `close_component` refuse a definition over either value. A file with such a definition loads and gets a note.
- Explode, `MAX_EXPLODE_FACES` = 20,000 and `MAX_EXPLODE_PARTS` = 2,000. An Explode that would weld more faces, or more parts, into the model of the level is refused before any work. The part limit exists because the cost follows the number of parts. The message suggests exploding in smaller groups. The weld uses `surface::Absorber`, which keeps one model and its index while it takes in the parts. A test compares it with the plain `absorb_reference` step by step on random models, and the results are equal to the bit. Explode of 10,000 squares fell from 160 s to 22 s, and 3,333 cubes from 62 s to 7 s. The cost still grows faster than the number of parts.

**Inference.** `kadakas_ui_core::infer3d::scene_of_instances(doc, instances, first_model)` makes one `Scene` of any number of instances. A definition is turned into a `DefinitionScene` once (in the coordinates of the definition, nested components placed) and kept in a `DefinitionCache`; each instance adds a placed copy of the points, edges and faces. The surface models of the definition are never copied. The model number of a tag is `first_model` plus the ordinal of the model in the definition; `InstanceScenes::instance_of(tag)` turns it back into the instance. The cache keeps the content version of a definition with its scene (`component::version`: revisions of the surface models, placements, nested definitions), so an undo, a change by a peer and a change of a nested component are seen without any call; `forget` only frees memory.

**Collaboration and files.** Groups and instances are objects and definitions are resources, so the commands need no new rule. The surface models are changed with `SurfaceModel::apply`, so `rev` and the log of MOD-06 stay true. In the file a definition has `parts` in its symbol, an instance is `{"kind": "component", "symbol": ..., "transform": [...]}`.

**Broken instances.** An instance whose definition is missing, is not a component, contains itself or is over a limit is not dropped and not hidden: the plan, the SVG, the print and the 3D scene draw a cross in the warning colour at its origin, `component::findings` and `document.summary.broken_components` give the reason, and the load notes list it. A component instance may only name a definition with 3D parts; a symbol instance may not name a definition that has only parts (a mixed symbol with 2D items and parts serves both).

**Modeler.** The Edit menu has Make Group, Make Component, Explode, Make Unique, Edit Group or Component and Close Group or Component. They work on the selected objects (a selection of faces and edges moves, turns and scales with the tools above) and show the message of the core when a command is refused. A new component is named `Component 1`, `Component 2`, ... (a name box comes later).

**Not done.** Glue To, Cut Opening, Reload and Save As, Always Face Camera, Live and Dynamic Components, Reset Scale and Skew, Change Axes, lock, a live update of the other instances while an edit is open, and the plan drawing of components (MOD-13: the plan shows the box around an instance, the same on the screen, in the SVG and in the print; the box is the box of the definition placed by the instance transform, kept per definition and content version, and read once per definition in a `plan_frame`).

## Clean room

The layout, the tool names, the logic of the tools and the default keys follow public documentation of programs of this kind (`docs/research/sketchup-public-features.md`). No program of that kind was downloaded or run. No icon, image or text was copied. SketchUp is a trademark of Trimble Inc. The About window says that Kadakas Modeler is not affiliated with it.

## Not done yet

- A window of the web build. The program is for the desktop. The 3D view and the tray need a native window and a file dialog.
- The rest of MOD-08: Polygon, Follow Me, Offset, Intersect Faces With Context, drawing tools for soft and smooth edges (the attributes are set by Move now), and the hole through a slab for Push/Pull.
- Not tried in a real window: the drawing, the marks and the hatch on a GPU window were checked in headless frames and in one GPU test of the back color, not by hand. The same holds for the Paint Bucket (the WGSL shader that draws one side of a triangle is checked by reading and by the CPU renderers, not on a GPU), the thumbnails and the editor of the Materials panel, and "Open in the Kadakas app" (the start of the program is tested with a script in a temporary folder, not with a real app).
- An editor for the keys of the modeler, and a review window for crash reports. A report stays in the queue until `kadakas-app` starts.
- Autosave and the agent server of the app.
